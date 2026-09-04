//! Project 体系（RFC 009 阶段 3）：`arc.toml` 建模的包单元与依赖图加载。
//!
//! 提供 [`Project`]（包元数据）、[`parse_arc_toml`]（`arc.toml` 解析）与
//! [`locate_arcgr`]（`.arcgr` 约定路径定位）。主包 + 依赖包沿依赖图自动加载，
//! 取代手动 `load_dependency_package`；并把主包 `.arcgr` 携带的
//! `ContextManifest` L0 信息并入 `Project`，供精确 FQN 解析与依赖路由。

use std::path::{Path, PathBuf};

use arcgr::context_manifest::{L0ProjectOverview, ProjectKind};

/// 依赖来源——`arc.toml` 中 `[[dependencies]]` 的 `source` 键。
///
/// 依赖唯一形态为本地 `path` 源码引用，故仅保留 [`Path`](DependencySource::Path)
/// 一种取值（aopkg 包引用已移除，依赖统一收敛为 path-only）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DependencySource {
    #[default]
    Path,
}

/// 依赖引用——名称 + 来源 + 定位信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyRef {
    pub name: String,
    /// 路径依赖的根路径（相对项目根）。
    pub path: PathBuf,
    pub source: DependencySource,
}

impl DependencyRef {
    pub fn path(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            source: DependencySource::Path,
        }
    }
}

/// 一个项目单元（`arc.toml` 建模的包）。
#[derive(Debug, Clone, Default)]
pub struct Project {
    pub name: String,
    pub kind: ProjectKind,
    pub root: PathBuf,
    pub dependencies: Vec<DependencyRef>,
    /// 主包 `.arcgr` 携带的 `ContextManifest` L0（若存在）。
    pub manifest: Option<L0ProjectOverview>,
}

/// `arc.toml` 解析的中间结果（未绑定 root）。
#[derive(Debug, Clone, Default)]
pub struct ProjectConfig {
    pub name: String,
    pub kind: ProjectKind,
    pub dependencies: Vec<DependencyRef>,
}

