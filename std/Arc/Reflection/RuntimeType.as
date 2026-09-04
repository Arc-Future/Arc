// RFC 018 §7 M2: Type 的运行时具体实现——封装 RtTypeInfo* 指针。
//
// RuntimeType 是 Type 抽象类的具体子类，每个 typeof(T) 在 codegen 阶段
// 发射为指向 @.typeinfo.{T} 全局常量的 RuntimeType 实例（或直接返回
// RtTypeInfo* 指针，由 codegen 决定具体形态）。
//
// **M3+ 最小面（本切片）实施状态**：
//   - typeof(T) 直接发射 RuntimeType 实例；_typeInfoHandle 为 `long`（i64），
//     容纳 64 位 RtTypeInfo*（ptrtoint 转换结果）。
//   - TypeId / Name / FullName / Kind / BaseType 由 codegen 从 RtTypeInfo 拦截。
//   - GetMethods / GetFields / GetProperties / Declared* 由 codegen 枚举
//     declared_* 数组，元素为 RuntimeMethodInfo / RuntimeFieldInfo /
//     RuntimePropertyInfo（仅 Name）。
//   - 自定义属性 / 成员签名细节（ReturnType/FieldType/PropertyType/继承合并）
//     未实现：相关成员一律抛 NotImplementedException（2026-08-30 P1 诚实化，
//     不再返回空 List/false 假数据）。
//
// **物理边界**（RFC 018 §3.3）：RuntimeType 仅持有 RtTypeInfo* 指针（long 句柄），
// 不持有任何函数指针/字段偏移。所有 GetXxx() 方法通过 ABI 查询函数读取 rodata
// 全局常量，无法 Invoke/GetValue/SetValue。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// Type 的运行时具体实现——封装 RtTypeInfo* 指针。
///
/// **internal 实现细节**：`RuntimeType` 是 `Type` 抽象类的内部具体实现，仅供
/// 标准库反射层使用，不属公共 API 面。用户通过 `Type` 抽象基类访问；对外屏蔽
/// 本类，禁止直接实例化/引用（标准库访问权限控制）。
///
/// 每个 typeof(T) 在 codegen 阶段发射为指向 @.typeinfo.{T} 全局常量的
/// RuntimeType 实例。本类仅持有 RtTypeInfo* 指针（以 long 形式存储），
/// 所有元数据查询通过 ABI 函数（rt_type_by_id / rt_type_find_method 等）读取。
///
/// **物理边界**（RFC 018 §3.3）：不持有函数指针/字段偏移，从 ABI 物理层面
/// 阻绝 Invoke/GetValue/SetValue。
///
/// **命名规范化（2026-07-24）**：原 RuntimeTypeInfo 已重命名为 RuntimeType，
/// 杜绝 C# 历史中 TypeInfo/Type 并存的兼容冗余问题。RuntimeType 直接继承 Type，
/// 无中间抽象层——单一惯用原则。
/// </summary>
internal class RuntimeType : Type {
    /// <summary>
    /// RtTypeInfo* 指针（以 long 形式存储，i64 容纳 64 位 ptr）。
    /// 由 codegen 在 `typeof(T)` 发射处填充，
    /// 值为 `ptrtoint(ptr @.typeinfo.{T} to i64)`。
    /// </summary>
    private long _typeInfoHandle;

    /// <summary>受保护构造函数——由 codegen 在 typeof(T) 处发射时调用。</summary>
    /// <param name="typeInfoHandle">RtTypeInfo* 指针（long 句柄）。</param>
    internal RuntimeType(long typeInfoHandle) {
        _typeInfoHandle = typeInfoHandle;
    }

    /// <summary>类型唯一标识（FNV-1a 32 位哈希）。</summary>
    public override int TypeId {
        get {
            // M2 step 2: codegen 拦截此 getter，发射
            // `%ptr = inttoptr i64 %handle to ptr`
            // `%id = load i32, ptr %ptr`（RtTypeInfo.type_id 在 offset 0）。
            // 用户的 stub 返回值 0 不会被使用。
            return 0;
        }
    }

    /// <summary>类型完整限定名（含命名空间前缀，如 <c>MyApp.Services.Greeter</c>）。</summary>
    public override string FullName {
        get {
            // codegen 拦截：load RtTypeInfo.full_name（RFC 018 M2：Ns.Type 点分限定名）
            return "";
        }
    }

    /// <summary>类型名（不含命名空间前缀）。</summary>
    public override string Name {
        get {
            // codegen 拦截：load RtTypeInfo.name（类型键短名）
            return "";
        }
    }

    /// <summary>类型分类枚举。</summary>
    public override TypeKind Kind {
        get {
            // codegen 拦截：load RtTypeInfo.kind
            return TypeKind.Class;
        }
    }

    /// <summary>是否为 class（引用类型）。</summary>
    public override bool IsClass {
        get { return this.Kind == TypeKind.Class; }
    }

    /// <summary>是否为 struct（值类型，非 enum）。</summary>
    public override bool IsStruct {
        get { return this.Kind == TypeKind.Struct; }
    }

    /// <summary>是否为 interface。</summary>
    public override bool IsInterface {
        get { return this.Kind == TypeKind.Interface; }
    }

    /// <summary>是否为 enum。</summary>
    public override bool IsEnum {
        get { return this.Kind == TypeKind.Enum; }
    }

