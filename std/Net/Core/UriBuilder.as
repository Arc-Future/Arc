// RFC 025 M4: Arc.Net — UriBuilder URL 构造器。
//
// 对标 C# System.UriBuilder。提供编程式 URL 构造。

namespace Arc.Net;

/// <summary>
/// URL 构造器——程序化构建和修改 URI。
///
/// 使用方式：
///   var b = new UriBuilder();
///   b.Scheme = "https";
///   b.Host = "api.example.com";
///   b.Path = "/v1/users";
///   b.Query = "page=1";
///   var uri = b.ToUri();
/// </summary>
public struct UriBuilder {
    /// <summary>URL 方案。</summary>
    public string Scheme;
    /// <summary>主机名。</summary>
    public string Host;
    /// <summary>端口号。</summary>
    public int Port;
    /// <summary>路径。</summary>
    public string Path;
    /// <summary>查询字符串（不含前导 "?"）。</summary>
    public string Query;
    /// <summary>片段（不含前导 "#"）。</summary>
    public string Fragment;

    /// <summary>创建空的 UriBuilder。</summary>
    public UriBuilder() {
        this.Scheme = "http";
        this.Host = "localhost";
        this.Port = -1;
        this.Path = "/";
        this.Query = "";
        this.Fragment = "";
    }

    /// <summary>从现有 Uri 创建 UriBuilder。</summary>
    public UriBuilder(Uri uri) {
        this.Scheme = uri.Scheme;
        this.Host = uri.Host;
        this.Port = uri.Port;
        this.Path = uri.AbsolutePath;
        this.Query = uri.Query.StartsWith("?") ? uri.Query.Substring(1, uri.Query.Length - 1) : "";
        this.Fragment = uri.Fragment.StartsWith("#") ? uri.Fragment.Substring(1, uri.Fragment.Length - 1) : "";
    }

    /// <summary>构建 Uri。</summary>
    public Uri ToUri() {
        string url = this.Scheme + "://" + this.Host;
        int defaultPort = this.Scheme == "https" ? 443 : 80;
        if (this.Port > 0 && this.Port != defaultPort) {
            url = url + ":" + Convert.ToString(this.Port);
        }
        url = url + this.Path;
        if (this.Query != "") {
            url = url + "?" + this.Query;
        }
        if (this.Fragment != "") {
            url = url + "#" + this.Fragment;
        }
        return new Uri(url);
    }
}
