// RFC 037 D2.1 / RFC 037 D1 / M5: Arc.UI.Components — Button 按钮。
//
// Button 是 WPF 同构的 ContentControl 派生类，承载点击交互。
//
// **Signal 替代事件**（Arc 原则 2）：
//   - 废弃 C# event Click → 改用 Clicked : Signal<bool> + OnClick 便捷方法
//   - RaiseClick() 是平台层触发入口（WindowHost / 原生事件 → Button）
//   - 旧 Click: string 保留兼容 ARML Click="MethodName" 语法
//
// **ICommand 模式**（可选，MVVM 场景）：
//   - Command / CommandParameter DP 保留（RoutedCommand 预留）
//   - ICommand 接口定义见 ICommand.as
//
// 使用模式：
//   简单：btn.OnClick(_ => DoSomething());
//   完整：btn.Clicked.Subscribe(_ => DoSomething());
//   MVVM：btn.Command = myCommand;  // ICommand 实现

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Layout;

/// <summary>
/// 按钮控件——Signal 驱动的点击交互。
/// </summary>
public class Button : ContentControl {
    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Command 属性元数据——ICommand 实现对象，默认 null。</summary>
    public static DependencyProperty<object> CommandProperty =
        RegisterProperty<object>(nameof(Command), typeof(Button), null);

    /// <summary>CommandParameter 属性元数据——命令参数，默认 null。</summary>
    public static DependencyProperty<object> CommandParameterProperty =
        RegisterProperty<object>(nameof(CommandParameter), typeof(Button), null);

    /// <summary>IsDefault 属性元数据——是否为默认按钮（Enter 触发），默认 false。</summary>
    public static DependencyProperty<bool> IsDefaultProperty =
        RegisterProperty<bool>(nameof(IsDefault), typeof(Button), false);

    /// <summary>IsCancel 属性元数据——是否为取消按钮（Esc 触发），默认 false。</summary>
    public static DependencyProperty<bool> IsCancelProperty =
        RegisterProperty<bool>(nameof(IsCancel), typeof(Button), false);

    /// <summary>IsMouseOver 属性元数据——指针悬停（平台同步），默认 false。</summary>
    public static DependencyProperty<bool> IsMouseOverProperty =
        RegisterProperty<bool>(nameof(IsMouseOver), typeof(Button), false);

    /// <summary>IsPressed 属性元数据——指针按下（平台同步），默认 false。</summary>
    public static DependencyProperty<bool> IsPressedProperty =
        RegisterProperty<bool>(nameof(IsPressed), typeof(Button), false);

    // ===== 公共属性 wrapper =====

    /// <summary>命令绑定对象（ICommand 实现）。</summary>
    public object Command {
        get { return this.GetValue<object>(CommandProperty); }
        set { this.SetValue<object>(CommandProperty, value); }
    }

    /// <summary>命令参数。</summary>
    public object CommandParameter {
        get { return this.GetValue<object>(CommandParameterProperty); }
        set { this.SetValue<object>(CommandParameterProperty, value); }
    }

    /// <summary>是否为默认按钮（Enter 触发）。</summary>
    public bool IsDefault {
        get { return this.GetValue<bool>(IsDefaultProperty); }
        set { this.SetValue<bool>(IsDefaultProperty, value); }
    }

    /// <summary>是否为取消按钮（Esc 触发）。</summary>
    public bool IsCancel {
        get { return this.GetValue<bool>(IsCancelProperty); }
        set { this.SetValue<bool>(IsCancelProperty, value); }
    }

    /// <summary>指针是否悬停于按钮上（Win32 软件路径同步）。</summary>
    public bool IsMouseOver {
        get { return this.GetValue<bool>(IsMouseOverProperty); }
    }

    /// <summary>指针是否处于按下态（Win32 软件路径同步）。</summary>
    public bool IsPressed {
        get { return this.GetValue<bool>(IsPressedProperty); }
    }

    // ============================================================
    // Signal 驱动的点击交互（替代 C# event Click）
    // ============================================================

    /// <summary>
    /// 点击信号——按钮被点击时触发。
    /// 订阅示例：
    /// <code>
    ///   button.Clicked.Set(true);                    // 手动触发
    ///   button.OnClick(_ => DoSomething());          // 便捷订阅
    ///   button.Clicked.Subscribe(_ => DoSomething());// 完整 Subscribe API
    /// </code>
    /// </summary>
    public Signal<bool> Clicked;

    /// <summary>旧兼容字段：ARML Click="MethodName" 对应的方法名。</summary>
    public string Click;

    public Button() {
        this.Type = typeof(Button);
        this.Clicked = new Signal<bool>(false);
    }

    /// <summary>Enter/Space 默认激活（InputElement.Activate）：等价点击。</summary>
    internal override void Activate() {
        this.RaiseClick();
    }

    /// <summary>订阅点击事件——Clicked.Subscribe 的便捷封装。</summary>
    /// <param name="handler">点击回调（接收 bool 参数，值始终为 true）。</param>
    public void OnClick(Action<bool> handler) {
        if (Clicked != null && handler != null) {
            Clicked.Subscribe(handler);
        }
    }

    /// <summary>触发点击——由平台层（WindowHost / native event loop）调用。</summary>
    public void RaiseClick() {
        if (Clicked != null) {
            Clicked.Set(true);
        }
    }

    /// <summary>同步平台指针态——PointerRouter 专用。</summary>
    public void ApplyPointerState(bool isMouseOver, bool isPressed) {
        this.SetValue<bool>(IsMouseOverProperty, isMouseOver);
        this.SetValue<bool>(IsPressedProperty, isPressed);
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        LayoutSize est = LayoutHelper.EstimateTextSize(
            ContentHelper.TextOrEmpty(this.Content), this.FontSize,
            LayoutHelper.ButtonPaddingX, LayoutHelper.ButtonPaddingY,
            this.FontFamily, this.FontWeight);
        double w = est.Width;
        double h = est.Height;
        double availW = availableSize.Width;
        if (availW > 0.0 && w > availW) {
            w = availW;
        }
        if (this.Width > 0.0) {
            w = this.Width;
        }
        if (this.Height > 0.0) {
            h = this.Height;
        }
        return new LayoutSize(w, h);
    }
}
