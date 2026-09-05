// RFC 037 §10 AI 原生：LivePreviewHost —— 设计时实时预览宿主。
//
// 核心能力：
//   1. LoadSpec：解析 ARML 字符串 → Element 树 → 布局 → 离屏渲染
//   2. ApplyPatch：通过元素路径定位并修改属性 → 单帧重渲染
//   3. CapturePng：将预览内容或指定区域渲染为 PNG
//   4. GetLayoutSnapshot：收集元素树结构化布局信息（供 AI 理解 UI 结构）
//   5. Reset：清空当前预览
//
// 双宿主渲染架构（RFC 037 §3.1/§10）：
//   - LivePreviewHost（离屏、无窗口、无帧泵）——设计时预览
//   - WindowHost（有窗口、有帧泵）——运行时渲染
//   两者共享同一套渲染代码（WgpuRender + DrawList），保证预览与运行时效果一致。
//
// 使用模式：
//   var host = new LivePreviewHost();
//   host.Initialize();                    // 初始化离屏渲染
//   host.LoadSpec(armlString, 800, 600);  // 加载并渲染
//   host.CapturePng("preview.png");        // 截图
//   host.ApplyPatch("Root/Button", "Content", "Save"); // 实时修改
//   host.CapturePng("preview2.png");      // 修改后截图
//   host.GetLayoutSnapshot();             // 获取布局快照

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.Drawing;
using Arc.UI;
using Arc.UI.Markup;
using Arc.UI.Media;
using Arc.UI.Rendering;
using Arc.UI.Rendering.Wgpu;

/// <summary>
/// 设计时实时预览宿主——支持 ARML 加载、热修改、截图、布局快照。
/// 继承 VisualHost 获得隔离视觉树 + 内容变更通知能力。
/// </summary>
public class LivePreviewHost : VisualHost {
    private WgpuRender _renderer;
    private int _offscreenId;
    private double _viewportWidth;
    private double _viewportHeight;
    private Element _rootElement;
    private TreeDrawListBuilder _drawListBuilder;
    private bool _initialized;

    /// <summary>构造预览宿主。</summary>
    public LivePreviewHost() {
        _renderer = null;
        _offscreenId = 0;
        _viewportWidth = 800.0;
        _viewportHeight = 600.0;
        _rootElement = null;
        _drawListBuilder = new TreeDrawListBuilder();
        _initialized = false;
    }

    /// <summary>当前视口宽度。</summary>
    public double ViewportWidth {
        get { return _viewportWidth; }
    }

    /// <summary>当前视口高度。</summary>
    public double ViewportHeight {
        get { return _viewportHeight; }
    }

    /// <summary>当前根元素（已解析的 Element 树）。</summary>
    public Element RootElement {
        get { return _rootElement; }
    }

    // ===== 初始化 =====

    /// <summary>
    /// 初始化离屏渲染管线。必须在其他方法之前调用。
    /// </summary>
    /// <returns>是否初始化成功。</returns>
    public bool Initialize() {
        if (_initialized) {
            return true;
        }
        _renderer = new WgpuRender();
        if (!_renderer.InitializeOffscreen()) {
            return false;
        }
        _offscreenId = _renderer.CreateOffscreenTarget(
            (int)_viewportWidth, (int)_viewportHeight);
        if (_offscreenId == 0) {
            return false;
        }
        _initialized = true;
        return true;
    }

    /// <summary>
    /// 调整视口尺寸（离屏目标随之重建）。
    /// </summary>
    public void ResizeViewport(double width, double height) {
        _viewportWidth = width;
        _viewportHeight = height;
        if (_offscreenId > 0 && _renderer != null) {
            _renderer.DestroyOffscreenTarget(_offscreenId);
            _offscreenId = _renderer.CreateOffscreenTarget((int)width, (int)height);
        }
    }

    // ===== 核心 API =====

    /// <summary>
    /// 加载 ARML 字符串并渲染预览。
    /// </summary>
    /// <param name="arml">ARML 标记字符串。</param>
    /// <param name="width">视口宽度（0 = 使用当前）。</param>
    /// <param name="height">视口高度（0 = 使用当前）。</param>
    /// <returns>加载结果（含诊断信息）。</returns>
    public ArmlParseResult LoadSpec(string arml, double width, double height) {
        if (!_initialized) {
            ArmlParseResult err = new ArmlParseResult();
            err.Diagnostics = new List<string>();
            err.Diagnostics.Add("LivePreviewHost 未初始化");
            err.Success = false;
            return err;
        }

        // 更新视口
        if (width > 0.0 && height > 0.0) {
            this.ResizeViewport(width, height);
        }

        // 1. 解析 ARML → Element 树
        ArmlParseResult result = ArmlParser.Parse(arml);
        if (!result.Success || result.Root == null) {
            return result;
        }

        // 2. 保存根元素
        _rootElement = result.Root;

        // 3. 设置到 VisualHost（触发样式、生命周期）
        this.SetContent(_rootElement);

        // 4. 渲染第一帧
        this.RenderFrame();

        return result;
    }

