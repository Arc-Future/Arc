//! Workspace 状态骨架（RFC 038 M0 §D6）。
//!
//! M0 仅定义 [`WorkspaceState`] 与 [`WorkspaceManager`] 的基础结构——
//! 不加载 `arc.toml`/`.arcgr`/`.ao`，仅维护 root 路径与 workspace 列表。
//! M1+ 扩展时填充 `file_buffers`/`memory_arcgr`/`cross_package` 等字段。
//!
//! ## 多 workspace 隔离（M4 完整实现）
//!
//! arc-server 单进程支持多 root workspace（VS Code multi-root）：
//! - 每个 workspace 独立 `arc.toml` + 依赖图 + 内存版 `.arcgr`
//! - 全局缓存 `.ao` 索引共享只读
//! - 文件变更仅影响当前 workspace 的 `.arcgr`，不污染其他 workspace
//!
//! M0 不实现隔离逻辑——所有 workspace 共享一个空 `WorkspaceManager`，
//! M4 落地真正的隔离与路由。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::project::{locate_arcgr, parse_arc_toml, Project};
use crate::semantic::{Location, PackageIndex, Position, SemanticIndex, SymbolInformation};
use crate::syntax::{TextDocument, TextDocumentContentChangeEvent};
use arcgr::Visibility;

/// 单个 workspace 的状态。
///
/// M1 在 M0 的 root 路径基础上，增加 `.arcgr` 语义索引的加载与持有——
/// 供 LSP 语义 provider（definition/hover/references/documentSymbol）查询。
/// `packages[0]` 为主包，其后为依赖包（M3 跨包查询）。增量索引、多 workspace
/// 隔离分别在 M2/M4 扩展。
#[derive(Debug, Clone)]
pub struct WorkspaceState {
    /// workspace 根目录（含 `arc.toml`）。
    root: PathBuf,
    /// 已加载的包索引（M1 起）。`packages[0]` 为主包，其余为依赖包（M3 跨包）。
    packages: Vec<PackageIndex>,
    /// 项目元数据（阶段 3 `arc.toml` 驱动；未加载则为 `None`）。
    project: Option<Project>,
    /// 已加载的 `arc.toml` 路径（用于变更检测自动重载）。
    arc_toml: Option<PathBuf>,
    /// 上次加载 `arc.toml` 的修改时间。
    arc_toml_mtime: Option<std::time::SystemTime>,
}

