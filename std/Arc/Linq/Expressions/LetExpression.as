namespace Arc.Linq.Expressions;

/// 局部变量声明语句——对应 Arc AST `Stmt::Let { mutable, name, ty, init }`。
///
/// RFC 022 §2.2.10 L3 语句节点。表示 `var x = expr;` 或 `Type x = expr;`。
/// 在 BlockExpression.Statements 中出现。
///
/// Name 为变量名，TypeName 为可选类型标注（空串表示类型推断），Init 为初始化表达式（可空）。
///
/// 注意：Arc 不支持 `mutable` 关键字（变量默认不可变，与 Rust 不同），
/// 故 LetExpression 无 mutable 字段。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class LetExpression : Expression {
    public string Name { get; }
    public string TypeName { get; }
    public Expression Init { get; }

    public LetExpression(string name, string typeName, Expression init) {
        NodeType = ExpressionType.Let;
        Name = name;
        TypeName = typeName;
        Init = init;
    }
}
