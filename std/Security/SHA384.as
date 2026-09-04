namespace Arc.Security;

using Arc.Security.Cryptography;
using Arc.Text;

/// <summary>
/// SHA-384 hash facade（RFC 026 M3 · FIPS 180-4，SHA-512 截断 384 位）。
/// 输入 byte[] → 48 字节摘要；ToHex 输出 lowercase hex（96 字符）。
/// 底层失败抛 CryptographicException（报错 > 静默）。
/// 可证伪：UnitTest/Arc/SecurityTests（NIST CAVP SHA-384ShortMsg 向量）。
/// </summary>
public class SHA384 {
    /// <summary>内部 ABI：byte[] → byte[48]；失败返回 null（公开体转为异常）。</summary>
    [Builtin(ABI = "rt_crypto_sha384_arr")]
    private static byte[] _ComputeHash(byte[] data) { return null; }

    /// <summary>计算 SHA-384 摘要（48 字节）。输入 null 抛 ArgumentNullException。</summary>
    public static byte[] ComputeHash(byte[] data) {
        if (data == null) {
            throw new ArgumentNullException("data");
        }
        byte[] digest = _ComputeHash(data);
        if (digest == null) {
            throw new CryptographicException("SHA-384 computation failed.");
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