impl WorkspaceState {
    /// 创建新的 workspace 状态。
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            packages: Vec::new(),
            project: None,
            arc_toml: None,
            arc_toml_mtime: None,
        }
    }

    /// workspace 根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 项目元数据（`arc.toml` 驱动，阶段 3）。
    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    /// 从磁盘加载主包 `.arcgr` 语义索引（M1）。
    ///
    /// 失败返回 `Err`，不清空既有索引（保持上次可用状态）。
    pub fn load_arcgr(&mut self, arcgr_path: &Path) -> Result<(), String> {
        let index = SemanticIndex::load(arcgr_path, self.root.clone())
            .map_err(|e| format!("load .arcgr {}: {e}", arcgr_path.display()))?;
        self.packages = vec![PackageIndex::new("main", index)];
        log::info!(
            "loaded .arcgr for workspace {:?}: {}",
            self.root,
            arcgr_path.display()
        );
        Ok(())
    }

    /// 从磁盘加载一个依赖包 `.arcgr`，追加到包列表（M3 跨包查询）。
    ///
    /// 依赖包导出符号参与 `workspace/symbol` 聚合，但主包 provider 解析不受影响。
    pub fn load_dependency(&mut self, package_id: &str, arcgr_path: &Path) -> Result<(), String> {
        let index = SemanticIndex::load(arcgr_path, self.root.clone())
            .map_err(|e| format!("load dependency {}: {e}", arcgr_path.display()))?;
        self.packages.push(PackageIndex::new(package_id, index));
        log::info!(
            "loaded dependency package '{package_id}' for workspace {:?}: {}",
            self.root,
            arcgr_path.display()
        );
        Ok(())
    }

    /// 从 `arc.toml` 驱动加载整个项目（阶段 3）：主包 + 依赖包 `.arcgr`。
    ///
    /// 解析 `arc.toml` 得到项目元数据（name/kind/dependencies），沿依赖图自动
    /// 加载主包与各依赖包的 `.arcgr`（本地 `path` 源码依赖）。并把主包 `.arcgr`
    /// 携带的 `ContextManifest` L0 并入 `Project.manifest`。
    pub fn load_project(&mut self, arc_toml: &Path) -> Result<(), String> {
        let contents = std::fs::read_to_string(arc_toml)
            .map_err(|e| format!("read arc.toml {}: {e}", arc_toml.display()))?;
        let cfg = parse_arc_toml(&contents)?;
        if cfg.name.is_empty() {
            return Err("arc.toml missing [package] name".to_string());
        }

        // 主包 .arcgr
        let main_arcgr = locate_arcgr(&self.root, &cfg.name, None).ok_or_else(|| {
            format!(
                "no .arcgr found for package '{}' under {:?}",
                cfg.name, self.root
            )
        })?;
        let main_index = SemanticIndex::load(&main_arcgr, self.root.clone())
            .map_err(|e| format!("load main .arcgr {}: {e}", main_arcgr.display()))?;
        let manifest = main_index
            .arcgr()
            .context_manifest
            .as_ref()
            .map(|cm| cm.l0_project.clone());
        self.packages = vec![PackageIndex::new(cfg.name.clone(), main_index)];

        // 依赖包（沿依赖图；本地 path 源码依赖）
        for dep in &cfg.dependencies {
            let dep_arcgr = self.locate_dependency_arcgr(dep)?;
            let dep_index = SemanticIndex::load(&dep_arcgr, self.root.clone())
                .map_err(|e| format!("load dependency {}: {e}", dep_arcgr.display()))?;
            self.packages
                .push(PackageIndex::new(dep.name.clone(), dep_index));
        }

        // 记录 arc.toml 的 mtime 供变更检测
        let mtime = std::fs::metadata(arc_toml)
            .ok()
            .and_then(|m| m.modified().ok());
        self.arc_toml = Some(arc_toml.to_path_buf());
        self.arc_toml_mtime = mtime;

        self.project = Some(Project {
            name: cfg.name.clone(),
            kind: cfg.kind,
            root: self.root.clone(),
            dependencies: cfg.dependencies,
            manifest,
        });
        log::info!("loaded project '{}' from {}", cfg.name, arc_toml.display());
        Ok(())
    }

    /// 定位一个依赖包的 `.arcgr`（本地 `path` 源码引用）。
    fn locate_dependency_arcgr(
        &self,
        dep: &crate::project::DependencyRef,
    ) -> Result<PathBuf, String> {
        locate_arcgr(&self.root, &dep.name, Some(&dep.path))
            .ok_or_else(|| format!("no .arcgr found for dependency '{}'", dep.name))
    }

    /// 检测 `arc.toml` 是否变更；是则自动重载项目（阶段 3 自动重载）。
    ///
    /// 返回是否发生了重载。重载失败时仍更新 mtime（避免每次请求重复重试）。
    pub fn refresh_project_if_changed(&mut self) -> bool {
        let Some(arc_toml) = self.arc_toml.clone() else {
            return false;
        };
        let Ok(meta) = std::fs::metadata(&arc_toml) else {
            return false;
        };
        let mtime = meta.modified().ok();
        if mtime == self.arc_toml_mtime {
            return false;
        }
        self.arc_toml_mtime = mtime;
        match self.load_project(&arc_toml) {
            Ok(()) => {
                log::info!(
                    "project reloaded after arc.toml change: {}",
                    arc_toml.display()
                );
                true
            }
            Err(e) => {
                log::warn!("project reload failed after arc.toml change: {e}");
                false
            }
        }
    }

    /// 直接设置主包语义索引（测试友好）。
    pub fn set_semantic(&mut self, index: SemanticIndex) {
        self.packages = vec![PackageIndex::new("main", index)];
    }

    /// 直接追加一个依赖包语义索引（测试友好，等价于磁盘版 `load_dependency`）。
    pub fn add_dependency_index(&mut self, package_id: &str, index: SemanticIndex) {
        self.packages.push(PackageIndex::new(package_id, index));
    }

    /// 主包语义索引引用（`packages[0]`）。
    pub fn semantic(&self) -> Option<&SemanticIndex> {
        self.packages.first().map(|p| &p.semantic)
    }

    /// 主包语义索引可变引用。
    pub fn semantic_mut(&mut self) -> Option<&mut SemanticIndex> {
        self.packages.first_mut().map(|p| &mut p.semantic)
    }

    /// 是否已加载语义索引。
    pub fn has_semantic(&self) -> bool {
        !self.packages.is_empty()
    }

    /// 全部包（主包 + 依赖包）的可变迭代。
    pub fn packages_mut(&mut self) -> &mut [PackageIndex] {
        &mut self.packages
    }

    /// URI 是否属于本 workspace 的任一包（主包或依赖包）。
    pub fn uri_belongs(&self, uri: &str) -> bool {
        self.packages
            .iter()
            .any(|p| p.semantic.file_id_for_uri(uri).is_some())
    }

    /// 定位 URI 所属的（包索引, file_id）——供开放文档覆盖语义源码使用。
    pub fn file_id_for_uri(&self, uri: &str) -> Option<(usize, u32)> {
        self.packages
            .iter()
            .enumerate()
            .find_map(|(i, p)| p.semantic.file_id_for_uri(uri).map(|fid| (i, fid)))
    }

    /// 跨包 Goto Definition（M3 扩展）：解析 `(uri, pos)` 到定义位置。
    ///
    /// 解析流程：
    /// 1. 定位 URI 所属的源包（主包或依赖包）；
    /// 2. 先在源包本地解析（定义 span 或引用→本地目标）；
    /// 3. 本地无结果且光标在**外部引用**上 → 取被引用标识符名 →
    ///    在其他包中按名解析公共符号定义（跨包跳转到依赖库）。
    pub fn definition_at(&mut self, uri: &str, pos: Position) -> Option<Location> {
        let src_idx = self
            .packages
            .iter()
            .position(|p| p.semantic.file_id_for_uri(uri).is_some())?;
        let src_pkg_id = self.packages[src_idx].id.clone();
        let file_id = self.packages[src_idx].semantic.file_id_for_uri(uri)?;
        let offset = self.packages[src_idx]
            .semantic
            .position_to_offset(file_id, pos)?;

        // 2. 本地解析（定义 or 引用→本地目标）——返回本地定义位置
        if let Some(sym) = self.packages[src_idx]
            .semantic
            .symbol_at_offset(file_id, offset)
        {
            if let Some(loc) = self.packages[src_idx].semantic.definition(sym.symbol_id) {
                return Some(loc);
            }
        }

        // 3. 外部引用 → 跨包按名解析
        let name = self.packages[src_idx]
            .semantic
            .external_ref_name_at_offset(file_id, offset)?;
        self.definition_by_name(&name, &src_pkg_id)
    }

    /// 在除源包外的所有包中，按名查找公共符号的定义位置（跨包跳转）。
    fn definition_by_name(&mut self, name: &str, src_pkg_id: &str) -> Option<Location> {
        for pkg in &mut self.packages {
            if pkg.id == src_pkg_id {
                continue;
            }
            let ids: Vec<u32> = pkg
                .semantic
                .arcgr()
                .symbol_table
                .find_by_name(name)
                .into_iter()
                .filter(|s| s.visibility == Visibility::Public)
                .map(|s| s.symbol_id)
                .collect();
            for id in ids {
                if let Some(loc) = pkg.semantic.definition(id) {
                    return Some(loc);
                }
            }
        }
        None
    }
}

