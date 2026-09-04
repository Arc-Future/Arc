// Arc.CommandLine.Invocation —— 调用上下文与控制台抽象。
//
// 对标 C# System.CommandLine.Invocation.InvocationContext + System.CommandLine.IConsole。
//
// 成熟度：Stable 最小面（随 CommandLine）——CaptureConsole + IConsole PrintHelp 可证伪；
// 完整 System.CommandLine 对等后置。
//
// IConsole：解耦命令行处理器与控制台 I/O，包含重定向检测，默认实现委托至 Arc.Console。
// InvocationContext：封装 ParseResult + IConsole + ExitCode，作为 Handler 的参数传递。

namespace Arc.CommandLine {

/// <summary>
/// 控制台抽象层接口。解耦命令处理逻辑与控制台 I/O，便于测试。
/// 对标 C# System.CommandLine.IConsole。
///
/// Arc 简化：以 Write/WriteLine/ErrorWrite/ErrorWriteLine 方法代替 C# 的
/// IStandardStreamWriter Out/Error 属性，降低 Arc 接口复杂度。
/// </summary>
public interface IConsole {
    /// <summary>写入标准输出（不换行）。</summary>
    void Write(string message);

    /// <summary>写入标准输出并换行。</summary>
    void WriteLine(string message);

    /// <summary>写入标准错误（不换行）。</summary>
    void ErrorWrite(string message);

    /// <summary>写入标准错误并换行。</summary>
    void ErrorWriteLine(string message);

    /// <summary>标准输出是否被重定向。对标 C# IConsole.IsOutputRedirected。</summary>
    bool IsOutputRedirected { get; }

    /// <summary>标准错误是否被重定向。对标 C# IConsole.IsErrorRedirected。</summary>
    bool IsErrorRedirected { get; }

    /// <summary>标准输入是否被重定向。对标 C# IConsole.IsInputRedirected。</summary>
    bool IsInputRedirected { get; }
}

/// <summary>
/// IConsole 的默认实现，委托至 <see cref="Arc.Console"/> 静态方法。
///
/// 重定向检测当前返回 false（需运行时 ABI 支持，参见 rt_console_is_redirected）。
/// </summary>
public class DefaultConsole : IConsole {
    private bool _isOutputRedirected;
    private bool _isErrorRedirected;
    private bool _isInputRedirected;

    public DefaultConsole() {
        _isOutputRedirected = false;
        _isErrorRedirected = false;
        _isInputRedirected = false;
    }

    public void Write(string message) {
        Console.Write(message);
    }

    public void WriteLine(string message) {
        Console.WriteLine(message);
    }

    public void ErrorWrite(string message) {
        Console.ErrorWrite(message);
    }

    public void ErrorWriteLine(string message) {
        Console.ErrorWriteLine(message);
    }

    /// <summary>标准输出是否被重定向。</summary>
    public bool IsOutputRedirected {
        get { return _isOutputRedirected; }
    }

    /// <summary>标准错误是否被重定向。</summary>
    public bool IsErrorRedirected {
        get { return _isErrorRedirected; }
    }

    /// <summary>标准输入是否被重定向。</summary>
    public bool IsInputRedirected {
        get { return _isInputRedirected; }
    }
}

/// <summary>
/// 捕获 IConsole 输出的测试用实现（L2 诚实可测；非 DI）。
/// OutText / ErrorText 为累计写入内容。
/// </summary>
public class CaptureConsole : IConsole {
    private string _outText;
    private string _errorText;

    public CaptureConsole() {
        _outText = "";
        _errorText = "";
    }

    public string OutText {
        get { return _outText; }
    }

    public string ErrorText {
        get { return _errorText; }
    }

    public void Write(string message) {
        _outText = _outText + message;
    }

    public void WriteLine(string message) {
        _outText = _outText + message + "\n";
    }

    public void ErrorWrite(string message) {
        _errorText = _errorText + message;
    }

    public void ErrorWriteLine(string message) {
        _errorText = _errorText + message + "\n";
    }

    public bool IsOutputRedirected { get; }

    public bool IsErrorRedirected { get; }

    public bool IsInputRedirected { get; }
}

/// <summary>
/// 命令处理程序接口（L2 诚实替代 Action&lt;InvocationContext&gt;——
/// 后者在 tip 上缺 Func 单态发射）。对标 C# SetHandler 回调角色。
/// </summary>
public interface ICommandHandler {
    void Invoke(InvocationContext context);
}

/// <summary>
/// 命令行调用上下文。对标 C# System.CommandLine.Invocation.InvocationContext。
///
/// 封装解析结果、控制台抽象和退出码，传给 <see cref="Command.SetHandler"/> 注册的回调。
/// </summary>
public class InvocationContext {
    private ParseResult _parseResult;
    private IConsole _console;
    private int _exitCode;

    /// <summary>创建调用上下文。</summary>
    /// <param name="parseResult">解析结果。</param>
    /// <param name="console">控制台抽象。</param>
    public InvocationContext(ParseResult parseResult, IConsole console) {
        _parseResult = parseResult;
        _console = console;
        _exitCode = 0;
    }

    /// <summary>解析结果，包含所有选项和参数的解析值。</summary>
    public ParseResult ParseResult {
        get { return _parseResult; }
        set { _parseResult = value; }
    }

    /// <summary>控制台抽象，用于输出信息。</summary>
    public IConsole Console {
        get { return _console; }
    }

    /// <summary>进程退出码。默认 0 (成功)，Handler 中设置为非 0 表示错误。</summary>
    public int ExitCode {
        get { return _exitCode; }
        set { _exitCode = value; }
    }
}

}
