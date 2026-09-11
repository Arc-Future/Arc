// RFC 037 · theme-style-interaction-architecture P1：控件几何尺寸令牌单点。
//
// Ant Design 6 Seed：controlHeight=32、borderRadius=6、fontSize=14、padding 8-grid。
// Measure / RenderTree / BuiltInTheme.FillNonColor / InputMetrics 同源引用——禁止在
// 控件 `.as` 与 RenderTree 再堆魔法数。色值仍在 Themes/*.arml。

namespace Arc.UI.Layout;

/// <summary>控件尺寸/圆角/字号令牌（DIP；主题无关几何权威）。</summary>
internal class ControlMetrics {
    /// <summary>Ant controlHeightSM。</summary>
    public const double ControlHeightSM = 24.0;

    /// <summary>Ant controlHeight（中档默认）。</summary>
    public const double ControlHeight = 32.0;

    /// <summary>Ant controlHeightLG。</summary>
    public const double ControlHeightLG = 40.0;

    /// <summary>Ant borderRadius。</summary>
    public const double ControlRadius = 6.0;

    /// <summary>Ant borderRadiusLG（卡片/面板）。</summary>
    public const double SurfaceRadius = 8.0;

    /// <summary>胶囊圆角。</summary>
    public const double PillRadius = 999.0;

    /// <summary>发丝边框宽。</summary>
    public const double BorderWidth = 1.0;

    /// <summary>焦点环宽。</summary>
    public const double FocusRingWidth = 2.0;

    /// <summary>Ant fontSize。</summary>
    public const double FontBodySize = 14.0;

    /// <summary>Ant fontSizeSM。</summary>
    public const double FontCaptionSize = 12.0;

    /// <summary>标题字号（Alias）。</summary>
    public const double FontHeadingSize = 16.0;

    /// <summary>Button 小档水平内边距（Ant size=small）。</summary>
    public const double ButtonPaddingXSM = 7.0;

    /// <summary>Button 水平内边距（中档；Ant paddingInline=15）。</summary>
    public const double ButtonPaddingX = 15.0;

    /// <summary>Button 大档水平内边距（Ant size=large · paddingInlineLG=15）。</summary>
    public const double ButtonPaddingXLG = 15.0;

    /// <summary>Button 小档垂直内边距。</summary>
    public const double ButtonPaddingYSM = 0.0;

    /// <summary>Button 垂直内边距（中档；配合 controlHeight 与字号）。</summary>
    public const double ButtonPaddingY = 4.0;

    /// <summary>Button 大档垂直内边距。</summary>
    public const double ButtonPaddingYLG = 6.0;

    /// <summary>CheckBox / Radio 勾选盒边长（Ant checkboxSize 对标）。</summary>
    public const double ToggleBoxSize = 16.0;

    /// <summary>勾选盒与标签间距。</summary>
    public const double ToggleLabelGap = 8.0;

    /// <summary>Slider 轨道厚度。</summary>
    public const double SliderTrackThickness = 4.0;

    /// <summary>Slider 轨道水平内缩。</summary>
    public const double SliderTrackInsetX = 4.0;

    /// <summary>Slider thumb 宽。</summary>
    public const double SliderThumbWidth = 12.0;

    /// <summary>Slider thumb 高。</summary>
    public const double SliderThumbHeight = 16.0;

    /// <summary>Slider 默认测高（含轨道+thumb 垂直余量）。</summary>
    public const double SliderDefaultHeight = 24.0;

    /// <summary>ProgressBar 默认轨高（Ant line 对标）。</summary>
    public const double ProgressBarThickness = 8.0;

    /// <summary>不定长扫掠段相对轨宽比例（Ant line indeterminate 视觉对标）。</summary>
    public const double ProgressBarIndeterminateFraction = 0.35;

    /// <summary>
    /// 不定长扫掠周期 = Motion.Duration.Normal × 本因子（Normal≈160ms → ~1.28s 一圈）。
    /// </summary>
    public const double ProgressBarIndeterminatePeriodFactor = 8.0;

    /// <summary>竖滚动条槽宽。</summary>
    public const double VScrollWidth = 12.0;

    /// <summary>竖滚动条滑块最小高。</summary>
    public const double VScrollMinThumb = 20.0;

