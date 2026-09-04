namespace Arc.Linq.Expressions;

/// 拆箱表达式——对应 Arc AST `Expr::Unbox { expr, value_ty }`。
///
/// RFC 022 §2.2.10 L2 扩展节点。表示 object 引用类型→值类型的转换：
/// - 由 typeck 在 FFI `extern` 函数 `void*` 返回值处自动插入
/// - 非用户书写
/// - 类型不匹配（expected_size != payload_size）触发 panic
///
/// Operand 是被拆箱的 object 表达式，ValueTypeName 是目标值类型名。
/// 整体表达式类型为 ValueTypeName。
///
/// codegen 发射 `call i32 @rt_box_unbox(ptr, expected_size, out_ptr, out_size)` + size 校验。
///
/// 不进入 ORM 查询翻译路径——SqlTranslator 遇到此节点报「不可翻译」错误。
public class UnboxExpression : Expression {
    public Expression Operand { get; }
    public string ValueTypeName { get; }

    public UnboxExpression(Expression operand, string valueTypeName) {
        NodeType = ExpressionType.Unbox;
        Operand = operand;
        ValueTypeName = valueTypeName;
    }
}
