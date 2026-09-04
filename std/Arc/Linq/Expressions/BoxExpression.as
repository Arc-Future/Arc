namespace Arc.Linq.Expressions;

/// 装箱表达式——对应 Arc AST `Expr::Box { expr, value_ty }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示值类型→object 引用类型的隐式转换：
/// - 由 typeck 在 FFI `extern` 函数调用的 `void*` 形参处自动插入
/// - 非用户书写
///
/// Operand 是被装箱的源值表达式，ValueTypeName 是源值类型名。
/// 整体表达式类型为 `object`。
///
/// codegen 发射 `call ptr @rt_box_create(size, align)` + `@llvm.memcpy` + `@rt_arc_inc`。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class BoxExpression : Expression {
    public Expression Operand { get; }
    public string ValueTypeName { get; }

    public BoxExpression(Expression operand, string valueTypeName) {
        NodeType = ExpressionType.Box;
        Operand = operand;
        ValueTypeName = valueTypeName;
    }
}
