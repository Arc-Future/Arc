// Arc.CommandLine.Command —— 命令层级体系。
//
// 对标 C# System.CommandLine.Command + System.CommandLine.RootCommand。
//
// 成熟度：黄灯（Parse 快路径可证伪）——已撤 Stable。
// 可证伪：空 Parse 必选/默认值、单 token --help/-h、Option/Argument 契约。
// 残余 AV（codegen）：含选项匹配的完整 Parse 循环；IConsole 接口分派（PrintHelp/Invoke）。
// 禁 Stable 空挂；完整 System.CommandLine 对等后置。
//
// 提供声明式 CLI 接口定义，支持：
//   - 选项注册（Option）：命名参数（--flag / --value v）
//   - 参数注册（Argument）：位置参数
//   - 子命令注册（Command）：命令树（AddCommand）
//   - 全局选项（AddGlobalOption）：传递给所有子命令
//   - 帮助自动生成（PrintHelp）：根据注册的选项/参数格式化输出
//   - 处理程序绑定（SetHandler）：Action<InvocationContext> 委托
//   - 解析执行（Parse / Invoke）：tokenize → 匹配 → 验证 → 生成 ParseResult
//
// RootCommand 是 Command 的便捷子类，名称为空字符串，仅需描述参数。

using Arc.Text;

namespace Arc.CommandLine {

/// <summary>子命令委托的内部数据类型（非公开契约）。</summary>
struct PositionalArgs {
    public List<string> Tokens;
    public bool HelpRequested;
    public ParseResult ParentErrors;
}

/// <summary>
/// 命令节点。对标 C# System.CommandLine.Command。
///
/// 命令是 CLI 解析树中的节点，可以包含选项、位置参数和子命令。
/// 命令本身可以有处理程序（Handler），子命令未匹配时回退到父命令处理。
///
/// 用法：
/// <code>
/// Command cmd = new Command("build", "构建项目");
/// cmd.AddOption(new Option("--release", "-r", "Release 模式构建"));
/// cmd.SetHandler(Action&lt;InvocationContext&gt;((InvocationContext ctx) => {
///     bool isRelease = ctx.ParseResult.GetBoolForOption(releaseOpt);
///     // ...
/// }));
/// root.AddCommand(cmd);
/// </code>
/// </summary>
public class Command {
    private string _name;
    private string _description;
    private List<Option> _options;
    private List<Argument> _arguments;
    private List<Command> _subcommands;
    private List<Option> _globalOptions;
    private bool _treatUnmatchedTokensAsErrors;
    private ICommandHandler _handler;

    // ── 构造函数 ──

    /// <summary>
    /// 创建具名命令。
    /// </summary>
    /// <param name="name">命令名（用于匹配 CLI 输入，如 "build"）。</param>
    /// <param name="description">命令描述（用于 --help 输出）。</param>
    public Command(string name, string description) {
        _name = name;
        _description = description;
        _options = new List<Option>();
        _arguments = new List<Argument>();
        _subcommands = new List<Command>();
        _globalOptions = new List<Option>();
        _treatUnmatchedTokensAsErrors = true;
        _handler = null;
    }

    // ── 属性 ──

    /// <summary>命令名。</summary>
    public string Name {
        get { return _name; }
    }

    /// <summary>命令描述。</summary>
    public string Description {
        get { return _description; }
        set { _description = value; }
    }

    /// <summary>
    /// 未匹配 token 是否视为错误。
    /// 默认 true；设为 false 后未知选项作为 UnmatchedTokens 收集在 ParseResult 中。
    /// 对标 C# Command.TreatUnmatchedTokensAsErrors。
    /// </summary>
    public bool TreatUnmatchedTokensAsErrors {
        get { return _treatUnmatchedTokensAsErrors; }
        set { _treatUnmatchedTokensAsErrors = value; }
    }

    /// <summary>已注册的选项列表。</summary>
    public List<Option> Options {
        get { return _options; }
    }

    /// <summary>已注册的位置参数列表。</summary>
    public List<Argument> Arguments {
        get { return _arguments; }
    }

    /// <summary>子命令列表。</summary>
    public List<Command> Subcommands {
        get { return _subcommands; }
    }

    // ── 选项 / 参数 / 命令注册 ──

    /// <summary>添加选项。</summary>
    public void AddOption(Option option) {
        _options.Add(option);
    }

