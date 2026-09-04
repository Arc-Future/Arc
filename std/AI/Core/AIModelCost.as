// AIModelCost — 调用成本档位（RFC 041 §7.3 服务选项）。
//
// 供 Agent 回合调度（038 §14 回合预算护栏）按成本档位分级——Fast/Medium/Slow。
// 仅分类标签，不承载具体计量。
namespace Arc.AI;

/// <summary>模型调用成本档位（RFC 041 §7.3，Agent 回合调度用）。</summary>
public enum AIModelCost {
    /// <summary>低成本档（如嵌入 / 快速 OCR）。</summary>
    Fast,

    /// <summary>中等成本档。</summary>
    Medium,

    /// <summary>高成本档（如 TTS / 长转写）。</summary>
    Slow,
}