    /// <summary>
    /// 加载 ARML 字符串并渲染预览（使用当前视口尺寸）。
    /// </summary>
    public ArmlParseResult LoadSpec(string arml) {
        return this.LoadSpec(arml, 0.0, 0.0);
    }

    /// <summary>
    /// 应用属性补丁——通过元素路径定位并修改属性值，触发单帧重渲染。
    /// </summary>
    /// <param name="elementPath">元素路径（如 "Root/StackPanel/Button"）。</param>
    /// <param name="propertyName">属性名（如 "Content"、"Background"）。</param>
    /// <param name="value">新值（字符串形式）。</param>
    /// <returns>是否应用成功。</returns>
    public bool ApplyPatch(string elementPath, string propertyName, string value) {
        if (!_initialized || _rootElement == null) {
            return false;
        }

        // 1. 定位元素
        Element target = this.ResolveElementPath(elementPath);
        if (target == null) {
            return false;
        }

        // 2. 设置属性值
        bool ok = this.SetPropertyOnElement(target, propertyName, value);
        if (!ok) {
            return false;
        }

        // 3. 重新渲染（属性变更后需要重新布局 + 绘制）
        this.RenderFrame();

        return true;
    }

    /// <summary>
    /// 按元素路径批量应用补丁。
    /// </summary>
    /// <param name="patches">补丁列表（路径 → 属性名 → 值）。</param>
    /// <returns>成功应用的补丁数量。</returns>
    public int ApplyPatches(List<PropertyPatch> patches) {
        if (!_initialized || _rootElement == null || patches == null) {
            return 0;
        }
        int ok = 0;
        for (int i = 0; i < patches.Count; i++) {
            PropertyPatch p = patches[i];
            if (this.ApplyPatch(p.ElementPath, p.PropertyName, p.Value)) {
                ok++;
            }
        }
        return ok;
    }

    /// <summary>
    /// 捕获当前预览为 PNG 文件。
    /// </summary>
    /// <param name="filePath">输出 PNG 文件路径。</param>
    /// <returns>是否保存成功。</returns>
    public bool CapturePng(string filePath) {
        return this.CapturePng(filePath, 0.0, 0.0, _viewportWidth, _viewportHeight);
    }

    /// <summary>
    /// 捕获指定区域为 PNG 文件（按元素区块截图的 AI 友好能力）。
    /// </summary>
    /// <param name="filePath">输出 PNG 文件路径。</param>
    /// <param name="x">区域左上角 X。</param>
    /// <param name="y">区域左上角 Y。</param>
    /// <param name="width">区域宽度。</param>
    /// <param name="height">区域高度。</param>
    /// <returns>是否保存成功。</returns>
    public bool CapturePng(string filePath, double x, double y, double width, double height) {
        // 内容校验：未初始化、无渲染器/离屏目标或尚未加载元素树时拒绝截图。
        if (!_initialized || _renderer == null || _offscreenId == 0 || _rootElement == null) {
            return false;
        }

        // 确保当前帧已渲染
        this.RenderFrame();

        int vw = (int)_viewportWidth;
        int vh = (int)_viewportHeight;

        // 1. 创建全帧位图并回读像素
        Bitmap frameBitmap = new Bitmap(vw, vh);
        long pixels = frameBitmap.GetPixels();
        bool ok = _renderer.ReadbackPixels(_offscreenId, pixels, vw * vh * 4);
        if (!ok) {
            frameBitmap.Dispose();
            return false;
        }

        int cw = (int)width;
        int ch = (int)height;
        if (cw <= 0 || ch <= 0) {
            cw = vw;
            ch = vh;
        }

        // 2. 全帧直接保存
        bool fullFrame = (x <= 0.0 && y <= 0.0 &&
                        cw >= vw && ch >= vh);
        if (fullFrame) {
            frameBitmap.Save(filePath);
            frameBitmap.Dispose();
            return true;
        }

        // 3. 裁剪：创建目标位图并逐像素拷贝
        Bitmap cropped = new Bitmap(cw, ch);
        int ix = (int)x;
        int iy = (int)y;
        for (int py = 0; py < ch; py++) {
            int srcY = iy + py;
            for (int px = 0; px < cw; px++) {
                int srcX = ix + px;
                RgbColor color;
                if (srcX >= 0 && srcX < vw && srcY >= 0 && srcY < vh) {
                    color = frameBitmap.GetPixel(srcX, srcY);
                } else {
                    color = new RgbColor((byte)0, (byte)0, (byte)0, (byte)255);
                }
                cropped.SetPixel(px, py, color);
            }
        }
        cropped.Save(filePath);
        cropped.Dispose();
        frameBitmap.Dispose();
        return true;
    }