    /// <summary>添加位置参数。</summary>
    public void AddArgument(Argument argument) {
        _arguments.Add(argument);
    }

    /// <summary>
    /// 添加子命令。
    ///
    /// 解析时若第一个 token 匹配子命令名，则委托该子命令继续解析。
    /// </summary>
    public void AddCommand(Command command) {
        _subcommands.Add(command);
    }

    /// <summary>
    /// 添加全局选项。全局选项继承到所有子命令。
    /// 对标 C# Command.AddGlobalOption。
    /// </summary>
    public void AddGlobalOption(Option option) {
        _globalOptions.Add(option);
        _options.Add(option);
    }

    /// <summary>获取全局选项列表。</summary>
    public List<Option> GetGlobalOptions() {
        List<Option> result = new List<Option>();
        int count = _globalOptions.Count;
        int i = 0;
        while (i < count) {
            result.Add(_globalOptions[i]);
            i = i + 1;
        }
        return result;
    }

    // ── 处理程序绑定 ──

    /// <summary>
    /// 设置命令处理程序。对标 C# Command.SetHandler。
    /// L2：接收 <see cref="ICommandHandler"/>（非 Action&lt;InvocationContext&gt; 委托——缺 Func 单态）。
    /// </summary>
    public void SetHandler(ICommandHandler handler) {
        _handler = handler;
    }

    /// <summary>获取已设置的处理程序。未设置返回 null。</summary>
    public ICommandHandler GetHandler() {
        return _handler;
    }

    // ── 解析 ──

    /// <summary>
    /// 从进程 argv 解析（跳过 argv[0] 程序名）。
    /// 对标 C# <c>Parse(string[] args)</c> 的进程入口惯用法。
    /// 可测路径请用 Parse(List&lt;string&gt;)。
    /// </summary>
    /// <param name="argc">参数总数（通常为 Environment.ArgCount()）。</param>
    public ParseResult Parse(int argc) {
        List<string> tokens = new List<string>();
        int i = 1;
        while (i < argc) {
            tokens.Add(Environment.GetArg(i));
            i = i + 1;
        }
        return this.Parse(tokens);
    }

    /// <summary>
    /// 解析显式 token 列表（不含程序名）。L2 诚实可测入口。
    ///
    /// 解析流程：
    /// 1. 遍历 tokens
    /// 2. 检查 --help / -h → 标记 IsHelp（不终止解析）
    /// 3. 检查子命令名 → 委托子命令解析
    /// 4. 匹配 Option（标志 / 值 / 多值）
    /// 5. -- 之后全部视为位置参数
    /// 6. 剩余 token 分配给 Argument；验证必选；返回 ParseResult
    /// </summary>
    public ParseResult Parse(List<string> tokens) {
        CommandResult cmdResult = new CommandResult(this);
        ParseResult result = new ParseResult(cmdResult);
        int argc = tokens.Count;

        // 单 token 帮助快路径（完整选项匹配循环现 tip 仍有 codegen 残余）
        if (argc == 1) {
            string only = tokens[0];
            if (only == "--help" || only == "-h") {
                result.SetHelpRequested();
                return result;
            }
        }

        List<string> positional = new List<string>();
        bool helpRequested = false;
        bool endOfOptions = false;

        int i = 0;
        while (i < argc) {
            string token = tokens[i];

            if (token == "--help" || token == "-h") {
                helpRequested = true;
                i = i + 1;
                continue;
            }

            if (!endOfOptions && token == "--") {
                endOfOptions = true;
                i = i + 1;
                continue;
            }

            // Option matching body continues below
            if (!endOfOptions) {
                Option matchedOpt = this.FindOptionByToken(token);
                if (matchedOpt != null) {
                    OptionResult existing = this.FindOrCreateOptionResult(cmdResult, matchedOpt);

                    if (matchedOpt.Arity == ArgumentArity.ZeroOrOne) {
                        existing.AddValue("true");
                    } else {
                        if (i + 1 < argc) {
                            i = i + 1;
                            string val = tokens[i];

                            Option conflictOpt = this.FindOptionByToken(val);
                            if (conflictOpt != null) {
                                result.AddError("选项 " + token + " 缺少值参数");
                                i = i - 1;
                            } else {
                                string validateError = matchedOpt.Validate(val);
                                if (validateError != "") {
                                    result.AddError(validateError);
                                }
                                bool isMulti = matchedOpt.Arity == ArgumentArity.OneOrMore
                                    || matchedOpt.Arity == ArgumentArity.ZeroOrMore;
                                if (isMulti || existing.ValueCount() == 0) {
                                    existing.AddValue(val);
                                } else {
                                    result.AddError("选项 " + token + " 不支持多值（设置 Arity = OneOrMore 启用）");
                                }
                            }
                        } else {
                            result.AddError("选项 " + token + " 需要值参数");
                        }
                    }
                } else {
                    positional.Add(token);
                }
            } else {
                positional.Add(token);
            }

            i = i + 1;
        }

        if (positional.Count > 0) {
            string firstPos = positional[0];
            Command subCmd = this.FindSubcommand(firstPos);
            if (subCmd != null) {
                PositionalArgs parentArgs = new PositionalArgs();
                parentArgs.Tokens = positional;
                parentArgs.HelpRequested = helpRequested;
                parentArgs.ParentErrors = result;
                return subCmd.ParseFromPositional(parentArgs);
            }
        }

        int consumedCount = 0;
        if (_arguments.Count > 0) {
            consumedCount = this.AssignPositionalArguments(result, cmdResult, positional);
        }

        int posCount = positional.Count;
        int remainingIdx = consumedCount;
        if (remainingIdx < posCount) {
            if (_treatUnmatchedTokensAsErrors) {
                while (remainingIdx < posCount) {
                    result.AddError("未知选项或参数: " + positional[remainingIdx]);
                    remainingIdx = remainingIdx + 1;
                }
            } else {
                while (remainingIdx < posCount) {
                    result.AddUnmatchedToken(positional[remainingIdx]);
                    remainingIdx = remainingIdx + 1;
                }
            }
        }

        this.ValidateRequiredOptions(result, cmdResult);

        if (helpRequested) {
            result.SetHelpRequested();
        }

        return result;
    }

