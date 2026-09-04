// NullLogger —— 空日志记录器（对齐 .NET NullLogger，无任何输出）。
namespace Arc.Logging;

/// <summary>
/// 空日志记录器——不执行任何输出，用于默认回退或显式禁用日志。
/// </summary>
public class NullLogger : ILogger {
    /// <summary>全局唯一空日志记录器实例（static readonly 惰性：首触构造一次、线程安全）。</summary>
    public static readonly NullLogger Instance = new NullLogger();

    public NullLogger() { }

    public bool IsEnabled(LogLevel logLevel) { return false; }

    public void Log(LogLevel logLevel, EventId eventId, Exception? exception, string message, params ReadOnlySpan<string> args) {
        // 空操作
    }

    public IDisposable? BeginScope(object? state) { return null; }
}
