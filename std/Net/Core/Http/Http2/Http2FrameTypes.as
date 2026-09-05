// S2 (RFC 033 §2.4): Arc.Net — 公共类型：帧常量、头部表、响应对象。
//
// 触碰面：RFC 033 §2.2 S2 里程碑（`std/Net/Core/Http/` 内新增 `Http2` 子目录）；
// 异步单一惯用法（§1.4）。帧常量命名对齐 RFC 7540 表 3。

namespace Arc.Net;
using Arc.Collections;

/// <summary>HTTP/2 帧常量（RFC 7540 §6 表 3）。</summary>
/// <remarks>
/// 常数采用 `class + public const int`（与 std/Arc AttributeTargets 同例——本语言
/// `static int X = 常量` 的静态字段初始化器不受支持；const 在编译期折叠）。
/// </remarks>
internal class Http2FrameTypes {
    public const int Data = 0x0;
    public const int Headers = 0x1;
    public const int Priority = 0x2;
    public const int RstStream = 0x3;
    public const int Settings = 0x4;
    public const int PushPromise = 0x5;
    public const int Ping = 0x6;
    public const int GoAway = 0x7;
    public const int WindowUpdate = 0x8;
    public const int Continuation = 0x9;

    public const int FlagEndStream = 0x1;
    public const int FlagAck = 0x1;
    public const int FlagEndHeaders = 0x4;
    public const int FlagPadded = 0x8;
    public const int FlagPriority = 0x20;

    // 帧长上限（RFC 7540 §6.1——不可变，请求/响应均 16384）。
    public const int MaxFrameSize = 16384;

    // SETTINGS 参数标识（RFC 7540 §6.5.2 表 6）。
    public const int SettingsHeaderTableSize = 0x1;
    public const int SettingsEnablePush = 0x2;
    public const int SettingsMaxConcurrentStreams = 0x3;
    public const int SettingsInitialWindowSize = 0x4;
    public const int SettingsMaxFrameSize = 0x5;
    public const int SettingsMaxHeaderListSize = 0x6;

    // 常量帧长（§6.4 / §6.7 / §6.8 / §6.9）。
    public const int SettingsFrameLength = 0; // ACK；非 ACK 为 6 的倍数
    public const int PingFrameLength = 8;
    public const int GoAwayMinLength = 8;
    public const int WindowUpdateFrameLength = 4;
}
