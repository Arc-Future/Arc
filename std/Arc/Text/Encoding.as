namespace Arc.Text;

/// <summary>
/// 字符编码 facade（标准库就绪战役 P0 + L2 deepen）。
///
/// 当前仅 UTF-8 Stable：Arc <c>string</c> 即为 UTF-8 NUL 终止字节序列，
/// <see cref="GetBytes"/> / <see cref="GetString"/> / <see cref="GetByteCount"/>
/// 经 <c>rt_text_utf8_*</c> ABI，非 Skip e2e 覆盖。
///
/// 其它编码（UTF-16 / Latin-1 等）与 <c>Encoding.UTF8</c> 实例属性后置；
/// 本切片表面为静态方法（与 Resources 挂账用语一致）。
/// </summary>
public static class Encoding {
    /// <summary>将字符串编码为 UTF-8 字节数组（逐字节拷贝）。</summary>
    /// <param name="s">源字符串；null 视为空串。</param>
    /// <returns>UTF-8 字节数组；空串返回 Length 0 的数组（非 null）。</returns>
    [Builtin(ABI = "rt_text_utf8_get_bytes")]
    public static byte[] GetBytes(string s) { return null; }

    /// <summary>将 UTF-8 字节数组解码为字符串。</summary>
    /// <param name="bytes">源字节；null 视为空串。含内部 0x00 时拷贝完整，
    /// 但后续依赖 strlen 的 string 运算会在首个 NUL 处截断（Arc C-string 模型）。</param>
    /// <returns>解码后的字符串。</returns>
    [Builtin(ABI = "rt_text_utf8_get_string")]
    public static string GetString(byte[] bytes) { return ""; }

    /// <summary>返回将字符串编码为 UTF-8 所需的字节数（与 GetBytes.Length / string.Length 对齐）。</summary>
    /// <param name="s">源字符串；null 视为 0。</param>
    [Builtin(ABI = "rt_text_utf8_get_byte_count")]
    public static int GetByteCount(string s) { return 0; }

    /// <summary>将字符串编码为 UTF-16LE 字节数组（无 BOM，对齐 C# <c>Encoding.Unicode.GetBytes</c>）。</summary>
    /// <param name="s">源字符串（UTF-8）；null 视为空串。超出 BMP 的码点编码为代理对。</param>
    /// <returns>UTF-16LE 字节数组；每字符 2 或 4 字节。</returns>
    [Builtin(ABI = "rt_text_utf16_get_bytes")]
    public static byte[] GetBytesUtf16(string s) { return null; }

    /// <summary>将 UTF-16LE 字节数组解码为字符串（对齐 C# <c>Encoding.Unicode.GetString</c>）。</summary>
    /// <param name="bytes">UTF-16LE 源字节；null 视为空串。支持代理对，孤立代理映射为 U+FFFD。</param>
    /// <returns>解码后的 UTF-8 字符串。</returns>
    [Builtin(ABI = "rt_text_utf16_get_string")]
    public static string GetStringUtf16(byte[] bytes) { return ""; }

    /// <summary>将字符串编码为 Latin-1（ISO-8859-1）字节数组（对齐 C# <c>Encoding.Latin1.GetBytes</c>）。</summary>
    /// <param name="s">源字符串（UTF-8）；null 视为空串。码点 &gt;0xFF 映射为 '?'。</param>
    /// <returns>Latin-1 字节数组；每字符 1 字节。</returns>
    [Builtin(ABI = "rt_text_latin1_get_bytes")]
    public static byte[] GetBytesLatin1(string s) { return null; }

    /// <summary>将 Latin-1 字节数组解码为字符串（对齐 C# <c>Encoding.Latin1.GetString</c>）。</summary>
    /// <param name="bytes">Latin-1 源字节（0x00–0xFF）；null 视为空串。</param>
    /// <returns>解码后的 UTF-8 字符串。</returns>
    [Builtin(ABI = "rt_text_latin1_get_string")]
    public static string GetStringLatin1(byte[] bytes) { return ""; }
}
