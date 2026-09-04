namespace Arc.Security;

using Arc.Security.Cryptography;
using Arc.Text;

/// <summary>
/// HMAC-SHA512 facade（RFC 2104 over SHA-512）。
/// 输入 byte[] key/message → 64 字节 MAC；ToHex 输出 lowercase hex（128 字符）。
/// 底层失败抛 CryptographicException（报错 > 静默）。
/// 可证伪：UnitTest/Arc/SecurityTests（RFC 4231 HMAC-SHA-512 向量）。
/// </summary>
public class HMACSHA512 {
    /// <summary>内部 ABI：(key, message) → byte[64]；失败返回 null（公开体转为异常）。</summary>
    [Builtin(ABI = "rt_crypto_hmac_sha512_arr")]
    private static byte[] _ComputeHash(byte[] key, byte[] message) { return null; }

    /// <summary>计算 HMAC-SHA512（64 字节）。key/message 为 null 抛 ArgumentNullException。</summary>
    public static byte[] ComputeHash(byte[] key, byte[] message) {
        if (key == null) {
            throw new ArgumentNullException("key");
        }
        if (message == null) {
            throw new ArgumentNullException("message");
        }
        byte[] mac = _ComputeHash(key, message);
        if (mac == null) {
            throw new CryptographicException("HMAC-SHA512 computation failed.");
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
