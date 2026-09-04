namespace Arc.Linq.Expressions;

/// 赋值语句——对应 Arc AST `Stmt::Assign { target, value }`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `target = value;`。
/// 在 BlockExpression.Statements 中出现。
///
/// Target 为左值表达式（通常为 Ident 或 Field），Value 为右值表达式。
///
/// 注意：Arc 表达式树中赋值是语句级节点，不是表达式（与 C# 不同）。
/// 这意味着赋值不能嵌套在表达式中（如 `a = b = 1` 不允许）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class AssignExpression : Expression {
    public Expression Target { get; }
    public Expression Value { get; }

    public AssignExpression(Expression target, Expression value) {
        NodeType = ExpressionType.Assign;
        Target = target;
        Value = value;
    }
}
