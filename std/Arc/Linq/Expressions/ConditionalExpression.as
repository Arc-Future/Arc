namespace Arc.Linq.Expressions;

/// 三元条件（test ? ifTrue : ifFalse），字段命名对齐 System.Linq.Expressions.ConditionalExpression。
public class ConditionalExpression : Expression {
    /// <summary>条件表达式。</summary>
    public Expression Test { get; set; }

    /// <summary>条件为真时的分支。</summary>
    public Expression IfTrue { get; set; }

    /// <summary>条件为假时的分支。</summary>
    public Expression IfFalse { get; set; }

    /// <summary>构造三元条件表达式，NodeType 置为 Conditional。</summary>
    public ConditionalExpression() {
        NodeType = ExpressionType.Conditional;
    }

    /// <summary>返回条件表达式。</summary>
    public override Expression GetCond() { return Test; }
    /// <summary>返回真分支表达式。</summary>
    public override Expression GetThen() { return IfTrue; }
    /// <summary>返回假分支表达式。</summary>
    public override Expression GetElse() { return IfFalse; }

    /// <summary>按 Test 求值后分派 IfTrue/IfFalse 的 bool 结果。</summary>
    public override bool EvalBool(IEvalContext ctx) {
        if (Test.EvalBool(ctx)) { return IfTrue.EvalBool(ctx); }
        return IfFalse.EvalBool(ctx);
    }

    /// <summary>按 Test 求值后分派 IfTrue/IfFalse 的 int 结果。</summary>
    public override int EvalInt(IEvalContext ctx) {
        if (Test.EvalBool(ctx)) { return IfTrue.EvalInt(ctx); }
        return IfFalse.EvalInt(ctx);
    }

    /// <summary>按 Test 求值后分派 IfTrue/IfFalse 的 string 结果。</summary>
    public override string EvalString(IEvalContext ctx) {
        if (Test.EvalBool(ctx)) { return IfTrue.EvalString(ctx); }
        return IfFalse.EvalString(ctx);
    }
}
