//! Workspace / 解决方案聚合层（对标 C# `.sln`）。
//!
//! ## 设计定位
//!
//! **解决方案即 workspace，仍由 `arc.toml` 承载**——不引入独立 `.arcsln`
//! 文件格式。一个 workspace 根 `arc.toml` 通过 `[workspace] members` 枚举
//! 若干成员项目；`arc build/check/run` 在 workspace 根执行时，按依赖拓扑
//! 顺序逐一构建全部成员（对标 `dotnet sln` 一键全量构建）。
//!
//! ```toml
//! # <workspace>/arc.toml
//! [workspace]
//! members = ["src/App", "src/Lib"]
//! ```
//!
//! 每个成员必须是含自身 `arc.toml`（`[package]`）的独立项目。成员之间可经
//! `[dependencies]` 的 `path` 依赖互相引用（源码级 project reference）——
//! [`Workspace::build_order`] 依据该依赖图产出拓扑构建顺序：被依赖项目先构建。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::manifest::{ArcManifest, WorkspaceSection};

/// 一个 workspace 成员项目。
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    /// 成员项目根目录（绝对路径）。
    pub root: PathBuf,
    /// 成员项目的 `arc.toml` manifest。
    pub manifest: ArcManifest,
}

/// Workspace（解决方案）：`arc.toml` 的 `[workspace]` 聚合。
#[derive(Debug, Clone)]
pub struct Workspace {
    /// workspace 根目录（承载 `[workspace]` 的 `arc.toml` 所在目录）。
    pub root: PathBuf,
    /// 成员项目（已加载 manifest）。
    pub members: Vec<WorkspaceMember>,
}

impl Workspace {
    /// 从任意起点（文件或目录）向上查找**最近的** workspace 根 `arc.toml` 并加载。
    ///
    /// 逐级向上直到找到含非空 `[workspace] members` 的 `arc.toml`（对标 `dotnet
    /// build` 向上查找 `.sln`）。成员项目本身是普通单项目（无 workspace 聚合），
    /// 会被跳过、继续向上找所属解决方案。全程无解决方案根 → `Ok(None)`（纯单项目）。
    /// workspace 根可为纯解决方案（仅 `[workspace]`，无 `[package]`），对标独立 `.sln`。
    pub fn discover(start: &Path) -> Result<Option<Self>, String> {
        let start_dir = if start.is_dir() {
            start.to_path_buf()
        } else {
            start
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        };
        let mut dir: Option<&Path> = Some(&start_dir);
        while let Some(d) = dir {
            let arc_toml = d.join("arc.toml");
            if arc_toml.is_file() {
                let ws = WorkspaceSection::from_file(&arc_toml)
                    .map_err(|e| format!("read arc.toml {}: {e}", arc_toml.display()))?;
                if ws.is_solution() {
                    return Self::load(&arc_toml);
                }
            }
            dir = d.parent();
        }
        Ok(None)
    }

    /// 从指定 `arc.toml` 路径加载 workspace。不含 `[workspace] members` 返回 `None`。
    pub fn load(arc_toml: &Path) -> Result<Option<Self>, String> {
        let ws = WorkspaceSection::from_file(arc_toml)
            .map_err(|e| format!("load workspace arc.toml {}: {e}", arc_toml.display()))?;
        if !ws.is_solution() {
            return Ok(None);
        }
        let root = arc_toml
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Self::from_section(root, ws)
    }

    /// 由 workspace 根目录 + `[workspace]` 段构造；加载全部成员 manifest。
    ///
    /// **嵌套解决方案**：成员项指向的 `arc.toml` 若本身是纯 `[workspace]` 解决方案
    /// （无 `[package]`），递归展开其成员并入本解决方案（对标分层解决方案管理——
    /// `std` 根聚合 `AI`/`Net`/`Orm` 等域解决方案）。递归以规范化根路径集合检环
    /// （解决方案互引成环报错）；同一项目被多个嵌套解决方案引用时按规范化根去重。
    fn from_section(root: PathBuf, ws: WorkspaceSection) -> Result<Option<Self>, String> {
        let root = root.canonicalize().unwrap_or(root);
        let mut members = Vec::new();
        let mut solution_roots: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();
        let mut member_roots: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        load_members_recursive(
            &root,
            &ws,
            &mut members,
            &mut solution_roots,
            &mut member_roots,
        )?;
        Ok(Some(Workspace { root, members }))
    }

