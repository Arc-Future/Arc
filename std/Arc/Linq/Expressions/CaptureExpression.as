namespace Arc.Linq.Expressions;

/// 闭包捕获的外部变量（u => u.Age >= threshold 中的 threshold）。
/// 值快照按捕获类型分字段：IntValue / BoolValue / StringValue（与 ConstantExpression 对齐）。
public class CaptureExpression : Expression {
    /// <summary>被捕获变量的名称。</summary>
    public string Name { get; set; }

    /// <summary>捕获的整数值（ty=int）。</summary>
    public int IntValue { get; set; }

    /// <summary>捕获的布尔值（ty=bool；独立快照，非 IntValue 解释）。</summary>
    public bool BoolValue { get; set; }

    /// <summary>捕获的字符串值（ty=string）。</summary>
    public string StringValue { get; set; }

    /// <summary>构造捕获表达式，NodeType 置为 Capture。</summary>
    public CaptureExpression() {
        NodeType = ExpressionType.Capture;
        Name = "";
    }

    /// <summary>返回捕获变量名。</summary>
    /// <returns>变量名。</returns>
    public override string GetName() { return Name; }

    /// <summary>返回捕获的字符串值。</summary>
    /// <returns>字符串值。</returns>
    public override string GetStringValue() { return StringValue; }

    /// <summary>求值为捕获的整数值。</summary>
    /// <param name="ctx">求值上下文（未使用）。</param>
    /// <returns>捕获的整数值。</returns>
    public override int EvalInt(IEvalContext ctx) { return IntValue; }

    /// <summary>求值为捕获的布尔快照。</summary>
    /// <param name="ctx">求值上下文（未使用）。</param>
    /// <returns>BoolValue。</returns>
    public override bool EvalBool(IEvalContext ctx) { return BoolValue; }

    /// <summary>求值为捕获的字符串值。</summary>
    /// <param name="ctx">求值上下文（未使用）。</param>
    /// <returns>捕获的字符串值。</returns>
    public override string EvalString(IEvalContext ctx) { return StringValue; }
}
