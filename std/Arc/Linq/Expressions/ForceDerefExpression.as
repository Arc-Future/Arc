namespace Arc.Linq.Expressions;

/// 强制解引用表达式——对应 Arc AST `Expr::ForceDeref { access }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 `receiver!.member` 或 `receiver!.method(args)`：
/// - receiver 为 null 时触发 panic（运行时错误）
/// - receiver 非 null 时返回 `receiver.access` 的值
///
/// 与 NullConditionalExpression 对立：`?.` 是空条件（安全），`!.` 是强制解引用（断言非空）。
///
/// Access 字段是 MemberExpression 或 MethodCallExpression（receiver 已设置）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class ForceDerefExpression : Expression {
    public Expression Access { get; }

    public ForceDerefExpression(Expression access) {
        NodeType = ExpressionType.ForceDeref;
        Access = access;
    }
}
