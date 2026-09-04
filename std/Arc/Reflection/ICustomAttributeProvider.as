// RFC 018 §4.2.1: 自定义属性提供者接口——对标 C# System.Reflection.ICustomAttributeProvider。
//
// MemberInfo 与 ParameterInfo 实现此接口，统一属性查询入口。
// 仅描述属性元数据（类型 + 构造参数 + 命名参数），不实例化属性对象，
// 与 RFC 018 §3.2『反射元数据描述保留 / 反射动态操作删除』二分边界一致。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// 自定义属性提供者接口——对标 C# System.Reflection.ICustomAttributeProvider。
///
/// MemberInfo 与 ParameterInfo 实现此接口，统一属性查询入口。
/// 仅描述属性元数据（类型 + 构造参数 + 命名参数），不实例化属性对象，
/// 不提供 Invoke/GetValue 等反射动态操作（RFC 018 §3.2 二分边界）。
/// </summary>
public interface ICustomAttributeProvider {
    /// <summary>返回此成员上声明的所有属性数据（只读快照，不含运行时实例）。</summary>
    /// <returns>属性数据列表；无属性返回空列表。</returns>
    List<CustomAttributeData> GetCustomAttributes();

    /// <summary>判断此成员是否声明了指定类型的属性。</summary>
    /// <param name="attributeType">属性类型（typeof(Attribute 派生类)）。</param>
    /// <returns>声明返回 true；否则 false。</returns>
    bool IsDefined(Type attributeType);
}
