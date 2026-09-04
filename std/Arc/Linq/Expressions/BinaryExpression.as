namespace Arc.Linq.Expressions;

/// 二元运算（a + b, a >= b, a && b）。
///
/// RFC 022 Sprint 2d Slice 2：移除 `Op` 字符串字段，运算符由 per-op
/// `NodeType`（Add/Subtract/…/Equal/…/AndAlso/OrElse）标识——对齐
/// C# `System.Linq.Expressions`。codegen `emit_expr_tree` 以 per-op
/// 判别值写入 `NodeType`，不再写 `Op` 字段。
public class BinaryExpression : Expression {
    /// <summary>左操作数。</summary>
    public Expression Left { get; set; }

    /// <summary>右操作数。</summary>
    public Expression Right { get; set; }

    /// <summary>构造二元表达式。默认 NodeType 为 Binary（占位）；调用方/工厂置具体运算符。</summary>
    public BinaryExpression() {
        NodeType = ExpressionType.Binary;
    }

    /// <summary>返回左操作数。</summary>
    /// <returns>左操作数表达式。</returns>
    public override Expression GetLeft() { return Left; }

    /// <summary>返回右操作数。</summary>
    /// <returns>右操作数表达式。</returns>
    public override Expression GetRight() { return Right; }

    /// <summary>在上下文中求值为 bool，支持关系与逻辑运算。</summary>
    /// <param name="ctx">求值上下文。</param>
    /// <returns>关系/逻辑运算结果；NodeType 不匹配返回 false。</returns>
    public override bool EvalBool(IEvalContext ctx) {
        if (NodeType == ExpressionType.GreaterThanOrEqual) {
            return Left.EvalInt(ctx) >= Right.EvalInt(ctx);
        }
        if (NodeType == ExpressionType.GreaterThan) { return Left.EvalInt(ctx) > Right.EvalInt(ctx); }
        if (NodeType == ExpressionType.LessThan) { return Left.EvalInt(ctx) < Right.EvalInt(ctx); }
        if (NodeType == ExpressionType.LessThanOrEqual) { return Left.EvalInt(ctx) <= Right.EvalInt(ctx); }
        // == / !=：按操作数类型分派；勿一律 EvalInt（bool/string 的 IntValue 默认为 0）。
        if (NodeType == ExpressionType.Equal) {
            if (IsBoolOperand(Left) || IsBoolOperand(Right)) {
                return Left.EvalBool(ctx) == Right.EvalBool(ctx);
            }
            if (IsStringOperand(Left) || IsStringOperand(Right)) {
                return Left.EvalString(ctx) == Right.EvalString(ctx);
            }
            return Left.EvalInt(ctx) == Right.EvalInt(ctx);
        }
        if (NodeType == ExpressionType.NotEqual) {
            if (IsBoolOperand(Left) || IsBoolOperand(Right)) {
                return Left.EvalBool(ctx) != Right.EvalBool(ctx);
            }
            if (IsStringOperand(Left) || IsStringOperand(Right)) {
                return Left.EvalString(ctx) != Right.EvalString(ctx);
            }
            return Left.EvalInt(ctx) != Right.EvalInt(ctx);
        }
        if (NodeType == ExpressionType.AndAlso) { return Left.EvalBool(ctx) && Right.EvalBool(ctx); }
        if (NodeType == ExpressionType.OrElse) { return Left.EvalBool(ctx) || Right.EvalBool(ctx); }
        return false;
    }

    /// <summary>
    /// 操作数是否应按 bool 求值：TypeName=="bool"，或一元 ! / 关系与逻辑二元（结果为 bool）。
    /// </summary>
    private static bool IsBoolOperand(Expression e) {
        if (e.TypeName == "bool") { return true; }
        if (e.NodeType == ExpressionType.Not) { return true; }
        if (e.NodeType == ExpressionType.AndAlso) { return true; }
        if (e.NodeType == ExpressionType.OrElse) { return true; }
        if (e.NodeType == ExpressionType.Equal) { return true; }
        if (e.NodeType == ExpressionType.NotEqual) { return true; }
        if (e.NodeType == ExpressionType.GreaterThan) { return true; }
        if (e.NodeType == ExpressionType.GreaterThanOrEqual) { return true; }
        if (e.NodeType == ExpressionType.LessThan) { return true; }
        if (e.NodeType == ExpressionType.LessThanOrEqual) { return true; }
        return false;
    }

    /// <summary>操作数是否应按 string 求值：TypeName=="string"（含 Constant/Capture/Parameter/Member/Conditional）。</summary>
    private static bool IsStringOperand(Expression e) {
        if (e.TypeName == "string") { return true; }
        return false;
    }

    /// <summary>在上下文中求值为 int，支持算术运算。</summary>
    /// <param name="ctx">求值上下文。</param>
    /// <returns>算术运算结果；NodeType 不匹配返回 0。</returns>
    public override int EvalInt(IEvalContext ctx) {
        if (NodeType == ExpressionType.Add) { return Left.EvalInt(ctx) + Right.EvalInt(ctx); }
        if (NodeType == ExpressionType.Subtract) { return Left.EvalInt(ctx) - Right.EvalInt(ctx); }
        if (NodeType == ExpressionType.Multiply) { return Left.EvalInt(ctx) * Right.EvalInt(ctx); }
        if (NodeType == ExpressionType.Divide) { return Left.EvalInt(ctx) / Right.EvalInt(ctx); }
        return 0;
    }
}
