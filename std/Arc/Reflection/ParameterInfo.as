// RFC 018 §4.2.7: 参数信息——对齐 C# System.Reflection.ParameterInfo。
//
// 独立类，非 MemberInfo 派生（C# 同样独立）。
// 实现 ICustomAttributeProvider，提供参数上的属性查询入口。
// 默认实现返回空属性列表与 false——具体派生类（如 codegen 发射的运行时实例）
// 覆写 GetCustomAttributes / IsDefined 提供真实数据。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// 参数信息——对齐 C# System.Reflection.ParameterInfo。
///
/// 独立类，非 MemberInfo 派生（C# 同样独立）。
/// 实现 ICustomAttributeProvider，提供参数上的属性查询入口。
/// 默认实现返回空属性列表与 false——具体派生类（如 codegen 发射的运行时实例）
/// 覆写 GetCustomAttributes / IsDefined 提供真实数据。
/// </summary>
public class ParameterInfo : ICustomAttributeProvider {
    /// <summary>参数名。</summary>
    public string Name { get; }

    /// <summary>参数类型。</summary>
    public Type ParameterType { get; }

    /// <summary>参数位置（0 起始）。</summary>
    public int Position { get; }

    /// <summary>是否为 in 参数（readonly ref）。</summary>
    public bool IsIn { get; }

    /// <summary>是否为 out 参数。</summary>
    public bool IsOut { get; }

    /// <summary>是否为 ref 参数。</summary>
    public bool IsByRef { get; }

    /// <summary>是否为可选参数（有默认值）。</summary>
    public bool IsOptional { get; }

    /// <summary>是否有默认值。</summary>
    public bool HasDefaultValue { get; }

    /// <summary>默认值（HasDefaultValue=false 时为 null）。</summary>
    /// <remarks>
    /// 类型为 object?（RFC 016 v2 的 object 根类型），仅承载常量字面量装箱：
    /// int/long/bool/string/typeof(T)/enum 值。codegen 发射时按字面量类型分槽存储，
    /// 运行时通过 object 类型统一访问。
    /// </remarks>
    public object? DefaultValue { get; }

    /// <summary>默认构造函数——初始化所有字段为默认值。</summary>
    public ParameterInfo() {}

    /// <summary>
    /// 返回参数上声明的所有属性（ICustomAttributeProvider 实现）。
    /// 默认实现返回空列表——派生类覆写以提供真实数据。
    /// </summary>
    /// <returns>属性数据列表；默认实现返回空列表。</returns>
    public virtual List<CustomAttributeData> GetCustomAttributes() {
        return new List<CustomAttributeData>();
    }

    /// <summary>
    /// 判断参数是否声明了指定类型的属性（ICustomAttributeProvider 实现）。
    /// 默认实现返回 false——派生类覆写以提供真实判定。
    /// </summary>
    /// <param name="attributeType">属性类型。</param>
    /// <returns>声明返回 true；默认实现返回 false。</returns>
    public virtual bool IsDefined(Type attributeType) {
        return false;
    }
}
