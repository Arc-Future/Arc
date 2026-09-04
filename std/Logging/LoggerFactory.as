// LoggerFactory —— 日志工厂实现（对齐 .NET LoggerFactory）。
namespace Arc.Logging;

using Arc.Collections;

/// <summary>
/// 日志工厂实现——按类别缓存 <see cref="Logger"/>，并维护 Provider 注册表与全局最低级别。
///
/// 线程安全：LoggerFactory 通常于应用启动期配置、运行期只读；<see cref="AddProvider"/>
/// 应在开始记录前调用。日志记录路径（<see cref="Logger.Log"/>）仅读取 Provider 列表，
/// 无写竞争。
/// </summary>
public class LoggerFactory : ILoggerFactory {
    private List<ILoggerProvider> _providers;
    private Dictionary<string, ILogger> _loggers;
    private bool _disposed;

    public LoggerFactory() {
        _providers = new List<ILoggerProvider>();
        _loggers = new Dictionary<string, ILogger>();
    }

    /// <summary>全局最低输出级别，低于该级别的日志被过滤。</summary>
    public LogLevel MinimumLevel { get; set; } = LogLevel.Information;

    /// <summary>按类别名创建日志记录器（同类别返回已缓存实例）。</summary>
    public ILogger CreateLogger(string categoryName) {
        if (_disposed) { throw new ObjectDisposedException("LoggerFactory"); }
        if (_loggers.ContainsKey(categoryName)) {
            return _loggers[categoryName];
        }
        var logger = new Logger(this, categoryName);
        _loggers.Add(categoryName, logger);
        return logger;
    }

    /// <summary>注册一个日志提供程序。</summary>
    public void AddProvider(ILoggerProvider provider) {
        if (_disposed) { throw new ObjectDisposedException("LoggerFactory"); }
        if (provider == null) { throw new ArgumentNullException("provider"); }
        _providers.Add(provider);
    }

    /// <summary>清空全部已注册 Provider。</summary>
    public void ClearProviders() {
        if (_disposed) { throw new ObjectDisposedException("LoggerFactory"); }
        _providers.Clear();
    }

    /// <summary>当前 Provider 列表（只读快照引用）。</summary>
    internal List<ILoggerProvider> GetProviders() {
        return _providers;
    }

    public void Dispose() {
        if (_disposed) { return; }
        _disposed = true;
        // 逆序释放 Provider，级联 IDisposable；异常安全：单个 Dispose 失败不中断其余释放。
        // 先只读遍历逐个释放，结束后再整体 Clear（避免释放循环中修改容器触发
        // NLL E_ITERATOR_INVALIDATION）。
        int i = _providers.Count - 1;
        while (i >= 0) {
            var p = _providers[i];
            if (p != null && p is IDisposable) {
                var d = (IDisposable)p;
                d.Dispose();
            }
            i = i - 1;
        }
        _providers.Clear();
        _loggers.Clear();
        _loggers = null;
        _providers = null;
    }
}
