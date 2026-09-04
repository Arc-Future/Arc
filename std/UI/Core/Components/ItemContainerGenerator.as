// RFC 037 · RFC 037 M-VZ1 / RFC 037 M6 · RFC 037 D3.7 修订: ItemContainerGenerator
// — 视口物化 + 回收池 + 集合级增量复用 + ItemTemplate 模板物化。
//
// 在 ItemsHost 上为项序列物化容器 Visual；**只** materialize 可见 ± cache 窗口。
// **不引用 ItemsControl.ItemsSource（object）**——数据面统一经 ItemSourceView
// （object 本体 + string 显示投影，RFC 037 数据面目标态）：默认路径消费
// DisplayAt 投影，模板路径消费 ItemAt 本体（WPF DataContext 同构）。
// M6：接收视图变更表面（CollectionChangedEventArgs&lt;object&gt;，ObservableCollection
// 订阅经视图转发），按 kind/index 增量驱动容器复用（Add/Insert/Remove/Update/
// Move/Clear）——复用回收池容器、仅移动受影响项、不做全窗口重建。
// 增量复用证据计数器：TotalCreated（新建容器总数）与 TotalRebinds（内容重绑次数）。
//
// D3.7 修订（WPF ItemTemplate 对齐）：容器物化双路径——
//   - 模板路径：ItemsControl.ItemTemplate 提供 DataTemplate（Instantiate 新建 /
//     Recycle 重绑委托对）时，容器视觉完全由模板工厂产出（任意 Element 子树）；
//   - 默认路径：无模板时回退显示投影 → TextBlock（零回归）。
// 回收池仅收纳可重绑容器（模板容器须提供 Recycle 委托方可入池复用）。

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components.Layout;
using Arc.UI.Styling;

/// <summary>项 Visual 物化器——回收池 + 视口窗口 + ItemTemplate 模板路径（RFC 037 §6）。</summary>
public class ItemContainerGenerator {
    private Panel _itemsHost;
    private int _itemCount;
    private List<Element> _recyclePool;
    private int _windowFirst;
    private int _windowLast;
    private ItemSourceView _view;
    private DataTemplate _template;
    private int _totalCreated;
    private int _totalRebinds;

    public ItemContainerGenerator(Panel itemsHost) {
        _itemsHost = itemsHost;
        _itemCount = 0;
        _recyclePool = new List<Element>();
        _windowFirst = 0;
        _windowLast = -1;
        _view = null;
        _template = null;
        _totalCreated = 0;
        _totalRebinds = 0;
    }

    /// <summary>逻辑项总数（ItemsSource 计数，非 Visual 子节点数）。</summary>
    public int ItemCount {
        get { return _itemCount; }
    }

    public Panel ItemsHost {
        get { return _itemsHost; }
    }

    /// <summary>回收池内待用容器数（M-VZ2 smoke）。</summary>
    public int RecyclePoolCount {
        get { return _recyclePool.Count; }
    }

    /// <summary>全程新建容器总数（M6 增量复用证据：增删改/滚动均不再新建）。</summary>
    public int TotalCreated {
        get { return _totalCreated; }
    }

    /// <summary>全程容器内容重绑次数（M6 证据：增量操作只重绑受影响项，非全窗口）。</summary>
    public int TotalRebinds {
        get { return _totalRebinds; }
    }

    /// <summary>
    /// 设置项模板（ItemsControl.ItemTemplate → DataTemplate）。null 清除模板回退
    /// 默认 TextBlock 路径。模板切换时全量回收：树上与池内容器均属旧模板视觉，
    /// 不可跨模板复用（池一并清空，WPF 模板切换同构语义）。
    /// </summary>
    public void SetTemplate(DataTemplate template) {
        if (_template == template) {
            return;
        }
        this.RecycleAll();
        _recyclePool.Clear();
        _template = template;
    }

    /// <summary>绑定数据源视图（载荷读取 + 增量通道唯一来源）。</summary>
    public void SetView(ItemSourceView view) {
        _view = view;
        _windowFirst = 0;
        _windowLast = -1;
    }

    /// <summary>同步当前视口窗口（面板重算视口后调用，供增量操作判定窗口边界）。</summary>
    public void SyncWindow(int firstIndex, int lastIndex) {
        _windowFirst = firstIndex;
        _windowLast = lastIndex;
    }

    /// <summary>
    /// 确保 [firstIndex..lastIndex] 项已物化；区间外容器回收到池。
    /// 载荷经数据源视图读取（DisplayAt 投影 / ItemAt 本体）。
    /// </summary>
    public void EnsureRange(int firstIndex, int lastIndex, TextBlock itemDefaults) {
        if (_view == null) {
            this.RecycleAll();
            _windowFirst = 0;
            _windowLast = -1;
            return;
        }
        _itemCount = _view.Count;
        if (_itemCount == 0) {
            this.RecycleAll();
            _windowFirst = 0;
            _windowLast = -1;
            return;
        }
        if (lastIndex >= _itemCount) {
            lastIndex = _itemCount - 1;
        }
        if (lastIndex < firstIndex) {
            this.RecycleAll();
            _windowFirst = firstIndex;
            _windowLast = lastIndex;
            return;
        }
        _windowFirst = firstIndex;
        _windowLast = lastIndex;
        this.RecycleOutOfRange(firstIndex, lastIndex);
        int i = firstIndex;
        while (i <= lastIndex) {
            this.GetOrCreate(i, itemDefaults);
            i++;
        }
    }

