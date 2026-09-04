// RFC 049 M-C: Arc.Net.Grpc — gRPC 客户端通道（unary / server-streaming）。
//
// 基于传输层 `Arc.Net.Http2Connection`（public 传输原语）消费 HTTP/2 客户端面；
// 在框架层做 gRPC 5 字节分帧（GrpcMessageCodec，internal）+ trailers 状态解析。
//
// 访问权限：public（用户面契约，开发者直接触碰）。分帧 codec / 状态解析均为
// internal，开发者不直接操作 gRPC 内部字节。
//
// 调用形态（M-C1：unary + server-streaming；client-streaming / bidi 归 M-C2）：
//   - UnaryCall：单请求消息 → 单响应消息。
//   - ServerStreamingCall：单请求消息 → 多响应消息（List）。
//
// 诚实边界：压缩后置（仅透明未压缩帧）；跨帧消息分片后置（传输层聚合整流 DATA
// 后再按 5 字节前缀切分，长度须落在单流载荷内）。

namespace Arc.Net.Grpc;

using Arc.Collections;
using Arc.Net;
using Arc.Text.Protobuf;

/// <summary>gRPC 客户端通道：连接 HTTP/2 + gRPC 分帧 + 状态解析（unary/server-streaming）。</summary>
public class GrpcChannel {
    private Http2Connection _conn;

    public GrpcChannel() {
        _conn = new Http2Connection();
        Connected = false;
        LastStatus = GrpcStatus.Ok;
    }

    /// <summary>prior knowledge 建立到 <paramref name="host"/>:<paramref name="port"/> 的 gRPC(h2c) 连接。</summary>
    public bool Connect(string host, int port) {
        Connected = _conn.Connect(host, port);
        return Connected;
    }

    public bool Connected { get; set; }

    /// <summary>最近一次调用的 gRPC 状态（OK 或失败码）。</summary>
    public GrpcStatus LastStatus { get; set; }

    /// <summary>unary 调用：单请求 → 单响应。失败返回默认（空）消息，状态见 <see cref="LastStatus"/>。</summary>
    public TResp UnaryCall<TResp>(GrpcMethodDefinition method, IMessage request)
        where TResp : IMessage, new() {
        Http2Response resp = this.Call(method, request);
        if (resp == null) { LastStatus = GrpcStatus.Unavailable; return new TResp(); }
        LastStatus = this.ReadStatus(resp);
        if (LastStatus != GrpcStatus.Ok) { return new TResp(); }
        int pos = 0;
        byte[] msg = GrpcMessageCodec.ReadFrame(resp.BodyBytes, 0, out pos);
        if (msg == null) { LastStatus = GrpcStatus.Internal; return new TResp(); }
        // 直接经消息自身的 MergeFrom 就地反序列化（避免把泛型 TResp 作为约束实参再
        // 传给另一个泛型 Deserialize<T>——编译器不传播 where 约束到被调泛型实参）。
        TResp result = new TResp();
        CodedInputStream input = new CodedInputStream(msg);
        result.MergeFrom(input);
        return result;
    }

    /// <summary>server-streaming 调用：单请求 → 多响应（List）。失败返回空表，状态见 <see cref="LastStatus"/>。</summary>
    public List<TResp> ServerStreamingCall<TResp>(GrpcMethodDefinition method, IMessage request)
        where TResp : IMessage, new() {
        List<TResp> results = new List<TResp>();
        Http2Response resp = this.Call(method, request);
        if (resp == null) { LastStatus = GrpcStatus.Unavailable; return results; }
        LastStatus = this.ReadStatus(resp);
        if (LastStatus != GrpcStatus.Ok) { return results; }
        byte[] body = resp.BodyBytes;
        int pos = 0;
        while (true) {
            int next = 0;
            byte[] msg = GrpcMessageCodec.ReadFrame(body, pos, out next);
            if (msg == null) { break; }
            TResp item = new TResp();
            CodedInputStream input = new CodedInputStream(msg);
            item.MergeFrom(input);
            results.Add(item);
            pos = next;
            if (pos >= body.Length) { break; }
        }
        return results;
    }

    /// <summary>client-streaming 调用：多请求 → 单响应。失败返回默认消息，状态见 <see cref="LastStatus"/>。</summary>
    public TResp ClientStreamingCall<TResp>(GrpcMethodDefinition method, List<IMessage> requests)
        where TResp : IMessage, new() {
        Http2Response resp = this.CallMany(method, requests);
        if (resp == null) { LastStatus = GrpcStatus.Unavailable; return new TResp(); }
        LastStatus = this.ReadStatus(resp);
        if (LastStatus != GrpcStatus.Ok) { return new TResp(); }
        int pos = 0;
        byte[] msg = GrpcMessageCodec.ReadFrame(resp.BodyBytes, 0, out pos);
        if (msg == null) { LastStatus = GrpcStatus.Internal; return new TResp(); }
        TResp result = new TResp();
        CodedInputStream input = new CodedInputStream(msg);
        result.MergeFrom(input);
        return result;
    }

