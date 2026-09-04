// RFC 037 §10 AI 原生：DefaultElementFactory — 默认元素工厂。
//
// 注册 Arc.UI 全部已知元素类型的无参构造函数。
// 类型名 → 构造委托映射，供 ArmlParser 运行时实例化。
//
// 扩展纪律：新增元素类型只需在 RegisterAll 中添加一行，
// 零其他修改点——单一事实来源。

namespace Arc.UI.Markup;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Components.Layout;

/// <summary>
/// 默认元素工厂——按类型名创建 Arc.UI 元素实例。
/// </summary>
public class DefaultElementFactory : IElementFactory {
    /// <summary>类型名 → 构造器（引用类包装，规避 Arc 泛型容器存储委托缺陷）。</summary>
    private Dictionary<string, ElementCreator> _creators;

    /// <summary>构造并注册全部已知元素类型。</summary>
    public DefaultElementFactory() {
        _creators = new Dictionary<string, ElementCreator>();
        this.RegisterAll();
    }

    public Element Create(string typeName) {
        if (typeName == null || typeName.Length == 0) {
            return null;
        }
        ElementCreator creator = null;
        if (_creators.TryGetValue(typeName, out creator)) {
            return creator.Fn();
        }
        return null;
    }

    /// <summary>注册单个类型构造器——委托经引用类包装后存入字典，避免直接泛型容器存储。</summary>
    private void Add(string typeName, Func<Element> fn) {
        ElementCreator c = new ElementCreator();
        c.Fn = fn;
        _creators[typeName] = c;
    }

    /// <summary>注册全部已知元素类型的无参构造函数。</summary>
    private void RegisterAll() {
        // 根元素
        this.Add("Window", () => new Window());
        this.Add("Page", () => new Page());
        this.Add("UserControl", () => new UserControl());
        this.Add("Application", () => new Application());

        // 布局容器
        this.Add("StackPanel", () => new StackPanel());
        this.Add("Grid", () => new Grid());
        this.Add("Canvas", () => new Canvas());
        this.Add("DockPanel", () => new DockPanel());
        this.Add("WrapPanel", () => new WrapPanel());
        this.Add("ScrollView", () => new ScrollView());
        this.Add("VirtualizingStackPanel", () => new VirtualizingStackPanel());

        // 基础控件
        this.Add("TextBlock", () => new TextBlock());
        this.Add("Button", () => new Button());
        this.Add("ToggleButton", () => new ToggleButton());
        this.Add("CheckBox", () => new CheckBox());
        this.Add("TextBox", () => new TextBox());
        this.Add("Slider", () => new Slider());
        this.Add("Image", () => new Image());
        this.Add("Rectangle", () => new Rectangle());
        // ComboBox<T> 泛型→用非泛型基座 ComboBoxBase（TypeName="ComboBox"）
        this.Add("ComboBox", () => new ComboBoxBase());

        // 内容控件
        this.Add("ContentPresenter", () => new ContentPresenter());
        this.Add("ContentControl", () => new ContentControl());
        this.Add("ItemsControl", () => new ItemsControl());
        this.Add("ListView", () => new ListView());

        // 数据网格
        this.Add("DataGrid", () => new DataGrid());

        // 特殊
        this.Add("VisualHost", () => new VisualHost());
        this.Add("VideoSurface", () => new VideoSurface());
        this.Add("Element", () => new Element());
    }
}

/// <summary>
/// 元素构造器——引用类承载构造委托。
/// 仅作为 DefaultElementFactory 内部注册载体，规避泛型容器存储委托后调用崩溃的编译/运行时缺陷。
/// </summary>
internal class ElementCreator {
    public Func<Element> Fn;
}
