// RFC 037 §9 / custom-fonts：布局与绘制同源度量契约。
//
// UI 帧内文本度量入口——不绑定 Arc.Drawing.Font（离屏成像另轨，禁双轨）。
// 实现由 wgpu atlas 后端提供；FontManager / Application 可稍后挂接同一服务。

namespace Arc.UI.Layout;

/// <summary>
/// 布局侧文本度量——与 DrawText 同源（同一 atlas / 同一已解析族 / 字重）。
/// </summary>
public interface ITextMetrics {
    /// <summary>
    /// 测量文本在给定字号、族名与字重下的布局尺寸（含 padding）。
    /// fontFamily 为空或未解析时回退默认族；fontWeight 非 Bold 时走 Normal 面（与绘制一致）。
    /// </summary>
    LayoutSize MeasureText(string text, double fontSize, double padX, double padY,
                           string fontFamily, string fontWeight);
}
