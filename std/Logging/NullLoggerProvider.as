// NullLoggerProvider —— 空日志提供程序（对齐 .NET NullLoggerProvider）。
namespace Arc.Logging;

/// <summary>
/// 空日志提供程序——始终返回 <see cref="NullLogger"/>，用于默认回退。
/// </summary>
public class NullLoggerProvider : ILoggerProvider {
    /// <summary>全局唯一空提供程序实例（static readonly 惰性：首触构造一次、线程安全）。</summary>
    public static readonly NullLoggerProvider Instance = new NullLoggerProvider();

    public NullLoggerProvider() { }

    public ILogger CreateLogger(string categoryName) { return NullLogger.Instance; }

    public void Dispose() { }
}
