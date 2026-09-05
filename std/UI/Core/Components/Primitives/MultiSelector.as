// RFC 037 D2.1: Arc.UI.Components.Primitives — MultiSelector 多选语义层。
//
// WPF 同构层级对照（WPF MultiSelector 对标，Controls.Primitives 同构落位）：
//   WPF: Primitives.Selector → Primitives.MultiSelector → DataGrid
//   Arc:  Primitives.Selector → Primitives.MultiSelector → DataGrid
//
// 职责：多选语义通用封装——SelectionMode DP（Single/Multiple/Extended）+ SelectedItems
// 选中集合 + 增量选中 API（SelectItem/SelectAll/ClearSelection）。单选语义（四 DP +
// SelectIndex 模板方法 + 五钩子 + 镜像/Signal）由基类 Selector 承载；多选态下
// SelectedIndex/SelectedItem 反映主选中（最后选中项），平台镜像高亮跟随主选中。
//
// 诚实边界：SelectedItems 集合不参与镜像同步（PlatformTreeSync 契约为单值
// SelectedIndex number，多选镜像语义属平台渲染端后续面）；SelectedItems 条目为
// object 项数据本体（数据面 object 管道，WPF SelectedItems object 集合同构）。

namespace Arc.UI.Components.Primitives;

using Arc.Collections;
using Arc.UI.Components;

/// <summary>承载多选语义的 Selector 派生层（WPF MultiSelector 对标）。</summary>
public class MultiSelector : Selector {
    public MultiSelector() {
        this.SetupMulti();
    }

    /// <summary>自管视口派生（DataGrid）入口：ownsItemsHost=false 跳过基类项宿主装配。</summary>
    protected MultiSelector(bool ownsItemsHost) : base(ownsItemsHost) {
        this.SetupMulti();
    }

    private void SetupMulti() {
        this.Type = typeof(MultiSelector);
        this.TypeName = "MultiSelector";
        _selectedItems = new List<object>();
    }

    private List<object> _selectedItems;

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>SelectionMode 属性元数据——选择模式，默认 "Single"（从单选层上移：
    /// 模式是多选语义载体，单选层 Selector 不感知）。</summary>
    /// <value>"Single" / "Multiple" / "Extended"</value>
    public static DependencyProperty<string> SelectionModeProperty =
        RegisterProperty<string>(nameof(SelectionMode), typeof(MultiSelector), "Single");

    /// <summary>选择模式："Single"（默认）/"Multiple"/"Extended"。</summary>
    public string SelectionMode {
        get { return this.GetValue<string>(SelectionModeProperty); }
        set { this.SetValue<string>(SelectionModeProperty, value); }
    }

    /// <summary>是否允许多选（WPF CanSelectMultiple 对标，以 protected virtual 方法
    /// 承载——与选择钩子同惯用法）。Multiple/Extended 返回 true，派生控件可覆写扩展判定。</summary>
    protected virtual bool CanSelectMultiple() {
        return this.SelectionMode != "Single";
    }

    /// <summary>当前选中项集合（只读面：条目单一来源归一，不暴露可变写面——写入选
    /// 经 SelectItem/SelectAll/ClearSelection）。条目为数据项本体（WPF object 集合同构）。</summary>
    public List<object> SelectedItems {
        get { return _selectedItems; }
    }

    /// <summary>按索引定位项数据本体（默认数据源视图 ItemAt，不受虚拟化物化窗口
    /// 限制；DataGrid 覆写为行首列单元格）。无数据/越界返回 null。</summary>
    protected virtual object ItemDataAt(int index) {
        ItemSourceView view = this.View;
        if (view == null || index < 0 || index >= view.Count) {
            return null;
        }
        return view.ItemAt(index);
    }

    /// <summary>增量选中指定项：单选模式回落 SelectIndex（单选链完整流程）；多选模式
    /// 累加 SelectedItems 并将主选中（SelectedIndex/SelectedItem，镜像高亮跟随）指向
    /// 该项，SelectionChanged 触发。越界忽略。</summary>
    public void SelectItem(int index) {
        int count = this.SelectionItemCount();
        if (index < 0 || index >= count) {
            return;
        }
        if (!this.CanSelectMultiple()) {
            this.SelectIndex(index);
            return;
        }
        object item = this.ItemDataAt(index);
        if (item == null) {
            return;
        }
        if (!this.SelectedItems.Contains(item)) {
            this.SelectedItems.Add(item);
        }
        this.ApplySelectedIndexCore(index);
        this.SyncMirrorSelection();
        this.OnSelectionApplied();
        this.RaiseSelectionChanged();
    }

    /// <summary>全选（仅多选模式，单选模式忽略）：SelectedItems 清空后全量采集，
    /// 主选中指向最后一项，SelectionChanged 单次触发。</summary>
    public void SelectAll() {
        if (!this.CanSelectMultiple()) {
            return;
        }
        int count = this.SelectionItemCount();
        this.SelectedItems.Clear();
        int i = 0;
        while (i < count) {
            object item = this.ItemDataAt(i);
            if (item != null) {
                this.SelectedItems.Add(item);
            }
            i++;
        }
        if (count > 0) {
            this.ApplySelectedIndexCore(count - 1);
        }
        this.SyncMirrorSelection();
        this.OnSelectionApplied();
        this.RaiseSelectionChanged();
    }

    /// <summary>清空选中：SelectedItems 清空 + 主选中回 -1（复用单选链完整流程：
    /// 镜像高亮复位 + 附加同步 + SelectionChanged）。</summary>
    public void ClearSelection() {
        this.SelectedItems.Clear();
        this.SelectIndex(-1);
    }
}
