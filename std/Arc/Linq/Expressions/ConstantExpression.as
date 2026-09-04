namespace Arc.Linq.Expressions;

/// 常量表达式（u.Age >= 18 中的 18）。
public class ConstantExpression : Expression {
    /// <summary>整数值。</summary>
    public int IntValue { get; set; }

    /// <summary>浮点数值。</summary>
    public double FloatValue { get; set; }

    /// <summary>布尔值。</summary>
    public bool BoolValue { get; set; }

    /// <summary>字符串值。</summary>
    public string StringValue { get; set; }

    /// <summary>是否为字符串常量。</summary>
    public bool IsString { get; set; }

    /// <summary>构造常量表达式，NodeType 置为 Constant。</summary>
    public ConstantExpression() {
        NodeType = ExpressionType.Constant;
    }

    /// <summary>返回字符串值。</summary>
    /// <returns>字符串值。</returns>
    public override string GetStringValue() { return StringValue; }

    /// <summary>判断是否为字符串常量。</summary>
    /// <returns>是字符串常量返回 true；否则返回 false。</returns>
    public override bool IsStringConstant() { return IsString; }

    /// <summary>求值为整数值。</summary>
    /// <param name="ctx">求值上下文（未使用）。</param>
    /// <returns>整数值。</returns>
    public override int EvalInt(IEvalContext ctx) { return IntValue; }

    /// <summary>求值为布尔值。</summary>
    /// <param name="ctx">求值上下文（未使用）。</param>
    /// <returns>布尔值。</returns>
    public override bool EvalBool(IEvalContext ctx) { return BoolValue; }

    /// <summary>求值为字符串值。</summary>
    /// <param name="ctx">求值上下文（未使用）。</param>
    /// <returns>字符串值。</returns>
    public override string EvalString(IEvalContext ctx) { return StringValue; }
}
