// RFC 025 M4: Arc.Net — SocketType 枚举。
namespace Arc.Net;

/// <summary>Socket 类型。声明顺序对应 C# 数值：Stream=0, Dgram=1。</summary>
public enum SocketType {
    Stream,
    Dgram,
}