    /// 成员数量。
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// 是否无成员。
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// 每个成员的直接 workspace 依赖（下标列表）。
    ///
    /// `direct_deps[i]` = 成员 i 经 `[dependencies] path` 直接引用的、位于本
    /// workspace 内的成员下标列表。`[dependencies]` 的 path 指向 workspace 外
    /// 目录时**不参与**（非成员，无 workspace 级产物直连）。
    ///
    /// 供 `arc build` 注入成员库产物（ProjectReference 式 `.aopkg` 直连）：
    /// 依赖成员先构建产 `.aopkg`，本成员构建时按此表注入其库产物。
    pub fn direct_dependencies(&self) -> Vec<Vec<usize>> {
        self.dependency_edges()
    }

    /// 依赖边：`dep_on[i]` = 成员 i 经 `[dependencies] path` 直接依赖的成员下标列表。
    ///
    /// 成员根按规范化绝对路径匹配（`root_index`），自依赖（`j == i`）剔除。
    fn dependency_edges(&self) -> Vec<Vec<usize>> {
        // 成员根（规范化绝对路径）→ 下标
        let root_index: HashMap<PathBuf, usize> = self
            .members
            .iter()
            .enumerate()
            .map(|(i, m)| (m.root.clone(), i))
            .collect();

        let mut dep_on: Vec<Vec<usize>> = vec![Vec::new(); self.members.len()];
        for (i, member) in self.members.iter().enumerate() {
            for spec in member.manifest.dependencies.values() {
                let dep_dir = member.root.join(&spec.path);
                let canon = dep_dir.canonicalize().unwrap_or(dep_dir);
                if let Some(&j) = root_index.get(&canon) {
                    if j != i {
                        dep_on[i].push(j);
                    }
                }
            }
        }
        dep_on
    }

    /// 起点路径（目录或文件）对应的成员下标（对标 `dotnet build <csproj>` 的
    /// 项目定位：路径命中某成员项目根即视为「构建该项目」）。
    ///
    /// 成员根在加载时已规范化；输入同样规范化后精确匹配（不做前缀匹配——
    /// 成员内子目录不是成员本身）。
    pub fn member_index_of(&self, path: &Path) -> Option<usize> {
        // 目录：直接定位；文件：取父目录（对标 `dotnet build <csproj>`）；两者皆非
        // （路径不存在）→ 无法定位成员，返回 None——不得回退到父目录，否则
        // 成员内不存在的子路径会被误判为成员本身。
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else if path.is_file() {
            path.parent().map(|p| p.to_path_buf())?
        } else {
            return None;
        };
        let canon = dir.canonicalize().unwrap_or(dir);
        self.members.iter().position(|m| m.root == canon)
    }

    /// 成员 `member` 的 ProjectReference 闭包（含自身）按依赖拓扑序的下标列表。
    ///
    /// 对标 `dotnet build <csproj>`：只构建该项目 + 其传递项目引用，不构建
    /// 解决方案其余成员。顺序保证与 [`Workspace::build_order`] 一致（被依赖者
    /// 先出）；依赖环由 `build_order` 统一检出。
    pub fn closure_order(&self, member: usize) -> Result<Vec<usize>, String> {
        let dep_on = self.dependency_edges();
        let full = self.build_order()?;
        // 反向可达闭包：member 自身 + 其传递依赖。
        let mut closure = vec![false; self.members.len()];
        let mut stack = vec![member];
        closure[member] = true;
        while let Some(i) = stack.pop() {
            for &j in &dep_on[i] {
                if !closure[j] {
                    closure[j] = true;
                    stack.push(j);
                }
            }
        }
        Ok(full.into_iter().filter(|&i| closure[i]).collect())
    }

