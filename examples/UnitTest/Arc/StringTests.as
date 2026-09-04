// 后置（诚实；禁止冒充 Stable）：
//   - string.Empty 静态字段；IndexOfAny / LastIndexOfAny
//   - Split(string[])；两参 count（MIR enum/int 擦除）
//   - ToLowerInvariant / ToUpperInvariant；Intern；Normalize；Copy*
//   - Format 文化感知 / FormattableString（立宪拒绝）
//   - Compare 文化感知（Arc Compare/CompareOrdinal 均为 ordinal / UTF-8 码元 strcmp）
// Trim(params char[]) / Split Options·TrimEntries / 多分隔符 params·char[] / count 三参 ✅

namespace UnitTest.Arc;

using Arc;
using Arc.QIF;
using Arc.Text;

/// <summary>
/// Arc string / StringBuilder 诚实 Stable 面单元测试（非 Fact-Skip）。
/// </summary>
public class StringTests
{
    // ── Replace ──

    [Fact]
    public void Replace_Found()
    {
        string s = "hello world";
        string replaced = s.Replace("world", "arc");
        Assert.True(replaced == "hello arc");
    }

    // ── Substring (from StringOps) ──

    [Fact]
    public void Substring_TwoArgs()
    {
        string s = "hello world";
        string sub = s.Substring(0, 5);
        Assert.True(sub == "hello");
    }

    [Fact]
    public void Substring_OneArg()
    {
        string s = "hello world";
        string tail = s.Substring(6);
        Assert.True(tail == "world");
    }

    // ── Contains (from StringOps) ──

    [Fact]
    public void Contains_Found()
    {
        string s = "hello world";
        Assert.True(s.Contains("world"));
    }

    [Fact]
    public void Contains_NotFound()
    {
        string s = "hello world";
        Assert.False(s.Contains("xyz"));
    }

    // ── IndexOf (from StringOps) ──

    [Fact]
    public void IndexOf_Found()
    {
        string s = "hello world";
        Assert.Equal(6, s.IndexOf("world"));
    }

    [Fact]
    public void IndexOf_NotFound()
    {
        string s = "hello world";
        Assert.Equal(-1, s.IndexOf("xyz"));
    }

    [Fact]
    public void IndexOf_Char()
    {
        string s = "hello";
        Assert.Equal(2, s.IndexOf('l'));
    }

    [Fact]
    public void IndexOf_FromStartIndex()
    {
        string s = "ababa";
        Assert.Equal(2, s.IndexOf("ab", 1));
        Assert.Equal(-1, s.IndexOf("ab", 3));
    }

    [Fact]
    public void IndexOf_Char_FromStartIndex()
    {
        string s = "ababa";
        Assert.Equal(2, s.IndexOf('a', 1));
        Assert.Equal(4, s.IndexOf('a', 3));
    }

    // ── StartsWith / EndsWith (from StringOps) ──

    [Fact]
    public void StartsWith_True()
    {
        string s = "hello world";
        Assert.True(s.StartsWith("hello"));
    }

    [Fact]
    public void StartsWith_False()
    {
        string s = "hello world";
        Assert.False(s.StartsWith("world"));
    }

    [Fact]
    public void StartsWith_Char()
    {
        Assert.True("hello".StartsWith('h'));
        Assert.False("hello".StartsWith('x'));
        Assert.False("".StartsWith('a'));
    }

    [Fact]
    public void EndsWith_True()
    {
        string s = "hello world";
        Assert.True(s.EndsWith("world"));
    }

    [Fact]
    public void EndsWith_False()
    {
        string s = "hello world";
        Assert.False(s.EndsWith("hello"));
    }

    [Fact]
    public void EndsWith_Char()
    {
        Assert.True("hello".EndsWith('o'));
        Assert.False("hello".EndsWith('h'));
    }

    // ── Trim (from StringOps) ──

