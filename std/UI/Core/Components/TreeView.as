// RFC 037 -- TreeView -- hierarchical tree control (ItemsSource + keyboard + M-VZ4).
//
// WPF mental model: root hosts TreeViewItem surface; selection by node identity.
// FlatIndex = index in the expanded-visible flat list (not full DFS of collapsed).
//
// ItemsSource virtualization mode (List<TreeNode>):
//   - Keep _rootNodes; build _visNodes/_visDepths by walking expanded nodes only.
//   - ItemViewport + _rowPool recycle flat TreeViewItem children under TreeView
//     (NOT nested). Extent = visibleCount * TreeRowHeight (arithmetic).
//   - Honest M-VZ4 boundary: FlatIndex-window virt only; NOT full hierarchical
//     path virt (no per-branch viewport / no nested container generator).
//
// ItemsSource unset: nested full-materialize path for manual ARML children
// (AssignAllFlatIndices / CollectVisibleFlatIndices / nested Measure-Arrange).
//
// Does NOT derive Primitives.Selector (tree select + expand != flat index template).
// Focusable/IsTabStop + TryHandleKey; PointerRouter HitItemIndex/HitExpand.

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Internal;
using Arc.UI.Layout;

/// <summary>Hierarchical tree: virtual flat rows (ItemsSource) or nested kids (ARML).</summary>
public class TreeView : Control
{
    /// <summary>SelectedIndex DP -- visible FlatIndex, default -1.</summary>
    public static DependencyProperty<int> SelectedIndexProperty =
        RegisterProperty<int>(nameof(SelectedIndex), typeof(TreeView), -1);

    /// <summary>SelectedItem DP -- current TreeViewItem, default null.</summary>
    public static DependencyProperty<object> SelectedItemProperty =
        RegisterProperty<object>(nameof(SelectedItem), typeof(TreeView), null);

    /// <summary>ItemsSource DP -- hierarchical data entry (List&lt;TreeNode&gt;).</summary>
    public static DependencyProperty<object> ItemsSourceProperty =
        RegisterProperty<object>(nameof(ItemsSource), typeof(TreeView), null);

    /// <summary>VerticalOffset DP -- scroll offset (px) driving the virt window.</summary>
    public static DependencyProperty<double> VerticalOffsetProperty =
        RegisterProperty<double>(nameof(VerticalOffset), typeof(TreeView), 0.0);

    long _mirrorHandle;
    bool _virtualizing;
    List<TreeNode> _rootNodes;
    List<TreeNode> _visNodes;
    List<int> _visDepths;
    List<TreeViewItem> _rowPool;
    ItemViewport _viewport;
    double _lastViewportHeight;
    TreeNode _selectedNode;

    /// <summary>Construct and bind TypeName.</summary>
    public TreeView()
    {
        this.Type = typeof(TreeView);
        this.TypeName = "TreeView";
        this.SelectionChanged = new Signal<string>("");
        this.Focusable = true;
        this.IsTabStop = true;
        _virtualizing = false;
        _rootNodes = null;
        _selectedNode = null;
        _visNodes = new List<TreeNode>();
        _visDepths = new List<int>();
        _rowPool = new List<TreeViewItem>();
        _viewport = new ItemViewport();
        _lastViewportHeight = ItemViewport.DefaultViewportHeight;
    }

    /// <summary>Selected visible FlatIndex (-1 = none).</summary>
    public int SelectedIndex
    {
        get
        {
            return this.GetValue<int>(SelectedIndexProperty);
        }
        set
        {
            this.SelectFlatIndex(value);
        }
    }

    /// <summary>Selected node (TreeViewItem; null if none).</summary>
    public object SelectedItem
    {
        get
        {
            return this.GetValue<object>(SelectedItemProperty);
        }
        set
        {
            if (value == null)
            {
                this.SelectFlatIndex(-1);
                return;
            }
            if (value is TreeViewItem)
            {
                TreeViewItem item = (TreeViewItem)value;
                this.SelectFlatIndex(item.FlatIndex);
            }
        }
    }

    /// <summary>SelectionChanged signal -- payload is Header text.</summary>
    public Signal<string> SelectionChanged;

