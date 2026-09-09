// Arc.UI — MessageBox 图标种类（对标 WPF MessageBoxImage 子集）。
//
// 图标为 StackPanel 内自绘几何（色块 Rectangle + 符号 TextBlock），禁原生图标资源。

namespace Arc.UI;

/// <summary>MessageBox 左侧状态图标。</summary>
public enum MessageBoxImage {
    /// <summary>无图标。</summary>
    None,
    /// <summary>信息（Primary 色块 + i）。</summary>
    Information,
    /// <summary>警告（Warning 色块 + !）。</summary>
    Warning,
    /// <summary>错误（Danger 色块 + X）。</summary>
    Error,
    /// <summary>询问（Primary 色块 + ?）。</summary>
    Question,
}
