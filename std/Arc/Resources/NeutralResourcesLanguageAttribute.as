// RFC 027 M0: 标准库本地化 — 默认资源文化标记 [NeutralResourcesLanguage]。
//
// 对标 C# System.Resources.NeutralResourcesLanguageAttribute。

namespace Arc.Resources;

/// <summary>
/// 标记程序集的默认资源文化。
///
/// 用法：`[assembly: NeutralResourcesLanguage("en-US")]`。
/// 合法附加目标：Assembly。
///
/// 语义：标记 neutral `.resx`（如 `Messages.resx`）使用哪种文化编写，
/// ResX CodeGen 生成访问器时文化回退链到达 neutral 即完成，无需继续回退。
/// </summary>
[AttributeUsage(AttributeTargets.Assembly)]
public class NeutralResourcesLanguageAttribute : Attribute {
    /// <summary>默认文化的 BCP 47 名称（如 "en-US"）。</summary>
    public string CultureName { get; }

    /// <param name="cultureName">BCP 47 文化名称。</param>
    public NeutralResourcesLanguageAttribute(string cultureName) {
        this.CultureName = cultureName;
    }
}
