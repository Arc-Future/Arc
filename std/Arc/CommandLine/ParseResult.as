// Arc.CommandLine.ParseResult —— 解析结果类型体系。
//
// 对标 C# System.CommandLine.Parsing.ParseResult 及其关联类型：
//   - ParseResult：顶层解析结果，含 Errors / UnmatchedTokens / 值提取方法
//   - CommandResult：匹配到的命令结果，含子选项和参数结果
//   - OptionResult：单个选项的解析结果
//   - ArgumentResult：单个位置参数的解析结果

namespace Arc.CommandLine {

/// <summary>
/// 单个选项的解析结果。对标 C# System.CommandLine.Parsing.OptionResult。
///
/// 包含匹配到的选项定义及其解析值列表。
/// 多值选项（AllowMultiple = true）或多次指定时，Values 包含所有值。
/// </summary>
public class OptionResult {
    private Option _option;
    private List<string> _values;

    /// <summary>创建选项结果。</summary>
    public OptionResult(Option option) {
        _option = option;
        _values = new List<string>();
    }

    /// <summary>关联的选项定义。</summary>
    public Option Option {
        get { return _option; }
    }

    /// <summary>解析到的值列表。</summary>
    public List<string> Values {
        get { return _values; }
    }

    /// <summary>值数量（避免 Values.Count 经 List 属性回传的 codegen 债）。</summary>
    public int ValueCount() {
        return _values.Count;
    }

    /// <summary>添加一个解析值。</summary>
    public void AddValue(string value) {
        _values.Add(value);
    }

    /// <summary>获取第一个值（最常用）。未提供时返回空串。</summary>
    public string GetValue() {
        if (_values.Count > 0) {
            return _values[0];
        }
        return "";
    }

    /// <summary>获取第一个值，未提供时返回默认值。</summary>
    public string GetValueOrDefault(string defaultValue) {
        if (_values.Count > 0) {
            return _values[0];
        }
        return defaultValue;
    }

    /// <summary>获取所有值。</summary>
    public List<string> GetValues() {
        List<string> result = new List<string>();
        int count = _values.Count;
        int i = 0;
        while (i < count) {
            result.Add(_values[i]);
            i = i + 1;
        }
        return result;
    }

    /// <summary>是否有值（选项被指定 / 标志选项出现）。</summary>
    public bool HasValue {
        get { return _values.Count > 0; }
    }
}

/// <summary>
/// 单个位置参数的解析结果。对标 C# System.CommandLine.Parsing.ArgumentResult。
/// </summary>
public class ArgumentResult {
    private Argument _argument;
    private List<string> _values;

    /// <summary>创建参数结果。</summary>
    public ArgumentResult(Argument argument) {
        _argument = argument;
        _values = new List<string>();
    }

    /// <summary>关联的参数定义。</summary>
    public Argument Argument {
        get { return _argument; }
    }

    /// <summary>解析到的值列表。</summary>
    public List<string> Values {
        get { return _values; }
    }

    /// <summary>值数量。</summary>
    public int ValueCount() {
        return _values.Count;
    }

    /// <summary>添加一个解析值。</summary>
    public void AddValue(string value) {
        _values.Add(value);
    }

    /// <summary>获取第一个值。未提供时返回空串。</summary>
    public string GetValue() {
        if (_values.Count > 0) {
            return _values[0];
        }
        return "";
    }

    /// <summary>获取第一个值，未提供时返回默认值。</summary>
    public string GetValueOrDefault(string defaultValue) {
        if (_values.Count > 0) {
            return _values[0];
        }
        return defaultValue;
    }

    /// <summary>获取所有值。</summary>
    public List<string> GetValues() {
        List<string> result = new List<string>();
        int count = _values.Count;
        int i = 0;
        while (i < count) {
            result.Add(_values[i]);
            i = i + 1;
        }
        return result;
    }

