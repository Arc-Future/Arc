// RFC 025 M4: Arc.Net — SocketShutdown 枚举。
namespace Arc.Net;

/// <summary>
/// Socket 关闭方式（对标 C# SocketShutdown）。
/// 声明顺序对应 C# 数值：Receive=0, Send=1, Both=2。
/// </summary>
public enum SocketShutdown {
    Receive,
    Send,
    Both,
}
