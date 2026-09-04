// ServiceCollectionExtensions —— 日志 DI 集成（对齐 .NET LoggingServiceCollectionExtensions）。
namespace Arc.Logging;

using Arc.DI;

/// <summary>
/// 日志服务注册扩展——把 <see cref="ILoggerFactory"/> 与 <see cref="ILogger"/> 接入
/// <c>Arc.DI</c> 服务容器（对齐 .NET <c>services.AddLogging()</c>）。
///
/// 注册项：
///   - <c>ILoggerFactory</c>（Singleton）——构建后的 <see cref="LoggerFactory"/>；
///   - <c>ILogger</c>（Singleton）——类别为 "Default" 的根日志记录器。
///
/// 类型化日志记录器经 <c>loggerFactory.CreateLogger&lt;T&gt;()</c> 获取。
/// 注：Arc.DI 容器为封闭泛型 + 零反射模型，不提供开放泛型 <c>ILogger&lt;T&gt;</c>
/// 自动解析；按类别建 Logger 统一走 <c>CreateLogger</c>。
/// </summary>
public static class ServiceCollectionExtensions {
    /// <summary>接入日志服务，并默认注册内置控制台提供程序。</summary>
    public static IServiceCollection AddLogging(this IServiceCollection services) {
        var factory = new LoggerFactory();
        factory.AddProvider(new ConsoleLoggerProvider());
        return ServiceCollectionExtensions._AddLogging(services, factory);
    }

    /// <summary>接入日志服务，并注册指定的日志提供程序集合。</summary>
    public static IServiceCollection AddLogging(this IServiceCollection services, ILoggerProvider[] providers) {
        var factory = new LoggerFactory();
        if (providers != null) {
            int i = 0;
            while (i < providers.Length) {
                factory.AddProvider(providers[i]);
                i = i + 1;
            }
        }
        return ServiceCollectionExtensions._AddLogging(services, factory);
    }

    private static IServiceCollection _AddLogging(IServiceCollection services, LoggerFactory factory) {
        services.AddSingleton<ILoggerFactory>(factory);
        services.AddSingleton<ILogger>(factory.CreateLogger("Default"));
        return services;
    }
}
