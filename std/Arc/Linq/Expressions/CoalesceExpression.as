namespace Arc.Linq.Expressions;

/// 空合并表达式——对应 Arc AST `Expr::Coalesce { left, right }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 `left ?? right`：
/// - left 必须是 `T?`（Nullable 类型）
/// - left 为 null 时返回 right，否则返回 left
/// - right 必须是 `T` 或 `T?`
///
/// 语义等价于 `left.HasValue ? left.Value : right`，但独立节点提升 IR 可读性。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误
/// （查询谓词中的空合并应在应用层处理，不翻译到 SQL）。
public class CoalesceExpression : Expression {
    public Expression Left { get; }
    public Expression Right { get; }

    public CoalesceExpression(Expression left, Expression right) {
        NodeType = ExpressionType.Coalesce;
        Left = left;
        Right = right;
    }
}
