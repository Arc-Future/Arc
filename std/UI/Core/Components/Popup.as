// RFC 037 · Popup 弹出层体系（std 轨道）。
//
// **定位**：浮层宿主——蒙层（全窗口半透明拦截层）+ Child 内容挂在已运行
// 窗口平台镜像树末尾。rt_ui hit_test 对 children 逆序遍历（后挂优先命中），
// 渲染 forward = painter's algorithm 后画在上——与命中同源。
//
// **多弹层 Z 序（同窗口）**：后 Open 在上。Open 每次对层根调用
// ElementAddChild（`rt_ui_element_add_child` 同父已挂载时移至末尾）；活跃表
// `_activePopups` 按 Open 顺序追加，与平台 children 末尾序一致。Esc 严格 LIFO：
// 只裁决活跃表末尾一层（轻关闭则关；非轻关闭交给 MessageBox 等自管，不穿透）。
// 蒙层点击：命中顶层 PopupBackdrop（全窗蒙层遮挡下层）→ RouteBackdropClick。
//
// **三轨架构**（对齐 RFC 037 §6 三层同构契约）：
//   std 层：本文件——层根/蒙层/Child 的 Arc 侧组织 + 手动 Measure/Arrange；
//   同步轨：复用 PlatformTreeSync.BuildFromArc / SyncLayoutFromArc——层根是
//           独立 Arc 子树根，公共尾部统一镜像 Layout* 四项；层根子树内的
//           Button/TextBox/ListView 等经既有分支自动接入输入轨（注册/焦点/滚轮）。
//   渲染轨：WgpuRender PopupLayer/PopupBackdrop「仅背景 + 子树通用递归」；
//           置顶靠层根在窗口平台根 children 末尾（见上 Z 序），无专属 Z 字段。
//
// **关键契约**：
//   1. TypeName 显式赋值：手写 new 的元素不经 .arml codegen 注入，必须显式
//      赋 TypeName，否则 BuildFromArc 回退 "Element"。层根 "PopupLayer" /
//      蒙层 "PopupBackdrop" 为专属名——control handler 按 TypeName 全局注册，
//      复用 Rectangle 等既有名会污染全部同名实例。
//   2. 蒙层 Background 由 Popup 直写平台镜像：BuildFromArc 对未知 TypeName
//      只镜像公共几何尾部，不识 Panel.Background。
//   3. 关闭语义：WindowHost 无移除子元素 ABI——Close 递归镜像树写
//      LayoutX/LayoutY = -1e6 移出视口（渲染视口裁剪对完全出界子树整棵剔除，
//      零渲染消耗）；重开经 SyncLayoutFromArc 按既有句柄恢复几何，不重建树。
//   4. 蒙层回调：PointerRouter.Install 按 TypeName "PopupBackdrop" 注册点击 →
//      RouteBackdropClick；随 ClearControlHandlers 每 Show 重注册。点击路由由
//      Popup 自持活跃表 _activePopups 按蒙层句柄匹配实例。
//   5. C 侧 IsEnabled 未写即放行命中（rt_ui_pointer 仅在显式 false 时拒绝），
//      蒙层不写 IsEnabled 即可接收点击。
//   6. Owner：Open(Window) 显式宿主优先；Open() 上溯 Parent，再回退
//      Application.MainWindow（ComboBox 等浮层不在逻辑树内，必须可无 Parent）。
//   7. IsLightDismissEnabled：true（默认）= 蒙层点击 / Esc 关闭（ComboBox）；
//      false = 点蒙层与 Esc 不关（MessageBox 等模态对话框前置挂钩）。
//
// **诚实边界**：轻关闭 + Owner + Esc（KeyboardRouter）+ 窗口内翻 + 钳高 ScrollView
// + **同窗口多弹层 Z 序（后开在上 · Esc/蒙层 LIFO）** 已接。非模态无蒙层
// （StaysOpen 无 backdrop）另排。层根句柄随宿主重建：RootEpoch 重走建树；
// 僵尸 Close 跳过失效句柄。消费方：ComboBox（轻关闭）；MessageBox
// （IsLightDismissEnabled=false + Esc 自管）。

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.ComponentModel;
using Arc.UI;
using Arc.UI.Internal;
using Arc.UI.Layout;
using Arc.UI.Styling;

/// <summary>
/// 浮层宿主：蒙层 + Child；同窗口多开时后 Open 置顶（平台 children 末尾）。
/// 用法：popup.Child = content → PlacementX/Y → Open() / Open(owner) / Close()；
/// 轻关闭（IsLightDismissEnabled）时蒙层点击与 Esc（LIFO）自动 Close。
/// </summary>
public class Popup : FrameworkElement {

