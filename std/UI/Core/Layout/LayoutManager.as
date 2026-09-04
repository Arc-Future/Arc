// RFC 037 D5: Arc.UI.Layout — LayoutManager 布局入口。
//
// Window.Show 在 PlatformBuildFromArc 之前调用 Update，完成逻辑树 Measure/Arrange。
// 布局坐标经 FrameworkElement.LayoutX/Y + RenderSize 由 PlatformTreeSync 写入平台镜像。

namespace Arc.UI.Layout;

using Arc.UI.Components;

/// <summary>布局管线入口——Update 触发 Measure → Arrange（无平台双写）。</summary>
internal class LayoutManager {
    /// <summary>
    /// 对 Window 执行完整布局。
    /// available 通常为 (Window.Width, Window.Height)；零尺寸时安全 no-op。
    /// </summary>
    public static void Update(Window window) {
        if (window == null) {
            return;
        }
        double w = LayoutHelper.Sanitize(window.Width);
        double h = LayoutHelper.Sanitize(window.Height);
        if (w <= 0.0 || h <= 0.0) {
            return;
        }
        LayoutSize available = new LayoutSize(w, h);
        window.Measure(available);
        window.Arrange(available);
    }
}
