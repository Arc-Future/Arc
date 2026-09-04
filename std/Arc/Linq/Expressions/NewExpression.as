namespace Arc.Linq.Expressions;

using Arc.Collections;

/// 对象构造（new User { Name = "x" }）。
public class NewExpression : Expression {
    /// <summary>构造目标类型名。</summary>
    public string TypeName { get; set; }

    /// <summary>成员初始化的参数名列表。</summary>
    public List<string> ArgNames { get; set; }

    /// <summary>成员初始化的参数值表达式列表。</summary>
    public List<Expression> ArgValues { get; set; }

    /// <summary>构造对象创建表达式，NodeType 置为 New。</summary>
    public NewExpression() {
        NodeType = ExpressionType.New;
        TypeName = "";
    }
}
