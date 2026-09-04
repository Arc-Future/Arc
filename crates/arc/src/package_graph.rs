//! RFC 025 M2 / B4：子库 `arc.toml` 包图。
//!
//! 扫描 `std/` 下所有 `arc.toml`（解决方案目录下的子库，如 `std/Net/Core/`、
//! `std/Orm/SQLite/`），构建包名 → 目录 / 命名空间根 / 直接依赖的图；
//! 并为源文件解析所属包（最近 `arc.toml`）。
//!
//! 传递依赖闭包（本切片）：从入口 `[dependencies]` 根出发 BFS，得到允许
//! `using` 的包集合；图中缺失边 → 硬错误。
//!
//! B4 布局门禁：已知 std 包名必须落在 RFC 025 约定的解决方案目录
//! （`std/Net/Core` / `std/Net/Grpc` / `std/Orm/SQLite` / `std/UI/Core` 等），禁止
//! 平级 `std/Net.Grpc/` 等与文档双真相。
//!
//! 本模块**不**实现 RFC 017 完整 M4（缓存 / `arc.lock` / MVS）；仅提供
//! typeck 跨包 `internal`、显式依赖门禁与传递闭包所需的最小包身份。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::manifest::{ArcManifest, DependencySpec};

/// RFC 025 可治理布局：包名 → `std/<rel>/`（解决方案目录 + 子库，如 `Net/Core`）。
fn expected_std_rel(package_name: &str) -> Option<&'static str> {
    match package_name {
        "Arc" => Some("Arc"),
        "Arc.Net" => Some("Net/Core"),
        "Arc.Net.P2P" => Some("Net/P2P"),
        "Arc.Net.Grpc" => Some("Net/Grpc"),
        "Arc.Security" => Some("Security"),
        "Arc.Data" => Some("Data"),
        "Arc.Orm" => Some("Orm/Core"),
        "Arc.Orm.SQLite" => Some("Orm/SQLite"),
        "Arc.Orm.PostgreSQL" => Some("Orm/PostgreSQL"),
        "Arc.Orm.Mongo" => Some("Orm/MongoDB"),
        "Arc.DI" => Some("DI"),
        // UI 域解决方案（std/UI/arc.toml 聚合）：Core 为核心库，其余为独立组件库
        "Arc.UI" => Some("UI/Core"),
        "Arc.UI.Edit" => Some("UI/Edit"),
        "Arc.UI.Md" => Some("UI/Md"),
        "Arc.UI.WebView" => Some("UI/WebView"),
        "Arc.UI.WebWindow" => Some("UI/WebWindow"),
        "Arc.UI.Simulator" => Some("UI/Simulator"),
        "Arc.QIF" => Some("QIF"),
        _ => None,
    }
}

/// 纯 `[workspace]` 清单（std 解决方案根，对标独立 `.sln`）：无 `[package]`，
/// 不声明包成员，包图扫描须跳过（子目录照常递归发现各子库清单）。
fn is_workspace_only_manifest(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|s| {
            let has_workspace = s.lines().any(|l| l.trim().starts_with("[workspace]"));
            let has_package = s.lines().any(|l| l.trim().starts_with("[package]"));
            has_workspace && !has_package
        })
        .unwrap_or(false)
}

/// 一个已发现的包节点（std 子库或用户项目）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageNode {
    pub name: String,
    pub namespace: String,
    pub dir: PathBuf,
    pub dependencies: BTreeMap<String, DependencySpec>,
    /// RFC 025 M2+：允许访问本包 `internal` 的包名列表（对标 C# `InternalsVisibleTo`）。
    pub internals_visible_to: Vec<String>,
}

/// std 子库包图（按包名索引）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageGraph {
    pub packages: BTreeMap<String, PackageNode>,
}

/// 传递依赖闭包解析失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureError {
    /// 根包名不在包图中（声明了未知包）。
    UnknownRoot { package: String },
    /// 某包的 `[dependencies]` 指向图中不存在的包。
    MissingEdge { from: String, missing: String },
}

impl std::fmt::Display for ClosureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClosureError::UnknownRoot { package } => write!(
                f,
                "declared dependency `{package}` is not in the package graph \
                 (unknown std/path package name; check spelling or path)"
            ),
            ClosureError::MissingEdge { from, missing } => write!(
                f,
                "package `{from}` depends on `{missing}`, but `{missing}` is not in the package graph"
            ),
        }
    }
}

