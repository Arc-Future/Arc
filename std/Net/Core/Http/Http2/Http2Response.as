// Http2Response —— 拆分自 Http2Types.as（一文件一公开类型）。
namespace Arc.Net;
using Arc.Collections;

/// <summary>HTTP/2 响应。HTTP/2 无 reason phrase——`ReasonPhrase` 固定为空串。</summary>
public class Http2Response {
    /// <summary>`:status` 解析出的状态码（如 200）。</summary>
    public int StatusCode;

    /// <summary>响应头（含伪头 `:status`，值即状态码文本）。</summary>
    public Http2HeaderList Headers;

    /// <summary>DATA 载荷的 UTF-8 文本视图；非文本载荷请用 `BodyBytes`。</summary>
    public string Body;

    /// <summary>DATA 载荷原始字节。</summary>
    public byte[] BodyBytes;

    /// <summary>该响应对应请求流是否为「完整往返」的收尾（含 END_STREAM）。</summary>
    public bool EndOfStream;

    /// <summary>传输/协议失败原因；空串表示成功（StatusCode &gt; 0）。</summary>
    public string Failure;

    /// <summary>末尾 HEADERS（trailers：如 gRPC `grpc-status`/`grpc-message`）；无则空表。</summary>
    public Http2HeaderList Trailers;

    public Http2Response() {
        Headers = new Http2HeaderList();
        Body = "";
        BodyBytes = Http2ByteUtils.ZeroBytes(0);
        EndOfStream = false;
        Failure = "";
        Trailers = new Http2HeaderList();
    }
}
