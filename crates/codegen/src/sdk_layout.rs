//! SDK 布局与资源自定位（Phase 0：可重定位）。
//!
//! ## 背景
//!
//! 安装方案调研（`target/scratch/arc-dev-env-installer-design.md`）发现 `arc.exe`
//! 不可重定位：codegen/loader 在编译期用 `env!("CARGO_MANIFEST_DIR")` 固化绝对
//! 路径（runtime C 源码、vendored DLL、`std/` 兜底、rt_cache），产物离开构建机
//! 目录即失效。本模块在**运行期**按 `current_exe()` 自定位 SDK 根（Go 式 GOROOT
//! 模式），取代编译期固化。
//!
//! ## 布局契约
//!
//! 两种 SDK 布局等价识别：
//!
//! ```text
//! 安装态（分发物）                      仓库态（开发，仓库自身即开发 SDK）
//! <root>/bin/arc[.exe]                 <root>/std/
//!   （Windows `arc.exe` / Unix `arc`）
//! <root>/lib/std/                      <root>/crates/runtime/
//! <root>/lib/rt/                       <root>/crates/runtime-ui/
//!   └── runtime/、runtime-ui/、        <root>/crates/runtime-drawing/
//!       runtime-drawing/、             <root>/crates/runtime-sqlite/
//!       runtime-sqlite/、              <root>/crates/runtime-crypto/
//!       runtime-crypto/                <root>/crates/arc/native/
//! <root>/lib/native/
//! ```
//!
//! 资源根判定链：`ARC_SDK_ROOT` 环境变量（显式覆盖）→ `current_exe()` 逐级向上
//! 找标记目录 → 编译期 `CARGO_MANIFEST_DIR` 开发兜底（仅当自定位失败时生效）。

use std::path::{Path, PathBuf};

/// `ARC_SDK_ROOT` 环境变量名：显式指定 SDK 根（可重定位的显式覆盖）。
pub const ARC_SDK_ROOT_ENV: &str = "ARC_SDK_ROOT";

/// 安装态 `arc` 可执行文件名：Windows `arc.exe`，其余平台 `arc`。
///
/// SDK 安装态根标记（[`is_installed_sdk_root`]）、`arc doctor` 结构检查与
/// `arc self-update` 布局共用此单一解析——Unix 分发物按 `bin/arc` 落盘
/// （`scripts/packaging/arc-install.sh` 指针布局），Windows 按 `bin/arc.exe`
/// （`install.ps1`）。禁止各处各自 cfg 推导（双轨）。
pub fn installed_arc_exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "arc.exe"
    } else {
        "arc"
    }
}

/// clang 可执行文件名：Windows `clang.exe`，其余平台 `clang`。
///
/// toolchain 指针路径与 SDK 捆绑路径共用（clang 解析序单一来源，
/// 供 `mangle::clang_path` 的解析链消费）。
fn clang_exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "clang.exe"
    } else {
        "clang"
    }
}

/// 安装态 SDK 根标记：`bin/arc[.exe]`（见 [`installed_arc_exe_name`]）
/// + `lib/rt`（runtime C 源码）或 `lib/std`。
fn is_installed_sdk_root(dir: &Path) -> bool {
    if !dir.join("bin").join(installed_arc_exe_name()).is_file() {
        return false;
    }
    dir.join("lib").join("rt").is_dir() || dir.join("lib").join("std").is_dir()
}

/// 仓库态 SDK 根标记：`std/` + `crates/runtime`（仓库自身即开发 SDK）。
fn is_repo_sdk_root(dir: &Path) -> bool {
    dir.join("std").is_dir() && dir.join("crates").join("runtime").is_dir()
}

/// 判断目录是否为合法 SDK 根（安装态或仓库态布局）。
pub fn is_sdk_root(dir: &Path) -> bool {
    is_installed_sdk_root(dir) || is_repo_sdk_root(dir)
}

