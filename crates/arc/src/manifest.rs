//! Minimal `arc.toml` project manifest (RFC 007 / RFC 008 / RFC 003).
//!
//! 字段定义权威参考：`docs/rfc/031-compiler-cli.md`（arc.toml 面）。
//! 本文件为该 schema 的参考实现；新增字段须同步更新 5.2 与 RFC 出处交叉引用表。
//!
//! RFC 003 M2：`[package].global_usings` 由本模块解析，loader 合成 `global using`。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid arc.toml at {path}: {message}")]
    Invalid { path: PathBuf, message: String },
}

/// Parsed `[package]` section from `arc.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcManifest {
    pub package: PackageSection,
    /// `[dependencies]` 段（RFC 025 M2 / RFC 007）：包名 → 依赖规格。
    ///
    /// Phase 0 以 `path` 为主；`version` 字段可解析但本切片不跑 MVS。
    pub dependencies: BTreeMap<String, DependencySpec>,
    /// `[native]` 段（RFC 016 M2）：声明 native 契约库搜索路径。
    pub native: NativeSection,
    /// `[ui]` 段（RFC 037 M2）：声明 ARML 项目源文件清单。
    ///
    /// 当 `arc build` 接受 arc.toml 项目路径时，若含此节则触发 ARML codegen
    /// 流程：对每个 `arml` 文件生成 `.g.as` 到 `obj/<config>/`，合并所有
    /// `.g.as` + `sources` 为 `obj/<config>/Program.as`，再编译为可执行文件。
    /// 对标 WPF 的 `dotnet build` 自动 XAML codegen + csc 编译流程。
    pub ui: Option<UiSection>,
    /// `[qif]` 段（RFC 032 M1）：QIF 测试执行配置。
    pub qif: QifSection,
    /// `[compiler]` 段（RFC 005 里程碑④）：编译器行为旋钮。
    pub compiler: CompilerSection,
    /// `[workspace]` 段（解决方案 = workspace 聚合）。
    ///
    /// 对标 C# `.sln`：一个 `arc.toml` 承载多成员项目聚合，`arc build` 在
    /// workspace 根可一键全量构建全部成员。**解决方案即 workspace，仍由
    /// `arc.toml` 承载**——不引入独立 `.arcsln` 文件格式。
    pub workspace: WorkspaceSection,
    /// `[std]` 段（RFC 031 §8）：std 库路径覆盖（开发调试用）。
    ///
    /// 缺省 `None` 时走完整解析链（SDK 捆绑 std → `ARC_STD_ROOT` → 内置
    /// `workspace/std`）。消费方经 [`resolve_effective_std_root`] 统一解析。
    pub std: Option<StdSection>,
    pub path: PathBuf,
}

/// Parsed `[workspace]` section from `arc.toml`（解决方案 = workspace 聚合）。
///
/// 对标 C# `.sln` 的成员枚举：
/// ```toml
/// [workspace]
/// members = ["src/App", "src/Lib"]   # 相对 workspace 根的成员项目目录
/// ```
///
/// 每个成员必须是含自身 `arc.toml` 的独立项目。`arc build/check` 在 workspace
/// 根执行时，按依赖拓扑顺序逐一构建全部成员（对标 `dotnet sln` 一键构建）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceSection {
    /// workspace 成员项目（相对 workspace 根的子目录路径，各含 `arc.toml`）。
    pub members: Vec<String>,
}

impl WorkspaceSection {
    /// 是否构成解决方案（聚合多个成员项目）。
    pub fn is_solution(&self) -> bool {
        !self.members.is_empty()
    }

