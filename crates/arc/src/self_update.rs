//! `arc self-update`：签名发布自更新（RFC 031 §13）。
//!
//! ## 安装布局（与 `scripts/packaging/install.ps1` / `arc-install.sh` 对齐）
//!
//! ```text
//! <R>/                              ← 安装根（Windows `%LOCALAPPDATA%\arc` / Unix `~/.arc`；`ARC_INSTALL_ROOT` 可覆盖）
//! ├── bin/arc(.exe)                 ← 稳定 PATH 指针（活动版本的副本；唯一 PATH 注入点）
//! └── versions/
//!     ├── current                   ← 活动版本标记（内容 = 版本号，如 `1.0.0`）
//!     ├── current.previous          ← 上一版本（`--rollback` 目标）
//!     └── arc-<ver>-<triple>/       ← 版本目录（多版本共存；回滚即切指针）
//!         └── bin/arc(.exe)
//! ```
//!
//! ## 原子性与并发/中断安全
//!
//! - 下载 → SHA256/签名校验 → 解压全部发生在 `versions/.staging-<pid>/`，
//!   任一步失败即删除 staging，**不触碰 bin 指针与标记**（无副作用原则）。
//! - 提交分三步，每步先写临时名再 rename（Windows 上 rename 可替换未运行文件；
//!   `fs_util::rename_with_retry` 对 Defender/AV 瞬时文件锁做有界重试）：
//!   1. 复制新 `bin/arc` 指针
//!   2. 更新 `versions/current` 标记
//!   3. 记录 `versions/current.previous`（回滚目标）
//! - 本进程若以指针身份运行（`R/bin/arc.exe`），先 **spawn** 活动版本的
//!   `versions/arc-<ver>-<triple>/bin/arc.exe` 并**立即退出**——Windows 下
//!   运行中 exe 不可重命名，父进程退出后子进程可安全替换指针。
//! - 签名校验失败 → 硬错误中止，禁止降级跳过。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::release::{
    self, fetch_and_verify_manifest, fetch_artifact, resolve_artifact_url, resolve_source,
    verify_artifact,
};
use crate::version::Version;

/// 版本目录前缀（`arc-<ver>-<triple>`）。
fn pkg_dir_name(version: &str, triple: &str) -> String {
    format!("arc-{version}-{triple}")
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "arc.exe"
    } else {
        "arc"
    }
}

/// 安装根环境变量（`arc env` 展示与测试覆盖）。
pub const ARC_INSTALL_ROOT_ENV: &str = "ARC_INSTALL_ROOT";

/// 安装根（`versions/` 所在目录）：
/// `ARC_INSTALL_ROOT` 覆盖 > Windows `%LOCALAPPDATA%\arc` > `~/.arc`。
pub fn install_root() -> PathBuf {
    if let Some(r) = crate::env::env_var(ARC_INSTALL_ROOT_ENV) {
        return PathBuf::from(r);
    }
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("arc");
        }
    }
    let user_home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    user_home.join(".arc")
}

/// 安装态快照（由当前 exe 路径 + 可选 `--root` 覆盖解析）。
#[derive(Debug, Clone)]
pub struct InstallState {
    /// 安装根（含 `bin/` 与 `versions/`）。
    pub install_root: PathBuf,
    pub versions_dir: PathBuf,
    pub bin_dir: PathBuf,
    /// 活动版本（标记或 exe 路径推断）。
    pub current_version: Option<Version>,
    /// 活动版本的 exe 路径（`versions/arc-<ver>-<triple>/bin/arc`）。
    pub active_exe: Option<PathBuf>,
    /// 当前运行进程是否为 bin 指针（需要 re-exec）。
    pub is_pointer: bool,
}

