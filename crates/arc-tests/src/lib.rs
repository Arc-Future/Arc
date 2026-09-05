//! 进程内快测框架（L1）：直接调用编译器库层把 `.as` 源码编译到底，不 spawn
//! 子进程、不链接运行产物。
//!
//! arc-integration 旧模式的成本是每个测试 spawn 一个独立 `arc.exe` 跑完整管线、
//! 链接 + 运行。本框架在**单进程内**复用它，并把「合法程序应编译通过」的用例
//! 合并为一次编译，测试迭代从「小时级」降到「秒级/分钟级」。
//!
//! 分层：
//! - **L1 快测（默认 `cargo test`）**：[`assert_compiles`] / [`assert_rejected`] /
//!   进程内编译并断言结果或诊断。compile-rejected 类在 codegen 前即出错，无 clang
//!   也可运行。
//! - **L2 运行时（`--features full-rt` 门控）**：需要真实原生二进制运行行为
//!   （std 运行时、进程/网络/TLS/GPU）的用例，走批量编译一次 + 运行一次。
//!
//! 与 `arc-integration` 的取舍：编译正确性（L1）与运行时行为（L2）分治，进度可见、
//! 构建保绿。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use arc::{CompileOptions, ProjectKind};

/// 串行化进程内编译：编译管线存在共享状态（equipment/LLVM 发射），跨测试并发
/// 跑会在本进程中竞争（旧 subprocess 模式天然隔离无此问题）。以锁串行换取正确性。
static COMPILE_LOCK: Mutex<()> = Mutex::new(());

/// 仓库根。
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// 单个 e2e 项目的隔离目录（工作区卫生：产物仅落在 `target/arc-tests/**`）。
fn project_dir(name: &str) -> PathBuf {
    workspace_root().join(format!("target/arc-tests/{name}"))
}

/// clang 是否可用（compile-ok / L2 用例需要链接；compile-rejected 不需要）。
pub fn clang_available() -> bool {
    if Command::new("clang")
        .arg("--version")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return true;
    }
    if cfg!(windows) {
        for p in [
            r"C:\Program Files\LLVM\bin\clang.exe",
            r"C:\Program Files (x86)\LLVM\bin\clang.exe",
        ] {
            if Path::new(p).exists() {
                return true;
            }
        }
    }
    false
}

/// 写入一个含 `arc.toml` 的临时项目（RFC 034：编译需落于有清单的项目根）。
///
/// `extra_deps` 每项为 `(包名, 相对 std/ 的目录)`，为不用 `using Arc;` 之外的其他
/// std 子库时声明 path-only 依赖。
fn write_project(name: &str, src: &str, extra_deps: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = project_dir(name);
    std::fs::create_dir_all(&dir).expect("create arc-tests project dir");
    let mut deps = String::new();
    for (pkg, dir_rel) in extra_deps {
        deps.push_str(&format!(
            "\"{pkg}\" = {{ path = \"../../std/{dir_rel}\" }}\n"
        ));
    }
    std::fs::write(
        dir.join("arc.toml"),
        format!(
            "[package]\nname = \"{}\"\nedition = \"1\"\n\n[dependencies]\n{deps}",
            name.replace('_', "-")
        ),
    )
    .expect("write arc.toml");
    let prog = dir.join("Program.as");
    std::fs::write(&prog, src).expect("write Program.as");
    prog
}

/// 在**单进程内**把 `Program.as` 编译到底（parse→typeck→mir→codegen→链接）。
///
/// 不 spawn 子进程。返回 `Ok(())` 表示编译通过；`Err` 携带诊断文本。
///
/// 编译器 lowering/codegen 存在深递归，需在**大栈线程**上运行（默认 2MB 测试
/// 栈会栈溢出；旧 subprocess 模式靠子进程的大栈规避）——辅以全局锁串行，避免
/// 并发的进程内编译在共享编译器状态上竞争。
pub fn compile_in_process(
    name: &str,
    src: &str,
    extra_deps: &[(&str, &str)],
) -> Result<(), String> {
    compile_in_process_with(name, src, extra_deps, false)
}

