// RFC 037 D2.1 + RFC 037 D1: Arc.UI.Components — Page 元素。
//
// Page 是可导航的页面容器（D2.1），常用于单窗口多视图切换。
//
// WPF 同构层级对照：
//   WPF: ContentControl → Page
//   Arc:  ContentControl → Page
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Content 已由 ContentControl 声明——Page 不重复声明，使用继承版本
//   - Page 保留特有 DP：Title
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。
//
// 实现状态：
//   - M1: 骨架声明 + RFC 037 DP 化
//   - M3+: 导航栈集成

namespace Arc.UI.Components;

/// <summary>可导航的页面容器。Content 由 ContentControl 继承；本类仅声明 Title DP。</summary>
public class Page : ContentControl {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public Page() {
        this.Type = typeof(Page);
        // 容器型非 Tab 停靠（InputElement 默认的显式豁免；内容参与 Tab 循环）。
        this.IsTabStop = false;
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Title 属性元数据——页面标题，默认空串。</summary>
    public static DependencyProperty<string> TitleProperty =
        RegisterProperty<string>(nameof(Title), typeof(Page), "");

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>页面标题（用于导航 UI）。</summary>
    public string Title {
        get { return this.GetValue<string>(TitleProperty); }
        set { this.SetValue<string>(TitleProperty, value); }
    }
}
