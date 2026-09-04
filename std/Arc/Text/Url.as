namespace Arc.Text;

/// <summary>
/// URL 百分号编解码门面（对齐 C# System.Net.WebUtility.UrlEncode/UrlDecode）。
///
/// 输入按 Arc UTF-8 字符串逐字节处理：
/// - Encode：非保留字符（A-Z a-z 0-9 - _ . ~）原样保留，空格编码为 `+`，
///   其余字节（含非 ASCII 的 UTF-8 多字节）百分号编码为 `%HH`（大写十六进制）。
/// - Decode：`+` 还原为空格，`%HH` 还原为对应字节（大小写十六进制均可），
///   未配对 `%` 原样保留。
/// </summary>
public static class Url {
    /// <summary>按 UTF-8 字节对字符串做百分号编码（空格→`+`，其余非保留字符→`%HH`）。</summary>
    /// <param name="value">待编码文本；null 视为空串。</param>
    /// <returns>百分号编码结果。</returns>
    [Builtin(ABI = "rt_text_url_encode")]
    public static string Encode(string value) { return ""; }

    /// <summary>还原百分号编码（`+`→空格，`%HH`→字节）。</summary>
    /// <param name="value">编码文本；null 视为空串。</param>
    /// <returns>解码后的原始文本。</returns>
    [Builtin(ABI = "rt_text_url_decode")]
    public static string Decode(string value) { return ""; }
}