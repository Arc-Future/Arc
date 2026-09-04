// RFC 018 §4.2.8: 自定义属性数据——对齐 C# System.Reflection.CustomAttributeData。
//
// 仅描述属性元数据（类型 + 构造参数 + 命名参数），**不实例化**属性对象。
// 这是元数据描述 vs 反射调用的物理边界——不持有 Attribute 实例，无法触发其逻辑。
//
// **设计偏差（vs RFC 007 §4.2.8）**：
// RFC §4.2.8 中 NamedArguments 字段类型为 `List<(string Name, object Value)>`，
// 使用 C# 元组类型。但 Arc 不支持元组类型 (T1,T2)（1.4 裁决判例库），
// 故本文件在同文件内定义辅助类 NamedArgument（含 Name + Value 字段），
// NamedArguments 字段类型改为 `List<NamedArgument>`。语义与 RFC 等价。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// 自定义属性数据——对齐 C# System.Reflection.CustomAttributeData。
///
/// 仅描述属性元数据（类型 + 构造参数 + 命名参数），**不实例化**属性对象。
/// 这是元数据描述 vs 反射调用的物理边界——不持有 Attribute 实例，无法触发其逻辑
/// （RFC 018 §3.2 / §3.3）。
/// </summary>
public class CustomAttributeData {
    /// <summary>属性类型（如 typeof(FactAttribute)）。</summary>
    public Type AttributeType { get; }

    /// <summary>
    /// 构造函数位置参数值（按声明顺序）。
    /// </summary>
    /// <remarks>
    /// 仅常量字面量（string/int/bool/typeof(T)/enum 值），禁止运行时表达式。
    /// 元素类型为 object（RFC 016 v2 的 FFI Marshal 专用根类型），
    /// 可承载值类型装箱。codegen 发射时按字面量类型分槽存储。
    /// </remarks>
    public List<object> ConstructorArguments { get; }

    /// <summary>
    /// 命名参数（字段/属性名 → 值，仅常量字面量）。
    /// </summary>
    /// <remarks>
    /// 设计偏差：RFC 007 §4.2.8 原设计为 `List&lt;(string Name, object Value)&gt;`
    /// 元组类型，但 Arc 不支持元组类型 (T1,T2)（1.4 裁决判例库），
    /// 改为 `List&lt;NamedArgument&gt;`，NamedArgument 为同文件内辅助类。
    /// </remarks>
    public List<NamedArgument> NamedArguments { get; }

    /// <summary>默认构造函数——初始化空参数列表。</summary>
    public CustomAttributeData() {
        ConstructorArguments = new List<object>();
        NamedArguments = new List<NamedArgument>();
    }
}

/// <summary>
/// 命名参数辅助类——CustomAttributeData.NamedArguments 元素类型。
///
/// **设计偏差说明**：RFC 007 §4.2.8 原设计 NamedArguments 类型为
/// `List&lt;(string Name, object Value)&gt;` 元组类型，但 Arc 不支持元组类型
/// (T1,T2)（1.4 裁决判例库），故定义此辅助类替代元组语义。
/// 字段语义与 C# 元组 (string Name, object Value) 完全等价。
/// </summary>
public class NamedArgument {
    /// <summary>命名参数名（字段名或属性名）。</summary>
    public string Name { get; }

    /// <summary>
    /// 命名参数值（仅常量字面量）。
    /// </summary>
    /// <remarks>
    /// 类型为 object（RFC 016 v2 根类型），承载常量字面量装箱：
    /// string/int/bool/typeof(T)/enum 值。
    /// </remarks>
    public object Value { get; }

    /// <summary>默认构造函数。</summary>
    public NamedArgument() {}
}
