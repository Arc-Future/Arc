// RFC 037 D2.1: Arc.UI.Components.Primitives — MultiSelector 多选语义层。
//
// WPF 同构层级对照（WPF MultiSelector 对标，Controls.Primitives 同构落位）：
//   WPF: Primitives.Selector → Primitives.MultiSelector → DataGrid
//   Arc:  Primitives.Selector → Primitives.MultiSelector → DataGrid
//
// 职责：多选语义通用封装——SelectionMode DP（Single/Multiple/Extended）+ SelectedItems
// 选中集合 + 增量选中 API（SelectItem/SelectAll/ClearSelection）+ 指针修饰键手势
// （SelectIndexWithMods）。单选语义（四 DP + SelectIndex 模板方法 + 五钩子 + 镜像/
// Signal）由基类 Selector 承载；多选态下 SelectedIndex/SelectedItem 反映主选中
// （最后选中项），平台镜像高亮跟随主选中。
//
// 诚实边界：
//   - SelectedItems 集合不参与镜像同步（PlatformTreeSync 契约为单值 SelectedIndex
//     number，多选镜像语义属平台渲染端后续面）
//   - SelectedItems 条目为 object 项数据本体（数据面 object 管道，WPF object 集合同构）
//   - 成员判定以逻辑下标表为准（避开 List<object> 对 DataGrid 单元格串 Contains 不可靠）
//   - 程序化多选 ✅；Ctrl/Shift 修饰键手势最小面 ✅（SelectionMode=Multiple/Extended；
//     mods bit0=Shift bit1=Ctrl，同 RFC 037 §8；PointerRouter DataGrid 槽读 HitMods）
//   - SelectionChanged 仍走基类 Signal<string> 直挂 Subscribe（禁包装 lambda 逃逸）

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
        _selectedIndices = new List<int>();
        _selectionAnchor = -1;
    }

    private List<object> _selectedItems;

    /// <summary>与 SelectedItems 平行的逻辑下标表——成员判定唯一权威。</summary>
    private List<int> _selectedIndices;

    /// <summary>Shift 范围选锚点（WPF 心智：普通/Ctrl 点击更新；Shift 点击不改锚）。</summary>
    private int _selectionAnchor;

    public static DependencyProperty<string> SelectionModeProperty =
        RegisterProperty<string>(nameof(SelectionMode), typeof(MultiSelector), "Single");

    /// <summary>选择模式："Single"（默认）/"Multiple"/"Extended"。</summary>
    public string SelectionMode {
        get { return this.GetValue<string>(SelectionModeProperty); }
        set { this.SetValue<string>(SelectionModeProperty, value); }
    }

    /// <summary>是否允许多选（Multiple/Extended）。</summary>
    protected virtual bool CanSelectMultiple() {
        return this.SelectionMode != "Single";
    }

    /// <summary>当前选中项集合（只读面；写入经 SelectItem/SelectAll/ClearSelection/SelectIndexWithMods）。</summary>
    public List<object> SelectedItems {
        get { return _selectedItems; }
    }

    /// <summary>按索引定位项数据本体；DataGrid 覆写为行首列单元格。</summary>
    protected virtual object ItemDataAt(int index) {
        ItemSourceView view = this.View;
        if (view == null || index < 0 || index >= view.Count) {
            return null;
        }
        return view.ItemAt(index);
    }

    /// <summary>SelectIndex 主选中路径：替换 SelectedItems；-1 清空。</summary>
    protected override void SyncSelectionCollection(int index) {
        this.ClearSelectionSets();
        _selectionAnchor = index;
        if (index < 0) {
            return;
        }
        this.AddSelectedIndex(index);
    }

    /// <summary>指针点击入口：mods bit0=Shift bit1=Ctrl（RFC 037 §8）。
    /// Multiple：Shift 范围 / Ctrl 切换 / 无修饰替换。SelectionChanged 直挂。</summary>
    public void SelectIndexWithMods(int index, int mods) {
        int count = this.SelectionItemCount();
        if (index < -1 || index >= count) {
            return;
        }
        if (!this.CanSelectMultiple() || index < 0) {
            this.SelectIndex(index);
            return;
        }
        bool shift = (mods & 1) != 0;
        bool ctrl = (mods & 2) != 0;
        if (shift) {
            this.SelectRangeFromAnchor(index);
            return;
        }
        if (ctrl) {
            this.ToggleItemSelection(index);
            return;
        }
        this.SelectIndex(index);
    }

    /// <summary>增量选中；单选回落 SelectIndex；多选累加并触发 SelectionChanged。</summary>
    public void SelectItem(int index) {
        int count = this.SelectionItemCount();
        if (index < 0 || index >= count) {
            return;
        }
        if (!this.CanSelectMultiple()) {
            this.SelectIndex(index);
            return;
        }
        this.AddSelectedIndex(index);
        _selectionAnchor = index;
        this.ApplySelectedIndexCore(index);
        this.SyncMirrorSelection();
        this.OnSelectionApplied();
        this.RaiseSelectionChanged();
    }

    /// <summary>全选（仅多选）；SelectionChanged 单次触发。</summary>
    public void SelectAll() {
        if (!this.CanSelectMultiple()) {
            return;
        }
        int count = this.SelectionItemCount();
        this.ClearSelectionSets();
        int i = 0;
        while (i < count) {
            this.AddSelectedIndex(i);
            i++;
        }
        if (count > 0) {
            _selectionAnchor = count - 1;
            this.ApplySelectedIndexCore(count - 1);
        }
        this.SyncMirrorSelection();
        this.OnSelectionApplied();
        this.RaiseSelectionChanged();
    }

    /// <summary>清空选中。</summary>
    public void ClearSelection() {
        this.ClearSelectionSets();
        this.SelectIndex(-1);
    }

    void ToggleItemSelection(int index) {
        int slot = this.IndexOfSelectedIndex(index);
        if (slot >= 0) {
            this.RemoveSelectedAt(slot);
            if (_selectedIndices.Count == 0) {
                this.ApplySelectedIndexCore(-1);
            } else if (this.SelectedIndex == index) {
                this.ApplySelectedIndexCore(_selectedIndices[0]);
            }
        } else {
            this.AddSelectedIndex(index);
            this.ApplySelectedIndexCore(index);
        }
        _selectionAnchor = index;
        this.SyncMirrorSelection();
        this.OnSelectionApplied();
        this.RaiseSelectionChanged();
    }

    void SelectRangeFromAnchor(int index) {
        int anchor = _selectionAnchor;
        if (anchor < 0 || anchor >= this.SelectionItemCount()) {
            anchor = index;
        }
        int lo = anchor;
        int hi = index;
        if (lo > hi) {
            lo = index;
            hi = anchor;
        }
        this.ClearSelectionSets();
        int i = lo;
        while (i <= hi) {
            this.AddSelectedIndex(i);
            i++;
        }
        this.ApplySelectedIndexCore(index);
        this.SyncMirrorSelection();
        this.OnSelectionApplied();
        this.RaiseSelectionChanged();
    }

    void ClearSelectionSets() {
        this.SelectedItems.Clear();
        _selectedIndices.Clear();
    }

    void AddSelectedIndex(int index) {
        if (this.IndexOfSelectedIndex(index) >= 0) {
            return;
        }
        object item = this.ItemDataAt(index);
        if (item == null) {
            return;
        }
        this.SelectedItems.Add(item);
        _selectedIndices.Add(index);
    }

    void RemoveSelectedAt(int slot) {
        this.SelectedItems.RemoveAt(slot);
        _selectedIndices.RemoveAt(slot);
    }

    int IndexOfSelectedIndex(int index) {
        int i = 0;
        while (i < _selectedIndices.Count) {
            if (_selectedIndices[i] == index) {
                return i;
            }
            i++;
        }
        return -1;
    }
}