/// [`compile_in_process`] 的保留 IR 变体（RFC 017 产物域，UX 迭代评审 §2.3）。
///
/// 默认构建焚毁 `out.ll`；IR 文本断言类测试（如 l1_ir_codegen）须经此变体
/// 显式保留，才能读回 `obj/Debug/main/out.ll`。
pub fn compile_in_process_keep_ir(
    name: &str,
    src: &str,
    extra_deps: &[(&str, &str)],
) -> Result<(), String> {
    compile_in_process_with(name, src, extra_deps, true)
}

fn compile_in_process_with(
    name: &str,
    src: &str,
    extra_deps: &[(&str, &str)],
    keep_ir: bool,
) -> Result<(), String> {
    let prog = write_project(name, src, extra_deps);
    let dir = project_dir(name);
    let obj_dir = dir.join("obj/Debug");
    std::fs::create_dir_all(&obj_dir).expect("create obj dir");
    let out = dir.join(if cfg!(windows) { "main.exe" } else { "main" });
    let cfg = CompileOptions {
        keep_ir,
        ..CompileOptions::default()
    };

    let _guard = COMPILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let handle = std::thread::Builder::new()
        .name(format!("arc-compile-{name}"))
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            arc::compile_file(
                &prog,
                false,
                false,
                Some(&out),
                Some(&obj_dir),
                None,
                ProjectKind::Executable,
                &cfg,
            )
        })
        .expect("spawn compiler thread");
    match handle.join() {
        Ok(res) => res,
        Err(_) => Err(format!("{name}: compiler thread panicked")),
    }
}

/// L1：源应编译通过。需要 clang（codegen 链接）；无 clang 时跳过。
pub fn assert_compiles(name: &str, src: &str) {
    assert_compiles_with_deps(name, src, &[]);
}

/// L1：源应编译通过（带 std 子库依赖）。
pub fn assert_compiles_with_deps(name: &str, src: &str, extra_deps: &[(&str, &str)]) {
    if !clang_available() {
        eprintln!("skip {name}: clang not found");
        return;
    }
    let res = compile_in_process(name, src, extra_deps);
    assert!(
        res.is_ok(),
        "{name}: expected compile ok, got: {}",
        res.unwrap_err()
    );
}

/// L1 批量：N 个合法 case 合并为**一次**编译调用（核心提速：避免 N 次 std 库加载）。
///
/// 每个 case 的 `Main(` 重命名为 `Case{N}_Run(`，总 `Program.as` 的 `Main` 遍历
/// 调用所有 `Case{N}_Run`。一次 `compile_file` 完成所有断言。
pub fn assert_compiles_batch(name: &str, cases: &[(&str, &str)]) {
    assert_compiles_batch_with_deps(name, cases, &[]);
}

/// L1 批量（带 std 子库依赖）。
pub fn assert_compiles_batch_with_deps(
    name: &str,
    cases: &[(&str, &str)],
    extra_deps: &[(&str, &str)],
) {
    if cases.is_empty() {
        return;
    }
    if !clang_available() {
        eprintln!("skip batch {name}: clang not found");
        return;
    }
    let dir = project_dir(name);
    std::fs::create_dir_all(&dir).expect("create batch dir");
    let obj_dir = dir.join("obj/Debug");
    std::fs::create_dir_all(&obj_dir).expect("create obj dir");

    let mut deps = String::new();
    for (pkg, dir_rel) in extra_deps {
        deps.push_str(&format!(
            "\"{pkg}\" = {{ path = \"../../std/{dir_rel}\" }}\n"
        ));
    }
    std::fs::write(
        dir.join("arc.toml"),
        format!(
            "[package]\nname = \"{}\"\nedition = \"1\"\n\n[dependencies]\n{deps}",
            name.replace('_', "-")
        ),
    )
    .expect("write batch arc.toml");

    let mut combined = String::new();
    for (i, (_, src)) in cases.iter().enumerate() {
        let prefix = format!("Case{i}");
        let modified = src.replace("Main(", &format!("{prefix}_Run("));
        combined.push_str(&format!("// === Case {} ===\n", cases[i].0));
        combined.push_str(&modified);
        combined.push('\n');
    }

    let mut driver = String::from("void Main() {\n");
    for i in 0..cases.len() {
        let prefix = format!("Case{i}");
        driver.push_str(&format!("    {prefix}_Run();\n"));
    }
    driver.push_str("}\n");
    combined.push_str(&driver);

    let prog = dir.join("Program.as");
    std::fs::write(&prog, &combined).expect("write combined Program.as");
    let out = dir.join(if cfg!(windows) {
        "batch_main.exe"
    } else {
        "batch_main"
    });
    let cfg = CompileOptions::default();

    let _guard = COMPILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let handle = std::thread::Builder::new()
        .name(format!("arc-batch-{name}"))
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            arc::compile_file(
                &prog,
                false,
                false,
                Some(&out),
                Some(&obj_dir),
                None,
                ProjectKind::Executable,
                &cfg,
            )
        })
        .expect("spawn batch compiler thread");

    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            panic!(
                "{name}: batch compile failed with {} case(s). Check case_*.as files. Error:\n{e}",
                cases.len()
            );
        }
        Err(_) => panic!("{name}: batch compiler thread panicked"),
    }
}

