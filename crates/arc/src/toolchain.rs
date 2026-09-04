//! `arc toolchain`：按需工具链安装（Phase 2）。
//!
//! ## 布局
//!
//! ```text
//! <tools_root>/                        ← $ARC_HOME/tools（未设则 ~/.arc/tools）
//! └── llvm/
//!     ├── current                      ← 活动版本指针（内容 = 版本号，如 `22.1.8`）
//!     └── <ver>/                       ← 版本目录（多版本共存）
//!         └── bin/clang[.exe]
//! ```
//!
//! ## 与 clang 解析联动（单一解析序）
//!
//! `codegen::sdk_layout::toolchain_llvm_clang_path()` 读取 `llvm/current` 指针，
//! `codegen::clang_path()` 在 `ARC_CLANG` 之后、标准安装位之前解析 toolchain
//! 管理的 clang——安装后 `arc build` / `arc env` / `arc doctor` 自动使用该
//! clang，无需环境变量。安装同时可选写用户环境 `ARC_CLANG`（`--set-env`，
//! 默认开启；`--no-set-env` 关闭），供外部工具与新 shell 一致消费。
//!
//! 幂等与联动：
//! - 目标版本已安装 → 更新 `current` 指针并结束。
//! - `ARC_CLANG` 或 PATH 已有可用 clang → 提示「已就绪」并跳过下载
//!   （`--force` 强制重装）。
//!
//! ## 明确不在本切片（外部依赖，见 RFC 031 §12）
//!
//! - 真实 LLVM 下载端点（`LLVM_DOWNLOAD_TEMPLATE` 为占位；测试/离线用
//!   `--archive`，`--sha256` 可选校验）。
//! - tar.xz / 官方安装器解析（真实端点定版后补齐）。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::archive::extract_zip;
use crate::clang_version;
use crate::download::http_get_bytes;
use crate::env::{env_var, ARC_CLANG_ENV};
use crate::hash::content_sha256;

/// 工具链名（当前仅 `llvm`）。
pub const TOOLCHAIN_NAME_LLVM: &str = "llvm";
/// 默认 LLVM 版本（对齐 `.aopkg` metadata `llvm_version` 与 manifest `clang_min_version`）。
pub const DEFAULT_LLVM_VERSION: &str = "22.1.8";
/// LLVM 下载模板（占位——真实发布端点见 RFC 031 §12 外部依赖）。
pub const LLVM_DOWNLOAD_TEMPLATE: &str =
    "https://github.com/llvm/llvm-project/releases/download/llvmorg-{version}/{artifact}";
/// `llvm/current` 指针文件名（内容 = 活动版本号）。
pub const TOOLCHAIN_CURRENT_FILE: &str = "current";

/// `arc toolchain install` 参数（由 CLI 构建）。
#[derive(Debug, Clone, Default)]
pub struct ToolchainInstallOptions {
    /// 工具链名（`llvm`）。
    pub tool: String,
    /// 目标版本（缺省 `DEFAULT_LLVM_VERSION`）。
    pub version: Option<String>,
    /// 显式下载 URL（覆盖占位模板）。
    pub url: Option<String>,
    /// 本地分发包（`--archive`；测试/离线）。
    pub archive: Option<PathBuf>,
    /// 可选分发包 SHA256（64 hex；提供则校验，不符拒绝安装）。
    pub sha256: Option<String>,
    /// 写用户环境 `ARC_CLANG`（默认 `true`；`--no-set-env` 关闭）。
    pub set_env: bool,
    /// 跳过「clang 已可用 / 已安装」幂等捷径（强制重装）。
    pub force: bool,
}

/// toolchain 工具根：`$ARC_HOME/tools`（未设则 `~/.arc/tools`）。
pub fn tools_root() -> PathBuf {
    codegen::sdk_layout::toolchain_tools_root()
}

/// LLVM 工具链目录：`<tools>/llvm`。
pub fn llvm_dir() -> PathBuf {
    codegen::sdk_layout::toolchain_llvm_dir()
}

