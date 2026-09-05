// InjectAttribute —— DI 自动注册标记（RFC 023 §1.7.1）：显式声明，默认 Scoped。
// 归属：DI 框架库（Arc.DI）。自动注册是 DI 容器能力，非 Web 专属，故置于 DI 契约面。
namespace Arc.DI;
using Arc;
using Arc.Collections;
using Arc.Reflection;

/// <summary>
/// 标记类自动注册进 DI（编译器跨包扫描合成注册进入口）。仅显式标记者注册
/// （显式 > 隐式，非全量盲扫）。
///
/// 字段：
///   - <see cref="Lifetime"/>：生命周期，默认 <see cref="ServiceLifetime.Scoped"/>。
///   - <see cref="ServiceType"/>：服务注册键（Type）；null 则为类型本身（自注册），
///     设置键即可将实现类注册到指定服务接口（如 <c>[Inject(typeof(IService))]</c>）。
///   - <see cref="ServiceKey"/>：命名服务键（对标 .NET keyed service）；空串表示无 key。
///
/// 多注册（一个实现类注册到多个服务接口）：附加多个 <c>[Inject]</c> 标记达成
/// （<c>AllowMultiple = true</c>），每个标记独立成一条注册——单一惯用法，
/// 不设多键聚合字段。
///
/// 泛型形态 <see cref="InjectAttribute{T}"/> 提供 <c>[Inject&lt;IService&gt;]</c> 便捷写法。
/// 注：P3 编译器里程碑实现跨包扫描合成注册进入口，当前仅契约定义。
/// </summary>
[AttributeUsage(AttributeTargets.Class, AllowMultiple = true)]
public class InjectAttribute : Attribute {
    /// <summary>生命周期，默认 Scoped。</summary>
    public ServiceLifetime Lifetime;

    /// <summary>服务注册键（Type）；null 则为类型本身（自注册）。</summary>
    public Type? ServiceType;

    /// <summary>命名服务键（对标 .NET keyed service）；空串表示无 key。</summary>
    public string ServiceKey;

    // [Inject] / [Inject(ServiceLifetime.X)]
    public InjectAttribute(ServiceLifetime lifetime = ServiceLifetime.Scoped,
                           string serviceKey = "") {
        this.Lifetime = lifetime;
        this.ServiceType = null;
        this.ServiceKey = serviceKey != null ? serviceKey : "";
    }

    // [Inject(typeof(IService))] / [Inject(typeof(IService), ServiceLifetime.X)]
    public InjectAttribute(Type? serviceType, ServiceLifetime lifetime = ServiceLifetime.Scoped,
                           string serviceKey = "") {
        this.Lifetime = lifetime;
        this.ServiceType = serviceType;
        this.ServiceKey = serviceKey != null ? serviceKey : "";
    }
}

/// <summary>
/// 泛型形态：T 为服务注册键（接口），等价于 <c>[Inject(typeof(T))]</c>。
/// 如 <c>[Inject&lt;IUserService&gt;]</c> 将实现类注册为 IUserService（Scoped）。
/// </summary>
[AttributeUsage(AttributeTargets.Class)]
public class InjectAttribute<T> : InjectAttribute where T : class {
    // 注：统一 `: base()`（空实参）+ 直接写字段，而非 `: base(lifetime)`——
    // 泛型子类对非空 base 实参的构造器绑定当前受限；此处语义等价且更简单。
    // base() 已把 ServiceKey 置空串、ServiceTypes 置空列表，故无键/单注册时无需处理。
    public InjectAttribute() : base() {
        this.ServiceType = typeof(T);
    }

    public InjectAttribute(ServiceLifetime lifetime) : base() {
        this.Lifetime = lifetime;
        this.ServiceType = typeof(T);
    }

    public InjectAttribute(ServiceLifetime lifetime, string serviceKey) : base() {
        this.Lifetime = lifetime;
        this.ServiceType = typeof(T);
        this.ServiceKey = serviceKey != null ? serviceKey : "";
    }
}
