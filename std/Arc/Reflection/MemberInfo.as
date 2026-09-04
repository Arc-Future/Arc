// RFC 018 §4.2.2: 成员信息抽象基类——对齐 C# System.Reflection.MemberInfo。
//
// 所有类型成员（Type/MethodBase/FieldInfo/PropertyInfo/EventInfo）的统一基类。
// 实现只读元数据描述，不提供 Invoke/GetValue/SetValue 等反射动态操作。
//
// **设计偏差（vs RFC 018 §4.2.2）**：
// RFC §4.2.2 字段声明为 `public MemberAttributes Attributes { get; }`，引用
// MemberAttributes 类型；但 RFC 018 §4.3 枚举定义清单中遗漏了 MemberAttributes
// 枚举（仅列出 MemberTypes/TypeKind/TypeAttributes/MethodAttributes/FieldAttributes/
// PropertyAttributes/EventAttributes/ParameterAttributes/BindingFlags 九个）。
// 为保持设计自洽，本文件按 RFC §4.2.2 原设计使用 MemberAttributes 类型，
// 并在同目录下新增 MemberAttributes.as（class + public const int 模式，
// 与其它 *Attributes 文件一致）。RFC 018 后续修订应补充 §4.3 MemberAttributes 定义。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// 成员信息抽象基类——对齐 C# System.Reflection.MemberInfo。
///
/// 所有类型成员（Type/MethodBase/FieldInfo/PropertyInfo/EventInfo）的统一基类。
/// 实现只读元数据描述，**不提供** Invoke/GetValue/SetValue 等反射动态操作
/// （RFC 018 §3.2 二分边界）。
/// </summary>
public abstract class MemberInfo : ICustomAttributeProvider {
    /// <summary>成员名（不含命名空间前缀）。</summary>
    /// <remarks>
    /// RFC 018 M3+：抽象属性——具体实现（RuntimeType / RuntimeMethodInfo /
    /// RuntimeFieldInfo / RuntimePropertyInfo）由 codegen 从 Rt*Info rodata
    /// 拦截填充；禁止带存储的自动属性，否则 `t.Name` 会读零初始化字段而非元数据。
    /// </remarks>
    public abstract string Name { get; }

    /// <summary>成员类型枚举（Method/Field/Property/Event/Type/NestedType 等）。</summary>
    public MemberTypes MemberType { get; }

    /// <summary>声明此成员的类型。</summary>
    public Type DeclaringType { get; }

    /// <summary>此成员所属程序集的命名空间。</summary>
    public string Namespace { get; }

    /// <summary>成员特性位掩码（C# System.Reflection.MemberAttributes 对齐）。</summary>
    public MemberAttributes Attributes { get; }

    /// <summary>受保护构造函数——派生类通过 : base() 调用。</summary>
    protected MemberInfo() {}

    /// <summary>返回此成员上声明的所有属性数据（ICustomAttributeProvider 实现）。</summary>
    /// <returns>属性数据列表；无属性返回空列表。</returns>
    public abstract List<CustomAttributeData> GetCustomAttributes();

    /// <summary>判断此成员是否声明了指定类型的属性（ICustomAttributeProvider 实现）。</summary>
    /// <param name="attributeType">属性类型。</param>
    /// <returns>声明返回 true；否则 false。</returns>
    public abstract bool IsDefined(Type attributeType);
}