    /// <summary>竖滚动条滑块相对槽宽内缩（两侧合计的一半口径用 SpacingXS）。</summary>
    public const double VScrollThumbInset = 4.0;

    /// <summary>焦点环外扩（相对控件边框；与 FocusRingWidth 配对）。</summary>
    public const double FocusRingOutset = 2.0;

    /// <summary>ComboBox 下拉 chevron 区宽（右缘内缩至 chevron 轴）。</summary>
    public const double ComboChevronInset = 12.0;

    /// <summary>Chevron 条线粗（无三角原语时的横条近似）。</summary>
    public const double ComboChevronStroke = 1.5;

    /// <summary>Chevron 竖直堆叠相对控件中线的上偏（使三线视觉居中）。</summary>
    public const double ComboChevronCenterNudgeY = 2.25;

    /// <summary>Chevron 中档横条半宽。</summary>
    public const double ComboChevronMidHalfWidth = 2.5;

    /// <summary>Chevron 中档横条全宽。</summary>
    public const double ComboChevronMidWidth = 5.0;

    /// <summary>Chevron 尖档横条半宽。</summary>
    public const double ComboChevronTipHalfWidth = 1.0;

    /// <summary>Chevron 尖档横条全宽。</summary>
    public const double ComboChevronTipWidth = 2.0;

    /// <summary>Chevron 相邻横条竖直步距。</summary>
    public const double ComboChevronStepY = 2.0;

    /// <summary>TabControl 内置页签栏高。</summary>
    public const double TabHeaderBarHeight = 36.0;

    /// <summary>Tab 页签标题字号（介于 Caption 与 Body）。</summary>
    public const double TabHeaderFontSize = 13.0;

    /// <summary>页签标题左右内边距（内容测宽 + 2×本值 = HeaderWidth）。</summary>
    public const double TabHeaderPaddingX = 12.0;

    /// <summary>页签点击区最小宽（空/极短 Header 保底）。</summary>
    public const double TabHeaderMinWidth = 40.0;

    /// <summary>选中页签 Accent 指示条距栏底外扩。</summary>
    public const double TabIndicatorInsetBottom = 3.0;

    /// <summary>页签标题相对栏中线的微调上偏。</summary>
    public const double TabLabelNudgeY = 1.0;

    /// <summary>Tab 溢出左右箭头按钮宽（挤栏时顶栏两端预留；命中/裁剪同源）。</summary>
    public const double TabOverflowArrowWidth = 24.0;

    /// <summary>Tab 溢出箭头点击一次的水平滚步长（DIP）。</summary>
    public const double TabOverflowScrollStep = 96.0;

    /// <summary>页签关闭槽宽（文案区右侧；测宽 / 命中 / 渲染同源）。</summary>
    public const double TabCloseSlotWidth = 16.0;

    /// <summary>页签关闭「x」字号。</summary>
    public const double TabCloseGlyphSize = 11.0;

    /// <summary>MessageBox 对话框外宽（含描边；内边距 = SpacingLG → Size.MessageBox.Padding）。</summary>
    public const double MessageBoxWidth = 400.0;

    /// <summary>MessageBox 主/次按钮固定宽。</summary>
    public const double MessageBoxButtonWidth = 88.0;

    /// <summary>MessageBox 自绘图标徽章边长。</summary>
    public const double MessageBoxIconSize = 36.0;

    /// <summary>TreeView 行高（Header 条；与 ControlHeight 同档密度）。</summary>
    public const double TreeRowHeight = 28.0;

    /// <summary>TreeView 每级缩进。</summary>
    public const double TreeIndentPerLevel = 16.0;

    /// <summary>TreeView 展开三角命中/绘制宽。</summary>
    public const double TreeExpanderWidth = 16.0;

    /// <summary>竖滚动条滑块圆角（VSM ScrollBar 配方）。</summary>
    public const double VScrollThumbRadius = 4.0;

    /// <summary>Spacing.XS。</summary>
    public const double SpacingXS = 4.0;

    /// <summary>Spacing.SM。</summary>
    public const double SpacingSM = 8.0;

    /// <summary>Spacing.MD。</summary>
    public const double SpacingMD = 12.0;

    /// <summary>Spacing.LG。</summary>
    public const double SpacingLG = 16.0;

    /// <summary>Spacing.XL。</summary>
    public const double SpacingXL = 24.0;

    private ControlMetrics() {
    }
}
