// RFC 012 M5-1: Source Generator 标记属性（D13.2，v1.0 修订）。
//
// 本文件定义 M5 Source Generator 体系的标记属性：
//   - SourceGeneratorAttribute：标记一个类为 Source Generator
//
// **设计说明（RFC D13.1/D13.2 v1.0）**：
//   - [SourceGenerator] 标记一个类为 Source Generator——编译器在
//     Pass 3 调用其 Generate(GeneratorContext) 方法
//   - 生成器类必须实现 IGenerator 接口（定义于 CodeGeneration/Generators.as）
//   - Generate 方法返回 List<GeneratedFile>，每个 GeneratedFile 的 Content
//     被解析为独立的 Arc 源文件并追加到当前编译单元；FileName 用于诊断
//     与多生成器合并去重
//
// **与 M4 的关系（RFC D13.5/D13.6）**：
//   - M4（GenerateToAttribute）注入现有类方法体；M5 生成新独立编译单元
//   - M4 与 M5 共享 Pass 3（受限求值器执行阶段）与受限子集白名单
//   - M4 与 M5 可在同一编译单元共存，互不替代
//   - M5 生成代码的 span 映射指向 Generate 方法内拼接位置（D10.4 同机制）
//
// **架构红线（RFC 012 D13.7 非目标）**：
//   - 不引入跨 TU 的 Source Generator（仅当前 TU 内执行）
//   - 不引入增量生成（首版每次编译全量执行）
//   - 不引入第三方生成器插件（仅支持标准库与用户项目内的生成器）
//   - 不引入并行执行（首版串行执行所有生成器）
//   - 不引入代码缓存（每次编译重新执行，确保确定性）

namespace Arc.CodeGeneration;

/// <summary>
/// 标记一个类为 Source Generator（RFC 009 D13.2 v1.0）。
///
/// 被标记的类必须实现 <c>IGenerator</c> 接口（定义于
/// <c>Arc.CodeGeneration.Generators</c>）。编译器在 Pass 3 调用其
/// <c>Generate(GeneratorContext)</c> 方法，返回的 <c>GeneratedFile</c>
/// 列表中每个文件的 <c>Content</c> 被解析为独立的 Arc 源文件并追加到
/// 当前编译单元；<c>FileName</c> 用于诊断与多生成器合并去重。
///
/// 合法附加目标：仅 class。
/// </summary>
[AttributeUsage(AttributeTargets.Class)]
public class SourceGeneratorAttribute : Attribute {
    public SourceGeneratorAttribute() {}
}
