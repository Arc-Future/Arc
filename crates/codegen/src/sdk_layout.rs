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
//! <root>/bin/arc.exe                   <root>/std/
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

/// 安装态 SDK 根标记：`bin/arc.exe` + `lib/rt`（runtime C 源码）或 `lib/std`。
fn is_installed_sdk_root(dir: &Path) -> bool {
    if !dir.join("bin").join("arc.exe").is_file() {
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

/// 自定位 SDK 根。
///
/// 优先级：`ARC_SDK_ROOT` 环境变量 → `current_exe()` 向上逐级找标记目录。
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
    let name = if cfg!(target_os = "windows") {
        "clang.exe"
    } else {
        "clang"
    };
    let exe = llvm.join(ver).join("bin").join(name);
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
        std::fs::write(root.join("bin/arc.exe"), b"").unwrap();
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
        std::fs::write(root.join("bin/arc.exe"), b"").unwrap();
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
        let exe = if cfg!(target_os = "windows") {
            "clang.exe"
        } else {
            "clang"
        };
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