/// 从 exe 路径定位安装根。
///
/// - exe 位于 `<R>/versions/<pkg>/bin/arc` → `R`。
/// - 否则向上找含 `versions/` + `bin/arc` 的目录。
pub(crate) fn locate_install_root(exe: &Path) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = exe.ancestors().map(|p| p.to_path_buf()).collect();
    for i in 0..dirs.len() {
        if dirs[i].file_name().is_some_and(|n| n == "versions") {
            if let Some(root) = dirs.get(i + 1) {
                return Some(root.clone());
            }
        }
    }
    for dir in dirs.iter().skip(1) {
        if dir.join("versions").is_dir() && dir.join("bin").join(exe_name()).is_file() {
            return Some(dir.clone());
        }
    }
    None
}

/// 从 exe 路径推断版本目录名（`<R>/versions/arc-<ver>-<triple>/bin/arc`）。
fn version_from_exe_path(exe: &Path) -> Option<String> {
    let dirs: Vec<PathBuf> = exe.ancestors().map(|p| p.to_path_buf()).collect();
    for i in 0..dirs.len() {
        if dirs[i].file_name().is_some_and(|n| n == "versions") {
            let pkg = dirs.get(i + 1)?.file_name()?.to_string_lossy().into_owned();
            return strip_pkg_dir_name(&pkg);
        }
    }
    None
}

fn strip_pkg_dir_name(pkg: &str) -> Option<String> {
    let rest = pkg.strip_prefix("arc-")?;
    let triple = crate::target::TargetTriple::host().as_str().to_string();
    rest.strip_suffix(&format!("-{triple}"))
        .map(|v| v.to_string())
        .or_else(|| Some(rest.to_string()))
}

fn read_marker(versions_dir: &Path, name: &str) -> Option<String> {
    let path = versions_dir.join(name);
    let bytes = std::fs::read(&path).ok()?;
    // BOM 容忍（与 parse 入口同一姿态）：Windows 侧工具（install.ps1 / PS 5.1
    // Set-Content）可能以带 BOM 的 UTF-8 写标记文件，解析前剥离。
    let bytes = bytes
        .strip_prefix(&[0xEF, 0xBB, 0xBF])
        .unwrap_or(&bytes);
    let t = String::from_utf8_lossy(bytes);
    let t = t.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 解析安装态。
pub fn resolve_state(exe: &Path, root_override: Option<&Path>) -> Result<InstallState, String> {
    let install_root = match root_override {
        Some(r) => r.to_path_buf(),
        None => locate_install_root(exe).ok_or_else(|| {
            format!(
                "{} is not part of an arc installation (expected <root>/versions/<pkg>/bin or \
                 --root <dir>)",
                exe.display()
            )
        })?,
    };
    let versions_dir = install_root.join("versions");
    let bin_dir = install_root.join("bin");
    let is_pointer = exe == bin_dir.join(exe_name());

    let triple = crate::target::TargetTriple::host().as_str().to_string();
    let marker = read_marker(&versions_dir, "current");
    let from_exe = version_from_exe_path(exe);
    let current_version = marker
        .as_deref()
        .or(from_exe.as_deref())
        .map(|v| {
            v.parse::<Version>()
                .map_err(|e| format!("invalid version marker `{v}`: {e}"))
        })
        .transpose()?;
    let active_exe = current_version
        .as_ref()
        .map(|v| {
            versions_dir
                .join(pkg_dir_name(&v.to_string(), &triple))
                .join("bin")
                .join(exe_name())
        })
        .filter(|p| p.is_file());

    Ok(InstallState {
        install_root,
        versions_dir,
        bin_dir,
        current_version,
        active_exe,
        is_pointer,
    })
}

/// `arc self-update` 参数（由 CLI 构建）。
#[derive(Debug, Clone, Default)]
pub struct SelfUpdateOptions {
    /// 精确目标版本。
    pub version: Option<String>,
    /// 发布源（`--source` 覆盖 `$ARC_RELEASE_BASE`）。
    pub source: Option<String>,
    /// 安装根覆盖（CI / 测试；缺省自动定位）。
    pub root: Option<PathBuf>,
    /// 仅检查更新。
    pub check: bool,
    /// 回滚到上一版本。
    pub rollback: bool,
    /// 允许重复安装 / 显式目标等于当前版本。
    pub force: bool,
}

/// 顶层入口：指针 re-exec → 回滚 / 检查 / 更新。
pub fn run(opts: &SelfUpdateOptions) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let state = resolve_state(&exe, opts.root.as_deref())?;

    // 指针身份：spawn 活动版本后**立即退出**，释放 `bin/arc.exe` 文件锁
    //（Windows 下运行中 exe 不可重命名；父进程退出后子进程可安全替换指针）。
    // `--check` 不写盘，无需 re-exec。
    if state.is_pointer && !opts.check {
        let versioned = state.active_exe.clone().ok_or_else(|| {
            "running from the bin pointer but the active versioned arc executable is missing; \
             reinstall or pass --root"
                .to_string()
        })?;
        eprintln!("re-executing from active version: {}", versioned.display());
        Command::new(&versioned)
            .args(std::env::args().skip(1))
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {e}", versioned.display()))?;
        std::process::exit(0);
    }

    if opts.rollback {
        return rollback(&state);
    }
    if opts.check {
        return check_updates(opts);
    }
    update(opts)
}

