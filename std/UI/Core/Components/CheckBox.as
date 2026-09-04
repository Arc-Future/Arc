// RFC 037 D2.1 / RFC 037 D1: Arc.UI.Components — CheckBox 复选框控件。
// CheckBox 是 ToggleButton 的语义占位派生类——仅类型区分，无额外成员。

namespace Arc.UI.Components;

/// <summary>
/// 复选框控件——ToggleButton 的语义占位派生类。
/// 仅类型区分（用于样式/模板选择器），无额外成员。
/// </summary>
public class CheckBox : ToggleButton {
    // 语义占位类——所有 DP 与事件均从 ToggleButton 继承。
}
