// RFC 037 · TabControl 页签项（std 最小切片）。
//
// TabItem 是 TabControl 的逻辑子页：Header 供页签栏展示，内容为 Children
//（ARML 嵌套子树）。选中/隐藏由父 TabControl 布局权威决定——非选中页在
// Arrange 时移出视口（对齐 Popup 关闭语义），不依赖尚未落地的 Visibility DP。
//
// **关闭**：<see cref="Close"/> → 父 <see cref="TabControl.CloseTab"/>（选中常显
// 「x」、未选中悬停显）。关闭后 <see cref="IsClosed"/>，页签栏撤出、客户区
// 不再展示。仍留 Children（无 `rt_ui_element_remove_child` ABI）。

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Layout;

/// <summary>TabControl 的一页：Header 标题 + Children 内容树。</summary>
public class TabItem : Panel {
    /// <summary>Header 属性元数据——页签栏显示文本。</summary>
    public static DependencyProperty<string> HeaderProperty =
        RegisterProperty<string>(nameof(Header), typeof(TabItem), "");

    bool _closed;

    /// <summary>构造并绑定 TypeName（手写/ARML 均须显式名，供平台镜像分派）。</summary>
    public TabItem() {
        this.Type = typeof(TabItem);
        this.TypeName = "TabItem";
    }

    /// <summary>页签标题（页签栏按钮 Content）。</summary>
    public string Header {
        get { return this.GetValue<string>(HeaderProperty); }
        set { this.SetValue<string>(HeaderProperty, value); }
    }

    /// <summary>已关闭：页签栏与客户区撤出，仍留父 <see cref="TabControl.Children"/>。</summary>
    public bool IsClosed
    {
        get { return _closed; }
    }

    /// <summary>关闭本页（经父 TabControl；无父则仅标关闭）。</summary>
    public void Close()
    {
        if (_closed)
        {
            return;
        }
        Element parent = this.Parent;
        if (parent is TabControl)
        {
            ((TabControl)parent).CloseItem(this);
            return;
        }
        _closed = true;
    }

    /// <summary>父容器关闭写点（与 <see cref="Close"/> 同源）。</summary>
    internal void MarkClosed()
    {
        _closed = true;
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        double maxW = 0.0;
        double maxH = 0.0;
        if (this.Children != null) {
            int count = this.Children.Count;
            int i = 0;
            while (i < count) {
                FrameworkElement child = (FrameworkElement)this.Children[i];
                LayoutHelper.MeasureChild(child, availableSize);
                if (child.DesiredSize.Width > maxW) {
                    maxW = child.DesiredSize.Width;
                }
                if (child.DesiredSize.Height > maxH) {
                    maxH = child.DesiredSize.Height;
                }
                i++;
            }
        }
        double availW = availableSize.Width;
        double availH = availableSize.Height;
        if (availW > 0.0 && availW < LayoutHelper.Unbounded && maxW < availW) {
            maxW = availW;
        }
        if (availH > 0.0 && availH < LayoutHelper.Unbounded && maxH < availH) {
            maxH = availH;
        }
        if (this.Width > 0.0) {
            maxW = this.Width;
        }
        if (this.Height > 0.0) {
            maxH = this.Height;
        }
        return new LayoutSize(maxW, maxH);
    }

    /// <summary>
    /// 内容宿主：槽位为整页 <paramref name="finalSize"/>，由
    /// <see cref="LayoutHelper.ArrangeChild"/> 按子元素对齐展开 Stretch——
    /// 禁止用 DesiredSize 作槽（否则 ScrollView/StackPanel 无法拉满 TabControl）。
    /// </summary>
    protected override void ArrangeOverride(LayoutSize finalSize) {
        if (this.Children == null) {
            return;
        }
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            FrameworkElement child = (FrameworkElement)this.Children[i];
            LayoutHelper.ArrangeChild(this, child, 0.0, 0.0, finalSize.Width, finalSize.Height);
            i++;
        }
    }
}