/// L2：批量运行时测试结果（门控在 `feature = \"full-rt\"`）。
#[cfg(feature = "full-rt")]
pub struct BatchRunResult {
    pub name: String,
    pub passed: bool,
    pub stdout: String,
    pub error: Option<String>,
}

/// 批运行 watchdog：单 case 无进展超时秒数（§7.3/§7.4 债务 4）。
/// 观测基线：l2_net 单跑 ~8s、其余批 <60s；§7.5 取证时挂起 case 60s+ 零输出。
/// 默认 180s（约为正常耗时 2~3 倍余量，兼顾 CI 冷机与受限 VM 调度差异——
/// 全量批跑联载下慢宿主实测可达 125s+，原 120s 会误杀）；可用环境变量
/// `ARC_BATCH_TIMEOUT_SECS` 显式覆盖（缓慢 CI 宿主调大；本地取证可调小）。
#[cfg(feature = "full-rt")]
fn batch_case_timeout_secs() -> u64 {
    std::env::var("ARC_BATCH_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(180)
}

/// L2 批量：N 个 case 合并为**一次**编译 + **一次**运行，逐 case 断言运行时行为。
///
/// 每个 case 的源中 `Main(` 被包装为 `Case{N}_Run`，外层 `Main` 依次调用并通过
/// `ARC_CASE:{name}:BEGIN/PASS/FAIL` 标记切分结果。一次原生二进制运行完成所有断言。
///
/// case 入口支持 `void Main()` 与 `async Task<void> Main()` 两种形态：批内任一
/// case 为 async 时 driver 也生成为 async（EventLoop 驱动），async case 以
/// `await` 调用、sync case 照常直调；全 sync 批保持同步 driver（向后兼容）。
#[cfg(feature = "full-rt")]
pub fn assert_compiles_and_runs_batch(name: &str, cases: &[(&str, &str)]) -> Vec<BatchRunResult> {
    assert_compiles_and_runs_batch_with_deps(name, cases, &[])
}

/// L2 批量（带 std 子库依赖）。
#[cfg(feature = "full-rt")]
pub fn assert_compiles_and_runs_batch_with_deps(
    name: &str,
    cases: &[(&str, &str)],
    extra_deps: &[(&str, &str)],
) -> Vec<BatchRunResult> {
    if cases.is_empty() {
        return Vec::new();
    }
    if !clang_available() {
        eprintln!("skip runs_batch {name}: clang not found");
        return Vec::new();
    }
    let dir = project_dir(name);
    std::fs::create_dir_all(&dir).expect("create runs batch dir");
    let obj_dir = dir.join("obj/Debug");
    std::fs::create_dir_all(&obj_dir).expect("create obj dir");

    let mut deps = String::new();
    for (pkg, dir_rel) in extra_deps {
        deps.push_str(&format!(
            "\"{pkg}\" = {{ path = \"../../std/{dir_rel}\" }}\n"
        ));
    }
    std::fs::write(
        dir.join("arc.toml"),
        format!(
            "[package]\nname = \"{}\"\nedition = \"1\"\n\n[dependencies]\n{deps}",
            name.replace('_', "-")
        ),
    )
    .expect("write runs batch arc.toml");

    let mut combined = String::new();
    for (i, (case_name, src)) in cases.iter().enumerate() {
        let prefix = format!("Case{i}");
        let modified = src.replace("Main(", &format!("{prefix}_Run("));
        combined.push_str(&format!("// === Case {} ===\n", case_name));
        combined.push_str(&modified);
        combined.push_str("\n");
    }

    let case_is_async: Vec<bool> = cases
        .iter()
        .map(|(_, src)| src.contains("async Task<void> Main("))
        .collect();
    let any_async = case_is_async.iter().any(|a| *a);
    let mut driver = String::new();
    if any_async {
        driver.push_str("async Task<void> Main() {\n");
    } else {
        driver.push_str("void Main() {\n");
    }
    for (i, (case_name, _)) in cases.iter().enumerate() {
        let prefix = format!("Case{i}");
        driver.push_str(&format!(
            "    Console.WriteLine(\"ARC_CASE:{case_name}:BEGIN\");\n"
        ));
        if case_is_async[i] {
            driver.push_str(&format!("    await {prefix}_Run();\n"));
        } else {
            driver.push_str(&format!("    {prefix}_Run();\n"));
        }
    }
    driver.push_str("}\n");
    combined.push_str(&driver);

    let prog = dir.join("Program.as");
    std::fs::write(&prog, &combined).expect("write runs combined Program.as");
    let out = dir.join(if cfg!(windows) {
        "runs_batch.exe"
    } else {
        "runs_batch"
    });
    let cfg = CompileOptions::default();
    let out_run = out.clone();

    let _guard = COMPILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let handle = std::thread::Builder::new()
        .name(format!("arc-runs-{name}"))
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            arc::compile_file(
                &prog,
                false,
                false,
                Some(&out),
                Some(&obj_dir),
                None,
                ProjectKind::Executable,
                &cfg,
            )
        })
        .expect("spawn runs compiler thread");

    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            panic!(
                "{name}: runs batch compile failed with {} case(s). Error:\n{e}",
                cases.len()
            );
        }
        Err(_) => panic!("{name}: runs batch compiler thread panicked"),
    }

    // §7.3/§7.4 债务 4：批运行 watchdog。此前 `Command::output()` 无超时——
    // 单 case 挂起（§7.5 实证的 async accept 竞态谱系之一）即全批卡死需人工
    // 介入。现流式读 stdout：BEGIN/PASS/FAIL 实时进度标记（eprintln，配合
    // --nocapture 可观测），per-case 无进展超时即 kill 批进程，把悬挂收敛为
    // 可诊断失败（悬挂 case 注入 watchdog 错误，后续 case 标记未执行）。
    let case_timeout = std::time::Duration::from_secs(batch_case_timeout_secs());
    let mut child = Command::new(&out_run)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("{name}: failed to run batch binary: {e}"));
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    let child_stdout = child.stdout.take().expect("batch stdout piped");
    let stdout_reader = std::thread::spawn(move || {
        use std::io::BufRead as _;
        let reader = std::io::BufReader::new(child_stdout);
        for line in reader.lines().map_while(Result::ok) {
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });
    let child_stderr = child.stderr.take().expect("batch stderr piped");
    let stderr_reader = std::thread::spawn(move || {
        use std::io::BufRead as _;
        let reader = std::io::BufReader::new(child_stderr);
        let mut buf = String::new();
        for line in reader.lines().map_while(Result::ok) {
            buf.push_str(&line);
            buf.push('\n');
        }
        buf
    });

    let mut stdout_lines: Vec<String> = Vec::new();
    let mut current_case: Option<String> = None;
    let mut watchdog_error: Option<(String, String)> = None;
    let mut deadline = std::time::Instant::now() + case_timeout;
    loop {
        let wait = deadline.saturating_duration_since(std::time::Instant::now());
        match line_rx.recv_timeout(wait) {
            Ok(line) => {
                if let Some(rest) = line.trim().strip_prefix("ARC_CASE:") {
                    if let Some((case_name, phase)) = rest.split_once(':') {
                        if phase == "BEGIN" {
                            eprintln!("[batch {name}] case {case_name} BEGIN");
                            current_case = Some(case_name.to_string());
                            deadline = std::time::Instant::now() + case_timeout;
                        } else if phase == "PASS" || phase.starts_with("FAIL") {
                            eprintln!("[batch {name}] case {case_name} {phase}");
                            current_case = None;
                        }
                    }
                }
                stdout_lines.push(line);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let hung = current_case
                    .clone()
                    .unwrap_or_else(|| "<批驱动启动>".to_string());
                /* 临时取证：挂死前 dump 已收集的全部 stdout（含心跳 DIAG 行） */
                eprintln!(
                    "[batch {name}] watchdog stdout dump ({} lines):\n{}",
                    stdout_lines.len(),
                    stdout_lines.join("\n")
                );
                watchdog_error = Some((
                    hung.clone(),
                    format!(
                        "watchdog: case `{hung}` 超过 {}s 无进展，批进程已终止",
                        batch_case_timeout_secs()
                    ),
                ));
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let stderr_text = stderr_reader.join().unwrap_or_default();
    let stdout = stdout_lines.join("\n");
    let mut results = parse_batch_output(&stdout, cases);
    // 批宿主崩溃（非 watchdog kill）同样需要 stderr 可观测：AV/rt_panic 的
    // 唯一线索在 stderr，丢弃即盲区（表现为「case 未终结」无因可查）。
    if !stderr_text.trim().is_empty() {
        eprintln!("[batch {name}] 批进程 stderr:\n{stderr_text}");
    }
    if let Some((hung, msg)) = watchdog_error {
        eprintln!("[batch {name}] {msg}");
        for r in &mut results {
            if r.name == hung {
                r.passed = false;
                r.error = Some(msg.clone());
            }
        }
    }
    results
}

#[cfg(feature = "full-rt")]
fn parse_batch_output(stdout: &str, cases: &[(&str, &str)]) -> Vec<BatchRunResult> {
    let lines: Vec<&str> = stdout.lines().collect();
    let mut results = Vec::new();
    for (case_name, _) in cases {
        let begin = format!("ARC_CASE:{case_name}:BEGIN");
        let pass = format!("ARC_CASE:{case_name}:PASS");
        let fail_prefix = format!("ARC_CASE:{case_name}:FAIL:");
        let mut result = BatchRunResult {
            name: case_name.to_string(),
            passed: false,
            stdout: String::new(),
            error: None,
        };
        if let Some(begin_idx) = lines.iter().position(|l| l.trim() == begin) {
            let mut case_out: Vec<&str> = Vec::new();
            for line in &lines[begin_idx + 1..] {
                let t = line.trim();
                if t == pass {
                    result.passed = true;
                    break;
                }
                if t.starts_with(&fail_prefix) {
                    result.error = Some(t[fail_prefix.len()..].to_string());
                    break;
                }
                case_out.push(*line);
            }
            result.stdout = case_out.join("\n");
            if !result.passed && result.error.is_none() {
                result.error = Some("case 未终结（BEGIN 后无 PASS/FAIL 标记）".into());
            }
        } else {
            result.error = Some("未执行（批进程提前崩溃或标记缺失）".into());
        }
        results.push(result);
    }
    results
}

/// arc CLI 二进制路径（供 L2 批量测试 spawn `arc build` 使用）。
#[cfg(feature = "full-rt")]
pub(crate) fn arc_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_arc") {
        return PathBuf::from(p);
    }
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let name = if cfg!(windows) { "arc.exe" } else { "arc" };
    workspace_root().join(format!("target/{profile}/{name}"))
}

/// L1：源应被拒绝且诊断包含 `needle`（编译期错误，无需 clang）。
pub fn assert_rejected(name: &str, src: &str, needle: &str) {
    assert_rejected_with_deps(name, src, needle, &[]);
}

/// L1：带 std 子库依赖的拒绝断言。
pub fn assert_rejected_with_deps(name: &str, src: &str, needle: &str, extra_deps: &[(&str, &str)]) {
    let err = compile_in_process(name, src, extra_deps);
    let err = err.expect_err(&format!("{name}: expected compile rejection"));
    assert!(
        err.contains(needle),
        "{name}: expected diagnostic containing `{needle}`, got:\n{err}"
    );
}

/// 从 manifest 文本解析 project kind。缺省为 Executable。
fn parse_project_kind(manifest: &str) -> ProjectKind {
    for line in manifest.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("kind") {
            let rest = rest.trim_start();
            if let Some(val) = rest.strip_prefix('=') {
                let val = val.trim().trim_matches('"').trim_matches('\'');
                match val {
                    "library" => return ProjectKind::Library,
                    "executable" => return ProjectKind::Executable,
                    _ => {}
                }
            }
        }
    }
    ProjectKind::Executable
}

