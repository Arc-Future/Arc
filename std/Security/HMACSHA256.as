namespace Arc.Security;

using Arc.Security.Cryptography;
using Arc.Text;

/// <summary>
/// HMAC-SHA256 facade（RFC 2104 over SHA-256 · L3 Stable 最小面）。
/// 输入 byte[] key/message → 32 字节 MAC；ToHex 输出 lowercase hex（64 字符）。
/// 底层失败抛 CryptographicException（报错 > 静默）。
/// 可证伪：UnitTest/Arc/SecurityTests（RFC 4231 Case 2 向量）。
/// </summary>
public class HMACSHA256 {
    /// <summary>内部 ABI：(key, message) → byte[32]；失败返回 null（公开体转为异常）。</summary>
    [Builtin(ABI = "rt_crypto_hmac_sha256_arr")]
    private static byte[] _ComputeHash(byte[] key, byte[] message) { return null; }

    /// <summary>计算 HMAC-SHA256（32 字节）。key/message 为 null 抛 ArgumentNullException。</summary>
    public static byte[] ComputeHash(byte[] key, byte[] message) {
        if (key == null) {
            throw new ArgumentNullException("key");
        }
        if (message == null) {
            throw new ArgumentNullException("message");
        }
        byte[] mac = _ComputeHash(key, message);
        if (mac == null) {
            throw new CryptographicException("HMAC-SHA256 computation failed.");
        }
        return mac;
    }

    /// <summary>字节 → lowercase hex（每字节 2 字符）。输入 null 抛 ArgumentNullException。</summary>
    public static string ToHex(byte[] data) {
        if (data == null) {
            throw new ArgumentNullException("data");
        }
        return Hex.ToHexString(data);
    }
}
