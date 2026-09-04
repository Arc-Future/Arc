// RFC 038：DeepSeek 流式事件序列（internal，供 DeepSeekChatClient 门面构造）。
//
// 原生异步迭代器（RFC 044 单一惯用法）：冷启动建连 → SSE 拉取 → 领域映射 → 终结
// 收尾全部线性化在一个迭代器方法里——yield return 即事件投递，无手写状态机/待发
// 队列。每个 data: 行是完整 JSON 文档（DeepSeekResponse DTO 类型化反序列化）；
// 唯一"增量"的是 tool_calls[].function.arguments 的字符串值（跨行累积拼接）。
// usage 末块（include_usage）延迟到终结时上报。流恰以 Completed/Error 终结事件收尾。
namespace Arc.Agent.DeepSeek;

using Arc;
using Arc.Agent;
using Arc.Collections;
using Arc.Net;
using Arc.Text.Json;

/// <summary>
/// DeepSeek 流式事件流生产：把 OpenAI 兼容 SSE 响应解码为 <see cref="AIStreamEvent"/>
/// 异步序列。HTTP 发送延迟到首次枚举（冷流）；取消经参数令牌全链路传播。
/// 异常经非抛错边界（StartAsync / ParseLine / SseDecoder.ReadAsync）收敛为值，
/// 流内错误一律转 Error 终结事件而非逃逸。
/// </summary>
internal static class DeepSeekEventStream {

    /// <summary>流式事件序列：延迟建连 → 逐 SSE 事件领域映射 → 终结收尾。</summary>
    internal static async IAsyncEnumerable<AIStreamEvent> Events(DeepSeekOptions options, HttpClient http, string url, AIRequest request, CancellationToken cancellationToken) {
        DeepSeekStreamStart start = await DeepSeekEventStream.StartAsync(options, http, url, request);
        if (start.Error != null) {
            yield return start.Error;
            yield break;
        }
        HttpResponseMessage resp = start.Response;
        IAsyncEnumerator<SseEvent> sse = SseDecoder.Decode(resp.LiveStream, resp.IsChunkedStreaming, cancellationToken).GetAsyncEnumerator(CancellationToken.None);

        // 流式累积面（提升为状态机字段，跨挂起点存活）
        string text = "";
        string reasoning = "";
        List<string> tcCallIds = new List<string>();
        List<string> tcArgs = new List<string>();
        List<bool> tcStarted = new List<bool>();
        // usage 末块可能在 finish_reason 之后到达：延迟到终结时上报。
        AITokenUsage usageAcc = null;

        while (true) {
            if (cancellationToken.IsCancellationRequested) {
                resp.Dispose();
                yield return AIStreamEvent.Error("Cancelled", "DeepSeekChatClient.StreamEventsAsync: canceled during stream");
                yield break;
            }
            SseReadStep step = await SseDecoder.ReadAsync(sse);
            if (step.Error != null) {
                resp.Dispose();
                yield return AIStreamEvent.Error("StreamError", "DeepSeekChatClient.StreamEventsAsync: " + step.Error);
                yield break;
            }
            if (!step.Moved) {
                break;
            }
            if (step.Event.Data == "[DONE]") {
                break;
            }
            if (step.Event.Data == "") {
                continue;
            }
            DeepSeekLineParse parsed = DeepSeekEventStream.ParseLine(step.Event.Data);
            if (parsed.Error != null) {
                resp.Dispose();
                yield return parsed.Error;
                yield break;
            }
            DeepSeekResponse line = parsed.Line;
            if (line.Usage != null) {
                usageAcc = DeepSeekEventStream.ToTokenUsage(line.Usage);
            }
            if (line.Choices.Count == 0) {
                continue;
            }
            DeepSeekChoice choice = line.Choices[0];
            if (choice.Delta == null) {
                // 无 delta 块（可能仅携带 finish_reason）：终结由 [DONE]/EOF 驱动
                continue;
            }
            DeepSeekDelta delta = choice.Delta;

            string content = delta.Content != null ? delta.Content : "";
            if (content != "") {
                text = text + content;
                yield return AIStreamEvent.TextDelta(content);
            }

            string reasoningDelta = delta.ReasoningContent != null ? delta.ReasoningContent : "";
            if (reasoningDelta != "") {
                reasoning = reasoning + reasoningDelta;
                yield return AIStreamEvent.ReasoningDelta(reasoningDelta);
            }

            // 流式 tool_calls 增量：index 定位跟踪数组，arguments 跨行累积拼接
            if (delta.ToolCalls != null) {
                int i = 0;
                while (i < delta.ToolCalls.Count) {
                    DeepSeekToolCall tc = delta.ToolCalls[i];
                    int tcidx = tc.Index;
                    string tcid = tc.Id != null ? tc.Id : "";
                    DeepSeekFunction fn = tc.Function;
                    string tcFuncName = fn != null && fn.Name != null ? fn.Name : "";
                    string tcFuncArgs = fn != null && fn.Arguments != null ? fn.Arguments : "";

                    while (tcCallIds.Count <= tcidx) {
                        tcCallIds.Add("");
                        tcArgs.Add("");
                        tcStarted.Add(false);
                    }

                    if (tcid != "" && !tcStarted[tcidx]) {
                        tcCallIds[tcidx] = tcid;
                        tcArgs[tcidx] = "";
                        tcStarted[tcidx] = true;
                        yield return AIStreamEvent.ToolCallStart(new AIToolCallStart(tcid, tcFuncName));
                    }

                    if (tcStarted[tcidx] && tcFuncArgs != "") {
                        string prevArgs = tcArgs[tcidx];
                        if (tcFuncArgs.Length > prevArgs.Length) {
                            string argDelta = tcFuncArgs.Substring(prevArgs.Length, tcFuncArgs.Length - prevArgs.Length);
                            tcArgs[tcidx] = tcFuncArgs;
                            yield return AIStreamEvent.ToolArgDelta(new AIToolArgDelta(tcCallIds[tcidx], "arguments", argDelta));
                        } else if (tcFuncArgs != prevArgs) {
                            tcArgs[tcidx] = tcFuncArgs;
                            yield return AIStreamEvent.ToolArgDelta(new AIToolArgDelta(tcCallIds[tcidx], "arguments", tcFuncArgs));
                        }
                    }

                    i = i + 1;
                }
            }
        }

        // 正常终结：关闭在途工具流 → 上报 usage → 释放连接 → 最终回复
        int j = 0;
        while (j < tcCallIds.Count) {
            if (tcStarted[j]) {
                yield return AIStreamEvent.ToolCallEnd(new AIToolCallEnd(tcCallIds[j]));
                tcStarted[j] = false;
            }
            j = j + 1;
        }
        if (usageAcc != null) {
            yield return AIStreamEvent.Usage(usageAcc);
        }
        AIReply reply = AIReply.FromText(text);
        reply.ReasoningContent = reasoning;
        resp.Dispose();
        yield return AIStreamEvent.Completed(reply);
    }

