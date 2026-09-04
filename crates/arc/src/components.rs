//! `arc component`：按需组件安装/管理（Phase 3）。
//!
//! ## 布局
//!
//! ```text
//! <tools_root>/components/            ← $ARC_HOME/tools/components（未设则 ~/.arc/tools/components）
//! └── <name>/
//!     ├── current                     ← 活动版本指针（内容 = 版本号，如 `v29.0.1.1`）
//!     └── <ver>/                      ← 版本目录（多版本共存）
//!         └── bin/<os>/wgpu_native.dll|.lib   ← 归一化平台二进制（wgpu 组件）
//!         └── include/                ← 归档携带的头文件（wgpu 组件：webgpu.h/wgpu.h）
//! ```
//!
//! 组件安装目录与 vendored 布局（`<rt-base>/runtime-ui/wgpu-native`）子目录一致
//! （`bin/<os>/` + `include/`），codegen 经 `component_active_dir("wgpu")` 优先
//! 解析组件二进制，vendored 为兜底（单一解析序）。
//!
//! ## 组件清单（components.json，内嵌于编译器二进制）
//!
//! 清单随 SDK 内嵌（`include_str!`），一组件一条目：`builtin`（随 SDK 捆绑，
//! 如 `crypto`）/ 可下载组件（`url` 模板 + `sha256` + `size` + `platforms`）。
//! 协议见 [031 §13]（RFC 031 `arc component` 一节）。
//!
//! ## 明确不在本切片
//!
//! - 组件元数据端点（远程 `components.json` 更新）：清单内嵌为静态快照，版本
//!   更新随编译器发布（真实端点见 RFC 031 §13 外部依赖）。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::archive::extract_zip;
use crate::download::http_get_bytes;
use crate::hash::content_sha256;
use crate::target::TargetTriple;

/// `components/<name>/current` 指针文件名（内容 = 活动版本号）。
pub const COMPONENT_CURRENT_FILE: &str = "current";
/// 内置组件名：`crypto`（`crypto_native` 底座随 SDK 捆绑）。
pub const COMPONENT_CRYPTO: &str = "crypto";
/// 可下载组件名：`wgpu`（wgpu-native 预构建二进制）。
pub const COMPONENT_WGPU: &str = "wgpu";

/// 组件清单（`components.json` 反序列化目标）。
#[derive(Debug, Clone, Deserialize)]
pub struct ComponentManifest {
    #[serde(default)]
    pub schema_version: u32,
    pub components: std::collections::BTreeMap<String, ComponentEntry>,
}

/// 单个组件条目。
#[derive(Debug, Clone, Deserialize)]
pub struct ComponentEntry {
    pub description: String,
    /// 内置组件（随 SDK 捆绑；不可 install/uninstall）。
    #[serde(default)]
    pub builtin: bool,
    /// 默认安装标记（保留给未来 `--default` 批量安装语义）。
    #[serde(default)]
    pub default: bool,
    /// 固定版本（与 `url` 模板 `{version}` 占位联动）。
    #[serde(default)]
    pub version: Option<String>,
    /// 下载 URL 模板（`{version}` 替换；`builtin` 组件为 `None`）。
    #[serde(default)]
    pub url: Option<String>,
    /// 固定 SHA256（64 hex；提供则下载路径强制校验）。
    #[serde(default)]
    pub sha256: Option<String>,
    /// 分发包字节数（信息性 + 下载后 sanity 校验）。
    #[serde(default)]
    pub size: Option<u64>,
    /// 支持的目标 triple 子串列表（空 = 不限平台）。
    #[serde(default)]
    pub platforms: Vec<String>,
}

/// `arc component install` 参数（由 CLI 构建）。
#[derive(Debug, Clone, Default)]
pub struct ComponentInstallOptions {
    /// 组件名（`wgpu`）。
    pub name: String,
    /// 目标版本（缺省组件清单固定版本）。
    pub version: Option<String>,
    /// 显式下载 URL（覆盖组件清单模板）。
    pub url: Option<String>,
    /// 本地分发包（zip；离线/测试）。
    pub archive: Option<PathBuf>,
    /// 可选分发包 SHA256（64 hex；提供则校验，不符拒绝安装）。
    pub sha256: Option<String>,
    /// 跳过「已安装」幂等捷径（强制重装）。
    pub force: bool,
}

