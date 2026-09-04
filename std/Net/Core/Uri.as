// RFC 025 M4: Arc.Net — Uri 统一资源标识符。
//
// 对标 C# System.Uri（.NET 9）。提供结构化 URL 解析、构造、相对解析。
// 纯 Arc 代码（非 facade）。
//
// 支持格式：scheme://[user:pass@]host[:port][/path][?query][#fragment]

namespace Arc.Net;

/// <summary>
/// 统一资源标识符——结构化 URL 表示。
///
/// 使用方式：
///   var uri = new Uri("http://example.com:8080/path?q=1#sec");
///   uri.Scheme   // "http"
///   uri.Host     // "example.com"
///   uri.Port     // 8080
///   uri.AbsolutePath // "/path"
///   uri.Query    // "?q=1"
/// </summary>
public class Uri {
    /// <summary>原始 URL 字符串。</summary>
    public string OriginalString { get; }

    /// <summary>URL 方案（如 "http"、"https"）。</summary>
    public string Scheme;
    /// <summary>用户名（可选）。</summary>
    public string UserInfo;
    /// <summary>主机名或 IP 地址。</summary>
    public string Host;
    /// <summary>端口号（未指定时使用方案默认值）。</summary>
    public int Port;
    /// <summary>绝对路径（如 "/api/users"）。</summary>
    public string AbsolutePath;
    /// <summary>查询字符串（含前导 "?"）。</summary>
    public string Query;
    /// <summary>片段标识符（含前导 "#"）。</summary>
    public string Fragment;

    /// <summary>是否为绝对 URI。</summary>
    public bool IsAbsoluteUri { get { return this.Scheme != ""; } }

    /// <summary>完整 URI 字符串。</summary>
    public string AbsoluteUri {
        get { return this.ToString(); }
    }

    /// <summary>主机:端口 组合。</summary>
    public string Authority {
        get {
            string a = this.Host;
            int defaultPort = this.Scheme == "https" ? 443 : 80;
            if (this.Port > 0 && this.Port != defaultPort) {
                a = a + ":" + Convert.ToString(this.Port);
            }
            return a;
        }
    }

    /// <summary>路径 + 查询字符串。</summary>
    public string PathAndQuery {
        get { return this.AbsolutePath + this.Query; }
    }

    // ── 构造函数 ──

    /// <summary>从字符串解析 URL。</summary>
    public Uri(string uriString) {
        OriginalString = uriString;
        this.Parse(uriString);
    }

    /// <summary>基于 Base URI 解析相对 URI。</summary>
    public Uri(Uri baseUri, string relativeUri) {
        if (relativeUri.StartsWith("http://") || relativeUri.StartsWith("https://")) {
            OriginalString = relativeUri;
            this.Parse(relativeUri);
        } else {
            string combined = "";
            if (relativeUri.StartsWith("/")) {
                combined = baseUri.Scheme + "://" + baseUri.Authority + relativeUri;
            } else if (relativeUri.StartsWith("?")) {
                combined = baseUri.Scheme + "://" + baseUri.Authority + baseUri.AbsolutePath + relativeUri;
            } else if (relativeUri.StartsWith("#")) {
                combined = baseUri.AbsoluteUri.Split("#")[0] + relativeUri;
            } else {
                string basePath = baseUri.AbsolutePath;
                if (!basePath.EndsWith("/")) {
                    int lastSlash = basePath.LastIndexOf("/");
                    if (lastSlash >= 0) {
                        basePath = basePath.Substring(0, lastSlash + 1);
                    }
                }
                combined = baseUri.Scheme + "://" + baseUri.Authority + basePath + relativeUri;
            }
            OriginalString = combined;
            this.Parse(combined);
        }
    }

    // ── 公共方法 ──

    /// <summary>返回完整 URL 字符串。</summary>
    public string ToString() {
        string s = this.Scheme + "://";
        if (this.UserInfo != "") { s = s + this.UserInfo + "@"; }
        s = s + this.Authority;
        s = s + this.AbsolutePath;
        s = s + this.Query;
        s = s + this.Fragment;
        return s;
    }

    /// <summary>返回端口号（未指定时返回 -1）。</summary>
    public int GetPort() {
        int defaultPort = this.Scheme == "https" ? 443 : 80;
        if (this.Port == defaultPort) { return -1; }
        return this.Port;
    }

    // ── 静态方法 ──

    /// <summary>尝试解析 URL；失败返回 null。</summary>
    public static Uri TryCreate(string uriString) {
        if (uriString == "" || (!uriString.StartsWith("http://") && !uriString.StartsWith("https://") && !uriString.StartsWith("/"))) {
            return null;
        }
        return new Uri(uriString);
    }

