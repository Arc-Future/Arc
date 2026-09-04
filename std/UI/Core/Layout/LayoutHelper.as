// RFC 037 D5: Arc.UI.Layout — LayoutHelper 布局核心工具。
//
// Measure/Arrange 共享：尺寸消毒、边距解析、对齐计算、文本估算。
// M3.6 兼容：ArrangeChild 写入 LayoutX/LayoutY，由 PlatformTreeSync 同步到平台镜像。

namespace Arc.UI.Layout;

using Arc.UI;
using Arc.UI.Components;

/// <summary>布局算法共享工具（纯函数 + FrameworkElement 辅助）。</summary>
internal class LayoutHelper {
    public const double MaxLayout = 1000000.0;
    // 用一个极大但安全的值表示"无限"约束，避免编译器 const 字段限制
    // 此值在实际布局中远超任何合理窗口尺寸
    public const double Unbounded = 1000000000.0;

    /// <summary>判断约束是否为无限（Unbounded）。</summary>
    public static bool IsUnbounded(double value) {
        return value > 1000000000.0;
    }

    public const double MinTextPaddingX = 8.0;
    public const double MinTextPaddingY = 4.0;
    public const double ButtonPaddingX = 24.0;
    public const double ButtonPaddingY = 12.0;
    /// <summary>行高相对字号倍率（编辑器视口 / 多行估算）。</summary>
    public const double FontLineHeightRatio = 1.2;

    public static double Sanitize(double value) {
        if (value != value) {
            return 0.0;
        }
        if (value < 0.0) {
            return 0.0;
        }
        // 保留 Unbounded 值（大于 MaxLayout 视为无限，不截断）
        if (value > MaxLayout && value < Unbounded) {
            return MaxLayout;
        }
        return value;
    }

    /// <summary>坐标专用清洗：仅 NaN→0。位置合法为负（ScrollView 内容位于 -Offset），
    /// 尺寸清洗 <see cref="Sanitize"/> 的非负钳制不得用于坐标，否则滚动内容被钉死在原点。</summary>
    public static double SanitizePos(double value) {
        if (value != value) {
            return 0.0;
        }
        return value;
    }

    public static LayoutSize SanitizeSize(LayoutSize size) {
        return new LayoutSize(Sanitize(size.Width), Sanitize(size.Height));
    }

    public static double ParseNumber(string text, double defaultValue) {
        if (text == null || text.Length == 0) {
            return defaultValue;
        }
        double sign = 1.0;
        int i = 0;
        if (text[0] == '-') {
            sign = -1.0;
            i = 1;
        }
        double whole = 0.0;
        bool hasDigit = false;
        while (i < text.Length) {
            char c = text[i];
            if (c >= '0' && c <= '9') {
                whole = whole * 10.0 + (double)(c - '0');
                hasDigit = true;
                i++;
            } else {
                break;
            }
        }
        if (!hasDigit) {
            return defaultValue;
        }
        if (i < text.Length && text[i] == '.') {
            i++;
            double frac = 0.0;
            double place = 0.1;
            while (i < text.Length) {
                char c = text[i];
                if (c >= '0' && c <= '9') {
                    frac += (double)(c - '0') * place;
                    place *= 0.1;
                    i++;
                } else {
                    break;
                }
            }
            return sign * (whole + frac);
        }
        return sign * whole;
    }

    public static Thickness GetMargin(FrameworkElement fe) {
        if (fe == null) {
            return new Thickness(0.0);
        }
        return Thickness.Parse(fe.Margin).Sanitized();
    }

    public static LayoutSize Deflate(LayoutSize size, Thickness margin) {
        double w = Sanitize(size.Width - margin.Left - margin.Right);
        double h = Sanitize(size.Height - margin.Top - margin.Bottom);
        return new LayoutSize(w, h);
    }

    public static LayoutSize Inflate(LayoutSize inner, Thickness margin) {
        return new LayoutSize(
            Sanitize(inner.Width + margin.Left + margin.Right),
            Sanitize(inner.Height + margin.Top + margin.Bottom));
    }

    public static LayoutSize ApplyMinMax(FrameworkElement fe, LayoutSize size) {
        double w = Sanitize(size.Width);
        double h = Sanitize(size.Height);
        if (fe != null) {
            if (fe.MinWidth > 0.0 && w < fe.MinWidth) {
                w = fe.MinWidth;
            }
            if (fe.MaxWidth > 0.0 && w > fe.MaxWidth) {
                w = fe.MaxWidth;
            }
            if (fe.MinHeight > 0.0 && h < fe.MinHeight) {
                h = fe.MinHeight;
            }
            if (fe.MaxHeight > 0.0 && h > fe.MaxHeight) {
                h = fe.MaxHeight;
            }
        }
        return new LayoutSize(w, h);
    }

    public static LayoutSize ComputeConstraintSize(FrameworkElement fe, LayoutSize availableInner) {
        double w = availableInner.Width;
        double h = availableInner.Height;
        if (fe != null) {
            if (fe.Width > 0.0) {
                w = fe.Width;
            }
            if (fe.Height > 0.0) {
                h = fe.Height;
            }
        }
        return ApplyMinMax(fe, new LayoutSize(w, h));
    }

