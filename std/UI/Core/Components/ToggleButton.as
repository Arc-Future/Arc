// RFC 037 D2.1 / RFC 037 D1: Arc.UI.Components — ToggleButton 切换按钮基类。
//
// WPF 同构层级对照：
//   WPF: ContentControl → ButtonBase → ToggleButton → CheckBox
//   Arc:  ContentControl → ToggleButton → CheckBox（Arc 简化：合并 ButtonBase 角色）
//
// ToggleButton 承载三态切换语义（IsChecked/IsThreeState），
// CheckBox 是 ToggleButton 的语义占位派生类——仅类型区分，无额外成员。
//
// RFC 037 §5.3 控件事件通道：Toggled（IsChecked setter 统一触发，Signal<bool> 携带新勾选值；
// CheckBox 直接继承本通道；既有 Checked/Unchecked/Indeterminate string 字段保持为 ARML 事件处理器名）。
//
// Content/IsEnabled 已从 ContentControl/Control 继承，此处不重复声明。
// ToggleButton 特有 DP：IsChecked/IsThreeState + Checked/Unchecked/Indeterminate 事件名。
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。

namespace Arc.UI.Components;

/// <summary>
/// 切换按钮基类——承载三态切换语义（IsChecked/IsThreeState）。
/// Content/IsEnabled 等通用属性由 ContentControl/Control 继承。
/// </summary>
public class ToggleButton : ContentControl {
    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>IsChecked 属性元数据——是否勾选，默认 false。</summary>
    public static DependencyProperty<bool> IsCheckedProperty =
        RegisterProperty<bool>(nameof(IsChecked), typeof(ToggleButton), false);

    /// <summary>IsThreeState 属性元数据——是否支持三态（含 Indeterminate），默认 false。</summary>
    public static DependencyProperty<bool> IsThreeStateProperty =
        RegisterProperty<bool>(nameof(IsThreeState), typeof(ToggleButton), false);

    /// <summary>IsMouseOver 属性元数据——指针悬停（PointerRouter 同步），默认 false。</summary>
    public static DependencyProperty<bool> IsMouseOverProperty =
        RegisterProperty<bool>(nameof(IsMouseOver), typeof(ToggleButton), false);

    /// <summary>IsPressed 属性元数据——指针按下（PointerRouter 同步），默认 false。</summary>
    public static DependencyProperty<bool> IsPressedProperty =
        RegisterProperty<bool>(nameof(IsPressed), typeof(ToggleButton), false);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>是否勾选。</summary>
    public bool IsChecked {
        get { return this.GetValue<bool>(IsCheckedProperty); }
        set {
            this.SetValue<bool>(IsCheckedProperty, value);
            this.SyncMirrorChecked();
            this.RaiseToggled();
        }
    }

    /// <summary>是否支持三态（true 时支持 Indeterminate 中间态）。</summary>
    public bool IsThreeState {
        get { return this.GetValue<bool>(IsThreeStateProperty); }
        set { this.SetValue<bool>(IsThreeStateProperty, value); }
    }

    /// <summary>指针是否悬停于控件上（PointerRouter 同步）。</summary>
    public bool IsMouseOver {
        get { return this.GetValue<bool>(IsMouseOverProperty); }
    }

    /// <summary>指针是否处于按下态（PointerRouter 同步）。</summary>
    public bool IsPressed {
        get { return this.GetValue<bool>(IsPressedProperty); }
    }

    public ToggleButton() {
        this.Type = typeof(ToggleButton);
        this.Toggled = new Signal<bool>(false);
    }

    // ===== 指针交互（RFC 037 D10.6 · PointerRouter 分发入口）=====

    /// <summary>PointerRouter 点击入口：翻转勾选态（两态；IsThreeState 三态后续 RFC）。</summary>
    public void RaiseToggle() {
        this.IsChecked = !this.IsChecked;
    }

    /// <summary>Enter/Space 默认激活（InputElement.Activate）：等价点击切换。</summary>
    internal override void Activate() {
        this.RaiseToggle();
    }

    /// <summary>同步平台指针态——PointerRouter 专用（同 Button.ApplyPointerState）。</summary>
    public void ApplyPointerState(bool isMouseOver, bool isPressed) {
        this.SetValue<bool>(IsMouseOverProperty, isMouseOver);
        this.SetValue<bool>(IsPressedProperty, isPressed);
    }

    /// <summary>镜像登记（override）：基类句柄存储 + IsChecked 勾选绘制同步（幂等）。</summary>
    public override void BindPlatformMirror(long handle) {
        if (_mirrorHandle == handle) {
            return;
        }
        base.BindPlatformMirror(handle);
        this.SyncMirrorChecked();
    }

    void SyncMirrorChecked() {
        if (_mirrorHandle != 0) {
            int v = this.IsChecked ? 1 : 0;
            WindowHost.ElementSetBool(_mirrorHandle, "IsChecked", v);
        }
    }

    // ===== 控件事件通道（RFC 037 §5.3 · Signal 单引擎 · 与 Button.Clicked/OnClick 同一惯用法）=====
    //
    // Toggled 是勾选状态变更通知：在 IsChecked DP wrapper setter 内 SetValue 后统一触发，
    // 载荷为**新勾选值**（this.IsChecked 读取已落盘值）。CheckBox 为 ToggleButton 的语义
    // 占位派生类，直接继承本通道。既有 Checked/Unchecked/Indeterminate string 字段为
    // ARML 事件处理器名（ARML typeck EventHandler 属性已注册、保持不动），Signal 取 WPF
    // 同构的 Toggled 命名避免冲突。Signal.Set 无相等性短路，同值赋值仍触发。

    /// <summary>
    /// 勾选状态变更信号——IsChecked 属性被赋值后触发，载荷为新勾选值。
    /// 订阅示例：
    /// <code>
    ///   cb.OnToggled(x => DoSomething(x));      // 便捷订阅（CheckBox 继承）
    ///   int t = cb.Toggled.Subscribe(x => ...); // 完整 Subscribe API + token 退订
    /// </code>
    /// </summary>
    public Signal<bool> Toggled;

    /// <summary>订阅勾选状态变更——Toggled.Subscribe 的便捷封装（同 Button.OnClick 惯例）。</summary>
    /// <param name="handler">变更回调（接收新勾选值）。</param>
    public void OnToggled(Action<bool> handler) {
        if (Toggled != null && handler != null) {
            Toggled.Subscribe(handler);
        }
    }

    /// <summary>触发勾选状态变更——IsChecked DP wrapper setter 内调用。</summary>
    private void RaiseToggled() {
        if (Toggled != null) {
            Toggled.Set(this.IsChecked);
        }
    }

    // ===== 事件路由（RFC 037 §7 不在范围，保持 string 方法名）=====
    //
    // Checked/Unchecked/Indeterminate 是状态变更事件处理器名（指向
    // .arml.as partial class 中的方法）。事件路由系统由后续独立 RFC 处理。

    /// <summary>勾选事件处理器名（.arml.as partial class 中的方法名）。</summary>
    public string Checked;

    /// <summary>取消勾选事件处理器名。</summary>
    public string Unchecked;

    /// <summary>进入不确定态事件处理器名（仅 IsThreeState=true 时触发）。</summary>
    public string Indeterminate;
}
