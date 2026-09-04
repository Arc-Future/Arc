//! JSON-RPC 2.0 编解码 + LSP stdio 传输层（RFC 038 M0 §D1）。
//!
//! ## JSON-RPC 2.0 消息结构
//!
//! [JSON-RPC 2.0 规范](https://www.jsonrpc.org/specification) 定义三种消息：
//!
//! - **Request**（请求）：含 `id` + `method` + `params`，期待 Response
//! - **Response**（响应）：含 `id` + `result` 或 `error`，是 Request 的回应
//! - **Notification**（通知）：含 `method` + `params`，无 `id`，不期待 Response
//!
//! ## LSP stdio 传输层
//!
//! LSP 在 stdio 上使用 `Content-Length: <N>\r\n\r\n<json>` 格式封装 JSON-RPC 消息：
//!
//! ```text
//! Content-Length: 50\r\n
//! \r\n
//! {"jsonrpc":"2.0","id":1,"method":"initialize","params":{...}}
//! ```
//!
//! ## 设计要点
//!
//! - `JsonRpcMessage` 用 `Option` 字段统一表达三种消息类型——Request 有 `id`+`method`，
//!   Response 有 `id`+`result`/`error`，Notification 有 `method` 但 `id` 为 None
//! - `RequestId` 支持数字与字符串（LSP 客户端可能用任意类型）——
//!   数字 ID 最常见，字符串 ID 用于某些客户端的「请求追踪」
//! - 编解码使用 `serde_json::Value` 表达 `params`/`result`——M0 不做语义解析，
//!   逐方法 handler 自行反序列化（M1+ 实现 handler 时按需 typed-deserialize）

use std::io::{self, Read, Write};

use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// JSON-RPC 协议版本字符串（固定 `"2.0"`）。
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 请求 ID——LSP 客户端可能用数字或字符串。
///
/// JSON-RPC 2.0 规范允许 `id` 为 Number/String/Null；LSP 实际使用中
/// 数字最常见，字符串用于某些客户端追踪，null 不使用（保留位）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RequestId {
    /// 数字 ID（最常见）。
    Number(i64),
    /// 字符串 ID（某些客户端使用）。
    String(String),
}

impl RequestId {
    /// 从 JSON `Value` 解析 `RequestId`。
    ///
    /// - `Value::Number` → `Number`（仅整数；浮点 ID 在 LSP 不合法）
    /// - `Value::String` → `String`
    /// - 其他 → `Err`
    pub fn from_json_value(value: &Value) -> Result<Self, JsonRpcError> {
        match value {
            Value::Number(n) => {
                let i = n.as_i64().ok_or_else(|| JsonRpcError::InvalidRequestId {
                    reason: format!("id must be integer, got: {n}"),
                })?;
                Ok(Self::Number(i))
            }
            Value::String(s) => Ok(Self::String(s.clone())),
            other => Err(JsonRpcError::InvalidRequestId {
                reason: format!("id must be number or string, got: {other}"),
            }),
        }
    }

    /// 转换为 JSON `Value`。
    pub fn to_json_value(&self) -> Value {
        match self {
            Self::Number(n) => Value::from(*n),
            Self::String(s) => Value::String(s.clone()),
        }
    }
}

impl Serialize for RequestId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_json_value(&value).map_err(de::Error::custom)
    }
}

/// JSON-RPC 错误对象（[规范 §5.1](https://www.jsonrpc.org/specification#error_object)）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// 错误码（整数）。
    ///
    /// 预定义错误码：
    /// - `-32700` Parse error（JSON 解析失败）
    /// - `-32600` Invalid Request（不是合法的 Request 对象）
    /// - `-32601` Method not found（方法不存在或不可用）
    /// - `-32602` Invalid params（参数类型/结构错误）
    /// - `-32603` Internal error（内部错误）
    /// - `-32000` to `-32099` Server error（实现定义的服务器错误）
    pub code: i32,
    /// 错误消息（简短描述）。
    pub message: String,
    /// 附加数据（可选，详细错误信息）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// 创建错误对象。
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// 创建带附加数据的错误对象。
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    /// `-32700` Parse error。
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(-32700, message)
    }

    /// `-32600` Invalid Request。
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(-32600, message)
    }

    /// `-32601` Method not found。
    pub fn method_not_found(method: &str) -> Self {
        Self::new(-32601, format!("method not found: {method}"))
    }

    /// `-32602` Invalid params。
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }

    /// `-32603` Internal error。
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(-32603, message)
    }
}

