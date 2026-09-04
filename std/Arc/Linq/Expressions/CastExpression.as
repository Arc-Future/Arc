namespace Arc.Linq.Expressions;

/// 类型转换：(double)u.Age。
///
/// 内存求值：对 int/bool/string Eval 路径透明转发操作数（同类型或已由树化标注
/// TypeName 的窄转换）；不执行运行时数值宽窄转换。
public class CastExpression : Expression {
    /// <summary>被转换的表达式。</summary>
    public Expression Expr { get; set; }

    /// <summary>目标类型名。</summary>
    public string TargetType { get; set; }

    /// <summary>构造类型转换表达式，NodeType 置为 Cast。</summary>
    public CastExpression() {
        NodeType = ExpressionType.Cast;
        TargetType = "";
    }

    /// <summary>返回被转换的表达式。</summary>
    /// <returns>被转换的表达式。</returns>
    public override Expression GetExpr() { return Expr; }

    /// <summary>返回目标类型名。</summary>
    /// <returns>目标类型名。</returns>
    public override string GetTargetType() { return TargetType; }

    /// <summary>转发操作数 EvalInt。</summary>
    public override int EvalInt(IEvalContext ctx) { return Expr.EvalInt(ctx); }

    /// <summary>转发操作数 EvalBool。</summary>
    public override bool EvalBool(IEvalContext ctx) { return Expr.EvalBool(ctx); }

    /// <summary>转发操作数 EvalString。</summary>
    public override string EvalString(IEvalContext ctx) { return Expr.EvalString(ctx); }
}
