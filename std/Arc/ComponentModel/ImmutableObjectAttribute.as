// RFC 012 M3: 标准库属性 — 不可变对象标记 [ImmutableObject]。
//
// 对标 C# System.ComponentModel.ImmutableObjectAttribute。

namespace Arc.ComponentModel;

/// <summary>
/// 指定对象没有可编辑的子属性。
///
/// 用法：`[ImmutableObject(true)]`。
/// 合法附加目标：All。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class ImmutableObjectAttribute : Attribute {
    /// 是否不可变。
    public bool Immutable { get; }

    public ImmutableObjectAttribute(bool immutable) {
        Immutable = immutable;
    }
}
