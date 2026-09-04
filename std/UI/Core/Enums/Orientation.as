// Arc.UI — Orientation 排列方向强类型枚举（对标 WPF Orientation）。
//
// 取代 StackPanel/WrapPanel/VirtualizingStackPanel 的 string DP 存储，
// 提供强类型成员（Horizontal/Vertical）。成员顺序对齐 WPF：Horizontal=0, Vertical=1。
//
// **命名空间归属**：本文件位于 std/UI/Enums/ 子目录，但归属到 `Arc.UI` 命名空间
// （基类/基础值类型在上层命名空间，派生实现在子命名空间的层级原则）。

namespace Arc.UI;

/// <summary>排列方向（对标 WPF `Orientation`）。</summary>
public enum Orientation {
    /// <summary>水平排列。</summary>
    Horizontal,
    /// <summary>垂直排列。</summary>
    Vertical,
}