/// JSON-RPC 消息（统一表达 Request/Response/Notification 三种类型）。
///
/// 字段缺失规则（与 JSON-RPC 2.0 规范对齐）：
/// - Request：`id` + `method` + `params?`
/// - Response 成功：`id` + `result`
/// - Response 失败：`id` + `error`
/// - Notification：`method` + `params?`（无 `id`）
///
/// `Option<Value>` 的「null 保持」反序列化。
///
/// serde 默认把 JSON `null` 反序列化为 `Option::None`，使 `{"result":null}`
/// 读回后丢失 result。本函数将 `null` 映射为 `Some(Value::Null)`，保证成功响应的
/// null 结果（如 LSP `shutdown` 的 `result:null`）可正确往返；字段缺省时
/// 由 `#[serde(default)]` 兜底为 `None`。
fn deserialize_preserve_null<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Some(Value::deserialize(deserializer)?))
}

/// JSON-RPC 消息（统一表达 Request/Response/Notification 三种类型）。
///
/// 字段缺失规则（与 JSON-RPC 2.0 规范对齐）：
/// - Request：`id` + `method` + `params?`
/// - Response 成功：`id` + `result`
/// - Response 失败：`id` + `error`
/// - Notification：`method` + `params?`（无 `id`）
///
/// 序列化时使用 `#[serde(skip_serializing_if)]` 省略 `Option::None` 字段，
/// 反序列化时 `id`/`method`/`params`/`result`/`error` 全部为 `Option`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcMessage {
    /// JSON-RPC 协议版本（固定 `"2.0"`）。
    pub jsonrpc: String,
    /// 请求 ID。Request 与 Response 必须含；Notification 必须不含。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    /// 方法名。Request 与 Notification 必须含；Response 必须不含。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// 参数。Request 与 Notification 可选含；Response 必须不含。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// 响应结果。仅 Response 成功含；Request/Notification 必须不含。
    ///
    /// 用 [`deserialize_preserve_null`] 反序列化——保留 `result:null`
    /// （LSP `shutdown` 返回 `null`），避免读回时丢失导致 `kind()` 误判。
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "deserialize_preserve_null"
    )]
    pub result: Option<Value>,
    /// 响应错误。仅 Response 失败含；Request/Notification 必须不含。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// 消息种类（由字段组合判定）。
#[derive(Debug, Clone, PartialEq)]
pub enum MessageKind {
    /// 请求消息（有 `id` + `method`）。
    Request {
        id: RequestId,
        method: String,
        params: Option<Value>,
    },
    /// 通知消息（无 `id` + 有 `method`）。
    Notification {
        method: String,
        params: Option<Value>,
    },
    /// 成功响应（有 `id` + 有 `result`）。
    ResponseOk { id: RequestId, result: Value },
    /// 失败响应（有 `id` + 有 `error`）。
    ResponseError { id: RequestId, error: RpcError },
}