    /// <summary>移除全部活跃容器并回收到池（ItemsSource 清空）。</summary>
    public void RecycleAll() {
        while (_itemsHost.Children.Count > 0) {
            int tail = _itemsHost.Children.Count - 1;
            Element container = _itemsHost.Children[tail];
            this.RecycleContainer(container, tail);
        }
        _itemCount = 0;
    }

    // ── M6 增量复用（视图变更表面 → kind/index 驱动；载荷经视图直读）──

    /// <summary>Add（尾部追加）：仅当新项落在窗口内才物化，既有容器零移动。</summary>
    public void ApplyAdd(int index, TextBlock itemDefaults) {
        if (_view == null) {
            return;
        }
        _itemCount = _view.Count;
        if (_windowLast < _windowFirst) {
            return;
        }
        if (index > _windowLast) {
            return; // 新项在窗口下方：零视觉变更、零重绑
        }
        this.FillWindow(itemDefaults);
    }

    /// <summary>Insert：窗口内下标 index 起容器全部 +1（内容随容器走），窗口底池复用补位。</summary>
    public void ApplyInsert(int index, TextBlock itemDefaults) {
        if (_view == null) {
            return;
        }
        _itemCount = _view.Count;
        if (_windowLast < _windowFirst) {
            return;
        }
        if (index > _windowLast) {
            return; // 插入点在窗口下方：仅计数变化
        }
        this.ShiftTags(index, 1);
        this.FillWindow(itemDefaults);
    }

    /// <summary>Remove：回收该下标容器，其右容器 -1（内容随容器走），窗口底复用池补位。</summary>
    public void ApplyRemove(int index, TextBlock itemDefaults) {
        if (_view == null) {
            return;
        }
        _itemCount = _view.Count;
        if (_windowLast < _windowFirst) {
            return;
        }
        if (index > _windowLast) {
            return; // 移除点在窗口下方：仅计数变化
        }
        int pos = this.FindContainerPos(index);
        if (pos >= 0) {
            Element container = _itemsHost.Children[pos];
            this.RecycleContainer(container, pos);
        }
        this.ShiftTags(index + 1, -1);
        this.FillWindow(itemDefaults);
    }

    /// <summary>Update（索引器 set）：仅重绑该下标容器内容（新值经视图直读）。</summary>
    public void ApplyUpdate(int index, TextBlock itemDefaults) {
        if (_view == null) {
            return;
        }
        if (_windowLast < _windowFirst) {
            return;
        }
        int pos = this.FindContainerPos(index);
        if (pos >= 0) {
            Element container = _itemsHost.Children[pos];
            this.BindItemElement(container, index, itemDefaults);
        }
    }

