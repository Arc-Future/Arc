// CryptographicException — 密码学操作失败异常（RFC 026 M3）
// 对标 C# System.Security.Cryptography.CryptographicException。
// Security 哈希 / HMAC / CSPRNG 门面在底层熵源失败或计算失败时抛出（报错 > 静默）。
namespace Arc.Security.Cryptography;

/// <summary>
/// 密码学操作失败时抛出（CSPRNG 熵源失败、摘要计算失败等）。
/// </summary>
public class CryptographicException : SystemException {
    public CryptographicException() : base() { }
    public CryptographicException(string message) : base(message) { }
}