/// 安装指针布局 → 活动版本 SDK 根解析。
///
/// 安装布局（`install.ps1` / `arc-install.sh` / `arc self-update` 同源契约）：
/// `<R>/bin/arc(.exe)` 是活动版本的稳定副本（唯一 PATH 注入点），版本目录
/// 在 `<R>/versions/arc-<ver>-<triple>/`（含完整 SDK）。PATH 上的指针在普通
/// 调用路径**不 re-exec**——本解析使指针副本同样能自定位：读
/// `<R>/versions/current` 标记 → 在 `versions/` 下找前缀 `arc-<ver>-` 且含
/// `bin/arc(.exe)` + `lib/{std,rt}` 的版本目录，返回其作为 SDK 根。
///
/// 指针与版本目录内 exe 内容相同，直接解析（而非 spawn 活动版本）既保持
/// 单进程语义，又保留指针设计对更新/回滚的收益（运行中的 exe 是根 bin 副本，
/// 版本目录可被 `arc self-update` 安全替换）。
fn resolve_pointer_sdk_root(install_root: &Path) -> Option<PathBuf> {
    if !install_root
        .join("bin")
        .join(installed_arc_exe_name())
        .is_file()
    {
        return None;
    }
    let versions_dir = install_root.join("versions");
    let raw = std::fs::read_to_string(versions_dir.join("current")).ok()?;
    let ver = raw.trim().trim_start_matches('\u{feff}').to_string();
    if ver.is_empty() {
        return None;
    }
    let prefix = format!("arc-{ver}-");
    let entries = std::fs::read_dir(&versions_dir).ok()?;
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) {
            continue;
        }
        let sdk = entry.path();
        if sdk.join("bin").join(installed_arc_exe_name()).is_file()
            && (sdk.join("lib").join("rt").is_dir() || sdk.join("lib").join("std").is_dir())
        {
            return Some(sdk);
        }
    }
    None
}

/// 自定位 SDK 根。
///
/// 优先级：`ARC_SDK_ROOT` 环境变量 → `current_exe()` 向上逐级找标记目录
/// （安装态/仓库态/安装指针布局，见 [`resolve_pointer_sdk_root`]）。
/// 显式环境变量即使目录缺标记也原样返回，让下游给出明确错误。
pub fn sdk_root() -> Option<PathBuf> {
    if let Ok(root) = std::env::var(ARC_SDK_ROOT_ENV) {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    loop {
        if is_sdk_root(&dir) {
            return Some(dir);
        }
        if let Some(sdk) = resolve_pointer_sdk_root(&dir) {
            return Some(sdk);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// SDK 布局形态（决定资源子目录命名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkLayoutKind {
    /// `lib/{std,rt,native}`（分发物）。
    Installed,
    /// `{std,crates}`（仓库）。
    Repo,
}

impl SdkLayoutKind {
    /// 人类可读标签（`arc env` 输出）。
    pub fn label(self) -> &'static str {
        match self {
            SdkLayoutKind::Installed => "installed",
            SdkLayoutKind::Repo => "repo",
        }
    }
}

fn layout_kind(root: &Path) -> SdkLayoutKind {
    if is_installed_sdk_root(root) {
        SdkLayoutKind::Installed
    } else {
        SdkLayoutKind::Repo
    }
}

/// 判定 SDK 布局形态；目录缺任何布局标记返回 `None`（仅在 `ARC_SDK_ROOT`
/// 显式覆盖了非 SDK 目录时出现，供 `arc env` / `arc doctor` 报出明确诊断）。
pub fn detect_layout_kind(root: &Path) -> Option<SdkLayoutKind> {
    if is_installed_sdk_root(root) {
        Some(SdkLayoutKind::Installed)
    } else if is_repo_sdk_root(root) {
        Some(SdkLayoutKind::Repo)
    } else {
        None
    }
}

/// runtime C 源码基目录：含 `runtime/`、`runtime-ui/`、`runtime-drawing/`、
/// `runtime-sqlite/`、`runtime-crypto/` 子目录。
///
/// 安装态 `<sdk>/lib/rt`；仓库态 `<root>/crates`；自定位失败时回退
/// `CARGO_MANIFEST_DIR` 上溯一级（`crates/`，开发兜底）。
pub fn sdk_runtime_base() -> PathBuf {
    if let Some(root) = sdk_root() {
        return match layout_kind(&root) {
            SdkLayoutKind::Installed => root.join("lib").join("rt"),
            SdkLayoutKind::Repo => root.join("crates"),
        };
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("crates"))
}

/// SDK 捆绑 std 根目录：安装态 `<sdk>/lib/std`；仓库态 `<root>/std`。
///
/// 自定位失败时回退编译器源码树 `<repo>/std`（开发兜底）；均不存在返回 `None`。
pub fn sdk_std_root() -> Option<PathBuf> {
    if let Some(root) = sdk_root() {
        let candidate = match layout_kind(&root) {
            SdkLayoutKind::Installed => root.join("lib").join("std"),
            SdkLayoutKind::Repo => root.join("std"),
        };
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    let repo_std = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .join("std");
    repo_std.is_dir().then_some(repo_std)
}

/// 内置 native 契约目录：安装态 `<sdk>/lib/native`；仓库态 `<root>/crates/arc/native`。
///
/// 自定位失败时回退 `CARGO_MANIFEST_DIR` 上溯一级 + `arc/native`（开发兜底）。
pub fn sdk_native_dir() -> PathBuf {
    if let Some(root) = sdk_root() {
        return match layout_kind(&root) {
            SdkLayoutKind::Installed => root.join("lib").join("native"),
            SdkLayoutKind::Repo => root.join("crates").join("arc").join("native"),
        };
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("arc").join("native"))
        .unwrap_or_else(|| PathBuf::from("native"))
}

/// 用户主目录（`HOME` / `USERPROFILE` 兜底 `.`）。
fn user_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 用户级 runtime `.o` 缓存根：`$ARC_HOME/rt_cache`（未设时 `~/.arc/rt_cache`）。
///
/// 缓存是用户数据，不随 SDK 目录移动（对标 Go `GOCACHE`）；卸载 SDK 不影响缓存。
pub fn runtime_cache_dir() -> PathBuf {
    if let Ok(home) = std::env::var("ARC_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("rt_cache");
        }
    }
    user_home_dir().join(".arc").join("rt_cache")
}

/// 用户级产物共享缓存根：`$ARC_HOME/cache`（未设时 `~/.arc/cache`）。
///
/// RFC 017 产物域（U3 dll 单副本，UX 迭代评审 §2.3）：vendored 运行时 dll
///（wgpu_native / crypto_native，合计 ~75 MB）的唯一落点，项目 bin/ 经硬链接
/// 引用而非逐项目复制。`arc clean --cache` 清理此目录。
pub fn native_cache_dir() -> PathBuf {
    if let Ok(home) = std::env::var("ARC_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("cache");
        }
    }
    user_home_dir().join(".arc").join("cache")
}

/// toolchain 工具根：`$ARC_HOME/tools`（未设则 `~/.arc/tools`）。
///
/// 按需工具链（`arc toolchain install llvm`）的落点，见 RFC 031 §12。
pub fn toolchain_tools_root() -> PathBuf {
    if let Ok(home) = std::env::var("ARC_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("tools");
        }
    }
    user_home_dir().join(".arc").join("tools")
}