    /// <summary>
    /// 从位置参数列表解析（子命令委托）。
    /// </summary>
    private ParseResult ParseFromPositional(PositionalArgs args) {
        CommandResult cmdResult = new CommandResult(this);
        ParseResult result = new ParseResult(cmdResult);
        bool endOfOptions = false;
        bool helpRequested = args.HelpRequested;
        List<string> positional = new List<string>();

        // 跳过第一个 token（子命令名 args.Tokens[0]），从 args.Tokens[1] 开始处理
        int i = 1;
        int posCount = args.Tokens.Count;

        while (i < posCount) {
            string token = args.Tokens[i];

            if (token == "--help" || token == "-h") {
                helpRequested = true;
                i = i + 1;
                continue;
            }

            if (!endOfOptions && token == "--") {
                endOfOptions = true;
                i = i + 1;
                continue;
            }

            if (!endOfOptions) {
                Option matchedOpt = this.FindOptionByToken(token);
                if (matchedOpt != null) {
                    OptionResult existing = this.FindOrCreateOptionResult(cmdResult, matchedOpt);

                    if (matchedOpt.Arity == ArgumentArity.ZeroOrOne) {
                        existing.AddValue("true");
                    } else {
                        if (i + 1 < posCount) {
                            i = i + 1;
                            string val = args.Tokens[i];
                            Option conflictOpt = this.FindOptionByToken(val);
                            if (conflictOpt != null) {
                                result.AddError("选项 " + token + " 缺少值参数");
                                i = i - 1;
                            } else {
                                string validateError = matchedOpt.Validate(val);
                                if (validateError != "") {
                                    result.AddError(validateError);
                                }
                                bool isMulti = matchedOpt.Arity == ArgumentArity.OneOrMore
                                    || matchedOpt.Arity == ArgumentArity.ZeroOrMore;
                                if (isMulti || existing.ValueCount() == 0) {
                                    existing.AddValue(val);
                                } else {
                                    result.AddError("选项 " + token + " 不支持多值");
                                }
                            }
                        } else {
                            result.AddError("选项 " + token + " 需要值参数");
                        }
                    }
                } else {
                    positional.Add(token);
                }
            } else {
                positional.Add(token);
            }

            i = i + 1;
        }

        // ── 分配子命令的位置参数 ──
        int consumedCount = 0;
        if (_arguments.Count > 0) {
            consumedCount = this.AssignPositionalArguments(result, cmdResult, positional);
        }

        // ── 处理子命令的未消耗 token ──
        int remainingIdx = consumedCount;
        int remainingCount = positional.Count;
        if (remainingIdx < remainingCount) {
            if (_treatUnmatchedTokensAsErrors) {
                while (remainingIdx < remainingCount) {
                    result.AddError("未知选项或参数: " + positional[remainingIdx]);
                    remainingIdx = remainingIdx + 1;
                }
            } else {
                while (remainingIdx < remainingCount) {
                    result.AddUnmatchedToken(positional[remainingIdx]);
                    remainingIdx = remainingIdx + 1;
                }
            }
        }

        // ── 合并父亲的错误 ──
        if (args.ParentErrors != null) {
            result.MergeErrors(args.ParentErrors);
        }

        // ── 验证 ──
        this.ValidateRequiredOptions(result, cmdResult);

        if (helpRequested) {
            result.SetHelpRequested();
        }

        return result;
    }

