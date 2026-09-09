// 模板 chrome：PART_Chrome / PART_Glyph / PART_Content 经 ChromeHostHandle 消费宿主 VSM。
// 宿主有模板子树时跳过内置 chrome 硬分支；禁止再为已迁控件新增宿主硬编码 chrome。
//
// Slider/ProgressBar：PART Surface 画轨+填充（含 IsIndeterminate）；无 Button 式底/描边。
// ComboBox：PART Surface 画折叠壳；SelectedText/chevron 宿主层（同 TextBox 文本层）。

namespace Arc.UI.Rendering.Wgpu;

using Arc.UI;
using Arc.UI.Internal;
using Arc.UI.Layout;
using Arc.UI.Media;
using Arc.UI.Styling;

public partial class WgpuRender {
    /// <summary>模板部件是否挂了 chrome 宿主句柄。</summary>
    private long TemplateChromeHost(long handle) {
        double raw = WindowHost.ElementGetNumber(handle, "ChromeHostHandle", 0.0);
        return (long)raw;
    }

    /// <summary>ChromeRole：Surface / Glyph / Content。</summary>
    private string TemplateChromeRole(long handle) {
        return WindowHost.ElementGetString(handle, "ChromeRole", "");
    }

    /// <summary>按宿主类型解析 VSM 配方。</summary>
    private ControlVisual ResolveHostVisual(long host) {
        string hostType = WindowHost.ElementGetTypeName(host);
        int isEnabled = WindowHost.ElementGetBool(host, "IsEnabled", 1);
        int isMouseOver = WindowHost.ElementGetBool(host, "IsMouseOver", 0);
        int isPressed = WindowHost.ElementGetBool(host, "IsPressed", 0);
        int isFocused = WindowHost.ElementGetBool(host, "IsFocused", 0);
        int isChecked = WindowHost.ElementGetBool(host, "IsChecked", 0);
        ControlState st = ControlState.Of(isEnabled, isMouseOver, isPressed, isFocused, isChecked, 0);
        if (hostType == ElButton) {
            string styleKeys = WindowHost.ElementGetString(host, "StyleKeys", "");
            return VisualStateManager.ButtonForStyleKeys(st, styleKeys);
        }
        if (hostType == ElToggleButton
            || hostType == ElCheckBox
            || hostType == ElRadioButton) {
            return VisualStateManager.Toggle(st);
        }
        if (hostType == ElTextBox || hostType == ElPasswordBox) {
            return VisualStateManager.TextBox(ControlState.Of(isEnabled, 0, 0, isFocused, 0, 0));
        }
        if (hostType == ElSlider) {
            return VisualStateManager.Slider(st);
        }
        if (hostType == ElProgressBar) {
            return VisualStateManager.Progress(ControlState.Of(isEnabled, 0, 0, 0, 0, 0));
        }
        if (hostType == ElComboBox) {
            return VisualStateManager.ComboBox(ControlState.Of(isEnabled, isMouseOver, 0, isFocused, 0, 0));
        }
        return VisualStateManager.Button(st);
    }