/// 活动 LLVM 版本（`llvm/current` 指针；未设为 `None`）。
pub fn current_version() -> Option<String> {
    read_current(&llvm_dir())
}

/// clang 可执行文件名（平台后缀）。
pub fn clang_exe_name() -> &'static str {
    if cfg!(windows) {
        "clang.exe"
    } else {
        "clang"
    }
}

/// 版本目录内 clang 路径。
fn clang_path(version_dir: &Path) -> PathBuf {
    version_dir.join("bin").join(clang_exe_name())
}

/// 单个工具链条目（`arc toolchain list` 输出）。
#[derive(Debug, Clone)]
pub struct ToolchainEntry {
    pub version: String,
    pub dir: PathBuf,
    pub clang: PathBuf,
    pub active: bool,
    /// 未达 clang 支持基线时给出说明（达标为 `None`）。
    pub baseline_note: Option<String>,
}

/// 列出已安装工具链（版本目录含 `bin/clang[.exe]`）。
pub fn list() -> Result<Vec<ToolchainEntry>, String> {
    let dir = llvm_dir();
    let current = current_version();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let clang = clang_path(&p);
        if !clang.is_file() {
            continue;
        }
        let baseline_note = probe_baseline_note(&clang);
        out.push(ToolchainEntry {
            version: name.clone(),
            dir: p,
            clang,
            active: current.as_deref() == Some(name.as_str()),
            baseline_note,
        });
    }
    out.sort_by(|a, b| b.version.cmp(&a.version));
    Ok(out)
}

/// 对已装 clang 运行 `--version` 并评估支持基线（失败返回 `None`，不阻断 list）。
fn probe_baseline_note(clang: &Path) -> Option<String> {
    let out = Command::new(clang).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let ver = clang_version::version_from_clang_output(&text)?;
    clang_version::ensure_clang_min_version(ver)
}

/// `arc toolchain list`。
pub fn run_list() -> Result<(), String> {
    let entries = list()?;
    if entries.is_empty() {
        println!(
            "no toolchains installed ({}); `arc toolchain install llvm` to provision",
            llvm_dir().display()
        );
        return Ok(());
    }
    for e in &entries {
        let active = if e.active { " (active)" } else { "" };
        println!("{}{} installed  {}", e.version, active, e.clang.display());
        if let Some(note) = &e.baseline_note {
            println!("    warning: {note}");
        }
    }
    Ok(())
}

/// `arc toolchain status`：工具根、活动版本与 clang 解析结果（与 doctor 同一解析序）。
pub fn run_status() -> Result<(), String> {
    println!("tools root: {}", tools_root().display());
    println!("llvm dir:   {}", llvm_dir().display());
    match current_version() {
        Some(v) => println!("active llvm: {v}"),
        None => println!("active llvm: (none)"),
    }
    let clang = codegen::clang_path();
    println!("clang resolution: {clang}");
    if let Some(c) = &current_version() {
        let clang = clang_path(&llvm_dir().join(c));
        if let Some(note) = probe_baseline_note(&clang) {
            println!("warning: {note}");
        }
    }
    Ok(())
}

