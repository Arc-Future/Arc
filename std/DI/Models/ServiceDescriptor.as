// ServiceDescriptor —— 服务描述符（RFC 023 M1，对标 .NET ServiceDescriptor）。
namespace Arc.DI;

using Arc.Reflection;

/// <summary>
/// 服务描述符——一条注册记录，含 keyed 字段、双构造模式（对标 MEDI）。
///
/// 方式 1：实现类型构造——codegen 编译期生成类型化工厂委托，
///         在 emit_new 中构造后直接写入 Factory 字段。ServiceProvider 运行时
///         统一调用 `Factory(sp)` 创建实例（零反射、零 if-branch 分派）。
/// 方式 2：工厂委托构造——用户直接提供工厂委托，无需 codegen 参与。
///
/// 重载形态为对齐 .NET 的 `object? key = null` 默认参数语义（默认参数已由编译器
/// 支持，见 AssemblyLoadContext.LoadByName；此处保持既有重载面，不引入双轨）。
///
/// ServiceDescriptor 为 class（含引用类型字段 Factory/Key，struct 值语义与
/// 容器共享可变状态不匹配；struct 构造/属性虽已支持，此处维持 class 设计）。
/// </summary>
public class ServiceDescriptor {
    /// <summary>服务类型（通常是接口）。</summary>
    public Type ServiceType { get; }

    public Type? ImplementationType { get; }

    /// <summary>工厂委托——统一入口。
    /// 方式 1 由 codegen 在构造后直接写入闭包（内联字段 store）；
    /// 方式 2 由构造函数直接赋值。</summary>
    public Func<IServiceProvider, object>? Factory { get; set; }

    /// <summary>生命周期。</summary>
    public ServiceLifetime Lifetime { get; }

    /// <summary>keyed 服务的 key（null 表示默认注册）。</summary>
    public object? Key { get; }

    // 方式 1：实现类型构造（无 key，默认注册）
    // 注意：Factory 字段不在此赋值——codegen 在 emit_new 中构造
    // closed-after-construct 后注入（见 crates/codegen/src/llvm_ir/emit_call.rs emit_new）。
    public ServiceDescriptor(Type service, Type? impl, ServiceLifetime lifetime) {
        ServiceType = service;
        ImplementationType = impl;
        Factory = null;
        Lifetime = lifetime;
        Key = null;
    }

    // 方式 1：实现类型构造（带 key，keyed 注册）
    public ServiceDescriptor(Type service, Type? impl, ServiceLifetime lifetime, object? key) {
        ServiceType = service;
        ImplementationType = impl;
        Factory = null;
        Lifetime = lifetime;
        Key = key;
    }

    // 方式 2：工厂委托构造（无 key，默认注册）
    public ServiceDescriptor(Type service, Func<IServiceProvider, object> factory, ServiceLifetime lifetime) {
        ServiceType = service;
        ImplementationType = null;
        Factory = factory;
        Lifetime = lifetime;
        Key = null;
    }

    // 方式 2：工厂委托构造（带 key，keyed 注册）
    public ServiceDescriptor(Type service, Func<IServiceProvider, object> factory, ServiceLifetime lifetime, object? key) {
        ServiceType = service;
        ImplementationType = null;
        Factory = factory;
        Lifetime = lifetime;
        Key = key;
    }
}
