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
//   - Command / CommandParameter DP；RaiseClick 在 IsEnabled 通过后若
//     Command is ICommand 且 CanExecute(param) 则 Execute（再触发 Clicked）
//   - CanExecuteChanged → IsEnabled 自动同步（订阅 Signal；赋 Command /
//     改 Parameter 时立即重查；逃逸闭包：回调只按值捕获 route 槽 int）
//
// 使用模式：
//   简单：btn.OnClick(_ => DoSomething());
//   完整：btn.Clicked.Subscribe(_ => DoSomething());
//   MVVM：btn.Command = new RelayCommand(...); cmd.RaiseCanExecuteChanged();

namespace Arc.UI.Components;

using Arc;
using Arc.Collections;
using Arc.UI;
using Arc.UI.Layout;

/// <summary>
/// 按钮控件——Signal 驱动的点击交互。
/// 变体作者面 = <c>Style="{StaticResource Primary, Small}"</c>（短键；查找先控件作用域再全局 Shared）。
/// 禁 Class/Appearance/<c>*.Size.SM</c>；chrome 由已应用 Style 键驱动。
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

    /// <summary>IsPressed DP metadata (pointer pressed).</summary>
    public static DependencyProperty<bool> IsPressedProperty =
        RegisterProperty<bool>(nameof(IsPressed), typeof(Button), false);

    /// <summary>本实例在 <see cref="_commandHosts"/> 中的槽位（-1 = 未登记）。</summary>
    private int _commandRouteSlot;

    /// <summary>当前挂接的 CanExecuteChanged 订阅令牌（-1 = 未订）。</summary>
    private int _canExecuteToken;

    /// <summary>当前挂接的 CanExecuteChanged 信号（退订用）。</summary>
    private Signal<bool> _hookedCanExecute;

    /// <summary>CanExecuteChanged 多宿主路由表：回调按 route id 回查 Button。</summary>
    private static List<Button> _commandHosts;

    // ===== 公共属性 wrapper =====

    /// <summary>命令绑定对象（ICommand 实现）。</summary>
    public object Command {
        get { return this.GetValue<object>(CommandProperty); }
        set {
            this.SetValue<object>(CommandProperty, value);
            this.HookCommand(value);
        }
    }

    /// <summary>命令参数。</summary>
    public object CommandParameter {
        get { return this.GetValue<object>(CommandParameterProperty); }
        set {
            this.SetValue<object>(CommandParameterProperty, value);
            this.SyncIsEnabledFromCommand();
        }
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

    /// <summary>指针是否悬停于按钮上（PointerRouter / 平台镜像同步）。</summary>
    public bool IsMouseOver {
        get { return this.GetValue<bool>(IsMouseOverProperty); }
    }

    /// <summary>指针是否处于按下态（PointerRouter / 平台镜像同步）。</summary>
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
        _commandRouteSlot = -1;
        _canExecuteToken = -1;
        _hookedCanExecute = null;
        // Ant 内容尺寸：竖向 StackPanel 槽虽给满宽，Left 使 chrome 按文案+Padding 收缩（禁拉满栏宽）。
        this.HorizontalAlignment = HorizontalAlignment.Left;
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
    /// <remarks>
    /// 顺序对齐 WPF ButtonBase.OnClick 心智：先 Clicked 信号，再尝试 ICommand。
    /// Command 非 ICommand 或 CanExecute=false 时跳过 Execute（Clicked 仍发）。
    /// </remarks>
    public void RaiseClick() {
        if (!this.IsEnabled) {
            return;
        }
        if (Clicked != null) {
            Clicked.Set(true);
        }
        object cmdObj = this.Command;
        if (cmdObj is ICommand) {
            ICommand cmd = (ICommand)cmdObj;
            object param = this.CommandParameter;
            if (cmd.CanExecute(param)) {
                cmd.Execute(param);
            }
        }
    }

    /// <summary>同步平台指针态——PointerRouter 专用。</summary>
    public void ApplyPointerState(bool isMouseOver, bool isPressed) {
        this.SetValue<bool>(IsMouseOverProperty, isMouseOver);
        this.SetValue<bool>(IsPressedProperty, isPressed);
    }

    /// <summary>
    /// 挂接 Command.CanExecuteChanged：退订旧信号、订阅新信号、立即同步 IsEnabled。
    /// 回调只按值捕获 route 槽 int（禁捕获 this；逃逸闭包 UB 同 ItemsControl）。
    /// </summary>
    private void HookCommand(object cmdObj) {
        this.UnhookCommand();
        if (!(cmdObj is ICommand)) {
            return;
        }
        ICommand cmd = (ICommand)cmdObj;
        Signal<bool> sig = cmd.CanExecuteChanged;
        if (sig != null) {
            this.EnsureCommandRoute();
            int routeSlot = _commandRouteSlot;
            _hookedCanExecute = sig;
            _canExecuteToken = sig.Subscribe((v: bool) => {
                Button.DispatchCanExecuteChanged(routeSlot);
            });
        }
        this.SyncIsEnabledFromCommand();
    }

    /// <summary>退订当前 CanExecuteChanged（换绑 / 清空 Command 时）。</summary>
    private void UnhookCommand() {
        if (_hookedCanExecute != null && _canExecuteToken >= 0) {
            _hookedCanExecute.Unsubscribe(_canExecuteToken);
        }
        _hookedCanExecute = null;
        _canExecuteToken = -1;
    }

    /// <summary>按当前 Command/Parameter 重查 CanExecute 并直写 IsEnabled。</summary>
    /// <remarks>
    /// 诚实边界：非 WPF IsEnabledCore 合取；无 CommandManager.RequerySuggested。
    /// 有 ICommand 时以 CanExecute 结果覆盖本地 IsEnabled。
    /// </remarks>
    private void SyncIsEnabledFromCommand() {
        object cmdObj = this.Command;
        if (!(cmdObj is ICommand)) {
            return;
        }
        ICommand cmd = (ICommand)cmdObj;
        this.IsEnabled = cmd.CanExecute(this.CommandParameter);
    }

    /// <summary>登记多宿主路由槽（幂等）。</summary>
    private void EnsureCommandRoute() {
        if (_commandRouteSlot >= 0) {
            return;
        }
        if (_commandHosts == null) {
            _commandHosts = new List<Button>();
        }
        _commandRouteSlot = _commandHosts.Count;
        _commandHosts.Add(this);
    }

    private static void DispatchCanExecuteChanged(int routeSlot) {
        if (_commandHosts == null || routeSlot < 0 || routeSlot >= _commandHosts.Count) {
            return;
        }
        Button host = _commandHosts[routeSlot];
        if (host == null) {
            return;
        }
        host.SyncIsEnabledFromCommand();
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        if (this.HasTemplateVisual()) {
            LayoutSize templated = this.MeasureTemplateVisual(availableSize);
            double w = templated.Width;
            double h = templated.Height;
            if (this.Width > 0.0) {
                w = this.Width;
            }
            if (this.Height > 0.0) {
                h = this.Height;
            }
            return LayoutHelper.ApplyMinMax(this, new LayoutSize(w, h));
        }
        double padX = LayoutHelper.ButtonPaddingX;
        double padY = LayoutHelper.ButtonPaddingY;
        Thickness pad = Thickness.Parse(this.Padding).Sanitized();
        double padSumX = pad.Left + pad.Right;
        double padSumY = pad.Top + pad.Bottom;
        if (padSumX > 0.0) {
            padX = padSumX;
        }
        if (padSumY > 0.0 || (this.Padding != null && this.Padding != "0,0,0,0")) {
            padY = padSumY;
        }
        LayoutSize est = LayoutHelper.EstimateTextSize(
            ContentHelper.TextOrEmpty(this.Content), this.FontSize,
            padX, padY,
            this.FontFamily, this.FontWeight);
        double bw = est.Width;
        double bh = est.Height;
        double availW = availableSize.Width;
        if (availW > 0.0 && bw > availW) {
            bw = availW;
        }
        if (this.Width > 0.0) {
            bw = this.Width;
        }
        if (this.Height > 0.0) {
            bh = this.Height;
        }
        return LayoutHelper.ApplyMinMax(this, new LayoutSize(bw, bh));
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        if (this.HasTemplateVisual()) {
            this.ArrangeTemplateVisual(finalSize);
        }
    }
}
