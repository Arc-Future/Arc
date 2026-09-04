// RFC 012 M3: 标准库属性 — 数据绑定标记 [Bindable]。
//
// 对标 C# System.ComponentModel.BindableAttribute。

namespace Arc.ComponentModel;

/// <summary>
/// 数据绑定方向枚举。
/// </summary>
public enum BindingDirection {
    /// 单向绑定（源到目标）。
    OneWay,
    /// 双向绑定。
    TwoWay,
}

/// <summary>
/// 指定属性是否通常用于数据绑定。
///
/// 用法：`[Bindable(true)]` 或 `[Bindable(BindingDirection.TwoWay)]`。
/// 合法附加目标：All。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class BindableAttribute : Attribute {
    /// 是否可绑定。
    public bool Bindable { get; }
    /// 绑定方向（默认 OneWay）。
    public BindingDirection Direction { get; }

    public BindableAttribute(bool bindable) {
        Bindable = bindable;
        Direction = BindingDirection.OneWay;
    }

    public BindableAttribute(BindingDirection direction) {
        Bindable = true;
        Direction = direction;
    }
}
