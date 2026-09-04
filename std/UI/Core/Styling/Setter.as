// RFC 037 D3.4 / RFC 037 D3: Arc.UI.Styling — Setter 属性设置器。
//
// 将属性名映射到类型化值。与 WPF Setter 对齐，Value 使用 SetterValue
// variant（RFC 037 标签联合）而非 object——零装箱，类型安全。
//
// 使用模式（隐式构造，typeck 自动重写）：
//   Setter s = new Setter();
//   s.Property = "FontSize";
//   s.Value = 14.0;         // → SetterValue.Number(14.0)
//   s.Value = "Red";        // → SetterValue.String("Red")
//   s.Value = true;         // → SetterValue.Boolean(true)

namespace Arc.UI.Styling;

/// <summary>属性设置器——将属性名映射到类型化值。</summary>
public struct Setter {
    /// <summary>目标属性名（如 "Width"/"Background"/"IsEnabled"）。</summary>
    public string Property;

    /// <summary>类型化属性值（SetterValue variant）。</summary>
    public SetterValue Value;

    /// <summary>
    /// 控件模板载荷（Setter Property="Template" 专供，WPF 同构）。独立字段
    /// 而非并入 SetterValue variant——variant 分支集变更会触发 codegen 泛型
    /// cast 错位（挂账编译器侧修复后可归并）；null = 标量/枚举 setter。
    /// </summary>
    public ControlTemplate? TemplateValue;

    public Setter() {
        this.Property = "";
        this.Value = SetterValue.String("");
        this.TemplateValue = null;
    }

    /// <summary>便捷构造：指定属性名，值默认为空字符串。</summary>
    public Setter(string property) {
        this.Property = property;
        this.Value = SetterValue.String("");
        this.TemplateValue = null;
    }
}
