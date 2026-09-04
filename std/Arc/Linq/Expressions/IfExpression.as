namespace Arc.Linq.Expressions;

using Arc.Collections;

/// if-else 表达式——对应 Arc AST `Expr::If { cond, then_branch, else_branch }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 `if (cond) { then } else { else }` 表达式形式。
/// 与 ConditionalExpression（三元 `?:`）的区别：
/// - ConditionalExpression 是纯表达式：`cond ? a : b`，then/else 是表达式
/// - IfExpression 是语句级 if：`if (cond) { stmts } else { stmts }`，then/else 是 Block
///
/// Arc parser 把 `if` 解析为 `Expr::If`（表达式形式），作为 `Stmt::Expr` 包装出现。
/// D10.6 解释器识别 IfExpression 执行条件分支。
///
/// ElseBranch 为可空——无 else 分支时为 null。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class IfExpression : Expression {
    public Expression Cond { get; }
    public BlockExpression ThenBranch { get; }
    public BlockExpression ElseBranch { get; }

    public IfExpression(Expression cond, BlockExpression thenBranch, BlockExpression elseBranch) {
        NodeType = ExpressionType.If;
        Cond = cond;
        ThenBranch = thenBranch;
        ElseBranch = elseBranch;
    }
}
