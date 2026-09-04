namespace Arc.Text;

/// <summary>
/// 匹配选项（对齐 <c>System.Text.RegularExpressions.RegexOptions</c>，位标志组合，与
/// <c>rt_regex.c</c> 的 RX_ICASE/MLINE/SLINE/EXPLI 位序一致）。
/// </summary>
public enum RegexOptions {
    /// <summary>无特殊选项。</summary>
    None = 0,

    /// <summary>忽略大小写（ASCII 码点折叠；非 ASCII 多字节码点折叠限于诚实边界，不折叠）。</summary>
    IgnoreCase = 1,

    /// <summary>多行模式：<c>^</c>/<c>$</c> 在每行开头/结尾匹配。</summary>
    Multiline = 2,

    /// <summary>单行模式：<c>.</c> 匹配任意字符（含换行）。</summary>
    Singleline = 4,

    /// <summary>括号分组仅命名组可捕获，普通 <c>(...)</c> 视为非捕获组。</summary>
    ExplicitCapture = 8,
}

/// <summary>
/// 正则表达式 facade（对齐 <c>System.Text.RegularExpressions</c> 常用面）。
///
/// 诚实子集（byte-oriented，按 UTF-8 码元匹配，与 <c>string.Length</c>/<c>s[i]</c>
/// 一致），经 <c>rt_regex_*</c> ABI 下发。架构为**编译型字节码 + 显式回溯 VM**，
/// 携带统一步数预算作为 ReDoS 安全阀（达到预算即诚实返回无匹配，不假绿）——
/// 补齐 C#「默认无超时、需显式 MatchTimeout」的短板，Arc 默认自带 ReDoS 保护。
///
/// 能力面（语法）：字面量(含多字节 UTF-8)、<c>.</c>(受 Singleline)、<c>*</c>/<c>+</c>/<c>?</c>
/// 与计数量词 <c>{n,m}</c>/<c>{n,}</c>/<c>{n}</c> 及其懒量词、<c>[...]</c>
/// (区间/<c>^</c> 否定/类内转义/类内简写 <c>[\d]</c>)、<c>^</c>、<c>$</c>、<c>\b \B \A \z</c>、
/// <c>(...)</c>(捕获)、<c>(?:...)</c>(非捕获)、命名组 <c>(?&lt;name&gt;...)</c>、<c>|</c>(分支)、
/// 前瞻 <c>(?=...)</c>/<c>(?!...)</c>、后瞻 <c>(?&lt;=...)</c>/<c>(?&lt;!...)</c>（定长/变长皆可）、原子组 <c>(?&gt;...)</c>、
/// 反向引用 <c>\1</c>..<c>\9</c> 与 <c>\k&lt;name&gt;</c>、综合转义全集 + <c>\xHH</c>/<c>\uHHHH</c>/<c>\0</c>、
/// 忽略大小写 <c>(?i)</c>、多行 <c>(?m)</c>、单行 <c>(?s)</c>（含作用域 <c>(?i:...)</c>）。
///
/// 替换支持组引用 <c>$0</c>..<c>$9</c> 与 <c>$$</c>(字面 <c>$</c>)。
///
/// **诚实宽度边界**：IgnoreCase 折叠与 <c>\d \w \s</c>/<c>\b</c> 限定 ASCII；
/// 非 ASCII 折叠需 Unicode CaseFolding 表（后续可扩）；计数量词需前导下限，无前导
/// 下限的 <c>{,m}</c> 视为字面 <c>{{</c>（对齐 C#）；后瞻定长/变长皆可（对齐 .NET 7+
/// backtracking 变长后瞻）；无 possessive 量词（后续可扩）。
/// </summary>
public static class Regex {
    /// <summary>判断输入中是否存在与模式匹配的子串。</summary>
    /// <param name="pattern">正则模式；null 视为空串（恒匹配）。</param>
    /// <param name="input">待匹配字符串；null 视为空串。</param>
    [Builtin(ABI = "rt_regex_is_match")]
    public static bool IsMatch(string pattern, string input) { return false; }

