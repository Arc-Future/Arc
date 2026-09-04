// ILogger —— 日志记录器核心接口（对齐 .NET Microsoft.Extensions.Logging.ILogger）。
namespace Arc.Logging;

/// <summary>
/// 日志记录器抽象——应用程序通过它写入日志。
///
/// 实现约定：
///   - <see cref="Log"/> 接收"消息模板"（含 <c>{Name}</c> 占位符）与参数，
///     调用方（分发 Logger）将其格式化为最终文本后逐 Provider 下发；
///   - <see cref="IsEnabled"/> 用于在构造消息前快速判断是否值得格式化，避免无谓开销；
///   - <see cref="BeginScope"/> 用于界定一组日志的上下文，可返回 null（表示无作用域）。
///
/// 便利方法（LogInformation/LogWarning/...）见 <c>LoggerExtensions</c>。
/// </summary>
public interface ILogger {
    /// <summary>
    /// 写入一条日志。
    /// </summary>
    /// <param name="logLevel">日志级别。</param>
    /// <param name="eventId">事件标识。</param>
    /// <param name="exception">关联异常（可为 null）。</param>
    /// <param name="message">消息模板（含 <c>{Name}</c> 占位符，占位符按出现顺序绑定 args）。</param>
    /// <param name="args">模板参数（零堆 <c>params ReadOnlySpan&lt;string&gt;</c>，值以字符串传入）。</param>
    void Log(LogLevel logLevel, EventId eventId, Exception? exception, string message, params ReadOnlySpan<string> args);

    /// <summary>当前级别是否会被记录。</summary>
    bool IsEnabled(LogLevel logLevel);

    /// <summary>开启日志作用域（可为 null 表示无作用域）。</summary>
    IDisposable? BeginScope(object? state);
}
