// RFC 038：DeepSeek 非流式响应解析（internal，供 DeepSeekChatClient 门面调用）。
//
// 类型化反序列化（Arc.Text.Json）：整文档 JSON 经 DeepSeekResponse DTO 的 ReadJson
// 逐 token 解析（含 tool_calls 复杂嵌套与 reasoning_content 回填）；错误响应经
// DeepSeekError DTO 提取 message。不再手写字符串扫描。
namespace Arc.Agent.DeepSeek;

using Arc.Agent;
using Arc.Text.Json;

/// <summary>DeepSeek 非流式响应解析器与错误提取。</summary>
internal static class DeepSeekResponseParser {
    /// <summary>解析非流式响应体 JSON 为 <see cref="AIReply"/>。</summary>
    public static AIReply ParseNonStreamResponse(string json) {
        DeepSeekResponse resp = new DeepSeekResponse();
        JsonSerializer.Deserialize(json, (IJsonDeserializable)resp);

        if (resp.Choices.Count == 0) {
            return AIReply.Fail("ParseError", "missing choices array");
        }
        DeepSeekChoice choice = resp.Choices[0];
        if (choice.Message == null) {
            return AIReply.Fail("ParseError", "missing message object");
        }

        DeepSeekMessage msg = choice.Message;
        string content = msg.Content != null ? msg.Content : "";
        AIReply reply = AIReply.FromText(content);
        // 思维链（reasoning_content，与 content 同级）：思考模式思考链回填，供会话工具调用轮次回传。
        reply.ReasoningContent = msg.ReasoningContent != null ? msg.ReasoningContent : "";

        if (msg.ToolCalls != null) {
            int n = msg.ToolCalls.Count;
            int i = 0;
            while (i < n) {
                reply.ToolCalls.Add(ToAIToolCall(msg.ToolCalls[i]));
                i = i + 1;
            }
        }
        // token 用量上报（A5 成本核算前置：非流式路径回填 Usage，供会话 TotalUsage 聚合；
        // 与 OpenAI 非流式解析对齐，消除「非流式无用量」缺口）。
        if (resp.Usage != null) {
            reply.Usage = ToTokenUsage(resp.Usage);
        }
        return reply;
    }

    /// <summary>DeepSeek usage → <see cref="AITokenUsage"/>（缓存命中/写入对齐 AITokenUsage 字段语义）。</summary>
    private static AITokenUsage ToTokenUsage(DeepSeekUsage u) {
        AITokenUsage t = new AITokenUsage();
        t.PromptTokens = u.PromptTokens;
        t.CompletionTokens = u.CompletionTokens;
        t.TotalTokens = u.TotalTokens;
        t.CacheReadTokens = u.PromptCacheHitTokens;
        t.CacheCreationTokens = u.PromptCacheMissTokens;
        return t;
    }

    private static AIToolCall ToAIToolCall(DeepSeekToolCall tc) {
        string callId = tc.Id != null ? tc.Id : "";
        DeepSeekFunction fn = tc.Function;
        string name = fn != null && fn.Name != null ? fn.Name : "";
        string args = fn != null && fn.Arguments != null ? fn.Arguments : "";
        return new AIToolCall(callId, name, args);
    }

    /// <summary>从错误响应体提取可读错误消息；无 message 时回退为 HTTP 状态码文本。</summary>
    public static string ExtractErrorMsg(string respBody, int statusCode) {
        DeepSeekError err = new DeepSeekError();
        JsonSerializer.Deserialize(respBody, (IJsonDeserializable)err);
        string errMsg = err.Message != null ? err.Message : "";
        if (errMsg == "") {
            errMsg = "HTTP " + statusCode.ToString();
        }
        return errMsg;
    }
}
