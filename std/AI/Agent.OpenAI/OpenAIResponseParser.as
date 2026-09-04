// RFC 038：OpenAI 非流式响应解析（internal，供 OpenAIChatClient 门面调用）。
//
// 类型化反序列化（Arc.Text.Json）：整文档 JSON 经 OpenAIResponse DTO 逐 token 解析
// （含 tool_calls 复杂嵌套与 reasoning_content 前向兼容回填）；usage 缓存命中
// （prompt_tokens_details.cached_tokens）映射 AITokenUsage.CacheReadTokens。
namespace Arc.Agent.OpenAI;

using Arc.Agent;
using Arc.Text.Json;

/// <summary>OpenAI 非流式响应解析器与错误提取。</summary>
internal static class OpenAIResponseParser {
    /// <summary>解析非流式响应体 JSON 为 <see cref="AIReply"/>（含 usage 缓存命中）。</summary>
    public static AIReply ParseNonStreamResponse(string json) {
        OpenAIResponse resp = new OpenAIResponse();
        JsonSerializer.Deserialize(json, (IJsonDeserializable)resp);

        if (resp.Choices.Count == 0) {
            return AIReply.Fail("ParseError", "missing choices array");
        }
        OpenAIChoice choice = resp.Choices[0];
        if (choice.Message == null) {
            return AIReply.Fail("ParseError", "missing message object");
        }

        OpenAIMessage msg = choice.Message;
        string content = msg.Content != null ? msg.Content : "";
        AIReply reply = AIReply.FromText(content);
        // 前向兼容推理链（reasoning_content）：与 content 分离，空 content 但有推理 ≠ 空回复。
        reply.ReasoningContent = msg.ReasoningContent != null ? msg.ReasoningContent : "";

        if (msg.ToolCalls != null) {
            int n = msg.ToolCalls.Count;
            int i = 0;
            while (i < n) {
                reply.ToolCalls.Add(ToAIToolCall(msg.ToolCalls[i]));
                i = i + 1;
            }
        }
        if (resp.Usage != null) {
            reply.Usage = ToTokenUsage(resp.Usage);
        }
        return reply;
    }

    private static AIToolCall ToAIToolCall(OpenAIToolCall tc) {
        string callId = tc.Id != null ? tc.Id : "";
        OpenAIFunction fn = tc.Function;
        string name = fn != null && fn.Name != null ? fn.Name : "";
        string args = fn != null && fn.Arguments != null ? fn.Arguments : "";
        return new AIToolCall(callId, name, args);
    }

    private static AITokenUsage ToTokenUsage(OpenAIUsage u) {
        AITokenUsage t = new AITokenUsage();
        t.PromptTokens = u.PromptTokens;
        t.CompletionTokens = u.CompletionTokens;
        t.TotalTokens = u.TotalTokens;
        t.CacheReadTokens = u.CachedTokens;
        return t;
    }

    /// <summary>从错误响应体提取可读错误消息；无 message 时回退为 HTTP 状态码文本。</summary>
    public static string ExtractErrorMsg(string respBody, int statusCode) {
        OpenAIError err = new OpenAIError();
        JsonSerializer.Deserialize(respBody, (IJsonDeserializable)err);
        string errMsg = err.Message != null ? err.Message : "";
        if (errMsg == "") {
            errMsg = "HTTP " + statusCode.ToString();
        }
        return errMsg;
    }
}