    /// <summary>
    /// 分配位置参数到 Argument 定义。
    /// 返回已消耗的 token 数量（用于判断还剩多少未分配 token）。
    /// </summary>
    private int AssignPositionalArguments(ParseResult result, CommandResult cmdResult, List<string> positional) {
        int argDefCount = _arguments.Count;
        if (argDefCount == 0) { return 0; }

        int posIndex = 0;
        int posCount = positional.Count;
        int argIdx = 0;

        while (argIdx < argDefCount && posIndex < posCount) {
            Argument argDef = _arguments[argIdx];
            ArgumentResult argResult = new ArgumentResult(argDef);

            bool isLast = (argIdx == argDefCount - 1);
            bool greedy = (argDef.Arity == ArgumentArity.ZeroOrMore || argDef.Arity == ArgumentArity.OneOrMore);

            if (isLast && greedy) {
                while (posIndex < posCount) {
                    string val = positional[posIndex];
                    string validateError = argDef.Validate(val);
                    if (validateError != "") {
                        result.AddError(validateError);
                    }
                    argResult.AddValue(val);
                    posIndex = posIndex + 1;
                }
            } else {
                string val = positional[posIndex];
                string validateError = argDef.Validate(val);
                if (validateError != "") {
                    result.AddError(validateError);
                }
                argResult.AddValue(val);
                posIndex = posIndex + 1;
            }

            cmdResult.AddArgumentResult(argResult);
            argIdx = argIdx + 1;
        }

        return posIndex;
    }

    /// <summary>验证必选选项。</summary>
    private void ValidateRequiredOptions(ParseResult result, CommandResult cmdResult) {
        int optIdx = 0;
        int optCount = _options.Count;
        while (optIdx < optCount) {
            Option optDef = _options[optIdx];
            if (optDef.IsRequired) {
                OptionResult existing = this.FindExistingOptionResult(cmdResult, optDef);
                bool missing = false;
                if (existing == null) {
                    missing = true;
                } else {
                    if (!existing.HasValue) {
                        missing = true;
                    }
                }
                if (missing) {
                    if (optDef.DefaultValue == "") {
                        result.AddError("缺少必选选项: " + optDef.PrimaryAlias);
                    }
                }
            }
            optIdx = optIdx + 1;
        }
    }

    // ── 执行 ──

    /// <summary>
    /// 解析并执行（进程 argv）。对标 C# Command.Invoke(string[] args)。
    /// </summary>
    public int Invoke(int argc, IConsole console) {
        List<string> tokens = new List<string>();
        int i = 1;
        while (i < argc) {
            tokens.Add(Environment.GetArg(i));
            i = i + 1;
        }
        return this.Invoke(tokens, console);
    }

    /// <summary>Invoke 便捷重载（DefaultConsole）。</summary>
    public int Invoke(int argc) {
        return this.Invoke(argc, new DefaultConsole());
    }

