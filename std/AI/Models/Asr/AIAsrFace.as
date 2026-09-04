// RFC 041 §7.5：AIAsrFace — ASR 域子面（统一门面的域方法落点）。
//
// 轻量只读面：经 AIModelService 统一骨架执行；后端未加载（AIModelNotAvailableException）
// 原样传播。P2 最小张量契约：输入 Float32 [1,N] PCM，输出 UInt8 [M] UTF-8 字节
// （全文）；段/词级时间戳为 P4 细化（verbose_json 对齐）。
//
// 诚实标注（实战差距审查 P0-1）：本门面只提供「管道 + 调用面」，不含真实识别能力。
// 真实 ASR 转写需用户自备 ONNX 模型 + 前后处理（tokenizer / CTC 或段级解码）；
// 框架当前不内置任何预训练模型与 tokenizer/解码器——未注入真实后端即「不可识别」。
// 不得把「仅测试 fixture 能跑通管道」当作「已具备转写能力」对外宣称。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.Text;

/// <summary>ASR 域子面（RFC 041 §7.5）：TranscribeAsync / TranscribeBatchAsync。</summary>
public class AIAsrFace : AIModelService {
    /// <summary>由统一门面 AIModels 构造（包内）。</summary>
    internal AIAsrFace(AIModelRegistry registry, string modelId, AIModelServiceOptions options)
        : base(registry, modelId, options) {
    }

    /// <summary>单段转写（RFC 041 §7.5 主方法；request.Model 非空覆盖绑定默认模型）。</summary>
    public async Task<AITranscribeResult> TranscribeAsync(AITranscribeRequest request, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (request == null) {
            throw new ArgumentNullException("request");
        }
        if (request.Input == null) {
            throw new ArgumentException("AITranscribeRequest.Input is required");
        }
        // 逐请求模型覆盖（RFC 041 §7.5）：显式 Model 非空 → 覆盖构造绑定默认；
        // 结果 Model 回显实际执行模型（覆盖值或绑定默认）。
        string modelOverride = request.Model != null && request.Model != "" ? request.Model : "";
        string effectiveModel = this.ResolveModelId(modelOverride);
        AIModelResult result = await this.ExecuteAsync(async (runner: IAIModel) => {
            List<Tensor> inputs = request.Input.ToInputs();
            List<Tensor> outs = await runner.RunAsync(inputs, ct);
            return (AIModelResult)AIAsrFace.BuildResult(effectiveModel, outs);
        }, ct, modelOverride);
        return (AITranscribeResult)result;
    }

    /// <summary>批量转写（本地循环 + 按条进度 + 取消；对齐 §7.5 统一批量契约）。</summary>
    public async Task<List<AITranscribeResult>> TranscribeBatchAsync(
        List<AITranscribeRequest> requests, Action<AIModelProgress>? progress, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        List<AITranscribeResult> results = new List<AITranscribeResult>();
        if (requests == null || requests.Count == 0) {
            return results;
        }
        int total = requests.Count;
        int i = 0;
        while (i < total) {
            ct.ThrowIfCancellationRequested();
            AITranscribeResult r = await this.TranscribeAsync(requests[i], ct);
            results.Add(r);
            if (progress != null) {
                AIModelProgress p = new AIModelProgress();
                p.Current = i + 1;
                p.Total = total;
                p.Stage = "transcribe";
                // 本地装载后调用：可空 Action 形参直接调用会退化为对未定义符号的
                // 直接调用（编译器缺陷，对齐 Arc/Types/Lazy.as 规避先例）。
                Action<AIModelProgress> report = progress;
                report(p);
            }
            i = i + 1;
        }
        return results;
    }

