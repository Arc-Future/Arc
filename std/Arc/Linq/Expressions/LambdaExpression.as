namespace Arc.Linq.Expressions;

using Arc.Collections;

/// Lambda 表达式（u => u.Age >= 18）。
public class LambdaExpression : Expression {
    /// <summary>参数列表。</summary>
    public List<ParameterExpression> Parameters { get; set; }

    /// <summary>函数体表达式。</summary>
    public Expression Body { get; set; }

    /// <summary>构造 Lambda 表达式，NodeType 置为 Lambda。</summary>
    public LambdaExpression() {
        NodeType = ExpressionType.Lambda;
    }

    /// <summary>返回函数体表达式。</summary>
    /// <returns>函数体表达式。</returns>
    public override Expression GetBody() { return Body; }

    /// <summary>对函数体求值为 int。</summary>
    /// <param name="ctx">求值上下文。</param>
    /// <returns>函数体的整数值。</returns>
    public override int EvalInt(IEvalContext ctx) { return Body.EvalInt(ctx); }

    /// <summary>对函数体求值为 bool。</summary>
    /// <param name="ctx">求值上下文。</param>
    /// <returns>函数体的布尔值。</returns>
    public override bool EvalBool(IEvalContext ctx) { return Body.EvalBool(ctx); }

    /// <summary>对函数体求值为 string。</summary>
    /// <param name="ctx">求值上下文。</param>
    /// <returns>函数体的字符串值。</returns>
    public override string EvalString(IEvalContext ctx) { return Body.EvalString(ctx); }
}
