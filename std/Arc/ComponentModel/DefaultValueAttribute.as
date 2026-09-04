// RFC 012 M3: 标准库属性 — 默认值标记 [DefaultValue]。
//
// 对标 C# System.ComponentModel.DefaultValueAttribute。

namespace Arc.ComponentModel;

/// <summary>
/// 指定属性的默认值。
///
/// 用法：`[DefaultValue(0)]` 或 `[DefaultValue("hello")]`。
/// 合法附加目标：All。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class DefaultValueAttribute : Attribute {
    /// 默认值（可为任意类型）。
    public object Value { get; }

    public DefaultValueAttribute(object value) {
        Value = value;
    }
}
