//! 项目脚手架与项目识别（RFC 043 场景 1.2 B 面落地）。
//!
//! - [`detect_project`]：识别项目类型（未初始化 / Arc 项目 / Coding Harness / 领域二），
//!   供 Harness 消费（CLI `arc detect <dir>`，`--format json` 机器可读）。
//! - [`scaffold_project`]：生成最小可编译项目骨架（`arc.toml` + `Program.as` + 可选
//!   `README.md`）；`--agent` 追加 `Arc.Agent` + `Arc.Agent.Harness` 依赖（对齐 Coding
//!   Harness 三包消费：`Arc` + `Arc.Agent` + `Arc.Agent.Harness`）并落 `.arcagent/conventions.md`
//!   初始模板（模板内嵌分发，单一权威源 [`CONVENTIONS_TEMPLATE`]）。

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::manifest::ArcManifest;

/// 项目类型（`arc detect` / DetectProject 分类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectType {
    /// 无 `arc.toml` → 未初始化（提示 `arc new`）。
    Uninitialized,
    /// 基础 Arc 项目（有 `arc.toml`，无 Harness 依赖）。
    ArcProject,
    /// Coding Harness 项目（依赖含 `Arc.Agent.Harness.Coding`）。
    CodingHarness,
    /// 领域二（依赖含 `Arc.Agent.Harness` 且不含 Coding，如 ReviewAgent）。
    DomainTwo,
}

impl ProjectType {
    /// 机器可读标识（JSON / 协议面）。
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectType::Uninitialized => "uninitialized",
            ProjectType::ArcProject => "arc_project",
            ProjectType::CodingHarness => "coding_harness",
            ProjectType::DomainTwo => "domain_two",
        }
    }

    /// 人读描述（`arc detect` human 输出）。
    pub fn label(&self) -> &'static str {
        match self {
            ProjectType::Uninitialized => "uninitialized (no arc.toml; run `arc new <dir>`)",
            ProjectType::ArcProject => "arc project",
            ProjectType::CodingHarness => "coding harness project",
            ProjectType::DomainTwo => "domain two project (harness, no coding)",
        }
    }
}

/// 项目识别结果（CLI `arc detect` 输出；JSON 机器可消费）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectInfo {
    pub root: PathBuf,
    pub kind: ProjectType,
    /// 有 `arc.toml` 时的 `[package].name`（否则空）。
    pub name: String,
    /// 有 `arc.toml` 时的 `[package].namespace`（否则空）。
    pub namespace: String,
    /// `.arcagent/conventions.md` 是否存在（Rules 注入面）。
    pub has_conventions: bool,
}

impl ProjectInfo {
    pub fn human_summary(&self) -> String {
        let mut out = format!("{}: {}", self.kind.label(), self.root.display());
        if self.kind != ProjectType::Uninitialized {
            out.push_str(&format!(
                " (name={}, namespace={}, conventions={})",
                self.name,
                self.namespace,
                if self.has_conventions { "yes" } else { "no" }
            ));
        } else if self.has_conventions {
            out.push_str(" (.arcagent/conventions.md present, but no arc.toml)");
        }
        out
    }
}

