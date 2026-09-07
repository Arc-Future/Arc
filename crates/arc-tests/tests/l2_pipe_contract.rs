//! L2 批量：rt_pipe 门面析构契约（RFC 048 §9 M1，full-rt 门控）。
//!
//! 锁定「closed 标志 + 幂等 close + 方法入口守卫」析构契约（rt_pipe.c 状态注释）：
//! Terminate 后全方法安全返回（0/false）、双 Terminate/Dispose 幂等、
//! Terminate 后同名重建（POSIX 残骸接管自愈 / Windows 内核自动回收）、
//! Dispose → Terminate 转发、NamedPipeTransport 行协议（含 \r\n 剥离、
//! UTF-8 多字节、EOF null）。混合 sync/async case，driver 自动 async 化。

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
fn runs_pipe_contract_batch() {
    let results = assert_compiles_and_runs_batch_with_deps(
        "pipe_contract",
        &[
            (
                "pipe_contract_terminate_then_methods",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;

void Main() {
    NamedPipeServerStream server = new NamedPipeServerStream("arc.contract.term");
    NamedPipeClientStream client = new NamedPipeClientStream("arc.contract.term");
    server.Terminate();
    client.Terminate();
    byte[] buf = [0, 0, 0, 0];
    int n = server.Read(buf, 0, 4);
    if (n != 0) { Console.WriteLine("ARC_CASE:pipe_contract_terminate_then_methods:FAIL:read=" + n); return; }
    server.Write(buf, 0, 4);
    client.Write(buf, 0, 4);
    if (server.WaitForConnection()) { Console.WriteLine("ARC_CASE:pipe_contract_terminate_then_methods:FAIL:wait"); return; }
    if (client.Connect(50)) { Console.WriteLine("ARC_CASE:pipe_contract_terminate_then_methods:FAIL:connect"); return; }
    if (server.IsConnected || client.IsConnected) { Console.WriteLine("ARC_CASE:pipe_contract_terminate_then_methods:FAIL:connected"); return; }
    server.Disconnect();
    Console.WriteLine("ARC_CASE:pipe_contract_terminate_then_methods:PASS");
}
"#,
            ),
            (
                "pipe_contract_double_terminate",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;

void Main() {
    NamedPipeServerStream server = new NamedPipeServerStream("arc.contract.dbl");
    server.Terminate();
    server.Terminate();
    server.Dispose();
    NamedPipeClientStream client = new NamedPipeClientStream("arc.contract.dbl");
    client.Dispose();
    client.Dispose();
    client.Terminate();
    Console.WriteLine("ARC_CASE:pipe_contract_double_terminate:PASS");
}
"#,
            ),
            (
                "pipe_contract_recreate_after_close",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;

void Main() {
    NamedPipeServerStream first = new NamedPipeServerStream("arc.contract.recreate");
    if (first == null) { Console.WriteLine("ARC_CASE:pipe_contract_recreate_after_close:FAIL:first"); return; }
    first.Terminate();
    NamedPipeServerStream second = new NamedPipeServerStream("arc.contract.recreate");
    if (second == null) { Console.WriteLine("ARC_CASE:pipe_contract_recreate_after_close:FAIL:recreate"); return; }
    if (!second.IsConnected) {
        // 未连接态预期；仅验证重建成功与可 Terminate（连接路径由 smoke 批覆盖）。
    }
    second.Terminate();
    second.Terminate();
    Console.WriteLine("ARC_CASE:pipe_contract_recreate_after_close:PASS");
}
"#,
            ),
            (
                "pipe_contract_dispose_forwards",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;

void Main() {
    NamedPipeServerStream server = new NamedPipeServerStream("arc.contract.dispose");
    server.Dispose();
    if (server.IsConnected) { Console.WriteLine("ARC_CASE:pipe_contract_dispose_forwards:FAIL:connected"); return; }
    byte[] buf = [1, 2];
    int n = server.Read(buf, 0, 2);
    if (n != 0) { Console.WriteLine("ARC_CASE:pipe_contract_dispose_forwards:FAIL:read=" + n); return; }
    server.Terminate();
    Console.WriteLine("ARC_CASE:pipe_contract_dispose_forwards:PASS");
}
"#,
            ),
            (
                "pipe_transport_lines",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

async Task<void> Main() {
    NamedPipeServerStream server = new NamedPipeServerStream("arc.contract.transport");
    Thread t = new Thread(() => { server.WaitForConnection(); });
    t.Start();
    NamedPipeClientStream client = new NamedPipeClientStream("arc.contract.transport");
    if (!client.Connect(3000)) {
        Console.WriteLine("ARC_CASE:pipe_transport_lines:FAIL:connect");
        return;
    }
    // client.Connect ≠ server.IsConnected（后者由 WaitForConnection 置位）。
    // 固定 Delay 不能代替握手；与 pipe_echo 同构自旋（兼覆盖 Thread+Delay）。
    for (int spin = 0; spin < 500 && !server.IsConnected; spin++) {
        await Task.Delay(10);
    }
    if (!server.IsConnected) {
        Console.WriteLine("ARC_CASE:pipe_transport_lines:FAIL:server-handshake");
        return;
    }
    t.Join();
    NamedPipeTransport serverSide = new NamedPipeTransport(server);
    NamedPipeTransport clientSide = new NamedPipeTransport(client);
    clientSide.WriteLine("hello-pipe-行一");
    clientSide.WriteLine("with\ttab-and-空格");
    clientSide.WriteLine("tail");
    string l1 = serverSide.ReadLine();
    string l2 = serverSide.ReadLine();
    string l3 = serverSide.ReadLine();
    if (l1 != "hello-pipe-行一" || l2 != "with\ttab-and-空格" || l3 != "tail") {
        Console.WriteLine("ARC_CASE:pipe_transport_lines:FAIL:lines=" + l1 + "|" + l2 + "|" + l3);
        return;
    }
    serverSide.WriteLine("BACK:" + l1);
    string back = clientSide.ReadLine();
    if (back != "BACK:hello-pipe-行一") {
        Console.WriteLine("ARC_CASE:pipe_transport_lines:FAIL:back=" + back);
        return;
    }
    clientSide.Close();
    string eof = serverSide.ReadLine();
    if (eof != null) {
        Console.WriteLine("ARC_CASE:pipe_transport_lines:FAIL:eof=" + eof);
        return;
    }
    Console.WriteLine("ARC_CASE:pipe_transport_lines:PASS");
}
"#,
            ),
            (
                "pipe_async_roundtrip",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

async Task<void> Main() {
    NamedPipeServerStream server = new NamedPipeServerStream("arc.contract.async.rt");
    NamedPipeClientStream client = new NamedPipeClientStream("arc.contract.async.rt");
    Task wait = server.WaitForConnectionAsync();
    if (!await client.ConnectAsync(5000)) {
        Console.WriteLine("ARC_CASE:pipe_async_roundtrip:FAIL:connect");
        return;
    }
    await wait;
    if (!server.IsConnected || !client.IsConnected) {
        Console.WriteLine("ARC_CASE:pipe_async_roundtrip:FAIL:handshake");
        return;
    }
    byte[] payload = [72, 0, 105, 33];
    await client.WriteAsync(payload, 0, 4);
    byte[] inbox = [0, 0, 0, 0, 0, 0, 0, 0];
    int n = await server.ReadAsync(inbox, 0, 8);
    if (n != 4) {
        Console.WriteLine("ARC_CASE:pipe_async_roundtrip:FAIL:read=" + n);
        return;
    }
    await server.WriteAsync(inbox, 0, n);
    byte[] back = [0, 0, 0, 0, 0, 0, 0, 0];
    int m = await client.ReadAsync(back, 0, 8);
    if (m != 4 || back[0] != 72 || back[1] != 0 || back[2] != 105 || back[3] != 33) {
        Console.WriteLine("ARC_CASE:pipe_async_roundtrip:FAIL:echo");
        return;
    }
    client.Terminate();
    server.Terminate();
    Console.WriteLine("ARC_CASE:pipe_async_roundtrip:PASS");
}
"#,
            ),
        ],
        &[("Arc.Net.Pipes", "Net/Pipes")],
    );
    assert_all_passed("pipe_contract", &results);
}
