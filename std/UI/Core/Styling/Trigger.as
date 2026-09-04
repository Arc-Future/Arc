// RFC 037 WPF 对齐 / RFC 037 D3: Arc.UI.Styling — Trigger 属性触发器。
//
// 对标 WPF Style.Triggers 的属性触发器（Trigger）：目标元素某属性当前值
// 等于条件值时，应用触发器的 Setters（如 Background 命中色值时换 Foreground）；
// 条件失效时由 StyleManager 回退应用前快照（WPF Trigger 进入/退出核心语义）。
//
// 条件载荷按元素 DP 运行时类型分派：string（直接相等比较）/ bool（"True"/
// "False" 解析）/ int（十进制整数字面量解析）；解析失败不命中（防御边界，
// 条件值文本由 arml/用户代码自由传入，不得以异常外溢）。
//
// 与 VisualStateManager 的分工边界（RFC 037 §1 双轨禁令）：
// 交互态视觉（hover/pressed/focus/disabled/checked/selected）的唯一正道是
// VisualStateManager 的 internal 强类型配方（ControlState → ControlVisual，
// 态反馈唯一来源）；Style.Triggers 仅表达数据驱动的通用属性条件样式（如
// Content=="特定值" 时换色）。禁止用 Trigger 表达交互态——两条样式通道并存
// 会破坏态反馈唯一来源，形成双轨。

namespace Arc.UI.Styling;

using Arc.Collections;

/// <summary>
/// 属性触发器——条件命中（属性当前值 == Value）时应用 Setters，条件失效时
/// 由 StyleManager 恢复应用前快照（进入/退出语义）。仅用于通用属性条件样式；
/// 交互态视觉（hover/pressed/focus/…）归 VisualStateManager（RFC 037 §1，
/// 双轨禁令）。
/// </summary>
public class Trigger {
    /// <summary>条件属性名（如 "Background"，经元素 DP 注册表动态解析）。</summary>
    public string Property;

    /// <summary>
    /// 条件值文本：string 载荷作直接相等比较；bool 载荷按 "True"/"False"
    /// 解析；int 载荷按十进制整数字面量解析（见 TryParseBool/TryParseInt）。
    /// </summary>
    public string Value;

    /// <summary>条件命中时应用的属性设置器集合（覆盖基础 Setters 同名属性）。</summary>
    public List<Setter> Setters;

    public Trigger() {
        this.Property = "";
        this.Value = "";
        this.Setters = new List<Setter>();
    }

    /// <summary>
    /// 条件值文本 → bool：仅接受 "True"/"False" 精确匹配（C# bool.Parse 白名单
    /// 收敛；手写规避运行时 bool.Parse 可用性不确定）；其余文本不命中。
    /// </summary>
    public static bool TryParseBool(string raw, out bool value) {
        if (raw == "True") {
            value = true;
            return true;
        }
        if (raw == "False") {
            value = false;
            return true;
        }
        value = false;
        return false;
    }

    /// <summary>条件值文本 → int：可选负号 + 纯十进制数字；其余文本不命中。</summary>
    public static bool TryParseInt(string raw, out int value) {
        value = 0;
        if (raw == null || raw == "") {
            return false;
        }
        int start = 0;
        bool negative = false;
        if (raw.Substring(0, 1) == "-") {
            negative = true;
            start = 1;
            if (raw.Length == 1) {
                return false;
            }
        }
        int r = 0;
        int i = start;
        while (i < raw.Length) {
            int d = Trigger.DigitValue(raw.Substring(i, 1));
            if (d < 0) {
                return false;
            }
            r = r * 10 + d;
            i = i + 1;
        }
        if (negative) {
            r = -r;
        }
        value = r;
        return true;
    }

    /// <summary>单字符 → 数字（'0'-'9' → 0-9）；非数字返回 -1。</summary>
    private static int DigitValue(string ch) {
        if (ch == "0") { return 0; }
        if (ch == "1") { return 1; }
        if (ch == "2") { return 2; }
        if (ch == "3") { return 3; }
        if (ch == "4") { return 4; }
        if (ch == "5") { return 5; }
        if (ch == "6") { return 6; }
        if (ch == "7") { return 7; }
        if (ch == "8") { return 8; }
        if (ch == "9") { return 9; }
        return -1;
    }
}
