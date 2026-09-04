namespace Arc.Linq.Expressions;

using Arc.Collections;

/// 路径访问——对应 Arc AST `Expr::Path(Vec<Ident>)`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 `A.B.C` 形式的多段路径访问。
///
/// 与 MemberExpression 的区别：
/// - MemberExpression 是 `expr.MemberName`（含 receiver 表达式，receiver 可为任意 Expression）
/// - PathExpression 是 `A.B.C`（纯路径，无 receiver，全部段名为标识符）
///
/// 语义等价于 `MemberAccess(MemberAccess(... MemberAccess(null, A), B), C)`，
/// 但独立节点提升 IR 可读性，且避免引入虚假的 null receiver。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误
/// （查询谓词中的属性访问应使用 MemberExpression，receiver 为 Parameter）。
public class PathExpression : Expression {
    public List<string> Segments { get; }

    public PathExpression(List<string> segments) {
        NodeType = ExpressionType.Path;
        Segments = segments;
    }
}
