// RFC 037 D2.1 + RFC 037 D1: Arc.UI.Components —— Slider 元素。
//
// Slider 是数值滑块控件，承载用户在 [Minimum, Maximum] 区间内的数值选择。
//
// WPF 同构层级对照：
//   WPF: Control → Slider（WPF 中间还有 RangeBase，Arc 简化合并）
//   Arc:  Control → Slider
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。
//
// 编码模型要点：
//   - nameof(属性) 替代字符串字面量——IDE 重构可自动追踪符号引用
//   - typeof(类) 替代字符串字面量——避免魔法字符串与重构不同步
//   - 默认值用类型化字面量（0.0/100.0/1.0 等），值类型零装箱

// RFC 037 §5.3 控件事件通道：ValueChanged（Signal<double>，Value setter 统一触发，
// 同 Button.Clicked/OnClick 惯例；既有 string 占位字段更名 ValueChangedHandler 让出事件名）。

namespace Arc.UI.Components;

using Arc.UI.Layout;

/// <summary>数值滑块控件，承载 [Minimum, Maximum] 区间内的数值选择。</summary>
public class Slider : InputElement {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public Slider() {
        this.Type = typeof(Slider);
    }

    // ===== 静态依赖属性元数据（RFC 051 D1 WPF 同构）=====

    /// <summary>Value 属性元数据——当前滑块值，默认 0.0。</summary>
    public static DependencyProperty<double> ValueProperty =
        RegisterProperty<double>(nameof(Value), typeof(Slider), 0.0);

    /// <summary>Minimum 属性元数据——可选最小值，默认 0.0。</summary>
    public static DependencyProperty<double> MinimumProperty =
        RegisterProperty<double>(nameof(Minimum), typeof(Slider), 0.0);

    /// <summary>Maximum 属性元数据——可选最大值，默认 100.0。</summary>
    public static DependencyProperty<double> MaximumProperty =
        RegisterProperty<double>(nameof(Maximum), typeof(Slider), 100.0);

    /// <summary>Step 属性元数据——步进值，默认 1.0。</summary>
    public static DependencyProperty<double> StepProperty =
        RegisterProperty<double>(nameof(Step), typeof(Slider), 1.0);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>当前滑块值。</summary>
    public double Value {
        get { return this.GetValue<double>(ValueProperty); }
        set {
            double clamped = value;
            double min = this.Minimum;
            double max = this.Maximum;
            if (clamped < min) { clamped = min; }
            if (clamped > max) { clamped = max; }
            this.SetValue<double>(ValueProperty, clamped);
            this.RaiseValueChanged();
        }
    }

    /// <summary>可选最小值。</summary>
    public double Minimum {
        get { return this.GetValue<double>(MinimumProperty); }
        set { this.SetValue<double>(MinimumProperty, value); }
    }

    /// <summary>可选最大值。</summary>
    public double Maximum {
        get { return this.GetValue<double>(MaximumProperty); }
        set { this.SetValue<double>(MaximumProperty, value); }
    }

    /// <summary>步进值。</summary>
    public double Step {
        get { return this.GetValue<double>(StepProperty); }
        set { this.SetValue<double>(StepProperty, value); }
    }

    public Slider() {
        this.Type = typeof(Slider);
        this.ValueChanged = new Signal<double>(0.0);
    }

    // ===== 控件事件通道（RFC 037 §5.3 · Signal 单引擎 · 与 Button.Clicked/OnClick 同一惯用法）=====
    //
    // ValueChanged 是滑块值变更通知：在 Value DP wrapper setter 内 SetValue 后统一触发，
    // 载荷为**新滑块值**（this.Value 读取已落盘值）。Signal.Set 无相等性短路，同值赋值
    // 仍触发。On* 便捷订阅内部弃 token（常驻订阅随元素销毁确定退订；WS-C 规则仅约束
    // 用户面出口，不误伤 std 内部）。

    /// <summary>
    /// 值变更信号——Value 属性被赋值后触发，载荷为新滑块值。
    /// 订阅示例：
    /// <code>
    ///   sl.OnValueChanged(x => DoSomething(x));      // 便捷订阅
    ///   int t = sl.ValueChanged.Subscribe(x => ...); // 完整 Subscribe API + token 退订
    /// </code>
    /// </summary>
    public Signal<double> ValueChanged;

    /// <summary>订阅值变更——ValueChanged.Subscribe 的便捷封装（同 Button.OnClick 惯例）。</summary>
    /// <param name="handler">变更回调（接收新滑块值）。</param>
    public void OnValueChanged(Action<double> handler) {
        if (ValueChanged != null && handler != null) {
            ValueChanged.Subscribe(handler);
        }
    }

    /// <summary>触发值变更——Value DP wrapper setter 内调用。</summary>
    private void RaiseValueChanged() {
        if (ValueChanged != null) {
            ValueChanged.Set(this.Value);
        }
    }

    // ===== 指针交互（RFC 037 D10.6 · PointerRouter 分发入口）=====

    /// <summary>IsDragging 属性元数据——thumb 拖拽中为 true，默认 false。</summary>
    public static DependencyProperty<bool> IsDraggingProperty =
        RegisterProperty<bool>(nameof(IsDragging), typeof(Slider), false);

    /// <summary>是否正在拖拽 thumb（PointerRouter 同步）。</summary>
    public bool IsDragging {
        get { return this.GetValue<bool>(IsDraggingProperty); }
    }

    /// <summary>PointerRouter 拖拽/跳转入口：按 Step 就近取整并写入 Value（clamp 在 Value setter 内）。</summary>
    public void ApplyDragValue(double value) {
        double snapped = value;
        double step = this.Step;
        if (step > 0.0) {
            snapped = Math.Round(value / step) * step;
        }
        this.Value = snapped;
    }

    // ===== 事件路由（RFC 051 §7 不在范围，保持 string 方法名）=====
    //
    // ValueChanged 是值变更事件处理器名（指向 .arml.as partial class 中的方法）。
    // 事件路由系统（Signal 路由 vs Command 模式）由后续独立 RFC 处理。

    /// <summary>鍊煎彉鏇翠簨浠跺鐞嗗櫒鍚嶏紙.arml.as partial class 涓殑鏂规硶鍚嶏級銆</summary>
    /// <summary>值变更事件处理器名（.arml.as partial class 中的方法名）。
    /// RFC 037 §5.3 起事件通道由 <c>Signal&lt;double&gt; ValueChanged</c> 承载，
    /// 本 string 占位字段更名 ValueChangedHandler 让出事件名——ARML typeck 未注册
    /// 该事件属性、全仓零引用（死占位），更名无行为影响。</summary>
    public string ValueChangedHandler;

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        double w = 200.0;
        double h = 24.0;
        if (w < 120.0) {
            w = 120.0;
        }
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
