// RFC 037 §3 + WPF 精华：Arc.UI.Media — Brush 画刷体系。
//
// 对标 System.Windows.Media.Brush 家族：可继承的抽象画刷层级，承载
// 纯色（SolidColorBrush）与线性渐变（LinearGradientBrush）等，统一挂载
// 透明度（Opacity）。作为主题资源与 DP 的**类型化值**：
//   - 颜色成「家族体系」：同一语义可有纯色/渐变多态，不必各自散落字符串；
//   - 内置键（ResourceDictionary 键）可解析为任意 Brush 派生；
//   - arml `{StaticResource}` 与 as `new SolidColorBrush(Color.Parse(...))`
//     均可编码，渲染器按 Brush 解析上屏。
//
// 渲染桥接：本图层定义体系与解析；渲染器经 DrawLinearGradient 消费
// LinearGradientBrush（两停靠点线性渐变），无渐变键时回退纯色。

namespace Arc.UI.Media;

/// <summary>画刷基类（透明度 + 解析到纯色 hex 的虚方法）。</summary>
public class Brush {
    /// <summary>整体透明度（0–1，与颜色 A 相乘）。</summary>
    public double Opacity;

    public Brush() {
        this.Opacity = 1.0;
    }

    /// <summary>
    /// 解析画刷到可渲染的纯色 hex（AARRGGBB）。渐变取首停靠色 × Opacity。
    /// 渲染器以纯色上屏；渐变的真实多色渲染为后续里程碑。
    /// </summary>
    public virtual string ToHex() {
        return "#00000000";
    }

    /// <summary>统一解析入口：hex 字符串/命名色 → SolidColorBrush。</summary>
    public static Brush FromString(string value) {
        return new SolidColorBrush(Color.Parse(value));
    }
}

/// <summary>纯色画刷。</summary>
public class SolidColorBrush : Brush {
    /// <summary>颜色（RGBA 0–1）。</summary>
    public Color Color;

    public SolidColorBrush() {
        this.Color = Color.Transparent();
    }

    public SolidColorBrush(Color color) {
        this.Color = color;
    }

    public override string ToHex() {
        // Brush.Opacity 语义（文档：「与颜色 A 相乘」）——不能用 WithOpacity
        // （覆盖 A：透明色 A=0 被 Opacity=1 顶成不透明，ToHex 得 #FF000000）。
        return Color.FromRgba(this.Color.R, this.Color.G, this.Color.B,
            this.Color.A * this.Opacity).ToHex();
    }
}

/// <summary>渐变停靠点（颜色 + 位置 0–1）。</summary>
public class GradientStop {
    /// <summary>停靠颜色。</summary>
    public Color Color;

    /// <summary>停靠位置（0=起点，1=终点）。</summary>
    public double Offset;

    public GradientStop() {
        this.Color = Color.Transparent();
        this.Offset = 0.0;
    }

    public GradientStop(Color color, double offset) {
        this.Color = color;
        this.Offset = offset;
    }
}

/// <summary>线性渐变画刷（沿 StartPoint→EndPoint 插值停靠点）。</summary>
public class LinearGradientBrush : Brush {
    /// <summary>停靠点序列（按 Offset 升序）。</summary>
    public List<GradientStop> Stops;

    /// <summary>渐变起点（0–1 归一化坐标）。</summary>
    public double StartX;

    /// <summary>渐变起点 Y。</summary>
    public double StartY;

    /// <summary>渐变终点 X。</summary>
    public double EndX;

    /// <summary>渐变终点 Y。</summary>
    public double EndY;

    public LinearGradientBrush() {
        this.Stops = new List<GradientStop>();
        this.StartX = 0.0;
        this.StartY = 0.0;
        this.EndX = 1.0;
        this.EndY = 0.0;
    }

    /// <summary>便捷构造（两停靠点，左→右）。</summary>
    public static LinearGradientBrush Horizontal(Color src, Color to) {
        LinearGradientBrush b = new LinearGradientBrush();
        b.Stops.Add(new GradientStop(src, 0.0));
        b.Stops.Add(new GradientStop(to, 1.0));
        return b;
    }

    public override string ToHex() {
        if (this.Stops.Count > 0) {
            // 纯色回退路径取首停靠色（真实渐变经 DrawLinearGradient 消费）。
            return new SolidColorBrush(this.Stops[0].Color).ToHex();
        }
        return "#00000000";
    }
}