    /// <summary>
    /// Hierarchical items source. List&lt;TreeNode&gt; enables FlatIndex-window virt;
    /// null clears. Unknown types clear. Manual Children remain when never set.
    /// </summary>
    public object ItemsSource
    {
        get
        {
            return this.GetValue<object>(ItemsSourceProperty);
        }
        set
        {
            this.SetValue<object>(ItemsSourceProperty, value);
            this.MaterializeFromItemsSource();
        }
    }

    /// <summary>Vertical scroll offset (px); drives virt window.</summary>
    public double VerticalOffset
    {
        get
        {
            return this.GetValue<double>(VerticalOffsetProperty);
        }
        set
        {
            double next = value;
            if (next < 0.0)
            {
                next = 0.0;
            }
            this.SetValue<double>(VerticalOffsetProperty, next);
            if (_virtualizing)
            {
                this.RefreshWindow();
            }
            FramePump.InvalidateLayout();
        }
    }

    /// <summary>Arithmetic content extent (visibleCount * TreeRowHeight).</summary>
    public double ContentExtentHeight
    {
        get
        {
            return _viewport.ExtentHeight;
        }
    }

    /// <summary>First materialized FlatIndex in the virt window.</summary>
    public int FirstMaterializedIndex
    {
        get
        {
            return _viewport.FirstIndex;
        }
    }

    /// <summary>Last materialized FlatIndex (empty window = -1).</summary>
    public int LastMaterializedIndex
    {
        get
        {
            return _viewport.LastIndex;
        }
    }

    /// <summary>Expanded-visible flat row count (virt list length).</summary>
    public int VisibleRowCount
    {
        get
        {
            return _visNodes.Count;
        }
    }

    /// <summary>Subscribe to selection changes.</summary>
    public void OnSelectionChanged(Action<string> handler)
    {
        if (SelectionChanged != null && handler != null)
        {
            SelectionChanged.Subscribe(handler);
        }
    }

    /// <summary>No Measure pass yet: materialize default virt window.</summary>
    public void EnsureViewportMaterialization()
    {
        if (!_virtualizing)
        {
            return;
        }
        this.RefreshWindow();
    }

    /// <summary>ItemsSource replace: virt flat rows or clear.</summary>
    void MaterializeFromItemsSource()
    {
        object src = this.GetValue<object>(ItemsSourceProperty);
        this.RecycleAllRows();
        this.ClearTreeItems();
        _visNodes.Clear();
        _visDepths.Clear();
        _rootNodes = null;
        _virtualizing = false;
        _selectedNode = null;
        if (src == null)
        {
            this.SelectFlatIndex(-1);
            FramePump.InvalidateLayout();
            return;
        }
        if (src is List<TreeNode>)
        {
            List<TreeNode> roots = (List<TreeNode>)src;
            _rootNodes = roots;
            _virtualizing = true;
            this.RebuildVisibleRows();
            this.SetValue<double>(VerticalOffsetProperty, 0.0);
            this.RefreshWindow();
            this.SelectFlatIndexVirtual(-1);
            FramePump.InvalidateLayout();
            return;
        }
        this.SelectFlatIndex(-1);
        FramePump.InvalidateLayout();
    }

    /// <summary>Remove all current children.</summary>
    void ClearTreeItems()
    {
        if (this.Children == null)
        {
            return;
        }
        this.Children.Clear();
    }

    /// <summary>Walk expanded TreeNodes into _visNodes/_visDepths.</summary>
    void RebuildVisibleRows()
    {
        _visNodes.Clear();
        _visDepths.Clear();
        if (_rootNodes == null)
        {
            return;
        }
        int i = 0;
        while (i < _rootNodes.Count)
        {
            TreeNode node = _rootNodes[i];
            if (node != null)
            {
                this.WalkVisibleNode(node, 0);
            }
            i++;
        }
    }

    void WalkVisibleNode(TreeNode node, int depth)
    {
        _visNodes.Add(node);
        _visDepths.Add(depth);
        if (!node.IsExpanded)
        {
            return;
        }
        if (node.Children == null)
        {
            return;
        }
        int i = 0;
        while (i < node.Children.Count)
        {
            TreeNode child = node.Children[i];
            if (child != null)
            {
                this.WalkVisibleNode(child, depth + 1);
            }
            i++;
        }
    }

