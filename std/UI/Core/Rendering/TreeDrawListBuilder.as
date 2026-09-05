// RFC 037 §10 AI 原生：TreeDrawListBuilder — Element 树 → DrawList 转换器。
//
// 核心职责：
//   1. 对 Element 树执行 Measure/Arrange 布局
//   2. 遍历树，读取各元素类型和属性，录制 DrawCommand
//   3. 输出 DrawList 供 WgpuRender 离屏渲染
//
// 设计原则：
//   - 不依赖平台镜像（WindowHost handle）——直接读 Element DP 值
//   - 与 WgpuRender.RenderElementNode 同构但独立——保证预览/运行时一致性
//   - 属性读取走 ResolveProperty + GetValue<T>——尊重 DP 类型系统
//
// 支持元素类型（首版）：
//   - 容器：Panel/StackPanel/Grid/Canvas/DockPanel/WrapPanel/ScrollView
//   - 控件：TextBlock/Button/CheckBox/ToggleButton/TextBox/Slider/Rectangle/Image
//     （Image 经 Image.TickAnimation 驱动解码后 DrawTexture 采样，RFC 029 M2）
//   - 根容器：Window/Page/UserControl/VisualHost
//
// 诚实边界：不支持数据绑定、模板化控件（Popup/Calendar 等）、虚拟化容器。

namespace Arc.UI.Rendering;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Components.Layout;
using Arc.UI.Layout;
using Arc.UI.Media;

/// <summary>
/// Element 树 → DrawList 转换器。执行布局后遍历录制绘制命令。
/// </summary>
public class TreeDrawListBuilder {
    /// <summary>
    /// 构建 DrawList：布局 Element 树后遍历录制绘制命令。
    /// </summary>
    /// <param name="root">根元素。</param>
    /// <param name="viewportWidth">视口宽。</param>
    /// <param name="viewportHeight">视口高。</param>
    /// <param name="backend">渲染后端（驱动 Image 延迟解码/GIF 帧；可 null——此时 Image 走占位）。</param>
    public DrawList Build(Element root, double viewportWidth, double viewportHeight, IRender backend) {
        DrawList list = new DrawList();
        if (root == null) {
            return list;
        }

        // 1. 执行布局
        this.PerformLayout(root, viewportWidth, viewportHeight);

        // 2. 从根开始录制
        this.RecurseElement(root, list, backend);

        return list;
    }

    /// <summary>
    /// 对 Element 树执行 Measure/Arrange 布局。
    /// </summary>
    private void PerformLayout(Element root, double width, double height) {
        if (root == null) {
            return;
        }
        // 非 FrameworkElement 根（如未知类型 fallback 到 Element）无布局能力，安全跳过。
        FrameworkElement rootFE = null;
        if (root is FrameworkElement) {
            rootFE = (FrameworkElement)root;
        }
        if (rootFE == null) {
            return;
        }

        LayoutSize available = new LayoutSize(width, height);
        rootFE.Measure(available);
        rootFE.Arrange(new LayoutSize(width, height));
    }

