// RFC 049 M-C2: Arc.Net.Grpc — 单次 gRPC 调用上下文（服务端侧）。
//
// 承载已解码的请求消息字节 + 响应写出 + 调用状态。处理器经此读写消息；
// 内部持有传输连接与流 ID，但**不暴露传输内部对象**（分帧/状态 internal）。
//
// 访问权限：public（开发者面契约——处理器直接触碰）；底层连接/分帧 internal。

namespace Arc.Net.Grpc;

using Arc.Collections;
using Arc.Net;
using Arc.Text.Protobuf;

/// <summary>单次 gRPC 调用上下文（服务端侧）：请求消息 + 响应写出 + 状态。</summary>
public class GrpcCallContext {
    private Http2ServerConnection _conn;
    private int _streamId;

    /// <summary>已解码的请求消息字节（每帧一条；unary=1 · client-streaming=N）。</summary>
    public List<byte[]> Requests;

    internal GrpcCallContext(Http2ServerConnection conn, int streamId) {
        _conn = conn;
        _streamId = streamId;
        Status = GrpcStatus.Ok;
        Requests = new List<byte[]>();
    }

    /// <summary>调用结果状态；默认 Ok，处理器可置为错误码（写出到末尾 trailers）。</summary>
    public GrpcStatus Status { get; set; }

    /// <summary>写一条响应消息（自动 5 字节 gRPC 分帧；server-streaming/bidi 可多次调用）。</summary>
    public void WriteResponse(byte[] messageBytes) {
        // 空值由 GrpcMessageCodec.EncodeFrame 内部兜底为空帧（长度 0）。
        _conn.SendData(_streamId, GrpcMessageCodec.EncodeFrame(messageBytes), false);
    }

    /// <summary>写一条响应消息（protobuf 消息 → 序列化 → 分帧）。</summary>
    public void WriteResponse(IMessage message) {
        byte[] bytes = MessageCodec.Serialize(message);
        this.WriteResponse(bytes);
    }
}
