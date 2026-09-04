// RFC 049 M-C2: Arc.Net.Grpc — gRPC 服务端（服务注册 + 连接/请求处理）。
//
// 对齐 RFC 049 §5 解耦裁决：暴露 **HandleConnection / HandleRequest 能力入口**，
// 供 [RFC 050](050-webapplication-framework.md) `Arc.Web.Grpc` 集成挂接——
// **不自托管宿主、不反向依赖宿主**（宿主导流转由 WebApplication 决定）。
//
// 传输底座复用 M-B `Http2ServerConnection`/`Http2ServerRequest`（public 传输原语）；
// 框架层在此之上做 gRPC 装配：:path 路由 → 处理器分派 → 5 字节分帧（GrpcMessageCodec
// internal）→ trailers 状态。分帧/状态/装配均 internal，仅暴露能力契约。
//
// 诚实边界（对齐 M-B）：单连接顺序处理；服务端读完整请求体（全部 DATA 至 END_STREAM）
// 后统一写响应——四形态在此顺序模型下统一（unary=1 出 · server-streaming=N 出 ·
// client-streaming=1 出 · bidi=N 出），无并发双向交错。

namespace Arc.Net.Grpc;

using Arc.Collections;
using Arc.Net;

/// <summary>gRPC 服务端：服务注册 + 连接/请求处理能力入口（不自托管宿主）。</summary>
public class GrpcServer {
    private List<GrpcServiceDefinition> _services;

    public GrpcServer() {
        _services = new List<GrpcServiceDefinition>();
    }

    /// <summary>注册一个服务定义。</summary>
    public void AddService(GrpcServiceDefinition def) {
        if (def != null) { _services.Add(def); }
    }

    /// <summary>能力入口 1：完整服务一条连接（握手 + 顺序处理该连接上各请求）。</summary>
    public void HandleConnection(TcpClient client) {
        if (client == null) { return; }
        Http2ServerConnection conn = new Http2ServerConnection(client);
        if (!conn.AcceptHandshake()) { conn.Close(); return; }
        while (true) {
            Http2ServerRequest req = conn.ReadRequest();
            if (req == null) { break; }
            this.HandleRequest(conn, req);
        }
        conn.Close();
    }

    /// <summary>能力入口 2：处理单个已解析请求（供 WebApplication 集成层挂接）。</summary>
    public void HandleRequest(Http2ServerConnection conn, Http2ServerRequest req) {
        if (conn == null || req == null) { return; }
        string serviceName = "";
        string methodName = "";
        if (!this.ParsePath(req.Path, out serviceName, out methodName)) {
            this.SendStatusTrailers(conn, req.StreamId, GrpcStatus.Unimplemented);
            return;
        }
        IGrpcHandler handler = this.FindHandler(serviceName, methodName);
        if (handler == null) {
            this.SendStatusTrailers(conn, req.StreamId, GrpcStatus.Unimplemented);
            return;
        }
        // 响应头。
        Http2HeaderList rh = new Http2HeaderList();
        rh.Add(":status", "200");
        rh.Add("content-type", "application/grpc");
        rh.Add("grpc-encoding", "identity");
        conn.SendResponseHeaders(req.StreamId, rh);
        // 解码请求分帧 → 调用处理器 → 末尾 trailers（grpc-status）。
        GrpcCallContext ctx = new GrpcCallContext(conn, req.StreamId);
        this.DecodeFrames(req.Body, ctx.Requests);
        handler.Handle(ctx);
        this.SendStatusTrailers(conn, req.StreamId, ctx.Status);
    }

    // ── 内部 ──

    private IGrpcHandler FindHandler(string service, string method) {
        int i = 0;
        while (i < _services.Count) {
            GrpcServiceDefinition def = _services[i];
            if (def.Service == service) {
                return def.Find(method);
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>解析 :path 为 service + method（形如 /pkg.Service/Method）。</summary>
    private bool ParsePath(string path, out string service, out string method) {
        service = "";
        method = "";
        if (path == null || path == "") { return false; }
        if (path[0] != '/') { return false; }
        int slash = path.IndexOf("/", 1);
        if (slash < 0) { return false; }
        service = path.Substring(1, slash - 1);
        method = path.Substring(slash + 1, path.Length - slash - 1);
        return service != "" && method != "";
    }

    /// <summary>把请求体按 5 字节分帧切分为消息序列。</summary>
    private void DecodeFrames(byte[] body, List<byte[]> output) {
        byte[] b = body;
        int pos = 0;
        while (true) {
            int next = 0;
            byte[] msg = GrpcMessageCodec.ReadFrame(b, pos, out next);
            if (msg == null) { break; }
            output.Add(msg);
            pos = next;
            if (pos >= b.Length) { break; }
        }
    }

    private void SendStatusTrailers(Http2ServerConnection conn, int streamId, GrpcStatus status) {
        Http2HeaderList tr = new Http2HeaderList();
        tr.Add("grpc-status", this.IntValue(status));
        conn.SendTrailers(streamId, tr);
    }

    private string IntValue(GrpcStatus status) {
        if (status == GrpcStatus.Ok) { return "0"; }
        if (status == GrpcStatus.Cancelled) { return "1"; }
        if (status == GrpcStatus.Unknown) { return "2"; }
        if (status == GrpcStatus.InvalidArgument) { return "3"; }
        if (status == GrpcStatus.DeadlineExceeded) { return "4"; }
        if (status == GrpcStatus.NotFound) { return "5"; }
        if (status == GrpcStatus.AlreadyExists) { return "6"; }
        if (status == GrpcStatus.PermissionDenied) { return "7"; }
        if (status == GrpcStatus.ResourceExhausted) { return "8"; }
        if (status == GrpcStatus.FailedPrecondition) { return "9"; }
        if (status == GrpcStatus.Aborted) { return "10"; }
        if (status == GrpcStatus.OutOfRange) { return "11"; }
        if (status == GrpcStatus.Unimplemented) { return "12"; }
        if (status == GrpcStatus.Internal) { return "13"; }
        if (status == GrpcStatus.Unavailable) { return "14"; }
        if (status == GrpcStatus.DataLoss) { return "15"; }
        if (status == GrpcStatus.Unauthenticated) { return "16"; }
        return "2";
    }
}
