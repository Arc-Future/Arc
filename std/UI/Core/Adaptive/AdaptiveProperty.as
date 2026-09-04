// RFC 037 §5.2/§11.5 统一刷新终点（M-U2）：AdaptiveProperty —— 信号后端 DP 槽。
//
// 求值器重算 → SetValue<T>(prop, newValue) → Signal<T>.Set 触发通知 →
// 渲染层 Observe<T> 收到 → 局部重绘（§5.2）。本类以 `Signal<double>` 为后端
// （§5.1「内部通道」✅ 已实现），承载求值器产出的数值属性；元素 `Element`
// 的 `GetValue/SetValue` DP 槽应用待语言核心 ARC/泛型缺陷修复后就绪
// （§14 差距矩阵如实标注），M-U2 的闭环运行在本通道上。

namespace Arc.UI.Adaptive;

/// <summary>
/// 信号后端 DP 槽（§5.2 统一刷新终点：SetValue → Observe → 局部重绘）。
/// </summary>
public class AdaptiveProperty {
    private Signal<double> _signal;

    /// <summary>构造属性槽。</summary>
    /// <param name="initial">初始值。</param>
    public AdaptiveProperty(double initial) {
        _signal = new Signal<double>(initial);
    }

    /// <summary>读取当前值。</summary>
    public double Get() {
        return _signal.Value;
    }

    /// <summary>设值——触发 `Signal&lt;double&gt;.Set` 通知（局部重绘触发点）。</summary>
    public void Set(double v) {
        _signal.Set(v);
    }

    /// <summary>订阅通道（渲染层 Observe 入口）。</summary>
    public Signal<double> Observe() {
        return _signal;
    }
}
