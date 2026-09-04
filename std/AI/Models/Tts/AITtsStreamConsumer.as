// RFC 041 §7.9：IAITtsStreamConsumer — TTS 流式消费 sink（单一正道，禁旁路字节流）。
//
// IAsyncEnumerable 已由 RFC 008 定义并落地；Agent 侧流式管线已迁移至
// IAsyncEnumerable<AIStreamEvent> 单一惯用法（RFC 038），TTS/ASR 推理侧 sink 契约
// 与 RFC 008 的对齐为后续独立工作流（本文件不在该范围内）。
// Task 完成信号 ⇔ OnCompleted/OnError 已投递；
// 同步回调即天然背压（消费方阻塞 = 编排暂停），框架不建异步缓冲队列。
namespace Arc.AI.Models;

using Arc.AI;

/// <summary>TTS 流式消费 sink（RFC 041 §7.9）：音频块增量投递。</summary>
public interface IAITtsStreamConsumer {
    /// <summary>增量音频块（Index 0 起递增；IsFinal 标末块）。</summary>
    void OnAudioChunk(AITtsChunk chunk);

    /// <summary>完成汇总（Audio.Samples 为各块拼接；Usage.DurationMs 为各块累计）。</summary>
    void OnCompleted(AITtsResult result);

    /// <summary>中途失败（已产出块不撤回；半成品音频由消费侧处置）。</summary>
    void OnError(AIModelException error);
}