    /// 按依赖拓扑序返回成员构建顺序（下标列表）。
    ///
    /// 成员 A 依赖成员 B（A 的 `[dependencies]` path 指向 B 的项目根）时，
    /// B 必须先构建。返回顺序保证：任意成员的依赖均先于其本身。
    /// 存在依赖环时报错（C# 项目引用亦禁止环）。
    pub fn build_order(&self) -> Result<Vec<usize>, String> {
        // 依赖边：dep_on[i] = { j | 成员 i 依赖成员 j }
        let dep_on = self.dependency_edges();

        // Kahn 拓扑排序：被依赖者先出。
        let mut in_degree = vec![0usize; self.members.len()];
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); self.members.len()];
        for i in 0..self.members.len() {
            for &j in &dep_on[i] {
                in_degree[i] += 1;
                dependents[j].push(i);
            }
        }
        let mut ready: Vec<usize> = (0..self.members.len())
            .filter(|&i| in_degree[i] == 0)
            .collect();
        ready.sort_unstable();
        let mut order = Vec::with_capacity(self.members.len());
        let mut visited = 0usize;
        while let Some(i) = ready.pop() {
            order.push(i);
            visited += 1;
            for &d in &dependents[i] {
                in_degree[d] -= 1;
                if in_degree[d] == 0 {
                    ready.push(d);
                }
            }
        }
        if visited != self.members.len() {
            return Err("workspace has a circular project reference".to_string());
        }
        Ok(order)
    }
}