/// LLVM 工具链目录：`<tools>/llvm`。
pub fn toolchain_llvm_dir() -> PathBuf {
    toolchain_tools_root().join("llvm")
}

/// 按需组件根：`<tools_root>/components`（`arc component` 落点，Phase 3）。
///
/// 布局：`components/<name>/current`（活动版本指针）+ `components/<name>/<ver>/`
///（版本目录，多版本共存），与 toolchain `llvm/` 同一指针模式。
pub fn components_root() -> PathBuf {
    toolchain_tools_root().join("components")
}

/// 单个组件的版本目录根：`<tools_root>/components/<name>`。
pub fn component_dir(name: &str) -> PathBuf {
    components_root().join(name)
}

/// 组件活动版本目录：读 `<name>/current` 指针 → `<name>/<ver>`（目录存在才返回）。
///
/// 供 codegen 联动（wgpu 二进制目录解析）与 `arc component` 状态查询共用，
/// 单一解析序，避免双轨。
pub fn component_active_dir(name: &str) -> Option<PathBuf> {
    let dir = component_dir(name);
    let raw = std::fs::read_to_string(dir.join("current")).ok()?;
    let ver = raw.trim();
    if ver.is_empty() {
        return None;
    }
    let active = dir.join(ver);
    active.is_dir().then_some(active)
}

/// toolchain 管理的 clang 路径：读 `llvm/current` 指针 → `<ver>/bin/clang[.exe]`。
///
/// 返回 `Some` 仅当指针存在且 clang 文件存在；供 `mangle::clang_path` 在
/// `ARC_CLANG` 之后、标准安装位之前解析（单一解析序，`arc env` / `arc doctor`
/// 与 `arc build` 共用）。
pub fn toolchain_llvm_clang_path() -> Option<PathBuf> {
    let llvm = toolchain_llvm_dir();
    let raw = std::fs::read_to_string(llvm.join("current")).ok()?;
    let ver = raw.trim();
    if ver.is_empty() {
        return None;
    }
    let name = clang_exe_name();
    let exe = llvm.join(ver).join("bin").join(name);
    exe.is_file().then_some(exe)
}

