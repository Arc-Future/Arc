namespace Arc.Linq.Expressions;

/// 类型测试——对应 Arc AST `Expr::Is { expr, pattern }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。仅承载 `IsPattern::Type { ty, binding }` 形式
/// （Var/Null 模式不树化——Var 永远匹配无意义，Null 用 NullExpression 表示）。
///
/// `BindingName` 为可选绑定名（`is T name` 形式），无绑定时为空串。
/// 当 BindingName 非空时，类型测试成功的同时将 operand 绑定到该名字，
/// 在 then 分支内可见（C# 7 declaration pattern 语义）。
///
/// 用于编译期扩展（D10.6 解释器识别 `expression is ClassExpression classDef`
/// 模式匹配）与运行时类型测试（未来 variant 类型分支）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class IsExpression : Expression {
    public Expression Operand { get; }
    public string TypeName { get; }
    public string BindingName { get; }

    public IsExpression(Expression operand, string typeName, string bindingName) {
        NodeType = ExpressionType.Is;
        Operand = operand;
        TypeName = typeName;
        BindingName = bindingName;
    }
}
