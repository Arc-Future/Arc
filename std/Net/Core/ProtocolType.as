// RFC 025 M4: Arc.Net — ProtocolType 枚举。
namespace Arc.Net;

/// <summary>传输协议类型。声明顺序对应 C# 数值：Tcp=0, Udp=1。</summary>
public enum ProtocolType {
    Tcp,
    Udp,
}
