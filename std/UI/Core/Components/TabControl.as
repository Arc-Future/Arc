// RFC 037 · TabControl 页签容器（内置页签栏 chrome 切片）。
//
// **定位**：多页互斥展示——仅 SelectedIndex 对应 TabItem 参与客户区布局；
// 其余 TabItem Arrange 到视口外（-1e6，对齐 Popup 关闭语义）。页签栏由本类
// 自绘（Header 文案按测宽左对齐 + 选中 Accent 底线），点击经 PointerRouter
// 命中（C 写 HitTabIndex → SelectTab），禁为每页签新建 Button（槽位守恒）。
//
// **三轨**：布局自持（顶栏 HeaderBarHeight + 客户区）；平台镜像写 TabCount/
// Header{i}/HeaderWidth{i}/SelectedIndex；渲染走专属 chrome + 子树递归
// （wgpu 唯一后端）。HeaderWidth = 文案测宽 + 2×TabHeaderPaddingX（WPF 心智：
// 内容尺寸左对齐，不均分拉满栏宽）。
//
// **诚实边界**：无切换动画 / 关闭按钮 / 溢出滚动页签；Header 空串时画「Tab N」。

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Internal;
using Arc.UI.Layout;

/// <summary>页签容器：内置顶栏 chrome + 仅展示 <see cref="SelectedIndex"/> 对应页。</summary>
public class TabControl : Panel {
    /// <summary>内置页签栏高度（DIP；字面量对齐 <c>ControlMetrics.TabHeaderBarHeight</c>）。</summary>
    public const double HeaderBarHeight = 36.0;

    /// <summary>SelectedIndex 属性元数据——当前页索引，默认 0。</summary>
    public static DependencyProperty<int> SelectedIndexProperty =
        RegisterProperty<int>(nameof(SelectedIndex), typeof(TabControl), 0);

    long _mirrorHandle;

    /// <summary>构造并绑定 TypeName。</summary>
    public TabControl() {
        this.Type = typeof(TabControl);
        this.TypeName = "TabControl";
    }

    /// <summary>当前选中页索引（越界时布局按 0 兜底）。</summary>
    public int SelectedIndex {
        get { return this.GetValue<int>(SelectedIndexProperty); }
        set {
            this.SetValue<int>(SelectedIndexProperty, value);
            this.SyncMirrorSelection();
            // 页切换改可见子树几何 → 布局脏（SetValue 已标；显式保底防镜像-only 路径）。
            FramePump.InvalidateLayout();
        }
    }

    /// <summary>平台镜像句柄（PlatformTreeSync 登记；点击命中写回 HitTabIndex）。</summary>
    internal void BindPlatformMirror(long handle) {
        _mirrorHandle = handle;
        this.SyncMirrorHeaders();
        this.SyncMirrorSelection();
    }

    /// <summary>PointerRouter 入口：按 HitTabIndex 切换页（越界忽略）。</summary>
    internal void SelectHitTab() {
        if (_mirrorHandle == 0) {
            return;
        }
        int hit = (int)WindowHost.ElementGetNumber(_mirrorHandle, "HitTabIndex", -1.0);
        if (hit < 0) {
            return;
        }
        int tabCount = this.CountTabItems();
        if (hit >= tabCount) {
            return;
        }
        if (hit == this.SelectedIndex) {
            return;
        }
        this.SelectedIndex = hit;
    }

    void SyncMirrorSelection() {
        if (_mirrorHandle == 0) {
            return;
        }
        WindowHost.ElementSetNumber(_mirrorHandle, "SelectedIndex", (double)this.SelectedIndex);
        WindowHost.ElementSetNumber(_mirrorHandle, "HeaderBarHeight", HeaderBarHeight);
    }

