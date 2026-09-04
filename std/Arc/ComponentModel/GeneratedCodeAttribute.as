// RFC 027 M3: 标准库属性 — 生成代码标记 [GeneratedCode]。
//
// 对标 C# System.CodeDom.Compiler.GeneratedCodeAttribute。
// 由 `arc resx generate` 等代码生成工具在生成的 .g.as 文件上自动附加，
// 标记文件由工具生成、请勿手动修改。

namespace Arc.ComponentModel;

/// <summary>
/// 标记由代码生成工具生成的代码元素。
///
/// 用法：`[GeneratedCode("arc", "1.0")]`。
/// 合法附加目标：All。
///
/// 典型使用场景：
///   - `arc resx generate` 生成的强类型资源访问器类
///   - 其他 Source Generator（RFC 012 M5）生成的代码
///   - IDE / 静态分析工具通过此标记识别自动生成代码，跳过人工审查
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class GeneratedCodeAttribute : Attribute {
    /// 生成工具名称（如 "arc"、"MyGenerator"）。
    public string Tool { get; }
    /// 生成工具版本（如 "1.0"、"2.3.1"）。
    public string Version { get; }

    /// <param name="tool">生成工具名称。</param>
    /// <param name="version">生成工具版本。</param>
    public GeneratedCodeAttribute(string tool, string version) {
        Tool = tool;
        Version = version;
    }
}
