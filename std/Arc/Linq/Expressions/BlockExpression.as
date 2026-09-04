namespace Arc.Linq.Expressions;

using Arc.Collections;

/// 语句块——对应 Arc AST `Expr::Block(Block)` 或构造函数体/方法体。
///
/// RFC 022 §2.2.10 L2 扩展节点。含 L3 语句序列 + 可选 tail 表达式
/// （块最后一个表达式的值，无 tail 时块值为 void/unit）。
///
/// 用于 D10.6 解释器承载派生类构造函数体、Build lambda 体等——
/// 这些代码块含 `if`/`foreach`/`is` 等控制流，需要 BlockExpression
/// 统一表示语句序列。
///
/// Statements 中的元素可为任意 L2/L3 节点（IfExpression/ForExpression/
/// LetExpression/AssignExpression/ReturnExpression 等）。Tail 为可选的
/// 单个 Expression，表示块的返回值（Lambda body 的最后一个表达式）。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误
/// （查询谓词是纯表达式，不含语句块）。
public class BlockExpression : Expression {
    public List<Expression> Statements { get; }
    public Expression Tail { get; }

    public BlockExpression(List<Expression> statements, Expression tail) {
        NodeType = ExpressionType.Block;
        Statements = statements;
        Tail = tail;
    }
}
