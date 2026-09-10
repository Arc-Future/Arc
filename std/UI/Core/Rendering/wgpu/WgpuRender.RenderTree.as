// 渲染树遍历 / 布局权威消费 / 图元（partial 拆分）。
//
// WgpuRender 的渲染树实现（partial 扩展）：RenderElementTree /
// RenderElementNode / DrawBackground / DrawVScrollBar / DrawRectBorder。
// 布局 rect 一律读平台镜像 layout_*（Arc 层 LayoutManager → PlatformTreeSync
// 同步的绝对坐标），不再自行累加几何（消除 A2 分叉）。
// 方法与私有字段跨文件共享，详见核心文件 WgpuRender.as。

namespace Arc.UI.Rendering.Wgpu;

using Arc.Collections;
using Arc.UI.Components;
using Arc.UI.Internal;
using Arc.UI.Layout;
using Arc.UI.Media;
using Arc.UI.Rendering;
using Arc.UI.Styling;

public partial class WgpuRender {
    /// <summary>竖滚动条轨道+滑块（与 rt_ui_vscroll_* 同几何；production-surface §4）。</summary>
    private void DrawVScrollBar(long handle, double x, double y, double viewportW, double viewportH,
                                  double extentH, double offset, string visibility) {
        double scrollable = extentH - viewportH;
        if (scrollable < 0.0) {
            scrollable = 0.0;
        }
        // production-surface §4：Disabled/Hidden 不绘；Visible 总是；Auto 仅溢出。
        int show = 0;
        if (visibility == "Disabled" || visibility == "Hidden") {
            show = 0;
        } else if (visibility == "Visible") {
            show = 1;
        } else if (scrollable > 0.5) {
            show = 1;
        }
        if (show == 0) {
            return;
        }
        double trackX = x + viewportW - VScrollWidth;
        int scrollHover = WindowHost.ElementGetBool(handle, "IsMouseOver", 0);
        int scrollPressed = ScrollRouter.IsDragging(handle) ? 1 : 0;
        ControlVisual pal = VisualStateManager.ScrollBar(ControlState.Of(1, scrollHover, scrollPressed, 0, 0, 0));
        // 滚动条 chrome 只走 VSM/主题 token——禁止 StateColor 读宿主 Background：
        // ScrollView 常设白底，会吞掉轨道只剩粗拇指，看起来像独立竖条。
        Color track = this.ResolveThemeKey(pal.Track);
        Color thumb = this.ResolveThemeKey(pal.Thumb);
        this.DrawRect(trackX, y, VScrollWidth, viewportH, track);
        double ratio = viewportH / extentH;
        if (ratio > 1.0) {
            ratio = 1.0;
        }
        double thumbH = ratio * viewportH;
        if (thumbH < VScrollMinThumb) {
            thumbH = VScrollMinThumb;
        }
        if (thumbH > viewportH) {
            thumbH = viewportH;
        }
        double travel = viewportH - thumbH;
        double frac = 0.0;
        if (scrollable > 0.0 && travel > 0.0) {
            frac = offset / scrollable;
            if (frac < 0.0) { frac = 0.0; }
            if (frac > 1.0) { frac = 1.0; }
        }
        double thumbY = y + frac * travel;
        double thumbW = VScrollWidth - ControlMetrics.VScrollThumbInset;
        double thumbR = pal.Radius.TopLeft;
        double thumbInset = ControlMetrics.VScrollThumbInset / 2.0;
        this.DrawRoundedRect(trackX + thumbInset, thumbY, thumbW, thumbH, thumbR, thumb);
    }

    /// <summary>
    /// 绘制直角矩形边框（radius=0，1px 细线）——保持既有调用点向后兼容。
    /// 圆角/描边宽度版请用 <see cref="DrawRoundedBorder"/>。
    /// </summary>
    private void DrawRectBorder(double x, double y, double w, double h, Color color) {
        this.DrawRoundedBorder(x, y, w, h, 0.0, (double)RectBorderThickness, color);
    }

    /// <summary>解析 VSM 渐变起止色（类型化 Color）。两端键均有效时返回 true。</summary>
    private bool ResolveGradient(ControlVisual pal, ref Color start, ref Color end) {
        start = Color.Transparent();
        end = Color.Transparent();
        bool hasStart = pal.GradientStart != null && pal.GradientStart.Length > 0;
        bool hasEnd = pal.GradientEnd != null && pal.GradientEnd.Length > 0;
        if (Application.Current != null) {
            if (hasStart) {
                start = Color.Parse(Application.Current.ResolveColor(pal.GradientStart));
            }
            if (hasEnd) {
                end = Color.Parse(Application.Current.ResolveColor(pal.GradientEnd));
            }
        }
        return hasStart && hasEnd;
    }

    /// <summary>
    /// 是否有显式/样式 Background（非 Control DP 默认透明）。
    /// WPF 心智：本地值/隐式样式 Setter 优先于主题 Primary 渐变配方；
    /// 仅「未设底色」时才用 VSM Primary 渐变槽（实为 Color.Primary）作为主按钮 chrome。
    /// </summary>
    private bool HasExplicitBackground(long handle) {
        string raw = WindowHost.ElementGetString(handle, "Background", "");
        if (raw == null || raw.Length == 0) {
            return false;
        }
        // Control.BackgroundProperty 默认 "#00000000"
        if (raw == "#00000000" || raw == "Transparent") {
            return false;
        }
        return true;
    }

    /// <summary>
    /// 状态色解析：显式属性（用户 Style/本地覆盖）优先，否则 VSM 状态资源键默认；
    /// 结果经 MotionEngine 按角色插值后上屏（RFC 037 §3.6）。单一解析根 = Application.Current。
    /// 未设 / 透明背景视为无显式值——回落主题键（禁 DP 默认透明挡住 chrome 配方）。
    /// </summary>
    private Color StateColor(long handle, string prop, string key, int role) {
        string def = "";
        if (Application.Current != null) {
            def = Application.Current.ResolveColor(key);
        }
        string s = WindowHost.ElementGetString(handle, prop, "");
        if (this.IsUnsetBrushMirror(s)) {
            s = def;
        }
        if (s == null || s.Length == 0) {
            return Color.Transparent();
        }
        return MotionEngine.ResolveColor(handle, role, s);
    }

    /// <summary>
    /// 状态色解析（显式时长版本）：同 <see cref="StateColor"/>，但过渡时长由 VSM 每状态
    /// motion 覆写（<see cref="ControlVisual.MotionDuration"/>），实现「hover 跟手 / focus 从容」。
    /// </summary>
    private Color StateColorMotion(long handle, string prop, string key, int role, double durationMs) {
        return this.StateColorMotionCore(handle, prop, key, role, durationMs, false);
    }

    /// <summary>
    /// Chrome 交互态色：Hover/Pressed/Disabled 时 VSM 主题键覆盖静态 Background/Border
    /// （WPF VisualState 覆盖本地 Setter 语义）。Rest 态仍允许显式 Style/本地值优先。
    /// </summary>
    private Color ChromeStateColor(long handle, string prop, string key, int role, double durationMs, bool forceTheme) {
        return this.StateColorMotionCore(handle, prop, key, role, durationMs, forceTheme);
    }

