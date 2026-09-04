// RFC 037 D2.1 / D9.2 / D3.1 + RFC 037 D1/D6: Arc.UI.Components — Window 元素。
//
// Window 是顶级窗口容器（D2.1），提供：
//   - 标题、位置属性（对标 WPF System.Windows.Window）
//   - 内容容器（Content，继承自 ContentControl）
//   - 原生窗口句柄（rt_window_create ABI）
//   - 生命周期：OnInitialized → OnLoaded → OnClosed
//
// WPF 同构层级对照：
//   WPF: ContentControl → Window
//   Arc:  ContentControl → Window
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Width/Height 已由 FrameworkElement 声明——Window 不重复声明，使用继承版本
//   - Content 已由 ContentControl 声明——Window 不重复声明，使用继承版本
//   - Window 保留特有 DP：Title/Left/Top
//   - Window 保留特有方法：InitializeComponent/OnClosed/Show/Close + _platformRoot 字段
//
// RFC 037 D1 属性系统（WPF 同构编程模型）：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护（D6 字典存储），用户不感知。
//   渲染层通过 this.Observe<T>(WidthProperty) 获取 Signal<T> 订阅局部刷新。
//
// 使用模式（WPF-aligned）：
//   1. codegen 在 MainWindow.g.as 中 override InitializeComponent()
//      设置 this.Title/Width/Height 属性，触发 Signal 通知
//   2. 用户在 MainWindow.arml.as 中可选 override OnLoaded/OnClosed
//   3. Application.Run() 调用 MainWindow.Show() 触发原生窗口创建 + 帧泵（阻塞 compat）
//      或 await Application.RunAsync() → ShowAsync()（RFC 037 正道骨架）
//
// 渲染后端架构（RFC 037 §D7）：
//   ┌────────────────────────────────────────────────────┐
//   │ IRender trait（Arc.UI.Rendering）                   │
//   ├────────────────────────────────────────────────────┤
//   │ WgpuRender（唯一后端）                             │
//   │  - DX12/Metal/Vulkan/WebGPU                        │
//   │  - 通过 wgpu-native.ani + rt_wgpu_native.c        │
//   ├────────────────────────────────────────────────────┤
//   │ WindowHost：CreateWindow / NativeHandle / RunEvent  │
//   ├────────────────────────────────────────────────────┤
//   │ crates/runtime/platform/<os>/window.*：Win32 / X11 / macOS 窗口创建   │
//   └────────────────────────────────────────────────────┘

namespace Arc.UI.Components;

using Arc;
using Arc.UI.Internal;

using Arc.UI;
using Arc.UI.Input;
using Arc.UI.Layout;

/// <summary>顶级窗口容器。继承 ContentControl 获得 Content DP；本类仅声明窗口特有 DP。</summary>
public class Window : ContentControl {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public Window() {
        this.Type = typeof(Window);
        // 容器型非 Tab 停靠（InputElement 默认 Focusable+IsTabStop 的显式豁免；
        // WPF Window 同构——窗口内容才参与 Tab 循环）。
        this.IsTabStop = false;
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====
    //
    // 仅供绑定/样式系统识别属性身份，并通过 Element 字典存储定位 Signal<T>。
    // 全局唯一 Id 由 RegisterProperty<T> 工厂通过 DependencyPropertyRegistry 分配。
    //
    // 推荐编码模型（M1.1）：
    //   - nameof(属性) 替代字符串字面量——IDE 重构可自动追踪符号引用
    //   - typeof(类) 替代字符串字面量——避免魔法字符串与重构不同步
    //
    // **注意**：Width/Height/Content 不在此声明——已由 FrameworkElement/ContentControl
    // 声明并继承到 Window，重复声明会产生两个独立 DP 实例导致绑定/样式失效。

    /// <summary>Title 属性元数据——窗口标题。</summary>
    public static DependencyProperty<string> TitleProperty =
        RegisterProperty<string>(nameof(Title), typeof(Window), "");

    /// <summary>Left 属性元数据——窗口客户区左边距。</summary>
    public static DependencyProperty<double> LeftProperty =
        RegisterProperty<double>(nameof(Left), typeof(Window), 0.0);