impl JsonRpcMessage {
    /// 构造请求消息。
    pub fn request(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id: Some(id),
            method: Some(method.into()),
            params,
            result: None,
            error: None,
        }
    }

    /// 构造通知消息（无 `id`，不期待响应）。
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id: None,
            method: Some(method.into()),
            params,
            result: None,
            error: None,
        }
    }

    /// 构造成功响应。
    pub fn response_ok(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id: Some(id),
            method: None,
            params: None,
            result: Some(result),
            error: None,
        }
    }

    /// 构造失败响应。
    pub fn response_error(id: RequestId, error: RpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id: Some(id),
            method: None,
            params: None,
            result: None,
            error: Some(error),
        }
    }

    /// 判定消息种类。
    ///
    /// 返回 `Err(JsonRpcError)` 表示消息不符合 JSON-RPC 2.0 规范
    /// （如 `id` 与 `result` 同时缺失、`method` 与 `result` 同时存在等）。
    pub fn kind(&self) -> Result<MessageKind, JsonRpcError> {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(JsonRpcError::InvalidVersion {
                got: self.jsonrpc.clone(),
            });
        }
        let has_method = self.method.is_some();
        let has_id = self.id.is_some();
        let has_result = self.result.is_some();
        let has_error = self.error.is_some();
        let has_params = self.params.is_some();

        // Response：id 必须存在；result 与 error 互斥；method/params 必须不存在
        if has_id && (has_result || has_error) {
            if has_method || has_params {
                return Err(JsonRpcError::InvalidMessageStructure {
                    reason: "response must not contain method/params".into(),
                });
            }
            if has_result && has_error {
                return Err(JsonRpcError::InvalidMessageStructure {
                    reason: "result and error are mutually exclusive".into(),
                });
            }
            // id 已确认存在
            let id = self.id.clone().expect("checked above");
            if let Some(result) = self.result.clone() {
                return Ok(MessageKind::ResponseOk { id, result });
            }
            let error = self.error.clone().expect("checked above");
            return Ok(MessageKind::ResponseError { id, error });
        }

        // Request/Notification：必须有 method；不可有 result/error
        if has_method {
            if has_result || has_error {
                return Err(JsonRpcError::InvalidMessageStructure {
                    reason: "request/notification must not contain result/error".into(),
                });
            }
            let method = self.method.clone().expect("checked above");
            let params = self.params.clone();
            if let Some(id) = self.id.clone() {
                return Ok(MessageKind::Request { id, method, params });
            }
            return Ok(MessageKind::Notification { method, params });
        }

        Err(JsonRpcError::InvalidMessageStructure {
            reason: "message must be request/notification/response".into(),
        })
    }

    /// 序列化为 JSON 字符串。
    pub fn to_json(&self) -> Result<String, JsonRpcError> {
        serde_json::to_string(self).map_err(|e| JsonRpcError::Serialization {
            reason: e.to_string(),
        })
    }

    /// 从 JSON 字符串反序列化。
    pub fn from_json(s: &str) -> Result<Self, JsonRpcError> {
        serde_json::from_str(s).map_err(|e| JsonRpcError::Deserialization {
            reason: e.to_string(),
        })
    }
}

/// JSON-RPC 编解码与传输层错误。
#[derive(Debug, Error)]
pub enum JsonRpcError {
    /// JSON 反序列化失败。
    #[error("JSON-RPC deserialization failed: {reason}")]
    Deserialization { reason: String },

    /// JSON 序列化失败。
    #[error("JSON-RPC serialization failed: {reason}")]
    Serialization { reason: String },

    /// `jsonrpc` 字段不是 `"2.0"`。
    #[error("invalid jsonrpc version: expected \"2.0\", got {got:?}")]
    InvalidVersion { got: String },

    /// 消息字段组合不符合 JSON-RPC 2.0 规范。
    #[error("invalid JSON-RPC message structure: {reason}")]
    InvalidMessageStructure { reason: String },

    /// `id` 字段类型不合法。
    #[error("invalid request id: {reason}")]
    InvalidRequestId { reason: String },

    /// stdio 读写 IO 错误。
    #[error("stdio io error: {0}")]
    Io(#[from] io::Error),

    /// `Content-Length` header 解析失败。
    #[error("invalid Content-Length header: {0}")]
    InvalidContentLength(String),

    /// 消息体超过 `Content-Length` 声明的字节数。
    #[error("message body length mismatch: declared {declared}, actual {actual}")]
    BodyLengthMismatch { declared: usize, actual: usize },
}

/// LSP stdio 传输层读写工具。
///
/// LSP 在 stdio 上使用 `Content-Length: <N>\r\n\r\n<json>` 格式封装 JSON-RPC 消息。
/// `Content-Length` 是字节数（UTF-8 编码后），不是字符数。
///
/// 与普通 JSON-RPC over TCP 不同，LSP stdio 还有两个特点：
/// 1. Header 是 ASCII 文本（`Content-Length: 50\r\n\r\n`），Body 是 UTF-8 JSON
/// 2. 可能有其他 header（如 `Content-Type: application/json; charset=utf-8`）——
///    M0 只识别 `Content-Length`，其他 header 跳过
pub struct StdioTransport;

impl StdioTransport {
    /// 从 reader 读取一条 LSP 消息（header + body）。
    ///
    /// 阻塞读取直到完整消息到达。EOF 时返回 `Ok(None)`（连接关闭）。
    pub fn read_message<R: Read>(reader: &mut R) -> Result<Option<JsonRpcMessage>, JsonRpcError> {
        // 1. 读 header 区——连续读直到空行（`\r\n\r\n`）
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut current_header = String::new();
        loop {
            let mut byte = [0u8; 1];
            let n = reader.read(&mut byte)?;
            if n == 0 {
                // EOF——若已开始读 header 则报错，否则视为连接关闭
                if headers.is_empty() && current_header.is_empty() {
                    return Ok(None);
                }
                return Err(JsonRpcError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF while reading headers",
                )));
            }
            let ch = byte[0] as char;
            current_header.push(ch);
            // 检测 `\r\n\r\n` 终止符
            if current_header.ends_with("\r\n\r\n") {
                let header_str = current_header.trim_end_matches("\r\n\r\n");
                // 解析每个 header 行（`\r\n` 分隔）
                for line in header_str.split("\r\n") {
                    if line.is_empty() {
                        continue;
                    }
                    if let Some((name, value)) = line.split_once(": ") {
                        headers.push((name.to_ascii_lowercase(), value.to_string()));
                    } else if let Some((name, value)) = line.split_once(':') {
                        // 容错：无空格的 `Name:Value`
                        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
                    }
                }
                break;
            }
        }

