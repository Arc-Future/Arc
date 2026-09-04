// RFC 038：Agnes 非流式响应解析（internal，供 AgnesChatClient 门面调用）。
//
// 类型化反序列化（Arc.Text.Json）：整文档 JSON 经 AgnesResponse DTO 逐 token 解析
// （含 reasoning/thinking 与 content 分离、tool_calls 嵌套）；usage 缓存统计
// （cache_read_input_tokens / cache_creation_input_tokens）映射 AITokenUsage。
namespace Arc.Agent.Agnes;

using Arc.Agent;
using Arc.Text.Json;

/// <summary>Agnes 非流式响应解析器与错误提取。</summary>
internal static class AgnesResponseParser
{
    /// <summary>解析非流式响应体 JSON 为 <see cref="AIReply"/>（含 usage 缓存统计）。</summary>
    public static AIReply ParseNonStreamResponse(string json)
    {
        AgnesResponse resp = new AgnesResponse();
        JsonSerializer.Deserialize(json, (IJsonDeserializable)resp);

        if (resp.Choices.Count == 0)
        {
            return AIReply.Fail("ParseError", "missing choices array");
        }
        AgnesChoice choice = resp.Choices[0];
        if (choice.Message == null)
        {
            return AIReply.Fail("ParseError", "missing message object");
        }

        AgnesMessage msg = choice.Message;
        string content = msg.Content != null ? msg.Content : "";
        AIReply reply = AIReply.FromText(content);
        // 思维链（reasoning_content / thinking 扩展字段 / content 数组 reasoning 部件）与 content
        // 同级分离：空 content 但有 reasoning ≠ 空回复（会话层据此续问而非误判终结）。
        reply.ReasoningContent = msg.ReasoningContent != null ? msg.ReasoningContent : "";

        if (msg.ToolCalls != null)
        {
            int n = msg.ToolCalls.Count;
            int i = 0;
            while (i < n)
            {
                reply.ToolCalls.Add(ToAIToolCall(msg.ToolCalls[i]));
                i = i + 1;
            }
        }

        if (resp.Usage != null)
        {
            reply.Usage = ToTokenUsage(resp.Usage);
        }
        return reply;
    }

    private static AIToolCall ToAIToolCall(AgnesToolCall tc)
    {
        string callId = tc.Id != null ? tc.Id : "";
        AgnesFunction fn = tc.Function;
        string name = fn != null && fn.Name != null ? fn.Name : "";
        string args = fn != null && fn.Arguments != null ? fn.Arguments : "";
        return new AIToolCall(callId, name, args);
    }

    private static AITokenUsage ToTokenUsage(AgnesUsage u)
    {
        AITokenUsage t = new AITokenUsage();
        t.PromptTokens = u.PromptTokens;
        t.CompletionTokens = u.CompletionTokens;
        t.TotalTokens = u.TotalTokens;
        t.CacheReadTokens = u.CacheReadTokens;
        t.CacheCreationTokens = u.CacheCreationTokens;
        return t;
    }

    /// <summary>从错误响应体提取可读错误消息；无 message 时回退为 HTTP 状态码文本。</summary>
    public static string ExtractErrorMsg(string respBody, int statusCode)
    {
        AgnesError err = new AgnesError();
        JsonSerializer.Deserialize(respBody, (IJsonDeserializable)err);
        string errMsg = err.Message != null ? err.Message : "";
        if (errMsg == "")
        {
            errMsg = "HTTP " + statusCode.ToString();
        }
        return errMsg;
    }
}
