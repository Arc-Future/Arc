namespace Arc.Linq.Expressions;

/// 异步等待表达式——对应 Arc AST `Expr::Await(Box<Spanned<Expr>>)`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 `await expr`：
/// - operand 必须是 `Task<T>` 类型
/// - 整个表达式类型为 T（异步等待完成后的结果）
///
/// 用于异步方法/异步 lambda 中的 await 表达式树化。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class AwaitExpression : Expression {
    public Expression Operand { get; }

    public AwaitExpression(Expression operand) {
        NodeType = ExpressionType.Await;
        Operand = operand;
    }
}
