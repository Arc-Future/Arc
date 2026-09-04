// RFC 037 §3.5 + WPF 精华：Arc.UI.Media — Elevation 深度（软阴影）规格。
//
// 对标 WPF DropShadowEffect / 阴影 token：以结构化字段描述一层软阴影，
// 供「hover 抬升 / focus 辉光 / 浮层投影」等现代深度反馈使用。编译期固化
// （非资源键），渲染器经 DrawSurfaceShadow 落地。
//
// 取 WPF 精华去糟粕：不引入 Effect 抽象层级，仅用值类型承载阴影几何 + 透明度。

namespace Arc.UI.Media;

/// <summary>深度/软阴影规格（blur 越大越软，offsetY 向下偏移，alpha 为整体浓度）。</summary>
public struct Elevation {
    /// <summary>阴影圆角（贴合核心矩形，px）。</summary>
    public double Radius;

    /// <summary>高斯模糊半径（px；0 表示无阴影）。</summary>
    public double Blur;

    /// <summary>纵向偏移（px；正=向下，模拟光源在上）。</summary>
    public double OffsetY;

    /// <summary>阴影浓度（0–1）。</summary>
    public double Alpha;

    public Elevation() {
        this.Radius = 6.0;
        this.Blur = 0.0;
        this.OffsetY = 0.0;
        this.Alpha = 0.0;
    }

    /// <summary>构造带偏移的软阴影。</summary>
    public Elevation(double radius, double blur, double offsetY, double alpha) {
        this.Radius = radius;
        this.Blur = blur;
        this.OffsetY = offsetY;
        this.Alpha = alpha;
    }

    /// <summary>无阴影。</summary>
    public static Elevation None() {
        return new Elevation(6.0, 0.0, 0.0, 0.0);
    }

    /// <summary>是否可见（有模糊且浓度非零）。</summary>
    public bool IsVisible {
        get {
            return this.Blur > 0.0 && this.Alpha > 0.0;
        }
    }
}