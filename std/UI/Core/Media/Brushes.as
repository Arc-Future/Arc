// WPF 精髓对齐：Arc.UI.Media — Brushes 命名色注册表。
//
// 对标 System.Windows.Media.Brushes：内置命名色目录，作为「颜色家族」的单一事实来源，
// 供 .as 编码（`= Brushes.Red`）与 arml 命名色解析（`Foreground="Red"` → Brushes.Red）
// 共用。命名色经 <see cref="Color.Parse"/> 的命名回退统一接入，保证「一处定义、处处可解析」。
//
// WPF 对齐（取精华去糟粕）：WPF 的 Brushes.Red 是冻结（Freeze）的 static readonly 常量——
// 恒返回同一实例的稳定引用，而非每次 new 的工厂。本类以 `static readonly` 惰性字段复刻该语义：
//   1. 命名色 = 单一共享 SolidColorBrush 实例（`Brushes.Red` 恒为同一引用，WPF 常量语义），
//      而非每访问 new 一次的分配器（那是工厂，不是常量）；
//   2. 单一事实来源 <see cref="ColorValue"/>（name → Color），命名色字段与 LookupColor 共用，
//      消除重复；`static readonly` 惰性（首触构造一次、线程安全）替代手写 `_x == null` 缓存。
// SolidColorBrush 无 Freeze，共享安全性由约定保证：渲染器只读消费（ToHex），代码库无
// 任何对命名色 .Color/.Opacity 的改写。
//
// 使用模式：
//   - .as 编码：`SolidColorBrush b = Brushes.Red;` · `string hex = Brushes.Red.ToHex();`
//   - arml 解析：`Foreground="Red"`（渲染器经 Color.Parse → Brushes 命名查找）
//   - 主题资源：`<Color x:Key="Color.Primary" Value="#FF4F46E5"/>` 编译期为
//     `ResourceValue.Brush(Brushes.Parse("#FF4F46E5"))`（类型化 IBrush，非字符串）。
//
// 命名空间 Arc.UI.Media（System.Windows.Media 对齐）；框架源合并时剥离 namespace。

namespace Arc.UI.Media;

/// <summary>
/// 命名色注册表——WPF Brushes 对齐。命名色为单一共享常量（static readonly 惰性），
/// 提供 <see cref="Parse"/>（hex/命名二合一）与 <see cref="LookupColor"/>（命名→Color）。
/// </summary>
public class Brushes {
    /// <summary>解析 hex（#RRGGBB/#AARRGGBB）或命名色 → SolidColorBrush；失败回退透明。</summary>
    public static SolidColorBrush Parse(string value) {
        if (value != null && value.Length > 0 && value[0] == '#') {
            return new SolidColorBrush(Color.Parse(value));
        }
        return new SolidColorBrush(Brushes.LookupColor(value));
    }

    /// <summary>命名色查找 → Color；未命中返回透明。</summary>
    public static Color LookupColor(string name) {
        if (name == null || name.Length == 0) {
            return Color.Transparent();
        }
        return Brushes.ColorValue(name);
    }

    /// <summary>命名色存在性查询。</summary>
    public static bool TryGet(string name, ref SolidColorBrush brush) {
        if (name == null || name.Length == 0) {
            return false;
        }
        if (!Brushes.IsNamedColor(name)) {
            return false;
        }
        brush = new SolidColorBrush(Brushes.ColorValue(name));
        return true;
    }

    // ---- 常用命名色（WPF 对齐：单一共享常量，非每次 new 的工厂）----
    // 对标 System.Windows.Media.Brushes：`Brushes.Red` 为同一冻结实例的稳定引用。
    // `static readonly` 惰性：首触构造一次、线程安全；初始化器引用 ColorValue 单一来源。
    public static readonly SolidColorBrush Red = new SolidColorBrush(ColorValue("Red"));
    public static readonly SolidColorBrush Green = new SolidColorBrush(ColorValue("Green"));
    public static readonly SolidColorBrush Blue = new SolidColorBrush(ColorValue("Blue"));
    public static readonly SolidColorBrush Orange = new SolidColorBrush(ColorValue("Orange"));
    public static readonly SolidColorBrush Purple = new SolidColorBrush(ColorValue("Purple"));
    public static readonly SolidColorBrush Black = new SolidColorBrush(ColorValue("Black"));
    public static readonly SolidColorBrush White = new SolidColorBrush(ColorValue("White"));
    public static readonly SolidColorBrush Gray = new SolidColorBrush(ColorValue("Gray"));
    public static readonly SolidColorBrush Silver = new SolidColorBrush(ColorValue("Silver"));
    public static readonly SolidColorBrush Transparent = new SolidColorBrush(ColorValue("Transparent"));

    /// <summary>命名色存在性判定（TryGet 用；与命名色字段集合保持一致）。</summary>
    private static bool IsNamedColor(string name) {
        return name == "Red" || name == "Green" || name == "Blue" || name == "Orange"
            || name == "Purple" || name == "Black" || name == "White" || name == "Gray"
            || name == "Silver" || name == "Transparent";
    }

    /// <summary>命名色唯一事实来源（name → Color）；未知名回退透明。</summary>
    private static Color ColorValue(string name) {
        if (name == "Red") { return Color.FromRgba(0.816, 0.075, 0.075, 1.0); }
        if (name == "Green") { return Color.FromRgba(0.0, 0.502, 0.0, 1.0); }
        if (name == "Blue") { return Color.FromRgba(0.0, 0.0, 0.502, 1.0); }
        if (name == "Orange") { return Color.FromRgba(1.0, 0.647, 0.0, 1.0); }
        if (name == "Purple") { return Color.FromRgba(0.502, 0.0, 0.502, 1.0); }
        if (name == "Black") { return Color.Black(); }
        if (name == "White") { return Color.White(); }
        if (name == "Gray") { return Color.FromRgba(0.5, 0.5, 0.5, 1.0); }
        if (name == "Silver") { return Color.FromRgba(0.749, 0.749, 0.749, 1.0); }
        if (name == "Transparent") { return Color.Transparent(); }
        return Color.Transparent();
    }
}
