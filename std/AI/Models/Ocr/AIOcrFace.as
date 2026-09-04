// RFC 041 §7.5：AIOcrFace — OCR 域子面（统一门面的域方法落点）。
//
// 轻量只读面：经 AIModelService 统一骨架（Acquire → 执行 → 释放 → 统计）从注册表取
// 句柄执行；后端未加载（工厂抛 AIModelNotAvailableException）原样传播。语义翻译
// （值类型 ↔ Tensor）在服务内部——P2 最小张量契约：输入 UInt8 [H,W,C] 像素，输出
// UInt8 [N] UTF-8 字节（全文）；行级分段为 P4 细化。
//
// 诚实标注（实战差距审查 P0-1）：本门面只提供「管道 + 调用面」，不含真实识别能力。
// 真实 OCR 识别需用户自备 ONNX 模型 + 前后处理（图像预处理 / CTC 或段级解码）；
// 框架当前不内置任何预训练模型与 tokenizer/解码器——未注入真实后端即「不可识别」。
// 不得把「仅测试 fixture 能跑通管道」当作「已具备识别能力」对外宣称。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;
using Arc.Collections;
using Arc.Text;

/// <summary>OCR 域子面（RFC 041 §7.5）：RecognizeAsync / RecognizeBatchAsync。</summary>
public class AIOcrFace : AIModelService {
    /// <summary>由统一门面 AIModels 构造（包内）。</summary>
    internal AIOcrFace(AIModelRegistry registry, string modelId, AIModelServiceOptions options)
        : base(registry, modelId, options) {
    }

    /// <summary>单图识别（RFC 041 §7.5 主方法；request.Model 非空覆盖绑定默认模型）。</summary>
    public async Task<AIOcrResult> RecognizeAsync(AIOcrRequest request, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (request == null) {
            throw new ArgumentNullException("request");
        }
        if (request.Input == null) {
            throw new ArgumentException("AIOcrRequest.Input is required");
        }
        // 逐请求模型覆盖（RFC 041 §7.5）：显式 Model 非空 → 覆盖构造绑定默认；
        // 结果 Model 回显实际执行模型（覆盖值或绑定默认）。
        string modelOverride = request.Model != null && request.Model != "" ? request.Model : "";
        string effectiveModel = this.ResolveModelId(modelOverride);
        AIModelResult result = await this.ExecuteAsync(async (runner: IAIModel) => {
            List<Tensor> inputs = request.Input.ToInputs();
            List<Tensor> outs = await runner.RunAsync(inputs, ct);
            return (AIModelResult)AIOcrFace.BuildResult(effectiveModel, outs);
        }, ct, modelOverride);
        return (AIOcrResult)result;
    }

    /// <summary>批量识别（本地循环 + 按条进度 + 取消；对齐 §7.5 统一批量契约）。</summary>
    public async Task<List<AIOcrResult>> RecognizeBatchAsync(
        List<AIOcrRequest> requests, Action<AIModelProgress>? progress, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        List<AIOcrResult> results = new List<AIOcrResult>();
        if (requests == null || requests.Count == 0) {
            return results;
        }
        int total = requests.Count;
        int i = 0;
        while (i < total) {
            ct.ThrowIfCancellationRequested();
            AIOcrResult r = await this.RecognizeAsync(requests[i], ct);
            results.Add(r);
            if (progress != null) {
                AIModelProgress p = new AIModelProgress();
                p.Current = i + 1;
                p.Total = total;
                p.Stage = "ocr";
                // 本地装载后调用：可空 Action 形参直接调用会退化为对未定义符号的
                // 直接调用（编译器缺陷，对齐 Arc/Types/Lazy.as 规避先例）。
                Action<AIModelProgress> report = progress;
                report(p);
            }
            i = i + 1;
        }
        return results;
    }

    /// <summary>语义翻译：输出 UInt8 字节 → 全文（行级分段 P4）。</summary>
    private static AIOcrResult BuildResult(string model, List<Tensor> outs) {
        AIOcrResult result = new AIOcrResult();
        result.Model = model != null ? model : "";
        result.Text = AIOcrFace.DecodeText(outs);
        result.Lines = new List<AIOcrLine>();
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
