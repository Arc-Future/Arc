namespace Arc.Text;

/// <summary>十六进制编解码门面，提供字节与十六进制字符串互转能力。</summary>
public static class Hex {
    /// <summary>将 UTF-8 字符串编码为小写十六进制字符串。</summary>
    /// <param name="data">待编码文本。</param>
    /// <returns>小写十六进制编码结果。</returns>
    [Builtin(ABI = "rt_text_hex_encode")]
    public static string Encode(string data) { return ""; }

    /// <summary>将十六进制字符串解码为原始文本。</summary>
    /// <param name="data">十六进制编码文本。</param>
    /// <returns>解码后的原始文本。</returns>
    [Builtin(ABI = "rt_text_hex_decode")]
    public static string Decode(string data) { return ""; }

    /// <summary>将字节数组编码为小写十六进制字符串（RFC 021 M1 §1.2 ⑥）。</summary>
    /// <param name="data">待编码字节。</param>
    /// <returns>小写十六进制编码结果。</returns>
    [Builtin(ABI = "rt_text_hex_bytes_encode")]
    public static string ToHexString(byte[] data) { return ""; }

    /// <summary>将十六进制字符串解码为字节数组（RFC 026 M1 §1.2 ⑥）。</summary>
    /// <param name="hex">小写或大写十六进制字符串。</param>
    /// <returns>解码后的字节数组。</returns>
    [Builtin(ABI = "rt_text_hex_bytes_decode")]
    public static byte[] FromHexString(string hex) { return null; }
}
