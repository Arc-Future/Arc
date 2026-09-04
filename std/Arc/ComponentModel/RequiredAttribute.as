// RFC 012 M3: 内置属性 — 必填标记 [Required]（D2）。
//
// 标记 property/field 为必填，派生自 Attribute 基类。

namespace Arc.ComponentModel;

/// <summary>
/// 标记 property/field 为必填（RFC 012 D2）。
///
/// 用法：`[Required]`（无参数）。
/// 合法附加目标：property / field。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class RequiredAttribute : Attribute {
    public RequiredAttribute() {}
}
