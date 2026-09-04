//! 方法路由（RFC 038 M0 §D1）。
//!
//! 接收 [`JsonRpcMessage`]，按 `method` 字段路由到对应 handler，
//! 返回响应消息（Request）或 `None`（Notification）。
//!
//! ## M0 实现的方法
//!
//! - [`INITIALIZE_METHOD`]（`initialize`，Request）：返回 `capabilities` 声明
//!   （M0 所有 provider 暂为 `false`，符合 RFC 038 M0 验证标准）
//! - [`INITIALIZED_METHOD`]（`initialized`，Notification）：无响应
//! - [`SHUTDOWN_METHOD`]（`shutdown`，Request）：返回 `null`，标记 shutdown 状态
//! - [`EXIT_METHOD`]（`exit`，Notification）：无响应，触发进程退出
//!
//! ## Shutdown 状态机
//!
//! ```text
//! Running ──shutdown──▶ ShuttingDown ──exit──▶ Exited
//!    │                       │
//!    │                       └── 其他方法 → invalid_request 错误
//!    └── 未知方法 → method_not_found 错误
//! ```
//!
//! 与 [LSP 3.17 规范 `shutdown`](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#shutdown)
//! 要求一致：shutdown 后服务器应拒绝除 `exit` 之外的所有请求。

use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::json_rpc::{JsonRpcMessage, MessageKind, RequestId, RpcError};
use crate::semantic::Position;
use crate::syntax::{encode_semantic_tokens, TextDocumentContentChangeEvent, SEMANTIC_TOKEN_TYPES};
use crate::workspace::WorkspaceManager;

/// `initialize` 方法名。
pub const INITIALIZE_METHOD: &str = "initialize";

/// `initialized` 方法名（通知）。
pub const INITIALIZED_METHOD: &str = "initialized";

/// `shutdown` 方法名。
pub const SHUTDOWN_METHOD: &str = "shutdown";

/// `exit` 方法名（通知）。
pub const EXIT_METHOD: &str = "exit";

/// `textDocument/definition` 方法名。
pub const DEFINITION_METHOD: &str = "textDocument/definition";

/// `textDocument/hover` 方法名。
pub const HOVER_METHOD: &str = "textDocument/hover";

/// `textDocument/references` 方法名。
pub const REFERENCES_METHOD: &str = "textDocument/references";

/// `textDocument/documentSymbol` 方法名。
pub const DOCUMENT_SYMBOL_METHOD: &str = "textDocument/documentSymbol";

/// `textDocument/didOpen` 通知（文本同步）。
pub const DID_OPEN_METHOD: &str = "textDocument/didOpen";

/// `textDocument/didChange` 通知（文本同步）。
pub const DID_CHANGE_METHOD: &str = "textDocument/didChange";

/// `textDocument/didClose` 通知（文本同步）。
pub const DID_CLOSE_METHOD: &str = "textDocument/didClose";

/// `textDocument/foldingRange` 方法名。
pub const FOLDING_RANGE_METHOD: &str = "textDocument/foldingRange";

/// `textDocument/semanticTokens/full` 方法名。
pub const SEMANTIC_TOKENS_METHOD: &str = "textDocument/semanticTokens/full";

/// `textDocument/diagnostic` 方法名。
pub const DIAGNOSTIC_METHOD: &str = "textDocument/diagnostic";

/// `workspace/symbol` 方法名（M3 跨包符号查询）。
pub const WORKSPACE_SYMBOL_METHOD: &str = "workspace/symbol";

/// arc-server 服务端信息名称（用于 `initialize` 响应 `serverInfo.name`）。
pub const SERVER_NAME: &str = "arc-server";

/// arc-server 协议版本号（与 [Cargo workspace `version`](../../Cargo.toml) 一致）。
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// LSP `textDocumentSync` 模式。
///
/// - `0` None
/// - `1` Full
/// - `2` Incremental（M0+ 默认声明，M2 起真正实现）
pub const TEXT_DOCUMENT_SYNC_INCREMENTAL: i32 = 2;

/// 测试专用方法名：handler 内触发 panic（验证 [`MethodDispatcher::dispatch`] panic 兜底）。
#[cfg(test)]
const TEST_PANIC_METHOD: &str = "$test/panic";

/// 测试专用方法名：持有 workspace 锁期间触发 panic（验证锁中毒恢复 + 兜底）。
#[cfg(test)]
const TEST_PANIC_HOLDING_LOCK_METHOD: &str = "$test/panic_holding_lock";

/// LSP server 生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    /// 正常运行中——接收所有方法。
    Running,
    /// 已收到 `shutdown` 请求——仅接受 `exit` 通知。
    ///
    /// 与 LSP 规范一致：客户端 `shutdown` 后必须发送 `exit` 通知才能关闭进程；
    /// 期间其他请求应被拒绝（返回 `invalid_request` 错误）。
    ShuttingDown,
    /// 已收到 `exit` 通知——进程应退出。
    Exited,
}

/// 方法分发器。
///
/// 持有 server 状态（shutdown 标记）+ workspace 状态（M1 起承载 `.arcgr`
/// 语义索引），接收 [`JsonRpcMessage`] 并按方法路由。
///
/// ## 线程安全
///
/// `MethodDispatcher` 内部使用 [`Mutex`] 保护 `state` 与 `workspace` 字段——
/// 允许从多线程并发调用 [`dispatch`](Self::dispatch)。
/// M0 单线程主循环不触发并发；M1+ 索引层多线程查询时复用同一 dispatcher。
pub struct MethodDispatcher {
    state: Mutex<ServerState>,
    /// workspace 管理器（M1 起：每 workspace 持有 `.arcgr` 语义索引）。
    workspace: Mutex<WorkspaceManager>,
}