    /// <summary>是否有值。</summary>
    public bool HasValue {
        get { return _values.Count > 0; }
    }
}

/// <summary>
/// 匹配到的命令结果。对标 C# System.CommandLine.Parsing.CommandResult。
///
/// 包含匹配到的命令引用及所有选项/参数的解析结果。
/// </summary>
public class CommandResult {
    private Command _command;
    private List<OptionResult> _optionResults;
    private List<ArgumentResult> _argumentResults;

    /// <summary>创建命令结果。</summary>
    public CommandResult(Command command) {
        _command = command;
        _optionResults = new List<OptionResult>();
        _argumentResults = new List<ArgumentResult>();
    }

    /// <summary>匹配到的命令。</summary>
    public Command Command {
        get { return _command; }
    }

    /// <summary>选项解析结果列表。</summary>
    public List<OptionResult> Options {
        get { return _optionResults; }
    }

    /// <summary>已解析选项结果数量（避免 `Options.Count` 经 List 属性回传的 codegen 债）。</summary>
    public int OptionResultCount() {
        return _optionResults.Count;
    }

    /// <summary>按索引取选项结果。</summary>
    public OptionResult OptionResultAt(int index) {
        return _optionResults[index];
    }

    /// <summary>参数解析结果列表。</summary>
    public List<ArgumentResult> Arguments {
        get { return _argumentResults; }
    }

    /// <summary>已解析参数结果数量。</summary>
    public int ArgumentResultCount() {
        return _argumentResults.Count;
    }

    /// <summary>按索引取参数结果。</summary>
    public ArgumentResult ArgumentResultAt(int index) {
        return _argumentResults[index];
    }

    /// <summary>添加选项结果。</summary>
    public void AddOptionResult(OptionResult result) {
        _optionResults.Add(result);
    }

    /// <summary>添加参数结果。</summary>
    public void AddArgumentResult(ArgumentResult result) {
        _argumentResults.Add(result);
    }
}

/// <summary>
/// 顶层解析结果。对标 C# System.CommandLine.Parsing.ParseResult。
///
/// 封装完整的解析结果，包括匹配的命令、所有选项/参数值、错误信息、
/// 未匹配 token，以及类型安全的值提取方法。
///
/// 用法：
/// <code>
/// ParseResult result = cmd.Parse(Environment.ArgCount());
/// if (result.HasErrors) {
///     // 遍历 result.Errors 输出错误
/// }
/// string file = result.GetValueForOption(fileOpt);
/// bool verbose = result.GetBoolForOption(verboseOpt);
/// string input = result.GetValueForArgument(inputArg);
/// </code>
/// </summary>
public class ParseResult {
    private CommandResult _commandResult;
    private List<string> _errors;
    private List<string> _unmatchedTokens;
    private bool _isHelp;

    /// <summary>创建解析结果。</summary>
    public ParseResult(CommandResult commandResult) {
        _commandResult = commandResult;
        _errors = new List<string>();
        _unmatchedTokens = new List<string>();
        _isHelp = false;
    }

    /// <summary>根命令结果（当前 Arc 不支持嵌套命令结果，始终为主命令结果）。</summary>
    public CommandResult CommandResult {
        get { return _commandResult; }
    }

    /// <summary>解析错误列表。为空列表表示解析成功。</summary>
    public List<string> Errors {
        get { return _errors; }
    }

    /// <summary>是否有解析错误。</summary>
    public bool HasErrors {
        get { return _errors.Count > 0; }
    }

    /// <summary>添加解析错误。</summary>
    public void AddError(string error) {
        _errors.Add(error);
    }

    /// <summary>从另一个 ParseResult 合并错误。</summary>
    public void MergeErrors(ParseResult source) {
        int count = source._errors.Count;
        int i = 0;
        while (i < count) {
            _errors.Add(source._errors[i]);
            i = i + 1;
        }
    }

    /// <summary>未匹配的 token 列表（当 TreatUnmatchedTokensAsErrors = false 时）。</summary>
    public List<string> UnmatchedTokens {
        get { return _unmatchedTokens; }
    }

