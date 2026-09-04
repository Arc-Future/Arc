namespace Arc.Linq.Expressions;

/// 空条件访问表达式——对应 Arc AST `Expr::NullCond { access }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 `receiver?.member` 或 `receiver?.method(args)`：
/// - receiver 为 null 时整个表达式返回 null
/// - receiver 非 null 时返回 `receiver.member` 或 `receiver.method(args)` 的值
///
/// Access 字段是 MemberExpression 或 MethodCallExpression（receiver 已设置）。
///
/// 语义等价于 `receiver == null ? null : receiver.access`，但独立节点保留 ?. 语义。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class NullConditionalExpression : Expression {
    public Expression Access { get; }

    public NullConditionalExpression(Expression access) {
        NodeType = ExpressionType.NullConditional;
        Access = access;
    }
}
