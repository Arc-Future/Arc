namespace Arc.Linq.Expressions;

using Arc.Reflection;

/// <summary>
/// 类型标识表达式——对应 Arc AST `Expr::TypeOf(Spanned&lt;Type&gt;)`。
///
/// RFC 022 §2.2.10 L2 扩展节点；RFC 018 M2/M5/M3：表示 `typeof(T)`。
/// 基类 `Expression.Type` 为 T 对应的 RuntimeType（由 codegen/D10.6 填充）；
/// `TypeName` 为字符串形式的类型名（过渡期别名）。
///
/// 用于 DI 容器的 `GetService(typeof(T))` 等——无需运行时反射即可获取类型元数据。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报『不可翻译』错误。
/// L2 节点当前不进入 `emit_expr_tree`（仅 L1）；Type 字段预留给 M3/D10.6 填充。
/// </summary>
public class TypeOfExpression : Expression {
    /// <summary>类型名字符串（与 Expression.TypeName / Type.FullName 对齐）。</summary>
    public string TypeName { get; }

    public TypeOfExpression(string typeName) {
        NodeType = ExpressionType.TypeOf;
        TypeName = typeName;
        // 基类 Type 由 codegen / 解释器在可知 typeinfo 时填充为 RuntimeType。
        Type = null;
    }
}
