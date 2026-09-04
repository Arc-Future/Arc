// RFC 038：MCP 平级包——MCPHttpClient（JSON-RPC 2.0 over HTTP，连接外部 MCP server）。
// 核心保持 Arc.Agent 自包含（零外部依赖）；MCP 为可选平级包，不反向依赖 Arc.Agent.MCP。
// 工具源 MCPToolSource 把 MCP 工具映射为 [AITool] 等价描述（name/description/schema）。
// HTTP 传输统一走 Arc.Net.HttpClient（复用连接池），不再手写 socket。
// JSON 解析统一走 Arc.Text.Json 类型化反序列化（JsonReader/IJsonDeserializable），
// 与 DeepSeek 路径一致，不再手写 IndexOf 字符串扫描。
namespace Arc.Agent.MCP;

using Arc;
using Arc.Collections;
using Arc.Net;
using Arc.Text;
using Arc.Text.Json;

/// <summary>
/// MCP（Model Context Protocol）HTTP 客户端——JSON-RPC 2.0 over HTTP（Streamable HTTP transport）。
/// 连接远端 MCP server：initialize 握手 → tools/list 枚举 → tools/call 调用。
/// 内部持 HttpClient（连接池），实现 IDisposable 释放（幂等）。
/// </summary>
public class MCPHttpClient : IDisposable {
    private string _endpoint;
    private int _timeoutMs;
    private int _idSeq;
    private bool _initialized;
    private HttpClient _http;
    private bool _disposed;

    public MCPHttpClient(string endpoint) {
        _endpoint = endpoint != null ? endpoint : "";
        _timeoutMs = 30000;
        _idSeq = 0;
        _initialized = false;
        _disposed = false;
        _http = new HttpClient();
        _http.Timeout = _timeoutMs;
    }

    public MCPHttpClient(string endpoint, int timeoutMs) {
        _endpoint = endpoint != null ? endpoint : "";
        _timeoutMs = timeoutMs > 0 ? timeoutMs : 30000;
        _idSeq = 0;
        _initialized = false;
        _disposed = false;
        _http = new HttpClient();
        _http.Timeout = _timeoutMs;
    }

    public string Endpoint { get { return _endpoint; } }
    public bool IsInitialized { get { return _initialized; } }