    /// <summary>Top 属性元数据——窗口客户区上边距。</summary>
    public static DependencyProperty<double> TopProperty =
        RegisterProperty<double>(nameof(Top), typeof(Window), 0.0);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====
    //
    // 每个 wrapper 仅一行 get + 一行 set——内部由 Element 字典存储 + Signal<T>
    // 后端处理值存储与变更通知。Signal<T> 首次 SetValue/Observe 时惰性创建。
    //
    // **Width/Height/Content 属性继承自基类**：派生类无需重新声明 wrapper——
    // 直接使用 this.Width / this.Content 等即可访问基类 wrapper。

    /// <summary>窗口标题。修改后自动通知所有订阅者（渲染层局部刷新）。</summary>
    public string Title {
        get { return this.GetValue<string>(TitleProperty); }
        set { this.SetValue<string>(TitleProperty, value); }
    }

    /// <summary>窗口客户区左边距（像素）。</summary>
    public double Left {
        get { return this.GetValue<double>(LeftProperty); }
        set { this.SetValue<double>(LeftProperty, value); }
    }

    /// <summary>窗口客户区上边距（像素）。</summary>
    public double Top {
        get { return this.GetValue<double>(TopProperty); }
        set { this.SetValue<double>(TopProperty, value); }
    }

    // ===== 跨平台元素树根节点句柄 =====

    /// <summary>
    /// RFC 037 M3/M3.6：跨平台元素树根节点句柄。
    ///
    /// 由 <see cref="Show"/> 调用 <c>PlatformTreeSync.BuildFromArc(this)</c>
    /// 从 Arc 逻辑树一次性同步构建，再传递给
    /// <c>WindowHost.RunWithRoot</c>。codegen 不再双写 Element* 调用。
    ///
    /// 访问规范：protected 字段下划线开头，方法体内裸访问（不带 `this.`）。
    /// </summary>
    protected long _platformRoot;

    /// <summary>
    /// 平台树根句柄的进程内只读通道（std UI 内部轨道）——Popup 等 overlay 体系
    /// 挂载层根用：层根必须挂在已运行窗口的平台根 children 末尾，才能参与
    /// hit_test 逆序置顶与渲染遍历。
    /// </summary>
    internal long PlatformRootHandle {
        get { return _platformRoot; }
    }

    // ===== 生命周期钩子 =====

    /// <summary>
    /// 由 codegen 在派生 partial class 中 override：
    ///   public override void InitializeComponent() {
    ///       this.Title = "...";
    ///       this.Width = 640;
    ///       this.Height = 480;
    ///   }
    /// 基类提供空实现以支持 Application.Run() 的虚拟分派。
    /// </summary>
    public virtual void InitializeComponent() {
        // 默认空实现；派生 partial class 由 codegen override 设置属性
    }

    /// <summary>
    /// 窗口已关闭时触发，可释放资源。
    ///
    /// 这是 Window 特有的生命周期钩子（Element 仅有 OnInitialized/OnLoaded/
    /// OnUnloaded）。OnLoaded 由 Element 基类提供，Window 通过继承获得。
    /// </summary>
    public virtual void OnClosed() {
        // 默认空实现；派生类按需 override
    }

    // ===== 显示与关闭 =====

    /// <summary>
    /// 显示窗口并进入阻塞帧泵（compat 入口 — RFC 037：非终态正道；内部 FramePump.RunUntilClose）。
    ///
    /// 流程：
    ///   1. 触发 OnLoaded()（继承自 Element 基类）
    ///   2. LayoutManager.Update(this) —— 逻辑树 Measure/Arrange
    ///   3. PlatformTreeSync.BuildFromArc(this) —— Arc 逻辑树 → 平台镜像（含 LayoutX/Y）
    ///   4. FramePump.RunUntilClose —— PumpOnce 消息循环（阻塞至关闭）
    ///   5. 窗口关闭后触发 OnClosed()
    /// </summary>
    public void Show() {
        this.PrepareForShow();
        // 窗口尺寸来自 DP（codegen 以双精度字面量赋值，经 SetValue<double> 正确存储）
        double wRaw = this.Width;
        double hRaw = this.Height;
        int width = (int)wRaw;
        int height = (int)hRaw;
        // 回退：如果显式尺寸为 0，使用 DesiredSize 或内容尺寸
        if (width <= 0 && this.DesiredSize.Width > 0.0) {
            width = (int)this.DesiredSize.Width;
        }
        if (height <= 0 && this.DesiredSize.Height > 0.0) {
            height = (int)this.DesiredSize.Height;
        }
        if (width <= 0) { width = 720; }
        if (height <= 0) { height = 480; }
        ImeBridge.RunMessageLoop(this.Title, width, height, _platformRoot);
        this.OnClosed();
    }

