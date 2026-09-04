namespace Arc.Linq.Expressions;

/// 抛异常语句——对应 Arc AST `Stmt::Throw { expr }`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `throw expr;`。
/// 在 BlockExpression.Statements 中出现。
///
/// Value 为要抛出的异常对象表达式（必须派生自 System.Exception 或 Arc 等价类）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class ThrowExpression : Expression {
    public Expression Value { get; }

    public ThrowExpression(Expression value) {
        NodeType = ExpressionType.Throw;
        Value = value;
    }
}
