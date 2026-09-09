// Arc.UI.Components — ProgressBar 进度条（对标 WPF RangeBase 最小面）。
//
// WPF 同构：Control → ProgressBar（Arc 合并 RangeBase 的 Value/Minimum/Maximum）。
// 只读反馈控件：无指针交互、默认不可聚焦；三层契约见 production-surface §6。
//
// IsIndeterminate：RenderTree 扫掠段 + MotionEngine.ResolveLoop01（周期 =
// Motion.Duration.Normal × ControlMetrics.ProgressBarIndeterminatePeriodFactor）。

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Layout;

/// <summary>进度条——在 [Minimum, Maximum] 区间内以 Value 比例填充轨道。</summary>
public class ProgressBar : Control {
    /// <summary>Value 属性元数据——当前进度值，默认 0.0。</summary>
    public static DependencyProperty<double> ValueProperty =
        RegisterProperty<double>(nameof(Value), typeof(ProgressBar), 0.0);

    /// <summary>Minimum 属性元数据——区间下限，默认 0.0。</summary>
    public static DependencyProperty<double> MinimumProperty =
        RegisterProperty<double>(nameof(Minimum), typeof(ProgressBar), 0.0);

    /// <summary>Maximum 属性元数据——区间上限，默认 100.0。</summary>
    public static DependencyProperty<double> MaximumProperty =
        RegisterProperty<double>(nameof(Maximum), typeof(ProgressBar), 100.0);

    /// <summary>IsIndeterminate 属性元数据——不定长扫掠模式，默认 false。</summary>
    public static DependencyProperty<bool> IsIndeterminateProperty =
        RegisterProperty<bool>(nameof(IsIndeterminate), typeof(ProgressBar), false);

    /// <summary>当前进度值（写入时 clamp 到 [Minimum, Maximum]）。</summary>
    public double Value {
        get { return this.GetValue<double>(ValueProperty); }
        set {
            double clamped = value;
            double min = this.Minimum;
            double max = this.Maximum;
            if (clamped < min) {
                clamped = min;
            }
            if (clamped > max) {
                clamped = max;
            }
            this.SetValue<double>(ValueProperty, clamped);
            this.SyncMirrorValue();
        }
    }

    /// <summary>区间下限。</summary>
    public double Minimum {
        get { return this.GetValue<double>(MinimumProperty); }
        set { this.SetValue<double>(MinimumProperty, value); }
    }

    /// <summary>区间上限。</summary>
    public double Maximum {
        get { return this.GetValue<double>(MaximumProperty); }
        set { this.SetValue<double>(MaximumProperty, value); }
    }

    /// <summary>不定长模式——true 时忽略 Value 比例填充，改为轨道扫掠动画。</summary>
    public bool IsIndeterminate {
        get { return this.GetValue<bool>(IsIndeterminateProperty); }
        set {
            this.SetValue<bool>(IsIndeterminateProperty, value);
            this.SyncMirrorValue();
        }
    }

    public ProgressBar() {
        this.Type = typeof(ProgressBar);
        this.TypeName = "ProgressBar";
        this.Focusable = false;
        this.IsTabStop = false;
    }

    long _mirrorHandle;

    /// <summary>平台镜像登记（PlatformTreeSync；幂等）。</summary>
    public void BindPlatformMirror(long handle) {
        if (_mirrorHandle == handle) {
            return;
        }
        _mirrorHandle = handle;
        this.SyncMirrorValue();
    }

    void SyncMirrorValue() {
        if (_mirrorHandle != 0) {
            WindowHost.ElementSetNumber(_mirrorHandle, "Value", this.Value);
            WindowHost.ElementSetNumber(_mirrorHandle, "Minimum", this.Minimum);
            WindowHost.ElementSetNumber(_mirrorHandle, "Maximum", this.Maximum);
            WindowHost.ElementSetBool(_mirrorHandle, "IsIndeterminate", this.IsIndeterminate ? 1 : 0);
        }
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        if (this.HasTemplateVisual()) {
            LayoutSize templated = this.MeasureTemplateVisual(availableSize);
            double tw = templated.Width;
            double th = templated.Height;
            if (tw < 120.0) {
                tw = 120.0;
            }
            if (th < ControlMetrics.ProgressBarThickness) {
                th = ControlMetrics.ProgressBarThickness;
            }
            if (this.Width > 0.0) {
                tw = this.Width;
            }
            if (this.Height > 0.0) {
                th = this.Height;
            }
            return new LayoutSize(tw, th);
        }
        double w = 200.0;
        double h = ControlMetrics.ProgressBarThickness;
        double availW = availableSize.Width;
        if (availW > 0.0 && w > availW) {
            w = availW;
        }
        if (this.Width > 0.0) {
            w = this.Width;
        }
        if (this.Height > 0.0) {
            h = this.Height;
        }
        return new LayoutSize(w, h);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        if (this.HasTemplateVisual()) {
            this.ArrangeTemplateVisual(finalSize);
        }
    }
}