    /// <summary>流式转写（RFC 041 §7.9）：定长窗口分段 → 逐段批推理 → 段完成即投递。
    /// 流级句柄/在途/段级统计由 ExecuteStreamAsync 骨架收口；Task 完成 ⇔
    /// OnCompleted/OnError 已投递；块间检查取消，已产出段不撤回；ASR 幂等 →
    /// 段级重试按 Options.MaxRetries（出段前完成，消费侧无感）。</summary>
    /// <param name="request">转写请求（WindowSeconds 窗口时长，&lt;=0 → 30.0）。</param>
    /// <param name="consumer">流式消费 sink（同步回调即背压）。</param>
    /// <param name="ct">协作式取消令牌（取消收敛 OnError，已产出段不撤回）。</param>
    public async Task TranscribeStreamAsync(
        AITranscribeRequest request, IAIAsrStreamConsumer consumer, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (request == null) {
            throw new ArgumentNullException("request");
        }
        if (consumer == null) {
            throw new ArgumentNullException("consumer");
        }
        if (request.Input == null) {
            throw new ArgumentException("AITranscribeRequest.Input is required");
        }
        string modelOverride = request.Model != null && request.Model != "" ? request.Model : "";
        string effectiveId = this.ResolveModelId(modelOverride);
        List<AITranscribeSegment> segments = new List<AITranscribeSegment>();
        Stopwatch sw = Stopwatch.StartNew();
        try {
            await this.ExecuteStreamAsync(async (runner: IAIModel) => {
                List<float> samples = request.Input.Samples;
                int totalSamples = samples.Count;
                int rate = request.Input.SampleRate > 0 ? request.Input.SampleRate : 16000;
                int channels = request.Input.Channels > 0 ? request.Input.Channels : 1;
                double windowSeconds = request.WindowSeconds > 0.0 ? request.WindowSeconds : 30.0;
                // 窗口采样数（含声道展开）；非法窗口（<=0，如窗口短于单采样）兜底全量一块。
                int windowSamples = (int)(windowSeconds * (double)rate * (double)channels);
                if (windowSamples <= 0) {
                    windowSamples = totalSamples > 0 ? totalSamples : 1;
                }
                int start = 0;
                while (start < totalSamples) {
                    ct.ThrowIfCancellationRequested();
                    // 循环变量先落本地快照再入闭包（对齐 AITtsFace 闭包防御惯例）。
                    int segStart = start;
                    int segLen = windowSamples < totalSamples - start ? windowSamples : totalSamples - start;
                    int segIndex = segments.Count;
                    // 块结果经显式类型局部中转（编译器缺陷规避，对齐 AITtsFace：
                    // lambda 内联 await 提取/下转型回退 Int 截断指针）。
                    Task<AIModelResult> segTask = this.ExecuteBlockAsync(
                        async (r: IAIModel) => {
                            List<float> part = new List<float>();
                            int k = segStart;
                            while (k < segStart + segLen) {
                                part.Add(samples[k]);
                                k = k + 1;
                            }
                            AIAudioInput window = AIAudioInput.FromPcmFloat(part, rate, channels);
                            List<Tensor> inputs = window.ToInputs();
                            List<Tensor> outs = await r.RunAsync(inputs, ct);
                            AITranscribeSegment s = new AITranscribeSegment();
                            s.Index = segIndex;
                            s.Text = AIAsrFace.DecodeText(outs);
                            s.StartSeconds = (double)segStart / ((double)rate * (double)channels);
                            s.EndSeconds = (double)(segStart + segLen) / ((double)rate * (double)channels);
                            return (AIModelResult)s;
                        }, runner, effectiveId, true);
                    AIModelResult segResult = await segTask;
                    AITranscribeSegment segment = (AITranscribeSegment)segResult;
                    segments.Add(segment);
                    consumer.OnSegment(segment);
                    start = start + segLen;
                }
                // 返回值无语义恒 null（ExecuteStreamAsync 带值签名规避，见其注释）。
                return (AIModelResult)null;
            }, ct, modelOverride);
        } catch (Exception ex) {
            AIModelException error = AIModelService.ToStreamError(ex, effectiveId);
            consumer.OnError(error);
            return;
        }
        AITranscribeResult summary = new AITranscribeResult();
        summary.Model = effectiveId;
        string fullText = "";
        int t = 0;
        while (t < segments.Count) {
            fullText = fullText + segments[t].Text;
            t = t + 1;
        }
        summary.Text = fullText;
        summary.Language = null;
        summary.DurationSeconds = request.Input.Samples.Count
            / ((double)(request.Input.SampleRate > 0 ? request.Input.SampleRate : 16000)
            * (double)(request.Input.Channels > 0 ? request.Input.Channels : 1));
        summary.Segments = segments;
        summary.Words = null;
        summary.Usage = new AIUsage();
        summary.Usage.DurationMs = sw.ElapsedMilliseconds;
        consumer.OnCompleted(summary);
    }

    /// <summary>语义翻译：输出 UInt8 字节 → 全文（段/词时间戳 P4 置 null）。</summary>
    private static AITranscribeResult BuildResult(string model, List<Tensor> outs) {
        AITranscribeResult result = new AITranscribeResult();
        result.Model = model != null ? model : "";
        result.Text = AIAsrFace.DecodeText(outs);
        result.Language = null;
        result.DurationSeconds = -1.0;
        result.Segments = null;
        result.Words = null;
        result.Usage = new AIUsage();
        return result;
    }

    /// <summary>解码首个输出张量的 UTF-8 字节为文本（无输出 → 空串）。</summary>
    private static string DecodeText(List<Tensor> outs) {
        if (outs == null || outs.Count == 0 || outs[0] == null) {
            return "";
        }
        List<byte> bytes = outs[0].ReadByte();
        if (bytes == null) {
            return "";
        }
        return Encoding.GetString(bytes.ToArray());
    }
}
