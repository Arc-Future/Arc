// RFC 041 §7.9：IAIAsrStreamConsumer — ASR 流式消费 sink（单一正道，禁旁路字节流）。
//
// 段级增量（窗口段完成即投递），满足字幕/转写实时场景；词级 partial 需后端原生
// 流式 shim（§7.9 诚实边界 1），不在本契约范围。Task 完成信号 ⇔ OnCompleted/
// OnError 已投递；同步回调即天然背压。
namespace Arc.AI.Models;

using Arc.AI;

/// <summary>ASR 流式消费 sink（RFC 041 §7.9）：窗口段完成即投递。</summary>
public interface IAIAsrStreamConsumer {
    /// <summary>段完成投递（窗口边界；StartSeconds/EndSeconds 为全流时间戳）。</summary>
    void OnSegment(AITranscribeSegment segment);

    /// <summary>完成汇总（Text = 段拼接；Segments 为全段列表）。</summary>
    void OnCompleted(AITranscribeResult result);

    /// <summary>中途失败（已产出段不撤回；半成品文本由消费侧处置）。</summary>
    void OnError(AIModelException error);
}
