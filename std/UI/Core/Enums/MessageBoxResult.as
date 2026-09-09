// Arc.UI — MessageBox 关闭结果（对标 WPF MessageBoxResult 子集）。

namespace Arc.UI;

/// <summary>MessageBox.ShowAsync 完成结果。</summary>
public enum MessageBoxResult {
    /// <summary>未完成或占位。</summary>
    None,
    /// <summary>用户点确定（或 OK-only 下 Esc）。</summary>
    OK,
    /// <summary>用户点取消（或 OKCancel / YesNoCancel 下 Esc）。</summary>
    Cancel,
    /// <summary>用户点是。</summary>
    Yes,
    /// <summary>用户点否（或 YesNo 下 Esc）。</summary>
    No,
}
