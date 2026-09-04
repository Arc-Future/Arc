// S2 (RFC 033 §2.4): Arc.Net — RFC 7541 §A 静态表（61 项）。
//
// 纯 Arc 常量表，供 HPACK 编解码使用。索引 1..61 对齐 RFC 7541；索引 0 未用
//（占位空串，保证数组下标与索引一一对应）。
//
// 诚实边界：完整 61 项静态表（HPACK 解码必做）；动态表（§B 最小）在 Hpack.as。

namespace Arc.Net;

/// <summary>RFC 7541 §A 静态表——索引 1..61，索引 0 未用。</summary>
internal class HpackStaticTable {
    private static string[] _names;
    private static string[] _values;

    /// <summary>懒初始化静态表（Arc 无 static ctor，显式 Ensure）。</summary>
    internal static void Ensure() {
        if (_names != null) { return; }
        _names = [
            "",
            ":authority", ":method", ":method", ":path", ":path",
            ":scheme", ":scheme", ":status", ":status", ":status",
            ":status", ":status", ":status", ":status",
            "accept-charset", "accept-encoding", "accept-language", "accept-ranges", "accept",
            "access-control-allow-origin", "age", "allow", "authorization", "cache-control",
            "content-disposition", "content-encoding", "content-language", "content-length", "content-location",
            "content-range", "content-type", "cookie", "date", "etag",
            "expect", "expires", "from", "host", "if-match",
            "if-modified-since", "if-none-match", "if-range", "if-unmodified-since", "last-modified",
            "link", "location", "max-forwards", "proxy-authenticate", "proxy-authorization",
            "range", "referer", "refresh", "retry-after", "server",
            "set-cookie", "strict-transport-security", "transfer-encoding", "user-agent", "vary",
            "via", "www-authenticate"
        ];
        _values = [
            "",
            "", "GET", "POST", "/", "/index.html",
            "http", "https", "200", "204", "206",
            "304", "400", "404", "500",
            "", "gzip, deflate", "", "", "",
            "", "", "", "", "",
            "", "", "", "", "",
            "", "", "", "", "",
            "", "", "", "", "",
            "", "", "", "", "",
            "", "", "", "", "",
            "", "", "", "", "",
            "", "", "", "", "",
            "", ""
        ];
    }

    /// <summary>静态表项数（61）。</summary>
    internal static int EntryCount() {
        Ensure();
        return _names.Length - 1;
    }

    /// <summary>索引 → 头名（索引 0 返回空串）。</summary>
    internal static string GetName(int index) {
        Ensure();
        string[] names = _names;
        if (index < 0 || index >= names.Length) { return ""; }
        return names[index];
    }

    /// <summary>索引 → 头值（无值返回空串）。</summary>
    internal static string GetValue(int index) {
        Ensure();
        string[] values = _values;
        if (index < 0 || index >= values.Length) { return ""; }
        return values[index];
    }

    /// <summary>头名（小写）+ 值 → 静态表索引（0 表示未命中）。</summary>
    internal static int Find(string name, string value) {
        Ensure();
        string[] names = _names;
        string[] values = _values;
        int i = 1;
        while (i < names.Length) {
            if (names[i] == name && values[i] == value) { return i; }
            i = i + 1;
        }
        return 0;
    }

    /// <summary>仅头名 → 静态表首个匹配索引（0 表示未命中）。</summary>
    internal static int FindName(string name) {
        Ensure();
        string[] names = _names;
        int i = 1;
        while (i < names.Length) {
            if (names[i] == name) { return i; }
            i = i + 1;
        }
        return 0;
    }
}
