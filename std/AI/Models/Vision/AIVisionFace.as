// RFC 041 §7.5：AIVisionFace — 多模态理解域子面（后续 P4 · 未接线）。
//
// 请求/响应模型已定型（AIUnderstandRequest/AIUnderstandResult + AIUnderstandPart
// content parts，对齐 /v1/chat/completions 多模态）；执行属 P4 生态，本切片以
// AIModelNotAvailableException 显式标注「后续」，非空桩冒充实现。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;

/// <summary>多模态理解域子面（RFC 041 §7.5；后续 P4）。当前抛 <see cref="AIModelNotAvailableException"/>。</summary>
public class AIVisionFace {
    /// <summary>由统一门面 AIModels 构造（包内）。</summary>
    internal AIVisionFace() {
    }

    /// <summary>多模态理解（后续 P4）：AIUnderstandRequest → AIUnderstandResult（图片问答）。</summary>
    public Task<AIUnderstandResult> UnderstandAsync(AIUnderstandRequest request, CancellationToken ct) {
        throw new AIModelNotAvailableException("多模态理解域后续（P4）落地，尚未接线");
    }
}
