namespace Arc.Linq.Expressions;

using Arc.Collections;

/// switch 表达式——对应 Arc AST `Expr::Switch(SwitchExpr)`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 `switch (scrutinee) { case pattern: body; default: body; }`。
/// 含 Scrutinee（被匹配的表达式）+ Cases（case 列表）。
///
/// 每个 SwitchCase 含 Pattern（可选，None 表示 default 分支）+ Body（BlockExpression）。
/// Pattern 类型对齐 Arc AST Pattern enum：Wildcard/Ident/Literal/Struct/Variant。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class SwitchExpression : Expression {
    public Expression Scrutinee { get; }
    public List<SwitchCaseExpression> Cases { get; }

    public SwitchExpression(Expression scrutinee, List<SwitchCaseExpression> cases) {
        NodeType = ExpressionType.Switch;
        Scrutinee = scrutinee;
        Cases = cases;
    }
}

/// switch case 子句——含 Pattern（可选，None=default）+ Body。
public class SwitchCaseExpression : Expression {
    public Expression Pattern { get; }
    public BlockExpression Body { get; }

    public SwitchCaseExpression(Expression pattern, BlockExpression body) {
        NodeType = ExpressionType.Block;
        Pattern = pattern;
        Body = body;
    }
}
