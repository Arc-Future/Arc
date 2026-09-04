// RFC 037 M-VZ1: 项列表视口算术（对齐 EditorViewport 纪律）。

namespace Arc.UI.Layout;

/// <summary>
/// ItemsControl 视口：滚动偏移 → 可见项索引范围 + 算术 Extent（M-VZ1）。
/// </summary>
/// <remarks>
/// ExtentHeight = itemCount × itemStride；禁止为不可见项 Measure/AddChild。
/// 见 RFC 037 §6.1–§6.2。
/// </remarks>
internal class ItemViewport {
    public const double DefaultViewportHeight = 480.0;

    private int _firstIndex;
    private int _lastIndex;
    private double _subItemOffset;
    private double _itemStride;
    private double _extentHeight;

    public ItemViewport() {
        _firstIndex = 0;
        _lastIndex = -1;
        _subItemOffset = 0.0;
        _itemStride = 0.0;
        _extentHeight = 0.0;
    }

    public int FirstIndex {
        get { return _firstIndex; }
    }

    public int LastIndex {
        get { return _lastIndex; }
    }

    public double SubItemOffset {
        get { return _subItemOffset; }
    }

    public double ItemStride {
        get { return _itemStride; }
    }

    public double ExtentHeight {
        get { return _extentHeight; }
    }

    /// <summary>
    /// 根据滚动偏移、视口高度与项 stride 更新可见项窗口。
    /// </summary>
    /// <param name="cacheBeforeScreens">视口上方缓冲（视口倍数，默认 1）。</param>
    /// <param name="cacheAfterScreens">视口下方缓冲（视口倍数，默认 1）。</param>
    public void Update(double scrollOffsetY, double viewportHeight, int itemCount,
                       double itemStride, double cacheBeforeScreens, double cacheAfterScreens) {
        _itemStride = itemStride;
        if (_itemStride < 1.0) {
            _itemStride = 20.0;
        }

        if (itemCount <= 0) {
            _firstIndex = 0;
            _lastIndex = -1;
            _subItemOffset = 0.0;
            _extentHeight = 0.0;
            return;
        }

        _extentHeight = (double)itemCount * _itemStride;

        double vpH = viewportHeight;
        if (vpH <= 0.0) {
            vpH = DefaultViewportHeight;
        }

        double cacheBeforePx = cacheBeforeScreens * vpH;
        double cacheAfterPx = cacheAfterScreens * vpH;

        double bandTop = scrollOffsetY - cacheBeforePx;
        if (bandTop < 0.0) {
            bandTop = 0.0;
        }

        int first = (int)(bandTop / _itemStride);
        if (first < 0) {
            first = 0;
        }

        double bandBottom = scrollOffsetY + vpH + cacheAfterPx;
        int last = (int)(bandBottom / _itemStride);
        if (last >= itemCount) {
            last = itemCount - 1;
        }
        if (last < first) {
            last = first;
        }

        _firstIndex = first;
        _lastIndex = last;
        _subItemOffset = scrollOffsetY - (double)first * _itemStride;
    }
}
