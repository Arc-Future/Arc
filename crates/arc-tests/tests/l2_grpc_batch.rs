//! L2 批量：gRPC 路由端到端回归集（RFC 049 M-C2 装配缺陷 P0 承接）。
//!
//! 首批 1 case，`grpc_route_echo_roundtrip`：TCP 回环起 GrpcServer + 注册
//! echo service/handler（`new GrpcServiceDefinition("echo.EchoService")`）+
//! 客户端手工 5 字节 gRPC 分帧（照 GrpcMessageCodec.EncodeFrame 语义：
//! [0x00 压缩标志][uint32 大端长度][消息]）发 POST `:path = /echo.EchoService/Echo`
//! → 断言响应为 UNIMPLEMENTED 之外的正确路由结果（`:status` 200 + trailers
//! `grpc-status: 0` + DATA 载荷解帧后与 handler 回显体逐字节一致）。
//!
//! 并发模型：GrpcServer.HandleConnection（服务端握手/读循环）与客户端
//! Http2Connection.Connect 均同步阻塞（SETTINGS 互等），单线程顺序必然
//! 死锁——服务端整链（Pending 限时门 + AcceptTcpClient + HandleConnection）
//! 置于 Arc.Threading.Thread 子线程，主线程做客户端全流程，收尾 Join 收尸。
//! 注意 Http2Connection.Connect 内部自建 TcpClient（prior knowledge 全程
//! 封装），客户端侧不得/也无法复用外部已连 socket。
//!
//! 端口策略（对齐 l2_net_batch）：TcpListener.Start 绑定 INADDR_ANY 且无端口
//! 回读 → 确定性高端口（47431 起，Start 失败 +1 重试 8 次）；子线程 Accept 前
//! 以 Pending() 限时轮询做防劫持门（SO_REUSEADDR 环境下端口被占时不无限阻塞）；
//! 服务端 TcpClient 设置收发超时防挂死。
//!
//! 批依赖：`("Arc.Net", "Net/Core")` + `("Arc.Net.Grpc", "Net/Grpc")`——包名
//! 取自各自 arc.toml 的 name 字段；Grpc 包传递依赖 Arc/Arc.Net 由 path 依赖
//! 拓扑解析（对齐 net_core 先例）。Arc.Threading 随基础库默认可用。
//!
//! 客户端不走 GrpcChannel（其 unary 面约束 protobuf IMessage 载体），以
//! Http2Connection public 传输原语直发——正好覆盖「分帧语义 + 路由装配 +
//! handler 回显 + trailers 状态」的框架内闭环，不引入 protobuf 消息定义面。
//! examples/UnitTest 下此前无 Grpc 调用方/测试先例（本批为首例）。

#[cfg(feature = "full-rt")]
use arc_tests::assert_compiles_and_runs_batch_with_deps;

