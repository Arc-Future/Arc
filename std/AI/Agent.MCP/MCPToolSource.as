// RFC 038：MCPToolSource——MCP 工具源，把远端 MCP 工具映射为 Arc.Agent 工具契约
// （AIToolDescriptor + 转发 handler）。连接 initialize → tools/list → 注册进 AIToolSet。
// 工具执行转发 tools/call；结果取 content[0].text。
namespace Arc.Agent.MCP;

using Arc;
using Arc.Agent;
using Arc.Collections;

/// <summary>
/// MCP 工具源：连接外部 MCP server 并把其工具映射为 Arc.Agent 的 [AITool] 等价契约。
/// ConnectAsync 从 tools/list 提取并保留每个工具的 name/description/inputSchema，
/// Fill 时逐工具注册 AIToolDescriptor（含 schema）+ 转发 handler。
/// </summary>
public class MCPToolSource {
    private MCPHttpClient _client;
    private List<MCPToolInfo> _tools;

    public MCPToolSource(MCPHttpClient client) {
        if (client == null) {
            throw new ArgumentNullException("client");
        }
        _client = client;
        _tools = new List<MCPToolInfo>();
    }

    public MCPHttpClient Client { get { return _client; } }
    public int ToolCount { get { return _tools.Count; } }

    /// <summary>初始化握手 + 枚举远端工具（含 description/inputSchema）。返回 "" = 成功；非空 = 错误消息。</summary>
    public async Task<string> ConnectAsync(CancellationToken ct) {
        string initErr = await _client.InitializeAsync(ct);
        if (initErr != "") {
            return initErr;
        }
        List<MCPToolInfo> tools = await _client.ListToolsAsync(ct);
        if (tools.Count == 0) {
            return "no tools";
        }
        _tools = tools;
        return "";
    }

    /// <summary>把已枚举的 MCP 工具注册进 AIToolSet（[AITool] 等价描述 + 转发 handler）。</summary>
    public void Fill(AIToolSet tools) {
        if (tools == null) {
            return;
        }
        int i = 0;
        int n = _tools.Count;
        while (i < n) {
            MCPToolInfo info = _tools[i];
            AIToolDescriptor d = new AIToolDescriptor(info.Name, info.Description, "ai.Tool", false);
            if (info.InputSchema != "") {
                d.ParametersSchema = info.InputSchema;
            }
            tools.Add(d, new MCPToolHandler(_client, info.Name));
            i = i + 1;
        }
    }

    /// <summary>指定索引工具的 inputSchema 原始 JSON；越界返回 ""。</summary>
    public string ToolSchema(int index) {
        if (index < 0 || index >= _tools.Count) {
            return "";
        }
        return _tools[index].InputSchema;
    }
}

/// <summary>MCP 工具转发 handler：InvokeAsync → tools/call → content[0].text。</summary>
internal class MCPToolHandler : AIToolHandler {
    private MCPHttpClient _client;
    private string _name;

    public MCPToolHandler(MCPHttpClient client, string name) {
        _client = client;
        _name = name != null ? name : "";
    }

    public override string Name {
        get { return _name; }
    }

    public override string Capability {
        get { return "ai.Tool"; }
    }

    public override async Task<AIToolResult> InvokeAsync(AIToolCall call, CancellationToken cancellationToken) {
        string cid = call != null && call.CallId != null ? call.CallId : "";
        string args = call != null && call.ArgumentsJson != null ? call.ArgumentsJson : "";
        string text = await _client.CallToolAsync(_name, args, cancellationToken);
        if (text == "") {
            return AIToolResult.Fail(cid, "MCPEmptyResult", "MCP tool returned empty result: " + _name);
        }
        return AIToolResult.Ok(cid, text);
    }
}
