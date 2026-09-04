namespace Arc.Text;

/// <summary>Base64 编解码门面，对齐 C# System.Convert.ToBase64String/FromBase64String。</summary>
public static class Base64 {
    /// <summary>将 UTF-8 字符串编码为 Base64 字符串。</summary>
    /// <param name="data">待编码文本。</param>
    /// <returns>Base64 编码结果。</returns>
    [Builtin(ABI = "rt_text_base64_encode")]
    public static string Encode(string data) { return ""; }

    /// <summary>将 Base64 字符串解码为原始文本。</summary>
    /// <param name="data">Base64 编码文本。</param>
    /// <returns>解码后的原始文本。</returns>
    [Builtin(ABI = "rt_text_base64_decode")]
    public static string Decode(string data) { return ""; }

    /// <summary>将字节数组编码为 Base64 字符串（对齐 C# <c>Convert.ToBase64String</c>；
    /// 按数组长度计字节，内嵌 <c>0x00</c> 不截断）。</summary>
    /// <param name="data">待编码字节。</param>
    /// <returns>Base64 编码结果。</returns>
    [Builtin(ABI = "rt_text_base64_bytes_encode")]
    public static string ToBase64String(byte[] data) { return ""; }

    /// <summary>将 Base64 字符串解码为字节数组（对齐 C# <c>Convert.FromBase64String</c>）。</summary>
    /// <param name="data">Base64 编码字符串。</param>
    /// <returns>解码后的字节数组。</returns>
    [Builtin(ABI = "rt_text_base64_bytes_decode")]
    public static byte[] FromBase64String(string data) { return null; }
}