/// L1：多文件项目编译断言。
///
/// `files` 为 `(相对路径, 内容)` 映射，`manifest` 为完整 `arc.toml` 内容（含
/// 自定义 `namespace`/`global_usings`/`dependencies`）。入口文件必须为 `Program.as`。
pub fn assert_compiles_project(name: &str, files: &[(&str, &str)], manifest: &str) {
    if !clang_available() {
        eprintln!("skip project {name}: clang not found");
        return;
    }
    let dir = project_dir(name);
    std::fs::create_dir_all(&dir).expect("create project dir");
    let obj_dir = dir.join("obj/Debug");
    std::fs::create_dir_all(&obj_dir).expect("create obj dir");

    std::fs::write(dir.join("arc.toml"), manifest).expect("write arc.toml");
    for (path, content) in files {
        let file_path = dir.join(path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("create file parent dir");
        }
        std::fs::write(&file_path, content).expect("write project file");
    }

    let prog = dir.join("Program.as");
    let is_lib = parse_project_kind(manifest) == ProjectKind::Library;
    let out_name = if is_lib {
        if cfg!(windows) {
            "proj_lib.dll"
        } else {
            "libproj_lib.so"
        }
    } else {
        if cfg!(windows) {
            "proj_main.exe"
        } else {
            "proj_main"
        }
    };
    let out = dir.join(out_name);
    let cfg = CompileOptions::default();
    let kind = parse_project_kind(manifest);

    let _guard = COMPILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let handle = std::thread::Builder::new()
        .name(format!("arc-proj-{name}"))
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            arc::compile_file(
                &prog,
                false,
                false,
                Some(&out),
                Some(&obj_dir),
                None,
                kind,
                &cfg,
            )
        })
        .expect("spawn project compiler thread");

    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("{name}: project compile failed. Error:\n{e}"),
        Err(_) => panic!("{name}: project compiler thread panicked"),
    }
}

