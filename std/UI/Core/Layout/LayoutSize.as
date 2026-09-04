// Arc.UI.Layout — LayoutSize 布局尺寸。
//
// 设计决策：Arc 不支持元组返回类型 `(double, double)`。
// 使用 struct（值类型，栈分配）封装 Width/Height 对，替代 (double,double) 元组。
//
// 零开销保障：
//   - 值类型语义，栈上分配，无 GC 压力
//   - LLVM 优化器可完全标量化（scalar replacement）
//   - 无线程共享，无线程安全开销

namespace Arc.UI.Layout;

/// <summary>布局尺寸——封装宽高对，替代 (double,double) 元组。</summary>
public struct LayoutSize {
    /// <summary>宽度（CSS 像素）。0 = 自动。</summary>
    public double Width;

    /// <summary>高度（CSS 像素）。0 = 自动。</summary>
    public double Height;

    public LayoutSize() {
        this.Width = 0.0;
        this.Height = 0.0;
    }

    /// <summary>创建指定尺寸。</summary>
    public LayoutSize(double w, double h) {
        this.Width = w;
        this.Height = h;
    }
}