    private Color StateColorMotionCore(long handle, string prop, string key, int role, double durationMs, bool forceTheme) {
        string def = "";
        if (Application.Current != null) {
            def = Application.Current.ResolveColor(key);
        }
        string s = def;
        if (!forceTheme) {
            string mirror = WindowHost.ElementGetString(handle, prop, "");
            if (!this.IsUnsetBrushMirror(mirror)) {
                s = mirror;
            }
        }
        if (s == null || s.Length == 0) {
            return Color.Transparent();
        }
        return MotionEngine.ResolveColorDur(handle, role, s, durationMs);
    }

    /// <summary>平台镜像画刷是否「未设」（空或透明）——与 HasExplicitBackground 同判据。</summary>
    private bool IsUnsetBrushMirror(string raw) {
        if (raw == null || raw.Length == 0) {
            return true;
        }
        if (raw == "#00000000" || raw == "Transparent") {
            return true;
        }
        return false;
    }

    public void RenderElementTree(long rootHandle) {
        if (!_initialized || _pass == null) {
            return;
        }
        if (rootHandle == 0) {
            return;
        }
        Color rootBackground = this.ElementColor(rootHandle, "Background",
            this.ResolveThemeKey(BuiltInTheme.Background));
        this.DrawRect(0.0, 0.0, (double)_dipWidth, (double)_dipHeight, rootBackground);
        // 布局权威收敛：每个元素的绝对 rect 由 Arc 层 LayoutManager 计算并经
        // PlatformTreeSync 同步到平台镜像（LayoutX/Y/Width/Height → layout_*）。
        // 渲染仅消费权威 rect，不再自行推导几何（消除 A2 分叉）。
        this.RenderElementNode(rootHandle);
    }

