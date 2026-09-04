// RFC 037 D4.1: Arc.UI — DataContext 数据上下文。
//
// DataContext 是数据绑定的源，沿可视树继承。
// 子元素若未显式 SetValue DataContextProperty，则经 Element.DataContext getter
// 沿 Parent 链回退至最近祖先的有效值（WPF inherit DP 语义）；GetValue<object>
// (DataContextProperty) 为本地槽语义（未 SetValue 返 null，不沿 Parent 继承）。
//
// **命名空间归属**：本文件位于 std/UI/Data/ 子目录，但归属到 `Arc.UI`
// 命名空间（按 RFC 020 §3.2「子命名空间与目录解耦」+ RFC 037 D9.2
// Data 扁平化原则）。

namespace Arc.UI;

/// <summary>数据上下文，数据绑定的源。</summary>
public class DataContext {
    /// <summary>数据对象。</summary>
    public object Value;

    /// <summary>父级 DataContext 包装（可选；树继承以 Element.Parent 为准）。</summary>
    public DataContext Parent;

    public DataContext() { }

    public DataContext(object value) {
        this.Value = value;
    }

    /// <summary>
    /// 从元素解析有效 DataContext（含 Parent 链继承）。
    /// 等价于 <see cref="Element.DataContext"/> getter。
    /// </summary>
    /// <param name="element">起始元素；null 时返回 null。</param>
    /// <returns>继承解析后的数据对象；无上下文时为 null。</returns>
    public static object ResolveFrom(Element element) {
        if ((long)element == (long)0) {
            return null;
        }
        return element.DataContext;
    }
}
