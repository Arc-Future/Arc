// Arc.CommandLine.Option —— 命令行选项定义。
//
// 对标 C# System.CommandLine.Option，提供声明式选项契约定义：
//   - 长/短别名（--verbose / -v）
//   - 描述文本（用于 --help 自动生成）
//   - 必选/可选标记
//   - ArgumentArity 元数控制（对标 C# Option.Arity）
//   - IsFlag / AllowMultiple 便捷属性（操作 Arity）
//   - 默认值
//   - 验证器（AddValidator）
//   - 候选取值限制（FromAmong）

namespace Arc.CommandLine {

/// <summary>
/// 命令行选项定义。对标 C# System.CommandLine.Option。
///
/// 支持长别名（--xxx）和短别名（-x），至少提供一个别名作为标识符。
/// 通过 <see cref="Arity"/> 控制选项期待的值数量，对标 C# Option.Arity。
///
/// 用法：
/// <code>
/// // 标志选项（开关）
/// Option verboseOpt = new Option("--verbose", "-v", "启用详细输出");
/// verboseOpt.IsFlag = true;
///
/// // 值选项（单值）
/// Option portOpt = new Option("--port", "-p", "监听端口号");
/// portOpt.DefaultValue = "8080";
/// portOpt.AddValidator(Func&lt;string, string&gt;((string val) => {
///     return ""; // 通过
/// }));
///
/// // 多值选项
/// Option includeOpt = new Option("--include", "-i", "包含路径");
/// includeOpt.AllowMultiple = true;
/// </code>
/// </summary>
public class Option {
    private List<string> _aliases;
    private string _description;
    private bool _isRequired;
    private ArgumentArity _arity;
    private string _defaultValue;
    private List<Func<string, string>> _validators;
    private List<string> _allowedValues;

    // ── 构造函数 ──

    /// <summary>单别名选项。默认 Arity = ExactlyOne（值选项）。</summary>
    public Option(string alias, string description) {
        _aliases = new List<string>();
        _aliases.Add(alias);
        _description = description;
        _isRequired = false;
        _arity = ArgumentArity.ExactlyOne;
        _defaultValue = "";
        _validators = new List<Func<string, string>>();
        _allowedValues = new List<string>();
    }

    /// <summary>双别名选项（长 + 短）。默认 Arity = ExactlyOne（值选项）。</summary>
    public Option(string alias1, string alias2, string description) {
        _aliases = new List<string>();
        _aliases.Add(alias1);
        _aliases.Add(alias2);
        _description = description;
        _isRequired = false;
        _arity = ArgumentArity.ExactlyOne;
        _defaultValue = "";
        _validators = new List<Func<string, string>>();
        _allowedValues = new List<string>();
    }

    // ── 属性 ──

    /// <summary>选项描述（用于 --help 输出）。</summary>
    public string Description {
        get { return _description; }
        set { _description = value; }
    }

    /// <summary>是否为必选选项。必选选项缺失时解析报错。</summary>
    public bool IsRequired {
        get { return _isRequired; }
        set { _isRequired = value; }
    }

    /// <summary>
    /// 选项元数（对标 C# Option.Arity）。控制选项期待的值数量：
    ///   - ZeroOrOne：标志选项，不消费值，出现即 true
    ///   - ExactlyOne：单值选项（默认），如 --output path
    ///   - ZeroOrMore：零或多次指定，如 --include a --include b
    ///   - OneOrMore：至少一次指定
    /// </summary>
    public ArgumentArity Arity {
        get { return _arity; }
        set { _arity = value; }
    }

    /// <summary>
    /// 便捷属性：是否为标志选项（布尔开关）。设置此属性等价于设置
    /// Arity = ZeroOrOne / ExactlyOne。读取时判断 Arity == ZeroOrOne。
    /// </summary>
    public bool IsFlag {
        get { return _arity == ArgumentArity.ZeroOrOne; }
        set { _arity = value ? ArgumentArity.ZeroOrOne : ArgumentArity.ExactlyOne; }
    }

