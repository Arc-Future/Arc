//! L2 批量：rt_pipe 跨进程 echo（RFC 048 §9 M1，full-rt 门控）。
//!
//! 真·双进程回环：父进程经 `Environment.SelfProcessPath()` spawn 自身，
//! 子进程以 argv 角色标记进入 echo server 分支（`Environment.Exit` 收口防
//! driver 继续跑后续 case）；父进程经 NamedPipeClientStream + Transport
//! 完成 16 行（含 UTF-8 多字节）跨进程回环、BYE 收束、子进程退出码校验。
//! 子进程 stdout/stderr 双重定向 + Thread 吸干（防管道满阻塞 + 隔离
//! ARC_CASE 标记污染父进程 stdout 协议流）。

#[cfg(feature = "full-rt")]
use arc_tests::assert_compiles_and_runs_batch_with_deps;

#[cfg(feature = "full-rt")]
#[test]
fn runs_pipe_echo_batch() {
    let results = assert_compiles_and_runs_batch_with_deps(
        "pipe_echo",
        &[(
            "pipe_echo_cross_process",
            r#"using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;
using Arc.Diagnostics;

async Task<void> Main() {
    string role = Environment.ArgCount() > 1 ? Environment.GetArg(1) : "";
    if (role == "--arc-pipe-echo-server") {
        string name = Environment.ArgCount() > 2 ? Environment.GetArg(2) : "arc.ipc.echo.cross";
        NamedPipeServerStream server = new NamedPipeServerStream(name);
        Thread t = new Thread(() => { server.WaitForConnection(); });
        t.Start();
        for (int spin = 0; spin < 1000 && !server.IsConnected; spin++) {
            await Task.Delay(10);
        }
        if (!server.IsConnected) {
            Environment.Exit(2);
        }
        NamedPipeTransport transport = new NamedPipeTransport(server);
        for (;;) {
            string line = transport.ReadLine();
            if (line == null) {
                break;
            }
            if (line == "BYE") {
                break;
            }
            transport.WriteLine("ECHO:" + line);
        }
        Environment.Exit(0);
        return;
    }
    string selfExe = Environment.SelfProcessPath();
    if (selfExe == null || selfExe.Length == 0) {
        Console.WriteLine("ARC_CASE:pipe_echo_cross_process:FAIL:self-path");
        return;
    }
    Process child = new Process();
    child.StartInfo.FileName = selfExe;
    child.StartInfo.Arguments = "--arc-pipe-echo-server arc.ipc.echo.cross";
    child.StartInfo.RedirectStandardOutput = true;
    child.StartInfo.RedirectStandardError = true;
    child.StartInfo.CreateNoWindow = true;
    child.Start();
    Thread outDrain = new Thread(() => { string? l = child.StandardOutput.ReadLine(); while (l != null) { l = child.StandardOutput.ReadLine(); } });
    Thread errDrain = new Thread(() => { string? l = child.StandardError.ReadLine(); while (l != null) { l = child.StandardError.ReadLine(); } });
    outDrain.Start();
    errDrain.Start();

    NamedPipeClientStream client = new NamedPipeClientStream("arc.ipc.echo.cross");
    if (!client.Connect(10000)) {
        Console.WriteLine("ARC_CASE:pipe_echo_cross_process:FAIL:connect");
        child.Kill();
        return;
    }
    NamedPipeTransport clientSide = new NamedPipeTransport(client);
    for (int i = 0; i < 16; i++) {
        clientSide.WriteLine("cross-proc-line-" + i + "-载荷");
    }
    clientSide.WriteLine("BYE");
    for (int i = 0; i < 16; i++) {
        string echoed = clientSide.ReadLine();
        if (echoed != "ECHO:cross-proc-line-" + i + "-载荷") {
            Console.WriteLine("ARC_CASE:pipe_echo_cross_process:FAIL:echo-" + i + "=" + echoed);
            child.Kill();
            return;
        }
    }
    client.Terminate();
    if (!child.WaitForExit(5000)) {
        Console.WriteLine("ARC_CASE:pipe_echo_cross_process:FAIL:child-exit-timeout");
        child.Kill();
        return;
    }
    if (child.ExitCode != 0) {
        Console.WriteLine("ARC_CASE:pipe_echo_cross_process:FAIL:exit-code=" + child.ExitCode);
        return;
    }
    Console.WriteLine("ARC_CASE:pipe_echo_cross_process:PASS");
}
"#,
        )],
        &[("Arc.Net.Pipes", "Net/Pipes")],
    );
    for r in &results {
        assert!(
            r.passed,
            "pipe_echo: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}
