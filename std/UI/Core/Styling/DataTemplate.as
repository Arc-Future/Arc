// RFC 037 D3.7 修订：Arc.UI.Styling — DataTemplate 数据模板（工厂化）。
//
// 对齐 WPF DataTemplate 语义：模板 = 数据项的视觉工厂。WPF 经 XAML 声明 +
// DataContext 反射绑定；Arc 无反射，模板以**显式委托对**表达同一语义——
// Instantiate 新建容器视觉（≈ WPF 加载模板 XAML），Recycle 回收重绑
//（≈ WPF 容器复用时刷新 DataContext）。两委托即「容器创建 / 内容刷新」
// 分离的单一惯用法（一语义一写法），供 ItemContainerGenerator 虚拟化
// 回收池复用；开发者也可在 code-behind 构造任意自定义项视觉。

namespace Arc.UI.Styling;

using Arc.UI;

/// <summary>
/// 数据模板：定义数据项的视觉呈现（ItemsControl.ItemTemplate 消费）。
/// </summary>
/// <remarks>
/// 契约：Instantiate 与 Recycle 成对提供——Instantiate 新建项容器视觉并完成
/// 首次数据填充；Recycle 在容器从回收池复用时刷新内容（虚拟化必需）。
/// 仅提供 Instantiate 的只建不绑用法不受支持（ApplyUpdate 将无法刷新）。
/// </remarks>
public class DataTemplate {
    /// <summary>目标数据类型名（文档性标注；运行期不校验，编译期类型由委托签名承载）。</summary>
    public string DataType;

    /// <summary>项容器视觉工厂：为数据项新建容器元素（含首次数据填充）。</summary>
    public Func<object, Element> Instantiate;

    /// <summary>容器复用重绑：把回收池容器的内容刷新为新数据项。</summary>
    public Action<Element, object> Recycle;

    public DataTemplate() {
        this.DataType = "";
        this.Instantiate = null;
        this.Recycle = null;
    }
}
