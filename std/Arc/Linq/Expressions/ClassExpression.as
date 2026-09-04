namespace Arc.Linq.Expressions;

using Arc.Collections;

// RFC 022 §2.2.9 扩展（RFC 032 QIF 路径触发）：
//
// ClassExpression 是 GenerateToAttribute<T> 派生类构造函数 Expression 参数的
// 具体节点类型之一——当派生属性标注的容器类为 typeof(包含 [Fact] 方法的类) 时，
// typeck 在编译期将该类树化为 ClassExpression 对象，传递给派生属性构造函数。
//
// 派生属性构造函数体由 typeck 编译期解释器（RFC 009 M4 D10.6）执行：
//   if (expression is ClassExpression classDef) {
//       foreach (var method in classDef.Methods) {
//           this.Build(() => { ... });
//       }
//   }
//
// 与查询翻译路径正交——ClassExpression 不进入运行时 IQueryable 翻译流程，
// 不被 SqlTranslator 等翻译器遍历，也不进入 codegen emit_expr_tree.rs 发射
// 路径。它仅在编译期由 D10.6 解释器构造并消费，供 GenerateTo 派生类构造函数
// 体通过 `is` 模式匹配下转后访问 Methods 列表。
//
// 设计原则：与 RFC 022 §2.6「Expression 不是通用编程语言 IR」约束不冲突——
// ClassExpression 不携带方法体（仅含签名信息），不引入循环/递归/赋值能力，
// 仍是「可树化表达式子集」的 IR 节点。

/// 类定义表达式——表示一个类的完整类型定义（仅签名信息，不含方法体）。
///
/// 用于 GenerateToAttribute<T> 派生类构造函数接收的 Expression 参数：
/// typeof(标注了派生属性的容器类) 在编译期树化为 ClassExpression，
/// 派生类构造函数体由 typeck 编译期解释器（D10.6）执行，通过
/// `expression is ClassExpression classDef` 模式匹配下转后遍历
/// classDef.Methods 调用 this.Build(Func<string>) 注册展开委托。
public class ClassExpression : Expression {
    /// <summary>类名（不含命名空间前缀）。</summary>
    public string ClassName { get; set; }

    /// <summary>类中定义的方法签名列表（仅 MethodExpression 节点，不含方法体）。</summary>
    public List<MethodExpression> Methods { get; set; }

    /// <summary>类上声明的属性名列表（如 ["Fact"]、["Theory"]）。</summary>
    public List<string> Attributes { get; set; }

    /// <summary>构造类定义表达式，NodeType 置为 Class。</summary>
    public ClassExpression() {
        NodeType = ExpressionType.Class;
        ClassName = "";
    }
}
