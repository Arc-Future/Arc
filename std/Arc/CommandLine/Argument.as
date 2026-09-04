// Arc.CommandLine.Argument —— 命令行位置参数定义。
//
// 对标 C# System.CommandLine.Argument，提供声明式位置参数契约定义：
//   - 参数名（用于帮助输出）
//   - 描述文本
//   - 必选/可选标记
//   - 默认值
//   - 验证器（AddValidator）
//   - 多值支持（Arity）
//
// 位置参数按声明顺序匹配，在所有选项处理完毕后分配给剩余 token。

namespace Arc.CommandLine {

/// <summary>
/// 命令行位置参数定义。对标 C# System.CommandLine.Argument。
///
/// 位置参数按 <see cref="Command.AddArgument"/> 的顺序匹配：
/// 选项处理完毕后，剩余 token 按声明顺序依次分配给各 Argument。
/// 最后一个 Argument 若 Arity 为 ZeroOrMore / OneOrMore，消费所有剩余 token。
///
/// 用法：
/// <code>
/// Argument fileArg = new Argument("file", "输入文件路径");
/// fileArg.IsRequired = true;
/// cmd.AddArgument(fileArg);
///
/// Argument extraArg = new Argument("remaining", "额外参数");
/// extraArg.Arity = ArgumentArity.ZeroOrMore;
/// cmd.AddArgument(extraArg);
/// </code>
/// </summary>
public class Argument {
    private string _name;
    private string _description;
    private bool _isRequired;
    private string _defaultValue;
    private ArgumentArity _arity;
    private List<Func<string, string>> _validators;
    private List<string> _allowedValues;

    // ── 构造函数 ──

    /// <summary>具名位置参数。</summary>
    public Argument(string name, string description) {
        _name = name;
        _description = description;
        _isRequired = false;
        _defaultValue = "";
        _arity = ArgumentArity.ExactlyOne;
        _validators = new List<Func<string, string>>();
        _allowedValues = new List<string>();
    }

    // ── 属性 ──

    /// <summary>参数名（用于 --help 输出，如 "&lt;file&gt;"）。</summary>
    public string Name {
        get { return _name; }
    }

    /// <summary>参数描述（用于 --help 输出）。</summary>
    public string Description {
        get { return _description; }
        set { _description = value; }
    }

    /// <summary>是否为必选参数。必选参数缺失时解析报错。</summary>
    public bool IsRequired {
        get { return _isRequired; }
        set { _isRequired = value; }
    }

    /// <summary>默认值（用户未提供时使用）。</summary>
    public string DefaultValue {
        get { return _defaultValue; }
        set { _defaultValue = value; }
    }

    /// <summary>参数元数（决定可接受的值数量）。</summary>
    public ArgumentArity Arity {
        get { return _arity; }
        set { _arity = value; }
    }

    // ── 验证 ──

    /// <summary>添加自定义验证器。验证器接收参数值，返回空串 = 通过，非空 = 错误消息。</summary>
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

    /// <summary>限制参数值只能取指定集合。</summary>
    public void FromAmong(List<string> values) {
        _allowedValues.Clear();
        int count = values.Count;
        int i = 0;
        while (i < count) {
            _allowedValues.Add(values[i]);
            i = i + 1;
        }
    }

    /// <summary>获取允许值列表。</summary>
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

    /// <summary>对参数值执行完整验证。返回空串 = 通过，非空 = 错误消息。</summary>
    public string Validate(string value) {
        if (_allowedValues.Count > 0) {
            bool found = false;
            foreach (var allowed in _allowedValues) {
                if (value == allowed) {
                    found = true;
                    break;
                }
            }
            if (!found) {
                return "参数 " + _name + " 的值 '" + value + "' 不在允许范围内";
            }
        }

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