    int IndexOfVisibleNode(TreeNode node)
    {
        if (node == null)
        {
            return -1;
        }
        int i = 0;
        while (i < _visNodes.Count)
        {
            if (_visNodes[i] == node)
            {
                return i;
            }
            i++;
        }
        return -1;
    }

    /// <summary>Expand/collapse from a pooled virt row: sync TreeNode, rebuild, reselect.</summary>
    internal void OnVirtualRowExpandChanged(TreeViewItem row)
    {
        if (!_virtualizing)
        {
            FramePump.InvalidateLayout();
            return;
        }
        if (row == null)
        {
            return;
        }
        object tag = row.Tag;
        if (tag is TreeNode)
        {
            TreeNode node = (TreeNode)tag;
            node.IsExpanded = row.IsExpanded;
        }
        this.RebuildVisibleRows();
        int keep = this.IndexOfVisibleNode(_selectedNode);
        this.ClampVerticalOffset();
        this.RefreshWindow();
        if (keep >= 0)
        {
            this.SelectFlatIndexVirtual(keep);
        }
        else if (_selectedNode != null)
        {
            this.SelectFlatIndexVirtual(-1);
        }
        FramePump.InvalidateLayout();
    }

    void ClampVerticalOffset()
    {
        double extent = (double)_visNodes.Count * ControlMetrics.TreeRowHeight;
        double maxOff = extent - _lastViewportHeight;
        if (maxOff < 0.0)
        {
            maxOff = 0.0;
        }
        double vo = this.GetValue<double>(VerticalOffsetProperty);
        if (vo > maxOff)
        {
            this.SetValue<double>(VerticalOffsetProperty, maxOff);
        }
        if (vo < 0.0)
        {
            this.SetValue<double>(VerticalOffsetProperty, 0.0);
        }
    }

    void RefreshWindow()
    {
        this.UpdateViewportWindow(_lastViewportHeight);
        this.MaterializeWindowRows();
        this.ApplySelectionToWindow();
        this.SyncMirrorAll();
    }

    void UpdateViewportWindow(double viewportHeight)
    {
        double stride = ControlMetrics.TreeRowHeight;
        _viewport.Update(this.VerticalOffset, viewportHeight, _visNodes.Count, stride, 0.0, 0.0);
    }

    /// <summary>Materialize [first,last]: recycle outside, TakePooledRow + BindVirtualRow.</summary>
    void MaterializeWindowRows()
    {
        int first = _viewport.FirstIndex;
        int last = _viewport.LastIndex;
        if (_visNodes.Count == 0 || last < first)
        {
            this.RecycleAllRows();
            return;
        }
        List<TreeViewItem> recycle = new List<TreeViewItem>();
        int count = this.Children.Count;
        int i = 0;
        while (i < count)
        {
            Element raw = this.Children[i];
            if (!(raw is TreeViewItem))
            {
                i++;
                continue;
            }
            TreeViewItem row = (TreeViewItem)raw;
            if (row.VisibleIndex < first || row.VisibleIndex > last)
            {
                recycle.Add(row);
            }
            i++;
        }
        int r = 0;
        int recycleCount = recycle.Count;
        while (r < recycleCount)
        {
            TreeViewItem row = recycle[r];
            row.PrepareForPool();
            this.Children.Remove(row);
            row.Parent = null;
            _rowPool.Add(row);
            r++;
        }
        int idx = first;
        while (idx <= last)
        {
            TreeViewItem row = this.FindRowByVisibleIndex(idx);
            if (row == null)
            {
                row = this.TakePooledRow();
                this.AddChild(row);
            }
            TreeNode node = _visNodes[idx];
            int depth = _visDepths[idx];
            row.BindVirtualRow(node, depth, idx);
            idx++;
        }
    }

