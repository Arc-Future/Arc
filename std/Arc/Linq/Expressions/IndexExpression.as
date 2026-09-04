namespace Arc.Linq.Expressions;

using Arc;

/// 索引访问（arr[0] 或 dict["key"]）。
///
/// 内存求值（RFC 022 §9.4.8）：Object 须为 Parameter/Member（经 GetName/GetMember
/// 得到集合名），Index 求值为 int，经 IEvalContext.HasAt/Get*At 取值；未绑定硬错误。
public class IndexExpression : Expression {
    /// <summary>被索引的对象表达式。</summary>
    public Expression Object { get; set; }

    /// <summary>索引表达式。</summary>
    public Expression Index { get; set; }

    /// <summary>构造索引访问表达式，NodeType 置为 Index。</summary>
    public IndexExpression() {
        NodeType = ExpressionType.Index;
    }

    /// <summary>在上下文中按索引求值为 int。</summary>
    public override int EvalInt(IEvalContext ctx) {
        string name = Object.GetMember();
        if (name == "") { name = Object.GetName(); }
        int idx = Index.EvalInt(ctx);
        if (!ctx.HasAt(name, idx)) {
            throw new InvalidOperationException(
                "IndexExpression is not bound in IEvalContext");
        }
        return ctx.GetIntAt(name, idx);
    }

    /// <summary>在上下文中按索引求值为 bool。</summary>
    public override bool EvalBool(IEvalContext ctx) {
        string name = Object.GetMember();
        if (name == "") { name = Object.GetName(); }
        int idx = Index.EvalInt(ctx);
        if (!ctx.HasAt(name, idx)) {
            throw new InvalidOperationException(
                "IndexExpression is not bound in IEvalContext");
        }
        return ctx.GetBoolAt(name, idx);
    }

    /// <summary>在上下文中按索引求值为 string。</summary>
    public override string EvalString(IEvalContext ctx) {
        string name = Object.GetMember();
        if (name == "") { name = Object.GetName(); }
        int idx = Index.EvalInt(ctx);
        if (!ctx.HasAt(name, idx)) {
            throw new InvalidOperationException(
                "IndexExpression is not bound in IEvalContext");
        }
        return ctx.GetStringAt(name, idx);
    }
}
