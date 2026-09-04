// string.Split Options — 对齐 C# System.StringSplitOptions 最小面。
// None / RemoveEmptyEntries / TrimEntries（可按位或）；多分隔符 params char / count 三参。

namespace Arc {

/// <summary>
/// <c>string.Split</c> 选项（Stable 最小面）。
/// 值与 runtime <c>rt_str_split*</c> 的 <c>options</c> 对齐：
/// <c>None=0</c>、<c>RemoveEmptyEntries=1</c>、<c>TrimEntries=2</c>（可组合为 3）。
/// </summary>
public enum StringSplitOptions {
    /// <summary>保留空段（默认）。</summary>
    None = 0,

    /// <summary>丢弃空段。</summary>
    RemoveEmptyEntries = 1,

    /// <summary>对每段做空白 trim（先于 RemoveEmptyEntries）。</summary>
    TrimEntries = 2,
}

}