/// `arc toolchain install <tool>`（当前仅 `llvm`）。
pub fn run_install(opts: &ToolchainInstallOptions) -> Result<(), String> {
    if opts.tool != TOOLCHAIN_NAME_LLVM {
        return Err(format!(
            "unknown toolchain `{}` (expected: {TOOLCHAIN_NAME_LLVM})",
            opts.tool
        ));
    }
    let version = opts
        .version
        .clone()
        .unwrap_or_else(|| DEFAULT_LLVM_VERSION.to_string());
    let dir = llvm_dir();
    let target = dir.join(&version);

    // 幂等：同版本已装 → 刷新 current 指针并结束。
    if target.join("bin").join(clang_exe_name()).is_file() {
        if !opts.force {
            write_current(&dir, &version)?;
            println!("llvm {version} already installed: {}", target.display());
            return Ok(());
        }
        std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    }

    // 联动：`ARC_CLANG` 或 PATH 已有 clang → 提示已就绪（幂等；`--force` 跳过）。
    if !opts.force {
        if let Some(existing) = clang_already_available() {
            println!("clang already available: {existing}");
            println!(
                "  `arc doctor` uses that clang. To provision an arc-managed LLVM {version} anyway, re-run with `--force`."
            );
            return Ok(());
        }
    }

    // 来源：--archive > --url > 占位模板。
    let bytes = if let Some(archive) = &opts.archive {
        std::fs::read(archive).map_err(|e| format!("read {}: {e}", archive.display()))?
    } else if let Some(url) = &opts.url {
        http_get_bytes(url).map_err(|e| e.to_string())?
    } else {
        let url = placeholder_url(&version);
        return Err(format!(
            "LLVM download endpoint is a Phase 2 placeholder ({url}); pass `--url <url>` or \
             `--archive <file>` (real endpoint tracking: RFC 031 §12 external dependencies)"
        ));
    };

    // 可选 SHA256 校验（提供则不符拒绝安装）。
    if let Some(expected) = &opts.sha256 {
        let actual = content_sha256(&bytes);
        if actual != expected.to_ascii_lowercase() {
            return Err(format!(
                "sha256 mismatch for llvm {version}: expected {}, got {} (refusing to install)",
                expected.to_ascii_lowercase(),
                actual
            ));
        }
        println!("verified: sha256 ok ({actual})");
    }

    // staging 解压 + clang 布局自检。
    let staging = dir.join(format!(".staging-{}", std::process::id()));
    let mut guard = StagingGuard::new(staging.clone());
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    extract_zip(&bytes, &staging)?;
    let staged = resolve_extracted_root(&staging);
    let staged_clang = clang_path(&staged);
    if !staged_clang.is_file() {
        return Err(format!(
            "archive has no `bin/{}` (not an LLVM toolchain layout)",
            clang_exe_name()
        ));
    }
    match Command::new(&staged_clang).arg("--version").output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let line = text.lines().next().unwrap_or("(unknown)").trim();
            println!("clang probe: {line}");
        }
        _ => {
            println!(
                "clang probe: {} (could not run --version; continuing)",
                staged_clang.display()
            );
        }
    }

    // 原子提交：rename staged → target，写 current 指针。
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::fs_util::rename_with_retry(&staged, &target).map_err(|e| {
        format!(
            "rename {} → {} failed: {e}",
            staged.display(),
            target.display()
        )
    })?;
    guard.committed = true;
    write_current(&dir, &version)?;

    let clang = clang_path(&target);
    if opts.set_env {
        set_arc_clang_env(&clang)?;
    }
    println!("installed: llvm {version} → {}", clang.display());
    if opts.set_env {
        println!("  ARC_CLANG updated in the user environment (new terminals only)");
    } else {
        println!(
            "  ARC_CLANG not set (--no-set-env); build already resolves it via tools/llvm/current"
        );
    }
    Ok(())
}

/// `arc toolchain uninstall <tool> [--version <ver>]`：删除版本目录；删除的是
/// 活动版本时同步清除 `current` 指针（`ARC_CLANG` 环境变量不做静默修改，提示用户）。
pub fn run_uninstall(tool: &str, version: Option<&str>) -> Result<(), String> {
    if tool != TOOLCHAIN_NAME_LLVM {
        return Err(format!(
            "unknown toolchain `{tool}` (expected: {TOOLCHAIN_NAME_LLVM})"
        ));
    }
    let dir = llvm_dir();
    let ver = match version {
        Some(v) => v.to_string(),
        None => current_version().ok_or_else(|| {
            "no version specified and no `llvm/current` marker; pass --version <ver>".to_string()
        })?,
    };
    let target = dir.join(&ver);
    if !target.is_dir() {
        return Err(format!(
            "llvm {ver} is not installed ({})",
            target.display()
        ));
    }
    std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    if current_version().as_deref() == Some(ver.as_str()) {
        let pointer = dir.join(TOOLCHAIN_CURRENT_FILE);
        let _ = std::fs::remove_file(&pointer);
        println!("current pointer cleared (llvm {ver} was active)");
        if env_var(ARC_CLANG_ENV).is_some() {
            println!("note: ARC_CLANG still points at the removed clang; unset it if unused");
        }
    }
    println!("uninstalled: llvm {ver}");
    Ok(())
}

