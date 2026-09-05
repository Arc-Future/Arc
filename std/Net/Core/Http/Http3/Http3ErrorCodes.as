// Http3ErrorCodes —— 拆分自 Http3Types.as（一文件一公开类型）。
namespace Arc.Net;

/// <summary>HTTP/3 错误码（RFC 9114 §8.1）。</summary>
internal class Http3ErrorCodes {
    public const long NoError = 0x100;
    public const long ProtocolError = 0x101;
    public const long InternalError = 0x102;
    public const long StreamCreationError = 0x103;
    public const long ClosedCriticalStream = 0x104;
    public const long FrameUnexpected = 0x105;
    public const long FrameError = 0x106;
    public const long ExcessLoad = 0x107;
    public const long IdError = 0x108;
    public const long SettingsError = 0x109;
    public const long MissingSettings = 0x10a;
    public const long RequestRejected = 0x10b;
    public const long RequestCanceled = 0x10c;
    public const long RequestIncomplete = 0x10d;
    public const long MessageError = 0x10e;
    public const long ConnectError = 0x10f;
    public const long VersionFallback = 0x110;
}
