// RFC 018 M3+: FieldInfo 运行时具体实现——封装 RtFieldInfo* 指针。
//
// GetFields()/DeclaredFields 返回本类实例；Name 由 codegen 从
// RtFieldInfo.name（offset 0）拦截读取。永久剔除 GetValue/SetValue。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// FieldInfo 的运行时具体实现——封装 RtFieldInfo* 指针。
///
/// **internal 实现细节**：`RuntimeFieldInfo` 是 `FieldInfo` 抽象类的内部具体
/// 实现，仅供标准库反射层（GetFields/DeclaredFields 枚举）使用，不属公共
/// API 面。用户通过 `FieldInfo` 抽象基类访问；对外屏蔽本类，禁止直接实例化。
/// </summary>
internal class RuntimeFieldInfo : FieldInfo {
    /// <summary>RtFieldInfo* 指针（i64 句柄）。</summary>
    private long _fieldInfoHandle;

    /// <summary>由 codegen 在 GetFields 枚举处发射。</summary>
    internal RuntimeFieldInfo(long fieldInfoHandle) {
        _fieldInfoHandle = fieldInfoHandle;
    }

    /// <summary>字段名——codegen 拦截，load RtFieldInfo.name。</summary>
    public override string Name {
        get { return ""; }
    }

    /// <summary>字段类型——codegen 拦截，load RtFieldInfo.field_type。显式死代码体（禁自动属性打断拦截）。</summary>
    public override Type FieldType {
        get { return null; }
    }

    /// <summary>字段上自定义属性——本切片空列表。</summary>
    public override List<CustomAttributeData> GetCustomAttributes() {
        return new List<CustomAttributeData>();
    }

    public override bool IsDefined(Type attributeType) {
        return false;
    }
}