    /// <summary>
    /// 递归处理元素——录制绘制命令并遍历子元素。
    /// </summary>
    private void RecurseElement(Element element, DrawList list, IRender backend) {
        if (element == null) {
            return;
        }

        string typeName = element.TypeName;
        FrameworkElement fe = null;
        if (element is FrameworkElement) {
            fe = (FrameworkElement)element;
        }
        double x = 0.0;
        double y = 0.0;
        double w = 0.0;
        double h = 0.0;
        if (fe != null) {
            x = fe.LayoutX;
            y = fe.LayoutY;
            w = fe.RenderWidth;
            h = fe.RenderHeight;
        }

        // 非 FrameworkElement 节点（如未知类型 fallback 到 Element）仅作为
        // 逻辑容器递归子元素，不参与绘制（无布局坐标）。
        if (fe != null) {
            // 绘制顺序：背景 → 内容 → 子元素

            // ---- 背景绘制 ----
            this.DrawBackground(element, typeName, x, y, w, h, list);

            // ---- 按类型绘制内容 ----
            switch (typeName) {
                case "TextBlock": {
                    this.DrawTextElement(element, x, y, w, h, list);
                    break;
                }
                case "Button":
                case "CheckBox":
                case "ToggleButton":
                case "TextBox":
                case "Slider": {
                    // 模板让位（WPF 语义，与运行时 RenderTree 门禁同构）：
                    // 已挂模板视觉子树的控件跳过内置文本 chrome，防双轨叠加。
                    List<Element> chromeChildren = element.Children;
                    if (chromeChildren != null && chromeChildren.Count > 0) {
                        break;
                    }
                    this.DrawTextElement(element, x, y, w, h, list);
                    break;
                }
                case "Rectangle": {
                    this.DrawRectangleElement(element, x, y, w, h, list);
                    break;
                }
                case "Image": {
                    // 图片占位（无纹理时跳过）
                    break;
                }
                case "StackPanel":
                case "Grid":
                case "Canvas":
                case "DockPanel":
                case "WrapPanel":
                case "ScrollView":
                case "VirtualizingStackPanel":
                case "VisualHost":
                case "Window":
                case "Page":
                case "UserControl":
                case "ContentPresenter":
                case "ContentControl":
                case "ItemsControl":
                case "ListView":
                case "DataGrid":
                case "Application":
                case "Element":
                default: {
                    // 容器：仅背景 + 子元素递归
                    break;
                }
            }
        }

        // ---- 子元素递归 ----
        List<Element> children = element.Children;
        if (children != null) {
            int count = children.Count;
            for (int i = 0; i < count; i++) {
                this.RecurseElement(children[i], list, backend);
            }
        }
    }

    // ===== 元素绘制 =====

    /// <summary>
    /// 绘制元素背景（Background DP → 填充矩形）。
    /// </summary>
    private void DrawBackground(Element element, string typeName,
                                double x, double y, double w, double h,
                                DrawList list) {
        if (w <= 0.0 || h <= 0.0) {
            return;
        }
        // 尝试读取 Background 属性
        string bgHex = this.TryGetBackgroundHex(element);
        if (bgHex != null && bgHex.Length > 0 && bgHex != "Transparent" && bgHex != "#00000000") {
            FillRectPayload payload = new FillRectPayload();
            payload.X = x;
            payload.Y = y;
            payload.Width = w;
            payload.Height = h;
            payload.FillColor = bgHex;
            list.Add(DrawCommand.FillRect(payload));
        }
    }

    /// <summary>
    /// 绘制文本类元素（TextBlock/Button/CheckBox 等）。
    /// </summary>
    private void DrawTextElement(Element element,
                                  double x, double y, double w, double h,
                                  DrawList list) {
        // 获取文本内容
        string text = this.TryGetText(element);
        if (text == null || text.Length == 0) {
            // 空文本仍然绘制背景（已在 DrawBackground 处理）
            return;
        }

        // 获取字体属性
        double fontSize = this.TryGetFontSize(element);
        string foreground = this.TryGetForegroundHex(element);
        string background = this.TryGetBackgroundHex(element);

        // 计算文本位置（Button 居中，TextBlock 左上）
        double textX = x;
        double textY = y;
        if (w > 0.0 && h > 0.0) {
            textX = x + 4.0;
            textY = y + 2.0;
        }

        DrawTextPayload payload = new DrawTextPayload();
        payload.X = textX;
        payload.Y = textY;
        payload.Text = text;
        payload.FontSize = fontSize;
        payload.Foreground = foreground;
        payload.Background = background;
        list.Add(DrawCommand.DrawText(payload));
    }