    /// <summary>释放内部 HttpClient（幂等）。</summary>
    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
        if (_http != null) {
            _http.Dispose();
            _http = null;
        }
    }

    /// <summary>MCP initialize 握手（2025-06-18 协议版本；能力最小面）。
    /// 返回 "" = 成功；非空 = 错误消息。</summary>
    public async Task<string> InitializeAsync(CancellationToken ct) {
        if (ct.IsCancellationRequested) {
            return "cancelled";
        }
        string body = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{"
            + "\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},"
            + "\"clientInfo\":{\"name\":\"arc-ai\",\"version\":\"0.1.0\"}}}";
        string resp = await this.PostAsync(body, ct);
        if (resp == "") {
            return "empty response";
        }
        MCPRpcEnvelope env = new MCPRpcEnvelope();
        JsonSerializer.Deserialize(resp, (IJsonDeserializable)env);
        if (env.HasResult) {
            _initialized = true;
            return "";
        }
        if (env.ErrorMessage != "") {
            return env.ErrorMessage;
        }
        return "initialize failed";
    }

    /// <summary>枚举远端工具（tools/list）。返回每个工具的 name/description/inputSchema；失败返回空列表。</summary>
    internal async Task<List<MCPToolInfo>> ListToolsAsync(CancellationToken ct) {
        List<MCPToolInfo> tools = new List<MCPToolInfo>();
        _idSeq = _idSeq + 1;
        string body = this.BuildBody(_idSeq, "tools/list", "{}");
        string resp = await this.PostAsync(body, ct);
        if (resp == "") {
            return tools;
        }
        MCPRpcEnvelope env = new MCPRpcEnvelope();
        JsonSerializer.Deserialize(resp, (IJsonDeserializable)env);
        return env.Tools;
    }

    /// <summary>调用远端工具（tools/call）。返回工具结果文本；失败返回 ""。</summary>
    public async Task<string> CallToolAsync(string toolName, string argumentsJson, CancellationToken ct) {
        if (toolName == null || toolName == "") {
            return "";
        }
        string args = argumentsJson != null && argumentsJson != "" ? argumentsJson : "{}";
        string paramsJson = "{\"name\":\"" + MCPHttpClient.EscapeJson(toolName) + "\",\"arguments\":" + args + "}";
        _idSeq = _idSeq + 1;
        string body = this.BuildBody(_idSeq, "tools/call", paramsJson);
        string resp = await this.PostAsync(body, ct);
        if (resp == "") {
            return "";
        }
        MCPRpcEnvelope env = new MCPRpcEnvelope();
        JsonSerializer.Deserialize(resp, (IJsonDeserializable)env);
        return env.Text;
    }

    private string BuildBody(int id, string method, string paramsJson) {
        string m = method;
        if (m == null) {
            m = "";
        }
        return "{\"jsonrpc\":\"2.0\",\"id\":\"" + ("" + id) + "\",\"method\":\""
            + m + "\",\"params\":" + paramsJson + "}";
    }

    private async Task<string> PostAsync(string body, CancellationToken ct) {
        if (_endpoint == "") {
            return "";
        }
        if (ct.IsCancellationRequested) {
            return "";
        }
        HttpRequestMessage req = new HttpRequestMessage(HttpMethod.POST, new Uri(_endpoint));
        req.Content = new StringContent(body, "application/json");
        HttpResponseMessage resp = await _http.SendAsync(req);
        if (resp == null) {
            return "";
        }
        string respBody = resp.Body != null ? resp.Body : "";
        resp.Dispose();
        return respBody;
    }

    /// <summary>JSON 字符串转义（\" \\ \n \r \t）。逐字符经 string 索引读取、StringBuilder.Append(char) 追加。</summary>
    internal static string EscapeJson(string s) {
        if (s == null) {
            return "";
        }
        StringBuilder sb = new StringBuilder();
        int i = 0;
        int n = s.Length;
        while (i < n) {
            char ch = s[i];
            if (ch == '"') { sb.Append("\\\""); }
            else if (ch == '\\') { sb.Append("\\\\"); }
            else if (ch == '\n') { sb.Append("\\n"); }
            else if (ch == '\r') { sb.Append("\\r"); }
            else if (ch == '\t') { sb.Append("\\t"); }
            else { sb.Append(ch); }
            i = i + 1;
        }
        return sb.ToString();
    }
}

/// <summary>MCP 工具条目（tools/list 中单个工具）：name/description/inputSchema 原始 JSON。</summary>
internal class MCPToolInfo {
    public string Name;
    public string Description;
    public string InputSchema;

    public MCPToolInfo() {
        Name = "";
        Description = "";
        InputSchema = "";
    }
}

/// <summary>
/// MCP JSON-RPC 响应信封（result/error 二选一）。result 内按字段提取 tools（tools/list）
/// 或 content 首个文本（tools/call）；error 提取 message。未知字段一律 Skip。
/// </summary>
internal class MCPRpcEnvelope : IJsonDeserializable {
    public bool HasResult;
    public string ErrorMessage;
    public List<MCPToolInfo> Tools;
    public string Text;

    public MCPRpcEnvelope() {
        HasResult = false;
        ErrorMessage = "";
        Tools = new List<MCPToolInfo>();
        Text = "";
    }

    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "result") {
                    HasResult = true;
                    this.ReadResult(reader);
                } else if (prop == "error") {
                    ErrorMessage = this.ReadError(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private void ReadResult(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "tools") {
                    this.ReadToolsArray(reader);
                } else if (prop == "content") {
                    Text = this.ReadContentText(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private void ReadToolsArray(JsonReader reader) {
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            MCPToolInfo tool = this.ReadTool(reader);
            if (tool != null) {
                Tools.Add(tool);
            }
            reader.Read();
        }
    }

    private MCPToolInfo ReadTool(JsonReader reader) {
        MCPToolInfo tool = new MCPToolInfo();
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return tool;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "name") {
                    tool.Name = reader.GetString();
                } else if (prop == "description") {
                    tool.Description = reader.GetString();
                } else if (prop == "inputSchema") {
                    MCPJsonValue schema = MCPJsonValue.ReadValue(reader);
                    if (schema.Kind != "null") {
                        tool.InputSchema = schema.ToJsonString();
                    }
                } else {
                    reader.Skip();
                }
            }
        }
        return tool;
    }

    private string ReadContentText(JsonReader reader) {
        reader.Read();
        string text = "";
        while (reader.TokenType != JsonTokenType.EndArray) {
            string itemText = this.ReadContentItem(reader);
            if (text == "" && itemText != "") {
                text = itemText;
            }
            reader.Read();
        }
        return text;
    }

    private string ReadContentItem(JsonReader reader) {
        string text = "";
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return text;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "text") {
                    text = reader.GetString();
                } else {
                    reader.Skip();
                }
            }
        }
        return text;
    }

    private string ReadError(JsonReader reader) {
        string message = "";
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return message;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "message") {
                    message = reader.GetString();
                } else {
                    reader.Skip();
                }
            }
        }
        return message;
    }
}

