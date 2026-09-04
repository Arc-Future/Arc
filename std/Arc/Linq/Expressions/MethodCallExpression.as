namespace Arc.Linq.Expressions;

using Arc.Collections;

/// <summary>
/// 方法调用——查询链节点（Where/Select/OrderBy/ToList）或方法调用（如 u.Name.StartsWith("al")）。
///
/// Arg0 字段：因 List&lt;T&gt; 当前为 stub（Arguments 为 null），用 Arg0 固定槽位存放
/// 单参数查询方法的 Lambda（Where/Select/OrderBy 均只取 1 个 Lambda 参数）。
///
/// RFC 018 M3：`MethodName` 过渡期仍为字符串；M3 起新增 `MethodInfo MethodInfo` 强类型字段，
/// 本 `MethodName` 保留为 Name 别名。
/// </summary>
public class MethodCallExpression : Expression {
    /// <summary>方法名。</summary>
    public string MethodName { get; set; }

    /// <summary>调用目标表达式（查询链上游）。</summary>
    public Expression Target { get; set; }

    /// <summary>参数表达式列表。</summary>
    public List<Expression> Arguments { get; set; }

    /// <summary>首参数固定槽位，存放单参数查询方法的 Lambda。</summary>
    public Expression Arg0 { get; set; }

    /// <summary>构造方法调用表达式，NodeType 置为 Call。</summary>
    public MethodCallExpression() {
        NodeType = ExpressionType.Call;
        MethodName = "";
    }

    /// <summary>返回方法名。</summary>
    public override string GetMethodName() { return MethodName; }

    /// <summary>返回调用目标表达式。</summary>
    public override Expression GetTarget() { return Target; }

    /// <summary>返回首参数表达式。</summary>
    public override Expression GetArg0() { return Arg0; }
}
