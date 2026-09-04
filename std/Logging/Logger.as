// Logger —— 日志记录器实现：负责模板格式化 + 按 Provider 分发。
namespace Arc.Logging;

using Arc.Collections;

/// <summary>
/// 日志记录器实现——持有工厂引用与类别名。记录时：
///   1. 按工厂最低级别过滤；
///   2. 将消息模板格式化为最终文本（仅一次）；
///   3. 逐个下发到各 Provider 的类别 Logger（按 Provider 索引缓存）。
///
/// 类别 Logger 缓存：按 Provider 索引缓存，避免每次重复调用 <c>CreateLogger</c>。
/// </summary>
internal class Logger : ILogger {
    private LoggerFactory _factory;
    private Dictionary<int, ILogger> _providerLoggers;

    public Logger(LoggerFactory factory, string category) {
        _factory = factory;
        this.Category = category;
        _providerLoggers = new Dictionary<int, ILogger>();
    }

    /// <summary>日志类别名。</summary>
    public string Category { get; }

    public bool IsEnabled(LogLevel logLevel) {
        if (logLevel == LogLevel.None) { return false; }
        return logLevel >= _factory.MinimumLevel;
    }

    public void Log(LogLevel logLevel, EventId eventId, Exception? exception, string message, params ReadOnlySpan<string> args) {
        if (!this.IsEnabled(logLevel)) { return; }
        string formatted = MessageTemplateFormatter.Format(message, args);
        var providers = _factory.GetProviders();
        int i = 0;
        int count = providers.Count;
        // 已前置格式化：Provider 侧无需再次解析模板，args 传空视图。
        // 先赋值给局部变量再传参（与 span_e2e 赋值路径一致），避免内联
        // `ReadOnlySpan<string>.Empty` 实参触发 codegen 构造调用降级。
        ReadOnlySpan<string> noArgs = ReadOnlySpan<string>.Empty;
        while (i < count) {
            ILogger providerLogger = this._GetProviderLogger(providers, i);
            if (providerLogger.IsEnabled(logLevel)) {
                providerLogger.Log(logLevel, eventId, exception, formatted, noArgs);
            }
            i = i + 1;
        }
    }

    /// <summary>作用域后置（诚实）：基本版本返回 null（对齐 .NET BeginScope 可返回 null）。</summary>
    public IDisposable? BeginScope(object? state) {
        return null;
    }

    /// <summary>获取/缓存指定 Provider 的类别 Logger。</summary>
    private ILogger _GetProviderLogger(List<ILoggerProvider> providers, int index) {
        if (_providerLoggers.ContainsKey(index)) {
            return _providerLoggers[index];
        }
        ILogger pl = providers[index].CreateLogger(this.Category);
        _providerLoggers.Add(index, pl);
        return pl;
    }
}
