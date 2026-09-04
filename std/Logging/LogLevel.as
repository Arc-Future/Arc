// LogLevel —— 日志级别枚举（对齐 .NET Microsoft.Extensions.Logging.LogLevel）。
namespace Arc.Logging;

/// <summary>
/// 日志严重性级别，数值越高越严重。
///
/// 级别过滤：Logger 仅当 <c>logLevel &gt;= 工厂 MinimumLevel</c> 且非 <c>None</c> 时输出。
/// <c>None</c> 用于显式关闭某类别或作为 "不输出任何级别" 的哨兵值。
/// </summary>
public enum LogLevel {
    /// <summary>最详细的诊断信息，通常仅用于开发环境。</summary>
    Trace = 0,

    /// <summary>调试期信息，可帮助开发者定位问题。</summary>
    Debug = 1,

    /// <summary>常规运行信息，描述应用正常流程。</summary>
    Information = 2,

    /// <summary>潜在问题或非理想状态，应用仍可继续运行。</summary>
    Warning = 3,

    /// <summary>已发生的错误，应用可能部分功能不可用。</summary>
    Error = 4,

    /// <summary>致命错误，应用可能无法继续运行。</summary>
    Critical = 5,

    /// <summary>不记录任何日志（用于关闭输出）。</summary>
    None = 6,
}