    /// <summary>PART_Chrome / PART_Glyph：宿主 VSM → 圆角底/描边/阴影/焦点环/勾选强调。</summary>
    private void DrawTemplateChromePart(long partHandle, long host, string role,
                                        double lx, double ly, double lw, double lh) {
        if (host == 0 || lw <= 0.0 || lh <= 0.0) {
            return;
        }
        string hostType = WindowHost.ElementGetTypeName(host);
        if (role == "Surface" && hostType == ElSlider) {
            this.DrawSliderChrome(host, lx, ly, lw, lh);
            return;
        }
        if (role == "Surface" && hostType == ElProgressBar) {
            this.DrawProgressBarChrome(host, lx, ly, lw, lh);
            return;
        }
        ControlVisual pal = this.ResolveHostVisual(host);
        int isEnabled = WindowHost.ElementGetBool(host, "IsEnabled", 1);
        int isMouseOver = WindowHost.ElementGetBool(host, "IsMouseOver", 0);
        int isPressed = WindowHost.ElementGetBool(host, "IsPressed", 0);
        int isChecked = WindowHost.ElementGetBool(host, "IsChecked", 0);
        bool forceChrome = isEnabled == 0 || isMouseOver != 0 || isPressed != 0;
        if ((hostType == ElToggleButton || hostType == ElCheckBox || hostType == ElRadioButton)
            && isChecked != 0) {
            forceChrome = true;
        }
        double cr = pal.Radius.Max;
        if (role == "Glyph" && hostType == ElRadioButton) {
            cr = lw * 0.5;
        }
        if (pal.Lift.IsVisible && role == "Surface") {
            this.DrawSurfaceShadow(lx, ly, lw, lh, pal.Lift.Radius, pal.Lift.Blur, pal.Lift.OffsetY, pal.Lift.Alpha);
        }
        Color bg = this.ChromeStateColor(host, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration, forceChrome);
        Color border = this.ChromeStateColor(host, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration, forceChrome);
        Color gradStart = Color.Transparent();
        Color gradEnd = Color.Transparent();
        bool hasGradient = this.ResolveGradient(pal, ref gradStart, ref gradEnd);
        bool useGradient = false;
        if (role == "Surface" && !forceChrome && !this.HasExplicitBackground(host) && hasGradient) {
            useGradient = true;
        }
        if (role == "Glyph" && isChecked != 0 && !this.HasExplicitBackground(host) && hasGradient
            && hostType != ElRadioButton) {
            useGradient = true;
        }
        if (useGradient) {
            this.DrawLinearGradient(lx, ly, lw, lh, gradStart, gradEnd, 0.0, 0.0, 1.0, 0.0, cr);
        } else {
            this.DrawRoundedRect(lx, ly, lw, lh, cr, bg);
        }
        this.DrawRoundedBorder(lx, ly, lw, lh, cr, (double)RectBorderThickness, border);
        if (isChecked != 0
            && (hostType == ElToggleButton || hostType == ElCheckBox || hostType == ElRadioButton)) {
            Color accent = this.StateColor(host, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
            double inset = ControlMetrics.SpacingXS;
            double innerCr = cr - inset;
            if (innerCr < 0.0) {
                innerCr = 0.0;
            }
            if (hostType == ElRadioButton && role == "Glyph") {
                double inner = ControlMetrics.SpacingMD / 2.0;
                double innerCr = inner / 2.0;
                double inset = (lw - inner) / 2.0;
                this.DrawRoundedRect(lx + inset, ly + inset, inner, inner, innerCr, accent);
            } else {
                this.DrawRoundedRect(lx + inset, ly + inset, lw - inset * 2.0, lh - inset * 2.0, innerCr, accent);
            }
        }
        if (role == "Surface" && WindowHost.ElementGetBool(host, "IsFocusVisible", 0) != 0) {
            if (pal.FocusGlow.IsVisible) {
                this.DrawSurfaceShadow(lx, ly, lw, lh, pal.FocusGlow.Radius, pal.FocusGlow.Blur,
                                       pal.FocusGlow.OffsetY, pal.FocusGlow.Alpha);
            }
            Color ring = this.StateColor(host, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
            this.DrawRoundedBorder(
                lx - ControlMetrics.FocusRingOutset,
                ly - ControlMetrics.FocusRingOutset,
                lw + ControlMetrics.FocusRingOutset * 2.0,
                lh + ControlMetrics.FocusRingOutset * 2.0,
                cr + ControlMetrics.FocusRingOutset,
                pal.FocusRingWidth, ring);
        }
        if (role == "Glyph" && WindowHost.ElementGetBool(host, "IsFocusVisible", 0) != 0) {
            Color ring = this.StateColor(host, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
            this.DrawRoundedBorder(
                lx - ControlMetrics.FocusRingOutset,
                ly - ControlMetrics.FocusRingOutset,
                lw + ControlMetrics.FocusRingOutset * 2.0,
                lh + ControlMetrics.FocusRingOutset * 2.0,
                cr + ControlMetrics.FocusRingOutset,
                pal.FocusRingWidth, ring);
        }
    }

    /// <summary>PART_Content：前景色跟宿主 VSM。</summary>
    private Color TemplateContentForeground(long host, Color fallback) {
        if (host == 0) {
            return fallback;
        }
        ControlVisual pal = this.ResolveHostVisual(host);
        return this.StateColor(host, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
    }

    /// <summary>TextBox/PasswordBox 模板态：仅画文本/选区/caret（壳在 PART_Chrome）。</summary>
    private void DrawTextBoxContentLayer(long handle, double lx, double ly, double lw, double lh) {
        string text = WindowHost.ElementGetString(handle, "Text", "");
        string placeholder = WindowHost.ElementGetString(handle, "Placeholder", "");
        string composition = WindowHost.ElementGetString(handle, "CompositionText", "");
        double caretIdx = WindowHost.ElementGetNumber(handle, "CaretIndex", 0.0);
        int caretIndex = (int)caretIdx;
        int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
        int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
        // 水印仅空且未聚焦时显示；聚焦即消（与 caret 并存会抢视觉 / Motion 闪）。
        bool empty = text == null || text.Length == 0;
        bool isPlaceholder = empty && isFocused == 0
            && placeholder != null && placeholder.Length > 0;
        string display = isPlaceholder ? placeholder : (text != null ? text : "");
        string compPrefix = "";
        string compSuffix = "";
        bool hasComposition = false;
        if (!isPlaceholder && composition != null && composition.Length > 0 && text != null) {
            int len = text.Length;
            if (caretIndex < 0) { caretIndex = 0; }
            if (caretIndex > len) { caretIndex = len; }
            compPrefix = text.Substring(0, caretIndex);
            compSuffix = text.Substring(caretIndex);
            display = compPrefix + composition + compSuffix;
            hasComposition = true;
        }
        double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", ControlMetrics.FontBodySize);
        int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
        int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
        double scaledGlyphHeight = this.EstTextHeight("Ag", 0.0, fontSize, family);
        if (scaledGlyphHeight <= 0.0) {
            scaledGlyphHeight = fontSize > 0.0 ? fontSize : GlyphHeight;
        }
        double iw = lw;
        if (iw <= 0.0) { iw = 100.0; }
        double ih = lh;
        if (ih <= 0.0) { ih = scaledGlyphHeight + 8.0; }
        ControlVisual pal = VisualStateManager.TextBox(ControlState.Of(isEnabled, 0, 0, isFocused, 0, 0));
        // Placeholder 禁走 RoleForeground Motion 槽（与 caret 同槽会跟 blink 插值闪）。
        Color fg;
        if (isPlaceholder) {
            fg = this.ResolveThemeKey(pal.Placeholder);
        } else {
            fg = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
        }
        double textX = lx + InputMetrics.PenOriginX - (MinTextPaddingX / 2.0);
        double textY = ly + (ih - scaledGlyphHeight) / 2.0;
        double penOriginX = lx + InputMetrics.PenOriginX;
        double selStartV = WindowHost.ElementGetNumber(handle, "SelectionStart", 0.0);
        double selLenV = WindowHost.ElementGetNumber(handle, "SelectionLength", 0.0);
        int selStart = (int)selStartV;
        int selLen = (int)selLenV;
        if (!isPlaceholder && text != null && selLen > 0 && selStart < text.Length) {
            if (selStart + selLen > text.Length) {
                selLen = text.Length - selStart;
            }
            if (selLen > 0) {
                double selX = penOriginX
                    + this.EstTextWidth(text.Substring(0, selStart), 0.0, fontSize, family, weight);
                double selW = this.EstTextWidth(
                    text.Substring(selStart, selLen), 0.0, fontSize, family, weight);
                if (selW > 0.0) {
                    this.DrawRect(selX, textY, selW, scaledGlyphHeight,
                        this.ResolveThemeKey(BuiltInTheme.TextSelection));
                }
            }
        }
        if (display != null && display.Length > 0) {
            this.DrawText(display, textX, textY, fontSize, this.ColorTransparent(), fg, family, weight);
        }
        if (hasComposition) {
            double compX = penOriginX + this.EstTextWidth(compPrefix, 0.0, fontSize, family, weight);
            double compW = this.EstTextWidth(composition, 0.0, fontSize, family, weight);
            double underlineY = textY + scaledGlyphHeight - 2.0;
            if (compW > 0.0) {
                this.DrawRect(compX, underlineY, compW, 1.0,
                    this.ResolveThemeKey(BuiltInTheme.TextPrimary));
            }
        }
        if (isFocused != 0 && FramePump.CaretBlinkOn()) {
            string caretPrefix = "";
            if (hasComposition) {
                caretPrefix = compPrefix + composition;
            } else if (text != null) {
                int ci = caretIndex;
                int tlen = text.Length;
                if (ci < 0) { ci = 0; }
                if (ci > tlen) { ci = tlen; }
                caretPrefix = text.Substring(0, ci);
            }
            double caretX = penOriginX
                + this.EstTextWidth(caretPrefix, 0.0, fontSize, family, weight);
            Color caretColor = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
            this.DrawRect(caretX, textY, 1.5, scaledGlyphHeight, caretColor);
        }
    }

    /// <summary>Slider 轨 + 比例填充 + thumb（宿主回退与 PART Surface 共用）。</summary>
    private void DrawSliderChrome(long handle, double lx, double ly, double lw, double lh) {
        double val = WindowHost.ElementGetNumber(handle, "Value", 0.0);
        double min = WindowHost.ElementGetNumber(handle, "Minimum", 0.0);
        double max = WindowHost.ElementGetNumber(handle, "Maximum", 100.0);
        int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
        int isMouseOver = WindowHost.ElementGetBool(handle, "IsMouseOver", 0);
        int isPressed = WindowHost.ElementGetBool(handle, "IsPressed", 0);
        int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
        double sw = lw;
        if (sw <= 0.0) { sw = 200.0; }
        double sh = lh;
        if (sh <= 0.0) { sh = GlyphHeight + ControlMetrics.SpacingSM; }
        ControlVisual pal = VisualStateManager.Slider(ControlState.Of(isEnabled, isMouseOver, isPressed, isFocused, 0, 0));
        Color trackColor = this.StateColor(handle, "TrackBrush", pal.Track, MotionEngine.RoleBorder);
        Color foregroundColor = this.StateColor(handle, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
        double trackT = ControlMetrics.SliderTrackThickness;
        double insetX = ControlMetrics.SliderTrackInsetX;
        double trackY = ly + sh / 2.0 - trackT / 2.0;
        this.DrawRoundedRect(lx + insetX, trackY, sw - insetX * 2.0, trackT, trackT / 2.0, trackColor);
        double range = max - min;
        double t = (range > 0.0) ? (val - min) / range : 0.0;
        if (t < 0.0) { t = 0.0; } if (t > 1.0) { t = 1.0; }
        double fillWidth = (sw - insetX * 2.0) * t;
        this.DrawRoundedRect(lx + insetX, trackY, fillWidth, trackT, trackT / 2.0, foregroundColor);
        double thumbW = ControlMetrics.SliderThumbWidth;
        double thumbH = ControlMetrics.SliderThumbHeight;
        this.DrawRoundedRect(
            lx + insetX + fillWidth - thumbW / 2.0,
            ly + sh / 2.0 - thumbH / 2.0,
            thumbW, thumbH, ControlMetrics.ControlRadius, foregroundColor);
        if (WindowHost.ElementGetBool(handle, "IsFocusVisible", 0) != 0) {
            Color ring = this.StateColor(handle, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
            double cr = pal.Radius.Max;
            this.DrawRoundedBorder(
                lx - ControlMetrics.FocusRingOutset,
                ly - ControlMetrics.FocusRingOutset,
                sw + ControlMetrics.FocusRingOutset * 2.0,
                sh + ControlMetrics.FocusRingOutset * 2.0,
                cr + ControlMetrics.FocusRingOutset,
                pal.FocusRingWidth, ring);
        }
    }

    /// <summary>ProgressBar 轨 + 比例填充 / IsIndeterminate 扫掠（宿主回退与 PART 共用）。</summary>
    private void DrawProgressBarChrome(long handle, double lx, double ly, double lw, double lh) {
        double val = WindowHost.ElementGetNumber(handle, "Value", 0.0);
        double min = WindowHost.ElementGetNumber(handle, "Minimum", 0.0);
        double max = WindowHost.ElementGetNumber(handle, "Maximum", 100.0);
        int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
        int isIndeterminate = WindowHost.ElementGetBool(handle, "IsIndeterminate", 0);
        double pw = lw;
        if (pw <= 0.0) { pw = 200.0; }
        double ph = lh;
        if (ph <= 0.0) { ph = ControlMetrics.ProgressBarThickness; }
        ControlVisual pal = VisualStateManager.Progress(ControlState.Of(isEnabled, 0, 0, 0, 0, 0));
        Color trackColor = this.StateColor(handle, "TrackBrush", pal.Track, MotionEngine.RoleBorder);
        Color fillColor = this.StateColor(handle, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
        double barCr = ph / 2.0;
        if (barCr > ControlMetrics.SpacingXS) { barCr = ControlMetrics.SpacingXS; }
        this.DrawRoundedRect(lx, ly, pw, ph, barCr, trackColor);
        if (isIndeterminate != 0) {
            double periodBase = BuiltInTheme.MotionFocusMs;
            if (Application.Current != null) {
                periodBase = Application.Current.ResolveNumber(BuiltInTheme.MotionDurationNormal);
                if (!(periodBase > 0.0)) {
                    periodBase = BuiltInTheme.MotionFocusMs;
                }
            }
            double period = periodBase * ControlMetrics.ProgressBarIndeterminatePeriodFactor;
            double phase = MotionEngine.ResolveLoop01(handle, period);
            double segW = pw * ControlMetrics.ProgressBarIndeterminateFraction;
            if (segW < 1.0) {
                segW = 1.0;
            }
            double travel = pw + segW;
            double x0 = lx + phase * travel - segW;
            double drawX = x0;
            double drawW = segW;
            if (drawX < lx) {
                drawW = drawW - (lx - drawX);
                drawX = lx;
            }
            if (drawX + drawW > lx + pw) {
                drawW = (lx + pw) - drawX;
            }
            if (drawW > 0.0) {
                this.DrawRoundedRect(drawX, ly, drawW, ph, barCr, fillColor);
            }
        } else {
            MotionEngine.CancelLoop(handle);
            double range = max - min;
            double t = (range > 0.0) ? (val - min) / range : 0.0;
            if (t < 0.0) { t = 0.0; } if (t > 1.0) { t = 1.0; }
            double fillWidth = pw * t;
            if (fillWidth > 0.0) {
                this.DrawRoundedRect(lx, ly, fillWidth, ph, barCr, fillColor);
            }
        }
    }

    /// <summary>ComboBox 模板态：SelectedText + chevron（壳在 PART_Chrome）。</summary>
    private void DrawComboBoxContentLayer(long handle, double lx, double ly, double lw, double lh) {
        int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
        double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", ControlMetrics.FontBodySize);
        int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
        int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
        double scaledGlyphHeight = this.EstTextHeight("Ag", 0.0, fontSize, family);
        if (scaledGlyphHeight <= 0.0) {
            scaledGlyphHeight = fontSize > 0.0 ? fontSize : GlyphHeight;
        }
        double iw = lw;
        if (iw <= 0.0) { iw = 160.0; }
        double ih = lh;
        if (ih <= 0.0) { ih = scaledGlyphHeight + ControlMetrics.SpacingMD; }
        ControlVisual pal = VisualStateManager.ComboBox(ControlState.Of(isEnabled, 0, 0, 0, 0, 0));
        Color fg = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
        string selectedText = WindowHost.ElementGetString(handle, "SelectedText", "");
        double textY = ly + (ih - scaledGlyphHeight) / 2.0;
        this.DrawText(selectedText, lx + ControlMetrics.SpacingXS, textY, fontSize, this.ColorTransparent(), fg, family, weight);
        double chevronCx = lx + iw - ControlMetrics.ComboChevronInset;
        double chevronCy = ly + ih / 2.0 - ControlMetrics.ComboChevronCenterNudgeY;
        double stroke = ControlMetrics.ComboChevronStroke;
        double stepY = ControlMetrics.ComboChevronStepY;
        this.DrawRect(chevronCx - ControlMetrics.SpacingXS, chevronCy,
            ControlMetrics.SpacingSM, stroke, fg);
        this.DrawRect(chevronCx - ControlMetrics.ComboChevronMidHalfWidth, chevronCy + stepY,
            ControlMetrics.ComboChevronMidWidth, stroke, fg);
        this.DrawRect(chevronCx - ControlMetrics.ComboChevronTipHalfWidth, chevronCy + stepY * 2.0,
            ControlMetrics.ComboChevronTipWidth, stroke, fg);
    }
}
