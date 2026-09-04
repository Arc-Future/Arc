// RFC 037 M-VH1/M-VH2 + §3.4/§4.3 切片: Arc.UI.Components — VisualHost 隔离视觉树宿主。
//
// 类 iframe 逻辑边界：独立 Element 子树 + 帧内 ResourceDictionary 根 + RFC 037 Light Theme；
// 宿主 Application/Window 隐式样式不穿透内层（StyleManager 边界 + ApplyHostStyles）。
//
// RFC 037 §3.4：DataContext 继承边界——IsDataContextBoundary() override 为 true；
// 内层子树沿 Parent 链查找 DataContext 时在边界截断（宿主 DataContext 不流入），
// 边界自身显式设置的 DataContext 仍供内层根/子节点继承。
//
// RFC 037 §4.3：生命周期事件——ContentChanged / InnerLoaded / InnerUnloaded
// （Signal<bool> 通道 + On* 便捷方法，与 Button.Clicked 同一惯用法）；InnerUnloaded
// 在内层根 OnUnloaded（G2 退订动作执行，Element.RegisterDetach）之后触发。
//
// 诚实缺口（Draft）：独立 HWND、焦点域/输入路由域/IME 隔离 — M-VH3+ 未实现（RFC 037 §8 ⬜）。

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Layout;
using Arc.UI.Styling;

/// <summary>
/// 隔离视觉树宿主。Content 为内层单根；Resources 自包含且默认合并 Light Theme。
/// </summary>
public class VisualHost : ContentControl {
    public VisualHost() {
        this.Type = typeof(VisualHost);
        ResourceDictionary dict = BuiltInTheme.CreateLight();
        this.Resources = dict;
        this.TypeName = "VisualHost";
        this.ContentChanged = new Signal<bool>(false);
        this.InnerLoaded = new Signal<bool>(false);
        this.InnerUnloaded = new Signal<bool>(false);
    }

    // ===== RFC 037 §4.3 生命周期事件（Signal 单引擎，同 Button.Clicked/OnClick 惯例）=====

    /// <summary>内容变更信号——SetContent/Rebuild/Clear 替换内层根后触发（预览刷新/脏标记）。</summary>
    public Signal<bool> ContentChanged;

    /// <summary>内层根挂载信号——SetContent 挂新根并完成宿主初始化后触发。</summary>
    public Signal<bool> InnerLoaded;

    /// <summary>内层根卸载信号——内层根 OnUnloaded（G2 退订动作）执行之后触发。</summary>
    public Signal<bool> InnerUnloaded;

    /// <summary>订阅内容变更——ContentChanged.Subscribe 的便捷封装。</summary>
    public void OnContentChanged(Action<bool> handler) {
        if (ContentChanged != null && handler != null) {
            ContentChanged.Subscribe(handler);
        }
    }

    /// <summary>订阅内层根挂载——InnerLoaded.Subscribe 的便捷封装。</summary>
    public void OnInnerLoaded(Action<bool> handler) {
        if (InnerLoaded != null && handler != null) {
            InnerLoaded.Subscribe(handler);
        }
    }

    /// <summary>订阅内层根卸载——InnerUnloaded.Subscribe 的便捷封装。</summary>
    public void OnInnerUnloaded(Action<bool> handler) {
        if (InnerUnloaded != null && handler != null) {
            InnerUnloaded.Subscribe(handler);
        }
    }

    /// <summary>内层子树 DataContext 继承边界（RFC 037 §3.4）：宿主 DataContext 不流入内层。</summary>
    public override bool IsDataContextBoundary() {
        return true;
    }

    /// <summary>内层子树根（<see cref="Content"/> 别名）。</summary>
    public Content Child {
        get { return this.Content; }
        set { this.Content = value; }
    }

    /// <summary>隔离区 ResourceDictionary（不自动合并宿主字典）。</summary>
    public ResourceDictionary GetHostResources() {
        object res = this.Resources;
        if (res == null) {
            ResourceDictionary dict = BuiltInTheme.CreateLight();
            this.Resources = dict;
            return dict;
        }
        return (ResourceDictionary)res;
    }