    void SyncMirrorHeaders() {
        if (_mirrorHandle == 0) {
            return;
        }
        int tabCount = this.CountTabItems();
        WindowHost.ElementSetNumber(_mirrorHandle, "TabCount", (double)tabCount);
        WindowHost.ElementSetNumber(_mirrorHandle, "HeaderBarHeight", HeaderBarHeight);
        double padX = ControlMetrics.TabHeaderPaddingX;
        double fontSize = ControlMetrics.TabHeaderFontSize;
        double minW = ControlMetrics.TabHeaderMinWidth;
        int t = 0;
        while (t < tabCount) {
            TabItem page = this.TabItemAt(t);
            string header = "";
            if (page != null && page.Header != null) {
                header = page.Header;
            }
            if (header.Length == 0) {
                header = "Tab " + (t + 1).ToString();
            }
            WindowHost.ElementSetString(_mirrorHandle, "Header" + t, header);
            // 测宽与 DrawText 同源；度量未就绪时 EstimateTextSize 诚实占位，
            // RelayoutSynced 后再 Arrange 写回真实 HeaderWidth。
            LayoutSize textSize = LayoutHelper.EstimateTextSize(
                header, fontSize, 0.0, 0.0, "", "Normal");
            double cellW = textSize.Width + padX * 2.0;
            if (cellW < minW) {
                cellW = minW;
            }
            WindowHost.ElementSetNumber(_mirrorHandle, "HeaderWidth" + t, cellW);
            t++;
        }
    }

    int CountTabItems() {
        if (this.Children == null) {
            return 0;
        }
        int n = 0;
        int i = 0;
        while (i < this.Children.Count) {
            if (this.Children[i] is TabItem) {
                n++;
            }
            i++;
        }
        return n;
    }

    TabItem TabItemAt(int tabIndex) {
        if (this.Children == null || tabIndex < 0) {
            return null;
        }
        int seen = 0;
        int i = 0;
        while (i < this.Children.Count) {
            Element raw = this.Children[i];
            if (raw is TabItem) {
                if (seen == tabIndex) {
                    return (TabItem)raw;
                }
                seen++;
            }
            i++;
        }
        return null;
    }

    int ResolveSelectedIndex() {
        int selected = this.SelectedIndex;
        int tabCount = this.CountTabItems();
        if (tabCount <= 0) {
            return 0;
        }
        if (selected < 0 || selected >= tabCount) {
            return 0;
        }
        return selected;
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        double availW = availableSize.Width;
        double availH = availableSize.Height;
        double contentAvailH = availH;
        if (availH > 0.0 && availH < LayoutHelper.Unbounded) {
            contentAvailH = availH - HeaderBarHeight;
            if (contentAvailH < 0.0) {
                contentAvailH = 0.0;
            }
        }
        int selected = this.ResolveSelectedIndex();
        int tabCount = this.CountTabItems();
        double contentW = 0.0;
        double contentH = 0.0;
        int t = 0;
        while (t < tabCount) {
            TabItem page = this.TabItemAt(t);
            if (page != null) {
                LayoutHelper.MeasureChild(page, new LayoutSize(availW, contentAvailH));
                if (t == selected) {
                    contentW = page.DesiredSize.Width;
                    contentH = page.DesiredSize.Height;
                }
            }
            t++;
        }
        if (availW > 0.0 && availW < LayoutHelper.Unbounded && contentW < availW) {
            contentW = availW;
        }
        if (contentAvailH > 0.0 && contentAvailH < LayoutHelper.Unbounded && contentH < contentAvailH) {
            contentH = contentAvailH;
        }
        double totalH = contentH + HeaderBarHeight;
        if (this.Width > 0.0) {
            contentW = this.Width;
        }
        if (this.Height > 0.0) {
            totalH = this.Height;
        }
        return new LayoutSize(contentW, totalH);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        this.SyncMirrorHeaders();
        this.SyncMirrorSelection();
        int selected = this.ResolveSelectedIndex();
        int tabCount = this.CountTabItems();
        double offscreen = -1000000.0;
        double contentH = finalSize.Height - HeaderBarHeight;
        if (contentH < 0.0) {
            contentH = 0.0;
        }
        int t = 0;
        while (t < tabCount) {
            TabItem page = this.TabItemAt(t);
            if (page != null) {
                if (t == selected) {
                    LayoutHelper.ArrangeChild(this, page, 0.0, HeaderBarHeight,
                        finalSize.Width, contentH);
                } else {
                    LayoutHelper.ArrangeChild(this, page, offscreen, offscreen,
                        page.DesiredSize.Width, page.DesiredSize.Height);
                }
            }
            t++;
        }
    }
}