    [Fact]
    public void Trim_Basic()
    {
        string trimmed = "  hi  ".Trim();
        Assert.True(trimmed == "hi");
    }

    // ── ToUpper / ToLower (from StringOps) ──

    [Fact]
    public void ToUpper_Basic()
    {
        string s = "hello world";
        string upper = s.ToUpper();
        Assert.True(upper == "HELLO WORLD");
    }

    [Fact]
    public void ToLower_Basic()
    {
        string s = "HELLO WORLD";
        string lower = s.ToLower();
        Assert.True(lower == "hello world");
    }

    [Fact]
    public void ToUpper_ToLower_Roundtrip()
    {
        string s = "hello world";
        string lower = s.ToUpper().ToLower();
        Assert.True(lower == s);
    }

    // ── 字符串插值 (from StringInterp) ──

    [Fact]
    public void Interp_StringVar()
    {
        string name = "Arc";
        string a = $"hello {name}";
        Assert.True(a == "hello Arc");
    }

    [Fact]
    public void Interp_IntVar()
    {
        int n = 42;
        string b = $"n={n}";
        Assert.True(b == "n=42");
    }

    [Fact]
    public void Interp_EscapeBraces()
    {
        string c = $"{{brace}}";
        Assert.True(c == "{brace}");
    }

    [Fact]
    public void Interp_MultiVar()
    {
        string name = "Arc";
        int n = 42;
        string d = $"x={n} y={name}";
        Assert.True(d == "x=42 y=Arc");
    }

    // ── PadLeft / PadRight ──

    [Fact]
    public void PadLeft_EnoughWidth()
    {
        string s = "42";
        string padded = s.PadLeft(5);
        Assert.True(padded == "   42");
    }

    [Fact]
    public void PadLeft_NoChange()
    {
        string s = "hello";
        string padded = s.PadLeft(3);
        Assert.True(padded == "hello");
    }

    [Fact]
    public void PadRight_EnoughWidth()
    {
        string s = "42";
        string padded = s.PadRight(5);
        Assert.True(padded == "42   ");
    }

    [Fact]
    public void PadLeft_Char()
    {
        Assert.True("42".PadLeft(5, '0') == "00042");
        Assert.True("hello".PadLeft(3, '*') == "hello");
    }

    [Fact]
    public void PadRight_Char()
    {
        Assert.True("42".PadRight(5, 'x') == "42xxx");
    }

    // ── Compare / CompareOrdinal ──

    [Fact]
    public void Compare_Equal()
    {
        Assert.Equal(0, "abc".Compare("abc"));
    }

    [Fact]
    public void Compare_Less()
    {
        Assert.True("abc".Compare("abd") < 0);
    }

    [Fact]
    public void Compare_Greater()
    {
        Assert.True("abd".Compare("abc") > 0);
    }

    [Fact]
    public void Compare_Static()
    {
        Assert.Equal(0, string.Compare("xy", "xy"));
        Assert.True(string.Compare("aa", "ab") < 0);
    }

    [Fact]
    public void CompareOrdinal_Static()
    {
        // Arc：CompareOrdinal ≡ ordinal UTF-8 码元 strcmp（无文化面）。
        Assert.Equal(0, string.CompareOrdinal("abc", "abc"));
        Assert.True(string.CompareOrdinal("abc", "abd") < 0);
        Assert.True(string.CompareOrdinal("abd", "abc") > 0);
        Assert.Equal(string.Compare("z", "a"), string.CompareOrdinal("z", "a"));
    }

    // ── TrimStart / TrimEnd / Trim(char) ──

    [Fact]
    public void TrimStart_Basic()
    {
        string trimmed = "  hi  ".TrimStart();
        Assert.True(trimmed == "hi  ");
    }

    [Fact]
    public void TrimEnd_Basic()
    {
        string trimmed = "  hi  ".TrimEnd();
        Assert.True(trimmed == "  hi");
    }

    [Fact]
    public void Trim_Char()
    {
        Assert.True("xxhixx".Trim('x') == "hi");
        Assert.True("---hi---".Trim('-') == "hi");
    }

