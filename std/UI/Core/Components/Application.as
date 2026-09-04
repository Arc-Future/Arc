// RFC 037 D2.1 / D9.2 / D3.1 / M3: Arc.UI — Application 根容器。
//
// Application 是 .arml 的根容器，承担：
//   - 应用生命周期入口（对标 WPF System.Windows.Application）
//   - 顶级资源字典宿主（样式/主题/转换器）
//   - 主窗口创建与编排
//   - 隐式样式自动应用
//
// 使用模式（WPF-aligned）：
//   1. codegen 在 App.g.as 中 override InitializeComponent()
//   2. 用户在 App.arml.as 中可选 override OnStartup/OnExit
//   3. Program.as 入口：var app = new App(); app.Run();  （compat 阻塞入口）
//      或 await app.RunAsync();  （RFC 037 M-AS1+ 正道骨架）
//
// 生命周期：
//   1. new App()         — 构造
//   2. app.Run() / RunAsync() — 入口
//      a. InitializeComponent()  — 设置 MainWindow + InitializeComponent
//      b. OnStartup()            — 用户启动钩子
//      c. ApplyImplicitStyles()  — M3 隐式样式自动应用
//      d. MainWindow.Show() / ShowAsync() — 显示主窗口，进入帧泵
//      e. OnExit()               — 退出钩子

namespace Arc.UI.Components;

using Arc;
using Arc.UI;
using Arc.UI.Input;
using Arc.UI.Internal;
using Arc.UI.Media;
using Arc.UI.Styling;

public class Application : Element {
    /// <summary>主窗口实例。</summary>
    public Window MainWindow;

    /// <summary>应用级资源字典（用户 arml 覆盖 + 隐式样式）。本地条目优先于合并主题。</summary>
    public ResourceDictionary Resources;

    /// <summary>主题资源持有器（Light/Dark + 切换）。</summary>
    public ThemeDictionary ThemeDictionaries;

    /// <summary>应用级字体注册表（RFC 037 §9；正道 <c>Fonts.RegisterFamily</c>）。</summary>
    private FontManager _fonts;

    /// <summary>应用级字体注册表（对标 WPF 启动期注册；见 custom-fonts.md）。</summary>
    public FontManager Fonts
    {
        get { return _fonts; }
    }

    /// <summary>单一解析根（WPF Application.Current 对齐）。所有 token 解析唯一入口。</summary>
    public static Application Current;

    public Application() {
        this.Type = typeof(Application);
        Application.Current = this;
        _fonts = new FontManager();
        ThemeDictionaries = new ThemeDictionary();
        Resources = new ResourceDictionary();
        // 活动主题资源并入 MergedDictionaries：本地覆盖（用户 arml）> 主题默认（DynamicResource 语义）。
        Resources.MergedDictionaries.Add(ThemeDictionaries.Active);
    }

    /// <summary>解析颜色资源（本地覆盖 > 活动主题；返回 hex 字符串，供渲染器既有路径）。</summary>
    public string ResolveColor(string key) {
        ResourceValue v = ResourceValue.String("");
        if (Resources.TryLookup(key, ref v)) {
            switch (v)
            {
                case ResourceValue.Brush(b):
                {
                    return b.ToHex();
                }
                case ResourceValue.String(s):
                {
                    return s;
                }
                default:
                {
                }
            }
        }
        return "";
    }

    /// <summary>解析画刷资源（本地覆盖 > 活动主题；未命中返回透明画刷）。颜色家族体系入口。</summary>
    public Brush ResolveBrush(string key) {
        ResourceValue v = ResourceValue.String("");
        if (Resources.TryLookup(key, ref v)) {
            switch (v)
            {
                case ResourceValue.Brush(b):
                {
                    return b;
                }
                case ResourceValue.String(s):
                {
                    return Brush.FromString(s);
                }
                default:
                {
                }
            }
        }
        return new SolidColorBrush(Color.Transparent());
    }

    /// <summary>解析数值资源（本地覆盖 > 活动主题；未命中返回 0）。</summary>
    public double ResolveNumber(string key) {
        ResourceValue v = ResourceValue.String("");
        if (Resources.TryLookup(key, ref v)) {
            switch (v)
            {
                case ResourceValue.Number(n):
                {
                    return n;
                }
                default:
                {
                }
            }
        }
        return 0.0;
    }

