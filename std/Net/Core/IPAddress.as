// RFC 025 M4: Arc.Net — IPAddress 网络地址模型。
// 对标 C# System.Net.IPAddress（.NET 9）。
// 纯 Arc 代码（非 facade）。

namespace Arc.Net;

/// <summary>
/// IP 地址——表示 IPv4 或 IPv6 地址。
///
/// 支持字符串格式解析和输出。内部以字符串存储。
/// </summary>
public struct IPAddress {
    private string _address;

    /// <summary>原始 IP 地址字符串。</summary>
    public string AddressString { get { return _address; } }

    /// <summary>从字符串构造 IPAddress。</summary>
    public IPAddress(string address) {
        _address = address;
    }

    /// <summary>IPv4 环回地址 127.0.0.1。</summary>
    public static IPAddress Loopback() {
        return new IPAddress("127.0.0.1");
    }

    /// <summary>IPv6 环回地址 ::1。</summary>
    public static IPAddress IPv6Loopback() {
        return new IPAddress("::1");
    }

    /// <summary>任意地址 0.0.0.0。</summary>
    public static IPAddress Any() {
        return new IPAddress("0.0.0.0");
    }

    /// <summary>IPv6 任意地址 ::。</summary>
    public static IPAddress IPv6Any() {
        return new IPAddress("::");
    }

    /// <summary>返回 "127.0.0.1" 格式的 IP 字符串。</summary>
    public string ToString() {
        return _address;
    }
}