impl std::error::Error for ClosureError {}

impl PackageGraph {
    /// 递归扫描 `workspace/std` 下所有 `arc.toml`，构建包图并校验 RFC 025 布局。
    pub fn discover_std(workspace: &Path) -> Result<Self, String> {
        Self::discover_std_at(&workspace.join("std"))
    }

    /// 递归扫描给定 std 根目录下所有 `arc.toml`（供 `[std].path` 覆盖）。
    pub fn discover_std_at(std_root: &Path) -> Result<Self, String> {
        let mut packages = BTreeMap::new();
        if !std_root.is_dir() {
            return Ok(Self { packages });
        }
        let mut stack = vec![std_root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let manifest_path = dir.join("arc.toml");
            if manifest_path.is_file() {
                match ArcManifest::load(&manifest_path) {
                    Ok(manifest) => {
                        let name = manifest.package.name.clone();
                        if packages.contains_key(&name) {
                            return Err(format!(
                                "duplicate std package name `{name}` at {} and {}",
                                packages[&name].dir.display(),
                                dir.display()
                            ));
                        }
                        packages.insert(
                            name.clone(),
                            PackageNode {
                                name,
                                namespace: manifest.package.namespace.clone(),
                                dir: dir.clone(),
                                dependencies: manifest.dependencies.clone(),
                                internals_visible_to: manifest.package.internals_visible_to.clone(),
                            },
                        );
                    }
                    Err(e) => {
                        // std 解决方案根清单（纯 [workspace]，无 [package]）不是包
                        // 成员——跳过并继续扫描子目录；其余无效清单仍为硬错误。
                        if !is_workspace_only_manifest(&manifest_path) {
                            return Err(e.to_string());
                        }
                    }
                }
            }
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if name == "obj" || name == "bin" || name == "target" || name == ".git" {
                            continue;
                        }
                        stack.push(path);
                    }
                }
            }
        }
        let graph = Self { packages };
        graph.validate_layout(std_root)?;
        Ok(graph)
    }

    /// RFC 039 §1.7.1 P1：便捷装配 = `discover_std` + 入口 path 依赖吸收。
    ///
    /// 使用户工作区成员（path 依赖包）进入包图，`allowed_for_entry` 不再静默跳过，
    /// 跨库端点 / `[Inject]` 聚合得以跨包 join。
    pub fn discover(
        workspace: &Path,
        project_root: &Path,
        entry_deps: &BTreeMap<String, DependencySpec>,
    ) -> Result<Self, String> {
        Self::discover_with_std(workspace, project_root, entry_deps, None)
    }

    /// 同 [`discover`]，可显式传入 std 根（`[std].path` 解析结果）。
    pub fn discover_with_std(
        workspace: &Path,
        project_root: &Path,
        entry_deps: &BTreeMap<String, DependencySpec>,
        std_root: Option<&Path>,
    ) -> Result<Self, String> {
        let default_std = workspace.join("std");
        let root = std_root.unwrap_or(&default_std);
        let mut graph = Self::discover_std_at(root)?;
        graph.absorb_path_dependencies(project_root, entry_deps)?;
        Ok(graph)
    }

    /// 将入口（及递归）path 依赖吸收进包图，供跨库聚合 / 传递闭包使用。
    ///
    /// - 已在图中的包（std 子库 / 已吸收）跳过
    /// - 环：`visiting` 去重，安全终止
    /// - path 无 `arc.toml` 或 `package.name` 与依赖键不符 → 硬错误（显式 > 隐式）
    pub fn absorb_path_dependencies(
        &mut self,
        project_root: &Path,
        deps: &BTreeMap<String, DependencySpec>,
    ) -> Result<(), String> {
        let mut queue: VecDeque<(String, PathBuf)> = VecDeque::new();
        for (name, spec) in deps {
            queue.push_back((name.clone(), project_root.join(&spec.path)));
        }
        let mut visiting: BTreeSet<String> = BTreeSet::new();
        while let Some((expected_name, dir)) = queue.pop_front() {
            if self.packages.contains_key(&expected_name) {
                continue;
            }
            if !visiting.insert(expected_name.clone()) {
                continue;
            }
            let manifest_path = dir.join("arc.toml");
            if !manifest_path.is_file() {
                return Err(format!(
                    "cannot resolve dependency `{expected_name}`: path `{}` has no arc.toml (resolved {})",
                    dir.display(),
                    manifest_path.display()
                ));
            }
            let manifest = ArcManifest::load(&manifest_path).map_err(|e| e.to_string())?;
            if manifest.package.name != expected_name {
                return Err(format!(
                    "cannot resolve dependency `{expected_name}`: path `{}` package.name is `{}` (must match key)",
                    dir.display(),
                    manifest.package.name
                ));
            }
            let canon_dir = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            for (dep_name, dep_spec) in &manifest.dependencies {
                if !self.packages.contains_key(dep_name) {
                    queue.push_back((dep_name.clone(), dir.join(&dep_spec.path)));
                }
            }
            self.packages.insert(
                expected_name.clone(),
                PackageNode {
                    name: expected_name,
                    namespace: manifest.package.namespace.clone(),
                    dir: canon_dir,
                    dependencies: manifest.dependencies.clone(),
                    internals_visible_to: manifest.package.internals_visible_to.clone(),
                },
            );
        }
        Ok(())
    }

    /// RFC 025 B4：已知包名须落在 `std/<rel>/`（解决方案目录），与文档包图一致。
    ///
    /// `std_root` 为实际扫描的 std 根（默认 `workspace/std`，或 `[std].path` 覆盖）。
    pub fn validate_layout(&self, std_root: &Path) -> Result<(), String> {
        for (name, node) in &self.packages {
            let Some(expected_rel) = expected_std_rel(name) else {
                continue;
            };
            let expected = std_root.join(expected_rel);
            let actual_canon = node.dir.canonicalize().unwrap_or_else(|_| node.dir.clone());
            let expected_canon = expected.canonicalize().unwrap_or_else(|_| expected.clone());
            if actual_canon != expected_canon {
                let shown = node
                    .dir
                    .strip_prefix(std_root)
                    .unwrap_or(&node.dir)
                    .display();
                return Err(format!(
                    "std package `{name}` layout mismatch: expected `std/{expected_rel}/`, \
                     found `{shown}` (RFC 025: solution-directory layout; see expected_std_rel)"
                ));
            }
        }
        Ok(())
    }

    /// 按命名空间最长前缀匹配：`using Arc.Net.Http` → `Arc.Net`。
    pub fn match_namespace(&self, using_path: &str) -> Option<&PackageNode> {
        let mut best: Option<&PackageNode> = None;
        for pkg in self.packages.values() {
            let ns = pkg.namespace.as_str();
            if (using_path == ns || using_path.starts_with(&format!("{ns}.")))
                && best.is_none_or(|b| pkg.namespace.len() > b.namespace.len())
            {
                best = Some(pkg);
            }
        }
        best
    }

    /// 源文件所属包：向上找最近 `arc.toml`；若在已知 std 包目录下则用该包名，
    /// 否则回退到 `entry_package`（用户项目）。
    pub fn package_for_file(&self, file: &Path, entry_package: &str) -> String {
        let mut dir = if file.is_file() {
            match file.parent() {
                Some(p) => p.to_path_buf(),
                None => return entry_package.to_string(),
            }
        } else {
            file.to_path_buf()
        };
        loop {
            let manifest_path = dir.join("arc.toml");
            if manifest_path.is_file() {
                if let Ok(m) = ArcManifest::load(&manifest_path) {
                    // 优先认包图中的 std 包；项目根 arc.toml 也走 name。
                    return m.package.name;
                }
            }
            if !dir.pop() {
                break;
            }
        }
        entry_package.to_string()
    }

    /// `Arc` 是否为默认隐式引入包（RFC 025 D3.1）。
    pub fn is_implicit_package(name: &str) -> bool {
        name == "Arc"
    }

    /// 从一组根包名出发，BFS 求传递依赖闭包（含根自身）。
    ///
    /// - 根不在图中 → [`ClosureError::UnknownRoot`]
    /// - 某包声明的依赖不在图中 → [`ClosureError::MissingEdge`]
    /// - 环：安全跳过已访问节点（闭包仍完整）
    pub fn transitive_closure<'a, I>(&self, roots: I) -> Result<BTreeSet<String>, ClosureError>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut out = BTreeSet::new();
        let mut queue = VecDeque::new();
        for root in roots {
            if !self.packages.contains_key(root) {
                return Err(ClosureError::UnknownRoot {
                    package: root.to_string(),
                });
            }
            if out.insert(root.to_string()) {
                queue.push_back(root.to_string());
            }
        }
        while let Some(name) = queue.pop_front() {
            let node = self
                .packages
                .get(&name)
                .expect("queued package must exist in graph");
            for dep in node.dependencies.keys() {
                if !self.packages.contains_key(dep) {
                    return Err(ClosureError::MissingEdge {
                        from: name.clone(),
                        missing: dep.clone(),
                    });
                }
                if out.insert(dep.clone()) {
                    queue.push_back(dep.clone());
                }
            }
        }
        Ok(out)
    }

    /// 入口项目允许 `using` 的包集合：显式依赖的传递闭包 ∪ 隐式 `Arc`。
    ///
    /// - 包图中已有的依赖键作为闭包根（含 std 子库）。
    /// - 尚未入图的 path 依赖（本地 Peer 等）跳过，不阻塞非 std path。
    /// - 入图但未列显式依赖的 std 子库不含（闭包从显式依赖出发）。
    pub fn allowed_for_entry(
        &self,
        declared_deps: &BTreeMap<String, DependencySpec>,
    ) -> Result<BTreeSet<String>, ClosureError> {
        let mut roots = Vec::new();
        for name in declared_deps.keys() {
            if self.packages.contains_key(name) {
                roots.push(name.as_str());
            }
            // else: path 依赖尚未入图（本地包）— 跳过
        }
        let mut allowed = self.transitive_closure(roots)?;
        allowed.insert("Arc".to_string());
        Ok(allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn discover_std_includes_core_sublibs() {
        let g = PackageGraph::discover_std(&workspace()).expect("discover");
        assert!(g.packages.contains_key("Arc"), "Arc core");
        assert!(g.packages.contains_key("Arc.Net"), "Arc.Net");
        assert!(g.packages.contains_key("Arc.Security"), "Arc.Security");
        let net = &g.packages["Arc.Net"];
        assert_eq!(net.namespace, "Arc.Net");
        assert!(net.dependencies.contains_key("Arc"));
    }

    #[test]
    fn discover_std_at_scans_explicit_root() {
        let root = std::env::temp_dir().join(format!("arc-std-at-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pkg = root.join("Net/Core");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("arc.toml"),
            r#"
[package]
name = "Arc.Net"
namespace = "Arc.Net"
"#,
        )
        .unwrap();
        let g = PackageGraph::discover_std_at(&root).expect("discover_std_at");
        assert!(g.packages.contains_key("Arc.Net"));
        assert_eq!(
            g.packages["Arc.Net"].dir.canonicalize().unwrap(),
            pkg.canonicalize().unwrap()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_std_data_flat_sibling_layout() {
        let ws = workspace();
        let g = PackageGraph::discover_std(&ws).expect("discover");
        assert!(g.packages.contains_key("Arc.Data"), "Arc.Data package");
        let data = &g.packages["Arc.Data"];
        assert_eq!(data.namespace, "Arc.Data");
        assert_eq!(data.dir, ws.join("std/Data"));
        assert!(data.dependencies.contains_key("Arc"), "Arc.Data → Arc");
        // 禁止回归嵌套包根：Arc.Data 不得再存在于 std/Arc/Data/
        assert!(!ws.join("std/Arc/Data/arc.toml").is_file());
    }

    #[test]
    fn package_for_file_resolves_arc_data() {
        let ws = workspace();
        let g = PackageGraph::discover_std(&ws).expect("discover");
        let idb = ws.join("std/Data/IDbProvider.as");
        assert!(idb.is_file(), "fixture: std/Data/IDbProvider.as");
        assert_eq!(g.package_for_file(&idb, "App"), "Arc.Data");
    }

    #[test]
    fn match_namespace_arc_data_longest_prefix() {
        // Arc.Data 是独立包，Longest-prefix 必须命中 Arc.Data 而非 Arc
        let g = PackageGraph::discover_std(&workspace()).expect("discover");
        assert_eq!(
            g.match_namespace("Arc.Data").map(|p| p.name.as_str()),
            Some("Arc.Data")
        );
    }

    #[test]
    fn discover_std_orm_nested_solution_layout() {
        let ws = workspace();
        let g = PackageGraph::discover_std(&ws).expect("discover");
        assert!(g.packages.contains_key("Arc.Orm"));
        assert!(g.packages.contains_key("Arc.Orm.SQLite"));
        assert!(g.packages.contains_key("Arc.Orm.PostgreSQL"));
        assert!(g.packages.contains_key("Arc.Orm.Mongo"));
        let orm = &g.packages["Arc.Orm"];
        assert_eq!(orm.dir, ws.join("std/Orm/Core"));
        assert_eq!(g.packages["Arc.Orm.SQLite"].dir, ws.join("std/Orm/SQLite"));
        assert_eq!(
            g.packages["Arc.Orm.PostgreSQL"].dir,
            ws.join("std/Orm/PostgreSQL")
        );
        assert_eq!(g.packages["Arc.Orm.Mongo"].dir, ws.join("std/Orm/MongoDB"));
        assert!(g.packages["Arc.Orm.SQLite"]
            .dependencies
            .contains_key("Arc.Orm"));
        // 禁止回归平级方言根（Arc.Orm.SQLite 应在 std/Orm/SQLite/ 而非 std/Orm.SQLite/）
        assert!(!ws.join("std/Orm.SQLite/arc.toml").is_file());
    }

    #[test]
    fn discover_std_ui_solution_layout() {
        // UI 域解决方案（std/UI/arc.toml 聚合）：Arc.UI 核心库在 UI/Core，
        // Edit/Md/WebView/WebWindow/Simulator 为独立组件库；
        // 依赖拓扑 WebView → Core → WebWindow；Edit/Md/Simulator → Core。
        let ws = workspace();
        let g = PackageGraph::discover_std(&ws).expect("discover");
        assert!(g.packages.contains_key("Arc.UI"));
        assert!(g.packages.contains_key("Arc.UI.Edit"));
        assert!(g.packages.contains_key("Arc.UI.Md"));
        assert!(g.packages.contains_key("Arc.UI.WebView"));
        assert!(g.packages.contains_key("Arc.UI.WebWindow"));
        assert!(g.packages.contains_key("Arc.UI.Simulator"));
        assert_eq!(g.packages["Arc.UI"].dir, ws.join("std/UI/Core"));
        assert_eq!(g.packages["Arc.UI.Edit"].dir, ws.join("std/UI/Edit"));
        assert_eq!(g.packages["Arc.UI.Md"].dir, ws.join("std/UI/Md"));
        assert_eq!(g.packages["Arc.UI.WebView"].dir, ws.join("std/UI/WebView"));
        assert_eq!(
            g.packages["Arc.UI.WebWindow"].dir,
            ws.join("std/UI/WebWindow")
        );
        assert_eq!(
            g.packages["Arc.UI.Simulator"].dir,
            ws.join("std/UI/Simulator")
        );
        assert!(g.packages["Arc.UI.Edit"]
            .dependencies
            .contains_key("Arc.UI"));
        assert!(g.packages["Arc.UI.Md"].dependencies.contains_key("Arc.UI"));
        assert!(g.packages["Arc.UI.Simulator"]
            .dependencies
            .contains_key("Arc.UI"));
        assert!(g.packages["Arc.UI.WebView"]
            .dependencies
            .contains_key("Arc.UI"));
        assert!(g.packages["Arc.UI.WebWindow"]
            .dependencies
            .contains_key("Arc.UI.WebView"));
        assert!(g.packages["Arc.UI.WebWindow"]
            .dependencies
            .contains_key("Arc.Web"));
        // std/UI/arc.toml 为纯 workspace 聚合（无 [package]），核心库包根在 UI/Core
        assert!(
            is_workspace_only_manifest(&ws.join("std/UI/arc.toml")),
            "std/UI/arc.toml must be workspace-only"
        );
    }

    #[test]
    fn validate_layout_rejects_flat_orm_sqlite() {
        let ws = workspace();
        let mut g = PackageGraph::default();
        g.packages.insert(
            "Arc.Orm.SQLite".into(),
            PackageNode {
                name: "Arc.Orm.SQLite".into(),
                namespace: "Arc.Orm.SQLite".into(),
                dir: ws.join("std/Orm.SQLite"),
                dependencies: BTreeMap::new(),
                internals_visible_to: Vec::new(),
            },
        );
        let err = g.validate_layout(&ws.join("std")).expect_err("flat root");
        assert!(err.contains("Arc.Orm.SQLite"), "{err}");
        assert!(err.contains("Orm.SQLite"), "{err}");
    }

    #[test]
    fn match_namespace_longest_prefix() {
        let g = PackageGraph::discover_std(&workspace()).expect("discover");
        assert_eq!(
            g.match_namespace("Arc.Collections")
                .map(|p| p.name.as_str()),
            Some("Arc")
        );
        assert_eq!(
            g.match_namespace("Arc.Security").map(|p| p.name.as_str()),
            Some("Arc.Security")
        );
        assert_eq!(
            g.match_namespace("Arc.Orm.SQLite.Foo")
                .map(|p| p.name.as_str()),
            Some("Arc.Orm.SQLite")
        );
    }

    #[test]
    fn package_for_file_resolves_std_net() {
        let ws = workspace();
        let g = PackageGraph::discover_std(&ws).expect("discover");
        let net_file = ws.join("std/Net/Core/Http/HttpClient.as");
        if net_file.is_file() {
            assert_eq!(g.package_for_file(&net_file, "App"), "Arc.Net");
        }
        let arc_file = ws.join("std/Arc/Console.as");
        if arc_file.is_file() {
            assert_eq!(g.package_for_file(&arc_file, "App"), "Arc");
        }
        let sqlite = ws.join("std/Orm/SQLite/SqliteProvider.as");
        assert!(sqlite.is_file(), "fixture: nested Orm/SQLite");
        assert_eq!(g.package_for_file(&sqlite, "App"), "Arc.Orm.SQLite");
        let orm_core = ws.join("std/Orm/Core/SqlTranslator.as");
        assert!(orm_core.is_file());
        assert_eq!(g.package_for_file(&orm_core, "App"), "Arc.Orm");
    }

    #[test]
    fn transitive_closure_net_p2p_includes_security_and_net() {
        let g = PackageGraph::discover_std(&workspace()).expect("discover");
        assert!(
            g.packages.contains_key("Arc.Net.P2P"),
            "fixture requires Arc.Net.P2P in std"
        );
        let c = g.transitive_closure(["Arc.Net.P2P"]).expect("closure");
        assert!(c.contains("Arc.Net.P2P"));
        assert!(c.contains("Arc"));
        assert!(c.contains("Arc.Net"), "P2P → Net");
        assert!(c.contains("Arc.Security"), "P2P → Security");
    }

    #[test]
    fn transitive_closure_unknown_root_is_error() {
        let g = PackageGraph::discover_std(&workspace()).expect("discover");
        let err = g
            .transitive_closure(["Arc.DoesNotExist"])
            .expect_err("unknown root");
        assert!(matches!(
            err,
            ClosureError::UnknownRoot { ref package } if package == "Arc.DoesNotExist"
        ));
        assert!(err.to_string().contains("Arc.DoesNotExist"));
    }

    #[test]
    fn transitive_closure_missing_edge_is_error() {
        let mut g = PackageGraph::default();
        g.packages.insert(
            "Root".into(),
            PackageNode {
                name: "Root".into(),
                namespace: "Root".into(),
                dir: PathBuf::from("/tmp/Root"),
                internals_visible_to: Vec::new(),
                dependencies: BTreeMap::from([(
                    "Missing".into(),
                    DependencySpec {
                        path: "../Missing".into(),
                    },
                )]),
            },
        );
        let err = g.transitive_closure(["Root"]).expect_err("missing edge");
        assert!(matches!(
            err,
            ClosureError::MissingEdge {
                ref from,
                ref missing
            } if from == "Root" && missing == "Missing"
        ));
        let msg = err.to_string();
        assert!(msg.contains("Root") && msg.contains("Missing"));
    }

    #[test]
    fn transitive_closure_cycle_is_safe_and_complete() {
        // 依赖环：A → B → A，外加叶子 C。transitive_closure 须以有界访问集
        // （BTreeSet）安全跳过已访问节点——环不导致无限循环，且闭包仍完整。
        let mut g = PackageGraph::default();
        for (name, deps) in [("A", vec!["B"]), ("B", vec!["A", "C"]), ("C", vec![])] {
            g.packages.insert(
                name.into(),
                PackageNode {
                    name: name.into(),
                    namespace: name.into(),
                    dir: PathBuf::from(format!("/tmp/{name}")),
                    internals_visible_to: Vec::new(),
                    dependencies: deps
                        .into_iter()
                        .map(|d| {
                            (
                                d.to_string(),
                                DependencySpec {
                                    path: format!("../{d}"),
                                },
                            )
                        })
                        .collect(),
                },
            );
        }
        let closure = g
            .transitive_closure(["A"])
            .expect("cycle must not hang; closure succeeds");
        let set: BTreeSet<String> = ["A", "B", "C"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            closure, set,
            "cycle-safe closure must contain all reachable packages"
        );
    }

    #[test]
    fn discover_absorbs_entry_path_dependency() {
        let ws = workspace();
        let temp =
            std::env::temp_dir().join(format!("arc_pkg_graph_discover_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::create_dir_all(temp.join("Peer")).unwrap();
        std::fs::write(
            temp.join("arc.toml"),
            "[package]\nname = \"HostApp\"\nnamespace = \"HostApp\"\n\n[dependencies]\nPeer = { path = \"Peer\" }\n",
        )
        .unwrap();
        std::fs::write(
            temp.join("Peer/arc.toml"),
            "[package]\nname = \"Peer\"\nnamespace = \"Peer\"\n\n[dependencies]\n",
        )
        .unwrap();

        let manifest = ArcManifest::load(&temp.join("arc.toml")).unwrap();
        // RFC 039 §1.7.1 P1：discover 吸收入口 path 依赖，跨库包入图。
        let graph = PackageGraph::discover(&ws, &temp, &manifest.dependencies).unwrap();
        assert!(graph.packages.contains_key("Peer"), "Peer absorbed");

        // 吸收后 allowed_for_entry 不再静默跳过 Peer → 成为闭包根被允许。
        let allowed = graph.allowed_for_entry(&manifest.dependencies).unwrap();
        assert!(allowed.contains("Peer"));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn allowed_for_entry_external_path_is_allowed() {
        // 非 Arc.* 的 path 依赖（RemoteLib）不在 PackageGraph 中，跳过进入 allowed。
        let g = PackageGraph::discover_std(&workspace()).expect("discover");
        let mut deps = BTreeMap::new();
        deps.insert(
            "RemoteLib".into(),
            DependencySpec {
                path: "../RemoteLib".into(),
            },
        );
        let allowed = g.allowed_for_entry(&deps).expect("external path ok");
        assert!(allowed.contains("Arc"));
        assert!(!allowed.contains("RemoteLib"));
    }

    #[test]
    fn allowed_for_entry_closes_p2p_to_security() {
        let g = PackageGraph::discover_std(&workspace()).expect("discover");
        let mut deps = BTreeMap::new();
        deps.insert(
            "Arc.Net.P2P".into(),
            DependencySpec {
                path: "../../std/Net/P2P".into(),
            },
        );
        // 本地未入图的 path 包应被忽略，不触发 UnknownRoot。
        deps.insert(
            "Peer".into(),
            DependencySpec {
                path: "Peer".into(),
            },
        );
        let allowed = g.allowed_for_entry(&deps).expect("allowed");
        assert!(allowed.contains("Arc"));
        assert!(allowed.contains("Arc.Net.P2P"));
        assert!(allowed.contains("Arc.Security"));
        assert!(allowed.contains("Arc.Net"));
        assert!(!allowed.contains("Peer"));
    }

    #[test]
    fn allowed_for_entry_direct_security() {
        let g = PackageGraph::discover_std(&workspace()).expect("discover");
        let mut deps = BTreeMap::new();
        deps.insert(
            "Arc.Security".into(),
            DependencySpec {
                path: "../../std/Security".into(),
            },
        );
        let allowed = g.allowed_for_entry(&deps).expect("allowed");
        assert!(allowed.contains("Arc.Security"));
        assert!(allowed.contains("Arc"));
        assert!(!allowed.contains("Arc.Net"));
    }
}
