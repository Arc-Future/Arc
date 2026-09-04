namespace Arc.Linq.Expressions;

using Arc.Collections;

// RFC 022 §2.2.9 扩展（RFC 032 QIF 路径触发）：
//
// MethodExpression 是 ClassExpression.Methods 列表元素的类型，表示类中一个
// 方法的签名信息。与 MethodCallExpression（方法调用表达式）语义不同——
// MethodCallExpression 表示「调用方法」这一动作（含 Target/Arguments），
// MethodExpression 表示「方法定义」这一声明（含 Parameters/ReturnType/Attributes）。
//
// 仅含方法签名，不含方法体——方法体可能包含循环/递归等不可树化语句，故不
// 暴露 Body 字段，符合 RFC 022 §2.6「Expression 不是通用编程语言 IR」约束。
//
// 与 ClassExpression 同理，MethodExpression 不进入运行时查询翻译路径，
// 仅在编译期由 D10.6 解释器构造并消费。

/// 方法定义表达式——表示类中一个方法的签名信息（不含方法体）。
///
/// 用于 ClassExpression.Methods 列表元素，供 GenerateToAttribute<T> 派生类
/// 构造函数体通过 `foreach (var method in classDef.Methods)` 遍历访问方法
/// 的 Name/Parameters/ReturnType/Attributes 信息，决定如何为每个方法注册
/// this.Build(Func<string>) 展开委托。
public class MethodExpression : Expression {
    /// <summary>方法名。</summary>
    public string Name { get; set; }

    /// <summary>方法形参列表（仅含参数名与类型，不含默认值）。</summary>
    public List<ParameterExpression> Parameters { get; set; }

    /// <summary>方法返回类型名（字符串形式，如 "void"/"int"/"string"）。</summary>
    public string ReturnType { get; set; }

    /// <summary>方法上声明的属性名列表（如 ["Fact"]、["Theory"]、["InlineData"]）。</summary>
    public List<string> Attributes { get; set; }

    /// <summary>构造方法定义表达式，NodeType 置为 Method。</summary>
    public MethodExpression() {
        NodeType = ExpressionType.Method;
        Name = "";
        ReturnType = "void";
    }

    /// <summary>返回方法名（覆写基类访问器，供通用翻译器分派）。</summary>
    /// <returns>方法名。</returns>
    public override string GetMethodName() { return this.Name; }
}
