// ConsoleLogger —— 控制台日志记录器（内部实现，输出到 Arc.Console）。
namespace Arc.Logging;

using Arc;
using Arc.Text;

/// <summary>
/// 控制台日志记录器——接收已由分发 Logger 格式化好的消息文本。
/// 输出格式：<c>yyyy-MM-dd HH:mm:ss [级别] [类别] 消息</c>，按级别着色；
/// Error/Critical 输出到 stderr 并附带异常栈。
/// </summary>
internal class ConsoleLogger : ILogger {
    private string _category;

    public ConsoleLogger(string category) {
        _category = category;
    }

    public bool IsEnabled(LogLevel logLevel) {
        return logLevel != LogLevel.None;
    }

    public void Log(LogLevel logLevel, EventId eventId, Exception? exception, string message, params ReadOnlySpan<string> args) {
        // 已由分发 Logger 前置格式化，args 为空
        ConsoleLogger._WriteLine(_category, logLevel, eventId, exception, message);
    }

    public IDisposable? BeginScope(object? state) { return null; }

    private static void _WriteLine(string category, LogLevel logLevel, EventId eventId, Exception? exception, string message) {
        var sb = new StringBuilder();
        var now = DateTime.Now;
        sb.Append(now.Year); sb.Append("-");
        sb.Append(ConsoleLogger._Pad2(now.Month)); sb.Append("-"); sb.Append(ConsoleLogger._Pad2(now.Day));
        sb.Append(" "); sb.Append(ConsoleLogger._Pad2(now.Hour)); sb.Append(":");
        sb.Append(ConsoleLogger._Pad2(now.Minute)); sb.Append(":"); sb.Append(ConsoleLogger._Pad2(now.Second));
        sb.Append(" ");
        sb.Append(ConsoleLogger._LevelName(logLevel));
        sb.Append(" [");
        sb.Append(category);
        sb.Append("] ");
        sb.Append(message);
        if (!eventId.IsDefault) {
            sb.Append(" ("); sb.Append(eventId.Id); sb.Append(")");
        }

        bool toError = (logLevel == LogLevel.Error || logLevel == LogLevel.Critical);
        int color = ConsoleLogger._LevelColor(logLevel);
        if (color >= 0) { Console.SetForegroundColor(color); }
        if (toError) { Console.ErrorWriteLine(sb.ToString()); }
        else { Console.WriteLine(sb.ToString()); }
        if (color >= 0) { Console.ResetColor(); }

        if (exception != null) {
            string ex = exception.ToString();
            if (toError) { Console.ErrorWriteLine(ex); }
            else { Console.WriteLine(ex); }
        }
    }

    /// <summary>两位十进制（不足补零）。</summary>
    private static string _Pad2(int v) {
        if (v < 10) { return "0" + v; }
        return "" + v;
    }

    /// <summary>级别短名（对齐 .NET 控制台日志：dbug/info/warn/fail/crit/trce）。</summary>
    private static string _LevelName(LogLevel level) {
        if (level == LogLevel.Trace) { return "trce"; }
        if (level == LogLevel.Debug) { return "dbug"; }
        if (level == LogLevel.Information) { return "info"; }
        if (level == LogLevel.Warning) { return "warn"; }
        if (level == LogLevel.Error) { return "fail"; }
        if (level == LogLevel.Critical) { return "crit"; }
        return "none";
    }

    /// <summary>级别对应控制台前景色；Information 返回 -1（使用默认色）。</summary>
    private static int _LevelColor(LogLevel level) {
        if (level == LogLevel.Trace) { return (int)ConsoleColor.Gray; }
        if (level == LogLevel.Debug) { return (int)ConsoleColor.Gray; }
        if (level == LogLevel.Information) { return -1; }
        if (level == LogLevel.Warning) { return (int)ConsoleColor.Yellow; }
        if (level == LogLevel.Error) { return (int)ConsoleColor.Red; }
        if (level == LogLevel.Critical) { return (int)ConsoleColor.Red; }
        return -1;
    }
}
