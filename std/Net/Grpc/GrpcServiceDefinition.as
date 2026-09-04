// RFC 049 M-C2: Arc.Net.Grpc — gRPC 服务定义（服务名 + 一组方法处理器）。
//
// 开发者以 `new GrpcServiceDefinition("echo.EchoService")` 建服务，逐方法
// `Add(handler)` 注册 IGrpcHandler 实现；再注册到 GrpcServer。

namespace Arc.Net.Grpc;

using Arc.Collections;

/// <summary>gRPC 服务定义：服务名 + 一组方法处理器。开发者注册到 <see cref="GrpcServer"/>。</summary>
public class GrpcServiceDefinition {
    private List<IGrpcHandler> _handlers;

    /// <summary>构造服务（<paramref name="service"/> 如 "echo.EchoService"；:path 首段）。</summary>
    public GrpcServiceDefinition(string service) {
        Service = service;
        _handlers = new List<IGrpcHandler>();
    }

    /// <summary>服务名。</summary>
    public string Service { get; }

    /// <summary>注册一个方法处理器。</summary>
    public void Add(IGrpcHandler handler) {
        if (handler != null) { _handlers.Add(handler); }
    }

    /// <summary>按方法名查找处理器；未命中返回 null。</summary>
    internal IGrpcHandler Find(string method) {
        int i = 0;
        while (i < _handlers.Count) {
            IGrpcHandler h = _handlers[i];
            if (h.Method == method) { return h; }
            i = i + 1;
        }
        return null;
    }
}
