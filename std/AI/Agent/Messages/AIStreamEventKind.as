namespace Arc.Agent;

/// <summary>
/// AI 流式事件种类（RFC 038 流式主惯用法）。事件集 = 原 IAIStreamConsumer 回调面全集
/// （TextDelta / ToolCallStart / ToolArgDelta / ToolCallEnd / Usage / Completed / Error）
/// + ReasoningDelta（三个 Provider 的 SSE reasoning 增量，原 sink 时代被静默累积、
/// 不向消费者投递）。禁另起同义事件枚举。
/// </summary>
public enum AIStreamEventKind {
    /// <summary>文本增量（delta.content）。</summary>
    TextDelta,
    /// <summary>思维链增量（delta.reasoning_content / thinking）。</summary>
    ReasoningDelta,
    /// <summary>工具流开始（工具名已知）。</summary>
    ToolCallStart,
    /// <summary>工具参数增量。</summary>
    ToolArgDelta,
    /// <summary>工具流结束（完整调用就绪）。</summary>
    ToolCallEnd,
    /// <summary>token 用量上报（usage 末块）。</summary>
    Usage,
    /// <summary>流正常终结（承载最终 AIReply）。</summary>
    Completed,
    /// <summary>流失败终结（ErrorKind + ErrorMessage）。</summary>
    Error
}
