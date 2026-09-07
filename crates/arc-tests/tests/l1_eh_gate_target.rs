//! L1：POSIX 目标 try/catch（里程碑⑨ Itanium）——管线级回归。
//!
//! 背景：原 `arc-eh-001` 硬拒 POSIX try/catch；现 codegen 发射
//! `landingpad` + `__gxx_personality_v0`，本测试断言 Linux 目标上
//! try/catch **不再**报 arc-eh-001（允许因缺交叉链接器等其它错误）。
//! 真机可运行验收见 WSL/`l2_eh_batch`（full-rt）。

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
fn linux_target_try_catch_is_not_eh_gated() {
    // 里程碑⑨：不得再报 arc-eh-001；交叉链接失败等其它错误可接受。
    match compile_with_target("try", TRY_CATCH_SRC) {
        Ok(()) => {}
        Err(msg) => assert!(
            !msg.contains("arc-eh-001"),
            "POSIX try/catch must not hit arc-eh-001 after Itanium EH: {msg}"
        ),
    }
}

#[test]
fn linux_target_without_try_catch_is_not_gated() {
    let res = compile_with_target("plain", PLAIN_SRC);
    match res {
        Ok(()) => {}
        Err(msg) => assert!(
            !msg.contains("arc-eh-001") && !msg.contains("try/catch"),
            "plain program must not hit the EH gate: {msg}"
        ),
    }
}
