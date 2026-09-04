// RFC 025 M4: Arc.Net — AddressFamily 枚举。
namespace Arc.Net;

/// <summary>网络地址族。声明顺序对应 C# 数值：InterNetwork=0, InterNetworkV6=1。</summary>
public enum AddressFamily {
    InterNetwork,
    InterNetworkV6,
}
