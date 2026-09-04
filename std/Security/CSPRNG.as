namespace Arc.Security;

using Arc.Security.Cryptography;
using Arc.Text;

/// <summary>
/// CSPRNG facade（RFC 026 M3 · 密码学安全随机数）。
/// GetBytes 返回 count 字节；系统熵源失败抛 CryptographicException
/// （绝不静默返回空数据 / 全零——P0-1 修复）。
/// </summary>
public class CSPRNG {
    /// <summary>内部 ABI：count → byte[count]；count&lt;0 或熵源失败返回 null。</summary>
    [Builtin(ABI = "rt_crypto_random_bytes_arr")]
    private static byte[] _GetBytes(int count) { return null; }

    /// <summary>
    /// 获取 count 字节密码学安全随机数。count&lt;0 抛 ArgumentOutOfRangeException；
    /// 熵源失败抛 CryptographicException。
    /// </summary>
    public static byte[] GetBytes(int count) {
        if (count < 0) {
            throw new ArgumentOutOfRangeException("count", "count must be non-negative.");
        }
        byte[] bytes = _GetBytes(count);
        if (bytes == null) {
            throw new CryptographicException("CSPRNG entropy source failed.");
        }
        return bytes;
    }

    /// <summary>字节 → lowercase hex（每字节 2 字符）。输入 null 抛 ArgumentNullException。</summary>
    public static string ToHex(byte[] data) {
        if (data == null) {
            throw new ArgumentNullException("data");
        }
        return Hex.ToHexString(data);
    }
}
