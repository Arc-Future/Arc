namespace Arc.Linq.Expressions;

using Arc;

/// <summary>字段/属性访问（如 u.Age）。</summary>
///
/// 内存求值：按 MemberName 经 IEvalContext.Has/GetInt/GetBool/GetString 取值；
/// `Has` 为 false 时抛 InvalidOperationException（禁止默默返回 0，对齐 Parameter）。
public class MemberExpression : Expression {
    /// <summary>成员所属对象表达式。</summary>
    public Expression Object { get; set; }

    /// <summary>成员名（RFC 018 M3 前为字符串；M3 起为 MemberInfo.Name 别名）。</summary>
    public string MemberName { get; set; }

    /// <summary>构造成员访问表达式，NodeType 置为 Member。</summary>
    public MemberExpression() {
        NodeType = ExpressionType.Member;
        MemberName = "";
    }

    /// <summary>返回成员名。</summary>
    public override string GetMember() { return MemberName; }

    /// <summary>从上下文按成员名求值为 int。</summary>
    public override int EvalInt(IEvalContext ctx) {
        if (!ctx.Has(MemberName)) {
            throw new InvalidOperationException(
                "MemberExpression member is not bound in IEvalContext");
        }
        return ctx.GetInt(MemberName);
    }

    /// <summary>从上下文按成员名求值为 bool（走 GetBool，不经 GetInt 权宜）。</summary>
    public override bool EvalBool(IEvalContext ctx) {
        if (!ctx.Has(MemberName)) {
            throw new InvalidOperationException(
                "MemberExpression member is not bound in IEvalContext");
        }
        return ctx.GetBool(MemberName);
    }

    /// <summary>从上下文按成员名求值为 string。</summary>
    public override string EvalString(IEvalContext ctx) {
        if (!ctx.Has(MemberName)) {
            throw new InvalidOperationException(
                "MemberExpression member is not bound in IEvalContext");
        }
        return ctx.GetString(MemberName);
    }
}
