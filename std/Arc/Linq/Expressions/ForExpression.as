namespace Arc.Linq.Expressions;

/// for/foreach 循环语句——对应 Arc AST `Stmt::For { var, iter, body }`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `for (var x in iter) { body }` 或
/// `foreach (var x in iter) { body }`。Arc parser 把 for 与 foreach 统一为 Stmt::For。
/// 在 BlockExpression.Statements 中出现。
///
/// VarName 为循环变量名，Iter 为可迭代对象表达式，Body 为循环体（BlockExpression）。
///
/// 注意：L3 循环节点是「IR 表示」——D10.6 解释器中的 foreach 是特例
/// （仅支持编译期已知集合的展开，不执行运行时循环）。L3 ForExpression 节点
/// 主要用于 Source Generator 等场景的代码 IR 表示。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class ForExpression : Expression {
    public string VarName { get; }
    public Expression Iter { get; }
    public BlockExpression Body { get; }

    public ForExpression(string varName, Expression iter, BlockExpression body) {
        NodeType = ExpressionType.For;
        VarName = varName;
        Iter = iter;
        Body = body;
    }
}
