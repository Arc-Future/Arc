// RFC 037 · Popup 弹出层体系（std 轨道首切片）。
//
// **定位**：模态浮层宿主——蒙层（全窗口半透明拦截层）+ Child 内容挂在已运行
// 窗口平台镜像树的末尾。rt_ui hit_test 对 children 逆序遍历（后添加优先命中），
// 层根挂主树根 children 末尾即天然置顶：输入/渲染/同步三轨零 C 侧、零 codegen 改动。
//
// **三轨架构**（对齐 RFC 037 §6 三层同构契约）：
//   std 层：本文件——层根/蒙层/Child 的 Arc 侧组织 + 手动 Measure/Arrange；
//   同步轨：复用 PlatformTreeSync.BuildFromArc / SyncLayoutFromArc——层根是
//           独立 Arc 子树根，公共尾部统一镜像 Layout* 四项；层根子树内的
//           Button/TextBox/ListView 等经既有分支自动接入输入轨（注册/焦点/滚轮）。
//   渲染轨：最小改动——WgpuRender 增 PopupLayer/PopupBackdrop 类型常量与「仅背景 +
//           子树通用递归」分支（RenderTree.as）。设计时假设存在未知 TypeName 兜底
//           背景分支，核实后发现该分支仅匹配 Window/Element，故补显式分支；
//           Child 子树内控件仍零改动经通用递归渲染。置顶依据：层根挂窗口平台根
//           children 末尾，渲染 forward 顺序 = painter's algorithm 后画在上，
//           与 hit_test 逆序命中同源（同一 children 顺序两种遍历）。
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
//   4. 蒙层回调注册挂 PointerRouter.Install（按 TypeName 全局注册，随控件回调
//      同一 Show 会话重注册——ClearControlHandlers 每 Show 清空，一次性标志跨
//      会话失效）；点击路由仍由 Popup 自持活跃表 _activePopups 按蒙层句柄匹配
//      实例（不走控件槽表，蒙层语义为窗口级拦截而非控件注册）。
//   5. C 侧 IsEnabled 未写即放行命中（rt_ui_pointer 仅在显式 false 时拒绝），
//      蒙层不写 IsEnabled 即可接收点击。
//
// **诚实边界**：v1 为模态语义（点击蒙层即关闭，下层控件被蒙层拦截）；点内容
// 空白间隙会落到蒙层（内容子树非命中目标的区域穿透）。DPI 缩放下蒙层与窗口
// 客户区的精确对齐、非模态（StaysOpen）与 Placement 目标定位增强另排。
// 层根句柄随宿主窗口重建代数：宿主再次 Show 重建主树后旧镜像句柄全部悬空
// （句柄号可被新树回收复用），经 RootEpoch 检测重走建树路径（成本与窗口自身
// 重建同级）；重建时打开中的 Popup 成为僵尸（IsOpen 悬挂、句柄失效），Close
// 跳过失效句柄写、路由跳过跨代匹配——下次 Close/重开自愈。
// 首个消费方：ComboBox 展开态。
//
// 冲突处理：与 RFC 037 既有小节无冲突；Popup 形态以本实现 + production-surface
// 增补为准。

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.ComponentModel;
using Arc.UI;
using Arc.UI.Internal;
using Arc.UI.Layout;

/// <summary>
/// 模态弹出层宿主：蒙层 + Child 内容挂已运行窗口平台镜像树末尾，天然置顶。
/// 用法：popup.Child = content → popup.PlacementX/PlacementY 定位 →
/// popup.Open() / popup.Close()；点击蒙层自动 Close，经 Opened/Closed 信号感知。
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
    private static List<Popup> _activePopups;

    /// <summary>构造并组装层根（TypeName 显式赋值契约见文件头）。</summary>
    public Popup() {
        this.Type = typeof(Popup);
        this.TypeName = "Popup";
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
    /// 展开弹层：上溯逻辑树找宿主 Window（无宿主或未运行时静默 no-op——层根必须
    /// 挂已运行窗口的平台根）。首次或宿主重建后（RootEpoch 前移、旧镜像句柄悬空）
    /// 走 BuildFromArc 独立建树 + 挂主树末尾 + 补写蒙层背景；同会话重开按既有
    /// 句柄 SyncLayoutFromArc 恢复几何，不重建树。
    /// </summary>
    public void Open() {
        if (this.IsOpen) {
            return;
        }
        Window? ownerFound = FindOwnerWindow();
        if (ownerFound == null) {
            return;
        }
        Window owner = ownerFound;
        double winW = owner.Width;
        double winH = owner.Height;
        if (winW <= 0.0 && owner.DesiredSize.Width > 0.0) {
            winW = owner.DesiredSize.Width;
        }
        if (winH <= 0.0 && owner.DesiredSize.Height > 0.0) {
            winH = owner.DesiredSize.Height;
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
            WindowHost.ElementAddChild(owner.PlatformRootHandle, _layerRootHandle);
            _backdropHandle = WindowHost.ElementGetChild(_layerRootHandle, 0);
            WindowHost.ElementSetString(_backdropHandle, "Background", "#80000000");
            _builtEpoch = PlatformTreeSync.RootEpoch;
        } else {
            PlatformTreeSync.SyncLayoutFromArc(_layerRoot, _layerRootHandle);
        }
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
    /// 撤出活跃表。Arc 侧层根布局字段不动——重开由 SyncLayoutFromArc 重写。
    /// 宿主重建后的僵尸实例跳过失效句柄写（句柄号可能已被新树回收复用）、仅复位
    /// 状态——下次 Open 经 RootEpoch 检测重走建树路径（自愈，见文件头诚实边界）。
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
    /// 手动布局层根子树：层根/蒙层铺满窗口，Child 按 Placement 定位、以期望
    /// 尺寸摆放（可用空间截至窗口边缘）。Panel 无布局覆写，此处即布局权威。
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
            LayoutSize available = new LayoutSize(winW - x, winH - y);
            LayoutHelper.MeasureChild(content, available);
            LayoutHelper.ArrangeChild(_layerRoot, content, x, y,
                content.DesiredSize.Width, content.DesiredSize.Height);
        }
    }

    // ===== 蒙层回调路由 =====

    /// <summary>C 侧蒙层点击入口：按蒙层句柄匹配活跃实例并关闭（不走 PointerRouter 槽表）。</summary>
    internal static void RouteBackdropClick(long backdropHandle) {
        if (_activePopups == null) {
            return;
        }
        int i = 0;
        while (i < _activePopups.Count) {
            Popup popup = _activePopups[i];
            if (popup._builtEpoch == PlatformTreeSync.RootEpoch && popup._backdropHandle == backdropHandle) {
                popup.Close();
                return;
            }
            i++;
        }
    }

    // ===== 关闭语义：镜像树移出视口 =====

    /// <summary>递归 C 侧镜像树，逐节点写 LayoutX/LayoutY 移出视口（渲染裁剪整棵剔除）。</summary>
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

    /// <summary>沿逻辑树 Parent 上溯找宿主 Window（is 检查 + 强转，对齐 PlatformTreeSync 先例）。</summary>
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
        return owner;
    }
}
