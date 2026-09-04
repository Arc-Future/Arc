//! L2 批量：网络 Core（Arc.Net）运行时冒烟集（P0-1 批前置基建 B0）。
//!
//! 3 case，TCP 回环 echo + HTTP/1.1 分帧字节正确性：
//! - `net_tcp_echo_sync`：string 面（TcpListener.Start/Pending/AcceptTcpClient +
//!   TcpClient.Connect/Send/Receive）同步回环。
//! - `net_tcp_echo_async`：byte 面（ConnectAsync/AcceptTcpClientAsync/
//!   SendBytesAsync + NetworkStream.ReadBytesAsync/WriteBytesAsync）async 回环，
//!   payload 含 0x00 验证字节面无 NUL 截断，客户端 ReadBytesAsync 循环收满。
//! - `net_http11_body_bytes`：HTTP/1.1 分帧（Http11ServerConnection.ReadRequest/
//!   WriteResponse）多字节 UTF-8 body（4096×「汉境」= 24576 字节）POST 回环——
//!   断言服务端 Body 与原文一致、响应 Content-Length 恰等于响应体字节数、body
//!   逐字节一致（客户端 ReadLine 转真字节 Read 循环读满，覆盖行缓冲/网络腿衔接）。
//!
//! 端口策略（std/Net API 缺口，见下）：TcpListener.Start 绑定 INADDR_ANY 且无
//! LocalEndpoint/端口回读（rt_net.c 无 getsockname 暴露），port 0 无法回读实际
//! 端口 → 采用确定性高端口（sync 47231+/async 47331+/http11 47431+，Start 失败则
//! +1 重试 8 次）；并在 Accept 前以 Pending()（listening fd 的 select-read）做
//! 防劫持门，避免 SO_REUSEADDR 环境下端口被占时 accept 无限阻塞。
//!
//! 批依赖：`("Arc.Net", "Net/Core")`——包名取自 std/Net/Core/arc.toml 的
//! `name = "Arc.Net"`（std/Net/arc.toml 仅是 workspace 聚合清单，无 name 字段）。
//! 批内混排 sync/async case，driver 由协议自动生成为 async（EventLoop 驱动）。
//!
//! 已知 std/Net API 缺口（供后续修复批次参考）：
//! 1. TcpListener/Socket 无法绑定指定地址（恒 INADDR_ANY，"127.0.0.1:0" 不可表达）；
//! 2. 绑定后无实际端口回读面（无 LocalEndpoint/getsockname 暴露）→ port 0 不可用；
//! 3. string 面（Send/Receive）按 NUL 截断二进制载荷——二进制对传走 byte 面；
//!    NetworkStream 同步 Read 已接 ReceiveBytes（真字节，无 NUL 截断），Write 仍
//!    基于 string 面（含 NUL 载荷截断，后置）；HTTP/1.1 分帧按字节计数
//!    （Http11ServerConnection ReadN/ReadNAsync），含内部 0x00 的请求体受 Body
//!    string 模型首 NUL 截断（诚实边界，BodyBytes 字节面后置）。
//!
//! 基建缺口回执（2026-08-30 登记 → std P3 修复）：Arc.Security 符号使产物
//! 隐式导入 vendored `crypto_native.dll`，其 beside-exe 自动复制曾因 codegen
//! `copy_crypto_native_dll_if_needed` 的 Windows 门卫不识别进程内编译路径
//! （target=None → Host）而未生效——产物旁无 DLL，进程启动即 0xC0000135
//! （STATUS_DLL_NOT_FOUND）。门卫已改用 `is_windows_target`（Host +
//! cfg!(windows) 兜底），测试侧预拷贝兜底随之移除。

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
fn runs_net_core_batch() {
    let results = assert_compiles_and_runs_batch_with_deps(
        "net_core",
        &[
            (
                "net_tcp_echo_sync",
                r#"using Arc;
using Arc.Net;

void Main() {
    TcpListener server = new TcpListener();
    int port = 47231;
    bool bound = false;
    for (int attempt = 0; attempt < 8; attempt++) {
        if (server.Start(port)) { bound = true; break; }
        port = port + 1;
    }
    if (!bound) { Console.WriteLine("ARC_CASE:net_tcp_echo_sync:FAIL:bind port=" + port); return; }
    Console.WriteLine("listener-port=" + port);

    TcpClient client = null;
    bool connected = false;
    for (int attempt = 0; attempt < 3 && !connected; attempt++) {
        client = new TcpClient();
        connected = client.Connect("127.0.0.1", port);
    }
    if (!connected) { Console.WriteLine("ARC_CASE:net_tcp_echo_sync:FAIL:connect"); return; }
    client.SetReceiveTimeout(5000);
    client.SetSendTimeout(5000);

    if (!server.Pending()) { Console.WriteLine("ARC_CASE:net_tcp_echo_sync:FAIL:pending"); return; }

    TcpClient accepted = server.AcceptTcpClient();
    if (accepted == null) { Console.WriteLine("ARC_CASE:net_tcp_echo_sync:FAIL:accept-null"); return; }
    accepted.SetReceiveTimeout(5000);
    accepted.SetSendTimeout(5000);

    string payload = "arc-net-smoke-ping-0123456789";
    int sent = client.Send(payload);
    if (sent <= 0) { Console.WriteLine("ARC_CASE:net_tcp_echo_sync:FAIL:sent=" + sent); return; }

    string echoed = accepted.Receive();
    if (echoed != payload) { Console.WriteLine("ARC_CASE:net_tcp_echo_sync:FAIL:echo-len=" + echoed.Length); return; }

    int back = accepted.Send(echoed);
    if (back <= 0) { Console.WriteLine("ARC_CASE:net_tcp_echo_sync:FAIL:back=" + back); return; }

    string received = client.Receive();
    if (received != payload) { Console.WriteLine("ARC_CASE:net_tcp_echo_sync:FAIL:recv-len=" + received.Length); return; }

    client.Close();
    accepted.Close();
    server.Stop();
    Console.WriteLine("ARC_CASE:net_tcp_echo_sync:PASS");
}
"#,
            ),
            (
                "net_tcp_echo_async",
                r#"using Arc;
using Arc.Net;

async Task<void> Main() {
    TcpListener server = new TcpListener();
    int port = 47331;
    bool bound = false;
    for (int attempt = 0; attempt < 8; attempt++) {
        if (server.Start(port)) { bound = true; break; }
        port = port + 1;
    }
    if (!bound) { Console.WriteLine("ARC_CASE:net_tcp_echo_async:FAIL:bind"); return; }

    TcpClient client = new TcpClient();
    await client.ConnectAsync("127.0.0.1", port);
    if (!client.Connected) { Console.WriteLine("ARC_CASE:net_tcp_echo_async:FAIL:connect"); return; }
    Console.WriteLine("net-async-step:connected");

    if (!server.Pending()) { Console.WriteLine("ARC_CASE:net_tcp_echo_async:FAIL:pending"); return; }
    Console.WriteLine("net-async-step:pending");

    TcpClient accepted = await server.AcceptTcpClientAsync();
    if (accepted == null) { Console.WriteLine("ARC_CASE:net_tcp_echo_async:FAIL:accept-null"); return; }
    Console.WriteLine("net-async-step:accepted");

    int len = 256;
    byte[] payload = new byte[len];
    for (int i = 0; i < len; i++) {
        payload[i] = (byte)((i * 7) % 256);
    }
    payload[0] = (byte)0;
    payload[len - 1] = (byte)0;

    int sent = await client.SendBytesAsync(payload, 0, len);
    if (sent != len) { Console.WriteLine("ARC_CASE:net_tcp_echo_async:FAIL:sent=" + sent); return; }

    NetworkStream srvStream = new NetworkStream(accepted);
    byte[] rbuf = new byte[len];
    int got = 0;
    while (got < len) {
        int n = await srvStream.ReadBytesAsync(rbuf, got, len - got);
        if (n <= 0) { break; }
        got = got + n;
    }
    if (got != len) { Console.WriteLine("ARC_CASE:net_tcp_echo_async:FAIL:server-read=" + got); return; }

    await srvStream.WriteBytesAsync(rbuf, 0, got);

    NetworkStream cliStream = new NetworkStream(client);
    byte[] ebuf = new byte[len];
    int got2 = 0;
    while (got2 < len) {
        int n2 = await cliStream.ReadBytesAsync(ebuf, got2, len - got2);
        if (n2 <= 0) { break; }
        got2 = got2 + n2;
    }
    if (got2 != len) { Console.WriteLine("ARC_CASE:net_tcp_echo_async:FAIL:client-read=" + got2); return; }

    for (int i = 0; i < len; i++) {
        if (ebuf[i] != payload[i]) { Console.WriteLine("ARC_CASE:net_tcp_echo_async:FAIL:mismatch@" + i); return; }
    }

    cliStream.Close();
    srvStream.Close();
    server.Stop();
    Console.WriteLine("ARC_CASE:net_tcp_echo_async:PASS");
}
"#,
            ),
            (
                "net_http11_body_bytes",
                r#"using Arc;
using Arc.Net;
using Arc.Text;

void Main() {
    string body = "";
    for (int i = 0; i < 4096; i++) {
        body = body + "汉境";
    }
    byte[] bodyBytes = Encoding.GetBytes(body);
    if (bodyBytes.Length != 24576) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:body-bytes=" + bodyBytes.Length); return; }

    TcpListener server = new TcpListener();
    int port = 47431;
    bool bound = false;
    for (int attempt = 0; attempt < 8; attempt++) {
        if (server.Start(port)) { bound = true; break; }
        port = port + 1;
    }
    if (!bound) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:bind port=" + port); return; }

    TcpClient client = null;
    bool connected = false;
    for (int attempt = 0; attempt < 3 && !connected; attempt++) {
        client = new TcpClient();
        connected = client.Connect("127.0.0.1", port);
    }
    if (!connected) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:connect"); return; }
    client.SetReceiveTimeout(5000);
    client.SetSendTimeout(5000);

    if (!server.Pending()) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:pending"); return; }

    TcpClient accepted = server.AcceptTcpClient();
    if (accepted == null) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:accept-null"); return; }
    accepted.SetReceiveTimeout(5000);
    accepted.SetSendTimeout(5000);

    string req = "POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: " + Convert.ToString(bodyBytes.Length) + "\r\nConnection: close\r\n\r\n" + body;
    int sent = client.Send(req);
    if (sent <= 0) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:sent=" + sent); return; }

    Http11ServerConnection conn = new Http11ServerConnection(accepted, 5000);
    HttpServerRequest reqParsed = conn.ReadRequest();
    if (reqParsed == null) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:req-null"); return; }
    if (reqParsed.Method != "POST") { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:method=" + reqParsed.Method); return; }
    if (reqParsed.Body != body) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:body-len=" + reqParsed.Body.Length + "/exp=" + body.Length); return; }

    bool respOk = conn.WriteResponse(200, "OK", null, "text/plain; charset=utf-8", reqParsed.Body, null);
    if (!respOk) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:write-resp"); return; }

    NetworkStream cs = new NetworkStream(client);
    string statusLine = cs.ReadLine();
    if (statusLine == null || statusLine.IndexOf(" 200 ") < 0) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:status=" + statusLine); return; }
    bool sawLen = false;
    while (true) {
        string hl = cs.ReadLine();
        if (hl == null) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:header-null"); return; }
        if (hl == "") { break; }
        if (hl == "Content-Length: " + Convert.ToString(bodyBytes.Length)) { sawLen = true; }
    }
    if (!sawLen) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:content-length-mismatch"); return; }

    byte[] rbuf = new byte[bodyBytes.Length];
    int got = 0;
    while (got < bodyBytes.Length) {
        int k = cs.Read(rbuf, got, bodyBytes.Length - got);
        if (k <= 0) { break; }
        got = got + k;
    }
    if (got != bodyBytes.Length) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:body-read=" + got); return; }
    for (int i = 0; i < bodyBytes.Length; i++) {
        if (rbuf[i] != bodyBytes[i]) { Console.WriteLine("ARC_CASE:net_http11_body_bytes:FAIL:mismatch@" + i); return; }
    }

    cs.Close();
    conn.Close();
    client.Close();
    accepted.Close();
    server.Stop();
    Console.WriteLine("ARC_CASE:net_http11_body_bytes:PASS");
}
"#,
            ),
        ],
        &[("Arc.Net", "Net/Core")],
    );
    assert_all_passed("net_core", &results);
    let get = |name: &str| {
        results
            .iter()
            .find(|r| r.name == name)
            .expect("case result present")
    };
    assert!(
        get("net_tcp_echo_sync").stdout.contains("listener-port="),
        "sync case should log bound port; stdout: {}",
        get("net_tcp_echo_sync").stdout
    );
}

#[cfg(not(feature = "full-rt"))]
#[test]
fn runs_net_core_batch() {
    // L2 runtime tests require --features full-rt
}
