// RFC 038：AI 流式事件（IAsyncEnumerable 单一惯用法的事件模型）。
//
// Provider 经 IAIChatClient.StreamEventsAsync 产出本事件序列；宿主（AISessionStreamCollector）
// 逐事件消费。终结语义：每个流恰以一个 Completed 或 Error 事件收尾（其后 MoveNextAsync
// 返回 false），取消中途表现为 Error("Cancelled", ...)。
namespace Arc.Agent;

using Arc;
using Arc.Collections;

/// <summary>
/// 单个 AI 流式事件：Kind 判别 + 按 Kind 有效的载荷槽（TextDelta → <see cref="Text"/>；
/// Completed → <see cref="Reply"/>；Error → <see cref="ErrorKind"/>/<see cref="ErrorMessage"/> 等）。
/// </summary>
public class AIStreamEvent {
    /// <summary>事件种类（判别载荷槽）。</summary>
    public AIStreamEventKind Kind;
    /// <summary>TextDelta：文本增量。</summary>
    public string Text;
    /// <summary>ReasoningDelta：思维链增量。</summary>
    public string Reasoning;
    /// <summary>ToolCallStart：工具流开始载荷。</summary>
    public AIToolCallStart ToolCallStart;
    /// <summary>ToolArgDelta：工具参数增量载荷。</summary>
    public AIToolArgDelta ToolArgDelta;
    /// <summary>ToolCallEnd：工具流结束载荷。</summary>
    public AIToolCallEnd ToolCallEnd;
    /// <summary>Usage：token 用量载荷。</summary>
    public AITokenUsage Usage;
    /// <summary>Completed：最终回复载荷。</summary>
    public AIReply Reply;
    /// <summary>Error：错误种类（Cancelled / HttpError / StreamError 等）。</summary>
    public string ErrorKind;
    /// <summary>Error：错误消息。</summary>
    public string ErrorMessage;

    public AIStreamEvent() {
        this.Kind = AIStreamEventKind.TextDelta;
        this.Text = "";
        this.Reasoning = "";
        this.ToolCallStart = null;
        this.ToolArgDelta = null;
        this.ToolCallEnd = null;
        this.Usage = null;
        this.Reply = null;
        this.ErrorKind = "";
        this.ErrorMessage = "";
    }

    /// <summary>文本增量事件。</summary>
    public static AIStreamEvent TextDelta(string text) {
        AIStreamEvent e = new AIStreamEvent();
        e.Kind = AIStreamEventKind.TextDelta;
        e.Text = text != null ? text : "";
        return e;
    }

    /// <summary>思维链增量事件。</summary>
    public static AIStreamEvent ReasoningDelta(string reasoning) {
        AIStreamEvent e = new AIStreamEvent();
        e.Kind = AIStreamEventKind.ReasoningDelta;
        e.Reasoning = reasoning != null ? reasoning : "";
        return e;
    }

    /// <summary>工具流开始事件。</summary>
    public static AIStreamEvent ToolCallStart(AIToolCallStart start) {
        AIStreamEvent e = new AIStreamEvent();
        e.Kind = AIStreamEventKind.ToolCallStart;
        e.ToolCallStart = start;
        return e;
    }

    /// <summary>工具参数增量事件。</summary>
    public static AIStreamEvent ToolArgDelta(AIToolArgDelta delta) {
        AIStreamEvent e = new AIStreamEvent();
        e.Kind = AIStreamEventKind.ToolArgDelta;
        e.ToolArgDelta = delta;
        return e;
    }

    /// <summary>工具流结束事件。</summary>
    public static AIStreamEvent ToolCallEnd(AIToolCallEnd end) {
        AIStreamEvent e = new AIStreamEvent();
        e.Kind = AIStreamEventKind.ToolCallEnd;
        e.ToolCallEnd = end;
        return e;
    }

    /// <summary>token 用量事件。</summary>
    public static AIStreamEvent Usage(AITokenUsage usage) {
        AIStreamEvent e = new AIStreamEvent();
        e.Kind = AIStreamEventKind.Usage;
        e.Usage = usage;
        return e;
    }

    /// <summary>流正常终结事件（承载最终回复）。</summary>
    public static AIStreamEvent Completed(AIReply reply) {
        AIStreamEvent e = new AIStreamEvent();
        e.Kind = AIStreamEventKind.Completed;
        e.Reply = reply;
        return e;
    }

    /// <summary>流失败终结事件。</summary>
    public static AIStreamEvent Error(string errorKind, string message) {
        AIStreamEvent e = new AIStreamEvent();
        e.Kind = AIStreamEventKind.Error;
        e.ErrorKind = errorKind != null ? errorKind : "";
        e.ErrorMessage = message != null ? message : "";
        return e;
    }

    /// <summary>
    /// 同步终结错误流：构造即完成、仅含单个 Error 事件的异步序列——供 Provider
    /// 在发请求前校验失败（取消/空请求等）以流形态返回错误，保持
    /// StreamEventsAsync 单一返回类型、无异常旁路。
    /// </summary>
    public static async IAsyncEnumerable<AIStreamEvent> ErrorStream(string errorKind, string message) {
        yield return AIStreamEvent.Error(errorKind, message);
    }
}
