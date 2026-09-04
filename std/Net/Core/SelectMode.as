// RFC 025 M4: Arc.Net — SelectMode 枚举。
namespace Arc.Net;

/// <summary>
/// Socket 轮询模式（对标 C# SelectMode）。
/// 声明顺序对应 C# 数值：Read=0, Write=1, Error=2。
/// </summary>
public enum SelectMode {
    Read,
    Write,
    Error,
}
