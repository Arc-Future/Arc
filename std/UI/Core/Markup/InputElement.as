// RFC 037 · Arc.UI — InputElement 输入型控件基类（输入共性单一归宿）。
//
// WPF 同构对照：WPF 把输入面（焦点/键盘/指针路由）收敛在 UIElement /
// IInputElement 层，InputManager 面向 IInputElement 分发；具体输入控件
// （TextBox/Button/CheckBox/Slider...）在控件层重写 Focusable 默认。Arc 单
// 继承下等价切法：在 Control 之上插 InputElement 层，承载全部输入共性——
//
//   - 焦点默认：ctor 统一 Focusable=true + IsTabStop=true（WPF 各控件层
//     重写默认的 Arc 单一惯用法等价物；容器型派生 Window/UserControl/Page
//     显式 IsTabStop=false 声明非停靠）；
//   - 焦点状态单一写点：SetFocused → OnFocusedChanged（基类镜像 IsFocused
//     + 标脏；TextBox 扩展 caret 闪烁/文本镜像）——消除 FocusManager 里
//     TypeName 字符串分派与各控件自建 _isFocused 的漂移；
//   - IME 接管钩子：OnGotFocus（默认 no-op；TextBox 接管 ImeBridge）；
//   - 默认激活：Activate = Enter/Space 键盘激活（Button→Click、
//     ToggleButton→Toggle；WPF ButtonBase 同构）；
//   - 键盘消费入口：OnKeyDown 虚方法（焦点元素优先消费，未消费才走 Tab/
//     方向焦点导航——根治方向键「TextBox 光标 + 焦点循环」双路由）。
//
// **命名空间归属**：与 Control 同——基类放 `Arc.UI` 根命名空间原则。

namespace Arc.UI;

using Arc.UI.Components;
using Arc.UI.Internal;

/// <summary>
/// 输入型控件基类——焦点状态、键盘路由、默认激活的输入共性单一归宿。
/// </summary>
public class InputElement : Control {
    /// <summary>平台镜像句柄；BindPlatformMirror 统一写入（PlatformTreeSync）。</summary>
    protected long _mirrorHandle;

    /// <summary>焦点状态（单一写点 SetFocused；渲染经平台镜像 IsFocused）。</summary>
    protected bool _isFocused;

    /// <summary>构造元素：绑定类型身份并开启输入默认（可聚焦 + Tab 停靠）。</summary>
    public InputElement() {
        this.Type = typeof(InputElement);
        this.Focusable = true;
        this.IsTabStop = true;
    }

    /// <summary>是否持有焦点（只读；SetFocused 单一写点）。</summary>
    public bool IsFocused {
        get { return _isFocused; }
    }

    /// <summary>平台镜像登记（PlatformTreeSync 统一调用；幂等）。派生扩展路由注册。</summary>
    public virtual void BindPlatformMirror(long handle) {
        _mirrorHandle = handle;
    }

    /// <summary>
    /// 焦点状态单一写点（FocusManager 专通道；幂等——同值不触发镜像/重绘）。
    /// </summary>
    internal void SetFocused(bool focused) {
        if (_isFocused == focused) {
            return;
        }
        _isFocused = focused;
        this.OnFocusedChanged(focused);
    }

    /// <summary>焦点视觉同步（基类：镜像 IsFocused + 标脏；派生扩展）。</summary>
    protected virtual void OnFocusedChanged(bool focused) {
        if (_mirrorHandle != 0) {
            WindowHost.ElementSetBool(_mirrorHandle, "IsFocused", focused ? 1 : 0);
        }
        FramePump.Invalidate();
    }

    /// <summary>获得焦点钩子（默认 no-op；TextBox 接管 ImeBridge 焦点）。</summary>
    internal virtual void OnGotFocus() {
    }

    /// <summary>
    /// 键盘消费入口（FocusManager 分发）：焦点元素优先消费，返回 true 则
    /// 不进入 Tab/方向焦点导航。默认不消费。
    /// </summary>
    internal virtual bool OnKeyDown(int virtualKey, int shiftDown) {
        return false;
    }

    /// <summary>Enter/Space 默认激活（Button→Click、ToggleButton→Toggle；默认 no-op）。</summary>
    internal virtual void Activate() {
    }
}
