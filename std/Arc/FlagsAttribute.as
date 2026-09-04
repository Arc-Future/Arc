// Arc — Flags 标记特性（对标 System.FlagsAttribute）。
//
// 标记枚举为位域组合（bit flags）：成员值通常为 2 的幂，可通过位运算
// `|` `&` `^` `~` 组合（RFC 004 枚举能力增强子项）。`[Flags]` 是
// **纯标记**（无数据成员、无行为）：不改变编译器对枚举的处理，仅作为
// 运行时元数据供工具/框架消费（如枚举组合的显示格式化）。
//
// 用法：
//   ```
//   [Flags]
//   public enum FileAccess {
//       None = 0,
//       Read = 1,
//       Write = 2,
//       ReadWrite = Read | Write,
//   }
//   ```
//
// 合法附加目标：仅 enum（AttributeUsage 约束，typeck 校验）。

namespace Arc;

/// <summary>
/// 标记枚举为位域组合（bit flags）。仅可附加于枚举声明。
/// </summary>
[AttributeUsage(AttributeTargets.Enum)]
public class FlagsAttribute : Attribute {
}