/// <summary>
/// 泛型 JSON 值节点：读取任意 JSON 子树并原样回写（供 inputSchema 原始 JSON 透传）。
/// 读取走 JsonReader（类型化）；回写走自建序列化，数字保留原文（JsonWriter 仅 int 面）。
/// </summary>
internal class MCPJsonValue {
    public string Kind;
    public string Text;
    public List<string> Keys;
    public List<MCPJsonValue> Values;
    public List<MCPJsonValue> Items;

    public MCPJsonValue() {
        Kind = "null";
        Text = "";
        Keys = new List<string>();
        Values = new List<MCPJsonValue>();
        Items = new List<MCPJsonValue>();
    }

    public static MCPJsonValue ReadValue(JsonReader reader) {
        MCPJsonValue value = new MCPJsonValue();
        if (reader.TokenType == JsonTokenType.String) {
            value.Kind = "string";
            value.Text = reader.GetString();
        } else if (reader.TokenType == JsonTokenType.Number) {
            value.Kind = "number";
            value.Text = reader.GetRawText();
        } else if (reader.TokenType == JsonTokenType.True) {
            value.Kind = "true";
            value.Text = "true";
        } else if (reader.TokenType == JsonTokenType.False) {
            value.Kind = "false";
            value.Text = "false";
        } else if (reader.TokenType == JsonTokenType.Null) {
            value.Kind = "null";
            value.Text = "null";
        } else if (reader.TokenType == JsonTokenType.StartObject) {
            value.Kind = "object";
            value.ReadObject(reader);
        } else if (reader.TokenType == JsonTokenType.StartArray) {
            value.Kind = "array";
            value.ReadArray(reader);
        }
        return value;
    }

    public string ToJsonString() {
        StringBuilder sb = new StringBuilder();
        this.AppendTo(sb);
        return sb.ToString();
    }

    private void ReadObject(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string key = reader.GetString();
                reader.Read();
                MCPJsonValue child = MCPJsonValue.ReadValue(reader);
                Keys.Add(key);
                Values.Add(child);
            }
        }
    }

    private void ReadArray(JsonReader reader) {
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            MCPJsonValue child = MCPJsonValue.ReadValue(reader);
            Items.Add(child);
            reader.Read();
        }
    }

    private void AppendTo(StringBuilder sb) {
        if (Kind == "string") {
            sb.Append("\"");
            sb.Append(MCPHttpClient.EscapeJson(Text));
            sb.Append("\"");
        } else if (Kind == "number" || Kind == "true" || Kind == "false" || Kind == "null") {
            sb.Append(Text);
        } else if (Kind == "object") {
            sb.Append("{");
            int i = 0;
            while (i < Keys.Count) {
                if (i > 0) {
                    sb.Append(",");
                }
                sb.Append("\"");
                sb.Append(MCPHttpClient.EscapeJson(Keys[i]));
                sb.Append("\"");
                sb.Append(":");
                Values[i].AppendTo(sb);
                i = i + 1;
            }
            sb.Append("}");
        } else if (Kind == "array") {
            sb.Append("[");
            int i = 0;
            while (i < Items.Count) {
                if (i > 0) {
                    sb.Append(",");
                }
                Items[i].AppendTo(sb);
                i = i + 1;
            }
            sb.Append("]");
        }
    }
}
