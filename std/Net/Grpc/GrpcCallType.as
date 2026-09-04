// RFC 049 M-C2: Arc.Net.Grpc — 调用形态枚举（对齐 gRPC 规范四形态）。
//
// 框架内部分帧/状态/装配统一处理四形态：服务端读完整请求体（全部 DATA 至
// END_STREAM）后按处理器写响应（unary=1 出 · server-streaming=N 出 ·
// client-streaming=1 出 · bidi=N 出）。本枚举为处理器自描述元数据（供注册校验/文档）。

namespace Arc.Net.Grpc;

/// <summary>gRPC 调用形态。</summary>
public enum GrpcCallType {
    /// <summary>单请求 → 单响应。</summary>
    Unary,

    /// <summary>单请求 → 多响应（服务器流式）。</summary>
    ServerStreaming,

    /// <summary>多请求 → 单响应（客户端流式）。</summary>
    ClientStreaming,

    /// <summary>多请求 → 多响应（双向流式）。</summary>
    BidiStreaming,
}
