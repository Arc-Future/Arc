// Arc.CommandLine.ArgumentArity —— 命令行参数元数枚举。
namespace Arc.CommandLine {

/// <summary>
/// 命令行参数元数枚举。对标 C# System.CommandLine.ArgumentArity。
/// </summary>
public enum ArgumentArity {
    /// <summary>恰好一个值（默认）。</summary>
    ExactlyOne,
    /// <summary>零或一个值（可选参数）。</summary>
    ZeroOrOne,
    /// <summary>零或多个值。</summary>
    ZeroOrMore,
    /// <summary>一个或多个值。</summary>
    OneOrMore,
}

}