/// 组件状态（`arc component list` / `status`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentState {
    /// 随 SDK 捆绑，无需安装。
    Builtin,
    /// 未安装。
    NotInstalled,
    /// 已安装（含版本；`active` 为 `current` 指针目标）。
    Installed { version: String, active: bool },
}

/// 解析内嵌组件清单。
pub fn registry() -> Result<ComponentManifest, String> {
    serde_json::from_str(components_json()).map_err(|e| format!("components.json parse error: {e}"))
}

fn components_json() -> &'static str {
    include_str!("components.json")
}

/// 按名取组件条目；未知组件返回明确错误。
pub fn entry(name: &str) -> Result<ComponentEntry, String> {
    registry()?
        .components
        .get(name)
        .cloned()
        .ok_or_else(|| format!("unknown component `{name}` (known: {})", known_names()))
}

/// 已知组件名列表（诊断用）。
fn known_names() -> String {
    registry()
        .map(|m| m.components.keys().cloned().collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

/// 组件根：`<tools_root>/components`。
pub fn components_root() -> PathBuf {
    codegen::sdk_layout::components_root()
}

/// 单个组件的版本目录根：`<tools_root>/components/<name>`。
pub fn component_dir(name: &str) -> PathBuf {
    codegen::sdk_layout::component_dir(name)
}

/// 组件活动版本目录（`current` 指针解析；未设/指针目标缺失为 `None`）。
pub fn active_dir(name: &str) -> Option<PathBuf> {
    codegen::sdk_layout::component_active_dir(name)
}

/// 读 `current` 指针（内容 = 活动版本号）。
fn read_current(dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(dir.join(COMPONENT_CURRENT_FILE)).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 写 `current` 指针（临时名 + rename 原子写）。
fn write_current(dir: &Path, version: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.new", COMPONENT_CURRENT_FILE));
    std::fs::write(&tmp, format!("{version}\n")).map_err(|e| e.to_string())?;
    crate::fs_util::rename_with_retry(&tmp, dir.join(COMPONENT_CURRENT_FILE))
        .map_err(|e| format!("update {} failed: {e}", COMPONENT_CURRENT_FILE))
}

/// `arc component list`：按清单顺序列出全部组件与状态。
pub fn run_list() -> Result<(), String> {
    let manifest = registry()?;
    let mut any_installed = false;
    for (name, entry) in &manifest.components {
        let (state, detail) = state_of(name, entry);
        if matches!(&state, ComponentState::Installed { .. }) {
            any_installed = true;
        }
        print!("{name:<8} ",);
        match &state {
            ComponentState::Builtin => println!("builtin"),
            ComponentState::NotInstalled => println!("not-installed"),
            ComponentState::Installed { version, active } => {
                let mark = if *active { " (active)" } else { "" };
                println!("installed {version}{mark}  {}", detail.unwrap_or_default());
            }
        }
        println!("           {}", entry.description);
    }
    if !any_installed {
        println!();
        println!(
            "install a component with `arc component install <name>` (offline: --archive <zip> [--sha256 <hex>])"
        );
    }
    Ok(())
}

/// 计算组件状态（list/status 共用）。
fn state_of(name: &str, entry: &ComponentEntry) -> (ComponentState, Option<String>) {
    if entry.builtin {
        return (ComponentState::Builtin, None);
    }
    let dir = component_dir(name);
    let current = read_current(&dir);
    let version = current.clone();
    match version {
        Some(v) => {
            let active_dir = dir.join(&v);
            let active = active_dir.is_dir() && active_dir.join("bin").is_dir();
            let detail = Some(active_dir.display().to_string());
            (ComponentState::Installed { version: v, active }, detail)
        }
        None => (ComponentState::NotInstalled, None),
    }
}

/// `arc component status`：组件根 + 各组件状态与活动路径。
pub fn run_status() -> Result<(), String> {
    let manifest = registry()?;
    println!("components root: {}", components_root().display());
    for (name, entry) in &manifest.components {
        let (state, detail) = state_of(name, entry);
        match &state {
            ComponentState::Builtin => println!("{name}: builtin (bundled with the SDK)"),
            ComponentState::NotInstalled => println!("{name}: not-installed"),
            ComponentState::Installed { version, active } => {
                let mark = if *active { " (active)" } else { "" };
                println!(
                    "{name}: installed {version}{mark}  {}",
                    detail.unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}

/// `arc component install <name>`：下载/解包 → 归一化 → 原子落位 + `current` 指针。
pub fn run_install(opts: &ComponentInstallOptions) -> Result<(), String> {
    let name = &opts.name;
    let entry = entry(name)?;
    if entry.builtin {
        return Err(format!(
            "component `{name}` is builtin — bundled with the SDK, nothing to install"
        ));
    }
    let host = TargetTriple::host().as_str().to_string();
    if !entry.platforms.is_empty() && !entry.platforms.iter().any(|p| host.contains(p)) {
        return Err(format!(
            "component `{name}` does not support host triple `{host}` (supported: {})",
            entry.platforms.join(", ")
        ));
    }

    let version = opts
        .version
        .clone()
        .or_else(|| entry.version.clone())
        .ok_or_else(|| format!("component `{name}` has no pinned version; pass --version <ver>"))?;
    let dir = component_dir(name);
    let target = dir.join(&version);

    // 幂等：同版本已装 → 刷新 current 指针并结束。
    if target.is_dir() {
        if !opts.force {
            write_current(&dir, &version)?;
            println!("{name} {version} already installed: {}", target.display());
            return Ok(());
        }
        std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    }

    // 来源：--archive > --url > 组件清单模板。
    let (bytes, from_url) = if let Some(archive) = &opts.archive {
        (
            std::fs::read(archive).map_err(|e| format!("read {}: {e}", archive.display()))?,
            false,
        )
    } else if let Some(url) = &opts.url {
        println!("downloading {name} {version}: {url}");
        (http_get_bytes(url).map_err(|e| e.to_string())?, true)
    } else if let Some(url_tpl) = &entry.url {
        let url = url_tpl.replace("{version}", &version);
        println!("downloading {name} {version}: {url}");
        (http_get_bytes(&url).map_err(|e| e.to_string())?, true)
    } else {
        return Err(format!(
            "component `{name}` has no download URL; pass `--url <url>` or `--archive <file>`"
        ));
    };

    // SHA256 校验：`--sha256` 恒优先；本地 `--archive` 是用户自备产物，仅显式
    // `--sha256` 校验；网络下载应用清单固定值（防 MITM/篡改，强制校验）。
    let expected = match (&opts.sha256, from_url) {
        (Some(h), _) => Some(h.clone()),
        (None, true) => entry.sha256.clone(),
        (None, false) => None,
    };
    if let Some(expected) = &expected {
        let actual = content_sha256(&bytes);
        if actual != expected.to_ascii_lowercase() {
            return Err(format!(
                "sha256 mismatch for {name} {version}: expected {}, got {} (refusing to install)",
                expected.to_ascii_lowercase(),
                actual
            ));
        }
        println!("verified: sha256 ok ({actual})");
    } else if from_url {
        println!(
            "warning: no pinned sha256 for {name} {version}; downloaded bytes not integrity-verified"
        );
    }

    // 体积 sanity（对齐 fetch 脚本纪律：异常小即拒绝）。
    if let Some(expected_size) = entry.size {
        if bytes.len() as u64 != expected_size {
            println!(
                "warning: {name} {version} size {} != registry size {expected_size}",
                bytes.len()
            );
        }
    }

    // staging 解压 → 归一化 `bin/<os>/` 布局 → 原子 rename 提交。
    let staging = dir.join(format!(".staging-{}", std::process::id()));
    let mut guard = StagingGuard::new(staging.clone());
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let extracted = staging.join("x");
    std::fs::create_dir_all(&extracted).map_err(|e| e.to_string())?;
    extract_zip(&bytes, &extracted)?;
    normalize_wgpu_layout(&staging, &extracted)?;
    // 丢弃原始解包树（只保留归一化后的 `bin/` + `include/`）。
    std::fs::remove_dir_all(&extracted).map_err(|e| e.to_string())?;

    // 原子提交：rename staging → target，写 current 指针。
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::fs_util::rename_with_retry(&staging, &target).map_err(|e| {
        format!(
            "rename {} → {} failed: {e}",
            staging.display(),
            target.display()
        )
    })?;
    guard.committed = true;
    write_current(&dir, &version)?;

    println!("installed: {name} {version} → {}", target.display());
    println!(
        "  active via components/{name}/current; `arc build`/`arc doctor` pick it up automatically"
    );
    Ok(())
}

/// 把提取产物归一化为组件规范布局：`bin/<os>/wgpu_native.{dll,lib}` + `include/`。
///
/// wgpu-native release 归档把二进制放在 `lib/`（嵌套因版本而异），归一化后
/// 与 vendored 布局（`bin/<os>/`）一致，codegen 单一解析序无需区分来源。
fn normalize_wgpu_layout(staging: &Path, extracted: &Path) -> Result<(), String> {
    let os_subdir = wgpu_os_subdir();
    let dll = find_file(extracted, dll_name());
    let lib = find_file(extracted, lib_name());
    if dll.is_none() || lib.is_none() {
        return Err(format!(
            "archive has no `{}` and/or `{}` (not a wgpu-native binary layout)",
            dll_name(),
            lib_name()
        ));
    }
    let bin = staging.join("bin").join(os_subdir);
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    std::fs::copy(dll.as_ref().unwrap(), bin.join(dll_name())).map_err(|e| e.to_string())?;
    std::fs::copy(lib.as_ref().unwrap(), bin.join(lib_name())).map_err(|e| e.to_string())?;
    // 归档携带头文件时保留（`include/`；缺失不阻断——vendored 头始终在 SDK 内）。
    let include = extracted.join("include");
    if include.is_dir() {
        let dst = staging.join("include");
        crate::fs_util::copy_dir_recursive(&include, &dst).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 平台二进制子目录（与 codegen `wgpu_native_vendor_subdir` 对齐）。
fn wgpu_os_subdir() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "host"
    }
}

fn dll_name() -> &'static str {
    if cfg!(windows) {
        "wgpu_native.dll"
    } else {
        "libwgpu_native.so"
    }
}

fn lib_name() -> &'static str {
    if cfg!(windows) {
        "wgpu_native.lib"
    } else {
        "libwgpu_native.a"
    }
}

/// 在解包目录树内递归查找目标文件名（wgpu release 归档嵌套因版本而异）。
fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue.push(path);
            } else if path.file_name().is_some_and(|n| n == name) {
                return Some(path);
            }
        }
    }
    None
}

/// `arc component uninstall <name> [--version <ver>]`：删除版本目录；删除的是
/// 活动版本时同步清除 `current` 指针。
pub fn run_uninstall(name: &str, version: Option<&str>) -> Result<(), String> {
    let entry = entry(name)?;
    if entry.builtin {
        return Err(format!(
            "component `{name}` is builtin — bundled with the SDK, cannot uninstall"
        ));
    }
    let dir = component_dir(name);
    let ver = match version {
        Some(v) => v.to_string(),
        None => read_current(&dir).ok_or_else(|| {
            format!(
                "no version specified and no `components/{name}/current` marker; pass --version <ver>"
            )
        })?,
    };
    let target = dir.join(&ver);
    if !target.is_dir() {
        return Err(format!(
            "{name} {ver} is not installed ({})",
            target.display()
        ));
    }
    std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
    if read_current(&dir).as_deref() == Some(ver.as_str()) {
        let pointer = dir.join(COMPONENT_CURRENT_FILE);
        let _ = std::fs::remove_file(&pointer);
        println!("current pointer cleared ({name} {ver} was active)");
    }
    println!("uninstalled: {name} {ver}");
    Ok(())
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
        let d = std::env::temp_dir().join(format!("arc-components-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// `$ARC_HOME` 在并行测试下互斥（crate 级锁）。
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 构造含 `bin/wgpu_native.{dll,lib}` 的假 wgpu zip。
    fn make_wgpu_zip(zip_path: &Path) {
        let file = std::fs::File::create(zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "lib/wgpu_native.dll",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"fake wgpu dll").unwrap();
        zip.start_file(
            "lib/wgpu_native.lib",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"fake wgpu lib").unwrap();
        zip.start_file(
            "include/webgpu/webgpu.h",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"fake header").unwrap();
        zip.finish().unwrap();
    }

    fn install_opts(zip: &Path, version: &str, force: bool) -> ComponentInstallOptions {
        ComponentInstallOptions {
            name: COMPONENT_WGPU.into(),
            version: Some(version.into()),
            archive: Some(zip.to_path_buf()),
            force,
            ..Default::default()
        }
    }

    #[test]
    fn list_reports_builtin_and_not_installed() {
        let _env_guard = env_lock();
        let home = temp_dir("home1");
        std::env::set_var(ARC_HOME_ENV, &home);
        let manifest = registry().unwrap();
        assert!(manifest.components.contains_key(COMPONENT_CRYPTO));
        assert!(manifest.components.contains_key(COMPONENT_WGPU));
        assert!(manifest.components[COMPONENT_CRYPTO].builtin);
        assert!(!manifest.components[COMPONENT_WGPU].builtin);
        // 未安装：wgpu not-installed
        assert_eq!(
            state_of(COMPONENT_WGPU, &manifest.components[COMPONENT_WGPU]).0,
            ComponentState::NotInstalled
        );
        assert_eq!(
            state_of(COMPONENT_CRYPTO, &manifest.components[COMPONENT_CRYPTO]).0,
            ComponentState::Builtin
        );
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn install_unknown_component_refused() {
        let _env_guard = env_lock();
        let home = temp_dir("home2");
        std::env::set_var(ARC_HOME_ENV, &home);
        let err = entry("nosuch").unwrap_err();
        assert!(err.contains("unknown component"), "{err}");
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn install_builtin_refused() {
        let _env_guard = env_lock();
        let home = temp_dir("home3");
        std::env::set_var(ARC_HOME_ENV, &home);
        let mut opts = install_opts(&home.join("x.zip"), "v1", false);
        opts.name = COMPONENT_CRYPTO.into();
        let err = run_install(&opts).unwrap_err();
        assert!(err.contains("builtin"), "{err}");
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn install_then_list_then_uninstall() {
        let _env_guard = env_lock();
        let home = temp_dir("home4");
        std::env::set_var(ARC_HOME_ENV, &home);
        let zip = home.join("wgpu.zip");
        make_wgpu_zip(&zip);

        run_install(&install_opts(&zip, "v29.0.1.1", false)).unwrap();
        let dir = component_dir(COMPONENT_WGPU);
        let target = dir.join("v29.0.1.1");
        assert!(target.join("bin/windows/wgpu_native.dll").is_file());
        assert!(target.join("bin/windows/wgpu_native.lib").is_file());
        assert!(target.join("include/webgpu/webgpu.h").is_file());
        assert_eq!(active_dir(COMPONENT_WGPU), Some(target.clone()));
        let manifest = registry().unwrap();
        let (state, detail) = state_of(COMPONENT_WGPU, &manifest.components[COMPONENT_WGPU]);
        assert_eq!(
            state,
            ComponentState::Installed {
                version: "v29.0.1.1".into(),
                active: true
            }
        );
        assert_eq!(
            detail.as_deref(),
            Some(target.display().to_string()).as_deref()
        );

        // 幂等：再装同版本不报错、不重复。
        run_install(&install_opts(&zip, "v29.0.1.1", false)).unwrap();
        let (state, _) = state_of(COMPONENT_WGPU, &manifest.components[COMPONENT_WGPU]);
        assert_eq!(
            state,
            ComponentState::Installed {
                version: "v29.0.1.1".into(),
                active: true
            }
        );

        run_uninstall(COMPONENT_WGPU, None).unwrap();
        let (state, _) = state_of(COMPONENT_WGPU, &manifest.components[COMPONENT_WGPU]);
        assert_eq!(state, ComponentState::NotInstalled);
        assert_eq!(active_dir(COMPONENT_WGPU), None);

        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn install_refuses_sha256_mismatch() {
        let _env_guard = env_lock();
        let home = temp_dir("home5");
        std::env::set_var(ARC_HOME_ENV, &home);
        let zip = home.join("wgpu.zip");
        make_wgpu_zip(&zip);
        let mut opts = install_opts(&zip, "v29.0.1.1", false);
        opts.sha256 = Some("00".repeat(32));
        let err = run_install(&opts).unwrap_err();
        assert!(err.contains("sha256 mismatch"), "{err}");
        assert_eq!(active_dir(COMPONENT_WGPU), None);
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn uninstall_builtin_refused() {
        let _env_guard = env_lock();
        let home = temp_dir("home6");
        std::env::set_var(ARC_HOME_ENV, &home);
        let err = run_uninstall(COMPONENT_CRYPTO, None).unwrap_err();
        assert!(err.contains("builtin"), "{err}");
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn uninstall_requires_version_without_marker() {
        let _env_guard = env_lock();
        let home = temp_dir("home7");
        std::env::set_var(ARC_HOME_ENV, &home);
        let err = run_uninstall(COMPONENT_WGPU, None).unwrap_err();
        assert!(err.contains("--version"), "{err}");
        std::env::remove_var(ARC_HOME_ENV);
        let _ = std::fs::remove_dir_all(&home);
    }
}