    /// <summary>按选项判断输入中是否存在与模式匹配的子串。</summary>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待匹配字符串。</param>
    /// <param name="options">匹配选项（可组合多标志）。</param>
    [Builtin(ABI = "rt_regex_is_match_opt")]
    public static bool IsMatch(string pattern, string input, RegexOptions options) { return false; }

    /// <summary>返回首个匹配子串；无匹配返回空串。</summary>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待匹配字符串。</param>
    [Builtin(ABI = "rt_regex_match")]
    public static string Match(string pattern, string input) { return ""; }

    /// <summary>按选项返回首个匹配子串；无匹配返回空串。</summary>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待匹配字符串。</param>
    /// <param name="options">匹配选项。</param>
    [Builtin(ABI = "rt_regex_match_opt")]
    public static string Match(string pattern, string input, RegexOptions options) { return ""; }

    /// <returns>捕获组子串；无匹配、组号越界或未捕获返回空串。</returns>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待匹配字符串。</param>
    /// <param name="groupIndex">捕获组编号（0 起，0 表示整段）。</param>
    [Builtin(ABI = "rt_regex_match_group")]
    public static string MatchGroup(string pattern, string input, int groupIndex) { return ""; }

    /// <returns>按选项捕获组子串；无匹配、组号越界或未捕获返回空串。</returns>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待匹配字符串。</param>
    /// <param name="groupIndex">捕获组编号（0 起，0 表示整段）。</param>
    /// <param name="options">匹配选项。</param>
    [Builtin(ABI = "rt_regex_match_group_opt")]
    public static string MatchGroup(string pattern, string input, int groupIndex, RegexOptions options) { return ""; }

    /// <summary>返回所有非重叠匹配子串数组；无匹配返回空数组。</summary>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待匹配字符串。</param>
    [Builtin(ABI = "rt_regex_matches")]
    public static string[] Matches(string pattern, string input) { return null; }

    /// <summary>按选项返回所有非重叠匹配子串数组；无匹配返回空数组。</summary>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待匹配字符串。</param>
    /// <param name="options">匹配选项。</param>
    [Builtin(ABI = "rt_regex_matches_opt")]
    public static string[] Matches(string pattern, string input, RegexOptions options) { return null; }

    /// <summary>将输入中所有非重叠匹配替换为替换串（支持 <c>$0</c>..<c>$9</c> 与 <c>$$</c>）。</summary>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待处理字符串。</param>
    /// <param name="replacement">替换串。</param>
    [Builtin(ABI = "rt_regex_replace")]
    public static string Replace(string pattern, string input, string replacement) { return ""; }

    /// <summary>按选项将输入中所有非重叠匹配替换为替换串。</summary>
    /// <param name="pattern">正则模式。</param>
    /// <param name="input">待处理字符串。</param>
    /// <param name="replacement">替换串。</param>
    /// <param name="options">匹配选项。</param>
    [Builtin(ABI = "rt_regex_replace_opt")]
    public static string Replace(string pattern, string input, string replacement, RegexOptions options) { return ""; }

    /// <summary>按所有非重叠匹配切割字符串（含匹配间与尾部区段，可能含空串）。</summary>
    /// <param name="pattern">正则模式（作为分隔符）。</param>
    /// <param name="input">待切割字符串。</param>
    [Builtin(ABI = "rt_regex_split")]
    public static string[] Split(string pattern, string input) { return null; }

    /// <summary>按选项、以所有非重叠匹配切割字符串（可能含空串）。</summary>
    /// <param name="pattern">正则模式（作为分隔符）。</param>
    /// <param name="input">待切割字符串。</param>
    /// <param name="options">匹配选项。</param>
    [Builtin(ABI = "rt_regex_split_opt")]
    public static string[] Split(string pattern, string input, RegexOptions options) { return null; }
}