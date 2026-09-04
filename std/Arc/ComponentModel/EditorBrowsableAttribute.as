// RFC 012 M3: 标准库属性 — 编辑器可浏览标记 [EditorBrowsable]。
//
// 对标 C# System.ComponentModel.EditorBrowsableAttribute。

namespace Arc.ComponentModel;

/// <summary>
/// 编辑器浏览状态枚举。
/// </summary>
public enum EditorBrowsableState {
    /// 始终可见。
    Always,
    /// 从不显示。
    Never,
    /// 仅高级模式显示。
    Advanced,
}

/// <summary>
/// 指定属性或方法在编辑器中的可见性。
///
/// 用法：`[EditorBrowsable(EditorBrowsableState.Never)]`。
/// 合法附加目标：All。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class EditorBrowsableAttribute : Attribute {
    /// 编辑器浏览状态。
    public EditorBrowsableState State { get; }

    public EditorBrowsableAttribute(EditorBrowsableState state) {
        State = state;
    }
}