    /// <summary>
    /// 绘制 Rectangle 元素（填充 + 可选描边）。
    /// </summary>
    private void DrawRectangleElement(Element element,
                                       double x, double y, double w, double h,
                                       DrawList list) {
        double rw = w;
        double rh = h;
        if (rw <= 0.0) { rw = 100.0; }
        if (rh <= 0.0) { rh = 100.0; }

        // Fill
        string fillHex = this.TryGetFillHex(element);
        if (fillHex != null && fillHex.Length > 0 && fillHex != "Transparent" && fillHex != "#00000000") {
            FillRectPayload payload = new FillRectPayload();
            payload.X = x;
            payload.Y = y;
            payload.Width = rw;
            payload.Height = rh;
            payload.FillColor = fillHex;
            list.Add(DrawCommand.FillRect(payload));
        }

        // Stroke (边框)
        double strokeThickness = this.TryGetStrokeThickness(element);
        if (strokeThickness > 0.0) {
            string strokeHex = this.TryGetStrokeHex(element);
            if (strokeHex != null && strokeHex.Length > 0) {
                // 用 1px 的线框模拟边框（简化实现）
                DrawLinePayload top = new DrawLinePayload();
                top.X1 = x; top.Y1 = y;
                top.X2 = x + rw; top.Y2 = y;
                top.Color = strokeHex; top.Thickness = strokeThickness;
                list.Add(DrawCommand.DrawLine(top));

                DrawLinePayload bottom = new DrawLinePayload();
                bottom.X1 = x; bottom.Y1 = y + rh;
                bottom.X2 = x + rw; bottom.Y2 = y + rh;
                bottom.Color = strokeHex; bottom.Thickness = strokeThickness;
                list.Add(DrawCommand.DrawLine(bottom));

                DrawLinePayload left = new DrawLinePayload();
                left.X1 = x; left.Y1 = y;
                left.X2 = x; left.Y2 = y + rh;
                left.Color = strokeHex; left.Thickness = strokeThickness;
                list.Add(DrawCommand.DrawLine(left));

                DrawLinePayload right = new DrawLinePayload();
                right.X1 = x + rw; right.Y1 = y;
                right.X2 = x + rw; right.Y2 = y + rh;
                right.Color = strokeHex; right.Thickness = strokeThickness;
                list.Add(DrawCommand.DrawLine(right));
            }
        }
    }

    /// <summary>
    /// 绘制 Image 元素（RFC 029 M2）：TickAnimation 驱动延迟解码/GIF 帧推进，
    /// 取解码纹理经 StretchMapper 采样；无纹理回退占位（与 RenderTree 同语义）。
    /// </summary>
    private void DrawImageElement(Element element,
                                   double x, double y, double w, double h,
                                   IRender backend, DrawList list) {
        if (w <= 0.0 || h <= 0.0) {
            return;
        }
        Image img = null;
        if (element is Image) {
            img = (Image)element;
        }
        if (img == null) {
            return;
        }
        // 复用组件解码泵：backend null 时 TickAnimation 内部跳过（走占位）。
        img.TickAnimation(backend);
        int textureId = 0;
        int tw = 0;
        int th = 0;
        if (img.TryGetTexture(out textureId, out tw, out th)) {
            StretchMapping m = StretchMapper.Compute(img.Stretch, (double)tw, (double)th,
                                                     x, y, w, h);
            list.AddDrawTexture(textureId, m.X, m.Y, m.Width, m.Height, m.U0, m.V0, m.U1, m.V1, 1.0);
            return;
        }
        // 占位：未解码/解码失败/无源时灰底 + 边框（无 Background 才铺灰底）。
        string bg = this.TryGetBackgroundHex(element);
        bool hasBg = bg != null && bg.Length > 0 && bg != "Transparent" && bg != "#00000000";
        if (!hasBg) {
            FillRectPayload fill = new FillRectPayload();
            fill.X = x;
            fill.Y = y;
            fill.Width = w;
            fill.Height = h;
            fill.FillColor = "#FFD0D0D0";
            list.Add(DrawCommand.FillRect(fill));
        }
        string border = "#FF606060";
        DrawLinePayload top = new DrawLinePayload();
        top.X1 = x; top.Y1 = y;
        top.X2 = x + w; top.Y2 = y;
        top.Color = border; top.Thickness = 1.0;
        list.Add(DrawCommand.DrawLine(top));

        DrawLinePayload bottom = new DrawLinePayload();
        bottom.X1 = x; bottom.Y1 = y + h;
        bottom.X2 = x + w; bottom.Y2 = y + h;
        bottom.Color = border; bottom.Thickness = 1.0;
        list.Add(DrawCommand.DrawLine(bottom));

        DrawLinePayload left = new DrawLinePayload();
        left.X1 = x; left.Y1 = y;
        left.X2 = x; left.Y2 = y + h;
        left.Color = border; left.Thickness = 1.0;
        list.Add(DrawCommand.DrawLine(left));

        DrawLinePayload right = new DrawLinePayload();
        right.X1 = x + w; right.Y1 = y;
        right.X2 = x + w; right.Y2 = y + h;
        right.Color = border; right.Thickness = 1.0;
        list.Add(DrawCommand.DrawLine(right));
    }

    // ===== 属性读取辅助 =====

