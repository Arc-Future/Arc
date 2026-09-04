// RFC 025 M4: Arc.Net — HTTP 请求/响应头集合。
//
// WebHeaderCollection 提供名-值对存储，支持 Add/Set/Get/Remove 操作。
// 内部以 "\r\n" 分隔的纯文本存储，键查找不区分大小写。

namespace Arc.Net;

/// <summary>
/// HTTP 头集合——存储请求头或响应头的名-值对。
/// 键不区分大小写，同名头允许多值。
/// </summary>
public class WebHeaderCollection {
    private string _headers;

    /// <summary>构造空的头集合。</summary>
    public WebHeaderCollection() {
        _headers = "";
    }

    /// <summary>添加一个头（保留已有同名头）。</summary>
    /// <param name="name">头名称（如 "Content-Type"）。</param>
    /// <param name="value">头值。</param>
    public void Add(string name, string value) {
        if (_headers == "") {
            _headers = name + ": " + value;
        } else {
            _headers = _headers + "\r\n" + name + ": " + value;
        }
    }

    /// <summary>设置头的值（覆盖已有同名头）。</summary>
    public void Set(string name, string value) {
        this.Remove(name);
        this.Add(name, value);
    }

    /// <summary>按名称获取第一个匹配的头值。</summary>
    /// <param name="name">头名称（大小写不敏感）。</param>
    /// <returns>头值字符串；未找到返回空串。</returns>
    public string Get(string name) {
        if (_headers == "" || name == "") { return ""; }
        string lower = name.ToLower();
        int pos = 0;
        int len = _headers.Length;
        while (pos < len) {
            int lineEnd = this.IndexOfFrom(_headers, "\r\n", pos);
            string line = "";
            if (lineEnd < 0) {
                line = _headers.Substring(pos, len - pos);
                pos = len;
            } else {
                line = _headers.Substring(pos, lineEnd - pos);
                pos = lineEnd + 2;
            }
            int colonPos = line.IndexOf(": ");
            if (colonPos > 0) {
                string key = line.Substring(0, colonPos);
                if (key.ToLower() == lower) {
                    return line.Substring(colonPos + 2, line.Length - colonPos - 2);
                }
            }
        }
        return "";
    }

    /// <summary>移除指定名称的头。</summary>
    public void Remove(string name) {
        if (_headers == "" || name == "") { return; }
        string lower = name.ToLower();
        string newHeaders = "";
        int pos = 0;
        int len = _headers.Length;
        while (pos < len) {
            int lineEnd = this.IndexOfFrom(_headers, "\r\n", pos);
            string line = "";
            if (lineEnd < 0) {
                line = _headers.Substring(pos, len - pos);
                pos = len;
            } else {
                line = _headers.Substring(pos, lineEnd - pos);
                pos = lineEnd + 2;
            }
            int colonPos = line.IndexOf(": ");
            if (colonPos > 0) {
                string key = line.Substring(0, colonPos);
                if (key.ToLower() != lower) {
                    if (newHeaders == "") {
                        newHeaders = line;
                    } else {
                        newHeaders = newHeaders + "\r\n" + line;
                    }
                }
            }
        }
        _headers = newHeaders;
    }

    /// <summary>获取 Content-Length 头的数值；不存在返回 -1。</summary>
    public int ContentLength() {
        string val = this.Get("Content-Length");
        if (val == "") { return -1; }
        return this.ParseInt(val);
    }

    /// <summary>原始头文本（用于调试）。</summary>
    public string ToHeaderString() {
        return _headers;
    }

    // ── Private helpers ──

    private int IndexOfFrom(string source, string target, int start) {
        int sourceLen = source.Length;
        int targetLen = target.Length;
        if (start + targetLen > sourceLen) { return -1; }
        for (int i = start; i <= sourceLen - targetLen; i++) {
            bool found = true;
            for (int j = 0; j < targetLen; j++) {
                if (source.Substring(i + j, 1) != target.Substring(j, 1)) {
                    found = false;
                    break;
                }
            }
            if (found) { return i; }
        }
        return -1;
    }

    private int ParseInt(string s) {
        try {
            return Convert.ToInt32(s, 10);
        } catch {
            return 0;
        }
    }
}
