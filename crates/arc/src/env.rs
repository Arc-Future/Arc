//! `arc env` / `arc doctor` 共享的环境快照（Phase 1：SDK 打包与工具命令）。
//!
//! 对标 `go env`：`arc env` 输出当前 SDK 根、std/rt/native 路径、rt_cache 与
//! std 解析链胜出来源（`[std].path` / SDK / `ARC_STD_ROOT` / workspace），
//! 供诊断与 CI 消费；`--json` 输出机器可读快照（`go env -json` 风格）。

use std::path::{Path, PathBuf};

use crate::manifest::StdSection;
use crate::target::TargetTriple;

use codegen::sdk_layout::{
    detect_layout_kind, runtime_cache_dir, sdk_native_dir, sdk_root, sdk_runtime_base,
    sdk_std_root, toolchain_llvm_clang_path, toolchain_tools_root, SdkLayoutKind, ARC_SDK_ROOT_ENV,
};

/// `ARC_STD_ROOT`：显式指定 std 库根目录（开发调试用）。
pub const ARC_STD_ROOT_ENV: &str = "ARC_STD_ROOT";
/// `ARC_HOME`：用户级工具链域根（cache / rt_cache / arc-keys）。
pub const ARC_HOME_ENV: &str = "ARC_HOME";
/// `ARC_CLANG`：clang 二进制显式覆盖。
pub const ARC_CLANG_ENV: &str = "ARC_CLANG";

/// std 解析链的胜出来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdSource {
    /// `[std].path`（arc.toml 项目显式覆盖）
    Project,
    /// SDK 捆绑 std
    Sdk,
    /// `ARC_STD_ROOT` 环境变量
    Env,
    /// `workspace/std` 兜底
    Workspace,
}

impl StdSource {
    /// 人类可读标签（`arc env` 输出）。
    pub fn label(self) -> &'static str {
        match self {
            StdSource::Project => "[std].path",
            StdSource::Sdk => "sdk",
            StdSource::Env => "ARC_STD_ROOT",
            StdSource::Workspace => "workspace",
        }
    }
}

/// 环境快照：`arc env` 输出；`arc doctor` 消费。
#[derive(Debug, Clone)]
pub struct EnvSnapshot {
    /// 编译器版本（`CARGO_PKG_VERSION`）。
    pub version: String,
    /// `arc` 可执行文件路径（`current_exe()`）。
    pub exe: PathBuf,
    /// SDK 根（自定位或 `ARC_SDK_ROOT` 显式覆盖）；None 表示两者均失败。
    pub sdk_root: Option<PathBuf>,
    /// SDK 布局形态（None = 目录缺布局标记）。
    pub sdk_layout: Option<SdkLayoutKind>,
    /// 生效的 std 根（完整解析链结果）。
    pub std_root: PathBuf,
    /// std 解析链胜出来源。
    pub std_source: StdSource,
    /// `[std].path` 原始值（当前项目 arc.toml 声明；未声明为 None）。
    pub std_manifest_path: Option<String>,
    /// `ARC_STD_ROOT` 原始值（未设为 None）。
    pub arc_std_root_env: Option<String>,
    /// runtime C 源码基目录（`runtime/`、`runtime-ui/` 等子目录所在）。
    pub rt_base: PathBuf,
    /// 内置 native 契约（`.ani`）目录。
    pub native_dir: PathBuf,
    /// 用户级 runtime `.o` 缓存根（`$ARC_HOME/rt_cache`）。
    pub rt_cache: PathBuf,
    /// 用户级工具链域根 `$ARC_HOME`。
    pub arc_home: PathBuf,
    /// 当前目录向上定位的 workspace 根（`find_workspace_root`，含 SDK 兜底）。
    pub workspace_root: PathBuf,
    /// 当前项目 `arc.toml` 所在目录（无项目为 None）。
    pub manifest_dir: Option<PathBuf>,
    /// clang 二进制解析结果（与 `arc build` 同一解析序）。
    pub clang: String,
    /// 宿主 target triple。
    pub host_triple: String,
    /// toolchain 工具根（`arc toolchain install llvm` 落点 `<tools>/llvm`）。
    pub arc_tools_root: PathBuf,
    /// 按需组件根（`arc component install` 落点 `<tools>/components`）。
    pub arc_components_root: PathBuf,
    /// toolchain 管理的 clang（`<tools>/llvm/current` 指针解析；未装为 None）。
    pub toolchain_clang: Option<PathBuf>,
    /// clang 支持基线（与 `.aopkg` metadata / manifest `clang_min_version` 对齐）。
    pub clang_min_version: String,
}

