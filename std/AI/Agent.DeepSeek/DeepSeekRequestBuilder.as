// RFC 038：DeepSeek 请求体 JSON 构造（internal，供 DeepSeekChatClient 门面调用）。
//
// OpenAI 兼容 chat/completions 请求体：model / messages（含 tool_calls 回显与
// reasoning_content 回传）/ tools / response_format / stream / stream_options /
// max_tokens / temperature / top_p / thinking / reasoning_effort。StringBuilder 拼接
// （线性复杂度），字段发射条件与原手写实现逐字节等价。
namespace Arc.Agent.DeepSeek;

using Arc.Agent;
using Arc.Text;

/// <summary>DeepSeek 请求体 JSON 构造器（StringBuilder 拼接，与门面解耦）。</summary>
internal static class DeepSeekRequestBuilder {
    /// <summary>构造 chat/completions 请求体 JSON；stream 决定发射流式开关与 include_usage。</summary>
    public static string BuildRequestJson(AIRequest request, DeepSeekOptions options, bool stream) {
        StringBuilder sb = new StringBuilder();
        sb.Append("{");

        // model
        string model = options.Model != null ? options.Model : "deepseek-v4-pro";
        sb.Append("\"model\":\"");
        sb.Append(JsonEsc(model));
        sb.Append("\"");

        // messages
        sb.Append(",\"messages\":[");
        if (request != null && request.Messages != null) {
            int msgCount = request.Messages.Count;
            for (int i = 0; i < msgCount; i++) {
                if (i > 0) { sb.Append(","); }
                AIMessage m = request.Messages[i];
                string role = RoleToString(m.Role);
                if (role == "assistant" && m.ToolCalls != null && m.ToolCalls.Count > 0) {
                    // assistant 消息承载 tool_calls 回显（多轮工具关联的协议前提）。
                    // 思考模式工具调用轮次须完整回传 reasoning_content（DeepSeek 思考模式文档）——
                    // 否则思考链上下文断裂，模型在后续工具轮失上下文。
                    string reasoningPart = "";
                    if (m.ReasoningContent != null && m.ReasoningContent != "") {
                        reasoningPart = ",\"reasoning_content\":\"" + JsonEsc(m.ReasoningContent) + "\"";
                    }
                    sb.Append("{\"role\":\"assistant\",\"content\":");
                    sb.Append(m.BuildContentJson());
                    sb.Append(reasoningPart);
                    sb.Append(",\"tool_calls\":[");
                    int tcN = m.ToolCalls.Count;
                    for (int tc = 0; tc < tcN; tc++) {
                        AIToolCall c = m.ToolCalls[tc];
                        if (tc > 0) { sb.Append(","); }
                        sb.Append("{\"id\":\"");
                        sb.Append(JsonEsc(c.CallId));
                        sb.Append("\",\"type\":\"function\",");
                        sb.Append("\"function\":{\"name\":\"");
                        sb.Append(JsonEsc(c.Name));
                        sb.Append("\",\"arguments\":\"");
                        sb.Append(JsonEsc(c.ArgumentsJson));
                        sb.Append("\"}}");
                    }
                    sb.Append("]}");
                } else if (role == "tool") {
                    // tool 结果消息须带 tool_call_id 关联被执行的调用。
                    sb.Append("{\"role\":\"tool\",\"tool_call_id\":\"");
                    sb.Append(JsonEsc(m.ToolCallId));
                    sb.Append("\",\"content\":");
                    sb.Append(m.BuildContentJson());
                    sb.Append("}");
                } else {
                    sb.Append("{\"role\":\"");
                    sb.Append(role);
                    sb.Append("\",\"content\":");
                    sb.Append(m.BuildContentJson());
                    sb.Append("}");
                }
            }
        }
        sb.Append("]");

        // tools（OpenAI 兼容；Host 从 AIToolSet 注入 schema，空则保持不发射）
        if (request != null && request.ToolsJson != null && request.ToolsJson != "") {
            sb.Append(",\"tools\":");
            sb.Append(request.ToolsJson);
        }

        // response_format（MAF contract-first：宿主声明契约，此处按 DeepSeek 协议映射）
        //   JsonObject → {"type":"json_object"}
        //   JsonSchema → {"type":"json_schema","json_schema":{"name":"structured_output","strict":true,"schema":{...}}}
        if (request != null && request.ResponseFormat != null) {
            AIResponseFormatKind rfKind = request.ResponseFormat.Kind;
            if (rfKind == AIResponseFormatKind.JsonObject) {
                sb.Append(",\"response_format\":{\"type\":\"json_object\"}");
            } else if (rfKind == AIResponseFormatKind.JsonSchema) {
                string schema = request.ResponseFormat.SchemaJson != null ? request.ResponseFormat.SchemaJson : "{}";
                sb.Append(",\"response_format\":{\"type\":\"json_schema\",\"json_schema\":{\"name\":\"structured_output\",\"strict\":true,\"schema\":");
                sb.Append(schema);
                sb.Append("}}");
            }
        }

        // stream
        sb.Append(",\"stream\":");
        sb.Append(stream ? "true" : "false");
        if (stream) {
            // include_usage：流式末块携带 usage（业务端 token 统计 / prompt cache 命中观察）。
            sb.Append(",\"stream_options\":{\"include_usage\":true}");
        }

        // max_tokens
        if (options.MaxTokens > 0) {
            sb.Append(",\"max_tokens\":");
            sb.Append(options.MaxTokens.ToString());
        }

        // temperature
        if (options.Temperature >= 0.0) {
            sb.Append(",\"temperature\":");
            sb.Append(options.Temperature.ToString());
        }

        // top_p
        if (options.TopP >= 0.0) {
            sb.Append(",\"top_p\":");
            sb.Append(options.TopP.ToString());
        }

        // thinking — enables reasoning_content in response（可经 DeepSeekOptions.Thinking 关闭；-1 不发射）
        if (options.Thinking >= 0) {
            sb.Append(",\"thinking\":{\"type\":\"enabled\"}");
        }

        // reasoning_effort
        if (options.ReasoningEffort != null && options.ReasoningEffort != "") {
            sb.Append(",\"reasoning_effort\":\"");
            sb.Append(JsonEsc(options.ReasoningEffort));
            sb.Append("\"");
        }

        sb.Append("}");
        return sb.ToString();
    }

    private static string RoleToString(AIRole role) {
        if (role == AIRole.System) { return "system"; }
        if (role == AIRole.User) { return "user"; }
        if (role == AIRole.Assistant) { return "assistant"; }
        if (role == AIRole.Tool) { return "tool"; }
        return "user";
    }

    /// <summary>JSON 字符串转义（双引号、反斜杠、控制字符）。逐字符经 string 索引读取、
    /// StringBuilder.Append(char) 追加，无每字符 Substring 分配。</summary>
    private static string JsonEsc(string s) {
        if (s == null) { return ""; }
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
