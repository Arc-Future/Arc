// RFC 022 Sprint 2a: Expression 类层次结构。
//
// Expression 是 Arc 一等公民类型（运行时表达式树节点），由 ORM 框架在运行时
// 遍历、翻译、执行。编译期树化由 typeck 完成（TypeId::Expression），生成
// 运行时构造这些节点对象的代码（Sprint 2b）。
//
// 设计原则：表达式树是领域无关的通用 AST 数据结构。
//   - 不包含任何 SQL/领域特定逻辑（无 Translate/NeedsSelect 等方法）
//   - 通过 NodeType 字段 + 虚方法访问器暴露结构信息
//   - 具体翻译器（SqlTranslator 等）通过访问器遍历树并生成目标产物
//   - 利用 vtable 虚方法分派绕过 Arc 不支持 is/as 下转的限制
//
// 注意：
//   - `Expression<T>`（带泛型参数）是 typeck 的内置类型（TypeId::Expression），
//     用作 Lambda 表达式树化的目标类型，与本文件的 `Expression` 类层次不同。
//   - 本文件定义运行时表达式树节点类层次，供 IQueryProvider 翻译使用。
//
// RFC 018 M3（2026-07-24 首切片）：新增 `Type Type` 强类型字段；`TypeName` 过渡期保留。
// codegen `emit_expr_tree` 在可知类型名时填充 RuntimeType（无 @.typeinfo 的基元暂为 null）。
namespace Arc.Linq.Expressions;

using Arc.Reflection;

/// <summary>
/// 表达式树基类——所有表达式节点的抽象基类。
///
/// 派生类通过 NodeType 字段标识种类。翻译器（SqlTranslator 等）通过
/// NodeType 分派 + 虚方法访问器遍历树，无需 is/as 下转。
/// </summary>
public class Expression {
    /// <summary>节点类型，标识具体表达式种类。</summary>
    public ExpressionType NodeType { get; set; }

    /// <summary>
    /// 节点所表示的运行时类型（RFC 018 M3；通常为 RuntimeType）。
    /// 无可用 typeinfo 时为 null（如部分基元，待后续补齐）。
    /// </summary>
    public Type Type { get; set; }

    /// <summary>
    /// 节点所表示的运行时类型名（M3 过渡期保留；有 Type 时应等于 Type.FullName）。
    /// </summary>
    public string TypeName { get; set; }

    /// <summary>构造表达式节点，默认 NodeType 为 Constant。</summary>
    public Expression() {
        NodeType = ExpressionType.Constant;
        Type = null;
        TypeName = "";
    }

    // ---- 内存执行后端（Sprint 2c）---

    /// <summary>在给定上下文中求值为 int。</summary>
    public virtual int EvalInt(IEvalContext ctx) { return 0; }

    /// <summary>在给定上下文中求值为 bool。</summary>
    public virtual bool EvalBool(IEvalContext ctx) { return false; }

    /// <summary>在给定上下文中求值为 string。</summary>
    public virtual string EvalString(IEvalContext ctx) { return ""; }

    // ---- 通用结构访问器 ----

    /// <summary>返回方法名（MethodCallExpression 覆写）。</summary>
    public virtual string GetMethodName() { return ""; }

    /// <summary>返回调用目标表达式（MethodCallExpression 覆写）。</summary>
    public virtual Expression GetTarget() { return null; }

    /// <summary>返回首参数表达式（MethodCallExpression 覆写）。</summary>
    public virtual Expression GetArg0() { return null; }

    /// <summary>返回左操作数（BinaryExpression 覆写）。</summary>
    public virtual Expression GetLeft() { return null; }

    /// <summary>返回右操作数（BinaryExpression 覆写）。</summary>
    public virtual Expression GetRight() { return null; }

    /// <summary>返回一元操作数（UnaryExpression 覆写）。</summary>
    public virtual Expression GetOperand() { return null; }

    /// <summary>返回成员名（MemberExpression 覆写）。</summary>
    public virtual string GetMember() { return ""; }

    /// <summary>返回条件表达式（ConditionalExpression 覆写）。</summary>
    public virtual Expression GetCond() { return null; }

    /// <summary>返回真分支表达式（ConditionalExpression 覆写）。</summary>
    public virtual Expression GetThen() { return null; }

    /// <summary>返回假分支表达式（ConditionalExpression 覆写）。</summary>
    public virtual Expression GetElse() { return null; }

    /// <summary>返回被转换的表达式（CastExpression 覆写）。</summary>
    public virtual Expression GetExpr() { return null; }

    /// <summary>返回目标类型名（CastExpression 覆写）。</summary>
    public virtual string GetTargetType() { return ""; }

    /// <summary>返回函数体表达式（LambdaExpression 覆写）。</summary>
    public virtual Expression GetBody() { return null; }

    /// <summary>返回参数或单捕获变量名（ParameterExpression/CaptureExpression 覆写）。</summary>
    public virtual string GetName() { return ""; }

    /// <summary>返回值的字符串表示（ConstantExpression 覆写）。</summary>
    public virtual string GetStringValue() { return ""; }

    /// <summary>判断是否为字符串常量（ConstantExpression 覆写）。</summary>
    public virtual bool IsStringConstant() { return false; }
}
