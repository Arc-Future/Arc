// RFC 041 §7.5/§7.9：AITtsFace — TTS 域子面（批式 + 流式编排）。
//
// 轻量只读面：批式经 AIModelService 统一骨架执行；流式（§7.9）走流式专用路径——
// Acquire 一次贯穿流生命周期，逐句批推理增量投递 sink，块间检查取消，已产出块
// 不撤回。P2 最小张量契约：输入 UInt8 [N] UTF-8 字节（文本），输出 Float32 [M]
// PCM 采样块（容器编码属应用层）。TTS 非幂等（§7.3）：批式默认不重试由组合根
// Options 保证，流式硬规则不重试（ExecuteBlockAsync retry=false）。
//
// 诚实标注（实战差距审查 P0-1，对齐 §7.8）：本门面只提供「管道 + 调用面 + 流式
// 编排」，不含真实合成能力。真实 TTS 需用户自备 ONNX 模型 + 前后处理（文本正则
// 化 / 声学后处理）；框架不内置任何预训练模型——未注入真实后端即「不可合成」。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.Text;

/// <summary>TTS 域子面（RFC 041 §7.5/§7.9）：SynthesizeAsync / SynthesizeStreamAsync。</summary>
public class AITtsFace : AIModelService {
    /// <summary>由统一门面 AIModels 构造（包内）。</summary>
    internal AITtsFace(AIModelRegistry registry, string modelId, AIModelServiceOptions options)
        : base(registry, modelId, options) {
    }

    /// <summary>批式合成（RFC 041 §7.5 主方法；request.Model 非空覆盖绑定默认模型）：
    /// 全文单块执行（等同流式一块）。</summary>
    public async Task<AITtsResult> SynthesizeAsync(AITtsRequest request, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (request == null) {
            throw new ArgumentNullException("request");
        }
        if (request.Input == null) {
            throw new ArgumentException("AITtsRequest.Input is required");
        }
        string modelOverride = request.Model != null && request.Model != "" ? request.Model : "";
        string effectiveModel = this.ResolveModelId(modelOverride);
        AIModelResult result = await this.ExecuteAsync(async (runner: IAIModel) => {
            List<Tensor> inputs = AITtsFace.TextInputs(request.Input);
            List<Tensor> outs = await runner.RunAsync(inputs, ct);
            AITtsResult r = AITtsFace.BuildResult(effectiveModel, request, outs);
            return (AIModelResult)r;
        }, ct, modelOverride);
        return (AITtsResult)result;
    }

    /// <summary>流式合成（RFC 041 §7.9）：切句 → 逐句批推理 → 音频块增量投递。
    /// 流级句柄/在途/块级统计由 ExecuteStreamAsync 骨架收口；Task 完成 ⇔
    /// OnCompleted/OnError 已投递；块间检查取消，已产出块不撤回。</summary>
    /// <param name="request">合成请求（MaxChunkChars 兜底切句上限，&lt;=0 → 120）。</param>
    /// <param name="consumer">流式消费 sink（同步回调即背压）。</param>
    /// <param name="ct">协作式取消令牌（取消收敛 OnError，已产出块不撤回）。</param>
    public async Task SynthesizeStreamAsync(AITtsRequest request, IAITtsStreamConsumer consumer, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (request == null) {
            throw new ArgumentNullException("request");
        }
        if (consumer == null) {
            throw new ArgumentNullException("consumer");
        }
        if (request.Input == null) {
            throw new ArgumentException("AITtsRequest.Input is required");
        }
        string modelOverride = request.Model != null && request.Model != "" ? request.Model : "";
        string effectiveId = this.ResolveModelId(modelOverride);
        List<float> allSamples = new List<float>();
        Stopwatch sw = Stopwatch.StartNew();
        try {
            await this.ExecuteStreamAsync(async (runner: IAIModel) => {
                List<string> sentences = AITtsFace.SplitSentences(request.Input, request.MaxChunkChars);
                int total = sentences.Count;
                int index = 0;
                while (index < total) {
                    ct.ThrowIfCancellationRequested();
                    // 循环变量先落本地快照再入闭包（防御闭包按引用捕获循环变量的
                    // 语义不确定性；对齐库内「本地装载后使用」规避惯例）。
                    int chunkIndex = index;
                    bool isFinalChunk = index == total - 1;
                    string chunkText = sentences[index];
                    // 块结果经显式类型局部中转：`await this.ExecuteBlockAsync(...)` 在
                    // lambda 内联时结果类型推断回退 Int，await 提取走 rt_task_result_int
                    // 截断指针；且 `(AITtsChunk)await …` 的 cast 内含 await 同样回退 Int
                    // （lower_arg_operand 物化 i32 临时再 inttoptr 截断）。逐级本地装载
                    // 规避（编译器缺陷规避，对齐库内「本地装载后使用」惯例）。
                    Task<AIModelResult> chunkTask = this.ExecuteBlockAsync(
                        async (r: IAIModel) => {
                            List<Tensor> inputs = AITtsFace.TextInputs(chunkText);
                            List<Tensor> outs = await r.RunAsync(inputs, ct);
                            AITtsChunk c = new AITtsChunk();
                            c.Index = chunkIndex;
                            c.IsFinal = isFinalChunk;
                            c.Text = chunkText;
                            AITtsFace.ReadSamples(outs, c.Samples);
                            return (AIModelResult)c;
                        }, runner, effectiveId, false);
                    AIModelResult chunkResult = await chunkTask;
                    AITtsChunk chunk = (AITtsChunk)chunkResult;
                    int s = 0;
                    while (s < chunk.Samples.Count) {
                        allSamples.Add(chunk.Samples[s]);
                        s = s + 1;
                    }
                    consumer.OnAudioChunk(chunk);
                    index = index + 1;
                }
                // 返回值无语义恒 null（ExecuteStreamAsync 带值签名规避，见其注释）。
                return (AIModelResult)null;
            }, ct, modelOverride);
        } catch (Exception ex) {
                AIModelException error = AIModelService.ToStreamError(ex, effectiveId);
                consumer.OnError(error);
                return;
            }
        AITtsResult summary = new AITtsResult();
        summary.Model = effectiveId;
        summary.Audio = AIAudioInput.FromPcmFloat(allSamples, 16000, 1);
        summary.ResponseFormat = request.ResponseFormat;
        summary.Usage = new AIUsage();
        summary.Usage.DurationMs = sw.ElapsedMilliseconds;
        consumer.OnCompleted(summary);
    }

