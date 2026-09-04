// ILoggerFactory —— 日志工厂接口（对齐 .NET Microsoft.Extensions.Logging.ILoggerFactory）。
namespace Arc.Logging;

/// <summary>
/// 日志工厂——按类别创建 <see cref="ILogger"/>，并持有 Provider 注册表。
/// 应用程序通常注入本接口，再经 <c>CreateLogger&lt;T&gt;()</c>（见 <c>LoggerFactoryExtensions</c>）
/// 或 <c>CreateLogger(category)</c> 获取类型化日志记录器。
/// </summary>
public interface ILoggerFactory : IDisposable {
    /// <summary>按类别名创建日志记录器（同类别返回已缓存实例）。</summary>
    ILogger CreateLogger(string categoryName);

    /// <summary>注册一个日志提供程序。</summary>
    void AddProvider(ILoggerProvider provider);
}