    /// 成员是否为空（非解决方案）。
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// 从文件读取 `[workspace]` 段——**不要求 `[package]`**，用于发现纯解决方案根
    /// （`arc.toml` 仅含 `[workspace] members`，对标独立 `.sln`）。
    pub fn from_file(path: &Path) -> Result<Self, ManifestError> {
        let source = std::fs::read_to_string(path).map_err(|e| ManifestError::Read {
            path: path.to_path_buf(),
            source: e,
        })?;
        let table: toml::Table = toml::from_str(&source).map_err(|e| ManifestError::Invalid {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        Ok(table
            .get("workspace")
            .and_then(toml::Value::as_table)
            .map(|w| WorkspaceSection {
                members: w
                    .get("members")
                    .and_then(toml::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .unwrap_or_default())
    }
}

/// `[dependencies]` 表中单条依赖（RFC 017 源码打包）。
///
/// 依赖唯一形态为本地 `path` 源码引用（对标 C# `ProjectReference`）：
/// 依赖源码合并进单一编译单元，全静态链接（见 RFC 017）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencySpec {
    /// path 依赖（如 `"../Arc"`，相对 `arc.toml` 所在目录）。
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSection {
    pub name: String,
    pub edition: String,
    /// 包版本（manifest 元数据；动态库场景嵌入产物供运行时版本校验）。
    ///
    /// 缺省 `"0.1.0"`。
    pub version: String,
    /// 包种类（RFC 017 D8 v1.0：`binary` / `library`）。
    ///
    /// - `binary`（缺省）：可执行文件
    /// - `library`：静态库产物
    /// - `library` + `dynamic = true`：动态库（RFC 017 D8 v1.0，
    ///   Windows `.dll` / Linux `.so` / macOS `.dylib`）
    ///
    /// RFC 017 D8 v1.0：删除 `plugin` 概念——框架无 plugin 实体，
    /// 动态库由 `library` + `dynamic` 组合表达。
    pub kind: String,
    /// 是否编译为动态库（RFC 017 D8 v1.0）。
    ///
    /// 仅当 `kind = "library"` 时生效；缺省 `false`。
    /// `true` 时产物为动态库（`.dll`/`.so`/`.dylib`），
    /// 通过 `rt_library_load` 加载、`rt_library_sym` 查找领域约定符号。
    pub dynamic: bool,
    /// 命名空间根（RFC 025 M1）：如 `Arc` / `Arc.Net` / `Arc.Orm.Sqlite`。
    ///
    /// 缺省时与 `name` 同步（保持 RFC 007 向后兼容）。
    pub namespace: String,
    /// RFC 003 M2：项目级全局导入路径列表（合成 `global using`）。
    ///
    /// 缺省空。每项为点分命名空间或类型路径（如 `"Arc"` / `"Arc.QIF"`）；
    /// 不含别名形式（别名仍用源码 `global using IO = Arc.IO;` 或 `GlobalUsings.as`）。
    pub global_usings: Vec<String>,
    /// RFC 025 M2+：InternalsVisibleTo（对标 C# `[assembly: InternalsVisibleTo]`）。
    ///
    /// 允许**指定包**访问本包的 `internal` 成员/类型；未列出包仍被 typeck 硬拒绝。
    /// 典型用途：
    /// - 测试程序（e2e / UnitTest）验证 internal 实现（C# 标准测试实践）
    /// - ARML 项目合并 std/UI/Core 框架源码时，框架文件与用户编译单元保持 internal 互通
    ///
    /// 缺省空（不向任何外部包开放 internal）。
    pub internals_visible_to: Vec<String>,
}

/// Parsed `[native]` section from `arc.toml`（RFC 016 M2）。
///
/// `ani-native-lib` 列表会被转换为链接器 `-L<DIR>` 标志，注入到 `-l<name>` 之前。
/// 缺省时为空列表——但合并层仍隐式注入主程序根目录（`merge_native_lib_paths`）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NativeSection {
    pub ani_native_lib: Vec<String>,
}

/// Parsed `[ui]` section from `arc.toml`（RFC 037 M2 ARML code-behind）。
///
/// 对标 WPF csproj 的 `<ApplicationDefinition>` + `<Page>` 项。声明 ARML
/// 项目的源文件清单，让 `arc build` 自动触发 codegen + 编译流程。
///
/// ```toml
/// [ui]
/// arml = ["App.arml", "MainWindow.arml"]
/// sources = ["App.arml.as", "MainWindow.arml.as"]
/// program = "Program.as"      # 入口文件（所有 Arc 项目统一标准）
/// namespace = "ArmlDemo"     # 可选，默认从 [package].namespace 推导
/// ```
///
/// ## 编译单元合并顺序（对标 WPF MSBuild 编译顺序）
///
/// 1. 头部：`namespace <ns>; using Arc;`
/// 2. 每个 `arml` 生成的 `.g.as`（partial class + InitializeComponent）
/// 3. 每个 `sources` 条目（用户 partial class 业务实现）
/// 4. `program` 指定的入口文件（含 `Main()` 函数）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UiSection {
    /// ARML 文件列表（按声明顺序处理，影响 `.g.as` 生成顺序）。
    pub arml: Vec<String>,
    /// 用户 partial class 源文件列表（`.arml.as`，合并到编译单元）。
    pub sources: Vec<String>,
    /// 程序入口文件（如 `Program.as`），含 `Main()` 函数。
    ///
    /// 所有 Arc 项目统一此标准——对标 WPF App.g.cs 自动生成的 Main 入口，
    /// 但 Arc 让用户显式控制入口文件，便于定制启动流程。合并到编译单元末尾，
    /// 确保 partial class 定义先于 Main 函数。
    pub program: Option<String>,
    /// 生成代码的 namespace（缺省时使用 `[package].namespace`）。
    pub namespace: Option<String>,
}

/// Parsed `[std]` section from `arc.toml`（RFC 031 §8）。
///
/// std 库路径覆盖（开发调试用）：覆盖编译器默认的 `workspace/std` 目录，
/// 指向本地 std 源码树以便开发调试。缺省时使用内置 `workspace/std`。
///
/// ```toml
/// [std]
/// path = "../ArcStd"   # 覆盖默认 std 目录（相对/绝对路径均可）
/// ```
///
/// 消费方一律经 [`resolve_effective_std_root`] 解析完整链；本纯函数仅处理
/// `[std].path` 覆盖与 `workspace/std` 兜底：相对路径相对 `arc.toml` 所在目录，
/// 并 canonicalize（与 `find_workspace_root` 路径比较口径一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdSection {
    /// std 库源码目录路径覆盖（相对 `arc.toml` 或绝对路径）。
    ///
    /// `[std]` 段给出时必填（fail-fast：缺省或空串即报错）。
    pub path: String,
}

/// 解析 std 库根目录：`[std].path` 覆盖或默认 `workspace/std`。
///
/// - `std` 给出时：相对路径相对 `manifest_dir`（`arc.toml` 所在目录）解析；
///   `manifest_dir` 缺失时退回相对 `workspace`。
/// - 否则：`workspace/std`。
/// - 结果尽量 canonicalize（失败则保留解析后路径）。
pub fn resolve_std_root(
    workspace: &Path,
    manifest_dir: Option<&Path>,
    std: Option<&StdSection>,
) -> PathBuf {
    let resolved = match std {
        Some(section) => {
            let p = Path::new(&section.path);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                let base = manifest_dir.unwrap_or(workspace);
                base.join(p)
            }
        }
        None => workspace.join("std"),
    };
    resolved.canonicalize().unwrap_or(resolved)
}

/// 完整 std 根解析链：`[std].path` → SDK 捆绑 std → `ARC_STD_ROOT` → `workspace/std`。
///
/// 产品消费方（build/lock/publish/core_arc 等）统一经此函数解析 std 根；纯函数
/// [`resolve_std_root`] 保留给无 SDK/环境依赖的调用（单元测试）。
///
/// - `[std].path`（项目显式覆盖）优先级最高；
/// - SDK 捆绑 std：安装态 `<sdk>/lib/std`，开发态仓库 `<repo>/std`
///   （[`codegen::sdk_layout::sdk_std_root`]）；
/// - `ARC_STD_ROOT` 环境变量（开发调试用）；
/// - 最后回退 `workspace/std`（现状默认行为）。
pub fn resolve_effective_std_root(
    workspace: &Path,
    manifest_dir: Option<&Path>,
    std: Option<&StdSection>,
) -> PathBuf {
    if std.is_some() {
        return resolve_std_root(workspace, manifest_dir, std);
    }
    if let Some(sdk_std) = codegen::sdk_layout::sdk_std_root() {
        if sdk_std.is_dir() {
            return sdk_std;
        }
    }
    if let Ok(env_root) = std::env::var("ARC_STD_ROOT") {
        let trimmed = env_root.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            return p.canonicalize().unwrap_or(p);
        }
    }
    resolve_std_root(workspace, None, None)
}

