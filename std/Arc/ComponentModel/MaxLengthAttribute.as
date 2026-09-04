// RFC 012 M3: 内置属性 — 字符串最大长度约束 [MaxLength]（D2）。
//
// 标记 string property/field 的最大长度约束，派生自 Attribute 基类。

namespace Arc.ComponentModel;

/// <summary>
/// 标记 string property/field 的最大长度约束（RFC 012 D2）。
///
/// 用法：`[MaxLength(50)]`（必选 1 个 int 参数）。
/// 合法附加目标：property / field。
///
/// **命名诚实**：属性为 <see cref="Max"/>（非 C# `Length`）。
/// tip 上任意成员名 `Length` 经 codegen 与 string/array `.Length` 冲突，读回损坏。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class MaxLengthAttribute : Attribute {
    /// 最大长度。
    public int Max { get; }

    public MaxLengthAttribute(int length) {
        Max = length;
    }
}
