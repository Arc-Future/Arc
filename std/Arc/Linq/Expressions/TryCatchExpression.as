namespace Arc.Linq.Expressions;

/// try-catch 语句——对应 Arc AST `Stmt::TryCatch { try_body, catch_ty, catch_name, catch_body }`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `try { ... } catch (T name) { ... }`。
/// 在 BlockExpression.Statements 中出现。
///
/// TryBody 为 try 块（BlockExpression），CatchTypeName 为捕获异常类型名，
/// CatchVarName 为捕获变量名，CatchBody 为 catch 块（BlockExpression）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class TryCatchExpression : Expression {
    public BlockExpression TryBody { get; }
    public string CatchTypeName { get; }
    public string CatchVarName { get; }
    public BlockExpression CatchBody { get; }

    public TryCatchExpression(
        BlockExpression tryBody,
        string catchTypeName,
        string catchVarName,
        BlockExpression catchBody
    ) {
        NodeType = ExpressionType.TryCatch;
        TryBody = tryBody;
        CatchTypeName = catchTypeName;
        CatchVarName = catchVarName;
        CatchBody = catchBody;
    }
}