    /// <summary>
    /// 获取布局快照——遍历元素树，收集结构化布局信息。
    /// AI 可通过此方法理解 UI 结构（元素类型、位置、尺寸、属性）。
    /// </summary>
    /// <returns>布局快照（根节点，含子树）。</returns>
    public LayoutSnapshotNode GetLayoutSnapshot() {
        if (_rootElement == null) {
            return null;
        }
        return this.BuildSnapshotRecursive(_rootElement);
    }

    /// <summary>
    /// 重置预览——清空当前元素树。
    /// </summary>
    public void Reset() {
        _rootElement = null;
        this.Clear();
    }

    // ===== 内部方法 =====

    /// <summary>
    /// 渲染一帧（将 Element 树 → DrawList → 离屏渲染）。
    /// </summary>
    private void RenderFrame() {
        if (!_initialized || _rootElement == null || _renderer == null) {
            return;
        }

        // 1. 生成 DrawList（backend 传入以驱动 Image 延迟解码/GIF 帧）
        DrawList list = _drawListBuilder.Build(_rootElement, _viewportWidth, _viewportHeight, _renderer);

        // 2. 渲染到离屏目标
        _renderer.RenderToOffscreen(_offscreenId, list, _viewportWidth, _viewportHeight);
    }

    /// <summary>
    /// 通过路径解析元素（格式："Root/Type1/Type2/..."）。
    /// 路径段匹配 TypeName 或 Name（x:Name）。
    /// </summary>
    private Element ResolveElementPath(string path) {
        if (path == null || path.Length == 0 || _rootElement == null) {
            return null;
        }
        string[] parts = path.Split('/');
        Element current = _rootElement;

        for (int i = 0; i < parts.Length; i++) {
            string segment = parts[i];
            if (segment == null || segment.Length == 0) {
                continue;
            }

            // "Root" 锚点别名——始终解析为根元素（路径可带或不带此前缀）。
            if (segment == "Root") {
                current = _rootElement;
                continue;
            }

            // 检查当前节点是否匹配
            if (this.MatchesSegment(current, segment)) {
                // 如果是最后一段，返回当前
                if (i == parts.Length - 1) {
                    return current;
                }
                // 否则在子元素中查找下一段
                current = this.FindChildBySegment(current, parts[i + 1]);
                if (current == null) {
                    return null;
                }
                continue;
            }

            // 当前节点不匹配，尝试在子元素中查找本段
            Element found = this.FindChildBySegment(current, segment);
            if (found == null) {
                return null;
            }
            current = found;
        }

        return current;
    }

    private bool MatchesSegment(Element element, string segment) {
        if (element == null) {
            return false;
        }
        // 按 Name (x:Name) 匹配
        if (element.Name != null && element.Name == segment) {
            return true;
        }
        // 按 TypeName 匹配
        if (element.TypeName != null && element.TypeName == segment) {
            return true;
        }
        return false;
    }

    private Element FindChildBySegment(Element parent, string segment) {
        if (parent == null || parent.Children == null) {
            return null;
        }
        List<Element> children = parent.Children;
        for (int i = 0; i < children.Count; i++) {
            Element child = children[i];
            if (this.MatchesSegment(child, segment)) {
                return child;
            }
        }
        return null;
    }

    /// <summary>
    /// 在元素上设置属性值（通过 DP 解析，转换统一走 DpValueConverter 单一事实来源）。
    /// </summary>
    private bool SetPropertyOnElement(Element element, string propertyName, string value) {
        if (element == null || propertyName == null || propertyName.Length == 0) {
            return false;
        }

        // 尝试通过 ResolveProperty 找到 DP
        object dp = element.ResolveProperty(propertyName);
        if (dp == null) {
            return false;
        }

        return DpValueConverter.SetValue(element, dp, value);
    }

