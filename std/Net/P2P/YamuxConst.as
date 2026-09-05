// YamuxConst —— yamux/1.0.0 协议常量（拆分自 Yamux.as）。
namespace Arc.Net.P2P;
using Arc;
using Arc.Net;
using Arc.Collections;
using Arc.Collections.Concurrent;
using Arc.Threading;
using Arc.Text;

/// <summary>yamux/1.0.0 协议常量（类型/标志/窗口/帧长）。</summary>
internal class YamuxConst {
    public const int TypeData = 0;
    public const int TypeWindowUpdate = 1;
    public const int TypePing = 2;
    public const int TypeGoAway = 3;

    public const int FlagSyn = 1;
    public const int FlagAck = 2;
    public const int FlagFin = 4;
    public const int FlagRst = 8;

    public const int DefaultWindow = 262144;   // 256 KiB（对齐 go-yamux 默认）
    public const int MaxFrame = 65536;         // 单帧载荷上限（超限分片写）
    public const int MaxReadFrame = 16777216;  // 读帧载荷上限（16 MiB，防恶意长度）
    public const int HeaderSize = 12;
}
