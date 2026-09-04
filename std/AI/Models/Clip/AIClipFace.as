// RFC 041 §7.5：AIClipFace — CLIP 域子面（后续 P4 · 未接线）。
//
// 域子面类型已就位（统一门面唯一入口的 7 子面之一）；请求/响应模型已定型
// （AIClipMatchRequest/AIClipMatchResult，嵌入+余弦惯例），嵌入（EmbedImageAsync/
// EmbedTextAsync）与张量契约属 P4 生态，本切片以 AIModelNotAvailableException 显式
// 标注「后续」，非空桩冒充实现。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;

/// <summary>CLIP 域子面（RFC 041 §7.5；后续 P4）。当前抛 <see cref="AIModelNotAvailableException"/>。</summary>
public class AIClipFace {
    /// <summary>由统一门面 AIModels 构造（包内）。</summary>
    internal AIClipFace() {
    }

    /// <summary>图文匹配（后续 P4）：AIClipMatchRequest → AIClipMatchResult。零样本分类：查询图像 ↔ 候选文本相似度。</summary>
    public Task<AIClipMatchResult> MatchAsync(AIClipMatchRequest request, CancellationToken ct) {
        throw new AIModelNotAvailableException("CLIP 域后续（P4）落地，尚未接线");
    }
}