    /// <summary>同步替换内层根；触发 Unload/Load + 帧内隐式样式 + §4.3 事件。</summary>
    public void SetContent(Element root) {
        this.unloadInnerRoot();
        if (root == null) {
            this.RaiseContentChanged();
            return;
        }
        this.AddChild(root);
        this.Content = Content.Element(root);
        root.OnLoaded();
        this.ApplyHostStyles();
        this.IsMeasured = false;
        this.RaiseInnerLoaded();
        this.RaiseContentChanged();
    }

    /// <summary>预览管线入口：可选替换 Resources + SetContent + 样式 + 失效 Measure。</summary>
    public void Rebuild(Element root, ResourceDictionary resources) {
        if (resources != null) {
            // 用户资源本地优先，Light 主题默认并入 MergedDictionaries（DynamicResource 语义）。
            resources.MergedDictionaries.Add(BuiltInTheme.CreateLight());
            this.Resources = resources;
        }
        this.SetContent(root);
    }

    /// <summary>卸载内层树；VisualHost 仍占位。触发 InnerUnloaded（detach 动作后）+ ContentChanged。</summary>
    public void Clear() {
        this.unloadInnerRoot();
        this.RaiseContentChanged();
    }

    /// <summary>Navigate 别名（M-VH1；无 URI/journal）。</summary>
    public void Navigate(Element root) {
        this.SetContent(root);
    }

    /// <summary>对 Content 子树应用帧内隐式样式（隔离 ResourceDictionary 作用域）。</summary>
    public void ApplyHostStyles() {
        ResourceDictionary dict = this.GetHostResources();
        if (dict.StyleCount <= 0) {
            return;
        }
        FrameworkElement innerRoot = this.resolveContentRoot();
        if (innerRoot == null) {
            return;
        }
        StyleManager sm = new StyleManager();
        sm.ApplyAllStyles(innerRoot, dict, null);
    }

    /// <summary>遍历视觉树，对所有 VisualHost 应用帧内样式（Application.Run 调用）。</summary>
    public static void ApplyAllHostStyles(Element root) {
        if (root == null) {
            return;
        }
        if (root.TypeName == "VisualHost") {
            VisualHost host = (VisualHost)root;
            host.ApplyHostStyles();
        }
        List<Element> children = root.Children;
        if (children != null) {
            foreach (var child in children) {
                VisualHost.ApplyAllHostStyles(child);
            }
        }
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        FrameworkElement content = this.resolveContentRoot();
        if (content == null) {
            return new LayoutSize(0.0, 0.0);
        }
        LayoutHelper.MeasureChild(content, availableSize);
        LayoutSize d = content.DesiredSize;
        return new LayoutSize(d.Width, d.Height);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        FrameworkElement content = this.resolveContentRoot();
        if (content == null) {
            return;
        }
        double fw = finalSize.Width;
        double fh = finalSize.Height;
        LayoutHelper.ArrangeChild(this, content, 0.0, 0.0, fw, fh);
    }

    private void unloadInnerRoot() {
        bool hadRoot = false;
        if (this.Children != null && this.Children.Count > 0) {
            hadRoot = true;
            int count = this.Children.Count;
            for (int i = 0; i < count; i++) {
                Element ch = this.Children[i];
                ch.OnUnloaded();          // G2 退订动作执行（RegisterDetach 登记）
                ch.Parent = null;
            }
            this.Children.Clear();
        }
        this.Content = Content.None;
        if (hadRoot) {
            this.RaiseInnerUnloaded();    // 在 detach 动作之后触发（§4.3 + G2 衔接）
        }
    }

    private void RaiseInnerLoaded() {
        if (InnerLoaded != null) {
            InnerLoaded.Set(true);
        }
    }

    private void RaiseInnerUnloaded() {
        if (InnerUnloaded != null) {
            InnerUnloaded.Set(true);
        }
    }

    private void RaiseContentChanged() {
        if (ContentChanged != null) {
            ContentChanged.Set(true);
        }
    }

    private FrameworkElement resolveContentRoot() {
        if (this.Children != null && this.Children.Count > 0) {
            return (FrameworkElement)this.Children[0];
        }
        Content c = this.Content;
        switch (c)
        {
            case Content.Element(el):
            {
                return (FrameworkElement)el;
            }
            default:
            {
                return null;
            }
        }
    }
}
