// Arc.UI.Styling — StyleEvaluator 样式评估引擎。
//
// **动态 DP 解析（替代硬编码属性 switch）**：
//   Setter.Property 是属性名（对标 WPF XAML 的字符串 Property 解析）。评估器
//   不再硬编码「属性名 → 固定 DependencyProperty」映射，而是委托目标元素
//   `Element.ResolveProperty(name)` 解析出目标自身类型作用域内的
//   DependencyProperty——同一属性名（如 Text/Stretch）自动落到目标自己的 DP
//   （TextBlock→TextBlock.TextProperty、TextBox→TextBox.TextProperty），新增控件的任意 DP
//   无需改动评估器即可被样式命中。类型链未命中时回退全局 owner 表按注册序
//   按名解析（mock / TypeName 标识元素，见 Element.ResolveProperty 两阶段语义）。
//
//   唯一的硬编码收敛为「值种类分派」（double/int/string/bool/枚举），由
//   DependencyProperty&lt;T&gt; 的运行时实例类型驱动（`is` 检查），而非属性名。
//   枚举（Orientation/Stretch/…）经 UIEnumConverter 解析，属有界集合。
//
//   未命中（目标类型链上无此属性）或值种类不支持 → 静默跳过，等价旧 switch 的
//   default 分支，不破坏既有行为。

namespace Arc.UI.Styling;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Components.Layout;
using Arc.UI.Internal;
using Arc.UI.Media;

/// <summary>样式评估引擎。</summary>
public class StyleEvaluator {
    /// <summary>将样式的所有 Setters 应用到元素。</summary>
    public void ApplySetters(Element element, Style style, ResourceDictionary resources) {
        if (element == null || style == null || style.Setters == null) {
            return;
        }
        foreach (var s in style.Setters) {
            if (s != null && s.Property != null) {
                this.ApplyOne(element, s, resources);
            }
        }
    }

    private void ApplyOne(Element element, Setter s, ResourceDictionary resources) {
        // 引用载荷先行分派（载荷字段驱动，值规则同 variant case）：ControlTemplate
        // 载荷走 Control 源组件定义的 Template DP（wrapper 属性通道写入）并套用
        // 视觉树。属性集合仍由 DP 注册表决定（ResolveProperty 动态解析），
        // 零属性名感知。
        if (s.TemplateValue != null) {
            object templateDp = element.ResolveProperty(s.Property);
            if (templateDp != null && element is Control) {
                Control host = (Control)element;
                host.Template = s.TemplateValue;
                s.TemplateValue.ApplyTo(host);
            }
            return;
        }
        // 动态 DP 解析：沿目标元素类型链按名命中其自身类型作用域内的 DP（object 擦除视图）。
        object dp = element.ResolveProperty(s.Property);
        if (dp == null) {
            return;
        }
        // 静态引用轨：{StaticResource key} 应用期按当前活动主题解析落值（键编译期
        // 确定，值来自编译期扁平主题字典；主题即资源，经 MergedDictionaries 并入
        // 解析链）。切主题刷新由 Application.SwitchTheme 重新应用隐式样式完成。
        SetterValue value = SetterValueHelper.ResolveStatic(s.Value, resources);
        this.ApplyDp(element, dp, value);
    }

    /// <summary>
    /// 值种类分派：按 DependencyProperty&lt;T&gt; 的运行时实例类型应用 Setter 值。
    /// 有界集合：标量（double/int/string/bool）、枚举（经 UIEnumConverter）；
    /// 引用载荷（ControlTemplate）由 ApplyOne 前置分派。支持哪些属性由源组件
    /// DP 注册表决定（ResolveProperty），本方法零属性名感知。
    /// </summary>
    private void ApplyDp(Element element, object dp, SetterValue value) {
        if (dp is DependencyProperty<double>) {
            element.SetStyleValue<double>((DependencyProperty<double>)dp,
                SetterValueHelper.NumberOrZero(value));
            return;
        }
        if (dp is DependencyProperty<int>) {
            element.SetStyleValue<int>((DependencyProperty<int>)dp,
                (int)SetterValueHelper.NumberOrZero(value));
            return;
        }
        if (dp is DependencyProperty<string>) {
            element.SetStyleValue<string>((DependencyProperty<string>)dp,
                SetterValueHelper.StringOrEmpty(value));
            return;
        }
        if (dp is DependencyProperty<Brush>) {
            element.SetStyleValue<Brush>((DependencyProperty<Brush>)dp,
                Brush.FromString(SetterValueHelper.StringOrEmpty(value)));
            return;
        }
        if (dp is DependencyProperty<bool>) {
            element.SetStyleValue<bool>((DependencyProperty<bool>)dp,
                SetterValueHelper.BooleanOrFalse(value));
            return;
        }
        if (dp is DependencyProperty<Orientation>) {
            element.SetStyleValue<Orientation>((DependencyProperty<Orientation>)dp,
                UIEnumConverter.ParseOrientation(SetterValueHelper.StringOrEmpty(value)));
            return;
        }
        if (dp is DependencyProperty<Stretch>) {
            element.SetStyleValue<Stretch>((DependencyProperty<Stretch>)dp,
                UIEnumConverter.ParseStretch(SetterValueHelper.StringOrEmpty(value)));
            return;
        }
        if (dp is DependencyProperty<HorizontalAlignment>) {
            element.SetStyleValue<HorizontalAlignment>((DependencyProperty<HorizontalAlignment>)dp,
                UIEnumConverter.ParseHorizontalAlignment(SetterValueHelper.StringOrEmpty(value)));
            return;
        }
        if (dp is DependencyProperty<VerticalAlignment>) {
            element.SetStyleValue<VerticalAlignment>((DependencyProperty<VerticalAlignment>)dp,
                UIEnumConverter.ParseVerticalAlignment(SetterValueHelper.StringOrEmpty(value)));
            return;
        }
        if (dp is DependencyProperty<ScrollBarVisibility>) {
            element.SetStyleValue<ScrollBarVisibility>((DependencyProperty<ScrollBarVisibility>)dp,
                UIEnumConverter.ParseScrollBarVisibility(SetterValueHelper.StringOrEmpty(value)));
            return;
        }
    }
}
