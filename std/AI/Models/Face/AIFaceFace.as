// RFC 041 §7.5：AIFaceFace — 人脸域子面（后续 P4 · 未接线）。
//
// 命名对齐七子面统一体系 AI{域}Face（§7.5 门面草图与域子面表硬性规定 AIFaceFace）：
// 「Face」为子面术语（门面调用面后缀），人脸域请求/响应类型以 AIFaceDetect* 前缀消歧，
// 不因单点撞车破坏七子面命名一致性。检测/嵌入执行属 P4 生态；本切片以
// AIModelNotAvailableException 显式标注「后续」，非空桩冒充实现。
// 身份判定在应用层（§7.5 关键裁决）。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;

/// <summary>人脸域子面（RFC 041 §7.5；后续 P4）。当前抛 <see cref="AIModelNotAvailableException"/>。</summary>
public class AIFaceFace {
    /// <summary>由统一门面 AIModels 构造（包内）。</summary>
    internal AIFaceFace() {
    }

    /// <summary>人脸检测（后续 P4）：AIFaceDetectRequest → AIFaceDetectResult（框/关键点/置信度/可选嵌入）。</summary>
    public Task<AIFaceDetectResult> DetectAsync(AIFaceDetectRequest request, CancellationToken ct) {
        throw new AIModelNotAvailableException("人脸域后续（P4）落地，尚未接线");
    }
}