/// 解析 `arc.toml` 内容（RFC 009 Project 体系的简化 TOML 子集）。
///
/// 支持：
/// ```toml
/// [package]
/// name = "myapp"
/// kind = "executable"     # executable | library | dynamic-library | test
///
/// [[dependencies]]          # 数组形式
/// name = "foo"
/// path = "vendor/foo"       # 路径依赖
///
/// [dependencies.baz]        # 表形式：以表名作依赖名
/// path = "vendor/baz"
/// ```
pub fn parse_arc_toml(contents: &str) -> Result<ProjectConfig, String> {
    let mut cfg = ProjectConfig::default();
    // 当前段落：(tag, 表依赖名) —— ("package", None) / ("dep", Some(表名))
    let mut section: (&str, Option<String>) = ("", None);
    // 待完成依赖条目（数组与表形式共用）
    let mut pending: Option<DependencyRef> = None;

    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            // 段落头：[[dependencies]] / [dependencies.<name>] / [package]
            let inner = rest.trim_end_matches(']').trim();
            if inner.starts_with('[') {
                flush_dep(&mut cfg.dependencies, &mut pending);
                if inner.trim_start_matches('[').trim() == "dependencies" {
                    section = ("dep", None);
                    pending = Some(DependencyRef {
                        name: String::new(),
                        path: PathBuf::new(),
                        source: DependencySource::Path,
                    });
                } else {
                    section = ("", None);
                }
            } else if inner == "package" {
                flush_dep(&mut cfg.dependencies, &mut pending);
                section = ("package", None);
            } else if let Some(dep) = inner.strip_prefix("dependencies.") {
                flush_dep(&mut cfg.dependencies, &mut pending);
                section = ("dep", Some(dep.trim().to_string()));
                pending = Some(DependencyRef {
                    name: dep.trim().to_string(),
                    path: PathBuf::new(),
                    source: DependencySource::Path,
                });
            } else {
                flush_dep(&mut cfg.dependencies, &mut pending);
                section = ("", None);
            }
            continue;
        }
        // `key = value`
        let Some(eq) = line.find('=') else { continue };
        let key = line[..eq].trim().to_string();
        let value = unquote(line[eq + 1..].trim());
        match section.0 {
            "package" => match key.as_str() {
                "name" => cfg.name = value,
                "kind" => cfg.kind = parse_kind(&value)?,
                _ => {}
            },
            "dep" => {
                let entry = pending.get_or_insert_with(|| DependencyRef {
                    name: section.1.clone().unwrap_or_default(),
                    path: PathBuf::new(),
                    source: DependencySource::Path,
                });
                match key.as_str() {
                    "name" => entry.name = value,
                    "path" => entry.path = PathBuf::from(value),
                    "source" => entry.source = parse_source(&value)?,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    flush_dep(&mut cfg.dependencies, &mut pending);
    Ok(cfg)
}

/// 按约定路径定位本地路径依赖的 `.arcgr`。
///
/// - 主包（`dep_path=None`）：`<root>/<name>.arcgr`、`<root>/target/<name>.arcgr`
/// - 依赖包：`<root>/<dep_path>/<name>.arcgr`、`<root>/<dep_path>/target/<name>.arcgr`
pub fn locate_arcgr(root: &Path, name: &str, dep_path: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dp) = dep_path {
        let base = root.join(dp);
        candidates.push(base.join(format!("{name}.arcgr")));
        candidates.push(base.join("target").join(format!("{name}.arcgr")));
    } else {
        candidates.push(root.join(format!("{name}.arcgr")));
        candidates.push(root.join("target").join(format!("{name}.arcgr")));
    }
    candidates.into_iter().find(|p| p.exists())
}

/// 把待完成的依赖条目并入依赖列表。
fn flush_dep(deps: &mut Vec<DependencyRef>, pending: &mut Option<DependencyRef>) {
    if let Some(dep) = pending.take() {
        deps.push(dep);
    }
}

/// 去掉值两侧引号（`"value"` → `value`）。
fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// 解析 `kind` 字符串为 [`ProjectKind`]。
fn parse_kind(value: &str) -> Result<ProjectKind, String> {
    Ok(match value {
        "executable" => ProjectKind::Executable,
        "library" => ProjectKind::Library,
        "dynamic-library" | "dynamic" => ProjectKind::DynamicLibrary,
        "test" => ProjectKind::Test,
        other => return Err(format!("unknown arc.toml kind: {other}")),
    })
}

/// 解析 `source` 字符串为 [`DependencySource`]。
fn parse_source(value: &str) -> Result<DependencySource, String> {
    Ok(match value {
        "path" => DependencySource::Path,
        other => return Err(format!("unknown dependency source: {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcgr::context_manifest::ProjectKind;

    #[test]
    fn parse_arc_toml_table_form() {
        let cfg = parse_arc_toml(
            r#"
            [package]
            name = "myapp"
            kind = "library"

            [dependencies.foo]
            path = "vendor/foo"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.name, "myapp");
        assert_eq!(cfg.kind, ProjectKind::Library);
        assert_eq!(cfg.dependencies.len(), 1);
        assert_eq!(cfg.dependencies[0].name, "foo");
        assert_eq!(cfg.dependencies[0].path, PathBuf::from("vendor/foo"));
    }

    #[test]
    fn parse_arc_toml_array_form() {
        let cfg = parse_arc_toml(
            r#"
            [package]
            name = "app"
            kind = "executable"

            [[dependencies]]
            name = "foo"
            path = "vendor/foo"

            [[dependencies]]
            name = "bar"
            path = "vendor/bar"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.kind, ProjectKind::Executable);
        assert_eq!(cfg.dependencies.len(), 2);
        assert_eq!(cfg.dependencies[0].name, "foo");
        assert_eq!(cfg.dependencies[1].name, "bar");
        assert_eq!(cfg.dependencies[1].path, PathBuf::from("vendor/bar"));
    }

    #[test]
    fn parse_arc_toml_ignores_comments_and_unknown_sections() {
        let cfg = parse_arc_toml(
            r#"
            # 顶部注释
            [package]
            name = "x"
            kind = "test"

            [profile.release]
            opt = 3
            "#,
        )
        .unwrap();
        assert_eq!(cfg.name, "x");
        assert_eq!(cfg.kind, ProjectKind::Test);
        assert!(cfg.dependencies.is_empty());
    }

    #[test]
    fn parse_arc_toml_rejects_unknown_kind() {
        let err = parse_arc_toml("[package]\nname = \"x\"\nkind = \"weird\"\n").unwrap_err();
        assert!(err.contains("weird"));
    }

    #[test]
    fn parse_arc_toml_path_references() {
        let cfg = parse_arc_toml(
            r#"
            [package]
            name = "app"
            kind = "executable"

            [[dependencies]]
            name = "foo"
            path = "vendor/foo"

            [dependencies.baz]
            path = "vendor/baz"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.dependencies.len(), 2);
        // 数组形式路径依赖
        assert_eq!(cfg.dependencies[0].name, "foo");
        assert_eq!(cfg.dependencies[0].source, DependencySource::Path);
        assert_eq!(cfg.dependencies[0].path, PathBuf::from("vendor/foo"));
        // 表形式路径依赖
        assert_eq!(cfg.dependencies[1].name, "baz");
        assert_eq!(cfg.dependencies[1].source, DependencySource::Path);
        assert_eq!(cfg.dependencies[1].path, PathBuf::from("vendor/baz"));
    }

    #[test]
    fn parse_arc_toml_rejects_unknown_source() {
        let err = parse_arc_toml(
            "[package]\nname = \"x\"\n\n[[dependencies]]\nname = \"a\"\nsource = \"git\"\n",
        )
        .unwrap_err();
        assert!(err.contains("git"));
    }
}
