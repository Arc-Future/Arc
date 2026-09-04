namespace Arc.Linq.Expressions;

/// 当前实例引用——对应 Arc AST `Expr::This`。
///
/// RFC 022 §2.2.10 L2 扩展节点。用于编译期扩展（D10.6 解释器/Source Generator）
/// 识别 `this.Build(...)`/`this.Register(...)` 等调用——派生类构造函数体中
/// `this` 关键字树化为 ThisExpression，作为 MethodCallExpression.Target。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class ThisExpression : Expression {
    public ThisExpression() {
        NodeType = ExpressionType.This;
    }
}
