namespace Arc.Linq.Expressions;

/// 基类实例引用——对应 Arc AST `Expr::Base`。
///
/// RFC 022 §2.2.10 L2 扩展节点。用于编译期扩展识别派生类构造函数体中
/// `base(...)` 调用——base 关键字树化为 BaseExpression。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class BaseExpression : Expression {
    public BaseExpression() {
        NodeType = ExpressionType.Base;
    }
}