    /// <summary>添加未匹配 token。</summary>
    public void AddUnmatchedToken(string token) {
        _unmatchedTokens.Add(token);
    }

    /// <summary>用户是否请求了帮助（传递了 --help / -h）。</summary>
    public bool IsHelp {
        get { return _isHelp; }
    }

    /// <summary>标记为帮助请求。</summary>
    public void SetHelpRequested() {
        _isHelp = true;
    }

    // ── 选项值查询 ──

    /// <summary>查找指定选项的解析结果。未找到时返回空 OptionResult（HasValue = false）。</summary>
    public OptionResult FindOptionResult(Option option) {
        int count = _commandResult.OptionResultCount();
        int i = 0;
        while (i < count) {
            OptionResult optResult = _commandResult.OptionResultAt(i);
            if (optResult.Option == option) {
                return optResult;
            }
            i = i + 1;
        }
        return new OptionResult(option);
    }

    /// <summary>获取选项的字符串值。未提供时返回 DefaultValue（若有），否则返回空串。标志选项未指定返回 "false"。</summary>
    public string GetValueForOption(Option option) {
        OptionResult optResult = this.FindOptionResult(option);
        if (optResult.HasValue) {
            return optResult.GetValue();
        }
        string defVal = option.DefaultValue;
        if (defVal != "") {
            return defVal;
        }
        if (option.IsFlag) {
            return "false";
        }
        return "";
    }

    /// <summary>获取选项的字符串值，未提供时返回指定的默认值。</summary>
    public string GetValueForOptionOrDefault(Option option, string defaultValue) {
        OptionResult optResult = this.FindOptionResult(option);
        if (optResult.HasValue) {
            return optResult.GetValue();
        }
        return defaultValue;
    }

    /// <summary>获取选项的 bool 值（标志选项或被指定即为 true）。</summary>
    public bool GetBoolForOption(Option option) {
        OptionResult optResult = this.FindOptionResult(option);
        if (optResult.HasValue) {
            return true;
        }
        string defVal = option.DefaultValue;
        if (defVal == "true") {
            return true;
        }
        return false;
    }

    /// <summary>检查指定选项是否被显式提供。</summary>
    public bool HasOption(Option option) {
        OptionResult optResult = this.FindOptionResult(option);
        return optResult.HasValue;
    }

    /// <summary>获取选项的所有值（多值选项 / 多次指定）。</summary>
    public List<string> GetValuesForOption(Option option) {
        OptionResult optResult = this.FindOptionResult(option);
        return optResult.GetValues();
    }

    // ── 参数值查询 ──

    /// <summary>查找指定参数的解析结果。</summary>
    public ArgumentResult FindArgumentResult(Argument argument) {
        int count = _commandResult.ArgumentResultCount();
        int i = 0;
        while (i < count) {
            ArgumentResult argResult = _commandResult.ArgumentResultAt(i);
            if (argResult.Argument == argument) {
                return argResult;
            }
            i = i + 1;
        }
        return new ArgumentResult(argument);
    }

    /// <summary>获取参数的字符串值。未提供时返回 DefaultValue。</summary>
    public string GetValueForArgument(Argument argument) {
        ArgumentResult argResult = this.FindArgumentResult(argument);
        if (argResult.HasValue) {
            return argResult.GetValue();
        }
        return argument.DefaultValue;
    }

    /// <summary>获取参数的字符串值，未提供时返回指定的默认值。</summary>
    public string GetValueForArgumentOrDefault(Argument argument, string defaultValue) {
        ArgumentResult argResult = this.FindArgumentResult(argument);
        if (argResult.HasValue) {
            return argResult.GetValue();
        }
        return defaultValue;
    }

    /// <summary>获取参数的所有值（多值参数 / ZeroOrMore / OneOrMore）。</summary>
    public List<string> GetValuesForArgument(Argument argument) {
        ArgumentResult argResult = this.FindArgumentResult(argument);
        return argResult.GetValues();
    }
}

}
