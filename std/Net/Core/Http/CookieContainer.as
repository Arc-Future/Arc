// RFC 025 M4: Arc.Net — CookieContainer 自动管理。
// 对标 C# System.Net.CookieContainer。
// 纯 Arc 代码（非 facade）。按请求 Host 分桶存储 cookie；
// GetCookieHeader 仅返回与请求 Host 匹配桶内的 cookie（跨域隔离）。

namespace Arc.Net;

using Arc.Collections;

/// <summary>
/// Cookie 容器——管理 HTTP Cookie 的存储和自动注入。
///
/// 使用方式：
///   var jar = new CookieContainer();
///   http.CookieContainer = jar;
///   // 后续请求自动携带 Cookie，响应自动存储 Set-Cookie
/// </summary>
public class CookieContainer {
    private List<string> _hosts;
    private List<string> _buckets;  // 与 _hosts 平行；桶内 "; "-separated name=value pairs

    public CookieContainer() {
        _hosts = new List<string>();
        _buckets = new List<string>();
    }

    /// <summary>从 Set-Cookie 响应头添加 Cookie（按 uri.Host 分桶）。</summary>
    public void Add(Uri uri, string setCookieHeader) {
        var c = Cookie.Parse(setCookieHeader);
        if (c.Name == "") { return; }
        int idx = _hosts.IndexOf(uri.Host);
        if (idx < 0) {
            _hosts.Add(uri.Host);
            _buckets.Add("");
            idx = _hosts.Count - 1;
        }
        this.SetCookie(idx, c.Name, c.Value);
    }

    /// <summary>获取指定 URI 的 Cookie 头值（仅返回匹配 Host 桶内的 cookie）。</summary>
    public string GetCookieHeader(Uri uri) {
        int idx = _hosts.IndexOf(uri.Host);
        if (idx < 0) { return ""; }
        return _buckets[idx];
    }

    /// <summary>清除所有 Cookie。</summary>
    public void Clear() {
        _hosts.Clear();
        _buckets.Clear();
    }

    // ── internal ──

    private void SetCookie(int idx, string name, string value) {
        string bucket = this.Remove(_buckets[idx], name);
        if (bucket == "") {
            bucket = name + "=" + value;
        } else {
            bucket = bucket + "; " + name + "=" + value;
        }
        _buckets[idx] = bucket;
    }

    private string Remove(string cookies, string name) {
        if (cookies == "") { return ""; }
        string result = "";
        int pos = 0; int len = cookies.Length;
        while (pos < len) {
            string rest = cookies.Substring(pos, len - pos);
            int semi = rest.IndexOf("; ");
            string pair = "";
            if (semi < 0) { pair = rest; pos = len; }
            else { pair = rest.Substring(0, semi); pos = pos + semi + 2; }
            int eq = pair.IndexOf("=");
            if (eq > 0) {
                string n = pair.Substring(0, eq);
                if (n != name) {
                    if (result == "") { result = pair; }
                    else { result = result + "; " + pair; }
                }
            }
        }
        return result;
    }
}