/// 探测 `ARC_CLANG` / 标准安装位 / PATH 上已有的 clang（toolchain 管理之外）。
fn clang_already_available() -> Option<String> {
    if let Some(v) = env_var(ARC_CLANG_ENV) {
        let p = PathBuf::from(&v);
        if p.is_file() || v == "clang" {
            return Some(v);
        }
    }
    if cfg!(windows) {
        for p in [
            r"C:\Program Files\LLVM\bin\clang.exe",
            r"C:\Program Files (x86)\LLVM\bin\clang.exe",
        ] {
            if Path::new(p).is_file() {
                return Some(p.to_string());
            }
        }
    }
    let on_path = Command::new("clang")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    on_path.then(|| "clang (PATH)".to_string())
}

/// 占位下载 URL（真实端点定版后替换模板）。
fn placeholder_url(version: &str) -> String {
    LLVM_DOWNLOAD_TEMPLATE
        .replace("{version}", version)
        .replace(
            "{artifact}",
            &format!("LLVM-{version}-{}.zip", host_triple_label()),
        )
}

/// 简写 host 三元组标签（仅用于占位 URL 命名）。
fn host_triple_label() -> String {
    crate::target::TargetTriple::host().as_str().to_string()
}

/// 解压产物根：zip 顶层可能是 `LLVM-<ver>-<triple>` 单目录 → 取之；否则取 staging 本身。
fn resolve_extracted_root(staging: &Path) -> PathBuf {
    let entries: Vec<PathBuf> = std::fs::read_dir(staging)
        .ok()
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    if entries.len() == 1 && entries[0].is_dir() {
        entries[0].clone()
    } else {
        staging.to_path_buf()
    }
}

/// 写 `llvm/current` 指针（临时名 + rename 原子写）。
fn write_current(dir: &Path, version: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.new", TOOLCHAIN_CURRENT_FILE));
    std::fs::write(&tmp, format!("{version}\n")).map_err(|e| e.to_string())?;
    crate::fs_util::rename_with_retry(&tmp, dir.join(TOOLCHAIN_CURRENT_FILE))
        .map_err(|e| format!("update {} failed: {e}", TOOLCHAIN_CURRENT_FILE))
}