    [Fact]
    public void TrimStart_Char()
    {
        Assert.True("..hi..".TrimStart('.') == "hi..");
    }

    [Fact]
    public void TrimEnd_Char()
    {
        Assert.True("..hi..".TrimEnd('.') == "..hi");
    }

    [Fact]
    public void Trim_ParamsChars()
    {
        Assert.True("xx-hi--".Trim('-', 'x') == "hi");
        Assert.True("--hi--".Trim('-', 'x') == "hi");
        Assert.True("abhiab".Trim('a', 'b') == "hi");
    }

    [Fact]
    public void Trim_CharArray()
    {
        char[] set = "x-".ToCharArray();
        Assert.True("xx-hi--".Trim(set) == "hi");
        char[] dots = ".".ToCharArray();
        Assert.True("..hi..".TrimStart(dots) == "hi..");
        Assert.True("..hi..".TrimEnd(dots) == "..hi");
    }

    [Fact]
    public void TrimStart_ParamsChars()
    {
        Assert.True("xxhiyy".TrimStart('x', 'y') == "hiyy");
    }

    [Fact]
    public void TrimEnd_ParamsChars()
    {
        Assert.True("xxhiyy".TrimEnd('x', 'y') == "xxhi");
    }

    // ── Insert ──

    [Fact]
    public void Insert_Middle()
    {
        string s = "hello";
        string result = s.Insert(5, " world");
        Assert.True(result == "hello world");
    }

    [Fact]
    public void Insert_Beginning()
    {
        string s = "world";
        string result = s.Insert(0, "hello ");
        Assert.True(result == "hello world");
    }

    // ── Remove ──

    [Fact]
    public void Remove_TwoArgs()
    {
        string s = "hello world";
        string result = s.Remove(5, 6);
        Assert.True(result == "hello");
    }

    [Fact]
    public void Remove_OneArg()
    {
        string s = "hello world";
        string result = s.Remove(5);
        Assert.True(result == "hello");
    }

    // ── LastIndexOf ──

    [Fact]
    public void LastIndexOf_Found()
    {
        string s = "hello hello";
        Assert.Equal(6, s.LastIndexOf("hello"));
    }

    [Fact]
    public void LastIndexOf_NotFound()
    {
        string s = "hello world";
        Assert.Equal(-1, s.LastIndexOf("xyz"));
    }

    [Fact]
    public void LastIndexOf_FromStartIndex()
    {
        string s = "ababa";
        Assert.Equal(2, s.LastIndexOf("ab", 2));
        Assert.Equal(0, s.LastIndexOf("ab", 1));
    }

    [Fact]
    public void LastIndexOf_Char_FromStartIndex()
    {
        string s = "ababa";
        Assert.Equal(2, s.LastIndexOf('a', 3));
        Assert.Equal(0, s.LastIndexOf('a', 1));
    }

    // ── Static helpers ──

    [Fact]
    public void IsNullOrEmpty_Empty()
    {
        Assert.True(string.IsNullOrEmpty(""));
        Assert.False(string.IsNullOrEmpty("x"));
    }

    [Fact]
    public void IsNullOrEmpty_NullAndWhitespace()
    {
        string n = null;
        Assert.True(string.IsNullOrEmpty(n));
        // 空白 ≠ 空
        Assert.False(string.IsNullOrEmpty(" "));
        Assert.False(string.IsNullOrEmpty("\t"));
    }

    [Fact]
    public void IsNullOrWhiteSpace_Whitespace()
    {
        Assert.True(string.IsNullOrWhiteSpace(""));
        Assert.True(string.IsNullOrWhiteSpace("  \t"));
        Assert.False(string.IsNullOrWhiteSpace(" a "));
    }

