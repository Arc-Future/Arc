// ServiceCollectionExtensions —— 服务注册便利 API（RFC 023 M1，对标 .NET MEDI ServiceCollectionExtensions）。
// v0.8 修订 (2026-07-21):
//   - P1: 补全实例注册 AddSingleton(TService instance)
//   - P1: 补全工厂委托重载 AddTransient<TService>(Func<IServiceProvider, TService>)
//   - P1: 补全 keyed 工厂委托重载
namespace Arc.DI;

/// <summary>
/// 服务集合扩展方法（对标 .NET Microsoft.Extensions.DependencyInjection.ServiceCollectionExtensions）。
/// </summary>
public static class ServiceCollectionExtensions {
    // ── 方法1: 实现类型构造 (codegen 生成工厂) ──

    public static IServiceCollection AddTransient<TService, TImpl>(this IServiceCollection services)
        where TImpl : TService {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TImpl), ServiceLifetime.Transient));
    }

    public static IServiceCollection AddScoped<TService, TImpl>(this IServiceCollection services)
        where TImpl : TService {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TImpl), ServiceLifetime.Scoped));
    }

    public static IServiceCollection AddSingleton<TService, TImpl>(this IServiceCollection services)
        where TImpl : TService {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TImpl), ServiceLifetime.Singleton));
    }

    public static IServiceCollection AddKeyedTransient<TService, TImpl>(this IServiceCollection services, object? key)
        where TImpl : TService {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TImpl), ServiceLifetime.Transient, key));
    }

    public static IServiceCollection AddKeyedScoped<TService, TImpl>(this IServiceCollection services, object? key)
        where TImpl : TService {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TImpl), ServiceLifetime.Scoped, key));
    }

    public static IServiceCollection AddKeyedSingleton<TService, TImpl>(this IServiceCollection services, object? key)
        where TImpl : TService {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TImpl), ServiceLifetime.Singleton, key));
    }

    // ── 方法1b: 自实现类型构造 (TService == TImpl，对标 MEDI AddTransient<T>() / AddScoped<T>() / AddSingleton<T>()) ──

    public static IServiceCollection AddTransient<TService>(this IServiceCollection services) {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TService), ServiceLifetime.Transient));
    }

    public static IServiceCollection AddScoped<TService>(this IServiceCollection services) {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TService), ServiceLifetime.Scoped));
    }

    public static IServiceCollection AddSingleton<TService>(this IServiceCollection services) {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TService), ServiceLifetime.Singleton));
    }

    public static IServiceCollection AddKeyedTransient<TService>(this IServiceCollection services, object? key) {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TService), ServiceLifetime.Transient, key));
    }

    public static IServiceCollection AddKeyedScoped<TService>(this IServiceCollection services, object? key) {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TService), ServiceLifetime.Scoped, key));
    }

    public static IServiceCollection AddKeyedSingleton<TService>(this IServiceCollection services, object? key) {
        return services.Add(new ServiceDescriptor(typeof(TService), typeof(TService), ServiceLifetime.Singleton, key));
    }

    // ── 方法2: 工厂委托构造 (用户提供工厂，无需 codegen 生成) ──

    /// <summary>添加 Transient 服务 —— 工厂委托每次解析时调用。</summary>
    public static IServiceCollection AddTransient<TService>(this IServiceCollection services, Func<IServiceProvider, TService> factory) {
        return services.Add(new ServiceDescriptor(typeof(TService), (sp: IServiceProvider) => (object)factory(sp), ServiceLifetime.Transient));
    }

    /// <summary>添加 Scoped 服务 —— 工厂委托在同一作用域内缓存。</summary>
    public static IServiceCollection AddScoped<TService>(this IServiceCollection services, Func<IServiceProvider, TService> factory) {
        return services.Add(new ServiceDescriptor(typeof(TService), (sp: IServiceProvider) => (object)factory(sp), ServiceLifetime.Scoped));
    }

    /// <summary>添加 Singleton 服务 —— 工厂委托全容器共享单例。</summary>
    public static IServiceCollection AddSingleton<TService>(this IServiceCollection services, Func<IServiceProvider, TService> factory) {
        return services.Add(new ServiceDescriptor(typeof(TService), (sp: IServiceProvider) => (object)factory(sp), ServiceLifetime.Singleton));
    }

    /// <summary>添加 Singleton 实例 —— 直接注入预构造实例 (对齐 MEDI)。</summary>
    public static IServiceCollection AddSingleton<TService>(this IServiceCollection services, TService instance) {
        return services.Add(new ServiceDescriptor(typeof(TService), (sp: IServiceProvider) => (object)instance, ServiceLifetime.Singleton));
    }

    // ── keyed 工厂委托重载 ──

    public static IServiceCollection AddKeyedTransient<TService>(this IServiceCollection services, object? key, Func<IServiceProvider, TService> factory) {
        return services.Add(new ServiceDescriptor(typeof(TService), (sp: IServiceProvider) => (object)factory(sp), ServiceLifetime.Transient, key));
    }

    public static IServiceCollection AddKeyedScoped<TService>(this IServiceCollection services, object? key, Func<IServiceProvider, TService> factory) {
        return services.Add(new ServiceDescriptor(typeof(TService), (sp: IServiceProvider) => (object)factory(sp), ServiceLifetime.Scoped, key));
    }

    public static IServiceCollection AddKeyedSingleton<TService>(this IServiceCollection services, object? key, Func<IServiceProvider, TService> factory) {
        return services.Add(new ServiceDescriptor(typeof(TService), (sp: IServiceProvider) => (object)factory(sp), ServiceLifetime.Singleton, key));
    }

    public static IServiceCollection AddKeyedSingleton<TService>(this IServiceCollection services, object? key, TService instance) {
        return services.Add(new ServiceDescriptor(typeof(TService), (sp: IServiceProvider) => (object)instance, ServiceLifetime.Singleton, key));
    }
}