/// 读取环境变量（空串视为未设置）。
pub fn env_var(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

/// 用户级工具链域根：`$ARC_HOME` 优先，未设则 `~/.arc`（跨平台 HOME/USERPROFILE）。
pub fn resolve_arc_home() -> PathBuf {
    if let Some(home) = env_var(ARC_HOME_ENV) {
        return PathBuf::from(home);
    }
    let base = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .unwrap_or_else(|| ".".to_string());
    PathBuf::from(base).join(".arc")
}

/// std 根完整解析链（含胜出来源）：`[std].path` → SDK 捆绑 → `ARC_STD_ROOT` → workspace。
///
/// 与 [`crate::manifest::resolve_effective_std_root`] 同序；本函数额外返回来源标签，
/// 供 `arc env` 展示「生效的解析链」。
pub fn resolve_std_with_source(
    workspace: &Path,
    manifest_dir: Option<&Path>,
    std: Option<&StdSection>,
) -> (PathBuf, StdSource) {
    if let Some(section) = std {
        let resolved = crate::manifest::resolve_std_root(workspace, manifest_dir, Some(section));
        return (resolved, StdSource::Project);
    }
    if let Some(sdk_std) = sdk_std_root() {
        if sdk_std.is_dir() {
            return (sdk_std, StdSource::Sdk);
        }
    }
    if let Some(env_root) = env_var(ARC_STD_ROOT_ENV) {
        let p = PathBuf::from(env_root);
        return (p.canonicalize().unwrap_or(p), StdSource::Env);
    }
    (
        crate::manifest::resolve_std_root(workspace, None, None),
        StdSource::Workspace,
    )
}

/// 采集当前目录环境快照。
pub fn snapshot() -> EnvSnapshot {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let workspace_root = crate::loader::find_workspace_root(&cwd);
    let manifest = crate::manifest::find_arc_manifest(&cwd);
    let manifest_dir = manifest.as_ref().map(|(dir, _)| dir.clone());
    let std_section = manifest.as_ref().and_then(|(_, m)| m.std.as_ref());
    let std_manifest_path = std_section.map(|s| s.path.clone());
    let arc_std_root_env = env_var(ARC_STD_ROOT_ENV);
    let (std_root, std_source) =
        resolve_std_with_source(&workspace_root, manifest_dir.as_deref(), std_section);
    let sdk_root = sdk_root();
    let sdk_layout = sdk_root.as_deref().and_then(detect_layout_kind);

    EnvSnapshot {
        version: env!("CARGO_PKG_VERSION").to_string(),
        exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("arc")),
        sdk_root,
        sdk_layout,
        std_root,
        std_source,
        std_manifest_path,
        arc_std_root_env,
        rt_base: sdk_runtime_base(),
        native_dir: sdk_native_dir(),
        rt_cache: runtime_cache_dir(),
        arc_home: resolve_arc_home(),
        workspace_root,
        manifest_dir,
        clang: codegen::clang_path(),
        host_triple: TargetTriple::host().as_str().to_string(),
        arc_tools_root: toolchain_tools_root(),
        arc_components_root: codegen::sdk_layout::components_root(),
        toolchain_clang: toolchain_llvm_clang_path(),
        clang_min_version: crate::clang_version::LLVM_MIN_VERSION.to_string(),
    }
}

/// 将快照输出为键值对映射（human 与 JSON 共用数据源，顺序确定）。
fn key_values(s: &EnvSnapshot) -> Vec<(&'static str, String)> {
    let layout = s
        .sdk_layout
        .map(|l| l.label().to_string())
        .unwrap_or_else(|| "none".to_string());
    vec![
        ("ARC_VERSION", s.version.clone()),
        ("ARC_EXE", s.exe.display().to_string()),
        (
            ARC_SDK_ROOT_ENV,
            s.sdk_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        ),
        ("SDK_LAYOUT", layout),
        ("ARC_STD_ROOT", s.std_root.display().to_string()),
        ("STD_SOURCE", s.std_source.label().to_string()),
        (
            "MANIFEST_STD_PATH",
            s.std_manifest_path.clone().unwrap_or_default(),
        ),
        (
            "ARC_STD_ROOT_ENV",
            s.arc_std_root_env.clone().unwrap_or_default(),
        ),
        ("ARC_RT_BASE", s.rt_base.display().to_string()),
        ("ARC_NATIVE_DIR", s.native_dir.display().to_string()),
        ("ARC_RT_CACHE", s.rt_cache.display().to_string()),
        ("ARC_HOME", s.arc_home.display().to_string()),
        ("ARC_WORKSPACE_ROOT", s.workspace_root.display().to_string()),
        (
            "MANIFEST_DIR",
            s.manifest_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        ),
        ("ARC_CLANG", s.clang.clone()),
        ("HOST_TRIPLE", s.host_triple.clone()),
        ("ARC_TOOLS_ROOT", s.arc_tools_root.display().to_string()),
        (
            "ARC_COMPONENTS_ROOT",
            s.arc_components_root.display().to_string(),
        ),
        (
            "ARC_TOOLCHAIN_CLANG",
            s.toolchain_clang
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        ),
        ("CLANG_MIN_VERSION", s.clang_min_version.clone()),
    ]
}

/// Human 输出（`go env` 风格：`NAME="value"` 一行一个）。
pub fn format_human(s: &EnvSnapshot) -> String {
    let mut out = String::new();
    for (name, value) in key_values(s) {
        out.push_str(&format!("{name}=\"{value}\"\n"));
    }
    out
}

/// JSON 输出（`go env -json` 风格：扁平对象，键为环境变量名）。
pub fn format_json(s: &EnvSnapshot) -> Result<String, String> {
    let map: std::collections::BTreeMap<String, String> = key_values(s)
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    serde_json::to_string_pretty(&map).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn std_chain_project_override_wins() {
        // `[std].path` 显式覆盖优先于 SDK / 环境变量（短路径先行验证）。
        let dir = std::env::temp_dir().join(format!("arc-env-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sec = StdSection {
            path: dir.display().to_string(),
        };
        let (resolved, source) =
            resolve_std_with_source(Path::new("."), Some(Path::new(".")), Some(&sec));
        assert_eq!(source, StdSource::Project);
        assert!(resolved.is_absolute());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_var_reader_treats_empty_as_unset() {
        std::env::remove_var("ARC_STD_ROOT");
        assert_eq!(env_var("ARC_STD_ROOT"), None);
        std::env::set_var("ARC_STD_ROOT", "");
        assert_eq!(env_var("ARC_STD_ROOT"), None);
        std::env::set_var("ARC_STD_ROOT", "x");
        assert_eq!(env_var("ARC_STD_ROOT"), Some("x".to_string()));
        std::env::remove_var("ARC_STD_ROOT");
    }
}
