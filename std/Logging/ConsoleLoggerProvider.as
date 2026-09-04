// ConsoleLoggerProvider —— 控制台日志提供程序（内置，输出到 Arc.Console）。
namespace Arc.Logging;

/// <summary>
/// 控制台日志提供程序——为每个类别创建 <see cref="ConsoleLogger"/>，将日志写入
/// <c>Arc.Console</c>（Error/Critical 走 stderr，其余走 stdout，并按级别着色）。
/// </summary>
public class ConsoleLoggerProvider : ILoggerProvider {
    public ConsoleLoggerProvider() { }

    public ILogger CreateLogger(string categoryName) {
        return new ConsoleLogger(categoryName);
    }

    public void Dispose() { }
}