    /// <summary>是否为基元类型。</summary>
    public override bool IsPrimitive {
        get { return this.Kind == TypeKind.Primitive; }
    }

    /// <summary>直接基类（struct/enum/object 返回 null；class 返回父类）。</summary>
    public override Type? BaseType {
        get {
            // codegen 拦截：load RtTypeInfo.parent → 新 RuntimeType 或 null
            return null;
        }
    }

    /// <summary>返回本类型声明的方法（M3+：仅 declared，不含继承合并）。</summary>
    public override List<MethodInfo> GetMethods() {
        // codegen 拦截：枚举 RtTypeInfo.declared_methods → RuntimeMethodInfo
        return new List<MethodInfo>();
    }

    /// <summary>返回本类型声明的字段（M3+：仅 declared，不含继承合并）。</summary>
    public override List<FieldInfo> GetFields() {
        // codegen 拦截：枚举 RtTypeInfo.declared_fields → RuntimeFieldInfo
        return new List<FieldInfo>();
    }

    /// <summary>返回本类型声明的属性（M3+：仅 declared，不含继承合并）。</summary>
    public override List<PropertyInfo> GetProperties() {
        // codegen 拦截：枚举 RtTypeInfo.declared_properties → RuntimePropertyInfo
        return new List<PropertyInfo>();
    }

    /// <summary>返回此类型上声明的所有事件（无元数据来源，未实现）。</summary>
    public override List<EventInfo> GetEvents() {
        throw new NotImplementedException("RuntimeType.GetEvents is not implemented (event metadata has no runtime source yet)");
    }

    /// <summary>返回此类型上声明的所有构造函数（无元数据来源，未实现）。</summary>
    public override List<ConstructorInfo> GetConstructors() {
        throw new NotImplementedException("RuntimeType.GetConstructors is not implemented (constructor metadata has no runtime source yet)");
    }

    /// <summary>返回此类型的所有成员（Methods/Fields/Properties 合并；GetMethods 含继承）。</summary>
    public override List<MemberInfo> GetMembers() {
        List<MemberInfo> members = new List<MemberInfo>();
        List<MethodInfo> methods = this.GetMethods();
        for (int i = 0; i < methods.Count; i++) { members.Add(methods[i]); }
        List<FieldInfo> fields = this.GetFields();
        for (int i = 0; i < fields.Count; i++) { members.Add(fields[i]); }
        List<PropertyInfo> properties = this.GetProperties();
        for (int i = 0; i < properties.Count; i++) { members.Add(properties[i]); }
        return members;
    }

    /// <summary>返回此类型上声明的所有属性数据（无元数据来源，未实现）。</summary>
    public override List<CustomAttributeData> GetCustomAttributes() {
        throw new NotImplementedException("RuntimeType.GetCustomAttributes is not implemented (custom attribute metadata has no runtime source yet)");
    }

    /// <summary>判断此类型是否声明了指定类型的属性（无元数据来源，未实现）。</summary>
    public override bool IsDefined(Type attributeType) {
        throw new NotImplementedException("RuntimeType.IsDefined is not implemented (custom attribute metadata has no runtime source yet)");
    }

    // ---- Declared*：M3+ GetMethods/GetFields 同源（仅 declared）----

    /// <summary>本类型自身声明的方法（不含继承）。</summary>
    public override List<MethodInfo> DeclaredMethods {
        // codegen 拦截：同 GetMethods
        get { return new List<MethodInfo>(); }
    }

    /// <summary>本类型自身声明的字段（不含继承）。</summary>
    public override List<FieldInfo> DeclaredFields {
        // codegen 拦截：同 GetFields
        get { return new List<FieldInfo>(); }
    }

    /// <summary>本类型自身声明的属性（不含继承）。</summary>
    public override List<PropertyInfo> DeclaredProperties {
        // codegen 拦截：同 GetProperties
        get { return new List<PropertyInfo>(); }
    }

    /// <summary>本类型自身声明的事件（无元数据来源，未实现）。</summary>
    public override List<EventInfo> DeclaredEvents {
        get { throw new NotImplementedException("RuntimeType.DeclaredEvents is not implemented (event metadata has no runtime source yet)"); }
    }

    /// <summary>本类型自身声明的构造函数（无元数据来源，未实现）。</summary>
    public override List<ConstructorInfo> DeclaredConstructors {
        get { throw new NotImplementedException("RuntimeType.DeclaredConstructors is not implemented (constructor metadata has no runtime source yet)"); }
    }

    /// <summary>本类型自身声明的所有成员（无元数据来源，未实现；用 GetMembers/DeclaredMethods/DeclaredFields/DeclaredProperties 组合替代）。</summary>
    public override List<MemberInfo> DeclaredMembers {
        get { throw new NotImplementedException("RuntimeType.DeclaredMembers is not implemented (use GetMembers/DeclaredMethods/DeclaredFields/DeclaredProperties instead)"); }
    }

    /// <summary>本类型自身声明的嵌套类型（无元数据来源，未实现）。</summary>
    public override List<Type> DeclaredNestedTypes {
        get { throw new NotImplementedException("RuntimeType.DeclaredNestedTypes is not implemented (nested type metadata has no runtime source yet)"); }
    }
}