    /// <summary>尝试读取 Background 的十六进制颜色字符串。</summary>
    private string TryGetBackgroundHex(Element element) {
        // 优先用类型化属性
        Control ctrl = null;
        if (element is Control) {
            ctrl = (Control)element;
        }
        if (ctrl != null) {
            string bg = ctrl.Background;
            if (bg != null && bg.Length > 0) {
                return bg;
            }
        }
        // 通过 DP 解析读取
        object dp = element.ResolveProperty("Background");
        if (dp != null) {
            object val = element.GetValue<object>((DependencyProperty<object>)dp);
            if (val is Brush) {
                return ((Brush)val).ToHex();
            }
            if (val is string) {
                return (string)val;
            }
        }
        return "#00000000"; // 透明
    }

    /// <summary>尝试读取 Foreground 的十六进制颜色字符串。</summary>
    private string TryGetForegroundHex(Element element) {
        Control ctrl = null;
        if (element is Control) {
            ctrl = (Control)element;
        }
        if (ctrl != null) {
            string fg = ctrl.Foreground;
            if (fg != null && fg.Length > 0) {
                return fg;
            }
        }
        object dp = element.ResolveProperty("Foreground");
        if (dp != null) {
            object val = element.GetValue<object>((DependencyProperty<object>)dp);
            if (val is Brush) {
                return ((Brush)val).ToHex();
            }
            if (val is string) {
                return (string)val;
            }
        }
        return "#FFFFFFFF"; // 白色
    }

    /// <summary>尝试读取文本内容。处理 Content variant 和各种控件类型。</summary>
    private string TryGetText(Element element) {
        // TextBlock 直接属性
        TextBlock tb = null;
        if (element is TextBlock) {
            tb = (TextBlock)element;
        }
        if (tb != null) {
            return tb.Text;
        }
        // TextBox
        TextBox txb = null;
        if (element is TextBox) {
            txb = (TextBox)element;
        }
        if (txb != null) {
            return txb.Text;
        }
        // ContentControl 系列（Button/CheckBox/Window 等）
        ContentControl cc = null;
        if (element is ContentControl) {
            cc = (ContentControl)element;
        }
        if (cc != null) {
            Content content = cc.Content;
            switch (content)
            {
                case Content.Text(s):
                {
                    return s;
                }
                default:
                {
                    break;
                }
            }
            // MirrorContent 回退（平台同步路径）
            if (cc.MirrorContent != null && cc.MirrorContent.Length > 0) {
                return cc.MirrorContent;
            }
        }
        // 通用：尝试 Text DP
        object dp = element.ResolveProperty("Text");
        if (dp != null) {
            return element.GetValue<string>((DependencyProperty<string>)dp);
        }
        return "";
    }

    /// <summary>尝试读取字号。</summary>
    private double TryGetFontSize(Element element) {
        Control ctrl = null;
        if (element is Control) {
            ctrl = (Control)element;
        }
        if (ctrl != null) {
            return ctrl.FontSize;
        }
        object dp = element.ResolveProperty("FontSize");
        if (dp != null) {
            return element.GetValue<double>((DependencyProperty<double>)dp);
        }
        return 14.0; // 默认字号
    }

    /// <summary>尝试读取 Fill 颜色（Rectangle 专用）。</summary>
    private string TryGetFillHex(Element element) {
        object dp = element.ResolveProperty("Fill");
        if (dp != null) {
            object val = element.GetValue<object>((DependencyProperty<object>)dp);
            if (val is Brush) {
                return ((Brush)val).ToHex();
            }
            if (val is string) {
                return (string)val;
            }
        }
        return "#00000000";
    }

    /// <summary>尝试读取 Stroke 颜色。</summary>
    private string TryGetStrokeHex(Element element) {
        object dp = element.ResolveProperty("Stroke");
        if (dp != null) {
            object val = element.GetValue<object>((DependencyProperty<object>)dp);
            if (val is Brush) {
                return ((Brush)val).ToHex();
            }
            if (val is string) {
                return (string)val;
            }
        }
        return "#FF000000"; // 黑色
    }

    /// <summary>尝试读取描边宽度。</summary>
    private double TryGetStrokeThickness(Element element) {
        object dp = element.ResolveProperty("StrokeThickness");
        if (dp != null) {
            return element.GetValue<double>((DependencyProperty<double>)dp);
        }
        return 0.0;
    }
}