    /// <summary>非抛错建连边界：发起流式 HTTP 请求；网络/HTTP 错误转 Error 终结事件。</summary>
    private static async Task<DeepSeekStreamStart> StartAsync(DeepSeekOptions options, HttpClient http, string url, AIRequest request) {
        DeepSeekStreamStart r = new DeepSeekStreamStart();
        try {
            string body = DeepSeekRequestBuilder.BuildRequestJson(request, options, true);
            HttpRequestMessage req = new HttpRequestMessage(HttpMethod.POST, new Uri(url));
            req.Headers.Add("Authorization", "Bearer " + options.ApiKey);
            req.Content = new StringContent(body, "application/json");
            req.StreamResponse = true;
            HttpResponseMessage resp = await http.SendAsync(req);
            if (resp == null) {
                r.Error = AIStreamEvent.Error("NetworkError", "Failed to connect to " + options.BaseUrl);
                return r;
            }
            if (resp.StatusCode != 200) {
                string errBody = resp.Body != null ? resp.Body : "";
                string errMsg = DeepSeekResponseParser.ExtractErrorMsg(errBody, resp.StatusCode);
                resp.Dispose();
                r.Error = AIStreamEvent.Error("HttpError", errMsg);
                return r;
            }
            r.Response = resp;
            return r;
        } catch (Exception ex) {
            r.Error = AIStreamEvent.Error("StreamError", "DeepSeekChatClient.StreamEventsAsync: " + (ex != null && ex.Message != null ? ex.Message : "stream error"));
            return r;
        }
    }

    /// <summary>非抛错解析边界：JSON 文档 → DeepSeekResponse DTO；解析失败转 Error 终结事件。</summary>
    private static DeepSeekLineParse ParseLine(string json) {
        DeepSeekLineParse r = new DeepSeekLineParse();
        try {
            r.Line = new DeepSeekResponse();
            JsonSerializer.Deserialize(json, (IJsonDeserializable)r.Line);
            return r;
        } catch (Exception ex) {
            r.Error = AIStreamEvent.Error("StreamError", "DeepSeekChatClient.StreamEventsAsync: " + (ex != null && ex.Message != null ? ex.Message : "stream error"));
            return r;
        }
    }

    private static AITokenUsage ToTokenUsage(DeepSeekUsage u) {
        AITokenUsage t = new AITokenUsage();
        t.PromptTokens = u.PromptTokens;
        t.CompletionTokens = u.CompletionTokens;
        t.TotalTokens = u.TotalTokens;
        t.CacheReadTokens = u.PromptCacheHitTokens;
        t.CacheCreationTokens = u.PromptCacheMissTokens;
        return t;
    }
}

/// <summary>建连结果（非抛错边界载体）：<see cref="Error"/> 非 null 即失败（此时 Response 为 null）。</summary>
internal class DeepSeekStreamStart {
    /// <summary>成功建立的流式响应。</summary>
    public HttpResponseMessage Response { get; set; }

    /// <summary>失败终结事件。</summary>
    public AIStreamEvent Error { get; set; }

    public DeepSeekStreamStart() {
        this.Response = null;
        this.Error = null;
    }
}

/// <summary>行解析结果（非抛错边界载体）：<see cref="Error"/> 非 null 即失败（此时 Line 为 null）。</summary>
internal class DeepSeekLineParse {
    /// <summary>解析出的响应 DTO。</summary>
    public DeepSeekResponse Line { get; set; }

    /// <summary>失败终结事件。</summary>
    public AIStreamEvent Error { get; set; }

    public DeepSeekLineParse() {
        this.Line = null;
        this.Error = null;
    }
}
