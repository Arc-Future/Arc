// RFC 012 M3: 标准库属性 — 类型转换器 [TypeConverter]。
//
// 对标 C# System.ComponentModel.TypeConverterAttribute。

namespace Arc.ComponentModel;

/// <summary>
/// 指定属性所使用的类型转换器类型名。
///
/// 用法：`[TypeConverter("Arc.UI.Converters.ColorConverter")]`。
/// 合法附加目标：All。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class TypeConverterAttribute : Attribute {
    /// 类型转换器的完整类型名。
    public string ConverterTypeName { get; }

    public TypeConverterAttribute(string converterTypeName) {
        ConverterTypeName = converterTypeName;
    }
}
