namespace Arc.Linq.Expressions;

/// while 循环语句——对应 Arc AST `Stmt::While { cond, body }`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `while (cond) { body }`。
/// 在 BlockExpression.Statements 中出现。
///
/// Cond 为循环条件表达式（必须为 bool），Body 为循环体（BlockExpression）。
///
/// 注意：L3 循环节点是「IR 表示」而非「可执行」——ORM 翻译器不识别，
/// D10.6 解释器禁用循环（D10.2 受限求值器禁止 while/for/foreach）。
/// L3 循环节点主要用于 Source Generator 等场景的代码 IR 表示。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class WhileExpression : Expression {
    public Expression Cond { get; }
    public BlockExpression Body { get; }

    public WhileExpression(Expression cond, BlockExpression body) {
        NodeType = ExpressionType.While;
        Cond = cond;
        Body = body;
    }
}
