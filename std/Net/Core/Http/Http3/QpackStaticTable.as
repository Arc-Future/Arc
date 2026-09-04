// S4 (RFC 033 §2.6): Arc.Net — RFC 9204 §A.1 QPACK 静态表（99 项）。
//
// 纯 Arc 常量表，供 Qpack 编解码使用。索引 0..98 对齐 RFC 9204 §A.1
//（索引 0 = ":authority"）；Find/FindName 未命中返回 -1（索引 0 是合法命中）。

namespace Arc.Net;

/// <summary>RFC 9204 §A.1 静态表——索引 0..98。</summary>
internal class QpackStaticTable {
    private static string[] _names;
    private static string[] _values;

    /// <summary>懒初始化静态表（Arc 无 static ctor，显式 Ensure）。</summary>
    internal static void Ensure() {
        if (_names != null) { return; }
        _names = [
            // 0..9
            ":authority", ":path", "age", "content-disposition", "content-length",
            "cookie", "date", "etag", "if-modified-since", "if-none-match",
            // 10..14
            "last-modified", "link", "location", "referer", "set-cookie",
            // 15..21
            ":method", ":method", ":method", ":method", ":method", ":method", ":method",
            // 22..28
            ":scheme", ":scheme",
            ":status", ":status", ":status", ":status", ":status",
            // 29..35
            "accept", "accept",
            "accept-encoding", "accept-ranges",
            "access-control-allow-headers", "access-control-allow-headers",
            "access-control-allow-origin",
            // 36..43
            "cache-control", "cache-control", "cache-control", "cache-control", "cache-control", "cache-control",
            "content-encoding", "content-encoding",
            // 44..54
            "content-type", "content-type", "content-type", "content-type", "content-type", "content-type",
            "content-type", "content-type", "content-type", "content-type", "content-type",
            // 55..62
            "range",
            "strict-transport-security", "strict-transport-security", "strict-transport-security",
            "vary", "vary",
            "x-content-type-options", "x-xss-protection",
            // 63..71
            ":status", ":status", ":status", ":status", ":status", ":status", ":status", ":status", ":status",
            // 72..82
            "accept-language",
            "access-control-allow-credentials", "access-control-allow-credentials",
            "access-control-allow-headers",
            "access-control-allow-methods", "access-control-allow-methods", "access-control-allow-methods",
            "access-control-expose-headers",
            "access-control-request-headers",
            "access-control-request-method", "access-control-request-method",
            // 83..98
            "alt-svc", "authorization", "content-security-policy", "early-data", "expect-ct", "forwarded",
            "if-range", "origin", "purpose", "server", "timing-allow-origin", "upgrade-insecure-requests",
            "user-agent", "x-forwarded-for", "x-frame-options", "x-frame-options"
        ];
        _values = [
            // 0..9
            "", "/", "0", "", "0",
            "", "", "", "", "",
            // 10..14
            "", "", "", "", "",
            // 15..21
            "CONNECT", "DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT",
            // 22..28
            "http", "https",
            "103", "200", "304", "404", "503",
            // 29..35
            "*/*", "application/dns-message",
            "gzip, deflate, br", "bytes",
            "cache-control", "content-type",
            "*",
            // 36..43
            "max-age=0", "max-age=2592000", "max-age=604800", "no-cache", "no-store", "public, max-age=31536000",
            "br", "gzip",
            // 44..54
            "application/dns-message", "application/javascript", "application/json", "application/x-www-form-urlencoded", "image/gif", "image/jpeg",
            "image/png", "text/css", "text/html; charset=utf-8", "text/plain", "text/plain;charset=utf-8",
            // 55..62
            "bytes=0-",
            "max-age=31536000", "max-age=31536000; includesubdomains", "max-age=31536000; includesubdomains; preload",
            "accept-encoding", "origin",
            "nosniff", "1; mode=block",
            // 63..71
            "100", "204", "206", "302", "400", "403", "421", "425", "500",
            // 72..82
            "",
            "FALSE", "TRUE",
            "*",
            "get", "get, post, options", "options",
            "content-length",
            "content-type",
            "get", "post",
            // 83..98
            "clear", "", "script-src 'none'; object-src 'none'; base-uri 'none'", "1", "", "",
            "", "", "prefetch", "", "*", "1",
            "", "", "deny", "sameorigin"
        ];
    }

    /// <summary>静态表项数（99）。</summary>
    internal static int EntryCount() {
        Ensure();
        return _names.Length - 1;
    }

    /// <summary>索引 → 头名（索引 0 / 越界返回空串）。</summary>
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

    /// <summary>头名（小写）+ 值 → 静态表索引（-1 表示未命中）。</summary>
    internal static int Find(string name, string value) {
        Ensure();
        string[] names = _names;
        string[] values = _values;
        int i = 0;
        while (i < names.Length) {
            if (names[i] == name && values[i] == value) { return i; }
            i = i + 1;
        }
        return -1;
    }

    /// <summary>仅头名 → 静态表首个匹配索引（-1 表示未命中）。</summary>
    internal static int FindName(string name) {
        Ensure();
        string[] names = _names;
        int i = 0;
        while (i < names.Length) {
            if (names[i] == name) { return i; }
            i = i + 1;
        }
        return -1;
    }
}
