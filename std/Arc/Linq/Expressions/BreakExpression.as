namespace Arc.Linq.Expressions;

/// 循环中断语句——对应 Arc AST `Stmt::Break`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `break;`。
/// 在 BlockExpression.Statements 中出现，用于中断 WhileExpression/ForExpression 循环。
///
/// 无字段——break 仅是控制流标记。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class BreakExpression : Expression {
    public BreakExpression() {
        NodeType = ExpressionType.Break;
    }
}