/// 识别项目类型。
///
/// 判定规则（场景 1.2 / scenario-operation）：
/// - 无 `arc.toml` → [`ProjectType::Uninitialized`]。
/// - `arc.toml` 依赖含 `Arc.Agent.Harness.Coding` → Coding Harness 项目。
/// - 依赖含 `Arc.Agent.Harness` 且不含 Coding → 领域二（ReviewAgent 型）。
/// - 其余 → 基础 Arc 项目。
/// - `.arcagent/conventions.md` 存在与否作为独立的 Rules 注入面标志。
pub fn detect_project(root: &Path) -> ProjectInfo {
    let has_conventions = root.join(".arcagent").join("conventions.md").is_file();
    let toml = root.join("arc.toml");
    if !toml.is_file() {
        return ProjectInfo {
            root: root.to_path_buf(),
            kind: ProjectType::Uninitialized,
            name: String::new(),
            namespace: String::new(),
            has_conventions,
        };
    }
    match ArcManifest::load(&toml) {
        Ok(m) => {
            let has_coding = m.dependencies.contains_key("Arc.Agent.Harness.Coding");
            let has_harness = m.dependencies.contains_key("Arc.Agent.Harness");
            let kind = if has_coding {
                ProjectType::CodingHarness
            } else if has_harness {
                ProjectType::DomainTwo
            } else {
                ProjectType::ArcProject
            };
            ProjectInfo {
                root: root.to_path_buf(),
                kind,
                name: m.package.name,
                namespace: m.package.namespace,
                has_conventions,
            }
        }
        // arc.toml 损坏：按基础 Arc 项目诚实返回（name/namespace 空缺），不冒充其他类型。
        Err(_) => ProjectInfo {
            root: root.to_path_buf(),
            kind: ProjectType::ArcProject,
            name: String::new(),
            namespace: String::new(),
            has_conventions,
        },
    }
}

