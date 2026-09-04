// RFC 037 §11.5 求值模型运行期（M-U2）：AdaptiveEvaluator —— 窗口级求值器。
//
// 运行期求值（§11.5）：
//   窗口级快照变化（resize→tier、切主题、改密度）：
//     一次投影索引计算（整数运算）→ 每 Token 一次内存读 values[idx]  ← 零分配
//     仅 lpx：同刻重算一次缩放系数（clamp(W_vp/1280, 0.5, 2.0) 标量）
//   容器级（finalSize 变化）：
//     仅受影响子树，对容器谓词做整数/浮点比较（唯一连续运行期坐标面）
//
// 确定性（§11.5）：维度独立求值 → 档位区间唯一 → 特异性次序（编译期已折叠
// 进投影表）→ 兜底。同一快照必然同一布局。热路径（Recompute/ResolveToken/
// EvalAdaptive）零分配：仅整数/浮点运算 + 数组读取。

namespace Arc.UI.Adaptive;

using Arc.Collections;

/// <summary>
/// 运行期求值器（窗口级实例；§11.6 每窗口独立快照/投影索引）。
/// </summary>
public class AdaptiveEvaluator {
    private AdaptiveSpec _spec;
    private int _staticIdx;
    private double _lpxScale;
    private double _densityScale;
    private List<double> _containerSize;

    /// <summary>构造求值器（携带编译期投影规格）。</summary>
    /// <param name="spec">窗口级投影规格（codegen 生成）。</param>
    public AdaptiveEvaluator(AdaptiveSpec spec) {
        _spec = spec;
        _staticIdx = 0;
        _lpxScale = 1.0;
        _densityScale = 1.0;
        _containerSize = new List<double>();
        for (int i = 0; i < spec.AdaptiveCount + 1; i++) {
            _containerSize.Add(0.0);
        }
    }

    /// <summary>当前静态投影索引（窗口级缓存；变化时一次重算）。</summary>
    public int StaticIndex { get { return _staticIdx; } }

    /// <summary>当前 lpx 缩放系数（每窗口每快照一次标量，§11.1）。</summary>
    public double LpxScale { get { return _lpxScale; } }

    // ===== 窗口级：快照 → 投影索引 =====

    /// <summary>
    /// 快照变化（resize→tier、切主题、改密度）→ 一次投影索引计算。
    /// 整数运算 + 数组读取，零分配。
    /// </summary>
    /// <param name="s">窗口级环境快照。</param>
    public void Recompute(AdaptiveSnapshot s) {
        _densityScale = s.DensityScale;
        // lpx 系数：clamp(W_vp/1280, 0.5, 2.0)，每窗口每快照一次标量（§11.1）
        double w = s.WindowWidthVp;
        double sc = w / 1280.0;
        if (sc < 0.5) { sc = 0.5; }
        if (sc > 2.0) { sc = 2.0; }
        _lpxScale = sc;

        // 维度坐标 → 引用索引（-1 = 该坐标值未被引用）
        int tierCoord = this.TierCoord(s.WindowWidthVp);
        int idiomCoord = -1;
        if (s.IdiomCode >= 0 && s.IdiomCode < 5) { idiomCoord = _spec.IdiomRef[s.IdiomCode]; }
        int densityCoord = -1;
        if (s.DensityCode >= 0 && s.DensityCode < 3) { densityCoord = _spec.DensityRef[s.DensityCode]; }

        // 索引公式：idx = Σ coord_i × stride_i（整数运算，零分配）
        int idx = 0;
        int mediaDim = 0;
        for (int d = 0; d < _spec.DimCount; d++) {
            int coord;
            int kind = _spec.DimKinds[d];
            if (kind == 0) {
                coord = tierCoord;
            } else if (kind == 1) {
                coord = idiomCoord;
            } else if (kind == 2) {
                coord = densityCoord;
            } else {
                coord = this.MediaRefAt(d, s.MediaValues, mediaDim);
                mediaDim = mediaDim + 1;
            }
            if (coord < 0) { coord = _spec.DimCards[d] - 1; }   // no-match 槽
            idx = idx + coord * _spec.DimStrides[d];
        }
        _staticIdx = idx;
    }

    /// <summary>档位位置（阈值 ≤ W_vp 的个数）→ 维度引用索引（或 -1）。</summary>
    private int TierCoord(double widthVp) {
        if (_spec.TierCount <= 0) { return -1; }
        int pos = 0;
        for (int i = 0; i < _spec.TierCount; i++) {
            if (widthVp >= _spec.TierThresholds[i]) { pos = i + 1; }
        }
        if (pos >= _spec.TierCount) { pos = _spec.TierCount - 1; }
        if (pos < 0) { pos = 0; }
        return _spec.TierRef[pos];
    }

