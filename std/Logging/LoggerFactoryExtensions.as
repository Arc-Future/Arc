// LoggerFactoryExtensions —— ILoggerFactory 便利扩展（对齐 .NET LoggerFactoryExtensions）。
namespace Arc.Logging;

/// <summary>
/// <see cref="ILoggerFactory"/> 的便利扩展：类型化建 Logger、注册 Provider、设置最低级别。
/// </summary>
public static class LoggerFactoryExtensions {
    /// <summary>按 <c>typeof(T).FullName</c> 创建类型化日志记录器。</summary>
    public static ILogger CreateLogger<T>(this ILoggerFactory factory) {
        return factory.CreateLogger(typeof(T).FullName);
    }

    /// <summary>注册日志提供程序（返回工厂以支持链式调用）。</summary>
    public static ILoggerFactory AddProvider(this ILoggerFactory factory, ILoggerProvider provider) {
        factory.AddProvider(provider);
        return factory;
    }

    /// <summary>注册内置控制台提供程序。</summary>
    public static ILoggerFactory AddConsole(this ILoggerFactory factory) {
        factory.AddProvider(new ConsoleLoggerProvider());
        return factory;
    }

    /// <summary>设置工厂全局最低输出级别。</summary>
    public static ILoggerFactory SetMinimumLevel(this ILoggerFactory factory, LogLevel minLevel) {
        // Arc `is` 运行时判定依赖对象 vtable（has_vtable），而 LoggerFactory
        // 无虚方法（has_vtable=false）无法判定，故直接用已验证的接口→具体类
        // 转型（UnboxIface），对齐 a2c 通路。工厂恒由本基础设施创建。
        var lf = (LoggerFactory)factory;
        lf.MinimumLevel = minLevel;
        return factory;
    }

    /// <summary>清空全部 Provider。</summary>
    public static ILoggerFactory ClearProviders(this ILoggerFactory factory) {
        var lf = (LoggerFactory)factory;
        lf.ClearProviders();
        return factory;
    }
}