#[cfg(feature = "full-rt")]
fn assert_all_passed(batch: &str, results: &[arc_tests::BatchRunResult]) {
    for r in results {
        assert!(
            r.passed,
            "{batch}: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_l2_grpc_route_batch() {
    let results = assert_compiles_and_runs_batch_with_deps(
        "grpc_route",
        &[(
            "grpc_route_echo_roundtrip",
            r#"using Arc;
using Arc.Net;
using Arc.Net.Grpc;
using Arc.Collections;
using Arc.Threading;

class EchoHandler : IGrpcHandler {
    public string Method { get { return "Echo"; } }
    public GrpcCallType CallType { get { return GrpcCallType.Unary; } }
    public void Handle(GrpcCallContext ctx) {
        byte[] reply = null;
        if (ctx.Requests.Count > 0) { reply = ctx.Requests[0]; }
        ctx.WriteResponse(reply);
    }
}

void Main() {
    GrpcServer server = new GrpcServer();
    GrpcServiceDefinition svc = new GrpcServiceDefinition("echo.EchoService");
    svc.Add(new EchoHandler());
    server.AddService(svc);

    TcpListener listener = new TcpListener();
    int port = 47431;
    bool bound = false;
    for (int attempt = 0; attempt < 8; attempt++) {
        if (listener.Start(port)) { bound = true; break; }
        port = port + 1;
    }
    if (!bound) { Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:FAIL:bind port=" + port); return; }
    Console.WriteLine("grpc-listener-port=" + port);

    Thread srvThread = new Thread(() => {
        bool ready = false;
        for (int w = 0; w < 60 && !ready; w++) {
            if (listener.Pending()) { ready = true; } else { Thread.Sleep(50); }
        }
        if (!ready) { return; }
        TcpClient acc = listener.AcceptTcpClient();
        if (acc == null) { return; }
        acc.SetReceiveTimeout(10000);
        acc.SetSendTimeout(10000);
        server.HandleConnection(acc);
    });
    srvThread.Start();

    Http2Connection h2 = new Http2Connection();
    if (!h2.Connect("127.0.0.1", port)) { Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:FAIL:h2-connect"); return; }
    Console.WriteLine("grpc-h2-connected");

    string payload = "grpc-echo-ping-0123456789";
    int n = payload.Length;
    byte[] msg = new byte[n];
    for (int i = 0; i < n; i++) { msg[i] = (byte)payload[i]; }
    List<byte> framed = new List<byte>();
    framed.Add((byte)0);
    framed.Add((byte)((n / 16777216) % 256));
    framed.Add((byte)((n / 65536) % 256));
    framed.Add((byte)((n / 256) % 256));
    framed.Add((byte)(n % 256));
    for (int i = 0; i < n; i++) { framed.Add(msg[i]); }

    Http2Request req = new Http2Request("POST", "/echo.EchoService/Echo");
    req.Headers.Add("content-type", "application/grpc");
    req.Headers.Add("te", "trailers");
    req.Body = framed.ToArray();
    Http2Response resp = h2.SendRequest(req);
    if (resp == null) { Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:FAIL:resp-null"); return; }
    Console.WriteLine("grpc-http-status=" + resp.StatusCode + " grpc-status=" + resp.Trailers.Get("grpc-status"));
    if (resp.StatusCode != 200) { Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:FAIL:http-status=" + resp.StatusCode); return; }
    if (resp.Trailers.Get("grpc-status") != "0") { Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:FAIL:grpc-status=" + resp.Trailers.Get("grpc-status")); return; }

    byte[] body = resp.BodyBytes;
    if (body.Length < 5) { Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:FAIL:body-len=" + body.Length); return; }
    int comp = (int)body[0];
    int mlen = (int)body[1] * 16777216 + (int)body[2] * 65536 + (int)body[3] * 256 + (int)body[4];
    if (comp != 0 || mlen != n) { Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:FAIL:frame comp=" + comp + " len=" + mlen); return; }
    for (int i = 0; i < n; i++) {
        if (body[5 + i] != msg[i]) { Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:FAIL:mismatch@" + i); return; }
    }

    h2.Close();
    srvThread.Join(5000);
    listener.Stop();
    Console.WriteLine("ARC_CASE:grpc_route_echo_roundtrip:PASS");
}
"#,
        )],
        &[("Arc.Net", "Net/Core"), ("Arc.Net.Grpc", "Net/Grpc")],
    );
    assert_all_passed("grpc_route", &results);
    let get = |name: &str| {
        results
            .iter()
            .find(|r| r.name == name)
            .expect("case result present")
    };
    assert!(
        get("grpc_route_echo_roundtrip")
            .stdout
            .contains("grpc-h2-connected"),
        "echo case should log h2 handshake; stdout: {}",
        get("grpc_route_echo_roundtrip").stdout
    );
}

#[cfg(not(feature = "full-rt"))]
#[test]
fn runs_l2_grpc_route_batch() {
    // L2 runtime tests require --features full-rt
}