    /// <summary>
    /// 递归渲染元素 + 其子树。几何一律读该元素权威布局 rect（layout_* 绝对坐标），
    /// 子元素各自持有绝对 rect，故无需自累加位置/尺寸推导。
    /// </summary>
    private void RenderElementNode(long handle) {
        if (handle == 0) {
            return;
        }
        string type = WindowHost.ElementGetTypeName(handle);
        double lx = WindowHost.ElementGetNumber(handle, "LayoutX", 0.0);
        double ly = WindowHost.ElementGetNumber(handle, "LayoutY", 0.0);
        double lw = WindowHost.ElementGetNumber(handle, "LayoutWidth", 0.0);
        double lh = WindowHost.ElementGetNumber(handle, "LayoutHeight", 0.0);

        // 视口裁剪（H2 性能 + 槽位守恒）：元素 rect 完全落在当前裁剪区外时整棵子树
        // 不发命令。ScrollView 的 PushClip 仅约束 GPU scissor 输出，若不剔除屏外
        // 子树，其 uniform 槽位照常消耗（每字形 1 槽）——长页面会耗尽槽位、挤掉
        // 可见尾部内容。留 CullMargin 余量容纳阴影/焦点环等 rect 外溢绘制。
        // 仅 rect 有效（宽高 > 0）才裁剪——未布局元素走后续 fallback 尺寸，交由 scissor 兜底。
        if (lw > 0.0 && lh > 0.0) {
            double clipL = 0.0;
            double clipT = 0.0;
            double clipR = (double)_dipWidth;
            double clipB = (double)_dipHeight;
            if (_clipDepth > 0) {
                clipL = _clipX[_clipDepth - 1];
                clipT = _clipY[_clipDepth - 1];
                clipR = clipL + _clipW[_clipDepth - 1];
                clipB = clipT + _clipH[_clipDepth - 1];
            }
            if (lx >= clipR + CullMargin || ly >= clipB + CullMargin
                || lx + lw <= clipL - CullMargin || ly + lh <= clipT - CullMargin) {
                return;
            }
        }

        // ---- ScrollView：裁剪视口 + 内容（内容 rect 已含滚动偏移）→ 竖滚动条 ----
        if (type == ElScrollView) {
            double voff = WindowHost.ElementGetNumber(handle, "VerticalOffset", 0.0);
            double extentH = WindowHost.ElementGetNumber(handle, "ExtentHeight", 0.0);
            double viewportH = lh;
            if (viewportH <= 0.0) {
                viewportH = WindowHost.ElementGetNumber(handle, "ViewportHeight", 0.0);
            }
            string vis = WindowHost.ElementGetString(handle, "VerticalScrollBarVisibility", "Auto");
            Color scrollViewBackground = this.ElementColor(handle, "Background", this.ColorTransparent());
            this.DrawBackground(scrollViewBackground, lx, ly, lw, viewportH);
            double drawWidth = lw;
            // 预留条宽与绘制同契约：Visible 总是；Auto 仅溢出；Disabled/Hidden 不预留。
            bool reserveBar = false;
            if (vis == "Visible") {
                reserveBar = true;
            } else if (vis != "Disabled" && vis != "Hidden" && extentH > viewportH + 0.5) {
                reserveBar = true;
            }
            if (reserveBar) {
                drawWidth = lw - VScrollWidth;
            }
            this.PushClip(lx, ly, drawWidth, viewportH);
            int childCount = WindowHost.ElementGetChildCount(handle);
            for (int i = 0; i < childCount; i++) {
                long child = WindowHost.ElementGetChild(handle, i);
                this.RenderElementNode(child);
            }
            this.PopClip();
            this.DrawVScrollBar(handle, lx, ly, lw, viewportH, extentH, voff, vis);
            return;
        }

        // ---- StackPanel ----
        if (type == ElStackPanel) {
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            this.DrawBackground(bg, lx, ly, lw, lh);
        }

        // ---- Rectangle ----
        if (type == ElRectangle) {
            double rw = lw;
            if (rw <= 0.0) { rw = 100.0; }
            double rh = lh;
            if (rh <= 0.0) { rh = 100.0; }
            double strokeWidth = WindowHost.ElementGetNumber(handle, "StrokeThickness", 1.0);
            Color fill = this.ElementColor(handle, "Fill", this.ColorTransparent());
            this.DrawRect(lx, ly, rw, rh, fill);
            if (strokeWidth > 0.0) {
                Color stroke = this.ElementColor(handle, "Stroke",
                    this.ResolveThemeKey(BuiltInTheme.TextPrimary));
                this.DrawRectBorder(lx, ly, rw, rh, stroke);
            }
        }

        // ---- TextBlock ----
        if (type == ElTextBlock) {
            string text = WindowHost.ElementGetString(handle, "Text", "");
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            Color fg = this.ElementColor(handle, "Foreground",
                this.ResolveThemeKey(BuiltInTheme.TextPrimary));
            long contentHost = this.TemplateChromeHost(handle);
            if (contentHost != 0 && this.TemplateChromeRole(handle) == "Content") {
                fg = this.TemplateContentForeground(contentHost, fg);
            }
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", 0.0);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            this.DrawText(text, lx, ly, fontSize, bg, fg, family, weight);
        }

        // ---- 模板让位（WPF 语义）：已挂视觉子树的控件跳过内置 chrome 分支 ----
        // 默认模板经 DefaultControlTemplates 挂到隐式 Style；chrome 画在 PART_*。
        // 已迁：Button/Toggle/Check/Radio/TextBox/PasswordBox/Slider/ProgressBar/ComboBox。
        // 无模板回退仅供遗留；禁止为已迁控件新增宿主硬编码 chrome。
        // 未迁：TabControl（Panel 内容子树）/ DataGrid（专属分支 + ApplyTo 清行风险）。
        bool templated = WindowHost.ElementGetChildCount(handle) > 0;

        // ---- Button / ToggleButton（无模板回退；有模板 → PART_Chrome）----
        if ((type == ElButton || type == ElToggleButton) && !templated) {
            string content = WindowHost.ElementGetString(handle, "Content", "");
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            int isChecked = WindowHost.ElementGetBool(handle, "IsChecked", 0);
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", ControlMetrics.FontBodySize);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            // 行高与 DrawText 同源（per-size atlas 度量），禁 8x16 GlyphHeight 缩放公式。
            double scaledGlyphHeight = this.EstTextHeight("Ag", 0.0, fontSize, family);
            if (scaledGlyphHeight <= 0.0) {
                scaledGlyphHeight = fontSize > 0.0 ? fontSize : GlyphHeight;
            }
            double padX = WindowHost.ElementGetNumber(handle, "PaddingX", LayoutPaddingX);
            double padY = WindowHost.ElementGetNumber(handle, "PaddingY", LayoutPaddingY);
            if (padX <= 0.0) { padX = LayoutPaddingX; }
            if (padY < 0.0) { padY = LayoutPaddingY; }
            double estimatedWidthOriginal = this.EstTextWidth(content, padX, fontSize, family, weight);
            double estimatedHeight = scaledGlyphHeight + padY;
            double bw = lw;
            if (bw <= 0.0) { bw = estimatedWidthOriginal; }
            double bh = lh;
            if (bh <= 0.0) { bh = estimatedHeight; }
            int isMouseOver = WindowHost.ElementGetBool(handle, "IsMouseOver", 0);
            int isPressed = WindowHost.ElementGetBool(handle, "IsPressed", 0);
            int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
            ControlState st = ControlState.Of(isEnabled, isMouseOver, isPressed, isFocused, isChecked, 0);
            ControlVisual pal = VisualStateManager.Button(st);
            if (type == ElToggleButton) {
                pal = VisualStateManager.Toggle(st);
            } else {
                string styleKeys = WindowHost.ElementGetString(handle, "StyleKeys", "");
                pal = VisualStateManager.ButtonForStyleKeys(st, styleKeys);
            }
            // Hover/Pressed/Disabled：VSM token 覆盖静态 Background（宿主 Style 不挡交互反馈）。
            bool forceChrome = isEnabled == 0 || isMouseOver != 0 || isPressed != 0;
            if (type == ElToggleButton && isChecked != 0) {
                forceChrome = true;
            }
            // 现代深度反馈：hover/pressed 抬升软阴影 + focus 辉光（DrawSurfaceShadow，圆角贴合）。
            if (pal.Lift.IsVisible) {
                this.DrawSurfaceShadow(lx, ly, bw, bh, pal.Lift.Radius, pal.Lift.Blur, pal.Lift.OffsetY, pal.Lift.Alpha);
            }
            double cr = pal.Radius.Max;
            Color bg = this.ChromeStateColor(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration, forceChrome);
            Color fg = this.StateColorMotion(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground, pal.MotionDuration);
            Color border = this.ChromeStateColor(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration, forceChrome);
            // RFC 037 §3.6：圆角+渐变同一 SDF；Rest 显式 Background 优先生效；交互态走 VSM 纯色。
            Color gradStart = Color.Transparent();
            Color gradEnd = Color.Transparent();
            bool hasGradient = this.ResolveGradient(pal, ref gradStart, ref gradEnd);
            if (!forceChrome && !this.HasExplicitBackground(handle) && hasGradient) {
                this.DrawLinearGradient(lx, ly, bw, bh, gradStart, gradEnd, 0.0, 0.0, 1.0, 0.0, cr);
            } else {
                this.DrawRoundedRect(lx, ly, bw, bh, cr, bg);
            }
            this.DrawRoundedBorder(lx, ly, bw, bh, cr, (double)RectBorderThickness, border);
            // ToggleButton 选中标记（Accent 强调）
            if (type == ElToggleButton && isChecked != 0) {
                Color accent = this.StateColor(handle, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
                double inset = ControlMetrics.SpacingXS;
                double innerCr = cr - inset;
                if (innerCr < 0.0) { innerCr = 0.0; }
                this.DrawRoundedRect(lx + inset, ly + inset, bw - inset * 2.0, bh - inset * 2.0, innerCr, accent);
            }
            // 焦点外晕（FocusRing）+ 辉光（FocusGlow）——仅 IsFocusVisible（键盘模态）
            int isFocusVisible = WindowHost.ElementGetBool(handle, "IsFocusVisible", 0);
            if (isFocusVisible != 0) {
                if (pal.FocusGlow.IsVisible) {
                    this.DrawSurfaceShadow(lx, ly, bw, bh, pal.FocusGlow.Radius, pal.FocusGlow.Blur,
                                           pal.FocusGlow.OffsetY, pal.FocusGlow.Alpha);
                }
                Color ring = this.StateColor(handle, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
                this.DrawRoundedBorder(
                    lx - ControlMetrics.FocusRingOutset,
                    ly - ControlMetrics.FocusRingOutset,
                    bw + ControlMetrics.FocusRingOutset * 2.0,
                    bh + ControlMetrics.FocusRingOutset * 2.0,
                    cr + ControlMetrics.FocusRingOutset,
                    pal.FocusRingWidth, ring);
            }
            // 按钮文字居中（textWidth = 原始文本宽 - padding；EstTextWidth 对 padding
            // 严格可加，故与独立无 padding 度量数学等价）
            double textWidth = estimatedWidthOriginal - padX;
            double textX = lx + (bw - textWidth) / 2.0;
            if (textX < lx + ControlMetrics.SpacingXS) { textX = lx + ControlMetrics.SpacingXS; }
            double textY = ly + (bh - scaledGlyphHeight) / 2.0;
            this.DrawText(content, textX, textY, fontSize, this.ColorTransparent(), fg, family, weight);
        }

        // ---- CheckBox（无模板回退；有模板 → PART_Glyph + PART_Content）----
        if (type == ElCheckBox && !templated) {
            string content = WindowHost.ElementGetString(handle, "Content", "");
            int isChecked = WindowHost.ElementGetBool(handle, "IsChecked", 0);
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            int isMouseOver = WindowHost.ElementGetBool(handle, "IsMouseOver", 0);
            int isPressed = WindowHost.ElementGetBool(handle, "IsPressed", 0);
            int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", ControlMetrics.FontBodySize);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            double box = ControlMetrics.ToggleBoxSize;
            ControlVisual pal = VisualStateManager.Toggle(ControlState.Of(isEnabled, isMouseOver, isPressed, isFocused, isChecked, 0));
            bool forceChrome = isEnabled == 0 || isMouseOver != 0 || isPressed != 0 || isChecked != 0;
            double cr = pal.Radius.Max;
            Color boxBackground = this.ChromeStateColor(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration, forceChrome);
            Color boxBorder = this.ChromeStateColor(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration, forceChrome);
            // checked 态：无显式 Background 时用主题渐变；否则纯色。描边仅为边框。
            Color gradStart = Color.Transparent();
            Color gradEnd = Color.Transparent();
            bool hasGradient = this.ResolveGradient(pal, ref gradStart, ref gradEnd);
            if (isChecked != 0
                && !this.HasExplicitBackground(handle)
                && hasGradient) {
                this.DrawLinearGradient(lx, ly, box, box, gradStart, gradEnd, 0.0, 0.0, 1.0, 0.0, cr);
            } else {
                this.DrawRoundedRect(lx, ly, box, box, cr, boxBackground);
            }
            this.DrawRoundedBorder(lx, ly, box, box, cr, (double)RectBorderThickness, boxBorder);
            if (isChecked != 0) {
                Color accent = this.StateColor(handle, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
                double inset = ControlMetrics.SpacingXS;
                double innerCr = cr - inset;
                if (innerCr < 0.0) { innerCr = 0.0; }
                this.DrawRoundedRect(lx + inset, ly + inset, box - inset * 2.0, box - inset * 2.0, innerCr, accent);
            }
            if (WindowHost.ElementGetBool(handle, "IsFocusVisible", 0) != 0) {
                Color ring = this.StateColor(handle, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
                this.DrawRoundedBorder(
                    lx - ControlMetrics.FocusRingOutset,
                    ly - ControlMetrics.FocusRingOutset,
                    box + ControlMetrics.FocusRingOutset * 2.0,
                    box + ControlMetrics.FocusRingOutset * 2.0,
                    cr + ControlMetrics.FocusRingOutset,
                    pal.FocusRingWidth, ring);
            }
            Color fg = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
            double textX = lx + box + ControlMetrics.ToggleLabelGap;
            this.DrawText(content, textX, ly + (box - GlyphHeight) / 2.0, fontSize, this.ColorTransparent(), fg, family, weight);
        }

        // ---- RadioButton（无模板回退；有模板 → PART_Glyph 圆形）----
        if (type == ElRadioButton && !templated) {
            string content = WindowHost.ElementGetString(handle, "Content", "");
            int isChecked = WindowHost.ElementGetBool(handle, "IsChecked", 0);
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            int isMouseOver = WindowHost.ElementGetBool(handle, "IsMouseOver", 0);
            int isPressed = WindowHost.ElementGetBool(handle, "IsPressed", 0);
            int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", ControlMetrics.FontBodySize);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            double box = ControlMetrics.ToggleBoxSize;
            double cr = box / 2.0;
            ControlVisual pal = VisualStateManager.Toggle(ControlState.Of(isEnabled, isMouseOver, isPressed, isFocused, isChecked, 0));
            bool forceChrome = isEnabled == 0 || isMouseOver != 0 || isPressed != 0 || isChecked != 0;
            Color boxBackground = this.ChromeStateColor(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration, forceChrome);
            Color boxBorder = this.ChromeStateColor(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration, forceChrome);
            this.DrawRoundedRect(lx, ly, box, box, cr, boxBackground);
            this.DrawRoundedBorder(lx, ly, box, box, cr, (double)RectBorderThickness, boxBorder);
            if (isChecked != 0) {
                Color accent = this.StateColor(handle, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
                double inner = ControlMetrics.SpacingMD / 2.0;
                double innerCr = inner / 2.0;
                double inset = (box - inner) / 2.0;
                this.DrawRoundedRect(lx + inset, ly + inset, inner, inner, innerCr, accent);
            }
            if (WindowHost.ElementGetBool(handle, "IsFocusVisible", 0) != 0) {
                Color ring = this.StateColor(handle, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
                this.DrawRoundedBorder(
                    lx - ControlMetrics.FocusRingOutset,
                    ly - ControlMetrics.FocusRingOutset,
                    box + ControlMetrics.FocusRingOutset * 2.0,
                    box + ControlMetrics.FocusRingOutset * 2.0,
                    cr + ControlMetrics.FocusRingOutset,
                    pal.FocusRingWidth, ring);
            }
            Color fg = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
            double textX = lx + box + ControlMetrics.ToggleLabelGap;
            this.DrawText(content, textX, ly + (box - GlyphHeight) / 2.0, fontSize, this.ColorTransparent(), fg, family, weight);
        }

        // ---- TextBox / PasswordBox：有模板时先 PART_Chrome，再宿主文本层（禁被壳盖住）----
        if (type == ElTextBox || type == ElPasswordBox) {
            if (templated) {
                // 子树（PART_Chrome）先画；文本/选区/caret 后画，否则不透明壳盖住内容层。
                int tbChildCount = WindowHost.ElementGetChildCount(handle);
                for (int tbi = 0; tbi < tbChildCount; tbi++) {
                    long tbChild = WindowHost.ElementGetChild(handle, tbi);
                    this.RenderElementNode(tbChild);
                }
                this.DrawTextBoxContentLayer(handle, lx, ly, lw, lh);
                return;
            }
            string text = WindowHost.ElementGetString(handle, "Text", "");
            string placeholder = WindowHost.ElementGetString(handle, "Placeholder", "");
            string composition = WindowHost.ElementGetString(handle, "CompositionText", "");
            double caretIdx = WindowHost.ElementGetNumber(handle, "CaretIndex", 0.0);
            int caretIndex = (int)caretIdx;
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
            bool empty = text == null || text.Length == 0;
            bool isPlaceholder = empty && isFocused == 0
                && placeholder != null && placeholder.Length > 0;
            string display = isPlaceholder ? placeholder : (text != null ? text : "");
            // 组字预览：composition 并入显示串，以下划线区分 committed 文本。
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
            // 行高与 DrawText 同源（per-size atlas 度量），禁 8x16 GlyphHeight 缩放公式。
            double scaledGlyphHeight = this.EstTextHeight("Ag", 0.0, fontSize, family);
            if (scaledGlyphHeight <= 0.0) {
                scaledGlyphHeight = fontSize > 0.0 ? fontSize : GlyphHeight;
            }
            double estimatedWidth = this.EstTextWidth(
                display != null && display.Length > 0 ? display : "Ag", 16.0, fontSize, family, weight);
            double estimatedHeight = scaledGlyphHeight + 8.0;
            double iw = lw;
            if (iw <= 0.0) { iw = estimatedWidth; }
            double ih = lh;
            if (ih <= 0.0) { ih = estimatedHeight; }
            ControlVisual pal = VisualStateManager.TextBox(ControlState.Of(isEnabled, 0, 0, isFocused, 0, 0));
            double cr = pal.Radius.Max;
            Color bg = this.StateColorMotion(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration);
            Color border = this.StateColorMotion(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration);
            this.DrawRoundedRect(lx, ly, iw, ih, cr, bg);
            this.DrawRoundedBorder(lx, ly, iw, ih, cr, (double)RectBorderThickness, border);
            if (WindowHost.ElementGetBool(handle, "IsFocusVisible", 0) != 0) {
                if (pal.FocusGlow.IsVisible) {
                    this.DrawSurfaceShadow(lx, ly, iw, ih, pal.FocusGlow.Radius, pal.FocusGlow.Blur,
                                           pal.FocusGlow.OffsetY, pal.FocusGlow.Alpha);
                }
                Color ring = this.StateColor(handle, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
                this.DrawRoundedBorder(
                    lx - ControlMetrics.FocusRingOutset,
                    ly - ControlMetrics.FocusRingOutset,
                    iw + ControlMetrics.FocusRingOutset * 2.0,
                    ih + ControlMetrics.FocusRingOutset * 2.0,
                    cr + ControlMetrics.FocusRingOutset,
                    pal.FocusRingWidth, ring);
            }
            Color fg;
            if (isPlaceholder) {
                fg = this.ResolveThemeKey(pal.Placeholder);
            } else {
                fg = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
            }
            // InputMetrics 同源：textX 使 DrawText 内部 pen = PenOriginX。
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
            // 组字下划线预览：DrawText 起始 pen 自带 MinTextPaddingX/2 内缩，补齐对齐。
            if (hasComposition) {
                double compX = penOriginX + this.EstTextWidth(compPrefix, 0.0, fontSize, family, weight);
                double compW = this.EstTextWidth(composition, 0.0, fontSize, family, weight);
                double underlineY = textY + scaledGlyphHeight - 2.0;
                if (compW > 0.0) {
                    this.DrawRect(compX, underlineY, compW, 1.0,
                        this.ResolveThemeKey(BuiltInTheme.TextPrimary));
                }
            }
            // 软件 caret 竖线（焦点即画——空 Text/placeholder 态画于文本起点，桌面惯例；
            // 相位由 FramePump.CaretBlinkOn 控制）：与 composition 下划线同源度量。
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
            return;
        }

        // ---- Image（RFC 029 M2：GIF/SVG/静态位图解码纹理采样；无纹理回退占位）----
        if (type == ElImage) {
            // 解码纹理经 TextureId 镜像（Image 组件 UploadFrame/SyncMirrorTexture 写入）
            // 采样；GIF 动画逐帧更新同一纹理，本分支无需感知动画状态。
            int textureId = (int)WindowHost.ElementGetNumber(handle, "TextureId", 0.0);
            int tw = 0;
            int th = 0;
            bool hasTex = textureId > 0 && this.GetTextureSize(textureId, out tw, out th);
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            if (bg.A > 0.001) {
                this.DrawRect(lx, ly, lw, lh, bg);
            }
            if (hasTex) {
                // Stretch（对标 WPF Stretch）：与 VideoSurface 同源 UV 计算。
                string stretch = WindowHost.ElementGetString(handle, "Stretch", "None");
                double dtw = (double)tw;
                double dth = (double)th;
                double sw = lw;
                double sh = lh;
                double u0 = 0.0;
                double v0 = 0.0;
                double u1 = 1.0;
                double v1 = 1.0;
                if (dtw > 0.0 && dth > 0.0 && lw > 0.0 && lh > 0.0) {
                    if (stretch == "Uniform") {
                        double scale = (lw / dtw) < (lh / dth) ? (lw / dtw) : (lh / dth);
                        sw = dtw * scale;
                        sh = dth * scale;
                    } else if (stretch == "UniformToFill") {
                        double scale = (lw / dtw) > (lh / dth) ? (lw / dtw) : (lh / dth);
                        double swSrc = lw / scale;
                        double shSrc = lh / scale;
                        u0 = (dtw - swSrc) / 2.0 / dtw;
                        u1 = u0 + swSrc / dtw;
                        v0 = (dth - shSrc) / 2.0 / dth;
                        v1 = v0 + shSrc / dth;
                    }
                    // None/Fill：全源映射到全目标（元素尺寸由布局给定）。
                }
                this.DrawTexture(textureId, lx, ly, sw, sh, u0, v0, u1, v1, 1.0);
            } else {
                // 占位：未解码/解码失败/无源时灰底 + 边框（首版占位语义保留）。
                if (bg.A <= 0.001) {
                    this.DrawRect(lx, ly, lw, lh,
                        this.ResolveThemeKey(BuiltInTheme.ImageFill));
                }
                this.DrawRectBorder(lx, ly, lw, lh,
                    this.ResolveThemeKey(BuiltInTheme.ImageBorder));
            }
        }

        // ---- VideoSurface（RFC 037 references/texture-surface）----
        if (type == ElVideoSurface) {
            int textureId = (int)WindowHost.ElementGetNumber(handle, "TextureId", 0.0);
            int tw = 0;
            int th = 0;
            bool hasTex = textureId > 0 && this.GetTextureSize(textureId, out tw, out th);
            if (hasTex) {
                // Stretch 映射走共享 StretchMapper（双宿主唯一实现，RFC 037 §10 G1）。
                string stretch = WindowHost.ElementGetString(handle, "Stretch", "None");
                StretchMapping m = StretchMapper.Compute(UIEnumConverter.ParseStretch(stretch),
                                                         (double)tw, (double)th, lx, ly, lw, lh);
                this.DrawTexture(textureId, m.X, m.Y, m.Width, m.Height, m.U0, m.V0, m.U1, m.V1, 1.0);
            }
            Color bg = this.ElementColor(handle, "Background", this.ColorTransparent());
            if (bg.A > 0.001) {
                this.DrawRect(lx, ly, lw, lh, bg);
            }
        }

        // ---- Slider（无模板回退；有模板 → PART_Chrome Surface 画轨/thumb）----
        if (type == ElSlider && !templated) {
            this.DrawSliderChrome(handle, lx, ly, lw, lh);
        }

        // ---- ProgressBar（无模板回退；有模板 → PART Surface；含 IsIndeterminate）----
        if (type == ElProgressBar && !templated) {
            this.DrawProgressBarChrome(handle, lx, ly, lw, lh);
        }

        // ---- ComboBox（折叠态；选项属 Popup 轨。有模板：先 PART 壳，再文本/chevron）----
        if (type == ElComboBox) {
            if (templated) {
                int cbChildCount = WindowHost.ElementGetChildCount(handle);
                for (int cbi = 0; cbi < cbChildCount; cbi++) {
                    long cbChild = WindowHost.ElementGetChild(handle, cbi);
                    this.RenderElementNode(cbChild);
                }
                this.DrawComboBoxContentLayer(handle, lx, ly, lw, lh);
                return;
            }
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
            double cr = pal.Radius.Max;
            Color bg = this.StateColorMotion(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration);
            Color border = this.StateColorMotion(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration);
            this.DrawRoundedRect(lx, ly, iw, ih, cr, bg);
            this.DrawRoundedBorder(lx, ly, iw, ih, cr, (double)RectBorderThickness, border);
            this.DrawComboBoxContentLayer(handle, lx, ly, lw, lh);
            return;
        }

        // ---- DataGrid（RFC 037 §4 · M-VZ4：表头带 + 斑马纹 + 选中 Accent 高亮 + 列分隔；
        //      行镜像由本分支内联消费，不走通用递归）----
        if (type == ElDataGrid) {
            this.RenderDataGrid(handle, lx, ly, lw, lh);
            return;
        }

        // ---- CodeEditor（RFC 037 §4 M-CE1：视口虚拟化 DrawList → ExecuteDrawList）----
        if (type == ElCodeEditor) {
            Color bg = this.ElementColor(handle, "Background",
                this.ResolveThemeKey(BuiltInTheme.Surface));
            this.DrawBackground(bg, lx, ly, lw, lh);
            IFrameDrawListProvider provider = FrameDrawListRouter.Lookup(handle);
            if (provider != null) {
                DrawList list = provider.BuildFrameDrawList();
                if (list != null && list.Count > 0 && lw > 0.0 && lh > 0.0) {
                    this.PushClip(lx, ly, lw, lh);
                    this.ExecuteDrawList(list, lx, ly);
                    this.PopClip();
                }
            }
            return;
        }

        // ---- TabControl（内置页签栏：测宽左对齐 + 溢出裁剪滚动 + 选中 Accent 底线）----
        if (type == "TabControl") {
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            this.DrawBackground(bg, lx, ly, lw, lh);
            int tabCount = (int)WindowHost.ElementGetNumber(handle, "TabCount", 0.0);
            double barH = WindowHost.ElementGetNumber(handle, "HeaderBarHeight",
                ControlMetrics.TabHeaderBarHeight);
            if (barH <= 0.0) {
                barH = ControlMetrics.TabHeaderBarHeight;
            }
            int selected = (int)WindowHost.ElementGetNumber(handle, "SelectedIndex", 0.0);
            double scrollOff = WindowHost.ElementGetNumber(handle, "HeaderScrollOffset", 0.0);
            if (scrollOff < 0.0) {
                scrollOff = 0.0;
            }
            if (tabCount > 0 && lw > 0.0) {
                Color barBg = this.ResolveThemeKey(BuiltInTheme.SurfaceStripe);
                Color border = this.ResolveThemeKey(BuiltInTheme.Border);
                Color accent = this.ResolveThemeKey(BuiltInTheme.Primary);
                Color fg = this.ResolveThemeKey(BuiltInTheme.TextPrimary);
                Color fgMuted = this.ResolveThemeKey(BuiltInTheme.TextSecondary);
                this.DrawRect(lx, ly, lw, barH, barBg);
                this.DrawRect(lx, ly + barH - ControlMetrics.BorderWidth, lw,
                    ControlMetrics.BorderWidth, border);
                double fontSize = ControlMetrics.TabHeaderFontSize;
                int family = this.ResolveFontFamily("");
                int weight = this.ResolveFontWeight("Normal");
                this.PushClip(lx, ly, lw, barH);
                double cursorX = lx - scrollOff;
                double fallbackCell = lw / (double)tabCount;
                int ti = 0;
                while (ti < tabCount) {
                    string header = WindowHost.ElementGetString(handle, "Header" + ti, "");
                    if (header == null || header.Length == 0) {
                        header = "Tab " + (ti + 1).ToString();
                    }
                    double cellW = WindowHost.ElementGetNumber(handle, "HeaderWidth" + ti, 0.0);
                    if (cellW <= 0.0) {
                        cellW = fallbackCell;
                    }
                    bool isSel = ti == selected;
                    Color labelFg = isSel ? fg : fgMuted;
                    double textW = this.EstTextWidth(header, 0.0, fontSize, family, weight);
                    double textX = cursorX + (cellW - textW) / 2.0;
                    if (textX < cursorX + ControlMetrics.SpacingXS) {
                        textX = cursorX + ControlMetrics.SpacingXS;
                    }
                    double textY = ly + (barH - fontSize) / 2.0 - ControlMetrics.TabLabelNudgeY;
                    this.DrawText(header, textX, textY, fontSize, this.ColorTransparent(),
                        labelFg, family, isSel ? 1 : weight);
                    if (isSel) {
                        this.DrawRect(
                            cursorX + ControlMetrics.SpacingXS,
                            ly + barH - ControlMetrics.TabIndicatorInsetBottom,
                            cellW - ControlMetrics.SpacingSM,
                            ControlMetrics.FocusRingWidth,
                            accent);
                    }
                    cursorX = cursorX + cellW;
                    ti++;
                }
                this.PopClip();
            }
            int childCountTabs = WindowHost.ElementGetChildCount(handle);
            for (int ci = 0; ci < childCountTabs; ci++) {
                long child = WindowHost.ElementGetChild(handle, ci);
                this.RenderElementNode(child);
            }
            return;
        }

        // ---- TreeViewItem（Header 条 chrome：选中高亮 + 展开三角 + 标题；子树递归）----
        if (type == ElTreeViewItem) {
            double headerH = WindowHost.ElementGetNumber(handle, "HeaderHeight", ControlMetrics.TreeRowHeight);
            if (headerH <= 0.0) {
                headerH = ControlMetrics.TreeRowHeight;
            }
            int isSelected = WindowHost.ElementGetBool(handle, "IsSelected", 0);
            int isExpanded = WindowHost.ElementGetBool(handle, "IsExpanded", 0);
            int hasItems = WindowHost.ElementGetBool(handle, "HasItems", 0);
            string header = WindowHost.ElementGetString(handle, "Header", "");
            if (header == null) {
                header = "";
            }
            ControlVisual pal = VisualStateManager.ListBoxItem(
                ControlState.Of(1, 0, 0, 0, 0, isSelected));
            Color rowBg = this.ResolveThemeKey(pal.Background);
            Color fg = this.ResolveThemeKey(pal.Foreground);
            Color accent = this.ResolveThemeKey(pal.Accent);
            double rowW = lw;
            if (rowW <= 0.0) {
                rowW = 1.0;
            }
            if (rowBg.A > 0.001) {
                this.DrawRoundedRect(lx, ly, rowW, headerH, pal.Radius.Max, rowBg);
            }
            if (isSelected != 0 && accent.A > 0.001) {
                this.DrawRect(lx, ly, ControlMetrics.SpacingXS, headerH, accent);
            }
            double expanderW = WindowHost.ElementGetNumber(handle, "ExpanderWidth",
                ControlMetrics.TreeExpanderWidth);
            if (expanderW <= 0.0) {
                expanderW = ControlMetrics.TreeExpanderWidth;
            }
            if (hasItems != 0) {
                Color chevron = this.ResolveThemeKey(BuiltInTheme.TextSecondary);
                double cx = lx + expanderW * 0.5;
                double cy = ly + headerH * 0.5;
                if (isExpanded != 0) {
                    // 展开：朝下三角（三横线近似）
                    this.DrawRect(cx - 4.0, cy - 1.0, 8.0, 2.0, chevron);
                    this.DrawRect(cx - 2.5, cy + 1.5, 5.0, 2.0, chevron);
                    this.DrawRect(cx - 1.0, cy + 4.0, 2.0, 2.0, chevron);
                } else {
                    // 折叠：朝右三角
                    this.DrawRect(cx - 1.0, cy - 4.0, 2.0, 8.0, chevron);
                    this.DrawRect(cx + 1.5, cy - 2.5, 2.0, 5.0, chevron);
                    this.DrawRect(cx + 4.0, cy - 1.0, 2.0, 2.0, chevron);
                }
            }
            double fontSize = ControlMetrics.FontBodySize;
            int family = this.ResolveFontFamily("");
            int weight = this.ResolveFontWeight("Normal");
            double textX = lx + expanderW + ControlMetrics.SpacingSM;
            double textH = this.EstTextHeight("Ag", 0.0, fontSize, family);
            if (textH <= 0.0) {
                textH = fontSize;
            }
            double textY = ly + (headerH - textH) / 2.0;
            this.DrawText(header, textX, textY, fontSize, this.ColorTransparent(), fg, family, weight);
            int tvChildCount = WindowHost.ElementGetChildCount(handle);
            for (int tvi = 0; tvi < tvChildCount; tvi++) {
                long tvChild = WindowHost.ElementGetChild(handle, tvi);
                this.RenderElementNode(tvChild);
            }
            return;
        }

        // ---- Border（布局装饰；模板 PART_* 走宿主 VSM chrome）----
        if (type == ElBorder) {
            string chromeRole = this.TemplateChromeRole(handle);
            long chromeHost = this.TemplateChromeHost(handle);
            if (chromeHost != 0 && (chromeRole == "Surface" || chromeRole == "Glyph")) {
                this.DrawTemplateChromePart(handle, chromeHost, chromeRole, lx, ly, lw, lh);
            } else {
                double cr = WindowHost.ElementGetNumber(handle, "CornerRadius", ControlMetrics.ControlRadius);
                double bt = WindowHost.ElementGetNumber(handle, "BorderThicknessUniform", ControlMetrics.BorderWidth);
                Color bg = this.ElementColor(handle, "Background", this.ResolveThemeKey(BuiltInTheme.Surface));
                Color border = this.ElementColor(handle, "BorderBrush", this.ResolveThemeKey(BuiltInTheme.Border));
                if (bg.A > 0.001) {
                    this.DrawRoundedRect(lx, ly, lw, lh, cr, bg);
                }
                if (bt > 0.0 && border.A > 0.001) {
                    this.DrawRoundedBorder(lx, ly, lw, lh, cr, bt, border);
                }
            }
        }

        // ---- VisualHost / LayoutShell（Grid/DockPanel/WrapPanel/Canvas/ListView）----
        if (type == ElVisualHost || this.IsLayoutShell(type)) {
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            double cw = lw;
            double ch = lh;
            this.DrawBackground(bg, lx, ly, cw, ch);
            // ListView / TreeView：裁剪项宿主，防错位行画出视口。
            if ((type == ElListView || type == ElTreeView) && lw > 0.0 && lh > 0.0) {
                this.PushClip(lx, ly, lw, lh);
                int lvChildCount = WindowHost.ElementGetChildCount(handle);
                for (int lvi = 0; lvi < lvChildCount; lvi++) {
                    long lvChild = WindowHost.ElementGetChild(handle, lvi);
                    this.RenderElementNode(lvChild);
                }
                this.PopClip();
                return;
            }
        }

        // ---- Window / Element / 未知容器：仅背景 ----
        if (type == ElWindow || type == ElElement) {
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            this.DrawBackground(bg, lx, ly, lw, lh);
        }

        // ---- Popup 层根 / 蒙层（RFC 037 Popup 轨）：仅背景，子树走通用递归。
        //      多弹层 Z 序：层根经 Open→ElementAddChild 挂/移至窗口平台根
        //      children 末尾；painter's algorithm 后画在上，与 hit_test 逆序同源。
        //      蒙层背景由 Popup.Open 直写平台镜像（公共尾部不识 Panel.Background）。----
        if (type == ElPopupLayer || type == ElPopupBackdrop) {
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            this.DrawBackground(bg, lx, ly, lw, lh);
        }

        // ---- 通用递归：子元素各自持有绝对 rect，直接渲染 ----
        int childCount = WindowHost.ElementGetChildCount(handle);
        for (int i = 0; i < childCount; i++) {
            long child = WindowHost.ElementGetChild(handle, i);
            this.RenderElementNode(child);
        }
    }

    /// <summary>
    /// 容器背景绘制：布局权威 rect 非零且背景不透明时绘制。
    /// 背景在递归子元素之前绘制，保证位于子元素下方（back-to-front）。
    /// </summary>
    private void DrawBackground(Color color, double x, double y, double w, double h) {
        if (w <= 0.0 || h <= 0.0) {
            return;
        }
        if (color.A > 0.001) {
            this.DrawRect(x, y, w, h, color);
        }
    }

    /// <summary>
    /// 主题键 → Color（Application 未起或键缺失时按键名保底解析，可绘制）。
    /// 结构色（表头带/斑马纹）无控件显式属性面，直连主题解析。
    /// </summary>
    private Color ResolveThemeKey(string key) {
        if (Application.Current != null) {
            string hex = Application.Current.ResolveColor(key);
            if (hex != null && hex.Length > 0) {
                return Color.Parse(hex);
            }
        }
        return Color.Parse(key);
    }

    /// <summary>
    /// 码点安全省略截断：文本超 maxW 时按 UTF-8 码点边界二分前缀 + "..."。
    /// 二分边界先经 Utf8DecodeAt 收集（避免切在多字节字符中间出 tofu）。
    /// </summary>
    private string ClipTextToWidth(string text, double maxW, double fontSize, int familyIdx, int weight) {
        if (text == null || text.Length == 0 || maxW <= 4.0) {
            return "";
        }
        double w = this.EstTextWidth(text, 0.0, fontSize, familyIdx, weight);
        if (w <= maxW) {
            return text;
        }
        double budget = maxW - this.EstTextWidth("...", 0.0, fontSize, familyIdx, weight);
        if (budget < 4.0) {
            return "";
        }
        // 收集码点边界（前缀结束位置集合：0..len 的合法切点）
        List<int> bounds = new List<int>();
        bounds.Add(0);
        int mi = 0;
        int textLength = text.Length;
        while (mi < textLength) {
            int mcp = 0;
            int mn = this.Utf8DecodeAt(text, mi, out mcp);
            if (mn == 0) {
                break;
            }
            mi += mn;
            bounds.Add(mi);
        }
        // 边界数组上二分最大前缀（宽度对前缀长度单调 → 可二分）
        int lo = 0;
        int hi = bounds.Count - 1;
        while (lo < hi) {
            int mid = (lo + hi + 1) / 2;
            string prefix = text.Substring(0, bounds[mid]);
            if (this.EstTextWidth(prefix, 0.0, fontSize, familyIdx, weight) <= budget) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        if (lo == 0) {
            return "";
        }
        return text.Substring(0, bounds[lo]) + "...";
    }

    /// <summary>
    /// DataGrid 专属渲染：整格底色 + 外框 → 表头带（Stripe 底 + 底分隔线 + 列头文本）→
    /// 行区裁剪内斑马纹/选中 Accent 行 + 单元格文本（列宽省略截断）→ 列分隔线（通高）。
    /// 列几何：固定宽（Width{i}&gt;0）累计在前，auto 列均分剩余宽；行几何读行镜像权威
    /// layout_*（ItemIndex&lt;0 的超编折叠行跳过）。
    /// </summary>
    private void RenderDataGrid(long handle, double lx, double ly, double lw, double lh) {
        int colCount = (int)WindowHost.ElementGetNumber(handle, "ColumnCount", 0.0);
        double headerH = WindowHost.ElementGetNumber(handle, "HeaderHeight", ControlMetrics.ControlHeight);
        double stride = WindowHost.ElementGetNumber(handle, "RowHeight", ControlMetrics.ControlHeight);
        if (stride <= 0.0) {
            stride = ControlMetrics.ControlHeight;
        }
        int selectedIndex = (int)WindowHost.ElementGetNumber(handle, "SelectedIndex", -1.0);
        int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
        double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", ControlMetrics.FontBodySize);
        int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
        int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
        double gw = lw > 0.0 ? lw : 320.0;
        double gh = lh > 0.0 ? lh : headerH + stride;
        double glyphHeight = fontSize > 0.0 ? fontSize : GlyphHeight;

        // 列区间：固定宽累计 → auto 列均分剩余（无剩余时给最小 64 防塌缩）
        List<double> colX = new List<double>();
        List<double> colW = new List<double>();
        double fixedTotal = 0.0;
        int autoCount = 0;
        int ci = 0;
        while (ci < colCount) {
            double cw = WindowHost.ElementGetNumber(handle, "Width" + ci, 0.0);
            if (cw > 0.0) {
                fixedTotal += cw;
            } else {
                autoCount++;
            }
            ci++;
        }
        double autoW = 64.0;
        if (autoCount > 0) {
            double remain = gw - fixedTotal;
            if (remain > 0.0) {
                autoW = remain / (double)autoCount;
            }
        }
        double acc = lx;
        ci = 0;
        while (ci < colCount) {
            double cw = WindowHost.ElementGetNumber(handle, "Width" + ci, 0.0);
            if (cw <= 0.0) {
                cw = autoW;
            }
            colX.Add(acc);
            colW.Add(cw);
            acc += cw;
            ci++;
        }

        // 主题色：整格底色显式 Background 优先；表头带/斑马为结构色（主题键直连）；
        // 分隔线/外框经 StateColor（显式 BorderBrush 可定制，MotionEngine 插值）。
        Color surface = this.ResolveThemeKey(BuiltInTheme.Surface);
        Color gridBackground = this.ElementColor(handle, "Background", surface);
        Color stripe = this.ResolveThemeKey(BuiltInTheme.SurfaceStripe);
        Color headerForeground = this.ResolveThemeKey(BuiltInTheme.TextSecondary);
        Color border = this.StateColor(handle, "BorderBrush", BuiltInTheme.Border, MotionEngine.RoleBorder);

        // 整格底 + 表头带 + 表头底分隔线
        this.DrawRect(lx, ly, gw, gh, gridBackground);
        this.DrawRect(lx, ly, gw, headerH, stripe);
        this.DrawRect(lx, ly + headerH - 1.0, gw, 1.0, border);

        // 行区（表头恒定置顶）：裁剪内画窗口行（斑马/选中）+ 单元格文本
        double rowsH = gh - headerH;
        if (rowsH > 0.0) {
            this.PushClip(lx, ly + headerH, gw, rowsH);
            int childCount = WindowHost.ElementGetChildCount(handle);
            int ri = 0;
            while (ri < childCount) {
                long rowHandle = WindowHost.ElementGetChild(handle, ri);
                ri++;
                if (rowHandle == 0) {
                    continue;
                }
                int rowIndex = (int)WindowHost.ElementGetNumber(rowHandle, "ItemIndex", -1.0);
                if (rowIndex < 0) {
                    continue; // 超编折叠行
                }
                double rly = WindowHost.ElementGetNumber(rowHandle, "LayoutY", 0.0);
                double rlh = WindowHost.ElementGetNumber(rowHandle, "LayoutHeight", stride);
                bool isSelected = rowIndex == selectedIndex;
                // 行配方：选中 = Accent 整行填充 + OnAccent 文本（管理后台语义）
                ControlVisual rowPal = VisualStateManager.DataGridRow(
                    ControlState.Of(isEnabled, 0, 0, 0, 0, isSelected ? 1 : 0));
                Color rowBackground = this.StateColor(rowHandle, "Background", rowPal.Background,
                    MotionEngine.RoleBackground);
                Color rowForeground = this.StateColor(rowHandle, "Foreground", rowPal.Foreground,
                    MotionEngine.RoleForeground);
                double bgAlpha = rowBackground.A;
                // 斑马纹：奇数行 Stripe 底（选中/hover 配方非透明时优先覆盖）
                if (bgAlpha <= 0.001 && (rowIndex % 2) == 1) {
                    this.DrawRect(lx, rly, gw, rlh, stripe);
                } else if (bgAlpha > 0.001) {
                    this.DrawRect(lx, rly, gw, rlh, rowBackground);
                }
                // 单元格文本：列 x + 8 内缩、垂直居中、列宽省略截断
                double textY = rly + (rlh - glyphHeight) / 2.0;
                int cj = 0;
                while (cj < colCount) {
                    string cell = WindowHost.ElementGetString(rowHandle, "C" + cj, "");
                    string clipped = this.ClipTextToWidth(cell, colW[cj] - ControlMetrics.SpacingLG, fontSize, family, weight);
                    if (clipped.Length > 0) {
                        this.DrawText(clipped, colX[cj] + ControlMetrics.SpacingSM, textY, fontSize,
                            this.ColorTransparent(), rowForeground, family, weight);
                    }
                    cj++;
                }
            }
            this.PopClip();
        }

        // 表头列文本（默认粗体；用户 FontWeight 显式设置时尊重用户值）
        int headerWeight = weight != 0 ? weight : 1;
        double headerTextY = ly + (headerH - glyphHeight) / 2.0;
        ci = 0;
        while (ci < colCount) {
            string headerText = WindowHost.ElementGetString(handle, "Header" + ci, "");
            string clippedHeader = this.ClipTextToWidth(headerText, colW[ci] - ControlMetrics.SpacingLG, fontSize, family, headerWeight);
            if (clippedHeader.Length > 0) {
                this.DrawText(clippedHeader, colX[ci] + ControlMetrics.SpacingSM, headerTextY, fontSize,
                    this.ColorTransparent(), headerForeground, family, headerWeight);
            }
            ci++;
        }

        // 列分隔线（通高：表头 + 行区）+ 外框
        ci = 1;
        while (ci < colCount) {
            this.DrawRect(colX[ci], ly, 1.0, gh, border);
            ci++;
        }
        this.DrawRectBorder(lx, ly, gw, gh, border);
    }
}