    // ===== 内容与定位 =====

    /// <summary>Child 内容依赖属性元数据（对齐 ScrollView.Content 先例）。</summary>
    public static DependencyProperty<FrameworkElement> ChildProperty =
        RegisterProperty<FrameworkElement>(nameof(Child), typeof(Popup), null);

    /// <summary>弹出内容（挂层根内蒙层之上；setter 维护层根 Children 组装，蒙层恒居 index 0）。</summary>
    public FrameworkElement Child {
        get { return this.GetValue<FrameworkElement>(ChildProperty); }
        set {
            FrameworkElement previous = this.Child;
            if (previous != null) {
                _layerRoot.Children.Remove(previous);
            }
            this.SetValue<FrameworkElement>(ChildProperty, value);
            if (value != null) {
                _layerRoot.Children.Add(value);
            }
        }
    }

    /// <summary>内容左上角的窗口内逻辑坐标 X（内容子树自行滚动/换行不回写此值）。</summary>
    public double PlacementX { get; set; }

    /// <summary>内容左上角的窗口内逻辑坐标 Y。</summary>
    public double PlacementY { get; set; }

    /// <summary>
    /// 是否允许轻关闭：蒙层点击与 Esc 关闭本层。默认 true（下拉类）；
    /// MessageBox 等模态对话框应置 false（点蒙层不关；Esc 策略由对话框自管）。
    /// </summary>
    public bool IsLightDismissEnabled { get; set; }

    /// <summary>
    /// 窗口内翻定位：优先在 <paramref name="preferredY"/> 下方展开；垂直溢出时翻到
    /// <paramref name="anchorTop"/> 上方；两侧皆不足时取空间较大侧并钳高。水平方向
    /// 钳入窗口。消费方（ComboBox 等）在 Open 前调用，将结果写入 Placement* 与内容 Height。
    /// </summary>
    public static void ComputeInWindowPlacement(
        double preferredX,
        double preferredY,
        double anchorTop,
        double contentWidth,
        double contentHeight,
        double windowWidth,
        double windowHeight,
        ref double placementX,
        ref double placementY,
        ref double fittedHeight)
    {
        double winW = windowWidth;
        double winH = windowHeight;
        if (winW < 0.0) {
            winW = 0.0;
        }
        if (winH < 0.0) {
            winH = 0.0;
        }
        double w = contentWidth;
        if (w < 0.0) {
            w = 0.0;
        }
        double h = contentHeight;
        if (h < 0.0) {
            h = 0.0;
        }

        double x = preferredX;
        if (x + w > winW) {
            x = winW - w;
        }
        if (x < 0.0) {
            x = 0.0;
        }

        double spaceBelow = winH - preferredY;
        if (spaceBelow < 0.0) {
            spaceBelow = 0.0;
        }
        double spaceAbove = anchorTop;
        if (spaceAbove < 0.0) {
            spaceAbove = 0.0;
        }

        double y;
        double fitted = h;
        if (h <= spaceBelow) {
            y = preferredY;
            fitted = h;
        } else if (h <= spaceAbove) {
            y = anchorTop - h;
            fitted = h;
        } else if (spaceBelow >= spaceAbove) {
            y = preferredY;
            fitted = spaceBelow;
        } else {
            fitted = spaceAbove;
            y = anchorTop - fitted;
        }
        if (y < 0.0) {
            y = 0.0;
        }
        if (y + fitted > winH) {
            fitted = winH - y;
        }
        if (fitted < 0.0) {
            fitted = 0.0;
        }

        placementX = x;
        placementY = y;
        fittedHeight = fitted;
    }

    // ===== 状态与信号 =====

    /// <summary>当前是否展开（Open/Close 维护；外部经方法开关，不暴露 setter）。</summary>
    public bool IsOpen { get; private set; }

    /// <summary>展开状态信号（true=已展开）；便捷订阅见 OnOpened。</summary>
    public Signal<bool> Opened;

    /// <summary>关闭状态信号（false=已关闭）；便捷订阅见 OnClosed。</summary>
    public Signal<bool> Closed;

    // ===== 平台轨私有状态 =====

    private Panel _layerRoot;
    private Panel _backdrop;
    private long _layerRootHandle;
    private long _backdropHandle;
    private int _builtEpoch;
    private Window? _ownerWindow;
    private static List<Popup> _activePopups;