/// Parsed `[compiler]` section from `arc.toml`（RFC 005 里程碑④）。
///
/// 编译器行为旋钮。字段定义权威参考：`docs/rfc/031-compiler-cli.md`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerSection {
    /// 编译期声明级字段环 warning 策略：`"warn"`（默认，打印 `arc-cycle-001`
    /// 不阻断编译）| `"off"`（静默）。**无 `error` 档**（RFC 005 §2.6 / §5——
    /// 声明级环不必然泄漏，永不当 error）。
    pub field_cycle_policy: String,
}

impl Default for CompilerSection {
    fn default() -> Self {
        CompilerSection {
            field_cycle_policy: "warn".into(),
        }
    }
}

/// Parsed `[qif]` section from `arc.toml`（RFC 032 M1）。
///
/// 配置 QIF 测试执行行为。字段定义见 RFC 032 D11.3。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QifSection {
    /// 质检产物输出目录（默认 "obj/qif"）。
    pub output: String,
    /// 最大并行测试数（默认 1，Phase 2c 固定串行）。
    pub max_parallel: i32,
    /// 默认单测试超时毫秒（0 = 不限制）。
    pub default_timeout: i32,
    /// 报告格式：`human`（默认）| `json` | `junit`。
    pub output_format: String,
    /// 测试过滤 glob 模式（空 = 全部运行）。
    pub filter: String,
    /// 是否生成 `report.json`（RFC 032 §7 默认 true）。
    pub emit_json_report: bool,
    /// 是否持久化 `.arcqif` 运行文件（RFC 032 §7 默认 true）。
    pub persist_results: bool,
}

