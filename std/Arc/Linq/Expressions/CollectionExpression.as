namespace Arc.Linq.Expressions;

using Arc.Collections;

/// 集合表达式——对应 Arc AST `Expr::CollectionExpr { elements }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 C# 12 集合表达式 `[e1, e2, ...]`：
/// 目标类型从声明上下文或元素类型推断。
///
/// Elements 为元素表达式列表。
///
/// 用于编译期扩展中表示集合字面量（如 `[Fact, Theory]` 属性列表）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class CollectionExpression : Expression {
    public List<Expression> Elements { get; }

    public CollectionExpression(List<Expression> elements) {
        NodeType = ExpressionType.Collection;
        Elements = elements;
    }
}