    /// <summary>
    /// 显示窗口并进入非阻塞帧泵（RFC 037 M-AS1 正道骨架）。非「异步 UI 完备」签收。
    /// </summary>
    public async Task ShowAsync() {
        this.PrepareForShow();
        int width = (int)this.Width;
        int height = (int)this.Height;
        await FramePump.RunAsync(this.Title, width, height, _platformRoot);
        this.OnClosed();
    }

    /// <summary>Show / ShowAsync 共享：布局、路由注册、平台树构建。</summary>
    void PrepareForShow() {
        this.OnLoaded();
        // 此时 TextMeasuring 可能尚未挂接（wgpu 需 HWND）——占位布局；
        // FramePump 初始化后端后调用 RelayoutSynced 用同源 atlas 重测。
        LayoutManager.Update(this);
        PointerRouter.Reset();
        MotionEngine.Reset();
        PointerRouter.Install();
        FocusManager.Reset();
        FocusManager.Install();
        InputFocusRouter.Reset();
        InputFocusRouter.Install();
        ScrollRouter.Reset();
        ScrollRouter.Install();
        _platformRoot = PlatformTreeSync.BuildFromArc(this);
        PlatformTreeSync.RootEpoch = PlatformTreeSync.RootEpoch + 1;
        ScrollRouter.BindWindow(this, _platformRoot);
    }

    /// <summary>
    /// atlas/度量服务就绪后重跑 Measure/Arrange 并写回平台镜像布局坐标。
    /// FramePump 在 WgpuRender.Initialize（TextMeasuring.Attach）之后调用。
    /// </summary>
    internal void RelayoutSynced() {
        LayoutManager.Update(this);
        if (_platformRoot != 0) {
            PlatformTreeSync.SyncLayoutFromArc(this, _platformRoot);
        }
    }

    /// <summary>
    /// 编程式关闭当前窗口。
    ///
    /// codegen 拦截：从 <c>FocusManager._windowHandle</c> 静态槽取平台窗口镜像
    /// 句柄（<c>rt_window_create</c> 返回值，由 <c>FramePump</c> 在
    /// <c>Show</c>/<c>ShowAsync</c> 流程经 <c>FocusManager.SetWindowHandle</c>
    /// 回填）传 <c>rt_window_close</c>（C 侧 Post WM_CLOSE → 消息循环退出）。
    /// 需在 <c>Show</c>/<c>ShowAsync</c> 之后调用（句柄已回填）；未 Show 时
    /// 句柄为 0，C 侧 no-op。
    /// </summary>
    [Builtin(ABI = "rt_window_close")]
    public void Close() {
        // body 不执行——codegen 拦截后直射 rt_window_close（句柄取自静态槽）
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        double maxW = 0.0;
        double maxH = 0.0;
        double availW = availableSize.Width;
        double availH = availableSize.Height;
        if (this.Children != null) {
            int count = this.Children.Count;
            for (int i = 0; i < count; i++) {
                Element raw = this.Children[i];
                FrameworkElement child = (FrameworkElement)raw;
                LayoutHelper.MeasureChild(child, new LayoutSize(availW, availH));
                if (child.DesiredSize.Width > maxW) {
                    maxW = child.DesiredSize.Width;
                }
                if (child.DesiredSize.Height > maxH) {
                    maxH = child.DesiredSize.Height;
                }
            }
        }
        if (this.Width > 0.0) {
            maxW = this.Width;
        }
        if (this.Height > 0.0) {
            maxH = this.Height;
        }
        return new LayoutSize(maxW, maxH);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        double fw = finalSize.Width;
        double fh = finalSize.Height;
        // 根窗口：原点固定 (0,0)，子元素绝对坐标 = 0 + 槽位。
        if (this.Children != null && this.Children.Count == 1) {
            Element raw = this.Children[0];
            FrameworkElement child = (FrameworkElement)raw;
            LayoutHelper.ArrangeChild(this, child, 0.0, 0.0, fw, fh);
            return;
        }
        double y = 0.0;
        if (this.Children != null) {
            int count = this.Children.Count;
            for (int i = 0; i < count; i++) {
                Element raw = this.Children[i];
                FrameworkElement child = (FrameworkElement)raw;
                double ch = child.DesiredSize.Height;
                LayoutHelper.ArrangeChild(this, child, 0.0, y, fw, ch);
                y += ch;
            }
        }
    }
}
