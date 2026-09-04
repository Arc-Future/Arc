// RFC 037 references/texture-surface：Stretch → 目标矩形 + 源 UV 的唯一映射实现。
//
// Image（RFC 029 M2）与 VideoSurface 的双宿主渲染（平台镜像 RenderTree /
// DrawList 预览）共用本映射器——缩放语义单一惯用法，渲染结果由构造保证一致
// （RFC 037 §10 G1）。语义对标 WPF Stretch，与 Arc.UI Stretch 枚举文档一致。

namespace Arc.UI.Rendering;

using Arc.UI;

/// <summary>Stretch 映射结果：目标矩形 + 源 UV（值类型，构造期即定）。</summary>
public struct StretchMapping {
    /// <summary>目标矩形左上角 X。</summary>
    public double X;
    /// <summary>目标矩形左上角 Y。</summary>
    public double Y;
    /// <summary>目标矩形宽。</summary>
    public double Width;
    /// <summary>目标矩形高。</summary>
    public double Height;
    /// <summary>源 U 起点（0..1）。</summary>
    public double U0;
    /// <summary>源 V 起点（0..1）。</summary>
    public double V0;
    /// <summary>源 U 终点（0..1）。</summary>
    public double U1;
    /// <summary>源 V 终点（0..1）。</summary>
    public double V1;

    /// <summary>构造映射结果。</summary>
    public StretchMapping(double x, double y, double width, double height,
                          double u0, double v0, double u1, double v1) {
        X = x;
        Y = y;
        Width = width;
        Height = height;
        U0 = u0;
        V0 = v0;
        U1 = u1;
        V1 = v1;
    }
}

/// <summary>
/// Stretch 缩放映射器（对标 WPF Stretch；双宿主唯一实现）。
/// </summary>
public static class StretchMapper {
    /// <summary>
    /// 计算纹理到元素的采样映射。
    /// </summary>
    /// <param name="stretch">缩放模式。</param>
    /// <param name="tw">纹理宽（像素）。</param>
    /// <param name="th">纹理高（像素）。</param>
    /// <param name="x">元素左上角 X。</param>
    /// <param name="y">元素左上角 Y。</param>
    /// <param name="w">元素宽。</param>
    /// <param name="h">元素高。</param>
    public static StretchMapping Compute(Stretch stretch, double tw, double th,
                                         double x, double y, double w, double h) {
        if (tw <= 0.0 || th <= 0.0 || w <= 0.0 || h <= 0.0) {
            return new StretchMapping(x, y, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0);
        }
        switch (stretch)
        {
            case Stretch.Uniform:
            {
                // 等比缩放完整显示：目标矩形按比例收窄并在元素内居中；全源映射。
                double scale = (w / tw) < (h / th) ? (w / tw) : (h / th);
                double dw = tw * scale;
                double dh = th * scale;
                return new StretchMapping(x + (w - dw) / 2.0, y + (h - dh) / 2.0,
                                          dw, dh, 0.0, 0.0, 1.0, 1.0);
            }
            case Stretch.UniformToFill:
            {
                // 等比缩放填满元素：目标矩形不变，源 UV 中心裁剪对齐元素宽高比。
                double scale = (w / tw) > (h / th) ? (w / tw) : (h / th);
                double swSrc = w / scale;
                double shSrc = h / scale;
                double u0 = (tw - swSrc) / 2.0 / tw;
                double v0 = (th - shSrc) / 2.0 / th;
                return new StretchMapping(x, y, w, h,
                                          u0, v0, u0 + swSrc / tw, v0 + shSrc / th);
            }
            case Stretch.None:
            {
                // 不缩放：按源尺寸原样绘制（元素内左上对齐，可能溢出或留白）。
                return new StretchMapping(x, y, tw, th, 0.0, 0.0, 1.0, 1.0);
            }
            case Stretch.Fill:
            default:
            {
                // 拉伸填满元素（宽高独立缩放，可能变形）；全源映射。
                return new StretchMapping(x, y, w, h, 0.0, 0.0, 1.0, 1.0);
            }
        }
    }
}
