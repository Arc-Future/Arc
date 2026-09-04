// ServiceLifetime —— 服务生命周期枚举（RFC 023 M0，对标 .NET ServiceLifetime）。
namespace Arc.DI;

/// <summary>
/// 服务生命周期（对标 .NET Microsoft.Extensions.DependencyInjection.ServiceLifetime）。
///
/// - Singleton：单例，整个根容器共享一个实例。
/// - Scoped：作用域，同一 IServiceScope 内共享一个实例。
/// - Transient：瞬态，每次解析返回新实例。
/// </summary>
public enum ServiceLifetime {
    Singleton,
    Scoped,
    Transient,
}
