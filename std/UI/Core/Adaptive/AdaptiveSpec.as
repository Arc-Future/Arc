// RFC 037 §11.5 求值模型运行期（M-U2）：AdaptiveSpec —— 窗口级投影规格。
//
// 静态坐标编组（§11.5）：有效状态空间 = ∏ 维度基数（末位 = 「无引用值命中」
// 槽，保证任何快照坐标映射到合法下标）。索引公式
// `idx = Σ coord_i × stride_i`（整数运算），窗口级缓存、变化时一次重算。
//
// 维度编码（DimKinds）：0=Tier 1=Idiom 2=Density 3=Media。规范序
// Idiom（Desktop/Mobile/Tablet/TV/Watch）与 Density（compact/comfortable/cozy）
// 的「规范码 → 维度引用索引」映射由 IdiomRef/DensityRef 承载；Tier 由
// TierThresholds + TierRef 承载（档位位置 → 引用索引）；Media 坐标由
// MediaRefValues/MediaValueCount/MediaRefOffset 承载（引用字面量 → 索引）。
//
// 本规格由 `arc ui codegen` 编译期生成（crates/arc-ui/src/projection.rs）。

namespace Arc.UI.Adaptive;

using Arc.Collections;

/// <summary>
/// 窗口级投影规格（§11.5 编译期产物；运行期求值器消费）。
/// </summary>
public class AdaptiveSpec {
    /// <summary>有效状态数 = ∏ 维度基数。</summary>
    public int NumStates;

    /// <summary>静态维度个数（仅实际用到的；死组合已剔除）。</summary>
    public int DimCount;

    /// <summary>每维基数（= 引用值数 + 1，末位 no-match 槽）。</summary>
    public List<int> DimCards;

    /// <summary>每维索引步长（索引公式 `idx = Σ coord_i × stride_i`）。</summary>
    public List<int> DimStrides;

    /// <summary>每维种类：0=Tier 1=Idiom 2=Density 3=Media。</summary>
    public List<int> DimKinds;

    /// <summary>规范 Idiom 码（0..4）→ 维度引用索引（或 -1）。</summary>
    public List<int> IdiomRef;

    /// <summary>规范 Density 码（0..2）→ 维度引用索引（或 -1）。</summary>
    public List<int> DensityRef;

    /// <summary>档位数（升序阈值长度）。</summary>
    public int TierCount;

    /// <summary>升序档位阈值（档位位置 = 阈值 ≤ W_vp 的个数）。</summary>
    public List<double> TierThresholds;

    /// <summary>档位位置 → 维度引用索引（或 -1）。</summary>
    public List<int> TierRef;

    /// <summary>每维（按 DimIndex）Media 引用值个数；非 Media 维为 0。</summary>
    public List<int> MediaValueCount;

    /// <summary>每维（按 DimIndex）Media 引用值在 MediaRefValues 中的偏移。</summary>
    public List<int> MediaRefOffset;

    /// <summary>展平的 Media 引用字面量值（每 Media 维首见序）。</summary>
    public List<double> MediaRefValues;

    /// <summary>Token 投影表（定义序）。</summary>
    public List<AdaptiveToken> Tokens;

    /// <summary>`<Adaptive>` 子树数。</summary>
    public int AdaptiveCount;

    /// <summary>每子树 MinWidth（缺省 0.0）。</summary>
    public List<double> AdaptiveMin;

    /// <summary>每子树 MaxWidth（缺省 +inf）。</summary>
    public List<double> AdaptiveMax;

    /// <summary>每子树静态条件在 AdaptiveCondDim/Value 中的偏移。</summary>
    public List<int> AdaptiveCondOffset;

    /// <summary>每子树静态条件数。</summary>
    public List<int> AdaptiveCondCount;

    /// <summary>展平静态条件维度索引。</summary>
    public List<int> AdaptiveCondDim;

    /// <summary>展平静态条件维度值索引。</summary>
    public List<int> AdaptiveCondValue;

    /// <summary>构造空规格（由 codegen 逐字段填充）。</summary>
    public AdaptiveSpec() {
        this.NumStates = 1;
        this.DimCount = 0;
        this.DimCards = new List<int>();
        this.DimStrides = new List<int>();
        this.DimKinds = new List<int>();
        this.IdiomRef = new List<int>();
        this.DensityRef = new List<int>();
        this.TierCount = 0;
        this.TierThresholds = new List<double>();
        this.TierRef = new List<int>();
        this.MediaValueCount = new List<int>();
        this.MediaRefOffset = new List<int>();
        this.MediaRefValues = new List<double>();
        this.Tokens = new List<AdaptiveToken>();
        this.AdaptiveCount = 0;
        this.AdaptiveMin = new List<double>();
        this.AdaptiveMax = new List<double>();
        this.AdaptiveCondOffset = new List<int>();
        this.AdaptiveCondCount = new List<int>();
        this.AdaptiveCondDim = new List<int>();
        this.AdaptiveCondValue = new List<int>();
    }
}
