//! L1：POSIX 目标 try/catch 编译门（`arc-eh-001`）——管线级回归。
//!
//! 背景：非 Windows 目标上 zero-cost EH 属里程碑⑨（1.1+，RFC 010）；try/catch
//! 此前在 codegen 深处以 ICE panic 暴露，现由发射前置门
//! `reject_try_catch_outside_windows` 收敛为结构化编译错误。本测试走完整
//! `arc::compile_file` 管线断言错误码与函数名，防止将来回退成 panic；
//! Windows 语义面不受影响（目标无关编译面由既有 L2/e2e 覆盖）。

use std::path::{Path, PathBuf};

use arc::target::TargetTriple;
use arc::{compile_file, CompileOptions, ProjectKind};

const LINUX_TRIPLE: &str = "x86_64-unknown-linux-gnu";

const TRY_CATCH_SRC: &str = r#"using Arc;

void Main() {
    try {
        throw new ArgumentNullException("buf");
    } catch (ArgumentNullException e) {
        if (e.Message.Length < 0) { }
    }
}
"#;

const PLAIN_SRC: &str = r#"using Arc;

void Main() {
    Console.WriteLine("no try/catch here");
}
"#;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// 工作区卫生：项目产物只落 `target/arc-tests/**`。
fn project_dir(label: &str) -> PathBuf {
    repo_root().join(format!("target/arc-tests/l1-eh-{label}"))
}

/// 写项目（arc.toml + Program.as）并进程内编译，返回 `arc::compile_file` 结果。
fn compile_with_target(label: &str, src: &str) -> Result<(), String> {
    let dir = project_dir(label);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create project dir");
    std::fs::write(
        dir.join("arc.toml"),
        "[package]\nname = \"eh_gate_probe\"\nedition = \"1\"\n",
    )
    .expect("write arc.toml");
    let prog = dir.join("Program.as");
    std::fs::write(&prog, src).expect("write Program.as");
    let obj_dir = dir.join("obj/Debug");
    std::fs::create_dir_all(&obj_dir).expect("create obj dir");
    let out = dir.join(if cfg!(windows) { "probe.exe" } else { "probe" });
    let triple = TargetTriple::parse(LINUX_TRIPLE).expect("parse linux triple");
    let cfg = CompileOptions::default();

    let handle = std::thread::Builder::new()
        .name(format!("arc-compile-{label}"))
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            compile_file(
                &prog,
                false,
                false,
                Some(&out),
                Some(&obj_dir),
                Some(&triple),
                ProjectKind::Executable,
                &cfg,
            )
        })
        .expect("spawn compiler thread");
    match handle.join() {
        Ok(res) => {
            let _ = std::fs::remove_dir_all(&dir);
            res
        }
        Err(_) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(format!("{label}: compiler thread panicked"))
        }
    }
}

#[test]
fn linux_target_try_catch_errors_with_eh_gate() {
    let msg = compile_with_target("try", TRY_CATCH_SRC).expect_err("linux try/catch must fail");
    assert!(
        msg.contains("arc-eh-001"),
        "missing arc-eh-001 diagnostic code: {msg}"
    );
    assert!(msg.contains("try/catch"), "missing construct name: {msg}");
    assert!(msg.contains("Main"), "missing function name: {msg}");
}

#[test]
fn linux_target_without_try_catch_is_not_gated() {
    let res = compile_with_target("plain", PLAIN_SRC);
    // 门只拦 try/catch：无 try/catch 的代码不得报 arc-eh-001（成功或其它
    // 阶段错误（如宿主侧缺 Linux 工具链）均与编译门无关）。
    match res {
        Ok(()) => {}
        Err(msg) => assert!(
            !msg.contains("arc-eh-001") && !msg.contains("try/catch"),
            "plain program must not hit the EH gate: {msg}"
        ),
    }
}