    /// <summary>Move：重定位容器到新下标并平移中间段（纯 re-tag），边界缺口复用池补位。</summary>
    public void ApplyMove(int oldIndex, int newIndex, TextBlock itemDefaults) {
        if (_view == null) {
            return;
        }
        _itemCount = _view.Count;
        if (_windowLast < _windowFirst || oldIndex == newIndex) {
            return;
        }
        if (oldIndex > _windowLast && newIndex > _windowLast) {
            return; // 两端都在窗口下方
        }
        if (oldIndex < _windowFirst && newIndex < _windowFirst) {
            return; // 两端都在窗口上方
        }
        int movedPos = this.FindContainerPos(oldIndex);
        if (oldIndex < newIndex) {
            this.ShiftSpan(oldIndex + 1, newIndex, -1);
        } else {
            this.ShiftSpan(newIndex, oldIndex - 1, 1);
        }
        if (movedPos >= 0) {
            Element moved = _itemsHost.Children[movedPos];
            moved.SetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, (double)newIndex);
        }
        this.FillWindow(itemDefaults);
    }

    /// <summary>Clear：全部回收到池，计数归零。</summary>
    public void ApplyClear() {
        this.RecycleAll();
        _windowFirst = 0;
        _windowLast = -1;
    }

    // ── 窗口增量辅助 ──

    /// <summary>窗口内缺口补齐 + 区间外回收（仅 materialize 缺失下标，不复用/重绑既有容器）。</summary>
    private void FillWindow(TextBlock itemDefaults) {
        if (_windowLast < _windowFirst) {
            this.RecycleAll();
            _itemCount = 0;
            return;
        }
        this.RecycleOutOfRange(_windowFirst, _windowLast);
        int i = _windowFirst;
        while (i <= _windowLast) {
            if (i < _itemCount && this.FindContainerPos(i) < 0) {
                this.GetOrCreate(i, itemDefaults);
            }
            i++;
        }
    }

    /// <summary>把下标 &gt;= fromIndex 的容器 ItemIndex 平移 delta（内容随容器走，不重绑）。</summary>
    private void ShiftTags(int fromIndex, int delta) {
        int c = _itemsHost.Children.Count;
        int i = 0;
        while (i < c) {
            Element container = _itemsHost.Children[i];
            int idx = (int)container.GetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, -1.0);
            if (idx >= fromIndex) {
                container.SetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, (double)(idx + delta));
            }
            i++;
        }
    }

    /// <summary>把 [fromIndex..toIndex] 内容器 ItemIndex 平移 delta（Move 中间段）。</summary>
    private void ShiftSpan(int fromIndex, int toIndex, int delta) {
        if (toIndex < fromIndex) {
            return;
        }
        int c = _itemsHost.Children.Count;
        int i = 0;
        while (i < c) {
            Element container = _itemsHost.Children[i];
            int idx = (int)container.GetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, -1.0);
            if (idx >= fromIndex && idx <= toIndex) {
                container.SetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, (double)(idx + delta));
            }
            i++;
        }
    }

    /// <summary>返回持有指定 ItemIndex 的子节点位置；未物化返回 -1。</summary>
    private int FindContainerPos(int index) {
        int c = _itemsHost.Children.Count;
        int i = 0;
        while (i < c) {
            Element container = _itemsHost.Children[i];
            int idx = (int)container.GetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, -1.0);
            if (idx == index) {
                return i;
            }
            i++;
        }
        return -1;
    }

    private void RecycleOutOfRange(int firstIndex, int lastIndex) {
        int tail = _itemsHost.Children.Count - 1;
        while (tail >= 0) {
            Element container = _itemsHost.Children[tail];
            int idx = (int)container.GetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, -1.0);
            if (idx < firstIndex || idx > lastIndex) {
                this.RecycleContainer(container, tail);
            }
            tail--;
        }
    }

    private void GetOrCreate(int index, TextBlock itemDefaults) {
        int c = _itemsHost.Children.Count;
        int i = 0;
        while (i < c) {
            Element existing = _itemsHost.Children[i];
            int idx = (int)existing.GetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, -1.0);
            if (idx == index) {
                this.BindItemElement(existing, index, itemDefaults);
                return;
            }
            i++;
        }

        if (_template != null && _template.Instantiate != null) {
            Element created = this.InstantiateTemplated(_view.ItemAt(index));
            created.SetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, (double)index);
            _itemsHost.AddChild(created);
            return;
        }

        Element container = null;
        if (_recyclePool.Count > 0) {
            int poolTail = _recyclePool.Count - 1;
            container = _recyclePool[poolTail];
            _recyclePool.RemoveAt(poolTail);
        } else {
            container = this.CreateItemTextBlock(itemDefaults);
        }
        container.SetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, (double)index);
        this.BindItemElement(container, index, itemDefaults);
        _itemsHost.AddChild(container);
    }

    private void RecycleContainer(Element container, int childIndex) {
        container.SetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, -1.0);
        _itemsHost.Children.RemoveAt(childIndex);
        container.Parent = null;
        // 模板容器无 Recycle 委托时不可重绑（无刷新通道），弃用不入池；
        // 默认 TextBlock 容器与提供 Recycle 的模板容器入池复用。
        if (_template != null && _template.Recycle == null) {
            return;
        }
        _recyclePool.Add(container);
    }

    /// <summary>容器内容重绑分派：模板路径经 DataTemplate.Recycle（数据项本体直付）；
    /// 默认路径写 TextBlock 三属性（显示投影经视图直读）。</summary>
    private void BindItemElement(Element container, int index, TextBlock itemDefaults) {
        if (_template != null) {
            Action<Element, object> recycle = _template.Recycle;
            if (recycle != null) {
                _totalRebinds++;
                recycle(container, _view.ItemAt(index));
            }
            return;
        }
        this.BindItemTextBlock((TextBlock)container, _view.DisplayAt(index), itemDefaults);
    }

    private void BindItemTextBlock(TextBlock text, string display, TextBlock itemDefaults) {
        _totalRebinds++;
        text.Text = display;
        text.FontSize = itemDefaults.FontSize;
        text.Foreground = itemDefaults.Foreground;
    }

    private TextBlock CreateItemTextBlock(TextBlock itemDefaults) {
        _totalCreated++;
        TextBlock text = new TextBlock();
        text.TypeName = "TextBlock";
        text.FontSize = itemDefaults.FontSize;
        text.Foreground = itemDefaults.Foreground;
        return text;
    }

    private Element InstantiateTemplated(object item) {
        _totalCreated++;
        Func<object, Element> instantiate = _template.Instantiate;
        return instantiate(item);
    }
}
