namespace Arc.Linq.Expressions;

/// 一元运算（!x, -x）。
///
/// RFC 022 Sprint 2d Slice 2：移除 `Op` 字符串字段，运算符由 per-op
/// `NodeType`（Not/Negate）标识——对齐 C# `System.Linq.Expressions`。
public class UnaryExpression : Expression {
    /// <summary>操作数表达式。</summary>
    public Expression Operand { get; set; }

    /// <summary>构造一元表达式。默认 NodeType 为 Unary（占位）；调用方/工厂置具体运算符。</summary>
    public UnaryExpression() {
        NodeType = ExpressionType.Unary;
    }

    /// <summary>返回操作数表达式。</summary>
    /// <returns>操作数表达式。</returns>
    public override Expression GetOperand() { return Operand; }

    /// <summary>在上下文中求值为 bool，支持逻辑非运算。</summary>
    /// <param name="ctx">求值上下文。</param>
    /// <returns>运算结果；NodeType 不匹配返回 false。</returns>
    public override bool EvalBool(IEvalContext ctx) {
        if (NodeType == ExpressionType.Not) { return !Operand.EvalBool(ctx); }
        return false;
    }

    /// <summary>在上下文中求值为 int，支持负号运算。</summary>
    /// <param name="ctx">求值上下文。</param>
    /// <returns>运算结果；NodeType 不匹配返回 0。</returns>
    public override int EvalInt(IEvalContext ctx) {
        if (NodeType == ExpressionType.Negate) { return -Operand.EvalInt(ctx); }
        return 0;
    }
}