    /// <summary>bidi-streaming 调用：多请求 → 多响应（List）。失败返回空表，状态见 <see cref="LastStatus"/>。</summary>
    public List<TResp> BidiStreamingCall<TResp>(GrpcMethodDefinition method, List<IMessage> requests)
        where TResp : IMessage, new() {
        List<TResp> results = new List<TResp>();
        Http2Response resp = this.CallMany(method, requests);
        if (resp == null) { LastStatus = GrpcStatus.Unavailable; return results; }
        LastStatus = this.ReadStatus(resp);
        if (LastStatus != GrpcStatus.Ok) { return results; }
        byte[] body = resp.BodyBytes;
        int pos = 0;
        while (true) {
            int next = 0;
            byte[] msg = GrpcMessageCodec.ReadFrame(body, pos, out next);
            if (msg == null) { break; }
            TResp item = new TResp();
            CodedInputStream input = new CodedInputStream(msg);
            item.MergeFrom(input);
            results.Add(item);
            pos = next;
            if (pos >= body.Length) { break; }
        }
        return results;
    }

    /// <summary>优雅关闭。</summary>
    public void Close() {
        _conn.Close();
        Connected = false;
    }

    // ── 内部 ──

    /// <summary>发送一次 gRPC 调用（POST + gRPC 头 + 5 字节分帧请求体），返回传输层响应。</summary>
    private Http2Response Call(GrpcMethodDefinition method, IMessage request) {
        if (!Connected) { return null; }
        Http2Request hreq = new Http2Request("POST", method.FullName);
        Http2HeaderList hs = hreq.Headers;
        hs.Add("content-type", "application/grpc");
        hs.Add("te", "trailers");
        byte[] msgBytes = MessageCodec.Serialize(request);
        hreq.Body = GrpcMessageCodec.EncodeFrame(msgBytes);
        return _conn.SendRequest(hreq);
    }

    /// <summary>发送多请求调用（client-streaming/bidi）：多请求消息分帧拼接为体。</summary>
    private Http2Response CallMany(GrpcMethodDefinition method, List<IMessage> requests) {
        if (!Connected) { return null; }
        Http2Request hreq = new Http2Request("POST", method.FullName);
        Http2HeaderList hs = hreq.Headers;
        hs.Add("content-type", "application/grpc");
        hs.Add("te", "trailers");
        hreq.Body = this.BuildBody(requests);
        return _conn.SendRequest(hreq);
    }

    /// <summary>把请求消息序列拼接为一条分帧体（多帧连接至 END_STREAM）。</summary>
    private byte[] BuildBody(List<IMessage> requests) {
        List<byte> frame = new List<byte>();
        int i = 0;
        while (i < requests.Count) {
            IMessage msg = requests[i];
            byte[] ser = MessageCodec.Serialize(msg);
            byte[] framed = GrpcMessageCodec.EncodeFrame(ser);
            int j = 0;
            while (j < framed.Length) {
                frame.Add(framed[j]);
                j = j + 1;
            }
            i = i + 1;
        }
        return frame.ToArray();
    }

    /// <summary>从响应解析 gRPC 状态：优先 trailers `grpc-status`；无则依 HTTP 状态兜底。</summary>
    private GrpcStatus ReadStatus(Http2Response resp) {
        string s = resp.Trailers.Get("grpc-status");
        if (s != "") {
            return this.FromInt(this.ParseInt(s));
        }
        if (resp.StatusCode == 200) { return GrpcStatus.Ok; }
        return GrpcStatus.Unknown;
    }

    private int ParseInt(string s) {
        if (s == null || s == "") { return 0; }
        try {
            return Convert.ToInt32(s, 10);
        } catch {
            return 0;
        }
    }

    private GrpcStatus FromInt(int code) {
        if (code == 0) { return GrpcStatus.Ok; }
        if (code == 1) { return GrpcStatus.Cancelled; }
        if (code == 2) { return GrpcStatus.Unknown; }
        if (code == 3) { return GrpcStatus.InvalidArgument; }
        if (code == 4) { return GrpcStatus.DeadlineExceeded; }
        if (code == 5) { return GrpcStatus.NotFound; }
        if (code == 6) { return GrpcStatus.AlreadyExists; }
        if (code == 7) { return GrpcStatus.PermissionDenied; }
        if (code == 8) { return GrpcStatus.ResourceExhausted; }
        if (code == 9) { return GrpcStatus.FailedPrecondition; }
        if (code == 10) { return GrpcStatus.Aborted; }
        if (code == 11) { return GrpcStatus.OutOfRange; }
        if (code == 12) { return GrpcStatus.Unimplemented; }
        if (code == 13) { return GrpcStatus.Internal; }
        if (code == 14) { return GrpcStatus.Unavailable; }
        if (code == 15) { return GrpcStatus.DataLoss; }
        if (code == 16) { return GrpcStatus.Unauthenticated; }
        return GrpcStatus.Unknown;
    }
}
