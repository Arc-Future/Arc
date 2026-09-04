// RFC 037 D3.4: Arc.UI.Styling — Style 样式定义。
//
// Style 把一批属性值打包，可应用到指定类型的所有实例。
// Setter 类见 Setter.as（单文件单类型原则）。
//
// 样式命中模型（两轴正交，按优先级递增应用，后应用者覆盖同名属性）：
// 隐式样式按 TargetType 匹配元素类型名（同 TargetType 多样式按声明顺序
// 后加者胜，见 StyleManager.ApplyStylesToElement）；显式样式经
// ResourceDictionary 键引用（Style={StaticResource K1, K2}，多资源绑定
// 依次应用；窗口内键 codegen 对象定型、App 域键持请求键字符串应用期
// 解析）。x:Key 统一资源键——样式无第二套类名标识。BasedOn 继承链
// 先父后子（跨 MergedDictionaries 查找，编译期 verify 检环 + 运行时
// visited 兜底）。CSS 式选择器（Kind/Value 魔法对 + 级联特异性）已撤除
// ——WPF 无此概念，态条件由 Trigger 承载（进入/退出语义见 StyleManager），
// 字符串选择键对 AI 驱动开发不友好（类型系统无法推断合法值）。

namespace Arc.UI.Styling;

using Arc.Collections;
using Arc.UI;

/// <summary>样式定义，把一批属性值打包应用到指定类型实例。</summary>
public class Style {
    /// <summary>目标类型名（如 "Button"）。</summary>
    public string TargetType;

    /// <summary>键（x:Key）；未设置则作为隐式默认样式。</summary>
    public string Key;

    /// <summary>基于样式键（继承自其他 Style）。</summary>
    public string BasedOn;

    /// <summary>属性设置器集合。</summary>
    public List<Setter> Setters;

    /// <summary>属性触发器集合（WPF Style.Triggers 对齐）：条件命中时其
    /// Setters 在基础 Setters 之后应用，覆盖同名属性。</summary>
    public List<Trigger> Triggers;

    public Style() {
        this.Setters = new List<Setter>();
        this.Triggers = new List<Trigger>();
    }

    /// <summary>样式是否命中元素（按 TargetType 类型名匹配）。</summary>
    public bool Matches(Element element) {
        if (element == null) {
            return false;
        }
        if (this.TargetType != null && this.TargetType != "") {
            return element.TypeName == this.TargetType;
        }
        return false;
    }
}