    void RecycleAllRows()
    {
        if (this.Children == null || this.Children.Count == 0)
        {
            return;
        }
        List<TreeViewItem> recycle = new List<TreeViewItem>();
        int count = this.Children.Count;
        int i = 0;
        while (i < count)
        {
            Element raw = this.Children[i];
            if (raw is TreeViewItem)
            {
                recycle.Add((TreeViewItem)raw);
            }
            i++;
        }
        int r = 0;
        int recycleCount = recycle.Count;
        while (r < recycleCount)
        {
            TreeViewItem row = recycle[r];
            row.PrepareForPool();
            this.Children.Remove(row);
            row.Parent = null;
            _rowPool.Add(row);
            r++;
        }
    }

    TreeViewItem TakePooledRow()
    {
        if (_rowPool.Count > 0)
        {
            int tail = _rowPool.Count - 1;
            TreeViewItem row = _rowPool[tail];
            _rowPool.RemoveAt(tail);
            return row;
        }
        return new TreeViewItem();
    }

    TreeViewItem FindRowByVisibleIndex(int index)
    {
        if (this.Children == null)
        {
            return null;
        }
        int count = this.Children.Count;
        int i = 0;
        while (i < count)
        {
            Element raw = this.Children[i];
            if (raw is TreeViewItem)
            {
                TreeViewItem row = (TreeViewItem)raw;
                if (row.VisibleIndex == index)
                {
                    return row;
                }
            }
            i++;
        }
        return null;
    }

    void ApplySelectionToWindow()
    {
        if (this.Children == null)
        {
            return;
        }
        int count = this.Children.Count;
        int i = 0;
        while (i < count)
        {
            Element raw = this.Children[i];
            if (raw is TreeViewItem)
            {
                TreeViewItem row = (TreeViewItem)raw;
                bool selected = false;
                if (_selectedNode != null && row.Tag is TreeNode)
                {
                    selected = ((TreeNode)row.Tag) == _selectedNode;
                }
                if (row.IsSelected != selected)
                {
                    row.IsSelected = selected;
                }
            }
            i++;
        }
    }

    void EnsureFlatIndexVisible(int flatIndex)
    {
        if (flatIndex < 0)
        {
            return;
        }
        double stride = ControlMetrics.TreeRowHeight;
        double top = (double)flatIndex * stride;
        double bottom = top + stride;
        double vo = this.GetValue<double>(VerticalOffsetProperty);
        double vh = _lastViewportHeight;
        if (vh <= 0.0)
        {
            vh = ItemViewport.DefaultViewportHeight;
        }
        if (top < vo)
        {
            this.SetValue<double>(VerticalOffsetProperty, top);
        }
        else if (bottom > vo + vh)
        {
            double next = bottom - vh;
            if (next < 0.0)
            {
                next = 0.0;
            }
            this.SetValue<double>(VerticalOffsetProperty, next);
        }
    }

    /// <summary>Register platform mirror handle.</summary>
    internal void BindPlatformMirror(long handle)
    {
        _mirrorHandle = handle;
        if (_virtualizing)
        {
            this.RefreshWindow();
        }
        else
        {
            this.AssignAllFlatIndices();
            this.SyncMirrorSelection();
        }
    }

    void SyncMirrorAll()
    {
        if (_mirrorHandle == 0)
        {
            return;
        }
        WindowHost.ElementSetNumber(_mirrorHandle, "VerticalOffset", this.VerticalOffset);
        WindowHost.ElementSetNumber(_mirrorHandle, "ExtentHeight", this.ContentExtentHeight);
        WindowHost.ElementSetNumber(_mirrorHandle, "VisibleRowCount", (double)this.VisibleRowCount);
        WindowHost.ElementSetNumber(_mirrorHandle, "RowHeight", ControlMetrics.TreeRowHeight);
        WindowHost.ElementSetNumber(_mirrorHandle, "SelectedIndex",
            (double)this.GetValue<int>(SelectedIndexProperty));
    }

    void SyncMirrorSelection()
    {
        if (_mirrorHandle == 0)
        {
            return;
        }
        WindowHost.ElementSetNumber(_mirrorHandle, "SelectedIndex",
            (double)this.GetValue<int>(SelectedIndexProperty));
    }