/// SDK 捆绑 clang 路径：`<sdk-root>/lib/llvm/bin/clang[.exe]`（安装包 Phase 3
/// `-BundleLlm` 落点；仅带捆绑 LLVM 的安装态分发物存在）。
///
/// 返回 `Some` 仅当 clang 文件实际存在。供 `mangle::clang_path` 在 toolchain
/// 指针之后、系统安装位之前解析——捆绑版 LLVM 是分发包自带的离线构建基线，
/// 自动接线后用户解压 SDK 即可离线 `arc build`，无需手工设 `ARC_CLANG`。
pub fn bundled_llvm_clang_path() -> Option<PathBuf> {
    let root = sdk_root()?;
    let name = clang_exe_name();
    let exe = root.join("lib").join("llvm").join("bin").join(name);
    exe.is_file().then_some(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `$ARC_HOME` 为进程级全局：codegen sdk_layout 各 ARC_HOME 相关测试须串行，
    /// 否则并行下相互覆盖导致偶发漂移（与 `arc::ENV_TEST_MUTEX` 同一模式）。
    static ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arc-sdk-layout-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn installed_layout_marker_detected() {
        let root = temp_dir("installed");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join(installed_arc_exe_name()), b"").unwrap();
        std::fs::create_dir_all(root.join("lib/rt/runtime")).unwrap();
        assert!(is_sdk_root(&root));
        // lib 缺 rt 但缺 bin 不判为 SDK 根。
        let no_bin = temp_dir("no-bin");
        std::fs::create_dir_all(no_bin.join("lib/std")).unwrap();
        assert!(!is_sdk_root(&no_bin));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&no_bin);
    }

    #[test]
    fn installed_marker_requires_host_exe_name() {
        // Unix 分发物落盘 `bin/arc`、Windows `bin/arc.exe`——错名文件不冒充
        // SDK 根标记（防 Unix 上旧 `arc.exe` 布局误判、Windows 上 `arc` 误判）。
        let root = temp_dir("marker-name");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("lib/rt/runtime")).unwrap();
        let wrong = if cfg!(target_os = "windows") {
            "arc"
        } else {
            "arc.exe"
        };
        std::fs::write(root.join("bin").join(wrong), b"").unwrap();
        assert!(!is_sdk_root(&root));
        std::fs::write(root.join("bin").join(installed_arc_exe_name()), b"").unwrap();
        assert!(is_sdk_root(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pointer_layout_resolves_active_version_sdk() {
        let root = temp_dir("ptr");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join(installed_arc_exe_name()), b"").unwrap();
        std::fs::create_dir_all(root.join("versions")).unwrap();
        std::fs::write(root.join("versions/current"), "1.0.0\n").unwrap();
        // 活动版本目录（完整 SDK）与非活动版本目录并存——前缀过滤须取对。
        let active = root.join("versions/arc-1.0.0-x86_64-pc-windows-msvc");
        std::fs::create_dir_all(active.join("bin")).unwrap();
        std::fs::write(active.join("bin").join(installed_arc_exe_name()), b"").unwrap();
        std::fs::create_dir_all(active.join("lib/std/Arc")).unwrap();
        std::fs::create_dir_all(root.join("versions/arc-0.9.0-x86_64-pc-windows-msvc/bin"))
            .unwrap();
        std::fs::write(
            root.join("versions/arc-0.9.0-x86_64-pc-windows-msvc/bin")
                .join(installed_arc_exe_name()),
            b"",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("versions/arc-0.9.0-x86_64-pc-windows-msvc/lib/std"))
            .unwrap();
        assert_eq!(resolve_pointer_sdk_root(&root), Some(active));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pointer_layout_requires_marker_and_complete_active_dir() {
        let mk = |root: &Path| {
            std::fs::create_dir_all(root.join("bin")).unwrap();
            std::fs::write(root.join("bin").join(installed_arc_exe_name()), b"").unwrap();
            std::fs::create_dir_all(root.join("versions")).unwrap();
        };
        // 无 current 标记 → None。
        let no_marker = temp_dir("ptr-no-marker");
        mk(&no_marker);
        assert_eq!(resolve_pointer_sdk_root(&no_marker), None);
        // 标记存在但版本目录缺 bin/<exe> → None（不把半成品目录当 SDK）。
        let broken = temp_dir("ptr-broken");
        mk(&broken);
        std::fs::write(broken.join("versions/current"), "1.0.0\n").unwrap();
        std::fs::create_dir_all(broken.join("versions/arc-1.0.0-x86_64-pc-windows-msvc/lib/std"))
            .unwrap();
        assert_eq!(resolve_pointer_sdk_root(&broken), None);
        let _ = std::fs::remove_dir_all(&no_marker);
        let _ = std::fs::remove_dir_all(&broken);
    }

    #[test]
    fn repo_layout_marker_detected() {
        let root = temp_dir("repo");
        std::fs::create_dir_all(root.join("std/Arc")).unwrap();
        std::fs::create_dir_all(root.join("crates/runtime")).unwrap();
        assert!(is_sdk_root(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sdk_root_respects_env_override() {
        let root = temp_dir("env");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join(installed_arc_exe_name()), b"").unwrap();
        std::fs::create_dir_all(root.join("lib/rt/runtime")).unwrap();
        std::env::set_var(ARC_SDK_ROOT_ENV, &root);
        assert_eq!(sdk_root(), Some(root.clone()));
        // 显式覆盖即使布局不全也原样返回。
        std::env::remove_var(ARC_SDK_ROOT_ENV);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_cache_dir_under_arc_home() {
        let _lock = ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temp_dir("home");
        std::env::set_var("ARC_HOME", &home);
        assert_eq!(runtime_cache_dir(), home.join("rt_cache"));
        std::env::remove_var("ARC_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn toolchain_paths_follow_arc_home_and_pointer() {
        let _lock = ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temp_dir("tools");
        std::env::set_var("ARC_HOME", &home);
        assert_eq!(toolchain_tools_root(), home.join("tools"));
        assert_eq!(toolchain_llvm_dir(), home.join("tools").join("llvm"));
        // 无指针 → None
        assert_eq!(toolchain_llvm_clang_path(), None);
        // 指针指向存在的版本目录 → Some
        std::fs::create_dir_all(home.join("tools/llvm/22.1.8/bin")).unwrap();
        std::fs::write(home.join("tools/llvm/current"), "22.1.8\n").unwrap();
        let exe = clang_exe_name();
        std::fs::write(home.join(format!("tools/llvm/22.1.8/bin/{exe}")), b"").unwrap();
        assert_eq!(
            toolchain_llvm_clang_path(),
            Some(home.join(format!("tools/llvm/22.1.8/bin/{exe}")))
        );
        // 指针指向缺失目录 → None
        std::fs::write(home.join("tools/llvm/current"), "99.0.0\n").unwrap();
        assert_eq!(toolchain_llvm_clang_path(), None);
        std::env::remove_var("ARC_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn bundled_llvm_clang_under_sdk_root() {
        let _lock = ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = temp_dir("bundled-llvm");
        let exe = clang_exe_name();
        std::fs::create_dir_all(root.join("lib/llvm/bin")).unwrap();
        // 无 clang 文件 → None（捆绑缺失不冒充可用）。
        assert_eq!(bundled_llvm_clang_path(), None);
        std::fs::write(root.join("lib/llvm/bin").join(exe), b"").unwrap();
        std::env::set_var(ARC_SDK_ROOT_ENV, &root);
        assert_eq!(
            bundled_llvm_clang_path(),
            Some(root.join("lib/llvm/bin").join(exe))
        );
        std::env::remove_var(ARC_SDK_ROOT_ENV);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn component_paths_follow_arc_home_and_pointer() {
        let _lock = ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = temp_dir("components");
        std::env::set_var("ARC_HOME", &home);
        assert_eq!(components_root(), home.join("tools").join("components"));
        assert_eq!(component_dir("wgpu"), home.join("tools/components/wgpu"));
        // 无指针 → None
        assert_eq!(component_active_dir("wgpu"), None);
        // 指针指向存在的版本目录 → Some
        std::fs::create_dir_all(home.join("tools/components/wgpu/v29.0.1.1/bin/windows")).unwrap();
        std::fs::write(home.join("tools/components/wgpu/current"), "v29.0.1.1\n").unwrap();
        assert_eq!(
            component_active_dir("wgpu"),
            Some(home.join("tools/components/wgpu/v29.0.1.1"))
        );
        // 指针指向缺失目录 → None
        std::fs::write(home.join("tools/components/wgpu/current"), "v99.0.0\n").unwrap();
        assert_eq!(component_active_dir("wgpu"), None);
        std::env::remove_var("ARC_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }
}
