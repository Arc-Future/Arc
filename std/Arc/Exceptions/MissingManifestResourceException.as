// MissingManifestResourceException — 缺少清单资源异常（RFC 027 M0）
// 对标 C# System.Resources.MissingManifestResourceException。
namespace Arc;

/// <summary>
/// 无法找到资源时抛出。
/// </summary>
public class MissingManifestResourceException : SystemException {
    /// <summary>资源基名。</summary>
    public string BaseName { get; }

    public MissingManifestResourceException(string baseName) : base(baseName) {
        this.BaseName = baseName;
    }
}
