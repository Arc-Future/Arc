// ILoggerProvider —— 日志提供程序接口（对齐 .NET Microsoft.Extensions.Logging.ILoggerProvider）。
namespace Arc.Logging;

/// <summary>
/// 日志提供程序——为指定类别创建 <see cref="ILogger"/> 实例并负责实际输出
/// （如控制台、文件、远程采集等）。一个工厂可注册多个 Provider。
/// </summary>
public interface ILoggerProvider : IDisposable {
    /// <summary>为指定类别创建日志记录器。</summary>
    /// <param name="categoryName">日志类别名（通常为 <c>typeof(T).FullName</c>）。</param>
    ILogger CreateLogger(string categoryName);
}