/// L1：多包项目编译断言。
///
/// `packages` 为 `(包名, 相对路径, manifest, 文件列表)` 映射。第一个包为入口包。
/// 每个文件列表为 `(相对路径, 内容)`。
pub fn assert_compiles_multipackage(
    name: &str,
    packages: &[(&str, &str, &str, Vec<(&str, &str)>)],
) {
    if !clang_available() {
        eprintln!("skip multipackage {name}: clang not found");
        return;
    }
    let root = project_dir(name);
    std::fs::create_dir_all(&root).expect("create multipackage root");

    let mut pkg_dirs: Vec<PathBuf> = Vec::new();
    for (_pkg_name, pkg_path, manifest, files) in packages {
        let pkg_dir = root.join(pkg_path);
        std::fs::create_dir_all(&pkg_dir).expect("create pkg dir");
        let obj_dir = pkg_dir.join("obj/Debug");
        std::fs::create_dir_all(&obj_dir).expect("create pkg obj dir");

        std::fs::write(pkg_dir.join("arc.toml"), manifest).expect("write pkg arc.toml");
        for (file_path, content) in files {
            let fp = pkg_dir.join(file_path);
            if let Some(parent) = fp.parent() {
                std::fs::create_dir_all(parent).expect("create file parent dir");
            }
            std::fs::write(&fp, content).expect("write pkg file");
        }
        pkg_dirs.push(pkg_dir.clone());
    }

    let main_pkg_dir = pkg_dirs.first().expect("at least one package").clone();
    let prog = main_pkg_dir.join("Program.as");
    let out = main_pkg_dir.join(if cfg!(windows) {
        "mp_main.exe"
    } else {
        "mp_main"
    });
    let obj_dir = main_pkg_dir.join("obj/Debug");
    let cfg = CompileOptions::default();

    let _guard = COMPILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let handle = std::thread::Builder::new()
        .name(format!("arc-mp-{name}"))
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            arc::compile_file(
                &prog,
                false,
                false,
                Some(&out),
                Some(&obj_dir),
                None,
                ProjectKind::Executable,
                &cfg,
            )
        })
        .expect("spawn multipackage compiler thread");

    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("{name}: multipackage compile failed. Error:\n{e}"),
        Err(_) => panic!("{name}: multipackage compiler thread panicked"),
    }
}

