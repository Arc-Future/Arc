namespace Arc.Linq.Expressions;

/// try-finally 语句——对应 Arc AST `Stmt::TryFinally { body, finally }`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `try { ... } finally { ... }`。
/// 在 BlockExpression.Statements 中出现。
///
/// TryBody 为 try 块（BlockExpression），FinallyBody 为 finally 块（BlockExpression）。
/// finally 块无论 try 块是否抛异常都会执行（资源清理）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class TryFinallyExpression : Expression {
    public BlockExpression TryBody { get; }
    public BlockExpression FinallyBody { get; }

    public TryFinallyExpression(BlockExpression tryBody, BlockExpression finallyBody) {
        NodeType = ExpressionType.TryFinally;
        TryBody = tryBody;
        FinallyBody = finallyBody;
    }
}
