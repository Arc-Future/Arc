// RFC 038：OpenAI 请求体 JSON 构造（internal，供 OpenAIChatClient 门面调用）。
//
// OpenAI 官方 chat/completions 请求体：model / messages（含 tool_calls 回显，不发射
// reasoning_content——OpenAI 官方不收该字段）/ tools / response_format / stream /
// stream_options / max_completion_tokens / temperature / top_p / reasoning_effort。
// 与 DeepSeek/Agnes 的差异即「OpenAI 官方差异面」：max_completion_tokens（替代 max_tokens）
// + reasoning_effort（o 系列）；无 thinking / tool_choice / chat_template_kwargs。
namespace Arc.Agent.OpenAI;

using Arc.Agent;
using Arc.Text;

/// <summary>OpenAI 请求体 JSON 构造器（StringBuilder 拼接，与门面解耦）。</summary>
internal static class OpenAIRequestBuilder {
    /// <summary>构造 chat/completions 请求体 JSON；stream 决定发射流式开关与 include_usage。</summary>
    public static string BuildRequestJson(AIRequest request, OpenAIOptions options, bool stream) {
        StringBuilder sb = new StringBuilder();
        sb.Append("{");

        // model
        string model = options.Model != null ? options.Model : "gpt-4o-mini";
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
                    // OpenAI 官方不回传 reasoning_content（推理不透明），故不发射。
                    sb.Append("{\"role\":\"assistant\",\"content\":");
                    sb.Append(m.BuildContentJson());
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

        // response_format（MAF contract-first：宿主声明契约，此处按 OpenAI 官方协议映射）
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
            // include_usage：流式末块携带 usage（业务端 token 统计 / prompt 缓存命中观察）。
            sb.Append(",\"stream_options\":{\"include_usage\":true}");
        }

        // max_completion_tokens（OpenAI 官方：o 系列与新模型；替代旧 max_tokens）
        if (options.MaxCompletionTokens > 0) {
            sb.Append(",\"max_completion_tokens\":");
            sb.Append(options.MaxCompletionTokens.ToString());
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

        // reasoning_effort（o 系列推理强度）
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
