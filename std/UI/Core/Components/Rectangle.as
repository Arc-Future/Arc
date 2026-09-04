// RFC 037 D2.1 + RFC 037 D1: Arc.UI.Components —— Rectangle 基础图形元素。
//
// Rectangle 是矩形图形元素，承载圆角等视觉属性。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Shape → Rectangle
//   Arc:  FrameworkElement → Shape → Rectangle
//
// **冲突处理（RFC 051 D1 WPF 同构）**：
//   - Width/Height 已由 FrameworkElement 声明——Rectangle 不重复声明，使用继承版本
//   - Fill/Stroke/StrokeThickness 已由 Shape 声明——Rectangle 不重复声明，使用继承版本
//   - Rectangle 保留特有 DP：RadiusX/RadiusY（圆角矩形半径）
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。
//
// 颜色字段说明：
//   - Fill/Stroke 当前为 string 类型（如 "#FF0000"），与 Window.as 中的
//     Title 等 string 属性一致——使用 DependencyProperty<string> 承载。
//   - M3+ 渲染层引入 Brush 类型后，可升级为 DependencyProperty<Brush>，
//     string 到 Brush 的转换由 .arml parser 在解析阶段完成。
//
// 编码模型要点：
//   - nameof(属性) 替代字符串字面量——IDE 重构可自动追踪符号引用
//   - typeof(类) 替代字符串字面量——避免魔法字符串与重构不同步

namespace Arc.UI.Components;

    /// <summary>矩形基础图形元素。Width/Height/Fill/Stroke/StrokeThickness 由基类继承；本类仅声明 RadiusX/RadiusY DP。</summary>
public class Rectangle : Shape {
    // ===== 静态依赖属性元数据（RFC 051 D1 WPF 同构）=====

    /// <summary>RadiusX 属性元数据——圆角水平半径，默认 0.0（直角矩形）。</summary>
    public static DependencyProperty<double> RadiusXProperty =
        RegisterProperty<double>(nameof(RadiusX), typeof(Rectangle), 0.0);

    /// <summary>RadiusY 属性元数据——圆角垂直半径，默认 0.0（直角矩形）。</summary>
    public static DependencyProperty<double> RadiusYProperty =
        RegisterProperty<double>(nameof(RadiusY), typeof(Rectangle), 0.0);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====
    //
    // **Width/Height/Fill/Stroke/StrokeThickness 属性继承自基类**——派生类
    // 无需重新声明 wrapper，直接使用 this.Width / this.Fill 等即可访问基类 wrapper。

    /// <summary>圆角水平半径（像素，0 表示直角）。</summary>
    public double RadiusX {
        get { return this.GetValue<double>(RadiusXProperty); }
        set { this.SetValue<double>(RadiusXProperty, value); }
    }

    /// <summary>圆角垂直半径（像素，0 表示直角）。</summary>
    public double RadiusY {
        get { return this.GetValue<double>(RadiusYProperty); }
        set { this.SetValue<double>(RadiusYProperty, value); }
    }
}
