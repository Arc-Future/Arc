// RFC 037 D2.1 + RFC 037 §8 修订（text-editing.md §2）：TextBlock 文本显示元素。
//
// TextBlock 是只读文本显示元素：Block=只读 / Box=可编辑（TextBox）对仗；
// 与属性名 Text 解撞（`<TextBlock Text="…">` 可读性）。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → TextBlock（直接派生，不经过 Control）
//   Arc:  Control → TextBlock（简化归到 Control 层以共享字体 DP——RFC 051 D1）
//
// **字体属性处理（RFC 051 D1 WPF 同构）**：
//   - FontFamily/FontSize/FontWeight/Foreground 由 Control 声明（环境属性，
//     沿树继承）——TextBlock 不重复声明，使用继承版本
//   - TextBlock 保留特有 DP：Text（string）
//
// RFC 037 D1 WPF 同构编程模型：每个公共属性由两套驱动——
//   1. 静态 DependencyProperty<T> 元数据（RegisterProperty 工厂创建）
//   2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>

namespace Arc.UI.Components;

using Arc.UI.Layout;

/// <summary>
/// 只读文本显示元素（Text DP；字体属性继承自 Control 环境属性）。
/// </summary>
public class TextBlock : Control {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public TextBlock() {
        this.Type = typeof(TextBlock);
    }

    // ===== 静态依赖属性元数据（RFC 051 D1 WPF 同构）=====

    /// <summary>
    /// Text dependency property metadata.
    /// </summary>
    public static DependencyProperty<string> TextProperty =
        RegisterProperty<string>(nameof(Text), typeof(TextBlock), "");

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>
    /// Displayed text content.
    /// </summary>
    public string Text {
        get { return this.GetValue<string>(TextProperty); }
        set { this.SetValue<string>(TextProperty, value); }
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        LayoutSize est = LayoutHelper.EstimateTextSize(
            this.Text, this.FontSize,
            LayoutHelper.MinTextPaddingX, LayoutHelper.MinTextPaddingY,
            this.FontFamily, this.FontWeight);
        double w = est.Width;
        double h = est.Height;
        double availW = availableSize.Width;

        // 只有当约束是有界的（大于 0 且非无限）时才裁剪
        bool isBoundedW = availW > 0.0 && availW < 1000000000.0;
        if (isBoundedW && w > availW) {
            w = availW;
        }
        if (this.Width > 0.0) {
            w = this.Width;
        }
        if (this.Height > 0.0) {
            h = this.Height;
        }
        return new LayoutSize(w, h);
    }
}