/// L2：把插件 `.as` 源码编译为动态库（`.dll`/`.so`），供宿主经
/// AssemblyLoadContext 加载（UX 迭代评审 §2.5 动态加载批的测试供给）。
///
/// `arc build --dynamic` 的进程内等价物：单文件直通（无需 arc.toml）；
/// `dependencies` 经 `__arc_package_meta` 嵌入依赖键，宿主端 LoadDependencies
/// 按 requestingAssembly 所在目录递归解析。产物与源码平铺同一目录——依赖
/// 解析的第一优先级即请求方目录，平铺使传递依赖免配探针路径。
///
/// 返回产物绝对路径；宿主端以 `plugin_name` 走 `LoadByName` 探针命中，
/// `_loaded` 键为探针拼接的路径（`<注入目录>/<plugin_name>.<平台扩展名>`）。
#[cfg(feature = "full-rt")]
pub fn compile_plugin_library(
    batch: &str,
    plugin_name: &str,
    src: &str,
    dependencies: &[&str],
) -> PathBuf {
    // 插件是运行时 fixture，与批宿主项目无编译期关系，放 project_dir 的兄弟
    // 目录：批宿主 write_project 会在 project_dir 写 arc.toml（包名=批名），
    // 若插件源码位于其子目录，后续运行的插件编译会沿目录树发现该 arc.toml，
    // 要求命名空间根与批包名匹配而炸掉（`u5-dynamic-load-batch` != `PluginU5`）。
    // 兄弟目录祖先链上无任何 arc.toml，插件编译始终保持单文件直通语义。
    let dir = project_dir(batch)
        .parent()
        .expect("project dir always has a parent")
        .join(format!("{batch}-plugins"));
    std::fs::create_dir_all(&dir).expect("create plugins dir");
    let source = dir.join(format!("{plugin_name}.as"));
    std::fs::write(&source, src).expect("write plugin source");
    let ext = if cfg!(windows) { ".dll" } else { ".so" };
    let output = dir.join(format!("{plugin_name}{ext}"));
    let obj_dir = dir.join(format!("obj-{plugin_name}"));
    let meta = arc::PackageMeta {
        name: plugin_name.to_string(),
        version: "1.0.0".to_string(),
        edition: "1".to_string(),
        dependencies: dependencies.iter().map(|d| d.to_string()).collect(),
        // 布局指纹表由 codegen 在 compile_module_to_dynamic_library 内
        // 按 layouts 填充。
        layout_sigs: Vec::new(),
    };
    // 导出符号是否进入 dll 导出表取决于 target 判定：Windows MSVC 链接器
    // （lld-link）默认不导出数据符号，须按 `-Wl,/EXPORT:` 显式列出；而该分支
    // 以传入 target 是否含 "windows-msvc" 为准——target=None 时即使本机
    // clang 默认 triple 就是 msvc，也会被判为非 MSVC 而跳过导出标志，导致
    // `__arc_package_meta`/`__arc_module_roots` 等全部约定符号缺席导出表
    // （依赖递归加载因此静默失效）。与 CLI 动态库路径同源取宿主 triple；
    // host() 返回值须先绑定局部再 move 进闭包（调用点按引用借用）。
    let target = arc::target::TargetTriple::host();
    let _guard = COMPILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let output_in_thread = output.clone();
    let handle = std::thread::Builder::new()
        .name(format!("arc-plugin-{plugin_name}"))
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            arc::compile_file_to_dynamic_library(
                &source,
                false,
                false,
                Some(&output_in_thread),
                Some(&obj_dir),
                Some(&target),
                &[],
                &[],
                Some(meta),
                &CompileOptions::default(),
            )
        })
        .expect("spawn plugin compiler thread");
    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("plugin `{plugin_name}` compile failed. Error:\n{e}"),
        Err(_) => panic!("plugin `{plugin_name}` compiler thread panicked"),
    }
    output
}

/// L2 批量运行时测试助手（门控在 `feature = "full-rt"`）。
#[cfg(feature = "full-rt")]
pub mod batch;
