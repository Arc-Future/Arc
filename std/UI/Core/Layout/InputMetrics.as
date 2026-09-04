// RFC 037 §8 修订（text-editing.md §4）：InputMetrics——TextBox 几何常量单点。
//
// 消除 D9（几何常量跨层硬编码）：文本原点内边距、caret 宽高等常量在此单点
// 定义，命中端（TextBoxController.HandleClick）、IME 候选窗端
// （TextBox.ApplyImeFocus）与渲染端（WgpuRender.RenderTree 的 TextBox 段）
// 同源引用——三处此前的 4.0/6.0/8.0 各写一套即本类要消除的缺陷形态。

namespace Arc.UI.Layout;

/// <summary>
/// TextBox 几何常量单点（DIP；命中/渲染/IME 候选窗同源）。
/// </summary>
internal class InputMetrics {
    /// <summary>元素左缘 → DrawText 起点笔尖盒的水平内缩。</summary>
    public const double TextInsetX = 4.0;

    /// <summary>
    /// DrawText 内部 pen 相对笔尖盒起点的内缩（= LayoutHelper.MinTextPaddingX / 2 = 4.0；
    /// const 初始化器须为字面量常量表达式，故内联展开——与 LayoutHelper 单点值同步维护）。
    /// </summary>
    public const double PenInsetX = 4.0;

    /// <summary>
    /// 真实字形绘制原点（元素左缘 → 首字形笔尖）：命中定位、选区高亮、
    /// caret 竖线、IME 候选窗锚点共享此原点（= TextInsetX + PenInsetX = 8.0；
    /// const 初始化器须为字面量，故内联展开）。
    /// </summary>
    public const double PenOriginX = 8.0;

    /// <summary>软件 caret 竖线宽。</summary>
    public const double CaretWidth = 1.5;

    /// <summary>FontSize 未设置/非法时的度量回退值。</summary>
    public const double FontSizeFallback = 14.0;

    /// <summary>MeasureOverride 水平内边距。</summary>
    public const double PadX = 8.0;

    /// <summary>MeasureOverride 垂直内边距。</summary>
    public const double PadY = 6.0;

    /// <summary>最小期望宽（未约束布局下限）。</summary>
    public const double MinWidth = 120.0;

    /// <summary>最小期望高（未约束布局下限）。</summary>
    public const double MinHeight = 28.0;

    /// <summary>组字下划线相对字形底部的上移量。</summary>
    public const double UnderlineLiftY = 2.0;

    /// <summary>组字下划线厚度。</summary>
    public const double UnderlineHeight = 1.0;

    private InputMetrics() {
    }
}