    /// <summary>
    /// 构建布局快照（递归）。
    /// </summary>
    private LayoutSnapshotNode BuildSnapshotRecursive(Element element) {
        if (element == null) {
            return null;
        }

        LayoutSnapshotNode node = new LayoutSnapshotNode();
        node.TypeName = element.TypeName;
        node.Name = element.Name;

        FrameworkElement fe = null;
        if (element is FrameworkElement) {
            fe = (FrameworkElement)element;
        }
        if (fe != null) {
            node.X = fe.LayoutX;
            node.Y = fe.LayoutY;
            node.Width = fe.RenderWidth;
            node.Height = fe.RenderHeight;
        }

        // 收集关键属性
        node.Properties = this.CollectKeyProperties(element);

        // 递归子元素
        if (element.Children != null) {
            node.Children = new List<LayoutSnapshotNode>();
            for (int i = 0; i < element.Children.Count; i++) {
                LayoutSnapshotNode child = this.BuildSnapshotRecursive(element.Children[i]);
                if (child != null) {
                    node.Children.Add(child);
                }
            }
        }

        return node;
    }

    private Dictionary<string, string> CollectKeyProperties(Element element) {
        Dictionary<string, string> props = new Dictionary<string, string>();

        // Background
        string bg = this.TryGetBgSimple(element);
        if (bg != null) {
            props["Background"] = bg;
        }

        // Foreground
        string fg = this.TryGetFgSimple(element);
        if (fg != null) {
            props["Foreground"] = fg;
        }

        // Text / Content
        string text = this.TryGetTextSimple(element);
        if (text != null && text.Length > 0) {
            props["Text"] = text;
        }

        // FontSize
        double fs = this.TryGetFontSizeSimple(element);
        if (fs > 0.0) {
            props["FontSize"] = fs.ToString();
        }

        // Width / Height (if explicitly set)
        FrameworkElement fe = null;
        if (element is FrameworkElement) {
            fe = (FrameworkElement)element;
        }
        if (fe != null) {
            if (fe.Width > 0.0) {
                props["Width"] = fe.Width.ToString();
            }
            if (fe.Height > 0.0) {
                props["Height"] = fe.Height.ToString();
            }
        }

        return props;
    }

    private string TryGetBgSimple(Element element) {
        Control ctrl = null;
        if (element is Control) {
            ctrl = (Control)element;
        }
        if (ctrl != null) {
            return ctrl.Background;
        }
        return null;
    }

    private string TryGetFgSimple(Element element) {
        Control ctrl = null;
        if (element is Control) {
            ctrl = (Control)element;
        }
        if (ctrl != null) {
            return ctrl.Foreground;
        }
        return null;
    }

    private string TryGetTextSimple(Element element) {
        TextBlock tb = null;
        if (element is TextBlock) {
            tb = (TextBlock)element;
        }
        if (tb != null) {
            return tb.Text;
        }
        TextBox txb = null;
        if (element is TextBox) {
            txb = (TextBox)element;
        }
        if (txb != null) {
            return txb.Text;
        }
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
        }
        return null;
    }

    private double TryGetFontSizeSimple(Element element) {
        Control ctrl = null;
        if (element is Control) {
            ctrl = (Control)element;
        }
        if (ctrl != null) {
            return ctrl.FontSize;
        }
        return 0.0;
    }

}

/// <summary>
/// 属性补丁——描述单个属性修改。
/// </summary>
public class PropertyPatch {
    /// <summary>元素路径（如 "Root/Button"）。</summary>
    public string ElementPath;

    /// <summary>属性名（如 "Content"、"Background"）。</summary>
    public string PropertyName;

    /// <summary>新值（字符串形式）。</summary>
    public string Value;
}

/// <summary>
/// 布局快照节点——结构化 UI 布局信息（供 AI 理解 UI）。
/// </summary>
public class LayoutSnapshotNode {
    /// <summary>元素类型名（如 "Button"、"StackPanel"）。</summary>
    public string TypeName;

    /// <summary>元素标识名（x:Name 属性）。</summary>
    public string Name;

    /// <summary>绝对 X 坐标（布局后）。</summary>
    public double X;

    /// <summary>绝对 Y 坐标（布局后）。</summary>
    public double Y;

    /// <summary>渲染宽度。</summary>
    public double Width;

    /// <summary>渲染高度。</summary>
    public double Height;

    /// <summary>关键属性字典。</summary>
    public Dictionary<string, string> Properties;

    /// <summary>子元素列表。</summary>
    public List<LayoutSnapshotNode> Children;
}