    /// <summary>解析字符串资源（本地覆盖 > 活动主题；未命中返回空串）。</summary>
    public string ResolveString(string key) {
        ResourceValue v = ResourceValue.String("");
        if (Resources.TryLookup(key, ref v)) {
            switch (v)
            {
                case ResourceValue.String(s):
                {
                    return s;
                }
                default:
                {
                }
            }
        }
        return "";
    }

    /// <summary>
    /// 切换主题：替换活动主题到 MergedDictionaries，重新应用隐式样式并触发重绘。
    /// Style Setter {StaticResource} 引用在应用期按活动主题解析落值（键编译期
    /// 确定、值来自编译期扁平主题字典），切主题后重应用即刷新到新值；SetStyleValue
    /// 样式通道本地值优先，不冲用户程序化覆盖（WPF DP 优先级）。
    /// </summary>
    public void SwitchTheme(string name) {
        ThemeDictionaries.Switch(name);
        this.SyncActiveTheme();
        if (this.MainWindow != null) {
            StyleManager sm = new StyleManager();
            sm.ApplyImplicitStyles(this.MainWindow, Resources);
        }
        FramePump.Invalidate();
    }

    /// <summary>
    /// 重新并入活动主题到资源链（本地覆盖 > 活动主题）。主题注册/切换可能更换
    /// `ThemeDictionaries.Active`，须重建 MergedDictionaries 使解析读到新值。
    /// </summary>
    void SyncActiveTheme() {
        Resources.MergedDictionaries.Clear();
        Resources.MergedDictionaries.Add(ThemeDictionaries.Active);
    }

    /// <summary>codegen 在派生 partial class 中 override。</summary>
    public virtual void InitializeComponent() { }

    /// <summary>启动钩子——在 MainWindow 显示前触发。用户可 override。</summary>
    public virtual void OnStartup() { }

    /// <summary>退出钩子——在 MainWindow 关闭后触发。用户可 override。</summary>
    public virtual void OnExit() { }

    /// <summary>
    /// 启动应用——阻塞 compat 入口（RFC 037：非终态正道；保留至 RunAsync 全量签收）。
    ///   1. InitializeComponent()  → 设置 MainWindow
    ///   2. OnStartup()            → 用户启动逻辑
    ///   3. ApplyStyleTree()       → 样式自动应用（隐式/显式两趟）
    ///   4. MainWindow.Show()      → FramePump.RunUntilClose（阻塞至窗口关闭）
    ///   5. OnExit()               → 用户清理逻辑
    /// </summary>
    public void Run() {
        this.RunCore();
        if (this.MainWindow != null) {
            this.MainWindow.Show();
        }
        this.OnExit();
    }

    /// <summary>
    /// 启动应用——非阻塞正道骨架（RFC 037 M-AS1）。FramePump + PumpOnce；非「异步 UI 完备」。
    /// </summary>
    public async Task RunAsync() {
        this.RunCore();
        if (this.MainWindow != null) {
            await this.MainWindow.ShowAsync();
        }
        this.OnExit();
    }

    /// <summary>Run / RunAsync 共享的启动前序（InitializeComponent → OnStartup → styles → IME）。</summary>
    void RunCore() {
        this.InitializeComponent();
        // arml `<Application.Resources>` 资源条目已在 InitializeComponent 落进 Resources 本地条目，
        // 经 TryLookup 本地优先于 MergedDictionaries 活动主题（WPF DynamicResource 覆盖语义）。
        // 主题注册（`<Application.Themes>`) 在 InitializeComponent 内执行，可能覆盖当前主题
        // (ThemeDictionaries.Active 被替换) → 重新并入活动主题到资源链，确保解析读到最新值。
        this.SyncActiveTheme();
        this.OnStartup();
        ImeBridge.WarmupHandler();

        // M3 + RFC 037: 样式自动应用（两趟；RFC 037 VisualHost 边界）
        this.ApplyStyleTree();
        if (this.MainWindow != null) {
            VisualHost.ApplyAllHostStyles(this.MainWindow);
        }

        ImeBridge.InstallHandler();
    }
}
