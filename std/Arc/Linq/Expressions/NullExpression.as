namespace Arc.Linq.Expressions;

/// null 字面量——对应 Arc AST `Expr::Null`。
///
/// RFC 022 §2.2.10 L2 扩展节点。与 ConstantExpression 区分：
/// NullExpression 是独立节点，语义更明确（C# Expression.Constant(null, typeof(T))
/// 不区分 null 与其他常量）。Arc 选择独立节点以提升 IR 可读性。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误
/// （查询谓词中的 null 应使用 ConstantExpression.IsNull）。
public class NullExpression : Expression {
    public NullExpression() {
        NodeType = ExpressionType.Null;
    }
}