impl Default for QifSection {
    fn default() -> Self {
        QifSection {
            output: "obj/qif".into(),
            max_parallel: 1,
            default_timeout: 0,
            output_format: "human".into(),
            filter: String::new(),
            emit_json_report: true,
            persist_results: true,
        }
    }
}

impl ArcManifest {
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let source = std::fs::read_to_string(path).map_err(|source| ManifestError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let table: toml::Table = toml::from_str(&source).map_err(|e| ManifestError::Invalid {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let package = table.get("package").ok_or_else(|| ManifestError::Invalid {
            path: path.to_path_buf(),
            message: "missing [package] section".into(),
        })?;
        let package = package.as_table().ok_or_else(|| ManifestError::Invalid {
            path: path.to_path_buf(),
            message: "[package] must be a table".into(),
        })?;
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| ManifestError::Invalid {
                path: path.to_path_buf(),
                message: "[package].name is required".into(),
            })?
            .to_string();
        if name.is_empty() {
            return Err(ManifestError::Invalid {
                path: path.to_path_buf(),
                message: "[package].name must not be empty".into(),
            });
        }
        let edition = package
            .get("edition")
            .and_then(toml::Value::as_str)
            .unwrap_or("1")
            .to_string();
        let version = package
            .get("version")
            .and_then(toml::Value::as_str)
            .unwrap_or("0.1.0")
            .to_string();
        let kind = package
            .get("kind")
            .and_then(toml::Value::as_str)
            .unwrap_or("binary")
            .to_string();
        // RFC 017 D8 v1.0：仅 binary / library 两态；plugin 概念已移除，
        // 动态库由 kind="library" + dynamic=true 表达。非法值显式报错（fail-fast）。
        if kind != "binary" && kind != "library" {
            return Err(ManifestError::Invalid {
                path: path.to_path_buf(),
                message: format!(
                    "[package].kind = \"{kind}\" is not supported (expected `binary` or `library`); \
                     the `plugin` concept was removed in RFC 024 D8 v1.0 — express a dynamic \
                     library as `kind = \"library\"` + `dynamic = true`"
                ),
            });
        }
        // RFC 017 D8 v1.0: [package].dynamic 字段——仅 kind="library" 时生效。
        let dynamic = package
            .get("dynamic")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        let namespace = package
            .get("namespace")
            .and_then(toml::Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.clone());
        // RFC 003 M2：[package].global_usings — 点分路径列表，缺省空。
        let global_usings = package
            .get("global_usings")
            .and_then(toml::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        // RFC 025 M2+：[package].internals_visible_to — 允许访问本包 internal 的包名列表，缺省空。
        let internals_visible_to = package
            .get("internals_visible_to")
            .and_then(toml::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // RFC 016 M2：[native] 段，缺省空 ani-native-lib。
        let native = table
            .get("native")
            .and_then(toml::Value::as_table)
            .map(|n| NativeSection {
                ani_native_lib: n
                    .get("ani-native-lib")
                    .and_then(toml::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .unwrap_or_default();

        // RFC 037 M2：[ui] 段，ARML 项目描述。
        let ui = table.get("ui").and_then(toml::Value::as_table).map(|u| {
            let arml = u
                .get("arml")
                .and_then(toml::Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let sources = u
                .get("sources")
                .and_then(toml::Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let namespace = u
                .get("namespace")
                .and_then(toml::Value::as_str)
                .map(|s| s.to_string());
            let program = u
                .get("program")
                .and_then(toml::Value::as_str)
                .map(|s| s.to_string());
            UiSection {
                arml,
                sources,
                program,
                namespace,
            }
        });

        let dependencies = parse_dependencies_section(
            path,
            table.get("dependencies").and_then(toml::Value::as_table),
        )?;

        // RFC 005 里程碑④：`[compiler]` 段——字段环 warning 策略旋钮。
        // 缺省 `"warn"`（默认打印）；`"off"` 静默；无 `error` 档（CLI/校验层拒绝）。
        let compiler = table
            .get("compiler")
            .and_then(toml::Value::as_table)
            .map(|c| CompilerSection {
                field_cycle_policy: c
                    .get("field_cycle_policy")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("warn")
                    .to_string(),
            })
            .unwrap_or_default();

        // 解决方案 = workspace 聚合：`[workspace] members` 枚举成员项目。
        let workspace = table
            .get("workspace")
            .and_then(toml::Value::as_table)
            .map(|w| WorkspaceSection {
                members: w
                    .get("members")
                    .and_then(toml::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .unwrap_or_default();

        // RFC 031 §8：[std] 段——std 库路径覆盖（开发调试用）。缺省 None 使用内置 std。
        let std = parse_std_section(path, table.get("std").and_then(toml::Value::as_table))?;

        Ok(ArcManifest {
            package: PackageSection {
                name,
                edition,
                version,
                kind,
                dynamic,
                namespace,
                global_usings,
                internals_visible_to,
            },
            dependencies,
            native,
            ui,
            qif: parse_qif_section(table.get("qif").and_then(toml::Value::as_table)),
            compiler,
            workspace,
            std,
            path: path.to_path_buf(),
        })
    }
}

fn parse_dependencies_section(
    manifest_path: &Path,
    table: Option<&toml::Table>,
) -> Result<BTreeMap<String, DependencySpec>, ManifestError> {
    let Some(table) = table else {
        return Ok(BTreeMap::new());
    };
    let mut deps = BTreeMap::new();
    for (name, value) in table {
        let spec = match value {
            toml::Value::Table(t) => DependencySpec {
                path: t
                    .get("path")
                    .and_then(toml::Value::as_str)
                    .map(|s| s.to_string())
                    .ok_or_else(|| ManifestError::Invalid {
                        path: manifest_path.to_path_buf(),
                        message: format!(
                            "[dependencies].{name} must specify `path` (source-code packaging; see RFC 017)"
                        ),
                    })?,
            },
            _ => {
                return Err(ManifestError::Invalid {
                    path: manifest_path.to_path_buf(),
                    message: format!(
                        "[dependencies].{name} must be a table with `path` (source-code packaging; see RFC 017)"
                    ),
                });
            }
        };
        validate_dependency_spec(manifest_path, name, &spec)?;
        deps.insert(name.clone(), spec);
    }
    Ok(deps)
}

fn validate_dependency_spec(
    manifest_path: &Path,
    name: &str,
    spec: &DependencySpec,
) -> Result<(), ManifestError> {
    if spec.path.trim().is_empty() {
        return Err(ManifestError::Invalid {
            path: manifest_path.to_path_buf(),
            message: format!("[dependencies].{name} `path` must not be empty"),
        });
    }
    Ok(())
}

/// Walk upward from `start` (file or directory) to locate `arc.toml`.
pub fn find_arc_manifest(start: &Path) -> Option<(PathBuf, ArcManifest)> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let manifest_path = dir.join("arc.toml");
        if manifest_path.is_file() {
            return ArcManifest::load(&manifest_path)
                .ok()
                .map(|m| (dir.clone(), m));
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Walk upward from `start` and require `arc.toml`.
///
/// 对标 MSBuild 项目模型：没有 `.csproj` 的项目不被识别为可编译项目。
/// 同样，没有 `arc.toml` 的目录不被识别为 Arc 项目。
///
/// 调用方（`arc build` / `arc test` / `arc run` / `arc check` / `arc publish`）
/// 必须使用此函数，而非 `find_arc_manifest`。
pub fn require_arc_manifest(start: &Path) -> Result<(PathBuf, ArcManifest), String> {
    find_arc_manifest(start).ok_or_else(|| {
        format!(
            "no `arc.toml` found (searched upward from \"{}\")\n  \
             every Arc project must have an `arc.toml`; create one with:\n  \
             [package]\n  name = \"my_project\"\n  edition = \"1\"",
            start.display()
        )
    })
}

/// Parse `[qif]` section from arc.toml（RFC 032 M1）。
fn parse_qif_section(table: Option<&toml::Table>) -> QifSection {
    let Some(t) = table else {
        return QifSection::default();
    };
    QifSection {
        output: t
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or("obj/qif")
            .to_string(),
        max_parallel: t
            .get("max_parallel")
            .and_then(|v| v.as_integer())
            .unwrap_or(1) as i32,
        default_timeout: t
            .get("default_timeout")
            .and_then(|v| v.as_integer())
            .unwrap_or(0) as i32,
        output_format: t
            .get("output_format")
            .and_then(|v| v.as_str())
            .unwrap_or("human")
            .to_string(),
        filter: t
            .get("filter")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        emit_json_report: t
            .get("emit_json_report")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        persist_results: t
            .get("persist_results")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    }
}

/// Parse `[std]` section from arc.toml（RFC 031 §8）。
///
/// 缺省 `None`；给出 `[std]` 时 `path` 必填且非空（fail-fast），
/// 与 `[dependencies]` / `[package].kind` 的校验风格一致。
fn parse_std_section(
    manifest_path: &Path,
    table: Option<&toml::Table>,
) -> Result<Option<StdSection>, ManifestError> {
    let Some(t) = table else {
        return Ok(None);
    };
    let path = t
        .get("path")
        .and_then(toml::Value::as_str)
        .map(|s| s.to_string());
    let Some(path) = path else {
        return Err(ManifestError::Invalid {
            path: manifest_path.to_path_buf(),
            message: "[std].path is required when the [std] section is present".into(),
        });
    };
    if path.trim().is_empty() {
        return Err(ManifestError::Invalid {
            path: manifest_path.to_path_buf(),
            message: "[std].path must not be empty".into(),
        });
    }
    Ok(Some(StdSection { path }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_minimal_manifest() {
        let dir = std::env::temp_dir().join(format!("arc-manifest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "myapp"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(manifest.package.name, "myapp");
        assert_eq!(manifest.package.edition, "1");
    }

    #[test]
    fn parse_manifest_with_edition() {
        let dir = std::env::temp_dir().join(format!("arc-manifest-ed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "demo"
edition = "1"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(manifest.package.name, "demo");
        assert_eq!(manifest.package.edition, "1");
    }

    #[test]
    fn parse_dependencies_path_only() {
        let dir = std::env::temp_dir().join(format!("arc-manifest-deps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "App"
edition = "1"

[dependencies]
"Arc.Net" = { path = "../Net" }
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(manifest.dependencies["Arc.Net"].path, "../Net");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_dependencies_rejects_version_string() {
        let dir = std::env::temp_dir().join(format!("arc-manifest-ver-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "App"
edition = "1"

[dependencies]
"Arc.Security" = "0.1.0"
"#,
        )
        .unwrap();
        let err = ArcManifest::load(&dir.join("arc.toml")).unwrap_err();
        assert!(
            err.to_string().contains("must be a table with `path`"),
            "{err}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_dependencies_rejects_git() {
        let dir = std::env::temp_dir().join(format!("arc-manifest-git-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "App"
edition = "1"

[dependencies]
Remote = { git = "git+file:///tmp/fixture.git", tag = "v0.2.0" }
"#,
        )
        .unwrap();
        let err = ArcManifest::load(&dir.join("arc.toml")).unwrap_err();
        assert!(err.to_string().contains("must specify `path`"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_manifest_walks_up() {
        let root = std::env::temp_dir().join(format!("arc-find-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let nested = root.join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            root.join("arc.toml"),
            r#"
[package]
name = "nested"
"#,
        )
        .unwrap();
        let found = find_arc_manifest(&nested.join("main.as")).unwrap();
        assert_eq!(found.1.package.name, "nested");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_native_section_defaults_empty() {
        let dir = std::env::temp_dir().join(format!("arc-native-default-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert!(manifest.native.ani_native_lib.is_empty());
    }

    #[test]
    fn parse_native_section_ani_native_lib() {
        let dir = std::env::temp_dir().join(format!("arc-native-paths-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"

[native]
ani-native-lib = ["/usr/local/lib", "vendor/lib"]
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(
            manifest.native.ani_native_lib,
            vec!["/usr/local/lib", "vendor/lib"]
        );
    }

    #[test]
    fn parse_global_usings_defaults_empty() {
        let dir = std::env::temp_dir().join(format!("arc-gu-default-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert!(manifest.package.global_usings.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_global_usings_list() {
        let dir = std::env::temp_dir().join(format!("arc-gu-list-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"
global_usings = ["Arc", "Arc.QIF"]
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(
            manifest.package.global_usings,
            vec!["Arc".to_string(), "Arc.QIF".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_compiler_section_default_warn() {
        let dir = std::env::temp_dir().join(format!("arc-compiler-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(manifest.compiler.field_cycle_policy, "warn");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_workspace_section() {
        let dir = std::env::temp_dir().join(format!("arc-ws-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[workspace]
members = ["src/App", "src/Lib"]

[package]
name = "ws"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(
            manifest.workspace.members,
            vec!["src/App".to_string(), "src/Lib".to_string()]
        );
        assert!(manifest.workspace.is_solution());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_workspace_defaults_empty() {
        let dir = std::env::temp_dir().join(format!("arc-ws-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert!(manifest.workspace.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dependency_spec_is_path_only() {
        let spec = DependencySpec {
            path: "../Lib".into(),
        };
        assert_eq!(spec.path, "../Lib");
    }

    #[test]
    fn parse_compiler_section_field_cycle_policy() {
        let dir = std::env::temp_dir().join(format!("arc-compiler-off-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"

[compiler]
field_cycle_policy = "off"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(manifest.compiler.field_cycle_policy, "off");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_std_defaults_none() {
        let dir = std::env::temp_dir().join(format!("arc-std-default-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert!(manifest.std.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_std_section_path() {
        let dir = std::env::temp_dir().join(format!("arc-std-path-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"

[std]
path = "../ArcStd"
"#,
        )
        .unwrap();
        let manifest = ArcManifest::load(&dir.join("arc.toml")).unwrap();
        assert_eq!(manifest.std.as_ref().unwrap().path, "../ArcStd");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_std_section_requires_path() {
        let dir = std::env::temp_dir().join(format!("arc-std-nopath-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"

[std]
"#,
        )
        .unwrap();
        let err = ArcManifest::load(&dir.join("arc.toml")).unwrap_err();
        assert!(err.to_string().contains("[std].path"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_std_section_rejects_empty_path() {
        let dir = std::env::temp_dir().join(format!("arc-std-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "app"

[std]
path = "   "
"#,
        )
        .unwrap();
        let err = ArcManifest::load(&dir.join("arc.toml")).unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_std_root_defaults_to_workspace_std() {
        let ws = std::env::temp_dir().join(format!("arc-std-res-def-{}", std::process::id()));
        let _ = fs::remove_dir_all(&ws);
        fs::create_dir_all(ws.join("std")).unwrap();
        let got = resolve_std_root(&ws, None, None);
        let expect = ws.join("std").canonicalize().unwrap();
        assert_eq!(got, expect);
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn resolve_std_root_relative_to_manifest_dir() {
        let root = std::env::temp_dir().join(format!("arc-std-res-rel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let app = root.join("app");
        let alt = root.join("AltStd");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&alt).unwrap();
        let section = StdSection {
            path: "../AltStd".into(),
        };
        let got = resolve_std_root(&root, Some(&app), Some(&section));
        let expect = alt.canonicalize().unwrap();
        assert_eq!(got, expect);
        let _ = fs::remove_dir_all(&root);
    }
}
