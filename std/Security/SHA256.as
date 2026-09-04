namespace Arc.Security;

using Arc.Security.Cryptography;
using Arc.Text;

/// <summary>
/// SHA-256 hash facade（RFC 026 M3 · FIPS 180-4）。
/// 输入 byte[] → 32 字节摘要；ToHex 输出 lowercase hex（64 字符）。
/// 底层失败抛 CryptographicException（报错 > 静默）。
/// 可证伪：UnitTest/Arc/SecurityTests（NIST CAVP SHA-256ShortMsg 向量）。
/// </summary>
public class SHA256 {
    /// <summary>内部 ABI：byte[] → byte[32]；失败返回 null（公开体转为异常）。</summary>
    [Builtin(ABI = "rt_crypto_sha256_arr")]
    private static byte[] _ComputeHash(byte[] data) { return null; }

    /// <summary>计算 SHA-256 摘要（32 字节）。输入 null 抛 ArgumentNullException。</summary>
    public static byte[] ComputeHash(byte[] data) {
        if (data == null) {
            throw new ArgumentNullException("data");
        }
        byte[] digest = _ComputeHash(data);
        if (digest == null) {
            throw new CryptographicException("SHA-256 computation failed.");
        }
        return digest;
    }

    /// <summary>字节 → lowercase hex（每字节 2 字符）。输入 null 抛 ArgumentNullException。</summary>
    public static string ToHex(byte[] data) {
        if (data == null) {
            throw new ArgumentNullException("data");
        }
        return Hex.ToHexString(data);
    }
}
