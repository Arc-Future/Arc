// RFC 025 M4: Arc.Net — Cookie 结构体。
namespace Arc.Net;

/// <summary>
/// HTTP Cookie——表示一个名-值对及可选属性。
/// </summary>
public struct Cookie {
    /// <summary>Cookie 名称。</summary>
    public string Name;
    /// <summary>Cookie 值。</summary>
    public string Value;
    /// <summary>Domain 属性。</summary>
    public string Domain;
    /// <summary>Path 属性。</summary>
    public string Path;
    /// <summary>HttpOnly 标志。</summary>
    public bool HttpOnly;
    /// <summary>Secure 标志。</summary>
    public bool Secure;

    public Cookie() {
        this.Name = "";
        this.Value = "";
        this.Domain = "";
        this.Path = "/";
        this.HttpOnly = false;
        this.Secure = false;
    }

    /// <summary>从 Set-Cookie 头解析。</summary>
    public static Cookie Parse(string setCookieHeader) {
        var c = new Cookie();
        int eq = setCookieHeader.IndexOf("=");
        if (eq > 0) {
            c.Name = setCookieHeader.Substring(0, eq);
            int semi = setCookieHeader.IndexOf(";");
            if (semi > eq) {
                c.Value = setCookieHeader.Substring(eq + 1, semi - eq - 1);
                // 解析属性
                string rest = setCookieHeader.Substring(semi + 1, setCookieHeader.Length - semi - 1);
                Cookie.ParseAttrs(rest, c);
            } else {
                c.Value = setCookieHeader.Substring(eq + 1, setCookieHeader.Length - eq - 1);
            }
        }
        return c;
    }

    private static void ParseAttrs(string attrs, Cookie c) {
        int pos = 0; int len = attrs.Length;
        while (pos < len) {
            string rest = attrs.Substring(pos, len - pos);
            int semi = rest.IndexOf(";");
            string attr = "";
            if (semi < 0) { attr = rest; pos = len; }
            else { attr = rest.Substring(0, semi); pos = pos + semi + 1; }
            if (attr.StartsWith(" ")) { attr = attr.Substring(1, attr.Length - 1); }
            string lower = attr.ToLower();
            if (lower == "httponly") { c.HttpOnly = true; }
            else if (lower == "secure") { c.Secure = true; }
            else if (lower.StartsWith("path=")) { c.Path = attr.Substring(5, attr.Length - 5); }
            else if (lower.StartsWith("domain=")) { c.Domain = attr.Substring(7, attr.Length - 7); }
        }
    }

    /// <summary>生成 "name=value" 格式的 Cookie 头值。</summary>
    public string ToHeaderValue() {
        return this.Name + "=" + this.Value;
    }
}
