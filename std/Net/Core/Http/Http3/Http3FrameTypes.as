// S4 (RFC 033 §2.6): Arc.Net — 帧/设置/流类型常量（RFC 9114）。
//
// 纯 Arc 常量层，供 Http3Frame / Qpack 使用。常量命名对齐 RFC 9114
// §7.2（帧类型表 1）、§7.2.4（SETTINGS 参数）、§6.2（流类型）。

namespace Arc.Net;

/// <summary>HTTP/3 帧类型（RFC 9114 §7.2 表 1）。</summary>
internal class Http3FrameTypes {
    public const long Data = 0x00;
    public const long Headers = 0x01;
    public const long CancelPush = 0x03;
    public const long Settings = 0x04;
    public const long PushPromise = 0x05;
    public const long GoAway = 0x07;
    public const long MaxPushId = 0x0d;
    public const long PriorityUpdateRequest = 0xf0700;   // RFC 9218（后置）
}