    // ── 解析引擎 ──

    private void Parse(string url) {
        string remaining = url;

        // 1. 方案
        if (remaining.StartsWith("https://")) {
            this.Scheme = "https";
            remaining = remaining.Substring(8, remaining.Length - 8);
        } else if (remaining.StartsWith("http://")) {
            this.Scheme = "http";
            remaining = remaining.Substring(7, remaining.Length - 7);
        } else {
            this.Scheme = "";
            // relative path — treat as path only
            this.Port = 0;
            this.Host = "";
            this.ParsePathAndRest(remaining);
            return;
        }

        // 2. authority 段边界（先于 userinfo：@ 只能在 authority 段内搜索，
        //    否则 query 中的 "?email=a@b.com" 会污染 userinfo/host）
        int slashPos = remaining.IndexOf("/");
        int qmPos = remaining.IndexOf("?");
        int hashPos = remaining.IndexOf("#");
        int authorityEnd = remaining.Length;
        if (slashPos >= 0 && slashPos < authorityEnd) { authorityEnd = slashPos; }
        if (qmPos >= 0 && qmPos < authorityEnd) { authorityEnd = qmPos; }
        if (hashPos >= 0 && hashPos < authorityEnd) { authorityEnd = hashPos; }
        string authority = remaining.Substring(0, authorityEnd);
        remaining = slashPos >= 0 ? remaining.Substring(slashPos, remaining.Length - slashPos) : "";

        // 3. 用户信息（可选；authority 段内取 LAST @——userinfo 自身可含 @，
        //    如 user:p@ss@host → userinfo="user:p@ss"、host="host"）
        int atPos = authority.LastIndexOf("@");
        if (atPos > 0) {
            this.UserInfo = authority.Substring(0, atPos);
            authority = authority.Substring(atPos + 1, authority.Length - atPos - 1);
        } else {
            this.UserInfo = "";
        }

        // 4. Host[:port]
        if (authority.StartsWith("[")) {
            // IPv6 字面量 [::1]——端口冒号必须取 "]" 之后的那个
            int rb = authority.IndexOf("]");
            if (rb > 0) {
                this.Host = authority.Substring(1, rb - 1);
                this.Port = 0;
                if (rb + 1 < authority.Length) {
                    string rest = authority.Substring(rb + 1, authority.Length - rb - 1);
                    if (rest.StartsWith(":")) {
                        this.Port = this.ParseInt(rest.Substring(1, rest.Length - 1));
                    }
                }
            } else {
                this.Host = authority;
                this.Port = 0;
            }
        } else {
            int colonPos = authority.IndexOf(":");
            if (colonPos > 0) {
                this.Host = authority.Substring(0, colonPos);
                this.Port = this.ParseInt(authority.Substring(colonPos + 1, authority.Length - colonPos - 1));
            } else {
                this.Host = authority;
                this.Port = 0;
            }
        }

        // 默认端口
        if (this.Port == 0) {
            this.Port = this.Scheme == "https" ? 443 : 80;
        }

        // 5. 路径 + 查询 + 片段
        this.ParsePathAndRest(remaining);
    }

    private void ParsePathAndRest(string rest) {
        if (rest == "") {
            this.AbsolutePath = "/";
            this.Query = "";
            this.Fragment = "";
            return;
        }

        // 查找 ? 和 #
        int qPos = rest.IndexOf("?");
        int hPos = rest.IndexOf("#");

        if (qPos < 0 && hPos < 0) {
            this.AbsolutePath = rest;
            this.Query = "";
            this.Fragment = "";
        } else if (qPos >= 0 && (hPos < 0 || qPos < hPos)) {
            this.AbsolutePath = rest.Substring(0, qPos);
            if (hPos > qPos) {
                this.Query = rest.Substring(qPos, hPos - qPos);
                this.Fragment = rest.Substring(hPos, rest.Length - hPos);
            } else {
                this.Query = rest.Substring(qPos, rest.Length - qPos);
                this.Fragment = "";
            }
        } else {
            this.AbsolutePath = hPos >= 0 ? rest.Substring(0, hPos) : rest;
            this.Query = "";
            this.Fragment = hPos >= 0 ? rest.Substring(hPos, rest.Length - hPos) : "";
        }
        if (this.AbsolutePath == "") { this.AbsolutePath = "/"; }
    }

    // ── 工具 ──

    private int ParseInt(string s) {
        try {
            return Convert.ToInt32(s, 10);
        } catch {
            return 0;
        }
    }
}
