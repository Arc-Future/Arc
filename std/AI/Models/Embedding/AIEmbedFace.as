// RFC 041 §7.5：AIEmbedFace — 嵌入域子面（统一门面的域方法落点）。
//
// 轻量只读面：经 AIModelService 统一骨架执行；后端未加载（AIModelNotAvailableException）
// 原样传播。P2 最小张量契约：输入 Int64 [N, maxLen] UTF-8 字节直通编码（tokenizer P4），
// 输出 Float32 [N, dim] → Data[i].Vector 对齐 Input 位置。向量本体由用户侧落库。
//
// 诚实标注（实战差距审查 P0-1）：本门面只提供「管道 + 调用面」，不含真实嵌入能力。
// 真实嵌入需用户自备 ONNX 模型 + 前后处理（tokenizer；当前输入为 UTF-8 字节直通编码）；
// 框架当前不内置任何预训练模型与 tokenizer——未注入真实后端即「不可生成有效向量」。
// 不得把「仅测试 fixture 能跑通管道」当作「已具备嵌入能力」对外宣称。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;
using Arc.Collections;
using Arc.Text;

/// <summary>嵌入域子面（RFC 041 §7.5）：EmbedAsync（Input[] 即批量）/ EmbedOneAsync。</summary>
public class AIEmbedFace : AIModelService {
    /// <summary>由统一门面 AIModels 构造（包内）。</summary>
    internal AIEmbedFace(AIModelRegistry registry, string modelId, AIModelServiceOptions options)
        : base(registry, modelId, options) {
    }

    /// <summary>批量嵌入（Input[] 条数决定批大小；Data[i].Index 对齐 Input 位置；
    /// request.Model 非空覆盖绑定默认模型）。</summary>
    public async Task<AIEmbedResult> EmbedAsync(AIEmbedRequest request, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (request == null) {
            throw new ArgumentNullException("request");
        }
        int n = request.Input != null ? request.Input.Count : 0;
        // 逐请求模型覆盖（RFC 041 §7.5）：显式 Model 非空 → 覆盖构造绑定默认；
        // 结果 Model 回显实际执行模型（覆盖值或绑定默认）。
        string modelOverride = request.Model != null && request.Model != "" ? request.Model : "";
        string effectiveModel = this.ResolveModelId(modelOverride);
        AIModelResult result = await this.ExecuteAsync(async (runner: IAIModel) => {
            List<Tensor> inputs = AIEmbedFace.BuildInputs(request.Input);
            List<Tensor> outs = await runner.RunAsync(inputs, ct);
            return (AIModelResult)AIEmbedFace.BuildResult(effectiveModel, request.Input.Count, outs);
        }, ct, modelOverride);
        return (AIEmbedResult)result;
    }

    /// <summary>单条嵌入（EmbedAsync 的退化情形）。</summary>
    public async Task<AIEmbedResult> EmbedOneAsync(string text, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        AIEmbedRequest request = new AIEmbedRequest();
        request.Input.Add(text != null ? text : "");
        return await this.EmbedAsync(request, ct);
    }

    /// <summary>语义翻译：Input[] → Int64 [N, maxLen] UTF-8 字节 token（零填充，tokenizer P4）。</summary>
    private static List<Tensor> BuildInputs(List<string> inputs) {
        List<Tensor> tensors = new List<Tensor>();
        if (inputs == null || inputs.Count == 0) {
            return tensors;
        }
        int n = inputs.Count;
        int maxLen = 1;
        int i = 0;
        while (i < n) {
            string s = inputs[i];
            int len = s != null ? Encoding.GetByteCount(s) : 0;
            if (len > maxLen) {
                maxLen = len;
            }
            i = i + 1;
        }
        List<long> ids = new List<long>();
        int row = 0;
        while (row < n) {
            byte[] bytes = Encoding.GetBytes(inputs[row] != null ? inputs[row] : "");
            int col = 0;
            while (col < maxLen) {
                if (col < bytes.Length) {
                    ids.Add((long)bytes[col]);
                } else {
                    ids.Add((long)0);
                }
                col = col + 1;
            }
            row = row + 1;
        }
        List<long> shape = new List<long>();
        shape.Add((long)n);
        shape.Add((long)maxLen);
        tensors.Add(Tensor.CreateInt64(shape, ids));
        return tensors;
    }

    /// <summary>语义翻译：Float32 [N, dim] → Data[i].Vector（行主序展平按行切分）。</summary>
    private static AIEmbedResult BuildResult(string model, int n, List<Tensor> outs) {
        AIEmbedResult result = new AIEmbedResult();
        result.Model = model != null ? model : "";
        result.Usage = new AIUsage();
        if (outs == null || outs.Count == 0 || outs[0] == null) {
            return result;
        }
        List<float> flat = outs[0].ReadFloat();
        if (flat == null || n <= 0 || flat.Count == 0) {
            return result;
        }
        int dim = flat.Count / n;
        int i = 0;
        while (i < n) {
            AIEmbeddingData data = new AIEmbeddingData();
            data.Index = i;
            int j = 0;
            while (j < dim) {
                int idx = i * dim + j;
                if (idx < flat.Count) {
                    data.Vector.Values.Add(flat[idx]);
                }
                j = j + 1;
            }
            result.Data.Add(data);
            i = i + 1;
        }
        return result;
    }
}
