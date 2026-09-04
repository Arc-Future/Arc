// RFC 037 §3.1 + WPF 精华：Arc.UI.Media — Color 结构化颜色。
//
// 对标 System.Windows.Media.Color：RGBA 分量（0–1 浮点），提供 hex/named 解析、
// ToHex 序列化、透明度派生与插值。作为画刷体系与主题资源的值类型底座：
//   - 主题资源以**类型化 Color/SolidColorBrush** 注册（编译期/加载期一次解析），
//     替代 hex 字符串逐帧 DecodeHexColor 的运行时开销；
//   - 渲染器消费 Color 可直接到位（RGBA 0–1），无需再解析字符串。
//
// 命名空间 Arc.UI.Media（WPF System.Windows.Media 对齐）；框架源合并时
// 剥离 namespace，与 Styling/ 同命名空间可见。

namespace Arc.UI.Media;

using Arc.Text;

/// <summary>结构化颜色（RGBA 分量 0–1 浮点；抗锯齿/插值友好）。</summary>
public struct Color {
    /// <summary>红分量（0–1）。</summary>
    public double R;

    /// <summary>绿分量（0–1）。</summary>
    public double G;

    /// <summary>蓝分量（0–1）。</summary>
    public double B;

    /// <summary>透明度（0=全透明，1=不透明）。</summary>
    public double A;

    public Color() {
    }

    /// <summary>
    /// 规范构造路径（静态工厂）——规避「struct 带参构造函数体整体不执行」的
    /// 编译器缺陷：`new Color(r,g,b,a)` 当前参数全部丢失（字段停留默认值）。
    /// 工厂经「默认构造 + 手动字段赋值」落地，是当前唯一可靠的 Color 构造方式；
    /// 编译器修复后本工厂与带参 ctor 等价，内部代码一律经本工厂（单一事实来源）。
    /// </summary>
    public static Color FromRgba(double r, double g, double b, double a) {
        Color c = new Color();
        c.R = r;
        c.G = g;
        c.B = b;
        c.A = a;
        return c;
    }

    /// <summary>构造不透明颜色（编译器缺陷修复前参数不生效，请用 <see cref="FromRgba"/>）。</summary>
    public Color(double r, double g, double b) {
        this.R = r;
        this.G = g;
        this.B = b;
        this.A = 1.0;
    }

    /// <summary>构造带透明度的颜色。</summary>
    public Color(double r, double g, double b, double a) {
        this.R = r;
        this.G = g;
        this.B = b;
        this.A = a;
    }

    /// <summary>全透明。</summary>
    public static Color Transparent() {
        return Color.FromRgba(0.0, 0.0, 0.0, 0.0);
    }

    /// <summary>纯黑。</summary>
    public static Color Black() {
        return Color.FromRgba(0.0, 0.0, 0.0, 1.0);
    }

    /// <summary>纯白。</summary>
    public static Color White() {
        return Color.FromRgba(1.0, 1.0, 1.0, 1.0);
    }

    /// <summary>派生新颜色（覆盖透明度）。</summary>
    public Color WithOpacity(double alpha) {
        return Color.FromRgba(this.R, this.G, this.B, alpha);
    }

    /// <summary>
    /// 解析 hex 或命名颜色。
    /// 支持 `#RRGGBB`（6 位）、`#AARRGGBB`（8 位，alpha first，XAML 约定）、
    /// 以及内置命名色（如 "Red"）。解析失败返回全透明。
    /// </summary>
    public static Color Parse(string value) {
        if (value == null || value.Length == 0) {
            return Color.Transparent();
        }
        int start = 0;
        if (value[0] == '#') {
            start = 1;
        }
        int len = value.Length - start;
        if (len == 6) {
            double r = (double)Color.BytePair(value, start) / 255.0;
            double g = (double)Color.BytePair(value, start + 2) / 255.0;
            double b = (double)Color.BytePair(value, start + 4) / 255.0;
            return Color.FromRgba(r, g, b, 1.0);
        }
        if (len == 8) {
            double a = (double)Color.BytePair(value, start) / 255.0;
            double r = (double)Color.BytePair(value, start + 2) / 255.0;
            double g = (double)Color.BytePair(value, start + 4) / 255.0;
            double b = (double)Color.BytePair(value, start + 6) / 255.0;
            return Color.FromRgba(r, g, b, a);
        }
        return Color.SafeNamed(value);
    }

    /// <summary>命名色查找（单一来源：Brushes 注册表；未命中返回透明）。</summary>
    private static Color SafeNamed(string name) {
        return Brushes.LookupColor(name);
    }

    private static int BytePair(string s, int pos) {
        if (pos + 1 >= s.Length) {
            return 0;
        }
        return Color.HexDigit(s[pos]) * 16 + Color.HexDigit(s[pos + 1]);
    }

    private static int HexDigit(char c) {
        if (c >= '0' && c <= '9') {
            return (int)c - (int)'0';
        }
        if (c >= 'a' && c <= 'f') {
            return (int)c - (int)'a' + 10;
        }
        if (c >= 'A' && c <= 'F') {
            return (int)c - (int)'A' + 10;
        }
        return 0;
    }

    /// <summary>序列化为 #AARRGGBB hex（钳制 0–255）。</summary>
    public string ToHex() {
        StringBuilder sb = new StringBuilder(9);
        sb.Append('#');
        Color.AppendByte(sb, this.A);
        Color.AppendByte(sb, this.R);
        Color.AppendByte(sb, this.G);
        Color.AppendByte(sb, this.B);
        return sb.ToString();
    }

    private static void AppendByte(StringBuilder sb, double v) {
        int iv = (int)(v * 255.0 + 0.5);
        if (iv < 0) { iv = 0; }
        if (iv > 255) { iv = 255; }
        sb.Append(Color.HexChar(iv / 16));
        sb.Append(Color.HexChar(iv % 16));
    }

    private static char HexChar(int d) {
        if (d < 10) {
            return (char)((int)'0' + d);
        }
        return (char)((int)'A' + (d - 10));
    }

    /// <summary>线性插值（t=0 → a，t=1 → b）；用于 MotionEngine 态色过渡。</summary>
    public static Color Lerp(Color a, Color b, double t) {
        if (t <= 0.0) { return a; }
        if (t >= 1.0) { return b; }
        return Color.FromRgba(
            Color.Lerp(a.R, b.R, t),
            Color.Lerp(a.G, b.G, t),
            Color.Lerp(a.B, b.B, t),
            Color.Lerp(a.A, b.A, t));
    }

    private static double Lerp(double x, double y, double t) {
        return x + (y - x) * t;
    }
}