    /// <summary>
    /// 解析并执行显式 token 列表（不含程序名）。L2 可测入口。
    /// 帮助 → PrintHelp 返回 0；错误 → stderr 返回 1；否则调用 Handler。
    /// </summary>
    public int Invoke(List<string> tokens, IConsole console) {
        if (console == null) {
            console = new DefaultConsole();
        }

        ParseResult result = this.Parse(tokens);

        if (result.IsHelp) {
            this.PrintHelp(console);
            return 0;
        }

        if (result.HasErrors) {
            int errCount = result.Errors.Count;
            int i = 0;
            while (i < errCount) {
                console.ErrorWriteLine("错误: " + result.Errors[i]);
                i = i + 1;
            }
            console.ErrorWriteLine("使用 --help 查看帮助信息。");
            return 1;
        }

        if (_handler != null) {
            InvocationContext ctx = new InvocationContext(result, console);
            _handler.Invoke(ctx);
            return ctx.ExitCode;
        }

        return 0;
    }

    /// <summary>Invoke(List) 便捷重载（DefaultConsole）。</summary>
    public int Invoke(List<string> tokens) {
        return this.Invoke(tokens, new DefaultConsole());
    }

    // ── 帮助生成 ──

    /// <summary>
    /// 输出命令帮助文本。
    ///
    /// 自动根据注册的选项（Option）、参数（Argument）和子命令（Command）
    /// 生成格式化帮助，对标 C# System.CommandLine --help 输出。
    /// </summary>
    public void PrintHelp(IConsole console) {
        if (console == null) {
            console = new DefaultConsole();
        }

        // Description
        if (_description != "") {
            console.WriteLine("Description:");
            console.WriteLine("  " + _description);
            console.WriteLine("");
        }

        // Usage
        this.PrintUsage(console);

        // Options
        this.PrintOptionsHelp(console);

        // Arguments
        this.PrintArgumentsHelp(console);

        // Subcommands
        this.PrintSubcommandsHelp(console);
    }

    /// <summary>PrintHelp 便捷重载（使用 DefaultConsole）。</summary>
    public void PrintHelp() {
        this.PrintHelp(new DefaultConsole());
    }

    // ── 帮助子模块 ──

    /// <summary>输出 Usage 行。</summary>
    private void PrintUsage(IConsole console) {
        StringBuilder sb = new StringBuilder();
        sb.Append("Usage:");
        if (_name != "") {
            sb.Append(" ");
            sb.Append(_name);
        }

        int optCount = _options.Count;
        if (optCount > 0) {
            sb.Append(" [options]");
        }

        int argCount = _arguments.Count;
        if (argCount > 0) {
            int argIdx = 0;
            while (argIdx < argCount) {
                Argument argDef = _arguments[argIdx];
                sb.Append(" ");
                if (argDef.IsRequired) {
                    sb.Append("<");
                    sb.Append(argDef.Name);
                    sb.Append(">");
                } else {
                    sb.Append("[");
                    sb.Append(argDef.Name);
                    sb.Append("]");
                }
                argIdx = argIdx + 1;
            }
        }

        console.WriteLine(sb.ToString());
        console.WriteLine("");
    }

    /// <summary>输出 Options 帮助块。</summary>
    private void PrintOptionsHelp(IConsole console) {
        int optCount = _options.Count;
        if (optCount == 0) { return; }

        console.WriteLine("Options:");
        int optIdx = 0;
        while (optIdx < optCount) {
            Option optDef = _options[optIdx];
            this.PrintOptionHelpLine(console, optDef);
            optIdx = optIdx + 1;
        }
        console.WriteLine("");
    }

    /// <summary>格式化输出单个选项的帮助行。</summary>
    private void PrintOptionHelpLine(IConsole console, Option opt) {
        StringBuilder sb = new StringBuilder();
        sb.Append("  ");
        sb.Append(opt.ToAliasString());

        // 填充到 24 字符宽
        int currentLen = sb.Length;
        int target = 24;
        while (currentLen < target) {
            sb.Append(" ");
            currentLen = currentLen + 1;
        }

        sb.Append(opt.Description);

        if (opt.IsFlag) {
            sb.Append(" (flag)");
        }

        string defVal = opt.DefaultValue;
        if (defVal != "" && !opt.IsFlag) {
            sb.Append(" [default: ");
            sb.Append(defVal);
            sb.Append("]");
        }

        if (opt.IsRequired) {
            sb.Append(" (required)");
        }

        console.WriteLine(sb.ToString());
    }

