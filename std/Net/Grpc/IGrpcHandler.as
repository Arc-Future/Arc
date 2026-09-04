// RFC 049 M-C2: Arc.Net.Grpc — gRPC 方法处理器契约（开发者面契约）。
//
// 泛型委托/回调面受限（RFC 049 §1.3）：处理器以「接口 + 具体注册对象」承载，而非
// 泛型委托。开发者实现本接口并注册到 GrpcServiceDefinition；框架按 :path 路由到
// 处理器，读请求消息（GrpcCallContext.Requests）→ 调用 Handle → 经 WriteResponse 写响应。

namespace Arc.Net.Grpc;

/// <summary>gRPC 方法处理器契约（开发者实现）。方法名 = <see cref="Method"/>，服务内唯一。</summary>
public interface IGrpcHandler {
    /// <summary>方法名（如 "Echo"）；框架按 :path 末段分派。</summary>
    string Method { get; }

    /// <summary>调用形态元数据（自描述）。</summary>
    GrpcCallType CallType { get; }

    /// <summary>处理一次调用：读 <see cref="GrpcCallContext.Requests"/>，经 <see cref="GrpcCallContext.WriteResponse"/> 写响应。</summary>
    void Handle(GrpcCallContext ctx);
}