/// 多 workspace 管理器。
///
/// M0 仅维护 workspace 列表——不实现路由与隔离逻辑（M4 落地）。
/// `initialize` 请求的 `workspaceFolders` 通过 [`WorkspaceManager::add_workspace`]
/// 注册到列表，M1+ handler 通过 `workspaces()` 迭代查询。
#[derive(Debug, Default)]
pub struct WorkspaceManager {
    workspaces: Vec<WorkspaceState>,
    /// 打开的文本文档（M2 语法服务）：URI → 文档。跨 workspace 全局维护。
    documents: HashMap<String, TextDocument>,
}

impl WorkspaceManager {
    /// 创建空 workspace 管理器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加一个 workspace。
    ///
    /// 返回新增 workspace 在列表中的索引。
    pub fn add_workspace(&mut self, root: PathBuf) -> usize {
        let idx = self.workspaces.len();
        self.workspaces.push(WorkspaceState::new(root));
        log::info!(
            "workspace added: root={:?} (index={})",
            self.workspaces[idx].root,
            idx
        );
        idx
    }

    /// 所有 workspace 切片。
    pub fn workspaces(&self) -> &[WorkspaceState] {
        &self.workspaces
    }

    /// 所有 workspace 可变切片。
    pub fn workspaces_mut(&mut self) -> &mut [WorkspaceState] {
        &mut self.workspaces
    }

