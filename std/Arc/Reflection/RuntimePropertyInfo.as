// RFC 018 M3+: PropertyInfo 运行时具体实现——封装 RtPropertyInfo* 指针。
//
// GetProperties()/DeclaredProperties 返回本类实例；Name 由 codegen 从
// RtPropertyInfo.name（offset 0）拦截读取。永久剔除 GetValue/SetValue。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// PropertyInfo 的运行时具体实现——封装 RtPropertyInfo* 指针。
///
/// **internal 实现细节**：`RuntimePropertyInfo` 是 `PropertyInfo` 抽象类的内部
/// 具体实现，仅供标准库反射层（GetProperties/DeclaredProperties 枚举）使用，
/// 不属公共 API 面。用户通过 `PropertyInfo` 抽象基类访问；对外屏蔽本类，
/// 禁止直接实例化。
/// </summary>
internal class RuntimePropertyInfo : PropertyInfo {
    /// <summary>RtPropertyInfo* 指针（i64 句柄）。</summary>
    private long _propertyInfoHandle;

    /// <summary>由 codegen 在 GetProperties 枚举处发射。</summary>
    internal RuntimePropertyInfo(long propertyInfoHandle) {
        _propertyInfoHandle = propertyInfoHandle;
    }

    /// <summary>属性名——codegen 拦截，load RtPropertyInfo.name。</summary>
    public override string Name {
        get { return ""; }
    }

    /// <summary>属性类型——codegen 拦截，load RtPropertyInfo.property_type。显式死代码体（禁自动属性打断拦截）。</summary>
    public override Type PropertyType {
        get { return null; }
    }

    /// <summary>属性上自定义属性——本切片空列表。</summary>
    public override List<CustomAttributeData> GetCustomAttributes() {
        return new List<CustomAttributeData>();
    }

    public override bool IsDefined(Type attributeType) {
        return false;
    }
}
