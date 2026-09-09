// Arc.UI — MessageBox 按钮集（对标 WPF MessageBoxButton 子集）。
//
// OK / OKCancel / YesNo / YesNoCancel。

namespace Arc.UI;

/// <summary>MessageBox 按钮布局。</summary>
public enum MessageBoxButton {
    /// <summary>仅确定。</summary>
    OK,
    /// <summary>确定 + 取消。</summary>
    OKCancel,
    /// <summary>是 + 否。</summary>
    YesNo,
    /// <summary>是 + 否 + 取消。</summary>
    YesNoCancel,
}
