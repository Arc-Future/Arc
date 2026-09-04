// RFC 037 D2.1 / RFC 037 D1: Arc.UI.Components — ListView 控件。
//
// ListView 是 Primitives.Selector 的派生，提供可滚动的项列表展示。
//
// WPF 同构层级对照：
//   WPF: Control → ItemsControl → Primitives.Selector → ListBox → ListView
//   Arc:  Control → ItemsControl → Primitives.Selector → ListView（Arc 简化：合并 ListBox 角色）
//
// 选择语义面（SelectedIndex/SelectedItem/SelectedValue/SelectedValuePath 四 DP +
// SelectIndex 入口 + 平台镜像高亮同步 + SelectionChanged Signal 通道 +
// SelectionChangedHandler 占位）统一由 Primitives.Selector 承载（RFC 037 §5.3）；
// 本类仅保留类型身份与差异钩子：
//   - OnSelectionApplied override：选中后按文本装箱 SelectedItem。

namespace Arc.UI.Components;

using Arc.UI.Components.Layout;
using Arc.UI.Components.Primitives;

/// <summary>可滚动的项列表控件，承载选择语义。</summary>
public class ListView : Selector {
    public ListView() {
        this.Type = typeof(ListView);
        this.TypeName = "ListView";
    }

    /// <summary>选中后附加同步：按选中项文本装箱 SelectedItem（基类默认空）。</summary>
    protected override void OnSelectionApplied() {
        this.SyncSelectedItem();
    }
}