    /// <summary>语义翻译：文本 → UInt8 [N] UTF-8 字节（P2 最小张量契约镜像）。</summary>
    private static List<Tensor> TextInputs(string text) {
        byte[] bytes = Encoding.GetBytes(text != null ? text : "");
        List<byte> data = new List<byte>();
        int i = 0;
        while (i < bytes.Length) {
            data.Add(bytes[i]);
            i = i + 1;
        }
        List<long> shape = new List<long>();
        shape.Add((long)data.Count);
        List<Tensor> inputs = new List<Tensor>();
        inputs.Add(Tensor.CreateByte(shape, data));
        return inputs;
    }

    /// <summary>语义翻译：Float32 [M] 输出 → PCM 采样（无输出 → 空块）。</summary>
    private static void ReadSamples(List<Tensor> outs, List<float> samples) {
        if (outs == null || outs.Count == 0 || outs[0] == null) {
            return;
        }
        List<float> flat = outs[0].ReadFloat();
        if (flat == null) {
            return;
        }
        int i = 0;
        while (i < flat.Count) {
            samples.Add(flat[i]);
            i = i + 1;
        }
    }

    /// <summary>批式结果（Float32 [M] → Audio；输出采样率元数据 P4 随模型元数据收口）。</summary>
    private static AITtsResult BuildResult(string model, AITtsRequest request, List<Tensor> outs) {
        AITtsResult result = new AITtsResult();
        result.Model = model != null ? model : "";
        result.Audio = new AIAudioInput();
        AITtsFace.ReadSamples(outs, result.Audio.Samples);
        result.ResponseFormat = request.ResponseFormat;
        result.Usage = new AIUsage();
        return result;
    }

    /// <summary>切句（RFC 041 §7.9 通用预处理，非 tokenizer）：标点边界（。！？；.!?;
    /// 与换行）+ 句长上限兜底；空文本返回空表（流式直接空完成）。字符串为 UTF-8
    /// 字节语义（Length/Substring/IndexOf 均字节制），标点含多字节 CJK，逐字节
    /// 比较会漏切——改经 IndexOf 定位最近标点整段切分。</summary>
    private static List<string> SplitSentences(string text, int maxChunkChars) {
        List<string> sentences = new List<string>();
        if (text == null || text.Length == 0) {
            return sentences;
        }
        int limit = maxChunkChars > 0 ? maxChunkChars : 120;
        List<string> delimiters = new List<string>();
        delimiters.Add("。");
        delimiters.Add("！");
        delimiters.Add("？");
        delimiters.Add("；");
        delimiters.Add(".");
        delimiters.Add("!");
        delimiters.Add("?");
        delimiters.Add(";");
        delimiters.Add("\n");
        int len = text.Length;
        int start = 0;
        while (start < len) {
            int cut = -1;
            int cutLen = 0;
            int d = 0;
            while (d < delimiters.Count) {
                int pos = text.IndexOf(delimiters[d], start);
                if (pos >= 0 && (cut < 0 || pos < cut)) {
                    cut = pos;
                    cutLen = delimiters[d].Length;
                }
                d = d + 1;
            }
            int end = len;
            if (cut >= 0) {
                end = cut + cutLen;
            } else if (start + limit < len) {
                end = start + limit;
            }
            if (end - start > limit) {
                end = start + limit;
            }
            sentences.Add(text.Substring(start, end - start));
            start = end;
        }
        return sentences;
    }
}
