// RFC 037 D5.4 / D5.2: Grid 轨道尺寸（Auto / Star / Pixel）。

namespace Arc.UI.Layout;

/// <summary>Grid 列宽/行高单位（WPF GridLength 最小子集）。</summary>
public class GridLength {
    public const int UnitAuto = 0;
    public const int UnitStar = 1;
    public const int UnitPixel = 2;

    /// <summary>单位类型：Auto / Star / Pixel。</summary>
    public int UnitType;

    /// <summary>Star 权重或 Pixel 像素值。</summary>
    public double Value;

    public static GridLength Auto() {
        GridLength gl = new GridLength();
        gl.UnitType = UnitAuto;
        gl.Value = 0.0;
        return gl;
    }

    public static GridLength Star(double weight) {
        GridLength gl = new GridLength();
        gl.UnitType = UnitStar;
        if (weight <= 0.0) {
            weight = 1.0;
        }
        gl.Value = weight;
        return gl;
    }

    public static GridLength Pixel(double pixels) {
        GridLength gl = new GridLength();
        gl.UnitType = UnitPixel;
        if (pixels < 0.0) {
            pixels = 0.0;
        }
        gl.Value = pixels;
        return gl;
    }

    /// <summary>解析 WPF 风格字面量：Auto / * / 2* / 100。</summary>
    public static GridLength Parse(string text) {
        if (text == null || text.Length == 0) {
            return GridLength.Star(1.0);
        }
        if (text == "Auto") {
            return GridLength.Auto();
        }
        int len = text.Length;
        if (len > 0 && text[len - 1] == '*') {
            if (len == 1) {
                return GridLength.Star(1.0);
            }
            string prefix = text.Substring(0, len - 1);
            double w = LayoutHelper.ParseNumber(prefix, 1.0);
            return GridLength.Star(w);
        }
        double px = LayoutHelper.ParseNumber(text, 0.0);
        return GridLength.Pixel(px);
    }
}
