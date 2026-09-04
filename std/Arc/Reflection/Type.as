// RFC 018 §4.2.3: 类型信息抽象类——对齐 C# System.Type。
//
// 表示一个类型的完整元数据描述：身份标识 + 名字 + 命名空间 + 基类 + 成员列表 + 属性。
// 永久剔除 Invoke/CreateInstance/GetType() 等反射动态操作。
// typeof(T) 在编译期由 codegen 发射为指向 RtTypeInfo 全局常量的 Type 实例。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// 类型信息抽象类——对齐 C# System.Type。
///
/// 表示一个类型的完整元数据描述：身份标识 + 名字 + 命名空间 + 基类 + 成员列表 + 属性。
/// 永久剔除 Invoke/CreateInstance/GetType() 等反射动态操作（RFC 018 §3.2 二分边界）。
/// typeof(T) 在编译期由 codegen 发射为指向 RtTypeInfo 全局常量的 Type 实例
/// （RFC 018 §3.4 / §7）。
/// </summary>
public abstract class Type : MemberInfo {
    /// <summary>类型唯一标识（FNV-1a 32 位哈希；DI / 本地化等 O(1) 查找键）。</summary>
    /// <remarks>语言层不再有独立 TypeId struct；身份即本 int 属性。</remarks>
    public abstract int TypeId { get; }

    /// <summary>类型完整限定名（含命名空间前缀，如 "Arc.Collections.List"）。</summary>
    public abstract string FullName { get; }

    /// <summary>类型分类枚举（Primitive/Class/Struct/Interface/Enum/Array/Nullable/...）。</summary>
    public abstract TypeKind Kind { get; }

    /// <summary>是否为 class（引用类型）。</summary>
    public abstract bool IsClass { get; }

    /// <summary>是否为 struct（值类型，非 enum）。</summary>
    public abstract bool IsStruct { get; }

    /// <summary>是否为 interface。</summary>
    public abstract bool IsInterface { get; }

    /// <summary>是否为 enum。</summary>
    public abstract bool IsEnum { get; }

    /// <summary>是否为基元类型（int/long/short/byte/char/float/double/bool/string/void）。</summary>
    public abstract bool IsPrimitive { get; }

    /// <summary>是否为数组类型。</summary>
    public bool IsArray { get; }

    /// <summary>是否为可空类型（T?）。</summary>
    public bool IsNullable { get; }

    /// <summary>是否为泛型类型（List&lt;T&gt; 已单态化为 List_int 则返回 false）。</summary>
    public bool IsGenericType { get; }

    /// <summary>是否为抽象类型（abstract class 或 interface）。</summary>
    public bool IsAbstract { get; }

    /// <summary>是否为 sealed 类型（不可继承）。</summary>
    public bool IsSealed { get; }

    /// <summary>直接基类（struct/enum 返回 null；object 返回 null；class 返回父类）。</summary>
    public abstract Type? BaseType { get; }

    /// <summary>元素类型（仅 IsArray/IsNullable/IsGenericType 时有效，否则返回 null）。</summary>
    public Type? ElementType { get; }

    /// <summary>实现的接口列表（仅 class/struct，按声明顺序）。</summary>
    public List<Type> ImplementedInterfaces { get; }

    /// <summary>受保护构造函数——派生类（如 TypeInfo）通过 : base() 调用。</summary>
    protected Type() {}

    /// <summary>返回此类型上声明的所有方法（含继承的 public 方法，不含 private）。</summary>
    /// <returns>方法信息列表。</returns>
    public abstract List<MethodInfo> GetMethods();

    /// <summary>返回此类型上声明的所有字段（含继承的 public 字段）。</summary>
    /// <returns>字段信息列表。</returns>
    public abstract List<FieldInfo> GetFields();

    /// <summary>返回此类型上声明的所有属性（含继承的 public 属性）。</summary>
    /// <returns>属性信息列表。</returns>
    public abstract List<PropertyInfo> GetProperties();

    /// <summary>返回此类型上声明的所有事件。</summary>
    /// <returns>事件信息列表。</returns>
    public abstract List<EventInfo> GetEvents();

    /// <summary>返回此类型上声明的所有构造函数。</summary>
    /// <returns>构造函数信息列表。</returns>
    public abstract List<ConstructorInfo> GetConstructors();

    /// <summary>返回此类型上声明的所有成员（方法 + 字段 + 属性 + 事件 + 嵌套类型）。</summary>
    /// <returns>成员信息列表。</returns>
    public abstract List<MemberInfo> GetMembers();

    /// <summary>
    /// 返回此类型上声明的所有属性（ICustomAttributeProvider 实现，覆写 MemberInfo）。
    /// </summary>
    /// <returns>属性数据列表。</returns>
    public override abstract List<CustomAttributeData> GetCustomAttributes();

    /// <summary>
    /// 判断此类型是否声明了指定类型的属性（ICustomAttributeProvider 实现，覆写 MemberInfo）。
    /// </summary>
    /// <param name="attributeType">属性类型。</param>
    /// <returns>声明返回 true；否则 false。</returns>
    public override abstract bool IsDefined(Type attributeType);

    // ---- RFC 018 §4.2.4：本类型自身声明的成员（不含继承）----
    //
    // 单一惯用原则：原 TypeInfo 抽象类已合并入 Type（C# System.Type 现代版本已
    // 包含 Declared* 成员）。RuntimeType 直接继承 Type，无需中间抽象层。

    /// <summary>本类型自身声明的方法（不含继承）。</summary>
    public abstract List<MethodInfo> DeclaredMethods { get; }

    /// <summary>本类型自身声明的字段（不含继承）。</summary>
    public abstract List<FieldInfo> DeclaredFields { get; }

    /// <summary>本类型自身声明的属性（不含继承）。</summary>
    public abstract List<PropertyInfo> DeclaredProperties { get; }

    /// <summary>本类型自身声明的事件（不含继承）。</summary>
    public abstract List<EventInfo> DeclaredEvents { get; }

    /// <summary>本类型自身声明的构造函数。</summary>
    public abstract List<ConstructorInfo> DeclaredConstructors { get; }

    /// <summary>本类型自身声明的所有成员。</summary>
    public abstract List<MemberInfo> DeclaredMembers { get; }

    /// <summary>本类型自身声明的嵌套类型。</summary>
    public abstract List<Type> DeclaredNestedTypes { get; }

    /// <summary>返回此 Type 的 Type 视图（C# AsType()，Arc 直接返回 this）。</summary>
    /// <returns>当前 Type 实例。</returns>
    public Type AsType() { return this; }
}