    /// <summary>PointerRouter: HitItemIndex / HitExpand -> select or expand.</summary>
    internal void RouteHit()
    {
        if (_mirrorHandle == 0)
        {
            return;
        }
        int hit = (int)WindowHost.ElementGetNumber(_mirrorHandle, "HitItemIndex", -1.0);
        int expand = (int)WindowHost.ElementGetNumber(_mirrorHandle, "HitExpand", 0.0);
        if (hit < 0)
        {
            return;
        }
        TreeViewItem item = this.FindByFlatIndex(hit);
        if (item == null)
        {
            return;
        }
        if (expand != 0 && item.HasItems)
        {
            item.IsExpanded = !item.IsExpanded;
            return;
        }
        this.SelectFlatIndex(hit);
    }

    /// <summary>Keyboard nav entry: virt or nested visible-flat path.</summary>
    internal bool TryHandleKey(int virtualKey)
    {
        if (_virtualizing)
        {
            return this.TryHandleKeyVirtual(virtualKey);
        }
        this.AssignAllFlatIndices();
        List<int> visible = this.CollectVisibleFlatIndices();
        int visibleCount = visible.Count;
        if (visibleCount <= 0)
        {
            return false;
        }
        int curFlat = this.GetValue<int>(SelectedIndexProperty);
        int curVis = this.IndexOfFlat(visible, curFlat);

        if (virtualKey == FocusManager.VirtualKeyUp())
        {
            if (curVis < 0)
            {
                this.SelectFlatIndex(visible[visibleCount - 1]);
            }
            else if (curVis > 0)
            {
                this.SelectFlatIndex(visible[curVis - 1]);
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyDown())
        {
            if (curVis < 0)
            {
                this.SelectFlatIndex(visible[0]);
            }
            else if (curVis < visibleCount - 1)
            {
                this.SelectFlatIndex(visible[curVis + 1]);
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyHome())
        {
            this.SelectFlatIndex(visible[0]);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyEnd())
        {
            this.SelectFlatIndex(visible[visibleCount - 1]);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyLeft())
        {
            TreeViewItem item = this.FindByFlatIndex(curFlat);
            if (item != null && item.HasItems && item.IsExpanded)
            {
                item.IsExpanded = false;
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyRight())
        {
            TreeViewItem item = this.FindByFlatIndex(curFlat);
            if (item != null && item.HasItems && !item.IsExpanded)
            {
                item.IsExpanded = true;
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyReturn())
        {
            if (curVis < 0)
            {
                this.SelectFlatIndex(visible[0]);
            }
            else
            {
                this.SelectFlatIndex(curFlat);
            }
            return true;
        }
        return false;
    }

    bool TryHandleKeyVirtual(int virtualKey)
    {
        int visibleCount = _visNodes.Count;
        if (visibleCount <= 0)
        {
            return false;
        }
        int curFlat = this.GetValue<int>(SelectedIndexProperty);

        if (virtualKey == FocusManager.VirtualKeyUp())
        {
            if (curFlat < 0)
            {
                this.SelectFlatIndexVirtual(visibleCount - 1);
            }
            else if (curFlat > 0)
            {
                this.SelectFlatIndexVirtual(curFlat - 1);
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyDown())
        {
            if (curFlat < 0)
            {
                this.SelectFlatIndexVirtual(0);
            }
            else if (curFlat < visibleCount - 1)
            {
                this.SelectFlatIndexVirtual(curFlat + 1);
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyHome())
        {
            this.SelectFlatIndexVirtual(0);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyEnd())
        {
            this.SelectFlatIndexVirtual(visibleCount - 1);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyLeft())
        {
            if (curFlat >= 0 && curFlat < visibleCount)
            {
                TreeNode node = _visNodes[curFlat];
                if (node != null && node.Children != null && node.Children.Count > 0 && node.IsExpanded)
                {
                    node.IsExpanded = false;
                    this.RebuildVisibleRows();
                    this.ClampVerticalOffset();
                    int keep = this.IndexOfVisibleNode(_selectedNode);
                    this.RefreshWindow();
                    if (keep >= 0)
                    {
                        this.SelectFlatIndexVirtual(keep);
                    }
                }
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyRight())
        {
            if (curFlat >= 0 && curFlat < visibleCount)
            {
                TreeNode node = _visNodes[curFlat];
                if (node != null && node.Children != null && node.Children.Count > 0 && !node.IsExpanded)
                {
                    node.IsExpanded = true;
                    this.RebuildVisibleRows();
                    this.ClampVerticalOffset();
                    int keep = this.IndexOfVisibleNode(_selectedNode);
                    this.RefreshWindow();
                    if (keep >= 0)
                    {
                        this.SelectFlatIndexVirtual(keep);
                    }
                }
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyReturn())
        {
            if (curFlat < 0)
            {
                this.SelectFlatIndexVirtual(0);
            }
            else
            {
                this.SelectFlatIndexVirtual(curFlat);
            }
            return true;
        }
        return false;
    }

    /// <summary>Select flat index (-1 clears; out of range ignored).</summary>
    public void SelectFlatIndex(int index)
    {
        if (_virtualizing)
        {
            this.SelectFlatIndexVirtual(index);
            return;
        }
        if (index < -1)
        {
            return;
        }
        this.AssignAllFlatIndices();
        int count = this.CountAllItems();
        if (index >= count)
        {
            return;
        }
        this.ClearSelection();
        TreeViewItem selected = null;
        if (index >= 0)
        {
            selected = this.FindByFlatIndex(index);
            if (selected == null)
            {
                return;
            }
            selected.IsSelected = true;
        }
        this.SetValue<int>(SelectedIndexProperty, index);
        object boxed = null;
        if (selected != null)
        {
            boxed = selected;
        }
        this.SetValue<object>(SelectedItemProperty, boxed);
        this.SyncMirrorSelection();
        if (SelectionChanged != null)
        {
            string payload = "";
            if (selected != null && selected.Header != null)
            {
                payload = selected.Header;
            }
            SelectionChanged.Set(payload);
        }
        FramePump.Invalidate();
    }

    void SelectFlatIndexVirtual(int index)
    {
        if (index < -1)
        {
            return;
        }
        if (index >= _visNodes.Count)
        {
            return;
        }
        _selectedNode = null;
        if (index >= 0)
        {
            _selectedNode = _visNodes[index];
        }
        this.EnsureFlatIndexVisible(index);
        this.RefreshWindow();
        this.SetValue<int>(SelectedIndexProperty, index);
        TreeViewItem selected = null;
        if (index >= 0)
        {
            selected = this.FindByFlatIndex(index);
        }
        object boxed = null;
        if (selected != null)
        {
            boxed = selected;
        }
        this.SetValue<object>(SelectedItemProperty, boxed);
        this.SyncMirrorAll();
        if (SelectionChanged != null)
        {
            string payload = "";
            if (_selectedNode != null && _selectedNode.Header != null)
            {
                payload = _selectedNode.Header;
            }
            SelectionChanged.Set(payload);
        }
        FramePump.Invalidate();
    }

    void ClearSelection()
    {
        this.WalkClearSelected(this);
    }

    void WalkClearSelected(Element root)
    {
        if (root == null || root.Children == null)
        {
            return;
        }
        int i = 0;
        while (i < root.Children.Count)
        {
            Element raw = root.Children[i];
            if (raw is TreeViewItem)
            {
                TreeViewItem item = (TreeViewItem)raw;
                if (item.IsSelected)
                {
                    item.IsSelected = false;
                }
                this.WalkClearSelected(item);
            }
            i++;
        }
    }

    /// <summary>Visible-row FlatIndex list (DFS; only descend when IsExpanded).</summary>
    List<int> CollectVisibleFlatIndices()
    {
        List<int> result = new List<int>();
        if (this.Children == null)
        {
            return result;
        }
        int i = 0;
        while (i < this.Children.Count)
        {
            Element raw = this.Children[i];
            if (raw is TreeViewItem)
            {
                this.CollectVisibleInSubtree((TreeViewItem)raw, result);
            }
            i++;
        }
        return result;
    }

    void CollectVisibleInSubtree(TreeViewItem item, List<int> result)
    {
        result.Add(item.FlatIndex);
        if (!item.IsExpanded)
        {
            return;
        }
        int count = item.CountChildItems();
        int i = 0;
        while (i < count)
        {
            TreeViewItem child = item.ChildItemAt(i);
            if (child != null)
            {
                this.CollectVisibleInSubtree(child, result);
            }
            i++;
        }
    }

    int IndexOfFlat(List<int> visible, int flatIndex)
    {
        int i = 0;
        while (i < visible.Count)
        {
            if (visible[i] == flatIndex)
            {
                return i;
            }
            i++;
        }
        return -1;
    }

    /// <summary>Assign DFS FlatIndex for whole nested tree (incl. collapsed).</summary>
    internal void AssignAllFlatIndices()
    {
        int next = 0;
        if (this.Children == null)
        {
            return;
        }
        int i = 0;
        while (i < this.Children.Count)
        {
            Element raw = this.Children[i];
            if (raw is TreeViewItem)
            {
                next = this.AssignItemIndices((TreeViewItem)raw, next);
            }
            i++;
        }
    }

    int AssignItemIndices(TreeViewItem item, int next)
    {
        item.AssignFlatIndex(next);
        next++;
        int count = item.CountChildItems();
        int i = 0;
        while (i < count)
        {
            TreeViewItem child = item.ChildItemAt(i);
            if (child != null)
            {
                next = this.AssignItemIndices(child, next);
            }
            i++;
        }
        return next;
    }

    int CountAllItems()
    {
        int n = 0;
        if (this.Children == null)
        {
            return 0;
        }
        int i = 0;
        while (i < this.Children.Count)
        {
            Element raw = this.Children[i];
            if (raw is TreeViewItem)
            {
                n = n + this.CountSubtree((TreeViewItem)raw);
            }
            i++;
        }
        return n;
    }

    int CountSubtree(TreeViewItem item)
    {
        int n = 1;
        int count = item.CountChildItems();
        int i = 0;
        while (i < count)
        {
            TreeViewItem child = item.ChildItemAt(i);
            if (child != null)
            {
                n = n + this.CountSubtree(child);
            }
            i++;
        }
        return n;
    }

    /// <summary>Find node by FlatIndex (nested walk or virt window row).</summary>
    internal TreeViewItem FindByFlatIndex(int index)
    {
        if (index < 0 || this.Children == null)
        {
            return null;
        }
        if (_virtualizing)
        {
            return this.FindRowByVisibleIndex(index);
        }
        int i = 0;
        while (i < this.Children.Count)
        {
            Element raw = this.Children[i];
            if (raw is TreeViewItem)
            {
                TreeViewItem found = this.FindInSubtree((TreeViewItem)raw, index);
                if (found != null)
                {
                    return found;
                }
            }
            i++;
        }
        return null;
    }

    TreeViewItem FindInSubtree(TreeViewItem item, int index)
    {
        if (item.FlatIndex == index)
        {
            return item;
        }
        int count = item.CountChildItems();
        int i = 0;
        while (i < count)
        {
            TreeViewItem child = item.ChildItemAt(i);
            if (child != null)
            {
                TreeViewItem found = this.FindInSubtree(child, index);
                if (found != null)
                {
                    return found;
                }
            }
            i++;
        }
        return null;
    }

    int CountRootItems()
    {
        if (this.Children == null)
        {
            return 0;
        }
        int n = 0;
        int i = 0;
        while (i < this.Children.Count)
        {
            if (this.Children[i] is TreeViewItem)
            {
                n++;
            }
            i++;
        }
        return n;
    }

    TreeViewItem RootItemAt(int itemIndex)
    {
        if (this.Children == null || itemIndex < 0)
        {
            return null;
        }
        int seen = 0;
        int i = 0;
        while (i < this.Children.Count)
        {
            Element raw = this.Children[i];
            if (raw is TreeViewItem)
            {
                if (seen == itemIndex)
                {
                    return (TreeViewItem)raw;
                }
                seen++;
            }
            i++;
        }
        return null;
    }

    public override void OnLoaded()
    {
        if (_virtualizing)
        {
            this.RefreshWindow();
        }
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize)
    {
        if (_virtualizing)
        {
            return this.MeasureVirtual(availableSize);
        }
        double availW = availableSize.Width;
        double totalH = 0.0;
        double maxW = 0.0;
        int count = this.CountRootItems();
        int i = 0;
        while (i < count)
        {
            TreeViewItem item = this.RootItemAt(i);
            if (item != null)
            {
                LayoutHelper.MeasureChild(item, new LayoutSize(availW, LayoutHelper.Unbounded));
                totalH = totalH + item.DesiredSize.Height;
                if (item.DesiredSize.Width > maxW)
                {
                    maxW = item.DesiredSize.Width;
                }
            }
            i++;
        }
        if (availW > 0.0 && availW < LayoutHelper.Unbounded && maxW < availW)
        {
            maxW = availW;
        }
        if (this.Width > 0.0)
        {
            maxW = this.Width;
        }
        if (this.Height > 0.0)
        {
            totalH = this.Height;
        }
        return new LayoutSize(maxW, totalH);
    }

    LayoutSize MeasureVirtual(LayoutSize availableSize)
    {
        double availW = availableSize.Width;
        double availH = availableSize.Height;
        bool hBounded = availH > 0.0 && availH < LayoutHelper.Unbounded;
        double vpH = _lastViewportHeight;
        if (hBounded)
        {
            vpH = availH;
            _lastViewportHeight = vpH > 0.0 ? vpH : _lastViewportHeight;
        }
        this.UpdateViewportWindow(vpH);
        this.MaterializeWindowRows();
        this.ApplySelectionToWindow();

        double stride = ControlMetrics.TreeRowHeight;
        int count = this.Children.Count;
        int i = 0;
        while (i < count)
        {
            Element raw = this.Children[i];
            if (raw is FrameworkElement)
            {
                FrameworkElement child = (FrameworkElement)raw;
                LayoutHelper.MeasureChild(child, new LayoutSize(availW, stride));
            }
            i++;
        }

        double extentH = _viewport.ExtentHeight;
        double h = extentH;
        if (hBounded)
        {
            if (h > vpH)
            {
                h = vpH;
            }
        }
        else
        {
            if (h > _lastViewportHeight)
            {
                h = _lastViewportHeight;
            }
        }
        double w = availW;
        if (w <= 0.0 || w >= LayoutHelper.Unbounded)
        {
            w = 320.0;
        }
        if (this.Width > 0.0)
        {
            w = this.Width;
        }
        if (this.Height > 0.0)
        {
            h = this.Height;
        }
        return new LayoutSize(w, h);
    }

    protected override void ArrangeOverride(LayoutSize finalSize)
    {
        if (_virtualizing)
        {
            this.ArrangeVirtual(finalSize);
            return;
        }
        this.AssignAllFlatIndices();
        double y = 0.0;
        int count = this.CountRootItems();
        int i = 0;
        while (i < count)
        {
            TreeViewItem item = this.RootItemAt(i);
            if (item != null)
            {
                double h = item.DesiredSize.Height;
                LayoutHelper.ArrangeChild(this, item, 0.0, y, finalSize.Width, h);
                y = y + h;
            }
            i++;
        }
        this.SyncMirrorSelection();
    }

    void ArrangeVirtual(LayoutSize finalSize)
    {
        _lastViewportHeight = finalSize.Height;
        double stride = ControlMetrics.TreeRowHeight;
        double indent = ControlMetrics.TreeIndentPerLevel;
        double scrollY = this.VerticalOffset;
        if (scrollY < 0.0)
        {
            scrollY = 0.0;
        }
        int count = this.Children.Count;
        int i = 0;
        while (i < count)
        {
            Element raw = this.Children[i];
            if (!(raw is TreeViewItem))
            {
                i++;
                continue;
            }
            TreeViewItem row = (TreeViewItem)raw;
            int vis = row.VisibleIndex;
            if (vis < 0)
            {
                i++;
                continue;
            }
            double x = (double)row.Depth * indent;
            double y = (double)vis * stride - scrollY;
            double w = finalSize.Width - x;
            if (w < 0.0)
            {
                w = 0.0;
            }
            LayoutHelper.ArrangeChild(this, row, x, y, w, stride);
            i++;
        }
        this.SyncMirrorAll();
    }
}
