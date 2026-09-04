//! L2 批量：rt_pipe 命名管道 M0 冒烟集（RFC 048 §6，full-rt 门控）。
//!
//! 覆盖：双工字节回环（含 0x00 载荷——无 NUL 截断证据）、对端关闭 EOF、
//! 对端断开后写入存活（Windows ERROR_BROKEN_PIPE / POSIX SIGPIPE 抑制 +
//! EPIPE→0，RFC 048 §3.1-1）、接入超时语义、名字规范化（§5.1-3）。
//! 服务端 WaitForConnection 经 Thread 后台执行——POSIX FIFO 阻塞 open
//! 语义下无死锁的可移植序列（§5.2）。

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
fn runs_pipe_smoke_batch() {
    let results = assert_compiles_and_runs_batch_with_deps(
        "pipe_smoke",
        &[
            (
                "pipe_smoke_roundtrip",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

class PipeHost {
    private NamedPipeServerStream _server;
    public bool Served;

    public PipeHost(string name) {
        _server = new NamedPipeServerStream(name);
        this.Served = false;
    }

    public void ServeInBackground() {
        Thread t = new Thread(() => { _server.WaitForConnection(); this.Served = true; });
        t.Start();
    }

    public NamedPipeServerStream Server { get { return _server; } }
}

async Task<void> Main() {
    PipeHost host = new PipeHost("arc.test.roundtrip");
    host.ServeInBackground();
    NamedPipeClientStream client = new NamedPipeClientStream("arc.test.roundtrip");
    bool connected = client.Connect(3000);
    if (!connected) {
        Console.WriteLine("ARC_CASE:pipe_smoke_roundtrip:FAIL:connect");
        return;
    }
    await Task.Delay(100);
    if (!host.Served || !host.Server.IsConnected || !client.IsConnected) {
        Console.WriteLine("ARC_CASE:pipe_smoke_roundtrip:FAIL:served=" + host.Served);
        return;
    }
    byte[] payload = [72, 101, 0, 108, 111, 0, 33];
    client.Write(payload, 0, 7);
    byte[] inbox = [0, 0, 0, 0, 0, 0, 0, 0];
    int n = host.Server.Read(inbox, 0, 8);
    if (n != 7) {
        Console.WriteLine("ARC_CASE:pipe_smoke_roundtrip:FAIL:server-read=" + n);
        return;
    }
    host.Server.Write(inbox, 0, n);
    byte[] back = [0, 0, 0, 0, 0, 0, 0, 0];
    int m = client.Read(back, 0, 8);
    bool ok = m == 7;
    if (ok) {
        for (int i = 0; i < 7; i++) {
            if (back[i] != payload[i]) {
                ok = false;
                break;
            }
        }
    }
    if (ok) {
        Console.WriteLine("ARC_CASE:pipe_smoke_roundtrip:PASS");
    } else {
        Console.WriteLine("ARC_CASE:pipe_smoke_roundtrip:FAIL:m=" + m);
    }
}
"#,
            ),
            (
                "pipe_smoke_eof",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

class EofHost {
    private NamedPipeServerStream _server;
    public bool Served;

    public EofHost(string name) {
        _server = new NamedPipeServerStream(name);
        this.Served = false;
    }

    public void ServeInBackground() {
        Thread t = new Thread(() => { _server.WaitForConnection(); this.Served = true; });
        t.Start();
    }

    public NamedPipeServerStream Server { get { return _server; } }
}

async Task<void> Main() {
    EofHost host = new EofHost("arc.test.eof");
    host.ServeInBackground();
    NamedPipeClientStream client = new NamedPipeClientStream("arc.test.eof");
    if (!client.Connect(3000)) {
        Console.WriteLine("ARC_CASE:pipe_smoke_eof:FAIL:connect");
        return;
    }
    await Task.Delay(100);
    byte[] payload = [1, 2, 3];
    client.Write(payload, 0, 3);
    byte[] inbox = [0, 0, 0, 0];
    int n = host.Server.Read(inbox, 0, 4);
    if (n != 3) {
        Console.WriteLine("ARC_CASE:pipe_smoke_eof:FAIL:first-read=" + n);
        return;
    }
    client.Dispose();
    await Task.Delay(50);
    int eof = host.Server.Read(inbox, 0, 4);
    if (eof == 0) {
        Console.WriteLine("ARC_CASE:pipe_smoke_eof:PASS");
    } else {
        Console.WriteLine("ARC_CASE:pipe_smoke_eof:FAIL:eof=" + eof);
    }
}
"#,
            ),
            (
                "pipe_smoke_write_closed_peer",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

class WcHost {
    private NamedPipeServerStream _server;
    public bool Served;

    public WcHost(string name) {
        _server = new NamedPipeServerStream(name);
        this.Served = false;
    }

    public void ServeInBackground() {
        Thread t = new Thread(() => { _server.WaitForConnection(); this.Served = true; });
        t.Start();
    }

    public NamedPipeServerStream Server { get { return _server; } }
}

async Task<void> Main() {
    WcHost host = new WcHost("arc.test.wc");
    host.ServeInBackground();
    NamedPipeClientStream client = new NamedPipeClientStream("arc.test.wc");
    if (!client.Connect(3000)) {
        Console.WriteLine("ARC_CASE:pipe_smoke_write_closed_peer:FAIL:connect");
        return;
    }
    await Task.Delay(100);
    host.Server.Disconnect();
    await Task.Delay(50);
    byte[] payload = [9, 8, 7];
    client.Write(payload, 0, 3);
    byte[] inbox = [0, 0, 0, 0];
    int eof = client.Read(inbox, 0, 4);
    if (eof == 0) {
        Console.WriteLine("ARC_CASE:pipe_smoke_write_closed_peer:PASS");
    } else {
        Console.WriteLine("ARC_CASE:pipe_smoke_write_closed_peer:FAIL:eof=" + eof);
    }
}
"#,
            ),
            (
                "pipe_smoke_connect_timeout",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

class CtHost {
    private NamedPipeServerStream _server;
    public bool Served;

    public CtHost(string name) {
        _server = new NamedPipeServerStream(name);
        this.Served = false;
    }

    public void ServeInBackground() {
        Thread t = new Thread(() => { _server.WaitForConnection(); this.Served = true; });
        t.Start();
    }

    public NamedPipeServerStream Server { get { return _server; } }
}

async Task<void> Main() {
    NamedPipeClientStream client = new NamedPipeClientStream("arc.test.ct");
    bool timedOut = !client.Connect(120);
    if (!timedOut) {
        Console.WriteLine("ARC_CASE:pipe_smoke_connect_timeout:FAIL:no-server-connected");
        return;
    }
    CtHost host = new CtHost("arc.test.ct");
    host.ServeInBackground();
    if (!client.Connect(3000)) {
        Console.WriteLine("ARC_CASE:pipe_smoke_connect_timeout:FAIL:reconnect");
        return;
    }
    await Task.Delay(100);
    if (!host.Served) {
        Console.WriteLine("ARC_CASE:pipe_smoke_connect_timeout:FAIL:served");
        return;
    }
    byte[] payload = [5, 5, 5];
    client.Write(payload, 0, 3);
    byte[] inbox = [0, 0, 0, 0];
    int n = host.Server.Read(inbox, 0, 4);
    if (n == 3) {
        Console.WriteLine("ARC_CASE:pipe_smoke_connect_timeout:PASS");
    } else {
        Console.WriteLine("ARC_CASE:pipe_smoke_connect_timeout:FAIL:n=" + n);
    }
}
"#,
            ),
            (
                "pipe_smoke_name_normalize",
                r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

class NnHost {
    private NamedPipeServerStream _server;
    public bool Served;

    public NnHost(string name) {
        _server = new NamedPipeServerStream(name);
        this.Served = false;
    }

    public void ServeInBackground() {
        Thread t = new Thread(() => { _server.WaitForConnection(); this.Served = true; });
        t.Start();
    }

    public NamedPipeServerStream Server { get { return _server; } }
}

async Task<void> Main() {
    NnHost host = new NnHost("arc ipc smoke t1");
    host.ServeInBackground();
    NamedPipeClientStream client = new NamedPipeClientStream("arc ipc smoke t1");
    if (!client.Connect(3000)) {
        Console.WriteLine("ARC_CASE:pipe_smoke_name_normalize:FAIL:connect");
        return;
    }
    await Task.Delay(100);
    byte[] payload = [64, 0, 65, 0, 66];
    client.Write(payload, 0, 5);
    byte[] inbox = [0, 0, 0, 0, 0, 0];
    int n = host.Server.Read(inbox, 0, 6);
    if (n == 5 && inbox[0] == 64 && inbox[1] == 0 && inbox[4] == 66) {
        Console.WriteLine("ARC_CASE:pipe_smoke_name_normalize:PASS");
    } else {
        Console.WriteLine("ARC_CASE:pipe_smoke_name_normalize:FAIL:n=" + n);
    }
}
"#,
            ),
        ],
        &[("Arc.Net.Pipes", "Net/Pipes")],
    );
    assert_all_passed("pipe_smoke", &results);
}