    /// <summary>Media 坐标值 → 引用字面量索引（相等比较；-1 = 未命中）。</summary>
    private int MediaRefAt(int dim, double[] mediaValues, int mediaDim) {
        int count = _spec.MediaValueCount[dim];
        int off = _spec.MediaRefOffset[dim];
        double v = mediaValues[mediaDim];
        for (int i = 0; i < count; i++) {
            if (v == _spec.MediaRefValues[off + i]) { return i; }
        }
        return -1;
    }

    // ===== 容器级：finalSize → 区间 + 子树谓词 =====

    /// <summary>容器 finalSize 变化（0 = 窗口根；adaptiveId+1 = `<Adaptive>` 子树）。</summary>
    public void SetContainerSize(int containerId, double finalSize) {
        if (containerId < 0 || containerId >= _containerSize.Count) { return; }
        _containerSize[containerId] = finalSize;
    }

    /// <summary>读取容器 finalSize。</summary>
    public double GetContainerSize(int containerId) {
        if (containerId < 0 || containerId >= _containerSize.Count) { return 0.0; }
        return _containerSize[containerId];
    }

    /// <summary>断点区间：升序阈值比较（唯一连续运行期坐标面，零分配）。</summary>
    private int IntervalOf(AdaptiveToken t, double size) {
        int n = t.Thresholds.Count;
        int k = 0;
        while (k < n && size >= t.Thresholds[k]) { k = k + 1; }
        return k;
    }

    /// <summary>
    /// 每 Token 一次内存读 values[idx]（静态坐标 × 区间），随后按单位换算。
    /// 窗口级重投影热路径零分配。
    /// </summary>
    /// <param name="tokenId">Token 在规格 Tokens 中的索引。</param>
    /// <param name="containerId">容器上下文（0 = 窗口根；adaptiveId+1）。</param>
    /// <returns>解析后的物理值（px）。</returns>
    public double ResolveToken(int tokenId, int containerId) {
        AdaptiveToken t = _spec.Tokens[tokenId];
        int interval = this.IntervalOf(t, _containerSize[containerId]);
        int idx = _staticIdx * t.IntervalCount + interval;
        double mag = t.Table[idx];
        int u = t.Units[idx];
        double v = mag;
        if (u == 0) {
            v = mag * _densityScale;                        // vp → px
        } else if (u == 1) {
            v = mag;                                        // px
        } else if (u == 2) {
            v = mag * _containerSize[containerId] / 100.0;  // % → avail × pct / 100
        } else {
            v = mag * _densityScale * _lpxScale;            // lpx → vp × density × scale
        }
        return v;
    }

    /// <summary>
    /// `<Adaptive>` 子树谓词（P(container)，§11.5）：静态条件（与快照 AND）+
    /// 断点区间（整数/浮点比较）。仅受影响子树重算。
    /// </summary>
    /// <param name="adaptiveId">`<Adaptive>` 子树索引。</param>
    /// <param name="s">当前快照（静态条件求值用）。</param>
    /// <param name="containerSize">该容器 finalSize。</param>
    /// <returns>子树是否命中（true = 显示）。</returns>
    public bool EvalAdaptive(int adaptiveId, AdaptiveSnapshot s, double containerSize) {
        if (adaptiveId < 0 || adaptiveId >= _spec.AdaptiveCount) { return false; }
        for (int i = 0; i < _spec.AdaptiveCondCount[adaptiveId]; i++) {
            int idx = _spec.AdaptiveCondOffset[adaptiveId] + i;
            int d = _spec.AdaptiveCondDim[idx];
            int vi = _spec.AdaptiveCondValue[idx];
            int coord = this.CoordForDim(d, s);
            if (coord < 0) { coord = _spec.DimCards[d] - 1; }
            if (coord != vi) { return false; }
        }
        double min = _spec.AdaptiveMin[adaptiveId];
        double max = _spec.AdaptiveMax[adaptiveId];
        return (containerSize >= min) && (containerSize < max);
    }

    /// <summary>维度 d 的当前坐标（与 Recompute 同一映射）。</summary>
    private int CoordForDim(int d, AdaptiveSnapshot s) {
        int kind = _spec.DimKinds[d];
        if (kind == 0) { return this.TierCoord(s.WindowWidthVp); }
        if (kind == 1) {
            if (s.IdiomCode >= 0 && s.IdiomCode < 5) { return _spec.IdiomRef[s.IdiomCode]; }
            return -1;
        }
        if (kind == 2) {
            if (s.DensityCode >= 0 && s.DensityCode < 3) { return _spec.DensityRef[s.DensityCode]; }
            return -1;
        }
        int mediaDim = 0;
        for (int i = 0; i < d; i++) {
            if (_spec.DimKinds[i] == 3) { mediaDim = mediaDim + 1; }
        }
        return this.MediaRefAt(d, s.MediaValues, mediaDim);
    }
}
