namespace Arc.Linq.Expressions;

using Arc;

/// Lambda 参数引用（u => u.Age 中的 u）。
///
/// 内存求值：按 Name 经 IEvalContext.Has/GetInt/GetBool/GetString 取实参；
/// `Has` 为 false 时抛 InvalidOperationException（禁止默默返回 0）。
public class ParameterExpression : Expression {
    /// <summary>参数名。</summary>
    public string Name { get; set; }

    /// <summary>构造参数表达式，NodeType 置为 Parameter。</summary>
    public ParameterExpression() {
        NodeType = ExpressionType.Parameter;
        Name = "";
    }

    /// <summary>返回参数名。</summary>
    /// <returns>参数名。</returns>
    public override string GetName() { return Name; }

    /// <summary>从上下文按形参名求值为 int。</summary>
    public override int EvalInt(IEvalContext ctx) {
        if (!ctx.Has(Name)) {
            throw new InvalidOperationException(
                "ParameterExpression is not bound in IEvalContext");
        }
        return ctx.GetInt(Name);
    }

    /// <summary>从上下文按形参名求值为 bool。</summary>
    public override bool EvalBool(IEvalContext ctx) {
        if (!ctx.Has(Name)) {
            throw new InvalidOperationException(
                "ParameterExpression is not bound in IEvalContext");
        }
        return ctx.GetBool(Name);
    }

    /// <summary>从上下文按形参名求值为 string。</summary>
    public override string EvalString(IEvalContext ctx) {
        if (!ctx.Has(Name)) {
            throw new InvalidOperationException(
                "ParameterExpression is not bound in IEvalContext");
        }
        return ctx.GetString(Name);
    }
}