    /// workspace 数量。
    pub fn len(&self) -> usize {
        self.workspaces.len()
    }

    /// 是否无 workspace。
    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }

    /// 清空所有 workspace（用于 `shutdown` 时释放资源）。
    pub fn clear(&mut self) {
        self.workspaces.clear();
    }

    /// 加载指定索引 workspace 的 `.arcgr` 语义索引（M1）。
    ///
    /// 供 [`super::lsp::method_dispatcher::MethodDispatcher::load_workspace_arcgr`]
    /// 调用——LSP 服务启动时把已构建的 workspace + 语义索引注入到分发器。
    pub fn load_arcgr(&mut self, index: usize, arcgr_path: &Path) -> Result<(), String> {
        let ws = self
            .workspaces
            .get_mut(index)
            .ok_or_else(|| format!("workspace index {index} out of range"))?;
        ws.load_arcgr(arcgr_path)
    }

    /// 从 `arc.toml` 驱动加载指定索引 workspace 的整个项目（阶段 3）。
    ///
    /// 自动加载主包 + 依赖包 `.arcgr`，并把项目元数据写入 [`WorkspaceState::project`]。
    pub fn load_project(&mut self, index: usize, arc_toml: &Path) -> Result<(), String> {
        let ws = self
            .workspaces
            .get_mut(index)
            .ok_or_else(|| format!("workspace index {index} out of range"))?;
        ws.load_project(arc_toml)
    }

    /// 检测所有 workspace 的 `arc.toml` 是否变更，是则自动重载项目。
    ///
    /// 返回是否发生了至少一次重载。供 LSP 分发前调用（惰性变更检测，客户端无关）。
    pub fn refresh_projects_if_changed(&mut self) -> bool {
        let mut reloaded = false;
        for ws in &mut self.workspaces {
            if ws.refresh_project_if_changed() {
                reloaded = true;
            }
        }
        reloaded
    }

    /// 查找 URI 所属 workspace（其语义索引能解析该 URI）的可变引用。
    ///
    /// 返回第一个能解析 `uri` 到 `file_id` 的 workspace；无匹配返回 `None`。
    pub fn find_workspace_mut_for_uri(&mut self, uri: &str) -> Option<&mut WorkspaceState> {
        self.workspaces.iter_mut().find(|ws| {
            ws.semantic()
                .map(|idx| idx.file_id_for_uri(uri).is_some())
                .unwrap_or(false)
        })
    }

    /// 跨包符号查询（M3）：聚合所有 workspace 全部包（主包 + 依赖包）的公共符号。
    ///
    /// 实现 LSP `workspace/symbol`——跨包定位依赖库导出的符号。
    pub fn workspace_symbols(&mut self, query: Option<&str>) -> Vec<SymbolInformation> {
        let mut out = Vec::new();
        for ws in &mut self.workspaces {
            for pkg in ws.packages_mut() {
                out.extend(pkg.semantic.workspace_symbols(query));
            }
        }
        out
    }

    /// 跨包 Goto Definition（M3 扩展）：在 URI 所属 workspace 内解析定义位置。
    ///
    /// 与 [`Self::find_workspace_mut_for_uri`] 不同，本方法按全部包（含依赖包）
    /// 定位源文件，并在本地解析失败时跨包按名解析（跳转到依赖库定义）。
    pub fn definition_at(&mut self, uri: &str, pos: Position) -> Option<Location> {
        let idx = self.workspaces.iter().position(|w| w.uri_belongs(uri))?;
        self.workspaces[idx].definition_at(uri, pos)
    }

    // ─── 文本文档存储（M2 语法服务 · M3 Document 统一）───
    //
    // 开放文档缓冲是文件真源：语法 provider 直接读 `documents`（TextDocument），
    // 语义 provider 经 `propagate_document_text` 覆盖到语义索引的源码缓存——
    // 使未保存编辑在定义/悬停/引用/文档符号的位置换算中即时生效。

    /// 打开文档（`textDocument/didOpen`）。
    pub fn open_document(&mut self, uri: &str, language_id: &str, version: i32, text: &str) {
        self.documents.insert(
            uri.to_string(),
            TextDocument::open(uri, language_id, version, text),
        );
        self.propagate_document_text(uri, text);
        log::debug!("document opened: {uri} (v{version}, {} bytes)", text.len());
    }

    /// 应用文档变更（`textDocument/didChange`）；文档未打开则忽略。
    pub fn change_document(
        &mut self,
        uri: &str,
        version: i32,
        changes: &[TextDocumentContentChangeEvent],
    ) {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.apply_changes(changes, version);
            let text = doc.text().to_string();
            self.propagate_document_text(uri, &text);
            log::debug!("document changed: {uri} (v{version})");
        } else {
            log::warn!("didChange for unopened document: {uri}");
        }
    }

    /// 关闭文档（`textDocument/didClose`）——失效语义覆盖，回落到磁盘。
    pub fn close_document(&mut self, uri: &str) {
        if self.documents.remove(uri).is_some() {
            for ws in &mut self.workspaces {
                if let Some((pkg_idx, file_id)) = ws.file_id_for_uri(uri) {
                    ws.packages_mut()[pkg_idx]
                        .semantic
                        .invalidate_source(file_id);
                }
            }
            log::debug!("document closed: {uri}");
        }
    }

    /// 将开放文档文本传播到所属包语义索引的源码缓存（Document 统一）。
    fn propagate_document_text(&mut self, uri: &str, text: &str) {
        for ws in &mut self.workspaces {
            if let Some((pkg_idx, file_id)) = ws.file_id_for_uri(uri) {
                ws.packages_mut()[pkg_idx]
                    .semantic
                    .set_source_text(file_id, text);
            }
        }
    }

    /// 查询已打开的文档；未打开返回 `None`。
    pub fn document(&self, uri: &str) -> Option<&TextDocument> {
        self.documents.get(uri)
    }

    /// 查询已打开文档的可变引用（供 provider 触发惰性 `SyntaxTree` 重解析）。
    pub fn document_mut(&mut self, uri: &str) -> Option<&mut TextDocument> {
        self.documents.get_mut(uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcgr::context_manifest::{
        ContextManifest, CrateDagSummary, CrateModule, L0ProjectOverview, L1ModuleSurface,
        ProjectKind,
    };
    use arcgr::{
        ArcgrFile, FileEntry, ReferenceContext, ReferenceEntry, SymbolEntry, SymbolKind, TypeSig,
    };

    #[test]
    fn workspace_state_holds_root() {
        let ws = WorkspaceState::new(PathBuf::from("/proj/a"));
        assert_eq!(ws.root(), Path::new("/proj/a"));
    }

    #[test]
    fn workspace_manager_starts_empty() {
        let mgr = WorkspaceManager::new();
        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);
    }

    #[test]
    fn add_workspace_returns_index_and_increments_len() {
        let mut mgr = WorkspaceManager::new();
        let i0 = mgr.add_workspace(PathBuf::from("/proj/a"));
        let i1 = mgr.add_workspace(PathBuf::from("/proj/b"));
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_eq!(mgr.len(), 2);
        assert_eq!(mgr.workspaces()[0].root(), Path::new("/proj/a"));
        assert_eq!(mgr.workspaces()[1].root(), Path::new("/proj/b"));
    }

    #[test]
    fn clear_empties_workspaces() {
        let mut mgr = WorkspaceManager::new();
        mgr.add_workspace(PathBuf::from("/proj/a"));
        mgr.add_workspace(PathBuf::from("/proj/b"));
        assert_eq!(mgr.len(), 2);

        mgr.clear();
        assert!(mgr.is_empty());
    }

    #[test]
    fn definition_at_resolves_cross_package_external_reference() {
        // 主包：对 "ExternalType" 有一个指向包外 symbol_id 的引用（外部引用）
        let mut main = ArcgrFile::new();
        main.file_table
            .entries
            .push(FileEntry::new(1, "/proj/src/main.as".into(), 0, 1));
        main.reference_table.entries.push(ReferenceEntry::new(
            1,
            99, // 外部目标（本包不存在）
            1,
            0,
            12,
            ReferenceContext::TypeAnnotation,
        ));
        let mut main_idx = SemanticIndex::from_arcgr(main, PathBuf::from("/proj"));
        main_idx.inject_source(1, "ExternalType\n");

        // 依赖包 lib：定义 Public 符号 ExternalType
        let mut lib = ArcgrFile::new();
        lib.file_table
            .entries
            .push(FileEntry::new(1, "/proj/vendor/lib.as".into(), 0, 1));
        lib.symbol_table.entries.push(SymbolEntry::new(
            7,
            "ExternalType",
            SymbolKind::Class,
            Visibility::Public,
            1,
            0,
            12,
            TypeSig::Named {
                fully_qualified_name: "ExternalType".into(),
                generic_args: vec![],
            },
            None,
        ));
        let mut lib_idx = SemanticIndex::from_arcgr(lib, PathBuf::from("/proj"));
        lib_idx.inject_source(1, "ExternalType\n");

        let mut ws = WorkspaceState::new(PathBuf::from("/proj"));
        ws.set_semantic(main_idx);
        ws.add_dependency_index("lib", lib_idx);

        // 光标在主包 "ExternalType" 使用处 → 跨包跳到依赖包定义
        let loc = ws
            .definition_at(
                "file:///proj/src/main.as",
                Position {
                    line: 0,
                    character: 1,
                },
            )
            .expect("cross-package definition must resolve");
        assert_eq!(loc.uri, "file:///proj/vendor/lib.as");
        assert_eq!(
            loc.range.start,
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            loc.range.end,
            Position {
                line: 0,
                character: 12
            }
        );
    }

    #[test]
    fn definition_at_resolves_local_reference_within_package() {
        // 主包：本地引用（symbol_id 存在）→ 仍在主包内解析，不跨包
        let mut main = ArcgrFile::new();
        main.file_table
            .entries
            .push(FileEntry::new(1, "/proj/src/main.as".into(), 0, 1));
        main.symbol_table.entries.push(SymbolEntry::new(
            5,
            "Local",
            SymbolKind::Class,
            Visibility::Public,
            1,
            0,
            5,
            TypeSig::Named {
                fully_qualified_name: "Local".into(),
                generic_args: vec![],
            },
            None,
        ));
        main.reference_table.entries.push(ReferenceEntry::new(
            1,
            5, // 本地目标
            1,
            12,
            17,
            ReferenceContext::TypeAnnotation,
        ));
        let mut main_idx = SemanticIndex::from_arcgr(main, PathBuf::from("/proj"));
        main_idx.inject_source(1, "Local\n      Local\n");

        let mut ws = WorkspaceState::new(PathBuf::from("/proj"));
        ws.set_semantic(main_idx);

        // 光标在本地引用（第 1 行 "Local"）处 → 定义位置仍在主包第 0 行
        let loc = ws
            .definition_at(
                "file:///proj/src/main.as",
                Position {
                    line: 1,
                    character: 7,
                },
            )
            .expect("local definition must resolve");
        assert_eq!(loc.uri, "file:///proj/src/main.as");
        assert_eq!(
            loc.range.start,
            Position {
                line: 0,
                character: 0
            }
        );
    }

    #[test]
    fn open_document_propagates_text_to_semantic_index() {
        // 语义索引：file_id 1，源文本 "Old\n"（模拟磁盘旧内容）
        let mut file = ArcgrFile::new();
        file.file_table
            .entries
            .push(FileEntry::new(1, "/proj/src/main.as".into(), 0, 1));
        let mut idx = SemanticIndex::from_arcgr(file, PathBuf::from("/proj"));
        idx.inject_source(1, "Old\n");

        let mut mgr = WorkspaceManager::new();
        mgr.add_workspace(PathBuf::from("/proj"));
        mgr.workspaces_mut()[0].set_semantic(idx);

        let uri = "file:///proj/src/main.as";
        // 打开 3 行文档 → 覆盖语义源码 → line 2 可解析
        mgr.open_document(uri, "arc", 1, "A\nB\nC\n");
        let sem = mgr.workspaces_mut()[0].semantic_mut().unwrap();
        assert_eq!(
            sem.position_to_offset(
                1,
                Position {
                    line: 2,
                    character: 0
                }
            ),
            Some(4),
            "open document text must override semantic source"
        );

        // didClose → 失效覆盖 → 回落到磁盘（无真实文件 → 读取失败 → None）
        mgr.close_document(uri);
        let sem = mgr.workspaces_mut()[0].semantic_mut().unwrap();
        assert!(
            sem.position_to_offset(
                1,
                Position {
                    line: 0,
                    character: 0
                }
            )
            .is_none(),
            "closing must invalidate override and fall back to disk"
        );
    }

    fn sample_l0(name: &str) -> L0ProjectOverview {
        L0ProjectOverview {
            name: name.into(),
            kind: ProjectKind::Executable,
            version_major: 1,
            version_minor: 0,
            version_patch: 0,
            edition: 2024,
            arc_abi_version: 1,
            llvm_version: 22,
            target_triple: "x86_64-pc-windows-msvc".into(),
            dependencies: vec![],
            capabilities: vec![],
            namespaces: vec![],
            architecture_redlines: vec![],
            crate_dag_summary: CrateDagSummary::new(1, 0),
        }
    }

    fn sample_l1() -> L1ModuleSurface {
        L1ModuleSurface {
            crates: vec![CrateModule {
                crate_id: 0,
                name: "myapp".into(),
                path: "src".into(),
                responsibility: String::new(),
                public_apis: vec![],
                namespaces: vec![],
            }],
            dag_edges: vec![],
        }
    }

    #[test]
    fn load_project_from_arc_toml() {
        let dir = std::env::temp_dir().join(format!("arc-ws-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("vendor/foo")).unwrap();

        // arc.toml：主包 myapp + 依赖 foo
        std::fs::write(
            dir.join("arc.toml"),
            "[package]\nname = \"myapp\"\nkind = \"executable\"\n\n\
             [[dependencies]]\nname = \"foo\"\npath = \"vendor/foo\"\n",
        )
        .unwrap();

        // 主包源 + .arcgr（外部引用，含 ContextManifest L0）
        let main_as = dir.join("src/main.as");
        std::fs::write(&main_as, "ExternalType\n").unwrap();
        let main_path = main_as.display().to_string().replace('\\', "/");
        let mut main = ArcgrFile::new();
        main.file_table
            .entries
            .push(FileEntry::new(1, main_path, 0xAA, 1));
        main.reference_table.entries.push(ReferenceEntry::new(
            1,
            99,
            1,
            0,
            12,
            ReferenceContext::TypeAnnotation,
        ));
        main.reference_graph = Default::default();
        main.context_manifest = Some(ContextManifest::new(sample_l0("myapp"), sample_l1()));
        std::fs::write(dir.join("myapp.arcgr"), main.serialize()).unwrap();

        // 依赖包源 + .arcgr（公共 ExternalType）
        let lib_as = dir.join("vendor/foo/lib.as");
        std::fs::write(&lib_as, "ExternalType\n").unwrap();
        let lib_path = lib_as.display().to_string().replace('\\', "/");
        let mut lib = ArcgrFile::new();
        lib.file_table
            .entries
            .push(FileEntry::new(1, lib_path, 0xBB, 1));
        lib.symbol_table.entries.push(SymbolEntry::new(
            7,
            "ExternalType",
            SymbolKind::Class,
            Visibility::Public,
            1,
            0,
            12,
            TypeSig::Named {
                fully_qualified_name: "ExternalType".into(),
                generic_args: vec![],
            },
            None,
        ));
        lib.reference_table = Default::default();
        lib.reference_graph = Default::default();
        std::fs::write(dir.join("vendor/foo/foo.arcgr"), lib.serialize()).unwrap();

        // arc.toml 驱动加载
        let mut ws = WorkspaceState::new(dir.clone());
        ws.load_project(&dir.join("arc.toml"))
            .expect("load_project");

        // 项目元数据
        let proj = ws.project().expect("project metadata");
        assert_eq!(proj.name, "myapp");
        assert_eq!(proj.kind, ProjectKind::Executable);
        assert_eq!(proj.dependencies.len(), 1);
        assert_eq!(proj.dependencies[0].name, "foo");
        // ContextManifest L0 并入 Project
        assert_eq!(
            proj.manifest.as_ref().map(|m| m.name.as_str()),
            Some("myapp")
        );

        // 主包 + 依赖包均已加载
        assert_eq!(ws.packages.len(), 2, "main + dependency package");

        // 跨包 Goto Definition（经 arc.toml 驱动加载生效）
        let main_uri = format!("file://{}", main_as.display());
        let loc = ws
            .definition_at(
                &main_uri,
                Position {
                    line: 0,
                    character: 1,
                },
            )
            .expect("cross-package definition via arc.toml");
        assert!(loc.uri.ends_with("lib.as"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