fn resolve_triple() -> String {
    crate::target::TargetTriple::host().as_str().to_string()
}

/// 仅检查：下载+验签 manifest，报告可用更新，不落盘。
fn check_updates(opts: &SelfUpdateOptions) -> Result<(), String> {
    let state = resolve_state(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        opts.root.as_deref(),
    )?;
    let current = state.current_version.ok_or_else(|| {
        "cannot determine current version: no `versions/current` marker and exe is not under \
         versions/"
            .to_string()
    })?;
    let source = resolve_source(opts.source.as_deref()).map_err(|e| e.to_string())?;
    let manifest = fetch_and_verify_manifest(&source).map_err(|e| e.to_string())?;
    match release::select_target(
        &manifest,
        &resolve_triple(),
        &current,
        opts.version.as_deref(),
    )
    .map_err(|e| e.to_string())?
    {
        Some((ver, _entry)) => {
            println!(
                "update available: arc {current} → {ver} (channel {})",
                manifest.channel
            );
        }
        None => {
            println!(
                "arc is up to date ({current}, channel {})",
                manifest.channel
            );
        }
    }
    Ok(())
}

/// 完整更新：下载 → 校验 → 解压 → 布局自检 → 原子提交。
fn update(opts: &SelfUpdateOptions) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let state = resolve_state(&exe, opts.root.as_deref())?;

    let current = state.current_version.ok_or_else(|| {
        "cannot determine current version: no `versions/current` marker and exe is not under \
         versions/"
            .to_string()
    })?;
    let source = resolve_source(opts.source.as_deref()).map_err(|e| e.to_string())?;
    let manifest = fetch_and_verify_manifest(&source).map_err(|e| e.to_string())?;
    let triple = resolve_triple();

    let (target_ver, entry) =
        release::select_target(&manifest, &triple, &current, opts.version.as_deref())
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "arc is already up to date ({current}); use --version to pin an older/newer \
                     release or --force to reinstall"
                )
            })?;

    if target_ver == current.to_string() && !opts.force {
        return Err(format!(
            "arc is already on {target_ver} (--force to reinstall)"
        ));
    }

    let artifact = entry.artifacts.get(&triple).ok_or_else(|| {
        format!("manifest has no artifact for host triple `{triple}` in version `{target_ver}`")
    })?;
    let url = resolve_artifact_url(&source, &artifact.url).map_err(|e| e.to_string())?;
    println!("downloading arc {current} → {target_ver}: {url}");
    let bytes = fetch_artifact(&source, &artifact.url).map_err(|e| e.to_string())?;
    verify_artifact(&pkg_dir_name(&target_ver, &triple), artifact, &bytes)
        .map_err(|e| e.to_string())?;
    println!("verified: sha256 ok ({} bytes)", bytes.len());

    let final_pkg_dir = state.versions_dir.join(pkg_dir_name(&target_ver, &triple));
    let staging = state
        .versions_dir
        .join(format!(".staging-{}", std::process::id()));
    let mut guard = StagingGuard::new(staging.clone());
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    crate::archive::extract_zip(&bytes, &staging)?;

    let staged_pkg = staging.join(pkg_dir_name(&target_ver, &triple));
    let staged_exe = staged_pkg.join("bin").join(exe_name());
    if !staged_exe.is_file() {
        return Err(format!(
            "package `{target_ver}` has no `bin/{}` (broken artifact)",
            exe_name()
        ));
    }
    // 布局自检：staged arc 必须能运行 `--version`。
    run_version_probe(&staged_exe, &target_ver)?;

    // 目录级原子提交：rename staged → final（同一文件系统；AV 瞬时锁带重试）。
    if final_pkg_dir.exists() {
        std::fs::remove_dir_all(&final_pkg_dir).map_err(|e| e.to_string())?;
    }
    crate::fs_util::rename_with_retry(&staged_pkg, &final_pkg_dir).map_err(|e| {
        format!(
            "rename {} → {} failed: {e}",
            staged_pkg.display(),
            final_pkg_dir.display()
        )
    })?;
    guard.committed = true;

    // 指针 + 标记提交（新 exe 此时不在运行，Windows 可安全替换）。
    commit(
        &state,
        &final_pkg_dir.join("bin").join(exe_name()),
        &target_ver,
        &current.to_string(),
    )?;

    let _ = std::fs::remove_dir_all(&staging);
    println!(
        "updated: arc {} → {} (rollback: `arc self-update --rollback`)",
        current, target_ver
    );
    Ok(())
}