    /// <summary>构造并组装层根（TypeName 显式赋值契约见文件头）。</summary>
    public Popup() {
        this.Type = typeof(Popup);
        this.TypeName = "Popup";
        this.IsLightDismissEnabled = true;
        this.Opened = new Signal<bool>(false);
        this.Closed = new Signal<bool>(false);
        _layerRoot = new Panel();
        _layerRoot.TypeName = "PopupLayer";
        _backdrop = new Panel();
        _backdrop.TypeName = "PopupBackdrop";
        _layerRoot.Children.Add(_backdrop);
    }

    // ===== 开关入口 =====

    /// <summary>
    /// 展开弹层（宿主解析：Parent 上溯 → Application.MainWindow）。
    /// 浮层常不在逻辑树内，宜改用 <see cref="Open(Window)"/> 显式 Owner。
    /// </summary>
    public void Open() {
        this.Open(null);
    }

    /// <summary>
    /// 展开弹层到指定宿主窗口。owner 为 null 时上溯 Parent，再回退 MainWindow。
    /// 首次或宿主重建后（RootEpoch 前移）走 BuildFromArc；同会话重开 SyncLayout。
    /// 每次 Open 末尾 ElementAddChild 层根——同父已挂载时 C 侧移至末尾（Z 序置顶）。
    /// </summary>
    public void Open(Window? owner) {
        if (this.IsOpen) {
            return;
        }
        Window? ownerFound = owner;
        if (ownerFound == null) {
            ownerFound = FindOwnerWindow();
        }
        if (ownerFound == null && Application.Current != null) {
            ownerFound = Application.Current.MainWindow;
        }
        if (ownerFound == null) {
            return;
        }
        Window resolved = ownerFound;
        if (resolved.PlatformRootHandle == 0) {
            return;
        }
        _ownerWindow = resolved;
        double winW = resolved.Width;
        double winH = resolved.Height;
        if (winW <= 0.0 && resolved.DesiredSize.Width > 0.0) {
            winW = resolved.DesiredSize.Width;
        }
        if (winH <= 0.0 && resolved.DesiredSize.Height > 0.0) {
            winH = resolved.DesiredSize.Height;
        }
        if (winW <= 0.0) {
            winW = 720.0;
        }
        if (winH <= 0.0) {
            winH = 480.0;
        }

        this.LayoutPopupContent(winW, winH);

        if (_layerRootHandle == 0 || _builtEpoch != PlatformTreeSync.RootEpoch) {
            _layerRootHandle = PlatformTreeSync.BuildFromArc(_layerRoot);
            _backdropHandle = WindowHost.ElementGetChild(_layerRootHandle, 0);
            string overlay = "#00000000";
            if (Application.Current != null) {
                string resolvedOverlay = Application.Current.ResolveColor(BuiltInTheme.Overlay);
                if (resolvedOverlay != null && resolvedOverlay.Length > 0) {
                    overlay = resolvedOverlay;
                }
            }
            WindowHost.ElementSetString(_backdropHandle, "Background", overlay);
            _builtEpoch = PlatformTreeSync.RootEpoch;
        } else {
            PlatformTreeSync.SyncLayoutFromArc(_layerRoot, _layerRootHandle);
        }
        // 首次挂载与复开置顶：同父已挂载时 rt_ui_element_add_child 移至末尾。
        WindowHost.ElementAddChild(resolved.PlatformRootHandle, _layerRootHandle);
        if (_activePopups == null) {
            _activePopups = new List<Popup>();
        }
        _activePopups.Add(this);
        WindowHost.InvalidateActiveWindow();
        this.IsOpen = true;
        this.RaiseOpened();
    }

    /// <summary>
    /// 关闭弹层：递归镜像树移出视口（无移除 ABI 的诚实替代，见文件头契约 3），
    /// 撤出活跃表。宿主重建后的僵尸实例跳过失效句柄写。
    /// </summary>
    public void Close() {
        if (!this.IsOpen) {
            return;
        }
        if (_layerRootHandle != 0 && _builtEpoch == PlatformTreeSync.RootEpoch) {
            HideMirrorTree(_layerRootHandle);
            WindowHost.InvalidateActiveWindow();
        }
        if (_activePopups != null) {
            _activePopups.Remove(this);
        }
        this.IsOpen = false;
        this.RaiseClosed();
    }

    // ===== 信号便捷订阅 =====

    /// <summary>订阅展开信号（ToggleButton.OnToggled 同款两件套）。</summary>
    public void OnOpened(Action<bool> handler) {
        if (Opened != null && handler != null) {
            Opened.Subscribe(handler);
        }
    }

    /// <summary>订阅关闭信号。</summary>
    public void OnClosed(Action<bool> handler) {
        if (Closed != null && handler != null) {
            Closed.Subscribe(handler);
        }
    }

