// RFC 037 -- TreeViewItem -- tree node (nested ARML + virt flat row).
//
// WPF mental model: Header + IsExpanded + IsSelected + nested TreeViewItem kids
// for manual ARML; ItemsSource virt mode binds flat rows via BindVirtualRow
// (no nested children; Depth/VisibleIndex drive Arrange indent).
//
// Honest M-VZ4 boundary (owned by TreeView): FlatIndex-window virt only.

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Internal;
using Arc.UI.Layout;

/// <summary>One TreeView node: Header + expand + select (+ nested kids or virt row).</summary>
public class TreeViewItem : Panel
{
    /// <summary>Header DP -- node title.</summary>
    public static DependencyProperty<string> HeaderProperty =
        RegisterProperty<string>(nameof(Header), typeof(TreeViewItem), "");

    /// <summary>IsExpanded DP -- whether children are shown.</summary>
    public static DependencyProperty<bool> IsExpandedProperty =
        RegisterProperty<bool>(nameof(IsExpanded), typeof(TreeViewItem), false);

    /// <summary>IsSelected DP -- whether this node is selected.</summary>
    public static DependencyProperty<bool> IsSelectedProperty =
        RegisterProperty<bool>(nameof(IsSelected), typeof(TreeViewItem), false);

    /// <summary>DFS / visible FlatIndex (assigned by TreeView).</summary>
    int _flatIndex = -1;

    /// <summary>Platform mirror handle.</summary>
    long _mirrorHandle;

    /// <summary>Indent depth for virt flat rows.</summary>
    int _depth;

    /// <summary>Visible FlatIndex for virt flat rows (-1 when pooled).</summary>
    int _visibleIndex = -1;

    /// <summary>True when bound as a recycled virt row (no nested kids).</summary>
    bool _virtualRow;

    /// <summary>Construct and bind TypeName.</summary>
    public TreeViewItem()
    {
        this.Type = typeof(TreeViewItem);
        this.TypeName = "TreeViewItem";
        _depth = 0;
        _visibleIndex = -1;
        _virtualRow = false;
    }

    /// <summary>Node title.</summary>
    public string Header
    {
        get
        {
            return this.GetValue<string>(HeaderProperty);
        }
        set
        {
            this.SetValue<string>(HeaderProperty, value);
        }
    }

    /// <summary>Whether children are expanded.</summary>
    public bool IsExpanded
    {
        get
        {
            return this.GetValue<bool>(IsExpandedProperty);
        }
        set
        {
            this.SetValue<bool>(IsExpandedProperty, value);
            this.SyncMirrorExpanded();
            if (this.Parent is TreeView)
            {
                ((TreeView)this.Parent).OnVirtualRowExpandChanged(this);
            }
            else
            {
                FramePump.InvalidateLayout();
            }
        }
    }

    /// <summary>Whether selected.</summary>
    public bool IsSelected
    {
        get
        {
            return this.GetValue<bool>(IsSelectedProperty);
        }
        set
        {
            this.SetValue<bool>(IsSelectedProperty, value);
            this.SyncMirrorSelected();
            FramePump.Invalidate();
        }
    }

    /// <summary>FlatIndex (read-only; TreeView assigns).</summary>
    public int FlatIndex
    {
        get
        {
            return _flatIndex;
        }
    }

    /// <summary>Whether this node has child items (virt: TreeNode.Children).</summary>
    public bool HasItems
    {
        get
        {
            if (_virtualRow)
            {
                object tag = this.Tag;
                if (tag is TreeNode)
                {
                    TreeNode node = (TreeNode)tag;
                    if (node.Children != null)
                    {
                        return node.Children.Count > 0;
                    }
                }
                return false;
            }
            return this.CountChildItems() > 0;
        }
    }

    /// <summary>Indent depth for virt Arrange (0 = root).</summary>
    public int Depth
    {
        get
        {
            return _depth;
        }
        set
        {
            _depth = value;
        }
    }

