// RFC 018 M3+: MethodInfo 运行时具体实现——封装 RtMethodInfo* 指针。
//
// GetMethods()/DeclaredMethods 返回本类实例；Name 由 codegen 从
// RtMethodInfo.name（offset 0）拦截读取。永久剔除 Invoke()。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// MethodInfo 的运行时具体实现——封装 RtMethodInfo* 指针。
///
/// **internal 实现细节**：`RuntimeMethodInfo` 是 `MethodInfo` 抽象类的内部具体
/// 实现，仅供标准库反射层（GetMethods/DeclaredMethods 枚举）使用，不属公共
/// API 面。用户通过 `MethodInfo` 抽象基类访问；对外屏蔽本类，禁止直接实例化。
/// </summary>
internal class RuntimeMethodInfo : MethodInfo {
    /// <summary>RtMethodInfo* 指针（i64 句柄）。</summary>
    private long _methodInfoHandle;

    /// <summary>由 codegen 在 GetMethods 枚举处发射。</summary>
    internal RuntimeMethodInfo(long methodInfoHandle) {
        _methodInfoHandle = methodInfoHandle;
    }

    /// <summary>方法名——codegen 拦截，load RtMethodInfo.name。</summary>
    public override string Name {
        get { return ""; }
    }

    /// <summary>返回类型——codegen 拦截，load RtMethodInfo.return_type。显式死代码体（禁自动属性打断拦截）。</summary>
    public override Type ReturnType {
        get { return null; }
    }

    /// <summary>形参列表——M3+ 本切片返回空列表。</summary>
    public override List<ParameterInfo> GetParameters() {
        return new List<ParameterInfo>();
    }

    /// <summary>方法上自定义属性——本切片空列表（attributes 发射属后续）。</summary>
    public override List<CustomAttributeData> GetCustomAttributes() {
        return new List<CustomAttributeData>();
    }

    public override bool IsDefined(Type attributeType) {
        return false;
    }
}