        // 2. 提取 Content-Length
        let content_length: usize = headers
            .iter()
            .find(|(name, _)| name == "content-length")
            .map(|(_, value)| value.trim().parse())
            .ok_or_else(|| {
                JsonRpcError::InvalidContentLength("Content-Length header missing".into())
            })?
            .map_err(|e| {
                JsonRpcError::InvalidContentLength(format!("invalid Content-Length value: {e}"))
            })?;

        // 3. 读 body——按 Content-Length 字节数读
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body)?;

        // 4. 反序列化为 JsonRpcMessage
        let body_str = std::str::from_utf8(&body).map_err(|e| JsonRpcError::Deserialization {
            reason: format!("body is not valid UTF-8: {e}"),
        })?;
        let message = JsonRpcMessage::from_json(body_str)?;
        Ok(Some(message))
    }

    /// 向 writer 写入一条 LSP 消息（header + body）。
    ///
    /// 自动添加 `Content-Length: <N>\r\n\r\n` header。
    pub fn write_message<W: Write>(
        writer: &mut W,
        message: &JsonRpcMessage,
    ) -> Result<(), JsonRpcError> {
        let body = message.to_json()?;
        let body_bytes = body.as_bytes();
        let header = format!("Content-Length: {}\r\n\r\n", body_bytes.len());
        writer.write_all(header.as_bytes())?;
        writer.write_all(body_bytes)?;
        writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── RequestId ───

    #[test]
    fn request_id_from_number_json() {
        let id = RequestId::from_json_value(&serde_json::json!(42)).unwrap();
        assert_eq!(id, RequestId::Number(42));
    }

    #[test]
    fn request_id_from_string_json() {
        let id = RequestId::from_json_value(&serde_json::json!("req-abc")).unwrap();
        assert_eq!(id, RequestId::String("req-abc".into()));
    }

    #[test]
    fn request_id_rejects_float() {
        let err = RequestId::from_json_value(&serde_json::json!(42.5)).unwrap_err();
        assert!(matches!(
            err,
            JsonRpcError::InvalidRequestId { .. } if err.to_string().contains("must be integer")
        ));
    }

    #[test]
    fn request_id_rejects_null() {
        let err = RequestId::from_json_value(&serde_json::json!(null)).unwrap_err();
        assert!(matches!(err, JsonRpcError::InvalidRequestId { .. }));
    }

    #[test]
    fn request_id_round_trip_number() {
        let id = RequestId::Number(42);
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, serde_json::json!(42));
        let back: RequestId = serde_json::from_value(json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn request_id_round_trip_string() {
        let id = RequestId::String("req-1".into());
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, serde_json::json!("req-1"));
        let back: RequestId = serde_json::from_value(json).unwrap();
        assert_eq!(back, id);
    }

    // ─── RpcError ───

    #[test]
    fn rpc_error_predefined_codes() {
        assert_eq!(RpcError::parse_error("e").code, -32700);
        assert_eq!(RpcError::invalid_request("e").code, -32600);
        assert_eq!(RpcError::method_not_found("m").code, -32601);
        assert_eq!(RpcError::invalid_params("e").code, -32602);
        assert_eq!(RpcError::internal_error("e").code, -32603);
    }

    #[test]
    fn rpc_error_method_not_found_message_includes_method() {
        let e = RpcError::method_not_found("textDocument/definition");
        assert!(e.message.contains("textDocument/definition"));
    }

    #[test]
    fn rpc_error_with_data_serializes_data_field() {
        let e = RpcError::with_data(-32000, "server error", serde_json::json!({"detail": "x"}));
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["data"]["detail"], serde_json::json!("x"));
    }

    #[test]
    fn rpc_error_without_data_omits_data_field() {
        let e = RpcError::new(-32603, "internal");
        let json = serde_json::to_value(&e).unwrap();
        assert!(json.get("data").is_none());
    }

    // ─── JsonRpcMessage ───

    #[test]
    fn message_kind_request() {
        let m = JsonRpcMessage::request(
            RequestId::Number(1),
            "initialize",
            Some(serde_json::json!({"capabilities": {}})),
        );
        match m.kind().unwrap() {
            MessageKind::Request { id, method, params } => {
                assert_eq!(id, RequestId::Number(1));
                assert_eq!(method, "initialize");
                assert!(params.is_some());
            }
            other => panic!("expected Request, got {other:?}"),
        }
    }

    #[test]
    fn message_kind_notification() {
        let m = JsonRpcMessage::notification("initialized", Some(serde_json::json!({})));
        match m.kind().unwrap() {
            MessageKind::Notification { method, params } => {
                assert_eq!(method, "initialized");
                assert!(params.is_some());
            }
            other => panic!("expected Notification, got {other:?}"),
        }
    }

    #[test]
    fn message_kind_response_ok() {
        let m =
            JsonRpcMessage::response_ok(RequestId::Number(2), serde_json::json!({"result": "ok"}));
        match m.kind().unwrap() {
            MessageKind::ResponseOk { id, result } => {
                assert_eq!(id, RequestId::Number(2));
                assert_eq!(result["result"], serde_json::json!("ok"));
            }
            other => panic!("expected ResponseOk, got {other:?}"),
        }
    }

    #[test]
    fn message_kind_response_error() {
        let err = RpcError::method_not_found("foo/bar");
        let m = JsonRpcMessage::response_error(RequestId::Number(3), err.clone());
        match m.kind().unwrap() {
            MessageKind::ResponseError { id, error } => {
                assert_eq!(id, RequestId::Number(3));
                assert_eq!(error.code, err.code);
                assert_eq!(error.message, err.message);
            }
            other => panic!("expected ResponseError, got {other:?}"),
        }
    }

    #[test]
    fn message_kind_rejects_wrong_version() {
        let m = JsonRpcMessage {
            jsonrpc: "1.0".into(),
            id: Some(RequestId::Number(1)),
            method: Some("foo".into()),
            params: None,
            result: None,
            error: None,
        };
        let err = m.kind().unwrap_err();
        assert!(matches!(err, JsonRpcError::InvalidVersion { .. }));
    }

    #[test]
    fn message_kind_rejects_result_and_error_coexist() {
        let m = JsonRpcMessage {
            jsonrpc: JSONRPC_VERSION.into(),
            id: Some(RequestId::Number(1)),
            method: None,
            params: None,
            result: Some(serde_json::json!("ok")),
            error: Some(RpcError::new(-1, "e")),
        };
        let err = m.kind().unwrap_err();
        assert!(matches!(err, JsonRpcError::InvalidMessageStructure { .. }));
    }

    // ─── JSON round-trip ───

    #[test]
    fn request_round_trip() {
        let original = JsonRpcMessage::request(
            RequestId::Number(10),
            "textDocument/definition",
            Some(serde_json::json!({
                "textDocument": {"uri": "file:///foo.as"},
                "position": {"line": 5, "character": 10}
            })),
        );
        let json = original.to_json().unwrap();
        let parsed = JsonRpcMessage::from_json(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn notification_round_trip() {
        let original = JsonRpcMessage::notification("initialized", Some(serde_json::json!({})));
        let json = original.to_json().unwrap();
        let parsed = JsonRpcMessage::from_json(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn response_ok_round_trip() {
        let original = JsonRpcMessage::response_ok(
            RequestId::String("req-1".into()),
            serde_json::json!({"capabilities": {"hoverProvider": false}}),
        );
        let json = original.to_json().unwrap();
        let parsed = JsonRpcMessage::from_json(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn response_ok_with_null_result_round_trip() {
        // LSP `shutdown` 返回 `result:null`——`Option<Value>` 默认把 JSON null 反序列化为
        // None 会丢失 result，使读回的响应被 `kind()` 判为非法结构。此测试锁定
        // `deserialize_preserve_null` 的「null 保持」修复。
        let original = JsonRpcMessage::response_ok(RequestId::Number(6), Value::Null);
        let json = original.to_json().unwrap();
        assert!(
            json.contains("\"result\":null"),
            "wire must carry result:null: {json}"
        );

        let parsed = JsonRpcMessage::from_json(&json).unwrap();
        assert_eq!(parsed, original);
        assert!(matches!(
            parsed.kind().unwrap(),
            MessageKind::ResponseOk { result, .. } if result == Value::Null
        ));
    }

    #[test]
    fn response_error_round_trip() {
        let original = JsonRpcMessage::response_error(
            RequestId::Number(42),
            RpcError::with_data(
                -32601,
                "method not found",
                serde_json::json!({"method": "foo/bar"}),
            ),
        );
        let json = original.to_json().unwrap();
        let parsed = JsonRpcMessage::from_json(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn request_serialization_omits_absent_fields() {
        let m = JsonRpcMessage::notification("exit", None);
        let json = m.to_json().unwrap();
        // 通知消息不应有 id 字段
        assert!(!json.contains("\"id\""));
        // exit 通知无 params
        assert!(!json.contains("\"params\""));
        // 不应有 result/error
        assert!(!json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    // ─── StdioTransport ───

    #[test]
    fn stdio_write_then_read_round_trip() {
        let original = JsonRpcMessage::request(
            RequestId::Number(1),
            "initialize",
            Some(serde_json::json!({"processId": null})),
        );
        let mut buffer = Vec::<u8>::new();
        StdioTransport::write_message(&mut buffer, &original).unwrap();

        let mut cursor = std::io::Cursor::new(buffer);
        let parsed = StdioTransport::read_message(&mut cursor).unwrap();
        assert_eq!(parsed, Some(original));
    }

    #[test]
    fn stdio_read_returns_none_on_eof() {
        let mut empty = std::io::Cursor::new(Vec::<u8>::new());
        let result = StdioTransport::read_message(&mut empty).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn stdio_multiple_messages_sequential() {
        let msgs = vec![
            JsonRpcMessage::request(RequestId::Number(1), "initialize", None),
            JsonRpcMessage::notification("initialized", None),
            JsonRpcMessage::request(RequestId::Number(2), "shutdown", None),
            JsonRpcMessage::notification("exit", None),
        ];
        let mut buffer = Vec::<u8>::new();
        for m in &msgs {
            StdioTransport::write_message(&mut buffer, m).unwrap();
        }
        let mut cursor = std::io::Cursor::new(buffer);
        let mut parsed = Vec::new();
        while let Some(m) = StdioTransport::read_message(&mut cursor).unwrap() {
            parsed.push(m);
        }
        assert_eq!(parsed.len(), msgs.len());
        for (got, expected) in parsed.iter().zip(msgs.iter()) {
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn stdio_rejects_missing_content_length_header() {
        // 仅 `\r\n\r\n` 终止符，无 Content-Length header
        let data = b"\r\n\r\n";
        let mut cursor = std::io::Cursor::new(&data[..]);
        let err = StdioTransport::read_message(&mut cursor).unwrap_err();
        assert!(matches!(err, JsonRpcError::InvalidContentLength(_)));
    }

    #[test]
    fn stdio_handles_extra_headers_ignoring_unknown() {
        // 模拟客户端发送 Content-Type 等额外 header
        let body = br#"{"jsonrpc":"2.0","method":"exit"}"#;
        let header = format!(
            "Content-Length: {}\r\nContent-Type: application/json; charset=utf-8\r\n\r\n",
            body.len()
        );
        let mut buffer = Vec::new();
        buffer.extend_from_slice(header.as_bytes());
        buffer.extend_from_slice(body);

        let mut cursor = std::io::Cursor::new(buffer);
        let msg = StdioTransport::read_message(&mut cursor)
            .unwrap()
            .expect("message");
        assert_eq!(
            msg.kind().unwrap(),
            MessageKind::Notification {
                method: "exit".into(),
                params: None
            }
        );
    }

    #[test]
    fn stdio_body_length_mismatch_detected() {
        // 声明 100 字节但只发 5 字节
        let data = b"Content-Length: 100\r\n\r\nshort";
        let mut cursor = std::io::Cursor::new(&data[..]);
        let err = StdioTransport::read_message(&mut cursor);
        // EOF 时 read_exact 报 UnexpectedEof
        assert!(err.is_err());
    }
}