/// 递归加载解决方案成员（含嵌套解决方案展开）。
///
/// 每个成员项：`arc.toml` 为纯 `[workspace]`（嵌套解决方案）→ 深入递归；
/// 含 `[package]`（普通项目）→ 校验并追加。`solution_roots` 检解决方案环，
/// `member_roots` 对同一项目去重（被多个嵌套解决方案引用时只保留一份）。
fn load_members_recursive(
    root: &Path,
    ws: &WorkspaceSection,
    members: &mut Vec<WorkspaceMember>,
    solution_roots: &mut std::collections::HashSet<PathBuf>,
    member_roots: &mut std::collections::HashSet<PathBuf>,
) -> Result<(), String> {
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !solution_roots.insert(canon_root) {
        return Err(format!(
            "workspace has a circular nested solution reference at {}",
            root.display()
        ));
    }
    for rel in &ws.members {
        let member_dir = root.join(rel);
        let member_arc_toml = member_dir.join("arc.toml");
        if !member_arc_toml.is_file() {
            return Err(format!(
                "workspace member \"{rel}\" has no arc.toml at {}",
                member_arc_toml.display()
            ));
        }
        let nested = WorkspaceSection::from_file(&member_arc_toml)
            .map_err(|e| format!("read arc.toml {}: {e}", member_arc_toml.display()))?;
        if nested.is_solution() {
            let nested_root = member_dir.canonicalize().unwrap_or(member_dir);
            load_members_recursive(&nested_root, &nested, members, solution_roots, member_roots)?;
            continue;
        }
        let m = ArcManifest::load(&member_arc_toml)
            .map_err(|e| format!("load workspace member \"{rel}\": {e}"))?;
        if m.package.name.is_empty() {
            return Err(format!(
                "workspace member \"{rel}\" missing [package].name in {}",
                member_arc_toml.display()
            ));
        }
        let canon = member_dir.canonicalize().unwrap_or(member_dir);
        if !member_roots.insert(canon.clone()) {
            continue;
        }
        members.push(WorkspaceMember {
            root: canon,
            manifest: m,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arc-ws-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_member(root: &Path, rel: &str, name: &str, deps: &str) {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("arc.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nkind = \"library\"\n\n{deps}"
            ),
        )
        .unwrap();
    }

    #[test]
    fn discover_solution_members() {
        let root = temp_root("discover");
        write_member(&root, "src/Lib", "Lib", "");
        write_member(
            &root,
            "src/App",
            "App",
            "[dependencies]\nLib = { path = \"../Lib\" }\n",
        );
        fs::write(
            root.join("arc.toml"),
            "[workspace]\nmembers = [\"src/App\", \"src/Lib\"]\n",
        )
        .unwrap();

        let ws = Workspace::discover(&root.join("src/App"))
            .unwrap()
            .expect("solution");
        assert_eq!(ws.len(), 2);
        assert_eq!(ws.members[0].manifest.package.name, "App");
        assert_eq!(ws.members[1].manifest.package.name, "Lib");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_non_solution_returns_none() {
        let root = temp_root("single");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src").join("arc.toml"),
            "[package]\nname = \"Single\"\n",
        )
        .unwrap();
        assert!(
            Workspace::discover(&root.join("src")).unwrap().is_none(),
            "single project must not be a solution"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_order_respects_dependencies() {
        let root = temp_root("order");
        // 声明顺序故意乱序：App → Mid → Base（App 依赖 Mid，Mid 依赖 Base）
        write_member(
            &root,
            "App",
            "App",
            "[dependencies]\nMid = { path = \"../Mid\" }\n",
        );
        write_member(&root, "Base", "Base", "");
        write_member(
            &root,
            "Mid",
            "Mid",
            "[dependencies]\nBase = { path = \"../Base\" }\n",
        );
        fs::write(
            root.join("arc.toml"),
            "[workspace]\nmembers = [\"App\", \"Base\", \"Mid\"]\n",
        )
        .unwrap();

        let ws = Workspace::load(&root.join("arc.toml"))
            .unwrap()
            .expect("solution");
        let order = ws.build_order().unwrap();
        let names: Vec<&str> = order
            .iter()
            .map(|&i| ws.members[i].manifest.package.name.as_str())
            .collect();
        // Base 与 Mid 在 App 前；Mid 在 Base 后
        let pos = |n: &str| names.iter().position(|&x| x == n).unwrap();
        assert!(pos("Base") < pos("Mid"), "{names:?}");
        assert!(pos("Base") < pos("App"), "{names:?}");
        assert!(pos("Mid") < pos("App"), "{names:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn direct_dependencies_edges() {
        let root = temp_root("direct");
        write_member(
            &root,
            "App",
            "App",
            "[dependencies]\nMid = { path = \"../Mid\" }\n",
        );
        write_member(&root, "Base", "Base", "");
        write_member(
            &root,
            "Mid",
            "Mid",
            "[dependencies]\nBase = { path = \"../Base\" }\n",
        );
        fs::write(
            root.join("arc.toml"),
            "[workspace]\nmembers = [\"App\", \"Base\", \"Mid\"]\n",
        )
        .unwrap();

        let ws = Workspace::load(&root.join("arc.toml"))
            .unwrap()
            .expect("solution");
        let deps = ws.direct_dependencies();
        let names = |i: usize| {
            deps[i]
                .iter()
                .map(|&j| ws.members[j].manifest.package.name.as_str())
                .collect::<Vec<_>>()
        };
        // App 依赖 Mid；Mid 依赖 Base；Base 无依赖。
        assert_eq!(names(0), vec!["Mid"]);
        assert_eq!(names(1), Vec::<&str>::new());
        assert_eq!(names(2), vec!["Base"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_order_detects_cycle() {
        let root = temp_root("cycle");
        write_member(&root, "A", "A", "[dependencies]\nB = { path = \"../B\" }\n");
        write_member(&root, "B", "B", "[dependencies]\nA = { path = \"../A\" }\n");
        fs::write(
            root.join("arc.toml"),
            "[workspace]\nmembers = [\"A\", \"B\"]\n",
        )
        .unwrap();
        let ws = Workspace::load(&root.join("arc.toml"))
            .unwrap()
            .expect("solution");
        assert!(ws.build_order().is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nested_solution_flattens_members_with_dedup_and_cycle_guard() {
        let root = temp_root("nested");
        // 域解决方案 AI（Core + Agent）、Net（Core）；顶层聚合 AI + Net + 独立 Util。
        write_member(&root, "AI/Core", "AI.Core", "");
        write_member(
            &root,
            "AI/Agent",
            "AI.Agent",
            "[dependencies]\n\"AI.Core\" = { path = \"../Core\" }\n",
        );
        write_member(&root, "Net/Core", "Net.Core", "");
        write_member(&root, "Util", "Util", "");
        fs::write(
            root.join("AI/arc.toml"),
            "[workspace]\nmembers = [\"Core\", \"Agent\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("Net/arc.toml"),
            "[workspace]\nmembers = [\"Core\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("arc.toml"),
            "[workspace]\nmembers = [\"AI\", \"Net\", \"Util\"]\n",
        )
        .unwrap();

        // 顶层解决方案：嵌套成员被递归展开（AI.Core、AI.Agent、Net.Core、Util）。
        let ws = Workspace::load(&root.join("arc.toml"))
            .unwrap()
            .expect("solution");
        let names: Vec<&str> = ws
            .members
            .iter()
            .map(|m| m.manifest.package.name.as_str())
            .collect();
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(names.contains(&"AI.Core"), "{names:?}");
        assert!(names.contains(&"AI.Agent"), "{names:?}");
        assert!(names.contains(&"Net.Core"), "{names:?}");
        assert!(names.contains(&"Util"), "{names:?}");
        // 展开后跨嵌套边依赖仍拓扑有序（AI.Core 在 AI.Agent 前）。
        let order = ws.build_order().unwrap();
        let onames: Vec<&str> = order
            .iter()
            .map(|&i| ws.members[i].manifest.package.name.as_str())
            .collect();
        let pos = |n: &str| onames.iter().position(|&x| x == n).unwrap();
        assert!(pos("AI.Core") < pos("AI.Agent"), "{onames:?}");
        // 子解决方案根也可独立发现（arc build std/AI 语义）。
        let ai = Workspace::discover(&root.join("AI"))
            .unwrap()
            .expect("AI solution");
        assert_eq!(ai.len(), 2);
        assert_eq!(ai.root, root.join("AI").canonicalize().unwrap());
        // 子解决方案根不是顶层成员（member_index_of 不命中）；其成员命中。
        assert!(ws.member_index_of(&root.join("AI")).is_none());
        let agent = ws
            .member_index_of(&root.join("AI/Agent"))
            .expect("Agent member");
        let closure = ws.closure_order(agent).unwrap();
        assert_eq!(closure.len(), 2); // AI.Core + AI.Agent
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nested_solution_cycle_is_error() {
        let root = temp_root("nested-cycle");
        fs::create_dir_all(root.join("A")).unwrap();
        fs::create_dir_all(root.join("B")).unwrap();
        fs::write(
            root.join("A/arc.toml"),
            "[workspace]\nmembers = [\"../B\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("B/arc.toml"),
            "[workspace]\nmembers = [\"../A\"]\n",
        )
        .unwrap();
        fs::write(root.join("arc.toml"), "[workspace]\nmembers = [\"A\"]\n").unwrap();
        let err = Workspace::load(&root.join("arc.toml")).unwrap_err();
        assert!(err.contains("circular"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn member_index_and_closure_order_match_csproj_semantics() {
        let root = temp_root("closure");
        // App → Mid → Base 链 + 与 App 无关的 Lone 成员。
        write_member(
            &root,
            "App",
            "App",
            "[dependencies]\nMid = { path = \"../Mid\" }\n",
        );
        write_member(&root, "Base", "Base", "");
        write_member(&root, "Lone", "Lone", "");
        write_member(
            &root,
            "Mid",
            "Mid",
            "[dependencies]\nBase = { path = \"../Base\" }\n",
        );
        fs::write(
            root.join("arc.toml"),
            "[workspace]\nmembers = [\"App\", \"Base\", \"Lone\", \"Mid\"]\n",
        )
        .unwrap();

        let ws = Workspace::load(&root.join("arc.toml"))
            .unwrap()
            .expect("solution");
        // 成员项目根精确命中；成员内子目录与解决方案根不命中。
        let app = ws.member_index_of(&root.join("App")).expect("App member");
        assert!(ws.member_index_of(&root).is_none());
        assert!(ws.member_index_of(&root.join("App").join("src")).is_none());
        // App 闭包 = Base → Mid → App（拓扑序），不含 Lone。
        let order = ws.closure_order(app).unwrap();
        let names: Vec<&str> = order
            .iter()
            .map(|&i| ws.members[i].manifest.package.name.as_str())
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
        let pos = |n: &str| names.iter().position(|&x| x == n).unwrap();
        assert!(pos("Base") < pos("Mid"), "{names:?}");
        assert!(pos("Mid") < pos("App"), "{names:?}");
        assert!(!names.contains(&"Lone"), "{names:?}");
        // Lone 闭包仅自身。
        let lone = ws.member_index_of(&root.join("Lone")).expect("Lone member");
        let lone_order = ws.closure_order(lone).unwrap();
        assert_eq!(lone_order.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn member_missing_manifest_errors() {
        let root = temp_root("missing");
        fs::create_dir_all(root.join("src/App")).unwrap();
        fs::write(
            root.join("src/App").join("arc.toml"),
            "[package]\nname = \"App\"\n",
        )
        .unwrap();
        fs::write(
            root.join("arc.toml"),
            "[workspace]\nmembers = [\"src/App\", \"src/Ghost\"]\n",
        )
        .unwrap();
        let err = Workspace::load(&root.join("arc.toml")).unwrap_err();
        assert!(err.contains("Ghost"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }
}