    /// <summary>
    /// 文本布局尺寸——经 <see cref="TextMeasuring"/> 同源 atlas 度量。
    /// atlas/后端未就绪时诚实返回「文本宽贡献=0、高=字号占位」（禁字节启发式双轨）；
    /// FramePump 挂接度量后经 <see cref="Window.RelayoutSynced"/> 重测。
    /// </summary>
    public static LayoutSize EstimateTextSize(string text, double fontSize,
                                               double padX, double padY) {
        return EstimateTextSize(text, fontSize, padX, padY, null, "Normal");
    }

    /// <summary>同 <see cref="EstimateTextSize(string, double, double, double)"/>，带 FontFamily。</summary>
    public static LayoutSize EstimateTextSize(string text, double fontSize,
                                               double padX, double padY,
                                               string fontFamily) {
        return EstimateTextSize(text, fontSize, padX, padY, fontFamily, "Normal");
    }

    /// <summary>同源度量：FontFamily + FontWeight 与 DrawText 解析一致。</summary>
    public static LayoutSize EstimateTextSize(string text, double fontSize,
                                               double padX, double padY,
                                               string fontFamily, string fontWeight) {
        if (text == null) {
            text = "";
        }
        double fs = Sanitize(fontSize);
        if (fs <= 0.0) {
            fs = 14.0;
        }
        ITextMetrics metrics = TextMeasuring.Current;
        if (metrics == null) {
            // 诚实占位：不冒充字形宽度。宽=仅 padding；高=字号+padding（供窗口尺寸回退）。
            return new LayoutSize(Sanitize(padX), Sanitize(fs + padY));
        }
        return metrics.MeasureText(text, fs, padX, padY, fontFamily, fontWeight);
    }

    /// <summary>
    /// 单行行高：与 DrawText 同源（探针字形高度）；度量未就绪时诚实回退字号×倍率。
    /// </summary>
    public static double EstimateLineHeight(double fontSize, string fontFamily, string fontWeight) {
        double fs = Sanitize(fontSize);
        if (fs <= 0.0) {
            fs = 14.0;
        }
        ITextMetrics metrics = TextMeasuring.Current;
        if (metrics != null) {
            LayoutSize sz = metrics.MeasureText("Mg", fs, 0.0, 0.0, fontFamily, fontWeight);
            if (sz.Height >= 1.0) {
                return sz.Height;
            }
        }
        double h = fs * FontLineHeightRatio;
        if (h < 1.0) {
            h = 16.0;
        }
        return h;
    }

    public static void MeasureChild(FrameworkElement child, LayoutSize available) {
        if (child == null) {
            return;
        }
        child.Measure(SanitizeSize(available));
    }

    /// <summary>
    /// 排列子元素：计算对齐后写入**绝对** LayoutX/LayoutY（父原点 + 槽位），再调用 Arrange。
    /// 统一布局权威契约：LayoutX/Y 为相对窗口根的原点，命中测试与渲染（WgpuRender）
    /// 都读同一份绝对 rect，单一几何来源（消除 A2 分叉）。PlatformTreeSync 读取
    /// LayoutX/Y + RenderSize 同步到平台 Layout* 属性。
    /// </summary>
    public static void ArrangeChild(FrameworkElement parent,
                                     FrameworkElement child,
                                     double slotX, double slotY,
                                     double slotW, double slotH) {
        if (child == null) {
            return;
        }
        double parentX = parent != null ? parent.LayoutX : 0.0;
        double parentY = parent != null ? parent.LayoutY : 0.0;
        double x = SanitizePos(parentX + slotX);
        double y = SanitizePos(parentY + slotY);
        double w = Sanitize(child.DesiredSize.Width);
        double h = Sanitize(child.DesiredSize.Height);

        double slotXa = parentX + slotX;
        double slotYa = parentY + slotY;
        double slotWg = slotW;
        double slotHg = slotH;

        HorizontalAlignment ha = child.HorizontalAlignment;
        VerticalAlignment va = child.VerticalAlignment;

        if (ha == HorizontalAlignment.Stretch || w <= 0.0 || w > slotWg) {
            w = slotWg;
        } else if (ha == HorizontalAlignment.Center) {
            x = SanitizePos(slotXa + (slotWg - w) * 0.5);
        } else if (ha == HorizontalAlignment.Right) {
            x = SanitizePos(slotXa + slotWg - w);
        }

        if (va == VerticalAlignment.Stretch || h <= 0.0 || h > slotHg) {
            h = slotHg;
        } else if (va == VerticalAlignment.Center) {
            y = SanitizePos(slotYa + (slotHg - h) * 0.5);
        } else if (va == VerticalAlignment.Bottom) {
            y = SanitizePos(slotYa + slotHg - h);
        }

        child.LayoutX = x;
        child.LayoutY = y;
        child.Arrange(new LayoutSize(w, h));
    }

    public static double Max(double a, double b) {
        if (a > b) {
            return a;
        }
        return b;
    }

    public static double Min(double a, double b) {
        if (a < b) {
            return a;
        }
        return b;
    }

    /// <summary>读取附加数值属性（Canvas.Left/Top 等；Grid.Row/Column 自 RFC 037 由 Grid.GetRow/GetColumn typed 访问器直读）。</summary>
    public static double GetAttachedNumber(Element elem, string key, double defaultValue) {
        if (elem == null) {
            return defaultValue;
        }
        return elem.GetAttachedNumber(key, defaultValue);
    }

    /// <summary>读取附加字符串属性（DockPanel.Dock 等）。</summary>
    public static string GetAttachedString(Element elem, string key, string defaultValue) {
        if (elem == null) {
            return defaultValue;
        }
        return elem.GetAttachedString(key, defaultValue);
    }
}