/// 运行 staged `arc --version` 自检。
fn run_version_probe(exe: &Path, version: &str) -> Result<(), String> {
    let out = Command::new(exe)
        .arg("--version")
        .output()
        .map_err(|e| format!("failed to run staged `{} --version`: {e}", exe.display()))?;
    if !out.status.success() {
        return Err(format!(
            "staged package `{version}` failed its `--version` probe (exit {:?})",
            out.status.code()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if !text.starts_with("arc ") {
        return Err(format!(
            "staged package `{version}` is not an arc executable (unexpected --version output: \
             {text:?})"
        ));
    }
    Ok(())
}

/// 原子提交：bin 指针 → current 标记 → previous 标记。
fn commit(
    state: &InstallState,
    new_exe: &Path,
    new_version: &str,
    old_version: &str,
) -> Result<(), String> {
    let exe_name = exe_name();
    let bin_dir = &state.bin_dir;
    std::fs::create_dir_all(bin_dir).map_err(|e| e.to_string())?;

    // 1. 指针（临时名 → rename 替换；目标不在运行）。
    let tmp = bin_dir.join(format!("{exe_name}.new"));
    std::fs::copy(new_exe, &tmp)
        .map_err(|e| format!("copy {} → {} failed: {e}", new_exe.display(), tmp.display()))?;
    crate::fs_util::rename_with_retry(&tmp, bin_dir.join(exe_name)).map_err(|e| {
        format!(
            "swap bin pointer {} failed: {e}",
            bin_dir.join(exe_name).display()
        )
    })?;

    // 2. current 标记。
    atomic_write_marker(&state.versions_dir, "current", new_version)?;
    // 3. previous 标记（回滚目标）。
    atomic_write_marker(&state.versions_dir, "current.previous", old_version)?;
    Ok(())
}

fn atomic_write_marker(versions_dir: &Path, name: &str, value: &str) -> Result<(), String> {
    std::fs::create_dir_all(versions_dir).map_err(|e| e.to_string())?;
    let tmp = versions_dir.join(format!("{name}.new"));
    std::fs::write(&tmp, format!("{value}\n")).map_err(|e| e.to_string())?;
    crate::fs_util::rename_with_retry(&tmp, versions_dir.join(name))
        .map_err(|e| format!("update marker `{name}` failed: {e}"))
}

/// 回滚到 `versions/current.previous` 记录的版本。
fn rollback(state: &InstallState) -> Result<(), String> {
    let current = read_marker(&state.versions_dir, "current")
        .ok_or_else(|| "no `versions/current` marker; nothing to roll back".to_string())?;
    let previous = read_marker(&state.versions_dir, "current.previous").ok_or_else(|| {
        "no `versions/current.previous` marker; rollback history is empty".to_string()
    })?;
    if current == previous {
        return Err(format!(
            "current and previous are both `{current}`; nothing to roll back"
        ));
    }
    let triple = resolve_triple();
    let prev_exe = state
        .versions_dir
        .join(pkg_dir_name(&previous, &triple))
        .join("bin")
        .join(exe_name());
    if !prev_exe.is_file() {
        return Err(format!(
            "rollback target `{previous}` is missing its executable: {}",
            prev_exe.display()
        ));
    }
    // 指针回切（目标不在运行）。
    std::fs::create_dir_all(&state.bin_dir).map_err(|e| e.to_string())?;
    let tmp = state.bin_dir.join(format!("{}.new", exe_name()));
    std::fs::copy(&prev_exe, &tmp).map_err(|e| e.to_string())?;
    crate::fs_util::rename_with_retry(&tmp, state.bin_dir.join(exe_name()))
        .map_err(|e| format!("swap bin pointer failed: {e}"))?;
    // 标记互换。
    atomic_write_marker(&state.versions_dir, "current.previous", &current)?;
    atomic_write_marker(&state.versions_dir, "current", &previous)?;
    println!("rolled back: arc {current} → {previous}");
    Ok(())
}

/// staging 目录守卫：未提交时删除（无副作用原则）。
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

    fn temp_dir(label: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("arc-self-update-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 构建一个"假 arc.exe"：用当前测试二进制复制（其 `--version` 探测按二进制成功）。
    fn fake_arc_exe(dir: &Path, name: &str) -> PathBuf {
        let dst = dir.join(name);
        // 复制本测试二进制作为可执行文件；单测不跑 --version 探测。
        let src = std::env::current_exe().unwrap();
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::copy(&src, &dst).unwrap();
        dst
    }

    #[test]
    fn locate_install_root_from_versioned_exe() {
        let root = temp_dir("locate");
        let exe = root
            .join("versions")
            .join("arc-0.1.0-x86_64-pc-windows-msvc")
            .join("bin")
            .join(exe_name());
        assert_eq!(locate_install_root(&exe), Some(root.clone()));
        // 非安装路径
        assert_eq!(
            locate_install_root(&root.join("elsewhere").join("arc.exe")),
            None
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn strip_pkg_dir_name_parses_version() {
        let triple = crate::target::TargetTriple::host().as_str().to_string();
        assert_eq!(
            strip_pkg_dir_name(&pkg_dir_name("0.1.0", &triple)).as_deref(),
            Some("0.1.0")
        );
        assert_eq!(strip_pkg_dir_name("arc-0.2.0").as_deref(), Some("0.2.0"));
        assert_eq!(strip_pkg_dir_name("not-a-pkg"), None);
    }

    #[test]
    fn markers_roundtrip() {
        let dir = temp_dir("markers");
        std::fs::create_dir_all(&dir).unwrap();
        atomic_write_marker(&dir, "current", "0.2.0").unwrap();
        assert_eq!(read_marker(&dir, "current").as_deref(), Some("0.2.0"));
        assert_eq!(read_marker(&dir, "missing"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn marker_bom_tolerated() {
        let dir = temp_dir("marker-bom");
        std::fs::create_dir_all(&dir).unwrap();
        // Windows 侧工具可能以带 BOM 的 UTF-8 写标记（PS 5.1 Set-Content 实测）。
        std::fs::write(dir.join("current"), [0xEF, 0xBB, 0xBF].iter().copied().chain(b"0.9.0".iter().copied()).collect::<Vec<u8>>()).unwrap();
        assert_eq!(read_marker(&dir, "current").as_deref(), Some("0.9.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn commit_writes_pointer_and_markers() {
        let root = temp_dir("commit");
        let versions = root.join("versions");
        std::fs::create_dir_all(&versions).unwrap();
        let old = versions.join("arc-0.1.0-x86_64-pc-windows-msvc");
        let new = versions.join("arc-0.2.0-x86_64-pc-windows-msvc");
        std::fs::create_dir_all(old.join("bin")).unwrap();
        std::fs::create_dir_all(new.join("bin")).unwrap();
        let old_exe = fake_arc_exe(&old.join("bin"), exe_name());
        let new_exe = fake_arc_exe(&new.join("bin"), exe_name());
        std::fs::write(&old_exe, b"old").unwrap();
        std::fs::write(&new_exe, b"new").unwrap();

        let state = InstallState {
            install_root: root.clone(),
            versions_dir: versions.clone(),
            bin_dir: root.join("bin"),
            current_version: Some("0.1.0".parse().unwrap()),
            active_exe: Some(old_exe.clone()),
            is_pointer: false,
        };
        commit(&state, &new_exe, "0.2.0", "0.1.0").unwrap();
        assert_eq!(
            std::fs::read(root.join("bin").join(exe_name())).unwrap(),
            b"new"
        );
        assert_eq!(read_marker(&versions, "current").as_deref(), Some("0.2.0"));
        assert_eq!(
            read_marker(&versions, "current.previous").as_deref(),
            Some("0.1.0")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rollback_switches_pointer_and_markers() {
        let root = temp_dir("rollback");
        let versions = root.join("versions");
        std::fs::create_dir_all(&versions).unwrap();
        let old = versions.join("arc-0.1.0-x86_64-pc-windows-msvc");
        let new = versions.join("arc-0.2.0-x86_64-pc-windows-msvc");
        std::fs::create_dir_all(old.join("bin")).unwrap();
        std::fs::create_dir_all(new.join("bin")).unwrap();
        let old_exe = fake_arc_exe(&old.join("bin"), exe_name());
        let new_exe = fake_arc_exe(&new.join("bin"), exe_name());
        std::fs::write(&old_exe, b"old").unwrap();
        std::fs::write(&new_exe, b"new").unwrap();
        let bin_dir = root.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join(exe_name()), b"new").unwrap();
        atomic_write_marker(&versions, "current", "0.2.0").unwrap();
        atomic_write_marker(&versions, "current.previous", "0.1.0").unwrap();

        let state = InstallState {
            install_root: root.clone(),
            versions_dir: versions.clone(),
            bin_dir,
            current_version: Some("0.2.0".parse().unwrap()),
            active_exe: Some(new_exe),
            is_pointer: false,
        };
        rollback(&state).unwrap();
        assert_eq!(
            std::fs::read(root.join("bin").join(exe_name())).unwrap(),
            b"old"
        );
        assert_eq!(read_marker(&versions, "current").as_deref(), Some("0.1.0"));
        assert_eq!(
            read_marker(&versions, "current.previous").as_deref(),
            Some("0.2.0")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_root_respects_override_and_defaults() {
        let _env_guard = crate::ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::set_var(ARC_INSTALL_ROOT_ENV, "/tmp/arc-root-override");
        assert_eq!(install_root(), PathBuf::from("/tmp/arc-root-override"));
        std::env::remove_var(ARC_INSTALL_ROOT_ENV);
    }
}