    /// <summary>Visible FlatIndex for virt window (-1 when pooled).</summary>
    public int VisibleIndex
    {
        get
        {
            return _visibleIndex;
        }
        set
        {
            _visibleIndex = value;
        }
    }

    /// <summary>Bind a recycled row to a TreeNode (virt path; no nested kids).</summary>
    internal void BindVirtualRow(TreeNode node, int depth, int visibleIndex)
    {
        _virtualRow = true;
        this.Tag = node;
        _depth = depth;
        _visibleIndex = visibleIndex;
        string header = "";
        if (node != null && node.Header != null)
        {
            header = node.Header;
        }
        this.Header = header;
        bool expanded = false;
        if (node != null)
        {
            expanded = node.IsExpanded;
        }
        // SetValue only -- property setter would notify TreeView and rebuild mid-bind.
        this.SetValue<bool>(IsExpandedProperty, expanded);
        this.AssignFlatIndex(visibleIndex);
        if (this.Children != null && this.Children.Count > 0)
        {
            this.Children.Clear();
        }
        this.SyncMirrorAll();
    }

    /// <summary>Reset row for pool reuse.</summary>
    internal void PrepareForPool()
    {
        _visibleIndex = -1;
        _depth = 0;
        _virtualRow = false;
        this.Tag = null;
        this.SetValue<bool>(IsSelectedProperty, false);
        this.SetValue<bool>(IsExpandedProperty, false);
        this.AssignFlatIndex(-1);
        if (this.Children != null && this.Children.Count > 0)
        {
            this.Children.Clear();
        }
    }

    /// <summary>PlatformTreeSync: bind mirror and push node surface.</summary>
    internal void BindPlatformMirror(long handle)
    {
        _mirrorHandle = handle;
        this.SyncMirrorAll();
    }

    /// <summary>TreeView assigns flat index.</summary>
    internal void AssignFlatIndex(int index)
    {
        _flatIndex = index;
        this.SyncMirrorFlatIndex();
    }

    void SyncMirrorAll()
    {
        if (_mirrorHandle == 0)
        {
            return;
        }
        string header = this.Header;
        if (header == null)
        {
            header = "";
        }
        WindowHost.ElementSetString(_mirrorHandle, "Header", header);
        WindowHost.ElementSetNumber(_mirrorHandle, "HeaderHeight", ControlMetrics.TreeRowHeight);
        WindowHost.ElementSetNumber(_mirrorHandle, "FlatIndex", (double)_flatIndex);
        WindowHost.ElementSetNumber(_mirrorHandle, "Depth", (double)_depth);
        WindowHost.ElementSetBool(_mirrorHandle, "IsExpanded", this.IsExpanded ? 1 : 0);
        WindowHost.ElementSetBool(_mirrorHandle, "IsSelected", this.IsSelected ? 1 : 0);
        WindowHost.ElementSetBool(_mirrorHandle, "HasItems", this.HasItems ? 1 : 0);
        WindowHost.ElementSetNumber(_mirrorHandle, "ExpanderWidth", ControlMetrics.TreeExpanderWidth);
    }

    void SyncMirrorExpanded()
    {
        if (_mirrorHandle == 0)
        {
            return;
        }
        WindowHost.ElementSetBool(_mirrorHandle, "IsExpanded", this.IsExpanded ? 1 : 0);
    }

    void SyncMirrorSelected()
    {
        if (_mirrorHandle == 0)
        {
            return;
        }
        WindowHost.ElementSetBool(_mirrorHandle, "IsSelected", this.IsSelected ? 1 : 0);
    }

    void SyncMirrorFlatIndex()
    {
        if (_mirrorHandle == 0)
        {
            return;
        }
        WindowHost.ElementSetNumber(_mirrorHandle, "FlatIndex", (double)_flatIndex);
    }

    /// <summary>Count TreeViewItem children.</summary>
    internal int CountChildItems()
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

    /// <summary>Nth TreeViewItem child.</summary>
    internal TreeViewItem ChildItemAt(int itemIndex)
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