    [Fact]
    public void IsNullOrWhiteSpace_NullAndNewline()
    {
        string n = null;
        Assert.True(string.IsNullOrWhiteSpace(n));
        Assert.True(string.IsNullOrWhiteSpace("\n\r"));
        Assert.False(string.IsNullOrWhiteSpace("x"));
    }

    [Fact]
    public void FromCharCount_Basic()
    {
        // 直接 `FromCharCount(...) == ""` 的嵌套比较在 codegen 仍有类型瑕疵；赋值后比较诚实可用。
        string s = string.FromCharCount('x', 3);
        Assert.True(s == "xxx");
        Assert.Equal(3, s.Length);
        string empty = string.FromCharCount('a', 0);
        Assert.Equal(0, empty.Length);
    }

    [Fact]
    public void Concat_TwoStrings()
    {
        string s = string.Concat("hello", " arc");
        Assert.True(s == "hello arc");
    }

    // ── StringBuilder 最小面 ──

    [Fact]
    public void StringBuilder_Append_ToString()
    {
        StringBuilder sb = new StringBuilder();
        sb.Append("Hello").Append(", ").Append("Arc");
        Assert.True(sb.ToString() == "Hello, Arc");
        Assert.Equal(10, sb.Length);
    }

    [Fact]
    public void StringBuilder_Clear_And_Indexer()
    {
        StringBuilder sb = new StringBuilder("ab");
        Assert.True(sb[0] == 'a');
        sb[1] = 'z';
        Assert.True(sb.ToString() == "az");
        sb.Clear();
        Assert.Equal(0, sb.Length);
        Assert.True(sb.ToString() == "");
    }

    // ── Char indexer s[i] → char（UTF-8 码元，与 Length 对齐）──

    [Fact]
    public void Indexer_Ascii()
    {
        string s = "hi";
        Assert.True(s[0] == 'h');
        Assert.True(s[1] == 'i');
        Assert.Equal(2, s.Length);
    }

    [Fact]
    public void Indexer_MatchesLengthRange()
    {
        string s = "abc";
        Assert.Equal(3, s.Length);
        Assert.True(s[0] == 'a');
        Assert.True(s[2] == 'c');
    }

    [Fact]
    public void Indexer_OutOfRange_ReturnsNul()
    {
        string s = "x";
        // 越界读返回 NUL（与 StringBuilder get_Item 对齐）；负索引同。
        Assert.Equal(0, (int)s[1]);
        Assert.Equal(0, (int)s[-1]);
    }

    // ── Split / Join / ToCharArray（string[]·char[] 类型桥 · Stable 最小面）──
    // 索引单位 = UTF-8 码元（与 Length / s[i] 对齐；非 C# UTF-16）。
    // Join(char,…) / ToCharArray(start,length) ✅（越界钳制同 Substring）。
    // Split(..., StringSplitOptions) ✅（None / RemoveEmptyEntries / TrimEntries）；
    // Split(params char|char[]) 多分隔符 ✅；Split(sep, count, options) ✅。

    [Fact]
    public void Split_ByChar()
    {
        string[] parts = "a,b,c".Split(',');
        Assert.Equal(3, parts.Length);
        Assert.True(parts[0] == "a");
        Assert.True(parts[1] == "b");
        Assert.True(parts[2] == "c");
    }

    [Fact]
    public void Split_ByString()
    {
        string[] parts = "a::b::c".Split("::");
        Assert.Equal(3, parts.Length);
        Assert.True(parts[0] == "a");
        Assert.True(parts[2] == "c");
    }

    [Fact]
    public void Split_EmptyParts()
    {
        string[] parts = ",a,".Split(',');
        Assert.Equal(3, parts.Length);
        Assert.True(parts[0] == "");
        Assert.True(parts[1] == "a");
        Assert.True(parts[2] == "");
    }