    /// <summary>
    /// 便捷属性：是否允许多次指定。设置此属性等价于设置
    /// Arity = OneOrMore / ExactlyOne。读取时判断 Arity 是否为多值模式。
    /// </summary>
    public bool AllowMultiple {
        get {
            return _arity == ArgumentArity.ZeroOrMore || _arity == ArgumentArity.OneOrMore;
        }
        set {
            if (value) {
                _arity = ArgumentArity.OneOrMore;
            } else {
                _arity = ArgumentArity.ExactlyOne;
            }
        }
    }

    /// <summary>选项默认值（用户未提供时使用）。</summary>
    public string DefaultValue {
        get { return _defaultValue; }
        set { _defaultValue = value; }
    }

    /// <summary>获取主别名（用于帮助输出和标识）。</summary>
    public string PrimaryAlias {
        get { return _aliases[0]; }
    }

    // ── 别名管理 ──

    /// <summary>添加额外别名。</summary>
    public void AddAlias(string alias) {
        _aliases.Add(alias);
    }

    /// <summary>获取所有已注册的别名。</summary>
    public List<string> GetAliases() {
        List<string> result = new List<string>();
        int count = _aliases.Count;
        int i = 0;
        while (i < count) {
            result.Add(_aliases[i]);
            i = i + 1;
        }
        return result;
    }

    /// <summary>判断给定 token 是否匹配该选项的任一别名。</summary>
    public bool MatchesAlias(string token) {
        int count = _aliases.Count;
        int i = 0;
        while (i < count) {
            if (token == _aliases[i]) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    // ── 别名显示 ──

    /// <summary>
    /// 格式化别名列表（用于帮助输出）。
    /// 如 "--verbose, -v" 或 "--port"。
    /// </summary>
    public string ToAliasString() {
        string result = _aliases[0];
        int count = _aliases.Count;
        int i = 1;
        while (i < count) {
            result = result + ", " + _aliases[i];
            i = i + 1;
        }
        return result;
    }

    // ── 验证 ──

    /// <summary>
    /// 添加自定义验证器。
    ///
    /// 验证器接收选项值字符串，返回空串表示通过，返回非空字符串表示错误消息。
    /// 可链式添加多个验证器，按添加顺序依次执行。
    /// 对标 C# Option.AddValidator。
    /// </summary>
    public void AddValidator(Func<string, string> validator) {
        _validators.Add(validator);
    }

    /// <summary>获取所有验证器。</summary>
    public List<Func<string, string>> GetValidators() {
        List<Func<string, string>> result = new List<Func<string, string>>();
        int count = _validators.Count;
        int i = 0;
        while (i < count) {
            result.Add(_validators[i]);
            i = i + 1;
        }
        return result;
    }

    /// <summary>
    /// 限制选项只能取指定值集合。
    /// 对标 C# Option.FromAmong。
    /// </summary>
    public void FromAmong(List<string> values) {
        _allowedValues.Clear();
        int count = values.Count;
        int i = 0;
        while (i < count) {
            _allowedValues.Add(values[i]);
            i = i + 1;
        }
    }

    /// <summary>获取允许值列表（FromAmong 设置的值；空列表表示不限制）。</summary>
    public List<string> GetAllowedValues() {
        List<string> result = new List<string>();
        int count = _allowedValues.Count;
        int i = 0;
        while (i < count) {
            result.Add(_allowedValues[i]);
            i = i + 1;
        }
        return result;
    }

    /// <summary>
    /// 对候选值执行完整验证（包括 FromAmong 检查和自定义验证器）。
    /// 返回空串表示通过，非空字符串为错误消息。
    /// </summary>
    public string Validate(string value) {
        // 先检查 FromAmong 约束
        if (_allowedValues.Count > 0) {
            bool found = false;
            foreach (var allowed in _allowedValues) {
                if (value == allowed) {
                    found = true;
                    break;
                }
            }
            if (!found) {
                return "选项 " + this.PrimaryAlias + " 的值 '" + value + "' 不在允许范围内";
            }
        }

        // 再执行自定义验证器
        foreach (var validator in _validators) {
            string error = validator.Invoke(value);
            if (error != "") {
                return error;
            }
        }
        return "";
    }
}

}