impl MethodDispatcher {
    /// 创建新的分发器（初始状态 `Running`）。
    pub fn new() -> Self {
        Self {
            state: Mutex::new(ServerState::Running),
            workspace: Mutex::new(WorkspaceManager::new()),
        }
    }

    /// 当前 server 状态。
    pub fn state(&self) -> ServerState {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 是否已 exit（进程应退出）。
    pub fn is_exited(&self) -> bool {
        self.state() == ServerState::Exited
    }

    /// 是否处于 shutdown 中（已收到 shutdown，等待 exit）。
    pub fn is_shutting_down(&self) -> bool {
        self.state() == ServerState::ShuttingDown
    }

    /// 注册 workspace 并加载其 `.arcgr` 语义索引（M1）。
    ///
    /// LSP 服务启动时，若目标 workspace 已产出 `.arcgr`，通过本方法注入，
    /// 使 `definition`/`hover`/`references`/`documentSymbol` 四个 provider 可用。
    /// 返回 workspace 索引；加载失败返回 `Err`（不保留半初始化 workspace）。
    pub fn load_workspace_arcgr(&self, root: PathBuf, arcgr_path: &Path) -> Result<usize, String> {
        let mut mgr = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let idx = mgr.add_workspace(root);
        mgr.load_arcgr(idx, arcgr_path).map(|_| idx)
    }

    /// 向首个 workspace 追加一个依赖包 `.arcgr`（M3 跨包查询）。
    ///
    /// 依赖包导出符号参与 `workspace/symbol` 聚合，实现跨包符号定位。
    pub fn load_dependency_package(
        &self,
        package_id: &str,
        arcgr_path: &Path,
    ) -> Result<(), String> {
        let mut mgr = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let ws = mgr
            .workspaces_mut()
            .first_mut()
            .ok_or_else(|| "no workspace loaded; load main .arcgr first".to_string())?;
        ws.load_dependency(package_id, arcgr_path)
    }

    /// 从 `arc.toml` 驱动加载一个项目（阶段 3）：主包 + 依赖包自动加载。
    ///
    /// workspace 根取 `arc.toml` 所在目录；沿依赖图按约定路径自动加载
    /// 各包 `.arcgr`，取代手动 `load_workspace_arcgr` + `load_dependency_package`。
    pub fn load_project(&self, arc_toml_path: &Path) -> Result<(), String> {
        let root = arc_toml_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let mut mgr = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let idx = mgr.add_workspace(root);
        mgr.load_project(idx, arc_toml_path).map(|_| ())
    }

    /// 分发消息——按 `method` 字段路由到 handler。
    ///
    /// 返回值：
    /// - `Some(msg)`：请求已处理，`msg` 是要发回客户端的响应（成功或错误）
    /// - `None`：通知消息（无响应）或无需响应
    ///
    /// ## panic 兜底
    ///
    /// handler 内部 panic 不会杀死进程：[`catch_unwind`] 捕获后对 Request 回
    /// `internal_error`（`-32603`）响应、对 Notification 静默丢弃并记录日志，
    /// 服务器继续服务后续消息。配套前提：所有锁采用中毒恢复语义
    /// （[`PoisonError::into_inner`]），panic 不会经 mutex 中毒连锁放大。
    pub fn dispatch(&self, message: &JsonRpcMessage) -> Option<JsonRpcMessage> {
        let result = catch_unwind(AssertUnwindSafe(|| self.dispatch_inner(message)));
        match result {
            Ok(response) => response,
            Err(payload) => {
                let detail = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic payload".to_string());
                log::error!("handler panicked while dispatching: {detail}");
                message.id.as_ref().map(|id| {
                    JsonRpcMessage::response_error(
                        id.clone(),
                        RpcError::internal_error("internal server error during message handling"),
                    )
                })
            }
        }
    }

    /// 分发主逻辑（无 panic 兜底）——由 [`Self::dispatch`] 的 catch_unwind 包裹调用。
    fn dispatch_inner(&self, message: &JsonRpcMessage) -> Option<JsonRpcMessage> {
        let kind = match message.kind() {
            Ok(k) => k,
            Err(_) => {
                // 消息本身不符合 JSON-RPC 规范——若含 id 则返回 invalid_request 错误
                if let Some(id) = &message.id {
                    return Some(JsonRpcMessage::response_error(
                        id.clone(),
                        RpcError::invalid_request("invalid JSON-RPC message structure"),
                    ));
                }
                // 通知消息结构错误——无法响应，直接丢弃
                return None;
            }
        };

        // 阶段 3 自动重载：分发前惰性检测所有 workspace 的 `arc.toml` 是否变更，
        // 是则重载项目（更新主包/依赖包语义索引），保证后续请求使用最新依赖图。
        if self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_projects_if_changed()
        {
            log::info!("one or more projects reloaded due to arc.toml change");
        }

        match kind {
            MessageKind::Notification { method, params } => {
                self.dispatch_notification(&method, params)
            }
            MessageKind::Request { id, method, params } => {
                self.dispatch_request(id, &method, params)
            }
            // 收到 Response 消息（不应发生在 server 端）——忽略
            MessageKind::ResponseOk { .. } | MessageKind::ResponseError { .. } => None,
        }
    }

    /// 处理通知消息——通知从不产生响应，恒返回 `None`。
    fn dispatch_notification(&self, method: &str, params: Option<Value>) -> Option<JsonRpcMessage> {
        match method {
            INITIALIZED_METHOD => {
                // initialized 是客户端对 initialize 响应的确认——无服务端动作
                log::debug!("received `initialized` notification");
                None
            }
            DID_OPEN_METHOD => {
                self.handle_did_open(params);
                None
            }
            DID_CHANGE_METHOD => {
                self.handle_did_change(params);
                None
            }
            DID_CLOSE_METHOD => {
                self.handle_did_close(params);
                None
            }
            EXIT_METHOD => {
                log::info!("received `exit` notification — shutting down process");
                *self.state.lock().unwrap_or_else(PoisonError::into_inner) = ServerState::Exited;
                None
            }
            #[cfg(test)]
            TEST_PANIC_METHOD => panic!("intentional panic in notification handler"),
            _ => {
                // 未知通知——按 LSP 规范忽略（不能响应无 id 的通知）
                log::debug!("ignoring unknown notification: {method}");
                None
            }
        }
    }

    /// 处理请求消息——返回响应（成功或失败）。
    fn dispatch_request(
        &self,
        id: RequestId,
        method: &str,
        params: Option<Value>,
    ) -> Option<JsonRpcMessage> {
        // shutdown 状态机：除 exit 通知（已在 dispatch_notification 处理）外，
        // 其他请求应被拒绝。但 `exit` 是通知，这里都是 Request，所以
        // shutdown 后所有 Request 都拒绝。
        if self.is_shutting_down() {
            return Some(JsonRpcMessage::response_error(
                id,
                RpcError::invalid_request("server is shutting down; only `exit` is accepted"),
            ));
        }

        match method {
            INITIALIZE_METHOD => Some(self.handle_initialize(id, params)),
            SHUTDOWN_METHOD => {
                // LSP 规范：shutdown 请求将状态转为 ShuttingDown，
                // 之后除 `exit` 通知外所有请求被拒绝（由本函数顶部检查处理）。
                let response = Self::handle_shutdown(id);
                *self.state.lock().unwrap_or_else(PoisonError::into_inner) =
                    ServerState::ShuttingDown;
                log::info!("server entered ShuttingDown state");
                Some(response)
            }
            DEFINITION_METHOD => Some(self.handle_definition(id, params)),
            HOVER_METHOD => Some(self.handle_hover(id, params)),
            REFERENCES_METHOD => Some(self.handle_references(id, params)),
            DOCUMENT_SYMBOL_METHOD => Some(self.handle_document_symbol(id, params)),
            FOLDING_RANGE_METHOD => Some(self.handle_folding_range(id, params)),
            SEMANTIC_TOKENS_METHOD => Some(self.handle_semantic_tokens(id, params)),
            DIAGNOSTIC_METHOD => Some(self.handle_diagnostic(id, params)),
            WORKSPACE_SYMBOL_METHOD => Some(self.handle_workspace_symbol(id, params)),
            #[cfg(test)]
            TEST_PANIC_METHOD => panic!("intentional panic for panic-recovery test"),
            #[cfg(test)]
            TEST_PANIC_HOLDING_LOCK_METHOD => {
                let _guard = self
                    .workspace
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                panic!("intentional panic while holding workspace lock");
            }
            _ => Some(JsonRpcMessage::response_error(
                id,
                RpcError::method_not_found(method),
            )),
        }
    }

    /// `initialize` 请求 handler——返回 capabilities 声明并注册 workspace。
    ///
    /// M1 从 `workspaceFolders` 注册 workspace，供语义 provider 查询 `.arcgr`。
    fn handle_initialize(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        // 注册 workspaceFolders（M1：单进程多 workspace 容器）
        if let Some(params) = &params {
            if let Some(folders) = params.get("workspaceFolders").and_then(|v| v.as_array()) {
                for folder in folders {
                    if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                        let path = uri.strip_prefix("file://").unwrap_or(uri).to_string();
                        self.workspace
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .add_workspace(std::path::PathBuf::from(path));
                    }
                }
            }
        }
        let result = serde_json::json!({
            "capabilities": Self::capabilities(),
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION,
            }
        });
        JsonRpcMessage::response_ok(id, result)
    }

    /// 构建 LSP `capabilities` 对象（M1 语义 provider + M2 语法 provider）。
    ///
    /// 与 [RFC 033 §C2](../../../../docs/rfc/033-lsp.md#c2-lsp-方法矩阵)
    /// 协议能力声明对齐——语义四 provider + 语法三 provider（折叠/语义高亮/诊断）
    /// + 文本同步（openClose + incremental change）。
    fn capabilities() -> Value {
        serde_json::json!({
            // 文本同步：支持 didOpen/didClose 通知 + 增量 didChange（M2 起真正实现）
            "textDocumentSync": {
                "openClose": true,
                "change": TEXT_DOCUMENT_SYNC_INCREMENTAL
            },
            // M1：启用 definition/hover/references/documentSymbol 语义 provider
            "hoverProvider": true,
            "definitionProvider": true,
            "referencesProvider": true,
            "documentSymbolProvider": true,
            // M2：语法三 provider（不依赖 .arcgr 语义索引，直接解析开放文档）
            "foldingRangeProvider": true,
            "semanticTokensProvider": {
                "legend": {
                    "tokenTypes": SEMANTIC_TOKEN_TYPES,
                    "tokenModifiers": []
                },
                "full": true
            },
            "diagnosticProvider": {
                "interFileDependencies": false,
                "workspaceDiagnostics": false
            },
            // M3：跨包符号查询（聚合所有包公共符号）
            "workspaceSymbolProvider": true,
            "signatureHelpProvider": null,
            "completionProvider": null
        })
    }

    /// `shutdown` 请求 handler——返回 `null`，状态转换由调用方负责。
    ///
    /// 与 LSP 规范一致：shutdown 响应 `result` 为 `null`；
    /// 客户端收到响应后必须发送 `exit` 通知才能真正关闭进程。
    ///
    /// 注意：状态转换（Running → ShuttingDown）在 [`dispatch_request`] 中执行，
    /// 而非本 handler——保持 handler 纯函数语义（仅构造响应，无副作用）。
    fn handle_shutdown(id: RequestId) -> JsonRpcMessage {
        JsonRpcMessage::response_ok(id, Value::Null)
    }

    // ============================================================================
    // M1 语义 provider
    // ============================================================================

    /// `textDocument/definition`——返回光标处符号的定义位置。
    ///
    /// M3 扩展：跨包跳转。源文件可能在主包或依赖包；本地无法解析的外部引用
    /// 按名解析到依赖库定义。
    fn handle_definition(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        let (uri, position) = match Self::parse_position_params(&params) {
            Some(v) => v,
            None => {
                return JsonRpcMessage::response_error(
                    id,
                    RpcError::invalid_params(
                        "textDocument/definition requires { textDocument.uri, position }",
                    ),
                )
            }
        };
        // 无匹配/无索引/无符号 → 返回 `null`（LSP 允许空结果，不视为错误）
        let mut ws = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let result = ws
            .definition_at(&uri, position)
            .and_then(|loc| serde_json::to_value(&loc).ok())
            .unwrap_or(Value::Null);
        JsonRpcMessage::response_ok(id, result)
    }

    /// `textDocument/hover`——返回光标处符号的签名与文档摘要。
    fn handle_hover(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        let (uri, position) = match Self::parse_position_params(&params) {
            Some(v) => v,
            None => {
                return JsonRpcMessage::response_error(
                    id,
                    RpcError::invalid_params(
                        "textDocument/hover requires { textDocument.uri, position }",
                    ),
                )
            }
        };
        let mut ws = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(state) = ws.find_workspace_mut_for_uri(&uri) else {
            return JsonRpcMessage::response_ok(id, Value::Null);
        };
        let Some(idx) = state.semantic_mut() else {
            return JsonRpcMessage::response_ok(id, Value::Null);
        };
        let Some(file_id) = idx.file_id_for_uri(&uri) else {
            return JsonRpcMessage::response_ok(id, Value::Null);
        };
        let Some(offset) = idx.position_to_offset(file_id, position) else {
            return JsonRpcMessage::response_ok(id, Value::Null);
        };
        let Some(sym) = idx.symbol_at_offset(file_id, offset) else {
            return JsonRpcMessage::response_ok(id, Value::Null);
        };
        let result = idx
            .hover(sym.symbol_id)
            .and_then(|h| serde_json::to_value(&h).ok())
            .unwrap_or(Value::Null);
        JsonRpcMessage::response_ok(id, result)
    }

    /// `textDocument/references`——返回符号的全部引用位置（含定义）。
    fn handle_references(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        let (uri, position) = match Self::parse_position_params(&params) {
            Some(v) => v,
            None => {
                return JsonRpcMessage::response_error(
                    id,
                    RpcError::invalid_params(
                        "textDocument/references requires { textDocument.uri, position }",
                    ),
                )
            }
        };
        let mut ws = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(state) = ws.find_workspace_mut_for_uri(&uri) else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let Some(idx) = state.semantic_mut() else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let Some(file_id) = idx.file_id_for_uri(&uri) else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let Some(offset) = idx.position_to_offset(file_id, position) else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let Some(sym) = idx.symbol_at_offset(file_id, offset) else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let refs = idx.references(sym.symbol_id);
        let result = serde_json::to_value(&refs).unwrap_or(Value::Array(vec![]));
        JsonRpcMessage::response_ok(id, result)
    }

    /// `textDocument/documentSymbol`——列出文档内全部符号。
    fn handle_document_symbol(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        let Some(uri) = Self::parse_text_document_uri(&params) else {
            return JsonRpcMessage::response_error(
                id,
                RpcError::invalid_params(
                    "textDocument/documentSymbol requires { textDocument.uri }",
                ),
            );
        };
        let mut ws = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(state) = ws.find_workspace_mut_for_uri(&uri) else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let Some(idx) = state.semantic_mut() else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let Some(file_id) = idx.file_id_for_uri(&uri) else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let symbols = idx.document_symbols(file_id);
        let result = serde_json::to_value(&symbols).unwrap_or(Value::Array(vec![]));
        JsonRpcMessage::response_ok(id, result)
    }

    /// `workspace/symbol`——跨包查询所有包（主包 + 依赖包）的公共符号。
    ///
    /// M3：`query` 为大小写不敏感子串匹配；无 query 返回全部公共符号。
    fn handle_workspace_symbol(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        let query = params
            .as_ref()
            .and_then(|p| p.get("query"))
            .and_then(|v| v.as_str());
        let mut ws = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let symbols = ws.workspace_symbols(query);
        let result = serde_json::to_value(&symbols).unwrap_or(Value::Array(vec![]));
        JsonRpcMessage::response_ok(id, result)
    }

    // ============================================================================
    // M2 文本同步通知（didOpen / didChange / didClose）
    // ============================================================================

    /// `textDocument/didOpen`——打开文档并解析语法树。
    fn handle_did_open(&self, params: Option<Value>) {
        let Some(td) = params.as_ref().and_then(|p| p.get("textDocument")) else {
            log::warn!("didOpen missing textDocument");
            return;
        };
        let Some(uri) = td.get("uri").and_then(|v| v.as_str()) else {
            log::warn!("didOpen missing textDocument.uri");
            return;
        };
        let language_id = td
            .get("languageId")
            .and_then(|v| v.as_str())
            .unwrap_or("arc");
        let version = td.get("version").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let text = td.get("text").and_then(|v| v.as_str()).unwrap_or("");
        self.workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .open_document(uri, language_id, version, text);
    }

    /// `textDocument/didChange`——应用增量/全量变更并重解析。
    fn handle_did_change(&self, params: Option<Value>) {
        let Some(td) = params.as_ref().and_then(|p| p.get("textDocument")) else {
            log::warn!("didChange missing textDocument");
            return;
        };
        let Some(uri) = td.get("uri").and_then(|v| v.as_str()) else {
            log::warn!("didChange missing textDocument.uri");
            return;
        };
        let version = td.get("version").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let changes: Vec<TextDocumentContentChangeEvent> = params
            .as_ref()
            .and_then(|p| p.get("contentChanges"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| serde_json::from_value(c.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        self.workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .change_document(uri, version, &changes);
    }

    /// `textDocument/didClose`——关闭文档（丢弃缓冲与语法树）。
    fn handle_did_close(&self, params: Option<Value>) {
        let Some(uri) = params
            .as_ref()
            .and_then(|p| p.get("textDocument"))
            .and_then(|td| td.get("uri"))
            .and_then(|v| v.as_str())
        else {
            log::warn!("didClose missing textDocument.uri");
            return;
        };
        self.workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .close_document(uri);
    }

    // ============================================================================
    // M2 语法 provider（不依赖 .arcgr，直接解析开放文档）
    // ============================================================================

    /// `textDocument/foldingRange`——基于花括号配对的折叠区间。
    fn handle_folding_range(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        let Some(uri) = Self::parse_text_document_uri(&params) else {
            return JsonRpcMessage::response_error(
                id,
                RpcError::invalid_params("textDocument/foldingRange requires { textDocument.uri }"),
            );
        };
        let mut ws = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(doc) = ws.document_mut(&uri) else {
            return JsonRpcMessage::response_ok(id, Value::Array(vec![]));
        };
        let ranges = doc.tree().folding_ranges();
        let result = serde_json::to_value(&ranges).unwrap_or(Value::Array(vec![]));
        JsonRpcMessage::response_ok(id, result)
    }

    /// `textDocument/semanticTokens/full`——编码为 LSP delta data 的高亮 token。
    fn handle_semantic_tokens(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        let Some(uri) = Self::parse_text_document_uri(&params) else {
            return JsonRpcMessage::response_error(
                id,
                RpcError::invalid_params(
                    "textDocument/semanticTokens/full requires { textDocument.uri }",
                ),
            );
        };
        let mut ws = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(doc) = ws.document_mut(&uri) else {
            return JsonRpcMessage::response_ok(id, Value::Null);
        };
        let tokens = doc.tree().semantic_tokens();
        let data = encode_semantic_tokens(&tokens);
        JsonRpcMessage::response_ok(id, serde_json::json!({ "data": data }))
    }

    /// `textDocument/diagnostic`——返回文档首个语法诊断（kind=full）。
    fn handle_diagnostic(&self, id: RequestId, params: Option<Value>) -> JsonRpcMessage {
        let Some(uri) = Self::parse_text_document_uri(&params) else {
            return JsonRpcMessage::response_error(
                id,
                RpcError::invalid_params("textDocument/diagnostic requires { textDocument.uri }"),
            );
        };
        let mut ws = self
            .workspace
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(doc) = ws.document_mut(&uri) else {
            return JsonRpcMessage::response_ok(
                id,
                serde_json::json!({ "kind": "full", "items": [] }),
            );
        };
        let items: Vec<_> = doc.tree().diagnostic().into_iter().collect();
        let result = serde_json::to_value(&items).unwrap_or(Value::Array(vec![]));
        JsonRpcMessage::response_ok(id, serde_json::json!({ "kind": "full", "items": result }))
    }

    // ─── 参数解析辅助 ───

    /// 从 `textDocument.uri` 提取文档 URI。
    fn parse_text_document_uri(params: &Option<Value>) -> Option<String> {
        let uri = params
            .as_ref()?
            .get("textDocument")?
            .get("uri")?
            .as_str()?
            .to_string();
        Some(uri)
    }

    /// 从 `position` 提取 LSP 位置。
    fn parse_position(params: &Option<Value>) -> Option<Position> {
        let pos = params.as_ref()?.get("position")?;
        let line = pos.get("line")?.as_u64()? as u32;
        let character = pos.get("character")?.as_u64()? as u32;
        Some(Position { line, character })
    }

    /// 同时提取 URI 与位置（definition/hover/references 共用）。
    fn parse_position_params(params: &Option<Value>) -> Option<(String, Position)> {
        Some((
            Self::parse_text_document_uri(params)?,
            Self::parse_position(params)?,
        ))
    }
}

impl Default for MethodDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── initialize ───

    #[test]
    fn initialize_returns_capabilities_with_m1_providers_enabled() {
        let dispatcher = MethodDispatcher::new();
        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            INITIALIZE_METHOD,
            Some(serde_json::json!({"processId": null, "capabilities": {}})),
        );
        let resp = dispatcher.dispatch(&req).expect("must respond");
        match resp.kind().unwrap() {
            MessageKind::ResponseOk { id, result } => {
                assert_eq!(id, RequestId::Number(1));
                let caps = &result["capabilities"];
                // 文本同步：openClose + incremental change 对象
                assert_eq!(caps["textDocumentSync"]["openClose"], true);
                assert_eq!(
                    caps["textDocumentSync"]["change"],
                    TEXT_DOCUMENT_SYNC_INCREMENTAL
                );
                // M1：四个语义 provider 已启用
                assert_eq!(caps["hoverProvider"], true);
                assert_eq!(caps["definitionProvider"], true);
                assert_eq!(caps["referencesProvider"], true);
                assert_eq!(caps["documentSymbolProvider"], true);
                // M2：三个语法 provider 已启用
                assert_eq!(caps["foldingRangeProvider"], true);
                assert_eq!(caps["semanticTokensProvider"]["full"], true);
                assert_eq!(
                    caps["semanticTokensProvider"]["legend"]["tokenTypes"][0],
                    "keyword"
                );
                assert_eq!(caps["diagnosticProvider"]["interFileDependencies"], false);
                // M3：workspace/symbol 已启用
                assert_eq!(caps["workspaceSymbolProvider"], true);
                assert!(caps["signatureHelpProvider"].is_null());
                assert!(caps["completionProvider"].is_null());
                // serverInfo
                assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
                assert_eq!(result["serverInfo"]["version"], SERVER_VERSION);
            }
            other => panic!("expected ResponseOk, got {other:?}"),
        }
    }

    #[test]
    fn initialize_response_includes_server_info() {
        let dispatcher = MethodDispatcher::new();
        let req = JsonRpcMessage::request(RequestId::Number(1), INITIALIZE_METHOD, None);
        let resp = dispatcher.dispatch(&req).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseOk { result, .. } => {
                assert_eq!(result["serverInfo"]["name"], "arc-server");
                assert_eq!(result["serverInfo"]["version"], SERVER_VERSION);
            }
            _ => panic!("expected ResponseOk"),
        }
    }

    #[test]
    fn initialize_with_empty_params_succeeds() {
        let dispatcher = MethodDispatcher::new();
        let req = JsonRpcMessage::request(RequestId::Number(1), INITIALIZE_METHOD, None);
        let resp = dispatcher.dispatch(&req).unwrap();
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseOk { .. }
        ));
    }

    // ─── initialized ───

    #[test]
    fn initialized_notification_returns_no_response() {
        let dispatcher = MethodDispatcher::new();
        let notification =
            JsonRpcMessage::notification(INITIALIZED_METHOD, Some(serde_json::json!({})));
        let resp = dispatcher.dispatch(&notification);
        assert!(resp.is_none(), "notification must not produce response");
        assert_eq!(dispatcher.state(), ServerState::Running);
    }

    // ─── shutdown ───

    #[test]
    fn shutdown_request_returns_null_result() {
        let dispatcher = MethodDispatcher::new();
        let req = JsonRpcMessage::request(RequestId::Number(5), SHUTDOWN_METHOD, None);
        let resp = dispatcher.dispatch(&req).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseOk { id, result } => {
                assert_eq!(id, RequestId::Number(5));
                assert!(result.is_null(), "shutdown result must be null");
            }
            _ => panic!("expected ResponseOk with null result"),
        }
    }

    #[test]
    fn shutdown_dispatch_transitions_state_to_shutting_down() {
        // 验证 dispatch 调用路径中 shutdown 请求自然触发状态转换
        // （非手动设置状态——这是 LSP 规范要求）
        let dispatcher = MethodDispatcher::new();
        assert_eq!(dispatcher.state(), ServerState::Running);

        let req = JsonRpcMessage::request(RequestId::Number(1), SHUTDOWN_METHOD, None);
        let _ = dispatcher.dispatch(&req);

        assert_eq!(dispatcher.state(), ServerState::ShuttingDown);
        assert!(dispatcher.is_shutting_down());
        assert!(!dispatcher.is_exited());
    }

    #[test]
    fn requests_after_shutdown_rejected_with_invalid_request() {
        let dispatcher = MethodDispatcher::new();
        // 通过 dispatch shutdown 请求自然触发状态转换
        let shutdown_req = JsonRpcMessage::request(RequestId::Number(1), SHUTDOWN_METHOD, None);
        let _ = dispatcher.dispatch(&shutdown_req);
        assert!(dispatcher.is_shutting_down());

        // shutdown 后任何 Request 都应被拒绝
        let req = JsonRpcMessage::request(RequestId::Number(2), INITIALIZE_METHOD, None);
        let resp = dispatcher.dispatch(&req).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseError { id, error } => {
                assert_eq!(id, RequestId::Number(2));
                assert_eq!(error.code, -32600); // invalid_request
                assert!(error.message.contains("shutting down"));
            }
            _ => panic!("expected ResponseError"),
        }
    }

    // ─── exit ───

    #[test]
    fn exit_notification_marks_state_exited() {
        let dispatcher = MethodDispatcher::new();
        let notification = JsonRpcMessage::notification(EXIT_METHOD, None);
        let resp = dispatcher.dispatch(&notification);
        assert!(
            resp.is_none(),
            "exit notification must not produce response"
        );
        assert!(dispatcher.is_exited());
    }

    // ─── unknown method ───

    #[test]
    fn unknown_request_returns_method_not_found() {
        let dispatcher = MethodDispatcher::new();
        let req = JsonRpcMessage::request(RequestId::Number(7), "textDocument/unknown", None);
        let resp = dispatcher.dispatch(&req).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseError { id, error } => {
                assert_eq!(id, RequestId::Number(7));
                assert_eq!(error.code, -32601); // method_not_found
                assert!(error.message.contains("textDocument/unknown"));
            }
            _ => panic!("expected ResponseError"),
        }
    }

    #[test]
    fn unknown_notification_is_ignored() {
        let dispatcher = MethodDispatcher::new();
        let notification = JsonRpcMessage::notification("$/cancelRequest", None);
        let resp = dispatcher.dispatch(&notification);
        assert!(resp.is_none());
        assert_eq!(dispatcher.state(), ServerState::Running);
    }

    // ─── 完整 initialize → shutdown → exit 流程 ───

    #[test]
    fn full_lifecycle_initialize_shutdown_exit() {
        let dispatcher = MethodDispatcher::new();
        assert_eq!(dispatcher.state(), ServerState::Running);

        // 1. initialize
        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            INITIALIZE_METHOD,
            Some(serde_json::json!({"processId": null})),
        );
        let resp = dispatcher.dispatch(&req).unwrap();
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseOk { .. }
        ));
        assert_eq!(dispatcher.state(), ServerState::Running);

        // 2. initialized 通知
        let notification =
            JsonRpcMessage::notification(INITIALIZED_METHOD, Some(serde_json::json!({})));
        assert!(dispatcher.dispatch(&notification).is_none());
        assert_eq!(dispatcher.state(), ServerState::Running);

        // 3. shutdown 请求（LSP 规范：客户端必须先 shutdown 再 exit）
        //    dispatch 自然将状态转为 ShuttingDown
        let req = JsonRpcMessage::request(RequestId::Number(2), SHUTDOWN_METHOD, None);
        let resp = dispatcher.dispatch(&req).unwrap();
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseOk { .. }
        ));
        assert_eq!(dispatcher.state(), ServerState::ShuttingDown);

        // 4. exit 通知——状态转 Exited
        let notification = JsonRpcMessage::notification(EXIT_METHOD, None);
        assert!(dispatcher.dispatch(&notification).is_none());
        assert!(dispatcher.is_exited());
    }

    // ─── M2 文本同步 + 语法 provider ───

    #[test]
    fn did_open_then_folding_range_returns_folds() {
        let dispatcher = MethodDispatcher::new();
        let uri = "file:///a.as";
        let open = JsonRpcMessage::notification(
            DID_OPEN_METHOD,
            Some(serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "arc",
                    "version": 1,
                    "text": "class A {\n  void F() {\n  }\n}\n"
                }
            })),
        );
        assert!(dispatcher.dispatch(&open).is_none());

        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            FOLDING_RANGE_METHOD,
            Some(serde_json::json!({ "textDocument": { "uri": uri } })),
        );
        let resp = dispatcher.dispatch(&req).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseOk { result, .. } => {
                let arr = result.as_array().expect("folding ranges array");
                // 两个跨行折叠：方法体 + 类体
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0]["startLine"], 1);
                assert_eq!(arr[0]["endLine"], 2);
                assert_eq!(arr[1]["startLine"], 0);
                assert_eq!(arr[1]["endLine"], 3);
            }
            _ => panic!("expected ResponseOk"),
        }
    }

    #[test]
    fn did_open_then_semantic_tokens_returns_encoded_data() {
        let dispatcher = MethodDispatcher::new();
        let uri = "file:///a.as";
        let open = JsonRpcMessage::notification(
            DID_OPEN_METHOD,
            Some(serde_json::json!({
                "textDocument": { "uri": uri, "languageId": "arc", "version": 1, "text": "namespace A;" }
            })),
        );
        assert!(dispatcher.dispatch(&open).is_none());

        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            SEMANTIC_TOKENS_METHOD,
            Some(serde_json::json!({ "textDocument": { "uri": uri } })),
        );
        let resp = dispatcher.dispatch(&req).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseOk { result, .. } => {
                let data = result["data"].as_array().expect("data array");
                // namespace/A/; → 3 个 token × 5 字段 = 15
                assert_eq!(data.len(), 15);
                // 首 token：deltaLine=0, deltaStart=0, length=9(namespace), type=0(keyword)
                assert_eq!(data[0], 0);
                assert_eq!(data[1], 0);
                assert_eq!(data[3], 0);
            }
            _ => panic!("expected ResponseOk"),
        }
    }

    #[test]
    fn did_open_valid_source_has_empty_diagnostic() {
        let dispatcher = MethodDispatcher::new();
        let uri = "file:///a.as";
        let open = JsonRpcMessage::notification(
            DID_OPEN_METHOD,
            Some(serde_json::json!({
                "textDocument": { "uri": uri, "languageId": "arc", "version": 1, "text": "class A { }" }
            })),
        );
        assert!(dispatcher.dispatch(&open).is_none());

        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            DIAGNOSTIC_METHOD,
            Some(serde_json::json!({ "textDocument": { "uri": uri } })),
        );
        let resp = dispatcher.dispatch(&req).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseOk { result, .. } => {
                assert_eq!(result["kind"], "full");
                assert_eq!(result["items"].as_array().map(|a| a.len()), Some(0));
            }
            _ => panic!("expected ResponseOk"),
        }
    }

    #[test]
    fn did_open_with_syntax_error_reports_diagnostic() {
        let dispatcher = MethodDispatcher::new();
        let uri = "file:///a.as";
        let open = JsonRpcMessage::notification(
            DID_OPEN_METHOD,
            Some(serde_json::json!({
                "textDocument": { "uri": uri, "languageId": "arc", "version": 1, "text": "class Foo {" }
            })),
        );
        assert!(dispatcher.dispatch(&open).is_none());

        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            DIAGNOSTIC_METHOD,
            Some(serde_json::json!({ "textDocument": { "uri": uri } })),
        );
        let resp = dispatcher.dispatch(&req).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseOk { result, .. } => {
                assert_eq!(result["kind"], "full");
                let items = result["items"].as_array().unwrap();
                assert_eq!(items.len(), 1);
                assert_eq!(items[0]["severity"], 1);
                assert_eq!(items[0]["source"], "arc");
                assert!(!items[0]["message"].as_str().unwrap().is_empty());
            }
            _ => panic!("expected ResponseOk"),
        }
    }

    #[test]
    fn did_change_incremental_updates_document() {
        let dispatcher = MethodDispatcher::new();
        let uri = "file:///a.as";
        let open = JsonRpcMessage::notification(
            DID_OPEN_METHOD,
            Some(serde_json::json!({
                "textDocument": { "uri": uri, "languageId": "arc", "version": 1, "text": "class A {}\n" }
            })),
        );
        assert!(dispatcher.dispatch(&open).is_none());

        // 在第 0 行第 7 列插入 "B" → "class AB {}"
        let change = JsonRpcMessage::notification(
            DID_CHANGE_METHOD,
            Some(serde_json::json!({
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [
                    {
                        "range": { "start": { "line": 0, "character": 7 }, "end": { "line": 0, "character": 7 } },
                        "text": "B"
                    }
                ]
            })),
        );
        assert!(dispatcher.dispatch(&change).is_none());

        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            FOLDING_RANGE_METHOD,
            Some(serde_json::json!({ "textDocument": { "uri": uri } })),
        );
        let resp = dispatcher.dispatch(&req).unwrap();
        // 修改后无跨行花括号 → 无折叠
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseOk { result, .. } if result.as_array().map(|a| a.is_empty()) == Some(true)
        ));
    }

    #[test]
    fn did_close_removes_document() {
        let dispatcher = MethodDispatcher::new();
        let uri = "file:///a.as";
        let open = JsonRpcMessage::notification(
            DID_OPEN_METHOD,
            Some(serde_json::json!({
                "textDocument": { "uri": uri, "languageId": "arc", "version": 1, "text": "class A { }" }
            })),
        );
        assert!(dispatcher.dispatch(&open).is_none());

        let close = JsonRpcMessage::notification(
            DID_CLOSE_METHOD,
            Some(serde_json::json!({ "textDocument": { "uri": uri } })),
        );
        assert!(dispatcher.dispatch(&close).is_none());

        // 关闭后文档不存在 → 折叠返回空
        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            FOLDING_RANGE_METHOD,
            Some(serde_json::json!({ "textDocument": { "uri": uri } })),
        );
        let resp = dispatcher.dispatch(&req).unwrap();
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseOk { result, .. } if result.as_array().map(|a| a.is_empty()) == Some(true)
        ));
    }

    #[test]
    fn syntax_provider_on_unopened_document_returns_empty() {
        let dispatcher = MethodDispatcher::new();
        let req = JsonRpcMessage::request(
            RequestId::Number(1),
            FOLDING_RANGE_METHOD,
            Some(serde_json::json!({ "textDocument": { "uri": "file:///ghost.as" } })),
        );
        let resp = dispatcher.dispatch(&req).unwrap();
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseOk { result, .. } if result.as_array().map(|a| a.is_empty()) == Some(true)
        ));
    }

    // ─── 消息结构错误处理 ───

    #[test]
    fn invalid_request_structure_returns_invalid_request_error() {
        let dispatcher = MethodDispatcher::new();
        // 构造一个不符合 JSON-RPC 规范的消息——既有 id 又无 method/result/error
        let bad = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: Some(RequestId::Number(1)),
            method: None,
            params: None,
            result: None,
            error: None,
        };
        let resp = dispatcher.dispatch(&bad).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseError { id, error } => {
                assert_eq!(id, RequestId::Number(1));
                assert_eq!(error.code, -32600);
            }
            _ => panic!("expected ResponseError"),
        }
    }

    #[test]
    fn response_message_received_is_ignored() {
        // server 不应收到 Response 消息——若收到则忽略
        let dispatcher = MethodDispatcher::new();
        let resp =
            JsonRpcMessage::response_ok(RequestId::Number(1), serde_json::json!("unexpected"));
        let result = dispatcher.dispatch(&resp);
        assert!(result.is_none());
    }

    // ─── panic 兜底与锁中毒恢复 ───

    #[test]
    fn handler_panic_returns_internal_error_and_server_survives() {
        let dispatcher = MethodDispatcher::new();
        let req = JsonRpcMessage::request(RequestId::Number(42), TEST_PANIC_METHOD, None);
        let resp = dispatcher
            .dispatch(&req)
            .expect("panic in request handler must yield an error response");
        match resp.kind().unwrap() {
            MessageKind::ResponseError { id, error } => {
                assert_eq!(id, RequestId::Number(42));
                assert_eq!(error.code, -32603); // internal_error
                assert!(error.message.contains("internal server error"));
            }
            _ => panic!("expected ResponseError"),
        }

        // panic 后服务器必须继续服务后续消息
        let follow_up = JsonRpcMessage::request(RequestId::Number(43), SHUTDOWN_METHOD, None);
        let resp = dispatcher.dispatch(&follow_up).unwrap();
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseOk { .. }
        ));
    }

    #[test]
    fn panic_holding_lock_does_not_poison_subsequent_requests() {
        let dispatcher = MethodDispatcher::new();
        // panic 发生时持有 workspace 锁 → mutex 中毒
        let req =
            JsonRpcMessage::request(RequestId::Number(1), TEST_PANIC_HOLDING_LOCK_METHOD, None);
        let resp = dispatcher
            .dispatch(&req)
            .expect("panic in request handler must yield an error response");
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseError { error, .. } if error.code == -32603
        ));

        // 中毒锁经恢复语义正常获取——后续 workspace 请求照常服务
        let follow_up = JsonRpcMessage::request(
            RequestId::Number(2),
            WORKSPACE_SYMBOL_METHOD,
            Some(serde_json::json!({ "query": "anything" })),
        );
        let resp = dispatcher.dispatch(&follow_up).unwrap();
        match resp.kind().unwrap() {
            MessageKind::ResponseOk { id, result } => {
                assert_eq!(id, RequestId::Number(2));
                assert_eq!(result.as_array().map(|a| a.len()), Some(0));
            }
            _ => panic!("expected ResponseOk"),
        }
    }

    #[test]
    fn notification_panic_is_swallowed_without_response() {
        let dispatcher = MethodDispatcher::new();
        // 通知无 id——panic 后无法响应，静默丢弃且服务器存活
        let notification = JsonRpcMessage::notification(TEST_PANIC_METHOD, None);
        let resp = dispatcher.dispatch(&notification);
        assert!(resp.is_none(), "panicked notification must not respond");
        // 服务器继续服务
        let follow_up = JsonRpcMessage::request(RequestId::Number(1), SHUTDOWN_METHOD, None);
        let resp = dispatcher.dispatch(&follow_up).unwrap();
        assert!(matches!(
            resp.kind().unwrap(),
            MessageKind::ResponseOk { .. }
        ));
    }
}