    [Fact]
    public void Split_RemoveEmptyEntries()
    {
        string[] parts = ",a,,b,".Split(',', StringSplitOptions.RemoveEmptyEntries);
        Assert.Equal(2, parts.Length);
        Assert.True(parts[0] == "a");
        Assert.True(parts[1] == "b");
        string[] none = ",a,".Split(',', StringSplitOptions.None);
        Assert.Equal(3, none.Length);
        string[] byStr = "a::b::::c".Split("::", StringSplitOptions.RemoveEmptyEntries);
        Assert.Equal(3, byStr.Length);
        Assert.True(byStr[0] == "a");
        Assert.True(byStr[1] == "b");
        Assert.True(byStr[2] == "c");
    }

    [Fact]
    public void Split_TrimEntries()
    {
        string[] parts = " a , b ".Split(',', StringSplitOptions.TrimEntries);
        Assert.Equal(2, parts.Length);
        Assert.True(parts[0] == "a");
        Assert.True(parts[1] == "b");
        string[] both = " a ,  , b ".Split(',', StringSplitOptions.TrimEntries);
        // Trim 后空段仍保留（未 RemoveEmpty）；组合位在 runtime options=3 路径覆盖。
        Assert.Equal(3, both.Length);
        Assert.True(both[0] == "a");
        Assert.True(both[1] == "");
        Assert.True(both[2] == "b");
    }

    [Fact]
    public void Split_MultiSep()
    {
        string[] parts = "a,b;c".Split(',', ';');
        Assert.Equal(3, parts.Length);
        Assert.True(parts[0] == "a");
        Assert.True(parts[1] == "b");
        Assert.True(parts[2] == "c");
        char[] seps = ",".ToCharArray();
        // char[] 单分隔符集
        string[] byArr = "x,y".Split(seps);
        Assert.Equal(2, byArr.Length);
        Assert.True(byArr[0] == "x");
    }

    [Fact]
    public void Split_Count()
    {
        string[] parts = "a,b,c,d".Split(',', 2, StringSplitOptions.None);
        Assert.Equal(2, parts.Length);
        Assert.True(parts[0] == "a");
        Assert.True(parts[1] == "b,c,d");
    }

    [Fact]
    public void Join_StringArray()
    {
        string[] parts = "a,b,c".Split(',');
        Assert.True(string.Join("-", parts) == "a-b-c");
    }

    [Fact]
    public void Join_CharSep()
    {
        string[] parts = "a,b,c".Split(',');
        Assert.True(string.Join('-', parts) == "a-b-c");
        Assert.True(string.Join('|', parts) == "a|b|c");
    }

    [Fact]
    public void SplitJoin_Roundtrip()
    {
        Assert.True(string.Join("|", "x-y".Split('-')) == "x|y");
    }

    [Fact]
    public void ToCharArray_Ascii()
    {
        char[] chars = "hi".ToCharArray();
        Assert.Equal(2, chars.Length);
        Assert.True(chars[0] == 'h');
        Assert.True(chars[1] == 'i');
    }

    [Fact]
    public void ToCharArray_Utf8CodeUnits()
    {
        // U+4E2D 中 = E4 B8 AD → 3 码元；与 Length / s[i] 对齐。
        char[] chars = "中".ToCharArray();
        Assert.Equal(3, chars.Length);
        Assert.Equal(228, (int)chars[0]);
        Assert.Equal(3, "中".Length);
        Assert.Equal(228, (int)"中"[0]);
    }

    [Fact]
    public void ToCharArray_Range()
    {
        char[] mid = "hello".ToCharArray(1, 3);
        Assert.Equal(3, mid.Length);
        Assert.True(mid[0] == 'e');
        Assert.True(mid[1] == 'l');
        Assert.True(mid[2] == 'l');
    }

    [Fact]
    public void ToCharArray_Range_Clamp()
    {
        // 越界钳制同 Substring（非 C# throw）
        char[] clamp = "ab".ToCharArray(1, 99);
        Assert.Equal(1, clamp.Length);
        Assert.True(clamp[0] == 'b');
        char[] empty = "ab".ToCharArray(5, 1);
        Assert.Equal(0, empty.Length);
    }
}

