// LoggerExtensions —— ILogger 便利记录方法（对齐 .NET LoggerExtensions）。
namespace Arc.Logging;

/// <summary>
/// <see cref="ILogger"/> 的级别便利扩展方法——省去显式构造 <see cref="EventId"/> 与异常参数。
/// 消息为结构化模板（<c>{Name}</c> 占位符按出现顺序绑定 args）。
/// </summary>
public static class LoggerExtensions {
    public static void LogTrace(this ILogger logger, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Trace, new EventId(0), null, message, args);
    }

    public static void LogDebug(this ILogger logger, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Debug, new EventId(0), null, message, args);
    }

    public static void LogInformation(this ILogger logger, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Information, new EventId(0), null, message, args);
    }

    public static void LogWarning(this ILogger logger, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Warning, new EventId(0), null, message, args);
    }

    public static void LogError(this ILogger logger, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Error, new EventId(0), null, message, args);
    }

    public static void LogCritical(this ILogger logger, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Critical, new EventId(0), null, message, args);
    }

    public static void LogError(this ILogger logger, Exception? exception, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Error, new EventId(0), exception, message, args);
    }

    public static void LogCritical(this ILogger logger, Exception? exception, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Critical, new EventId(0), exception, message, args);
    }

    public static void LogTrace(this ILogger logger, EventId eventId, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Trace, eventId, null, message, args);
    }

    public static void LogDebug(this ILogger logger, EventId eventId, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Debug, eventId, null, message, args);
    }

    public static void LogInformation(this ILogger logger, EventId eventId, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Information, eventId, null, message, args);
    }

    public static void LogWarning(this ILogger logger, EventId eventId, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Warning, eventId, null, message, args);
    }

    public static void LogError(this ILogger logger, EventId eventId, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Error, eventId, null, message, args);
    }

    public static void LogCritical(this ILogger logger, EventId eventId, string message, params ReadOnlySpan<string> args) {
        logger.Log(LogLevel.Critical, eventId, null, message, args);
    }
}
