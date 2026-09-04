// Arc — Enum 静态工具类（对标 System.Enum）。
//
// `Enum.GetOptions<T>()`：枚举 → 选项集合的编译期生成入口。编译器在
// 单态化为具体枚举 E 时，按 E 各成员的 `[DisplayName]`/`[Description]`
// （Arc.ComponentModel 通用属性）**编译期烘焙** `EnumOptions<E>` 构造体
// （零反射、零运行时开销）。未标注属性的成员回退成员名显示、空串描述。
//
// `Enum.HasFlag<T>` / `Enum.IsDefined<T>` / `Enum.GetNames<T>` /
// `Enum.GetValues<T>`：枚举能力增强（RFC 004 枚举能力增强子项）——编译器
// 在单态化为具体枚举 E 时**编译期烘焙**方法体（位组合判断 / 成员穷举 /
// 名称与值数组），零反射。方法体仅作 stub 占位，实际由 typeck 合成覆盖。
//
// 业务编码：
//   ```
//   public enum MyStatus {
//       [DisplayName("无")][Description("未开始")] None,
//       [DisplayName("完成")][Description("处理完成")] Done,
//   }
//   EnumOptions<MyStatus> options = Enum.GetOptions<MyStatus>();
//   combo.SetOptions(options);
//   bool done = Enum.HasFlag<MyStatus>(value, MyStatus.Done);
//   ```
//
// 分层契约：`[DisplayName]`/`[Description]` 为 Arc.ComponentModel 通用属性
// （RFC 009/012），不绑定 UI 语义；编译器仅消费属性表（通用机制，不感知
// 领域语义）。选项集合模型 `EnumOptions<T>` 归 Arc.ComponentModel。

namespace Arc;

using Arc.ComponentModel;
using Arc.Collections;

/// <summary>
/// 枚举静态工具类（对标 System.Enum）。
/// </summary>
public static class Enum {
    /// <summary>按枚举成员属性编译期生成选项集合（零反射）。</summary>
    /// <typeparam name="T">枚举类型。</typeparam>
    /// <returns>强类型枚举选项集合，供 ComboBox 等绑定。</returns>
    public static EnumOptions<T> GetOptions<T>() {
        return new EnumOptions<T>();
    }

    /// <summary>
    /// 判断 value 是否包含 flag 位（`(value &amp; flag) == flag`）。
    /// 编译器按具体枚举类型烘焙方法体（零反射、零运行时开销）。
    /// </summary>
    /// <typeparam name="T">枚举类型。</typeparam>
    /// <param name="value">待检查的枚举值（可为组合值）。</param>
    /// <param name="flag">目标标志位。</param>
    /// <returns>value 包含 flag 的全部位时为 true。</returns>
    public static bool HasFlag<T>(T value, T flag) {
        return false;
    }

    /// <summary>
    /// 判断 value 是否为枚举的已定义成员（判别值在成员集中）。
    /// 组合值（非单一成员）返回 false，对齐 System.Enum.IsDefined。
    /// </summary>
    /// <typeparam name="T">枚举类型。</typeparam>
    /// <param name="value">待检查的枚举值。</param>
    /// <returns>value 为已定义成员时返回 true。</returns>
    public static bool IsDefined<T>(T value) {
        return false;
    }

    /// <summary>按声明顺序返回全部成员名（零反射）。</summary>
    /// <typeparam name="T">枚举类型。</typeparam>
    /// <returns>成员名列表。</returns>
    public static List<string> GetNames<T>() {
        return new List<string>();
    }

    /// <summary>按声明顺序返回全部成员值（零反射）。</summary>
    /// <typeparam name="T">枚举类型。</typeparam>
    /// <returns>成员值列表。</returns>
    public static List<T> GetValues<T>() {
        return new List<T>();
    }
}