    protected override LayoutSize MeasureOverride(LayoutSize availableSize)
    {
        if (_virtualRow)
        {
            return this.MeasureVirtualRow(availableSize);
        }
        double rowH = ControlMetrics.TreeRowHeight;
        double indent = ControlMetrics.TreeIndentPerLevel;
        double availW = availableSize.Width;
        double childAvailW = availW;
        if (availW > 0.0 && availW < LayoutHelper.Unbounded)
        {
            childAvailW = availW - indent;
            if (childAvailW < 0.0)
            {
                childAvailW = 0.0;
            }
        }
        double contentH = rowH;
        double contentW = 0.0;
        if (this.Header != null && this.Header.Length > 0)
        {
            LayoutSize textSize = LayoutHelper.EstimateTextSize(
                this.Header, ControlMetrics.FontBodySize, 0.0, 0.0, "", "Normal");
            contentW = ControlMetrics.TreeExpanderWidth + ControlMetrics.SpacingSM + textSize.Width;
        }
        int count = this.CountChildItems();
        int i = 0;
        while (i < count)
        {
            TreeViewItem child = this.ChildItemAt(i);
            if (child != null)
            {
                LayoutHelper.MeasureChild(child, new LayoutSize(childAvailW, LayoutHelper.Unbounded));
                if (this.IsExpanded)
                {
                    contentH = contentH + child.DesiredSize.Height;
                    double childW = child.DesiredSize.Width + indent;
                    if (childW > contentW)
                    {
                        contentW = childW;
                    }
                }
            }
            i++;
        }
        if (availW > 0.0 && availW < LayoutHelper.Unbounded && contentW < availW)
        {
            contentW = availW;
        }
        if (this.Width > 0.0)
        {
            contentW = this.Width;
        }
        if (this.Height > 0.0)
        {
            contentH = this.Height;
        }
        return new LayoutSize(contentW, contentH);
    }

    LayoutSize MeasureVirtualRow(LayoutSize availableSize)
    {
        double rowH = ControlMetrics.TreeRowHeight;
        double contentW = 0.0;
        if (this.Header != null && this.Header.Length > 0)
        {
            LayoutSize textSize = LayoutHelper.EstimateTextSize(
                this.Header, ControlMetrics.FontBodySize, 0.0, 0.0, "", "Normal");
            contentW = ControlMetrics.TreeExpanderWidth + ControlMetrics.SpacingSM + textSize.Width;
        }
        double availW = availableSize.Width;
        if (availW > 0.0 && availW < LayoutHelper.Unbounded && contentW < availW)
        {
            contentW = availW;
        }
        if (this.Width > 0.0)
        {
            contentW = this.Width;
        }
        return new LayoutSize(contentW, rowH);
    }

    protected override void ArrangeOverride(LayoutSize finalSize)
    {
        if (_virtualRow)
        {
            this.SyncMirrorAll();
            return;
        }
        double rowH = ControlMetrics.TreeRowHeight;
        double indent = ControlMetrics.TreeIndentPerLevel;
        double offscreen = -1000000.0;
        double childW = finalSize.Width - indent;
        if (childW < 0.0)
        {
            childW = 0.0;
        }
        int count = this.CountChildItems();
        if (this.IsExpanded)
        {
            double y = rowH;
            int i = 0;
            while (i < count)
            {
                TreeViewItem child = this.ChildItemAt(i);
                if (child != null)
                {
                    double h = child.DesiredSize.Height;
                    LayoutHelper.ArrangeChild(this, child, indent, y, childW, h);
                    y = y + h;
                }
                i++;
            }
        }
        else
        {
            int i = 0;
            while (i < count)
            {
                TreeViewItem child = this.ChildItemAt(i);
                if (child != null)
                {
                    LayoutHelper.ArrangeChild(this, child, offscreen, offscreen,
                        child.DesiredSize.Width, child.DesiredSize.Height);
                }
                i++;
            }
        }
        this.SyncMirrorAll();
    }
}
