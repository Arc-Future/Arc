// RFC 025 M4: Arc.Net — Dns 静态门面类。
// 对标 C# System.Net.Dns（.NET 9）。提供主机名解析和本机主机名查询。
// Resolve 返回单地址；GetHostEntry/GetHostAddresses 返回多地址（空格分隔）。

namespace Arc.Net;

/// <summary>
/// DNS 解析器——提供主机名↔IP 地址的映射查询。
/// 对标 C# Dns 类。纯静态方法，操作同步阻塞（底层调用 OS getaddrinfo/gethostname）。
/// </summary>
public class Dns {
    /// <summary>解析主机名到首个 IP 地址字符串。</summary>
    /// <param name="host">待解析的主机名（如 "example.com"）。</param>
    /// <returns>IP 地址字符串（如 "93.184.216.34"）；失败返回 null。</returns>
    [Builtin(ABI = "rt_dns_resolve")]
    public static string Resolve(string host) { return ""; }

    /// <summary>获取本机的主机名。</summary>
    /// <returns>本机主机名字符串。</returns>
    [Builtin(ABI = "rt_dns_get_host_name")]
    public static string GetHostName() { return ""; }

    /// <summary>解析主机名并返回所有 IP 地址（空格分隔）。</summary>
    /// <param name="host">待解析的主机名或 IP 地址。</param>
    /// <returns>空格分隔的 IP 地址字符串；失败返回 null。</returns>
    [Builtin(ABI = "rt_dns_resolve_all")]
    public static string GetHostAddresses(string host) { return ""; }

    /// <summary>解析主机名并返回完整的 IPHostEntry。</summary>
    /// <param name="host">待解析的主机名。</param>
    /// <returns>包含主机名和地址列表的 IPHostEntry。</returns>
    [Builtin(ABI = "rt_dns_resolve_all")]
    public static IPHostEntry GetHostEntry(string host) { return null; }
}
