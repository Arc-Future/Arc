// RFC 037 M-VZ1: VirtualizingStackPanel — 项视口窗口 + extent 算术化。

namespace Arc.UI.Components.Layout;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Layout;

/// <summary>
/// 垂直虚拟化栈面板：只物化可见项 ± CacheLength 缓冲带（RFC 037 §6）。
/// </summary>
public class VirtualizingStackPanel : Panel {
    public const string ItemIndexKey = "Arc.UI.ItemIndex";

    public static DependencyProperty<double> VerticalOffsetProperty =
        RegisterProperty<double>(nameof(VerticalOffset), typeof(VirtualizingStackPanel), 0.0);

    public static DependencyProperty<double> ItemHeightProperty =
        RegisterProperty<double>(nameof(ItemHeight), typeof(VirtualizingStackPanel), 0.0);

    public static DependencyProperty<double> CacheLengthBeforeProperty =
        RegisterProperty<double>(nameof(CacheLengthBefore), typeof(VirtualizingStackPanel), 1.0);

    public static DependencyProperty<double> CacheLengthAfterProperty =
        RegisterProperty<double>(nameof(CacheLengthAfter), typeof(VirtualizingStackPanel), 1.0);

    public static DependencyProperty<Orientation> OrientationProperty =
        RegisterProperty<Orientation>(nameof(Orientation), typeof(VirtualizingStackPanel), Orientation.Vertical);

    public ItemContainerGenerator Generator;
    public ItemSourceView View;
    public TextBlock ItemDefaults;

    ItemViewport _viewport;
    double _lastViewportHeight;

    public VirtualizingStackPanel() {
        this.Type = typeof(VirtualizingStackPanel);
        this.TypeName = "VirtualizingStackPanel";
        _viewport = new ItemViewport();
        _lastViewportHeight = ItemViewport.DefaultViewportHeight;
    }

    public double VerticalOffset {
        get { return this.GetValue<double>(VerticalOffsetProperty); }
        set { this.SetValue<double>(VerticalOffsetProperty, value); }
    }

    public double ItemHeight {
        get { return this.GetValue<double>(ItemHeightProperty); }
        set { this.SetValue<double>(ItemHeightProperty, value); }
    }

    public double CacheLengthBefore {
        get { return this.GetValue<double>(CacheLengthBeforeProperty); }
        set { this.SetValue<double>(CacheLengthBeforeProperty, value); }
    }

    public double CacheLengthAfter {
        get { return this.GetValue<double>(CacheLengthAfterProperty); }
        set { this.SetValue<double>(CacheLengthAfterProperty, value); }
    }

    public Orientation Orientation {
        get { return this.GetValue<Orientation>(OrientationProperty); }
        set { this.SetValue<Orientation>(OrientationProperty, value); }
    }

    public double ExtentHeight {
        get { return _viewport.ExtentHeight; }
    }

    public int FirstMaterializedIndex {
        get { return _viewport.FirstIndex; }
    }

    public int LastMaterializedIndex {
        get { return _viewport.LastIndex; }
    }

    /// <summary>无 Measure  pass 时按默认视口物化（OnLoaded smoke）。</summary>
    public void EnsureViewportMaterialization() {
        this.UpdateViewport(_lastViewportHeight);
        this.MaterializeWindow();
    }

    /// <summary>
    /// 集合级变更入口（RFC 037 M6）：重算视口窗口后，按 kind/index 增量驱动容器复用。
    /// </summary>
    public void ApplyCollectionChange(CollectionChangedEventArgs<object> args, TextBlock itemDefaults) {
        this.UpdateViewport(_lastViewportHeight);
        if (this.Generator == null) {
            return;
        }
        this.Generator.SyncWindow(_viewport.FirstIndex, _viewport.LastIndex);
        CollectionChangeAction action = args.Action;
        if (action == CollectionChangeAction.Add) {
            this.Generator.ApplyAdd(args.Index, itemDefaults);
        } else if (action == CollectionChangeAction.Insert) {
            this.Generator.ApplyInsert(args.Index, itemDefaults);
        } else if (action == CollectionChangeAction.Remove) {
            this.Generator.ApplyRemove(args.Index, itemDefaults);
        } else if (action == CollectionChangeAction.Update) {
            // 视图（ItemSourceView）已先行同步，容器按 index 直读新值重绑
            //（RFC 037 M-VZ1 重构后 generator 按索引取 `ItemAt/DisplayAt`，
            // 不再接收变更项本体——`args.NewItem` 为旧直绑 API 残留）。
            this.Generator.ApplyUpdate(args.Index, itemDefaults);
        } else if (action == CollectionChangeAction.Move) {
            this.Generator.ApplyMove(args.OldIndex, args.Index, itemDefaults);
        } else if (action == CollectionChangeAction.Clear) {
            this.Generator.ApplyClear();
        }
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        double vpH = availableSize.Height;
        if (vpH <= 0.0) {
            vpH = _lastViewportHeight;
        } else {
            _lastViewportHeight = vpH;
        }

        this.UpdateViewport(vpH);
        this.MaterializeWindow();

        double maxCross = 0.0;
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            FrameworkElement child = (FrameworkElement)this.Children[i];
            LayoutHelper.MeasureChild(child, new LayoutSize(availableSize.Width, LayoutHelper.Unbounded));
            if (child.DesiredSize.Width > maxCross) {
                maxCross = child.DesiredSize.Width;
            }
            i++;
        }

        double extentH = _viewport.ExtentHeight;
        double w = maxCross;
        double h = extentH;
        if (availableSize.Width > 0.0 && w > availableSize.Width) {
            w = availableSize.Width;
        }
        return new LayoutSize(w, h);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        _lastViewportHeight = finalSize.Height;
        double stride = this.ResolveItemStride();
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            Element raw = this.Children[i];
            TextBlock child = (TextBlock)raw;
            int idx = (int)child.GetAttachedNumber(ItemIndexKey, -1.0);
            double y = (double)idx * stride;
            LayoutHelper.ArrangeChild(this, child, 0.0, y, finalSize.Width, stride);
            i++;
        }
    }

    private void UpdateViewport(double viewportHeight) {
        int itemCount = 0;
        if (this.View != null) {
            itemCount = this.View.Count;
        }
        double stride = this.ResolveItemStride();
        _viewport.Update(
            this.VerticalOffset,
            viewportHeight,
            itemCount,
            stride,
            this.CacheLengthBefore,
            this.CacheLengthAfter);
    }

    private void MaterializeWindow() {
        if (this.Generator == null) {
            return;
        }
        int first = _viewport.FirstIndex;
        int last = _viewport.LastIndex;
        if (this.View == null) {
            return;
        }
        if (this.View.Count == 0 || last < first) {
            this.Generator.RecycleAll();
            return;
        }
        this.Generator.EnsureRange(first, last, this.ItemDefaults);
    }

    private double ResolveItemStride() {
        double h = this.ItemHeight;
        if (h > 0.0) {
            return h;
        }
        if (this.ItemDefaults != null) {
            LayoutSize est = LayoutHelper.EstimateTextSize(
                "X", this.ItemDefaults.FontSize,
                LayoutHelper.MinTextPaddingX, LayoutHelper.MinTextPaddingY,
                this.ItemDefaults.FontFamily, this.ItemDefaults.FontWeight);
            if (est.Height > 0.0) {
                return est.Height;
            }
        }
        return 20.0;
    }
}