fn read_current(dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(dir.join(TOOLCHAIN_CURRENT_FILE)).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 写用户环境 `ARC_CLANG`（Windows `setx`；Unix 追加 `~/.profile`）。
///
/// 注意：本函数只由 CLI 路径调用；测试经 `--no-set-env` 规避真实环境写入。
fn set_arc_clang_env(clang: &Path) -> Result<(), String> {
    let value = clang.display().to_string();
    if cfg!(windows) {
        let out = Command::new("setx")
            .args(["ARC_CLANG", &value])
            .output()
            .map_err(|e| format!("setx failed: {e}"))?;
        if !out.status.success() {
            eprintln!(
                "warning: setx ARC_CLANG failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    } else {
        let profile = user_home().join(".profile");
        let line = format!("export ARC_CLANG=\"{value}\"");
        let mut content = std::fs::read_to_string(&profile).unwrap_or_default();
        if !content.contains("ARC_CLANG") {
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&line);
            content.push('\n');
            std::fs::write(&profile, content)
                .map_err(|e| format!("write {}: {e}", profile.display()))?;
            println!("ARC_CLANG appended to {}", profile.display());
        }
    }
    Ok(())
}

fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// staging 守卫：未提交时删除（无副作用原则）。
struct StagingGuard {
    path: PathBuf,
    committed: bool,
}

impl StagingGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ARC_HOME_ENV;
    use std::io::Write;

    fn temp_dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arc-toolchain-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// `$ARC_HOME` / `$ARC_CLANG` 在并行测试下互斥（crate 级锁）。
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 构造含 `bin/clang[.exe]` 的假 LLVM zip。
    fn make_llvm_zip(version: &str, zip_path: &Path) {
        let file = std::fs::File::create(zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let root = format!("LLVM-{version}-x86_64-pc-windows-msvc");
        zip.start_file(
            format!("{root}/bin/{}", clang_exe_name()),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"fake clang").unwrap();
        zip.finish().unwrap();
    }

    fn install_opts(archive: &Path, set_env: bool, force: bool) -> ToolchainInstallOptions {
        ToolchainInstallOptions {
            tool: TOOLCHAIN_NAME_LLVM.into(),
            version: Some("22.1.8".into()),
            archive: Some(archive.to_path_buf()),
            set_env,
            force,
            ..Default::default()
        }
    }

    #[test]
    fn list_reports_not_installed_first() {
        let _env_guard = env_lock();
        let home = temp_dir("home1");
        std::env::set_var(ARC_HOME_ENV, &home);
        assert!(list().unwrap().is_empty());
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn install_then_list_then_uninstall() {
        let _env_guard = env_lock();
        let home = temp_dir("home2");
        std::env::set_var(ARC_HOME_ENV, &home);
        let zip = home.join("llvm.zip");
        make_llvm_zip("22.1.8", &zip);

        // 本机可能已装系统 clang（`clang_already_available` 短路）→ 强制托管安装。
        run_install(&install_opts(&zip, false, true)).unwrap();
        let entries = list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version, "22.1.8");
        assert!(entries[0].active);
        assert!(entries[0].clang.is_file());
        assert_eq!(current_version().as_deref(), Some("22.1.8"));

        // 幂等：再装同版本不报错、不重复。
        run_install(&install_opts(&zip, false, true)).unwrap();
        assert_eq!(list().unwrap().len(), 1);

        run_uninstall(TOOLCHAIN_NAME_LLVM, None).unwrap();
        assert!(list().unwrap().is_empty());
        assert_eq!(current_version(), None);

        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn install_refuses_sha256_mismatch() {
        let _env_guard = env_lock();
        let home = temp_dir("home3");
        std::env::set_var(ARC_HOME_ENV, &home);
        let zip = home.join("llvm.zip");
        make_llvm_zip("22.1.8", &zip);
        let mut opts = install_opts(&zip, false, true);
        opts.sha256 = Some("00".repeat(32));
        let err = run_install(&opts).unwrap_err();
        assert!(err.contains("sha256 mismatch"), "{err}");
        assert!(list().unwrap().is_empty());
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn install_prompts_ready_when_arc_clang_set() {
        let _env_guard = env_lock();
        let home = temp_dir("home4");
        std::env::set_var(ARC_HOME_ENV, &home);
        std::env::set_var(ARC_CLANG_ENV, "clang");
        let zip = home.join("llvm.zip");
        make_llvm_zip("22.1.8", &zip);
        run_install(&install_opts(&zip, false, false)).unwrap();
        // 不下载、不落目录（提示已就绪）。
        assert!(list().unwrap().is_empty());
        std::env::remove_var(ARC_CLANG_ENV);
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn uninstall_requires_version_without_marker() {
        let _env_guard = env_lock();
        let home = temp_dir("home5");
        std::env::set_var(ARC_HOME_ENV, &home);
        let err = run_uninstall(TOOLCHAIN_NAME_LLVM, None).unwrap_err();
        assert!(err.contains("--version"), "{err}");
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }
}
