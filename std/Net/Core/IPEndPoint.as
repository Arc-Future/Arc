// RFC 025 M4: Arc.Net — IPEndPoint 网络端点模型。
namespace Arc.Net;

/// <summary>
/// IP 端点——IP 地址 + 端口号组合。
///
/// 对标 C# System.Net.IPEndPoint。用于 Socket.Bind/Connect 等操作。
/// </summary>
public struct IPEndPoint {
    /// <summary>IP 地址。</summary>
    public IPAddress Address;

    /// <summary>端口号。</summary>
    public int Port;

    /// <summary>从 IP 地址和端口构造 IPEndPoint。</summary>
    public IPEndPoint(IPAddress address, int port) {
        this.Address = address;
        this.Port = port;
    }

    /// <summary>从主机字符串和端口构造 IPEndPoint（自动解析 IP）。</summary>
    public IPEndPoint(string host, int port) {
        this.Address = new IPAddress(host);
        this.Port = port;
    }

    /// <summary>返回 "127.0.0.1:8080" 格式。</summary>
    public string ToString() {
        return this.Address.ToString() + ":" + Convert.ToString(this.Port);
    }
}