    void RaiseOpened() {
        if (Opened != null) {
            Opened.Set(true);
        }
    }

    void RaiseClosed() {
        if (Closed != null) {
            Closed.Set(false);
        }
    }

    // ===== 布局 =====

    /// <summary>
    /// 手动布局层根子树：层根/蒙层铺满窗口，Child 按 Placement 定位。
    /// 水平/垂直钳入客户区（消费方宜先经 ComputeInWindowPlacement）。
    /// </summary>
    void LayoutPopupContent(double winW, double winH) {
        _layerRoot.Width = winW;
        _layerRoot.Height = winH;
        _layerRoot.Measure(new LayoutSize(winW, winH));
        _layerRoot.Arrange(new LayoutSize(winW, winH));
        LayoutHelper.MeasureChild(_backdrop, new LayoutSize(winW, winH));
        LayoutHelper.ArrangeChild(_layerRoot, _backdrop, 0.0, 0.0, winW, winH);
        FrameworkElement content = this.Child;
        if (content != null) {
            double x = this.PlacementX;
            double y = this.PlacementY;
            if (x < 0.0) {
                x = 0.0;
            }
            if (y < 0.0) {
                y = 0.0;
            }
            LayoutSize available = new LayoutSize(winW - x, winH - y);
            LayoutHelper.MeasureChild(content, available);
            double w = content.DesiredSize.Width;
            double h = content.DesiredSize.Height;
            if (x + w > winW) {
                x = winW - w;
                if (x < 0.0) {
                    x = 0.0;
                }
            }
            if (y + h > winH) {
                h = winH - y;
                if (h < 0.0) {
                    h = 0.0;
                }
            }
            LayoutHelper.ArrangeChild(_layerRoot, content, x, y, w, h);
        }
    }

    // ===== 蒙层 / Esc 关闭路由 =====

    /// <summary>C 侧蒙层点击入口：仅 IsLightDismissEnabled 时按句柄关闭匹配实例。</summary>
    internal static void RouteBackdropClick(long backdropHandle) {
        if (_activePopups == null) {
            return;
        }
        int i = 0;
        while (i < _activePopups.Count) {
            Popup popup = _activePopups[i];
            if (popup._builtEpoch == PlatformTreeSync.RootEpoch
                && popup._backdropHandle == backdropHandle)
            {
                if (popup.IsLightDismissEnabled) {
                    popup.Close();
                }
                return;
            }
            i++;
        }
    }

    /// <summary>
    /// Esc：只裁决活跃表末尾一层（与平台 Z 序一致）。轻关闭则 Close 并消费；
    /// 顶层非轻关闭返回 false（交 MessageBox 等自管，不穿透下层）。
    /// </summary>
    internal static bool TryDismissTopOnEscape() {
        if (_activePopups == null || _activePopups.Count == 0) {
            return false;
        }
        int i = _activePopups.Count - 1;
        while (i >= 0) {
            Popup popup = _activePopups[i];
            if (popup.IsOpen && popup._builtEpoch == PlatformTreeSync.RootEpoch) {
                if (popup.IsLightDismissEnabled) {
                    popup.Close();
                    return true;
                }
                return false;
            }
            i--;
        }
        return false;
    }

    /// <summary>是否存在打开中的弹层（平台 Esc 退窗前可先询；现由 KeyboardRouter 消费）。</summary>
    internal static bool HasActivePopup() {
        if (_activePopups == null) {
            return false;
        }
        return _activePopups.Count > 0;
    }

    // ===== 关闭语义：镜像树移出视口 =====

    /// <summary>递归 C 侧镜像树，逐节点写 LayoutX/LayoutY 移出视口。</summary>
    static void HideMirrorTree(long handle) {
        double offscreen = -1000000.0;
        WindowHost.ElementSetNumber(handle, "LayoutX", offscreen);
        WindowHost.ElementSetNumber(handle, "LayoutY", offscreen);
        int count = WindowHost.ElementGetChildCount(handle);
        int i = 0;
        while (i < count) {
            long child = WindowHost.ElementGetChild(handle, i);
            if (child != 0) {
                HideMirrorTree(child);
            }
            i++;
        }
    }

    // ===== 宿主解析 =====

    /// <summary>沿逻辑树 Parent 上溯找宿主 Window。</summary>
    Window? FindOwnerWindow() {
        Element? node = this.Parent;
        Window? owner = null;
        while (node != null && owner == null) {
            if (node is Window) {
                owner = (Window)node;
            } else {
                node = node?.Parent;
            }
        }
        if (owner == null) {
            owner = _ownerWindow;
        }
        return owner;
    }
}
