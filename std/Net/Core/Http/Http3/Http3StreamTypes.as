// Http3StreamTypes —— 拆分自 Http3Types.as（一文件一公开类型）。
namespace Arc.Net;

/// <summary>HTTP/3 单向流类型（RFC 9114 §6.2）。</summary>
internal class Http3StreamTypes {
    public const long Control = 0x00;
    public const long Push = 0x01;
    public const long QpackEncoder = 0x02;
    public const long QpackDecoder = 0x03;
}
