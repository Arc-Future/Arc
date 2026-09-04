namespace Arc.Linq.Expressions;

/// 返回语句——对应 Arc AST `Stmt::Return(Option<Spanned<Expr>>)`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `return expr;` 或 `return;`。
/// 在 BlockExpression.Statements 中出现。
///
/// Value 为可选返回值表达式（null 表示 `return;`）。
///
/// D10.6 解释器识别 ReturnExpression 执行短路返回——构造函数体中的
/// `return` 不影响注册识别（解释器忽略 Return）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class ReturnExpression : Expression {
    public Expression Value { get; }

    public ReturnExpression(Expression value) {
        NodeType = ExpressionType.Return;
        Value = value;
    }
}