/// `arc detect` human 输出（多行摘要）。
pub fn format_detect_human(infos: &[ProjectInfo]) -> String {
    infos
        .iter()
        .map(|i| i.human_summary())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `arc new` 选项。
#[derive(Debug, Clone)]
pub struct ScaffoldOptions {
    /// 包名（缺省取目录名）。
    pub name: Option<String>,
    /// 追加 Agent 依赖（`Arc.Agent` + `Arc.Agent.Harness`）+ 落 conventions 模板。
    pub agent: bool,
    /// 生成 `README.md`（缺省 true）。
    pub readme: bool,
}

impl Default for ScaffoldOptions {
    fn default() -> Self {
        ScaffoldOptions {
            name: None,
            agent: false,
            readme: true,
        }
    }
}

/// 脚手架生成报告（供 CLI 打印 + e2e 断言）。
#[derive(Debug, Clone)]
pub struct ScaffoldReport {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub program: PathBuf,
    pub readme: Option<PathBuf>,
    pub conventions: Option<PathBuf>,
    /// 已写入 `[dependencies]` 的包名。
    pub included_deps: Vec<String>,
    /// 因 std 根不可达（跨盘 / 目录缺失）而省略的依赖。
    pub skipped_deps: Vec<String>,
}

impl ScaffoldReport {
    pub fn human_summary(&self) -> String {
        let mut out = format!(
            "created project '{}' at {}",
            self.manifest_name(),
            self.root.display()
        );
        out.push_str(&format!("\n  manifest: {}", self.manifest.display()));
        out.push_str(&format!("\n  program:  {}", self.program.display()));
        if let Some(r) = &self.readme {
            out.push_str(&format!("\n  readme:   {}", r.display()));
        }
        if let Some(c) = &self.conventions {
            out.push_str(&format!("\n  conventions: {}", c.display()));
        }
        if !self.included_deps.is_empty() {
            out.push_str(&format!(
                "\n  dependencies: {}",
                self.included_deps.join(", ")
            ));
        }
        if !self.skipped_deps.is_empty() {
            out.push_str(&format!(
                "\n  (skipped unresolvable deps: {})",
                self.skipped_deps.join(", ")
            ));
        }
        out
    }

    fn manifest_name(&self) -> String {
        // manifest 已生成：直接读 `[package].name`；读失败退回目录名。
        ArcManifest::load(&self.manifest)
            .map(|m| m.package.name)
            .unwrap_or_else(|_| {
                self.root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("app")
                    .to_string()
            })
    }
}

/// 生成最小可编译项目骨架（场景 1.2 B 面：空目录 → `arc new` → D0 build 绿）。
///
/// 产物：`arc.toml`（name / namespace + `Arc` 依赖；`--agent` 追加 `Arc.Agent` +
/// `Arc.Agent.Harness`）+ `Program.as`（`void Main()` 最小入口）+ 可选 `README.md`；
/// `--agent` 时落 conventions 初始模板到 `.arcagent/conventions.md`（[`CONVENTIONS_TEMPLATE`]
/// 经 `include_str!` 内嵌，脚手架不依赖 std 源码树）。
///
/// 依赖路径从目标目录到 std 根相对计算；std 根不可达（不同盘 / 目录缺失）时省略该
/// 依赖并记入 `skipped_deps`（骨架仍可编译——`Arc` 由编译器经 std 根隐式解析）。
pub fn scaffold_project(target: &Path, opts: &ScaffoldOptions) -> Result<ScaffoldReport, String> {
    let root = target.to_path_buf();
    if root.join("arc.toml").exists() {
        return Err(format!(
            "refusing to scaffold: {} already contains arc.toml",
            root.display()
        ));
    }
    std::fs::create_dir_all(&root).map_err(|e| format!("create {}: {e}", root.display()))?;

    let dir_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("app")
        .to_string();
    let raw_name = opts.name.clone().unwrap_or(dir_name);
    let pkg_name = sanitize_package_name(&raw_name)
        .ok_or_else(|| format!("cannot derive package name from `{raw_name}`"))?;
    let namespace = sanitize_namespace(&raw_name)
        .ok_or_else(|| format!("cannot derive namespace from `{raw_name}`"))?;

    // 依赖：默认 `Arc`；`--agent` 追加 `Arc.Agent` + `Arc.Agent.Harness`（三包消费）。
    let mut candidates: Vec<(&str, &str)> = vec![("Arc", "Arc")];
    if opts.agent {
        candidates.push(("Arc.Agent", "AI/Agent"));
        candidates.push(("Arc.Agent.Harness", "AI/Agent.Harness"));
    }
    let ws = crate::loader::find_workspace_root(&root);
    let std_root = crate::manifest::resolve_std_root(&ws, None, None);

    let mut deps: Vec<(String, PathBuf)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (pkg, rel) in candidates {
        let dir = std_root.join(rel);
        match relative_path(&root, &dir) {
            Some(rel_path) if dir.is_dir() => deps.push((pkg.to_string(), rel_path)),
            _ => skipped.push(pkg.to_string()),
        }
    }

    // arc.toml
    let manifest_path = root.join("arc.toml");
    let mut toml =
        format!("[package]\nname = \"{pkg_name}\"\nedition = \"1\"\nnamespace = \"{namespace}\"\n");
    if !deps.is_empty() {
        toml.push_str("\n[dependencies]\n");
        for (pkg, rel) in &deps {
            let rel_toml = rel.to_string_lossy().replace('\\', "/");
            toml.push_str(&format!("\"{pkg}\" = {{ path = \"{rel_toml}\" }}\n"));
        }
    }
    std::fs::write(&manifest_path, toml)
        .map_err(|e| format!("write {}: {e}", manifest_path.display()))?;

    // Program.as
    let program_path = root.join("Program.as");
    let program = format!(
        "// {pkg_name} 项目入口（`arc new` 脚手架生成的最小可编译骨架）。\n\
         using Arc;\n\n\
         void Main() {{\n\
         \x20   Console.WriteLine(\"Hello from {pkg_name}!\");\n\
         }}\n"
    );
    std::fs::write(&program_path, program)
        .map_err(|e| format!("write {}: {e}", program_path.display()))?;

    // README.md（可选）
    let readme_path = if opts.readme {
        let path = root.join("README.md");
        let readme = format!(
            "# {pkg_name}\n\n\
             Arc 项目骨架（`arc new` 生成）。\n\n\
             ## 结构\n\n\
             - `arc.toml` — 包清单（name / namespace / 依赖）\n\
             - `Program.as` — 入口（`void Main()`）\n\n\
             ## 构建\n\n\
             ```bash\narc build .\n```\n"
        );
        std::fs::write(&path, readme).map_err(|e| format!("write {}: {e}", path.display()))?;
        Some(path)
    } else {
        None
    };

    // conventions 初始模板（内嵌分发：编译期打包进 arc 二进制，无 std 源码树环境同样可用）
    let conventions_path = if opts.agent {
        let path = root.join(".arcagent").join("conventions.md");
        let text = CONVENTIONS_TEMPLATE;
        std::fs::create_dir_all(path.parent().unwrap_or(&root))
            .map_err(|e| format!("create .arcagent: {e}"))?;
        std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        Some(path)
    } else {
        None
    };

    Ok(ScaffoldReport {
        root,
        manifest: manifest_path,
        program: program_path,
        readme: readme_path,
        conventions: conventions_path,
        included_deps: deps.into_iter().map(|(n, _)| n).collect(),
        skipped_deps: skipped,
    })
}

/// conventions 初始模板（单一权威源 `templates/conventions.agent.md`）。
///
/// 为什么内嵌分发而非运行时读 std 源码树：arc 以单体二进制分发，脚手架在无源码树的
/// 安装环境（仅 CLI / std 根跨盘不可达）同样必须可用；`include_str!` 附带编译期存在性
/// 校验——模板缺失即编译失败，而非运行时静默回退。对标 `dotnet new` 模板归 SDK 分发、
/// 不随类库包携带的惯例（非代码资产归编译器 crate，同 `crates/arc/native/` 先例）。
const CONVENTIONS_TEMPLATE: &str = include_str!("../templates/conventions.agent.md");

/// 计算 `to` 相对 `from`（目录）的相对路径；跨卷（Windows 不同盘符）返回 `None`。
fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    let from = from.canonicalize().ok()?;
    let to = to.canonicalize().ok()?;
    let from_parts: Vec<OsString> = from
        .components()
        .map(|c| c.as_os_str().to_owned())
        .collect();
    let to_parts: Vec<OsString> = to.components().map(|c| c.as_os_str().to_owned()).collect();
    // 前缀（Windows 盘符）不同 → 无法相对
    let from_prefix = from.components().next();
    let to_prefix = to.components().next();
    match (from_prefix, to_prefix) {
        (Some(Component::Prefix(p1)), Some(Component::Prefix(p2))) if p1 != p2 => return None,
        _ => {}
    }
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = PathBuf::new();
    for _ in common..from_parts.len() {
        out.push("..");
    }
    for p in &to_parts[common..] {
        out.push(p);
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    Some(out)
}

/// 包名清洗：仅保留 `[A-Za-z0-9._-]`，其余替换为 `-`；空结果返回 `None`。
fn sanitize_package_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == '-' || c == '.').to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// 命名空间清洗：`-` / `_` / `.` 分段，逐段首字母大写后拼接（`my-app` → `MyApp`）。
fn sanitize_namespace(raw: &str) -> Option<String> {
    let mut out = String::new();
    for part in raw.split(['-', '_', '.']) {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_sanitization() {
        assert_eq!(sanitize_namespace("my-app").as_deref(), Some("MyApp"));
        assert_eq!(sanitize_namespace("my_app").as_deref(), Some("MyApp"));
        assert_eq!(sanitize_namespace("MyApp").as_deref(), Some("MyApp"));
        assert_eq!(
            sanitize_namespace("arc.demo.proj").as_deref(),
            Some("ArcDemoProj")
        );
        assert_eq!(sanitize_namespace("---"), None);
    }

    #[test]
    fn package_name_sanitization() {
        assert_eq!(sanitize_package_name("my app").as_deref(), Some("my-app"));
        assert_eq!(sanitize_package_name("my-app").as_deref(), Some("my-app"));
        assert_eq!(
            sanitize_package_name("app/evil").as_deref(),
            Some("app-evil")
        );
        assert_eq!(sanitize_package_name(".."), None);
    }

    #[test]
    fn relative_path_within_same_root() {
        let base = std::env::temp_dir().join(format!("arc-relpath-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let from = base.join("target/e2e/proj");
        let to = base.join("std/Arc");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        let rel = relative_path(&from, &to).expect("relative");
        let expected: Vec<_> = ["..", "..", "..", "std", "Arc"]
            .into_iter()
            .map(std::ffi::OsStr::new)
            .collect();
        let actual: Vec<_> = rel.components().map(|c| c.as_os_str()).collect();
        assert_eq!(actual, expected);
        let _ = std::fs::remove_dir_all(&base);
    }
}
