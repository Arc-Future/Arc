// RFC 037 §11.5 求值模型运行期（M-U2）：AdaptiveSnapshot —— 窗口级环境快照。
//
// 快照 S = (idiom, tier, media-vector, density, container-size)。前四项离散、
// 末项连续（§11.5）；另含连续坐标 W_vp（仅 lpx 系数用，§11.1）。
//
// 坐标以**数值码**承载（§16 非目标 1：运行期零字符串解析）：Idiom 与 Density
// 为规范枚举码，Tier 由 W_vp 经档位阈值推导，Media 坐标为每维一个 double
// （命名坐标 1.0/0.0；参数化坐标 = 实际值）。

namespace Arc.UI.Adaptive;

/// <summary>
/// 窗口级环境快照（§11.5）。每窗口独立实例（§11.6 多窗口求值器窗口级实例化）。
/// </summary>
public class AdaptiveSnapshot {
    /// <summary>规范 Idiom 码：0=Desktop 1=Mobile 2=Tablet 3=TV 4=Watch；-1 = 未设置。</summary>
    public int IdiomCode;

    /// <summary>规范 Density 码：0=compact 1=comfortable 2=cozy；-1 = 未设置。</summary>
    public int DensityCode;

    /// <summary>窗宽（vp）；Tier 档位推导 + lpx 系数（§11.1/§11.5）。</summary>
    public double WindowWidthVp;

    /// <summary>密度换算系数（px-per-vp）；`px = vp × density`（§11.1）。</summary>
    public double DensityScale;

    /// <summary>每 Media 维坐标值（命名坐标 1.0/0.0；参数化坐标实际值）。</summary>
    public double[] MediaValues;

    /// <summary>构造快照。</summary>
    /// <param name="idiomCode">规范 Idiom 码（0..4 或 -1）。</param>
    /// <param name="densityCode">规范 Density 码（0..2 或 -1）。</param>
    /// <param name="widthVp">窗宽（vp）。</param>
    /// <param name="densityScale">密度换算系数。</param>
    /// <param name="mediaValues">每 Media 维坐标值（长度 = 规格 Media 维数）。</param>
    public AdaptiveSnapshot(int idiomCode, int densityCode, double widthVp, double densityScale, double[] mediaValues) {
        IdiomCode = idiomCode;
        DensityCode = densityCode;
        WindowWidthVp = widthVp;
        DensityScale = densityScale;
        MediaValues = mediaValues;
    }
}
