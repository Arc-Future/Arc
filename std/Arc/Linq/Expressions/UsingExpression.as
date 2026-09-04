namespace Arc.Linq.Expressions;

/// using 语句——对应 Arc AST `Stmt::Using { name, ty, init, body }`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `using (Type name = init) { body }` 或
/// `using (var name = init) { body }`。脱糖为 `let name = init; try { body }
/// finally { name.Dispose(); }`。
/// 在 BlockExpression.Statements 中出现。
///
/// VarName 为资源变量名，TypeName 为可选类型标注（空串表示 var 推断），
/// Init 为资源初始化表达式（必须实现 IDisposable），Body 为 using 块（BlockExpression）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class UsingExpression : Expression {
    public string VarName { get; }
    public string TypeName { get; }
    public Expression Init { get; }
    public BlockExpression Body { get; }

    public UsingExpression(string varName, string typeName, Expression init, BlockExpression body) {
        NodeType = ExpressionType.Using;
        VarName = varName;
        TypeName = typeName;
        Init = init;
        Body = body;
    }
}
