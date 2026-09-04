namespace Arc.Linq.Expressions;

/// 默认值表达式——对应 Arc AST `Expr::Default { ty }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 `default(T)`：
/// - 数值类型 → 0
/// - bool → false
/// - 引用类型（string/class/interface）→ null
/// - struct → 零初始化
///
/// TypeName 为字符串形式的目标类型名。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class DefaultExpression : Expression {
    public string TypeName { get; }

    public DefaultExpression(string typeName) {
        NodeType = ExpressionType.Default;
        TypeName = typeName;
    }
}
