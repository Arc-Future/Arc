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
        Color track = this.StateColor(handle, "Background", pal.Background, MotionEngine.RoleBackground);
        Color thumb = this.StateColor(handle, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
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
        double thumbW = VScrollWidth - 4.0;
        double thumbR = pal.Radius.TopLeft;
        this.DrawRoundedRect(trackX + 2.0, thumbY, thumbW, thumbH, thumbR, thumb);
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
    /// 仅「未设底色」时才用 VSM AccentGradient 作为主按钮 chrome。
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
    /// </summary>
    private Color StateColor(long handle, string prop, string key, int role) {
        string def = "";
        if (Application.Current != null) {
            def = Application.Current.ResolveColor(key);
        }
        string s = WindowHost.ElementGetString(handle, prop, def);
        if (s == null || s.Length == 0) {
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
        string def = "";
        if (Application.Current != null) {
            def = Application.Current.ResolveColor(key);
        }
        string s = WindowHost.ElementGetString(handle, prop, def);
        if (s == null || s.Length == 0) {
            s = def;
        }
        if (s == null || s.Length == 0) {
            return Color.Transparent();
        }
        return MotionEngine.ResolveColorDur(handle, role, s, durationMs);
    }

    public void RenderElementTree(long rootHandle) {
        if (!_initialized || _pass == null) {
            return;
        }
        if (rootHandle == 0) {
            return;
        }
        Color rootBackground = this.ElementColor(rootHandle, "Background", Color.Parse("#FFFFFF"));
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
                Color stroke = this.ElementColor(handle, "Stroke", Color.Parse("#FF000000"));
                this.DrawRectBorder(lx, ly, rw, rh, stroke);
            }
        }

        // ---- TextBlock ----
        if (type == ElTextBlock) {
            string text = WindowHost.ElementGetString(handle, "Text", "");
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            Color fg = this.ElementColor(handle, "Foreground", this.ColorTextDefault());
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", 0.0);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            this.DrawText(text, lx, ly, fontSize, bg, fg, family, weight);
        }

        // ---- 模板让位（WPF 语义）：已挂视觉子树的控件跳过内置 chrome 分支 ----
        // ControlTemplate 套用后 chrome 完全由模板树负责（尾部队通用递归渲染），
        // 防内置 chrome + 模板子树双轨叠加（production-surface Template-first 门禁）。
        bool templated = WindowHost.ElementGetChildCount(handle) > 0;

        // ---- Button / ToggleButton ----
        if ((type == ElButton || type == ElToggleButton) && !templated) {
            string content = WindowHost.ElementGetString(handle, "Content", "");
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            int isChecked = WindowHost.ElementGetBool(handle, "IsChecked", 0);
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", 14.0);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            double scaledGlyphHeight = GlyphHeight;
            if (fontSize > 0.0) { scaledGlyphHeight = GlyphHeight * (fontSize / GlyphHeight); }
            double estimatedWidthOriginal = this.EstTextWidth(content, LayoutPaddingX, fontSize, family, weight);
            double estimatedHeight = scaledGlyphHeight + LayoutPaddingY;
            double bw = lw;
            if (bw <= 0.0) { bw = estimatedWidthOriginal; }
            double bh = lh;
            if (bh <= 0.0) { bh = estimatedHeight; }
            int isMouseOver = WindowHost.ElementGetBool(handle, "IsMouseOver", 0);
            int isPressed = WindowHost.ElementGetBool(handle, "IsPressed", 0);
            int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
            ControlVisual pal = VisualStateManager.Button(ControlState.Of(isEnabled, isMouseOver, isPressed, isFocused, isChecked, 0));
            // 现代深度反馈：hover/pressed 抬升软阴影 + focus 辉光（DrawSurfaceShadow，圆角贴合）。
            if (pal.Lift.IsVisible) {
                this.DrawSurfaceShadow(lx, ly, bw, bh, pal.Lift.Radius, pal.Lift.Blur, pal.Lift.OffsetY, pal.Lift.Alpha);
            }
            double cr = pal.Radius.Max;
            Color bg = this.StateColorMotion(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration);
            Color fg = this.StateColorMotion(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground, pal.MotionDuration);
            Color border = this.StateColorMotion(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration);
            // RFC 037 §3.6：圆角+渐变同一 SDF；显式 Background 优先生效（禁主题渐变盖住样式红）。
            Color gradStart = Color.Transparent();
            Color gradEnd = Color.Transparent();
            bool hasGradient = this.ResolveGradient(pal, ref gradStart, ref gradEnd);
            if (!this.HasExplicitBackground(handle) && hasGradient) {
                this.DrawLinearGradient(lx, ly, bw, bh, gradStart, gradEnd, 0.0, 0.0, 1.0, 0.0, cr);
            } else {
                this.DrawRoundedRect(lx, ly, bw, bh, cr, bg);
            }
            this.DrawRoundedBorder(lx, ly, bw, bh, cr, (double)RectBorderThickness, border);
            // ToggleButton 选中标记（Accent 强调）
            if (type == ElToggleButton && isChecked != 0) {
                Color accent = this.StateColor(handle, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
                double innerCr = cr - 3.0;
                if (innerCr < 0.0) { innerCr = 0.0; }
                this.DrawRoundedRect(lx + 3.0, ly + 3.0, bw - 6.0, bh - 6.0, innerCr, accent);
            }
            // 焦点外晕（FocusRing）+ 辉光（FocusGlow）
            if (isFocused != 0) {
                if (pal.FocusGlow.IsVisible) {
                    this.DrawSurfaceShadow(lx, ly, bw, bh, pal.FocusGlow.Radius, pal.FocusGlow.Blur,
                                           pal.FocusGlow.OffsetY, pal.FocusGlow.Alpha);
                }
                Color ring = this.StateColor(handle, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
                this.DrawRoundedBorder(lx - 2.0, ly - 2.0, bw + 4.0, bh + 4.0, cr + 2.0, pal.FocusRingWidth, ring);
            }
            // 按钮文字居中（textWidth = 原始文本宽 - padding；EstTextWidth 对 padding
            // 严格可加，故与独立无 padding 度量数学等价）
            double textWidth = estimatedWidthOriginal - LayoutPaddingX;
            double textX = lx + (bw - textWidth) / 2.0;
            if (textX < lx + 4.0) { textX = lx + 4.0; }
            double textY = ly + (bh - scaledGlyphHeight) / 2.0;
            this.DrawText(content, textX, textY, fontSize, this.ColorTransparent(), fg, family, weight);
        }

        // ---- CheckBox ----
        if (type == ElCheckBox && !templated) {
            string content = WindowHost.ElementGetString(handle, "Content", "");
            int isChecked = WindowHost.ElementGetBool(handle, "IsChecked", 0);
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            int isMouseOver = WindowHost.ElementGetBool(handle, "IsMouseOver", 0);
            int isPressed = WindowHost.ElementGetBool(handle, "IsPressed", 0);
            int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", 14.0);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            double box = 14.0;
            ControlVisual pal = VisualStateManager.Toggle(ControlState.Of(isEnabled, isMouseOver, isPressed, isFocused, isChecked, 0));
            double cr = pal.Radius.Max;
            Color boxBackground = this.StateColorMotion(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration);
            Color boxBorder = this.StateColorMotion(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration);
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
                double innerCr = cr - 3.0;
                if (innerCr < 0.0) { innerCr = 0.0; }
                this.DrawRoundedRect(lx + 3.0, ly + 3.0, box - 6.0, box - 6.0, innerCr, accent);
            }
            Color fg = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
            double textX = lx + box + 4.0;
            this.DrawText(content, textX, ly + (box - GlyphHeight) / 2.0, fontSize, this.ColorTransparent(), fg, family, weight);
        }

        // ---- TextBox ----
        if (type == ElTextBox && !templated) {
            string text = WindowHost.ElementGetString(handle, "Text", "");
            string placeholder = WindowHost.ElementGetString(handle, "Placeholder", "");
            string composition = WindowHost.ElementGetString(handle, "CompositionText", "");
            double caretIdx = WindowHost.ElementGetNumber(handle, "CaretIndex", 0.0);
            int caretIndex = (int)caretIdx;
            string display = text;
            bool isPlaceholder = false;
            if (display == null || display.Length == 0) {
                display = placeholder;
                isPlaceholder = true;
            }
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
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", 14.0);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            double scaledGlyphHeight = GlyphHeight;
            if (fontSize > 0.0) { scaledGlyphHeight = GlyphHeight * (fontSize / GlyphHeight); }
            double estimatedWidth = this.EstTextWidth(display, 16.0, fontSize, family, weight);
            double estimatedHeight = scaledGlyphHeight + 8.0;
            double iw = lw;
            if (iw <= 0.0) { iw = estimatedWidth; }
            double ih = lh;
            if (ih <= 0.0) { ih = estimatedHeight; }
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            int isFocused = WindowHost.ElementGetBool(handle, "IsFocused", 0);
            ControlVisual pal = VisualStateManager.TextBox(ControlState.Of(isEnabled, 0, 0, isFocused, 0, 0));
            double cr = pal.Radius.Max;
            Color bg = this.StateColorMotion(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration);
            Color border = this.StateColorMotion(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration);
            this.DrawRoundedRect(lx, ly, iw, ih, cr, bg);
            this.DrawRoundedBorder(lx, ly, iw, ih, cr, (double)RectBorderThickness, border);
            if (isFocused != 0) {
                if (pal.FocusGlow.IsVisible) {
                    this.DrawSurfaceShadow(lx, ly, iw, ih, pal.FocusGlow.Radius, pal.FocusGlow.Blur,
                                           pal.FocusGlow.OffsetY, pal.FocusGlow.Alpha);
                }
                Color ring = this.StateColor(handle, "FocusRingBrush", pal.FocusRing, MotionEngine.RoleFocusRing);
                this.DrawRoundedBorder(lx - 2.0, ly - 2.0, iw + 4.0, ih + 4.0, cr + 2.0, pal.FocusRingWidth, ring);
            }
            Color fg = this.StateColor(handle, "Foreground",
                                       isPlaceholder ? pal.Placeholder : pal.Foreground,
                                       MotionEngine.RoleForeground);
            double textX = lx + 4.0;
            double textY = ly + (ih - scaledGlyphHeight) / 2.0;
            // M-caret2 选区高亮：背景之上、文本之下；几何与 caret 同源（pen 内缩
            // MinTextPaddingX/2 + 前缀宽度），placeholder 态无选区。
            double selStartV = WindowHost.ElementGetNumber(handle, "SelectionStart", 0.0);
            double selLenV = WindowHost.ElementGetNumber(handle, "SelectionLength", 0.0);
            int selStart = (int)selStartV;
            int selLen = (int)selLenV;
            if (!isPlaceholder && text != null && selLen > 0 && selStart < text.Length) {
                if (selStart + selLen > text.Length) {
                    selLen = text.Length - selStart;
                }
                if (selLen > 0) {
                    double penX = textX + MinTextPaddingX / 2.0;
                    double selX = penX
                        + this.EstTextWidth(text.Substring(0, selStart), 0.0, fontSize, family, weight);
                    double selW = this.EstTextWidth(
                        text.Substring(selStart, selLen), 0.0, fontSize, family, weight);
                    if (selW > 0.0) {
                        this.DrawRect(selX, textY, selW, scaledGlyphHeight, Color.Parse("#402F6FDE"));
                    }
                }
            }
            this.DrawText(display, textX, textY, fontSize, this.ColorTransparent(), fg, family, weight);
            // 组字下划线预览：DrawText 起始 pen 自带 MinTextPaddingX/2 内缩，补齐对齐。
            if (hasComposition) {
                double compX = textX + MinTextPaddingX / 2.0 + this.EstTextWidth(compPrefix, 0.0, fontSize, family, weight);
                double compW = this.EstTextWidth(composition, 0.0, fontSize, family, weight);
                double underlineY = textY + scaledGlyphHeight - 2.0;
                if (compW > 0.0) {
                    this.DrawRect(compX, underlineY, compW, 1.0, Color.Parse("#FF000000"));
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
                double caretX = textX + MinTextPaddingX / 2.0
                    + this.EstTextWidth(caretPrefix, 0.0, fontSize, family, weight);
                Color caretColor = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
                this.DrawRect(caretX, textY, 1.5, scaledGlyphHeight, caretColor);
            }
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
                    this.DrawRect(lx, ly, lw, lh, Color.Parse("#FFD0D0D0"));
                }
                this.DrawRectBorder(lx, ly, lw, lh, Color.Parse("#FF606060"));
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

        // ---- Slider ----
        if (type == ElSlider && !templated) {
            double val = WindowHost.ElementGetNumber(handle, "Value", 0.0);
            double min = WindowHost.ElementGetNumber(handle, "Minimum", 0.0);
            double max = WindowHost.ElementGetNumber(handle, "Maximum", 100.0);
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            double sw = lw;
            if (sw <= 0.0) { sw = 200.0; }
            double sh = GlyphHeight + 8.0;
            ControlVisual pal = VisualStateManager.Slider(ControlState.Of(isEnabled, 0, 0, 0, 0, 0));
            Color trackColor = this.StateColor(handle, "TrackBrush", pal.Track, MotionEngine.RoleBorder);
            Color foregroundColor = this.StateColor(handle, "AccentBrush", pal.Accent, MotionEngine.RoleAccent);
            this.DrawRoundedRect(lx + 4.0, ly + sh / 2.0 - 2.0, sw - 8.0, 4.0, 2.0, trackColor);
            double range = max - min;
            double t = (range > 0.0) ? (val - min) / range : 0.0;
            if (t < 0.0) { t = 0.0; } if (t > 1.0) { t = 1.0; }
            double fillWidth = (sw - 8.0) * t;
            this.DrawRoundedRect(lx + 4.0, ly + sh / 2.0 - 2.0, fillWidth, 4.0, 2.0, foregroundColor);
            this.DrawRoundedRect(lx + 4.0 + fillWidth - 6.0, ly + sh / 2.0 - 8.0, 12.0, 16.0, 6.0, foregroundColor);
        }

        // ---- ComboBox（折叠态 chrome；选项列表属展开 Popup 轨，选项行不经通用
        //      递归渲染——同 DataGrid 行镜像内联消费先例，防选项行与 chrome 叠加）----
        if (type == ElComboBox) {
            int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
            double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", 14.0);
            int family = this.ResolveFontFamily(WindowHost.ElementGetString(handle, "FontFamily", ""));
            int weight = this.ResolveFontWeight(WindowHost.ElementGetString(handle, "FontWeight", "Normal"));
            double scaledGlyphHeight = GlyphHeight;
            if (fontSize > 0.0) { scaledGlyphHeight = GlyphHeight * (fontSize / GlyphHeight); }
            double iw = lw;
            if (iw <= 0.0) { iw = 160.0; }
            double ih = lh;
            if (ih <= 0.0) { ih = scaledGlyphHeight + 12.0; }
            ControlVisual pal = VisualStateManager.ComboBox(ControlState.Of(isEnabled, 0, 0, 0, 0, 0));
            double cr = pal.Radius.Max;
            Color bg = this.StateColorMotion(handle, "Background", pal.Background, MotionEngine.RoleBackground, pal.MotionDuration);
            Color border = this.StateColorMotion(handle, "BorderBrush", pal.Border, MotionEngine.RoleBorder, pal.MotionDuration);
            this.DrawRoundedRect(lx, ly, iw, ih, cr, bg);
            this.DrawRoundedBorder(lx, ly, iw, ih, cr, (double)RectBorderThickness, border);
            Color fg = this.StateColor(handle, "Foreground", pal.Foreground, MotionEngine.RoleForeground);
            string selectedText = WindowHost.ElementGetString(handle, "SelectedText", "");
            double textY = ly + (ih - scaledGlyphHeight) / 2.0;
            this.DrawText(selectedText, lx + 4.0, textY, fontSize, this.ColorTransparent(), fg, family, weight);
            // 下拉 chevron：无三角绘制原语，右侧三条渐窄横条堆叠近似（水平居中于 chevron 轴）
            double chevronCx = lx + iw - 12.0;
            double chevronCy = ly + ih / 2.0 - 2.25;
            this.DrawRect(chevronCx - 4.0, chevronCy, 8.0, 1.5, fg);
            this.DrawRect(chevronCx - 2.5, chevronCy + 2.0, 5.0, 1.5, fg);
            this.DrawRect(chevronCx - 1.0, chevronCy + 4.0, 2.0, 1.5, fg);
            return;
        }

        // ---- DataGrid（RFC 037 §4 · M-VZ4：表头带 + 斑马纹 + 选中 Accent 高亮 + 列分隔；
        //      行镜像由本分支内联消费，不走通用递归）----
        if (type == ElDataGrid) {
            this.RenderDataGrid(handle, lx, ly, lw, lh);
            return;
        }

        // ---- VisualHost / LayoutShell（Grid/DockPanel/WrapPanel/Canvas/ListView）----
        if (type == ElVisualHost || this.IsLayoutShell(type)) {
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            double cw = lw;
            double ch = lh;
            this.DrawBackground(bg, lx, ly, cw, ch);
        }

        // ---- Window / Element / 未知容器：仅背景 ----
        if (type == ElWindow || type == ElElement) {
            Color bg = this.ElementColor(handle, "Background", Color.Transparent());
            this.DrawBackground(bg, lx, ly, lw, lh);
        }

        // ---- Popup 层根 / 蒙层（RFC 037 Popup 轨）：仅背景，子树走通用递归。
        //      渲染顺序即置顶依据：层根挂窗口平台根 children 末尾，painter's
        //      algorithm 后画在上；蒙层背景由 Popup.Open 直写平台镜像
        //      （PlatformTreeSync 公共尾部不识 Panel.Background，无法走同步轨）。----
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
        double headerH = WindowHost.ElementGetNumber(handle, "HeaderHeight", 32.0);
        double stride = WindowHost.ElementGetNumber(handle, "RowHeight", 32.0);
        if (stride <= 0.0) {
            stride = 32.0;
        }
        int selectedIndex = (int)WindowHost.ElementGetNumber(handle, "SelectedIndex", -1.0);
        int isEnabled = WindowHost.ElementGetBool(handle, "IsEnabled", 1);
        double fontSize = WindowHost.ElementGetNumber(handle, "FontSize", 14.0);
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
                    string clipped = this.ClipTextToWidth(cell, colW[cj] - 16.0, fontSize, family, weight);
                    if (clipped.Length > 0) {
                        this.DrawText(clipped, colX[cj] + 8.0, textY, fontSize,
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
            string clippedHeader = this.ClipTextToWidth(headerText, colW[ci] - 16.0, fontSize, family, headerWeight);
            if (clippedHeader.Length > 0) {
                this.DrawText(clippedHeader, colX[ci] + 8.0, headerTextY, fontSize,
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