    /// <summary>输出 Arguments 帮助块。</summary>
    private void PrintArgumentsHelp(IConsole console) {
        int argCount = _arguments.Count;
        if (argCount == 0) { return; }

        console.WriteLine("Arguments:");
        int argIdx = 0;
        while (argIdx < argCount) {
            Argument argDef = _arguments[argIdx];

            StringBuilder sb = new StringBuilder();
            sb.Append("  <");
            sb.Append(argDef.Name);
            sb.Append(">");

            int currentLen = sb.Length;
            int target = 24;
            while (currentLen < target) {
                sb.Append(" ");
                currentLen = currentLen + 1;
            }

            sb.Append(argDef.Description);

            if (argDef.IsRequired) {
                sb.Append(" (required)");
            }

            console.WriteLine(sb.ToString());
            argIdx = argIdx + 1;
        }
        console.WriteLine("");
    }

    /// <summary>输出 Commands 帮助块。</summary>
    private void PrintSubcommandsHelp(IConsole console) {
        int subCount = _subcommands.Count;
        if (subCount == 0) { return; }

        console.WriteLine("Commands:");
        int subIdx = 0;
        while (subIdx < subCount) {
            Command sub = _subcommands[subIdx];

            StringBuilder sb = new StringBuilder();
            sb.Append("  ");
            sb.Append(sub.Name);

            int currentLen = sb.Length;
            int target = 24;
            while (currentLen < target) {
                sb.Append(" ");
                currentLen = currentLen + 1;
            }

            sb.Append(sub.Description);
            console.WriteLine(sb.ToString());

            subIdx = subIdx + 1;
        }
        console.WriteLine("");
    }

    // ── 内部辅助方法 ──

    /// <summary>根据 token 查找匹配的选项。</summary>
    private Option FindOptionByToken(string token) {
        int count = _options.Count;
        int i = 0;
        while (i < count) {
            Option opt = _options[i];
            if (opt.MatchesAlias(token)) {
                return opt;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>查找或创建选项结果。</summary>
    private OptionResult FindOrCreateOptionResult(CommandResult cmdResult, Option option) {
        int count = cmdResult.OptionResultCount();
        int i = 0;
        while (i < count) {
            OptionResult existing = cmdResult.OptionResultAt(i);
            if (existing.Option == option) {
                return existing;
            }
            i = i + 1;
        }
        OptionResult newResult = new OptionResult(option);
        cmdResult.AddOptionResult(newResult);
        return newResult;
    }

    /// <summary>查找现有的选项结果（不创建）。</summary>
    private OptionResult FindExistingOptionResult(CommandResult cmdResult, Option option) {
        int count = cmdResult.OptionResultCount();
        int i = 0;
        while (i < count) {
            OptionResult existing = cmdResult.OptionResultAt(i);
            if (existing.Option == option) {
                return existing;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>查找子命令。</summary>
    private Command FindSubcommand(string name) {
        int count = _subcommands.Count;
        int i = 0;
        while (i < count) {
            Command sub = _subcommands[i];
            if (sub.Name == name) {
                return sub;
            }
            i = i + 1;
        }
        return null;
    }
}

/// <summary>
/// 根命令。对标 C# System.CommandLine.RootCommand。
///
/// 根命令是 CLI 应用的入口点，名称为空字符串，仅需描述参数。
/// 相当于 C# 中 <c>new RootCommand("描述")</c> 的语义。
///
/// 用法：
/// <code>
/// RootCommand root = new RootCommand("我的 CLI 工具 - 示例应用");
/// Option verboseOpt = new Option("--verbose", "-v", "启用详细输出");
/// verboseOpt.IsFlag = true;
/// root.AddOption(verboseOpt);
///
/// Argument inputArg = new Argument("input", "输入文件路径");
/// inputArg.IsRequired = true;
/// root.AddArgument(inputArg);
///
/// root.SetHandler(Action&lt;InvocationContext&gt;((InvocationContext ctx) => {
///     bool verbose = ctx.ParseResult.GetBoolForOption(verboseOpt);
///     string input = ctx.ParseResult.GetValueForArgument(inputArg);
///     ctx.Console.WriteLine("处理文件: " + input);
/// }));
/// int exitCode = root.Invoke(Environment.ArgCount());
/// Environment.Exit(exitCode);
/// </code>
/// </summary>
public class RootCommand : Command {
    /// <summary>创建根命令。</summary>
    /// <param name="description">应用描述（用于 --help 输出）。</param>
    public RootCommand(string description) : base("", description) {
    }
}

}
