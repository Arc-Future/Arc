// RFC 037 §11.5 求值模型运行期（M-U2）：AdaptiveToken —— 单 Token 投影表数据。
//
// 每个 Token 的投影表 = 扁平表（静态坐标 × 断点区间），由 `arc ui codegen`
// 编译期生成（crates/arc-ui/src/projection.rs）。运行期求值 = 一次索引计算
// + 一次表读 `Table[idx]`（零分配，§11.5）。
//
// 单位码（§11.1）：0=vp 1=px 2=% 3=lpx。表项存「声明单位下的数值幅度」，
// 运行期换算（`px = vp × density`、`% = avail × pct / 100`、
// `lpx = 1 vp × clamp(W_vp/1280, 0.5, 2.0)`）。

namespace Arc.UI.Adaptive;

using Arc.Collections;

/// <summary>
/// 单个 Token 的投影表数据（§11.5；`arc ui codegen` 编译期生成）。
/// </summary>
public class AdaptiveToken {
    /// <summary>断点区间数（`Thresholds.Count + 1`；无断点 = 1）。</summary>
    public int IntervalCount;

    /// <summary>升序断点阈值（长度 = IntervalCount - 1）。</summary>
    public List<double> Thresholds;

    /// <summary>扁平值表：长度 = NumStates × IntervalCount（静态坐标 × 区间）。</summary>
    public List<double> Table;

    /// <summary>每项单位码：0=vp 1=px 2=% 3=lpx（§11.1）。</summary>
    public List<int> Units;

    /// <summary>构造空 Token（由 codegen 逐字段填充）。</summary>
    public AdaptiveToken() {
        IntervalCount = 1;
        Thresholds = new List<double>();
        Table = new List<double>();
        Units = new List<int>();
    }
}
