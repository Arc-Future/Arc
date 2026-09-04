//! Multi-file source loading: resolve `using` paths and merge translation units.
//!
//! # C# 铁律：namespace 解析
//!
//! **文件内 `namespace` 声明即真理。** 命名空间与目录结构解耦。
//! 编译器启动时递归扫描 `std/` 下所有 `.as` 文件，提取 `namespace` 声明
//! 构建全局索引。`using Arc.Net.P2P;` 直接查索引返回声明该 namespace 的文件列表。
//! 新增子库**零编译器修改**——在 `std/` 下创建目录 + 文件声明 `namespace` 即可。
//!
//! ## 设计原则
//!
//! 1. **零硬编码映射**：不存在 `STD_SUBLIB_ROOTS` / `STD_NAMESPACE_FLATTENED_DIRS` 等硬编码表。
//!    所有 namespace→文件 映射由文件自身的 `namespace` 声明唯一决定。
//! 2. **目录仅作物理组织**：`std/Net/P2P/` 目录结构仅是文件组织方式，
//!    不影响命名空间归属。文件声明 `namespace Arc.Foo;` 即归属 `Arc.Foo`，与目录无关。
//! 3. **不校验目录一致性**：`expected_namespace()` / `validate_namespace()` 已删除。
//!    文件声明即真理，编译器不根据目录路径推断或校验命名空间。
//! 4. **非 `Arc` 前缀路径**：仍按 `base_dir` 相对目录解析（项目本地文件），不受铁律约束。

use ast::*;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::manifest::{find_arc_manifest, ArcManifest};

/// 全局命名空间索引：namespace 路径（"."连接）→ 声明该 namespace 的文件列表。
type NamespaceIndex = IndexMap<String, Vec<PathBuf>>;

/// 构建全局 namespace 索引：递归扫描 std 根，提取每个 `.as` 文件的 `namespace` 声明。
fn build_namespace_index(std_root: &Path) -> NamespaceIndex {
    let mut index: NamespaceIndex = IndexMap::new();
    index_directory(std_root, &mut index);
    index
}

/// 扫描目录下所有 `.as` 文件并加入 namespace 索引。
fn index_directory(root: &Path, index: &mut NamespaceIndex) {
    if !root.is_dir() {
        return;
    }
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "as") {
                    if let Some(ns) = extract_namespace_from_file(&path) {
                        let key = ns.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(".");
                        index.entry(key).or_default().push(path);
                    }
                }
            }
        }
    }
}

/// 从 `.as` 文件快速提取 `namespace` 声明（轻量扫描，不完整解析 AST）。
fn extract_namespace_from_file(path: &Path) -> Option<Vec<Ident>> {
    let source = fs::read_to_string(path).ok()?;
    let mut chars = source.chars().peekable();
    let mut token = String::new();
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch == '/' {
            chars.next();
            if chars.peek() == Some(&'/') {
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            break; /* block comment not supported in extractor */
        }
        token.clear();
        while let Some(&c) = chars.peek() {
            if c.is_alphanumeric() || c == '_' || c == '.' {
                token.push(c);
                chars.next();
            } else {
                break;
            }
        }
        if token == "namespace" {
            let mut ns = String::new();
            while let Some(&c) = chars.peek() {
                if c == ';' || c == '{' {
                    chars.next();
                    break;
                }
                if !c.is_whitespace() {
                    ns.push(c);
                }
                chars.next();
            }
            let names: Vec<Ident> = ns
                .split('.')
                .filter(|s| !s.is_empty())
                .map(Ident::from)
                .collect();
            return if names.is_empty() { None } else { Some(names) };
        }
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ';' || c == '{' {
                if c == ';' || c == '{' {
                    chars.next();
                }
                break;
            }
            chars.next();
        }
    }
    None
}

/// 命名空间索引查找：`using Arc.Net.P2P;` → 返回声明该 namespace 的所有文件。
fn namespace_files<'a>(ns_path: &[Ident], index: &'a NamespaceIndex) -> Option<&'a [PathBuf]> {
    let key = ns_path
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(".");
    index.get(&key).map(|v| v.as_slice())
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parse error in {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("import not found: {path} (resolved to {resolved})")]
    NotFound { path: String, resolved: PathBuf },
    #[error("circular import: {path}")]
    Circular { path: PathBuf },
    #[error("namespace mismatch in {path}: expected `{expected}`, found `{found}`")]
    NamespaceMismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },
    #[error("duplicate public definition `{name}` from {first} and {second}")]
    DuplicatePublic {
        name: String,
        first: PathBuf,
        second: PathBuf,
    },
    #[error("package name `{package}` does not match entry namespace root `{found}` (expected `{expected}`)")]
    PackageNamespaceMismatch {
        package: String,
        expected: String,
        found: String,
    },
    /// RFC 025 M2：用户项目 `using` 了非隐式 std 子库，且该包不在
    /// `[dependencies]` 的传递闭包内。
    #[error(
        "package `{package}` is not in the [dependencies] closure before `using {using_path}` \
         (declare it directly, or depend on a package that transitively requires it; Arc is implicit)"
    )]
    UndeclaredDependency { package: String, using_path: String },
    /// RFC 025 M2+：包图传递闭包失败（未知根或缺失边）。
    #[error("{message}")]
    DependencyClosure { message: String },
}

/// A merged program ready for HIR lowering.
#[derive(Debug)]
pub struct CompileUnit {
    pub program: Program,
    pub root: PathBuf,
    /// 文件路径 → FileId 映射（RFC 024 M0：多文件 span 定位）。
    pub file_registry: FileRegistry,
    /// `.ani` 契约文件解析结果（RFC 016 M1）。
    ///
    /// 契约不进入 `program.items`，由管线层直接传递给 typeck/codegen，
    /// 跳过 hir lowering（参见 `hir::builder::lower_item` 的 `Item::Native` 断言）。
    pub native_modules: Vec<NativeModule>,
    /// 跨包外部符号（typeck 语义视图）。
    ///
    /// 由管线层传给 `TypeChecker::register_external_symbols` 注册到
    /// `TypeRegistry`，使 typeck 能解析跨包类型引用而不重解析源码。
    /// 源码打包路径始终置空（依赖源码合并进单一编译单元）。
    pub external_symbols: Vec<typeck::ExternalSymbolEntry>,
    /// RFC 025 M2：FileId → 包名（子库 `arc.toml` / 项目清单）。
    ///
    /// 供 typeck 跨包 `internal` 硬拒绝；由 loader 在合并 TU 时填充。
    pub file_packages: std::collections::HashMap<ast::FileId, String>,
    /// RFC 025 M2+：包名 → InternalsVisibleTo 列表（对标 C# `[assembly: InternalsVisibleTo]`）。
    ///
    /// 由 loader 从包图（std 子库）+ 入口项目清单收集；typeck 据此放行
    /// 指定包访问本包 `internal`（测试程序 / ARML 合并场景）。
    pub internals_visible_to: std::collections::HashMap<String, Vec<String>>,
    /// RFC 025 M2：入口项目包名（`Arc` 隐式引入的对照基准）。
    pub entry_package: String,
}

/// 编译期文件注册表：FileId ↔ 文件路径映射。
/// FileId 从 1 开始递增（0 保留给 DUMMY）。
#[derive(Debug, Default)]
pub struct FileRegistry {
    paths: Vec<PathBuf>,
}

impl FileRegistry {
    pub fn new() -> Self {
        Self { paths: Vec::new() }
    }

    /// 分配新 FileId（从 1 开始）。
    pub fn allocate(&mut self, path: PathBuf) -> FileId {
        self.paths.push(path);
        self.paths.len() as FileId
    }

    /// 查询 FileId 对应路径。
    pub fn path_of(&self, file_id: FileId) -> Option<&Path> {
        if file_id == 0 || file_id as usize > self.paths.len() {
            None
        } else {
            Some(&self.paths[(file_id - 1) as usize])
        }
    }

    /// RFC 024 D2 Phase 0 #5：按路径反查 FileId（用于 `collect_exports` 过滤
    /// 用户源码类型——仅导出在 `arc publish <entry>` 入口文件中声明的类型，
    /// 跳过通过 `using` 加载的 std 库依赖与外部 `.aopkg` 注册的符号）。
    ///
    /// 比较使用规范化绝对路径：调用方应传入 `path.canonicalize()` 的结果，
    /// 与 `load_compile_unit_inner` 中 `canonical_root` 的注册路径一致。
    /// 未命中返回 `None`（包括空 registry、file_id=0 占位等场景）。
    pub fn find_file_id(&self, path: &Path) -> Option<FileId> {
        self.paths
            .iter()
            .position(|p| p == path)
            .map(|i| (i + 1) as FileId)
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

struct ParsedFile {
    path: PathBuf,
    program: Program,
}

pub fn find_workspace_root(start: &Path) -> PathBuf {
    let mut dir = start
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| start.to_path_buf());
    loop {
        if dir.join("std").is_dir() && dir.join("Cargo.toml").is_file() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    // 兜底：源码不在任一 workspace 内（如 CodeAct 临时单文件）时，回退到 SDK
    // 捆绑 std（安装态 `<sdk>/lib/std`；开发态仓库 `<repo>/std`，对齐
    // [`crate::core_arc`] 的定位链），使 `arc build /tmp/foo.as` 从任意目录也
    // 能解析 `using Arc`。
    if let Some(embedded) = embedded_workspace_root() {
        if embedded.join("std").is_dir() {
            return embedded;
        }
    }
    start
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 编译器源码树自带 std 的兜底 workspace 根（SDK 捆绑 std 的父目录）。
///
/// 安装态 `<sdk>/lib/std` → `<sdk>/lib`；开发态仓库 `<repo>/std` → `<repo>`。
/// 由 [`codegen::sdk_layout::sdk_std_root`] 运行期自定位，取代编译期固化路径。
fn embedded_workspace_root() -> Option<PathBuf> {
    codegen::sdk_layout::sdk_std_root().and_then(|s| s.parent().map(Path::to_path_buf))
}

pub fn load_compile_unit(root: &Path) -> Result<CompileUnit, LoadError> {
    if root.is_dir() {
        return load_compile_unit_from_dir(root);
    }
    load_compile_unit_inner(&[root.to_path_buf()])
}

/// 从项目目录加载所有 `.as` 文件（递归扫描，排除 `obj/` / `bin/`）。
///
/// 对标 `dotnet test` 的项目级测试发现：将项目中所有源码合并为一个
/// [`CompileUnit`]，供 QIF 全局收集 `[Fact]`/`[Theory]` 方法。
pub fn load_compile_unit_from_dir(project_root: &Path) -> Result<CompileUnit, LoadError> {
    let roots = collect_as_files(project_root);
    if roots.is_empty() {
        return Err(LoadError::Parse {
            path: project_root.to_path_buf(),
            message: "no .as files found in project".to_string(),
        });
    }
    load_compile_unit_inner(&roots)
}

/// 递归收集目录下所有 `.as` 文件，排除 `obj/` / `bin/` 构建产物目录。
fn collect_as_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if fname == "obj" || fname == "bin" || fname == "target" || fname == ".git" {
                    continue;
                }
                files.extend(collect_as_files(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("as") {
                files.push(path);
            }
        }
    }
    files
}

/// 判断源码是否声明了文件级 `namespace X;`。
///
/// 用于单文件入口的同目录兄弟筛选：仅自动纳入声明了 namespace 的结构化项目源码，
/// 跳过无 namespace 的扁平/脚本文件（避免在共享 scratch 目录中误拉无关文件）。
fn declares_namespace(src: &str) -> bool {
    src.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("namespace ") && t.trim_end().ends_with(";")
    })
}

/// 判断入口文件是否位于生成物目录（`obj/` / `bin/` / `target/`，含任意层级）。
///
/// 用于单文件入口的兄弟扫描开关：codegen 合并产物（如 `obj/<config>/code/Program.as`）
/// 已把全部 `.g.as` 兄弟内容合并进入口文件，不应再走兄弟扫描，否则 partial class
/// 字段/方法重复定义。口径与 [`collect_as_files`] 排除 `obj/`/`bin/`/`target/` 一致。
fn root_under_output_dir(root: &Path) -> bool {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    canonical.components().any(|c| {
        matches!(c.as_os_str().to_str(), Some(name) if name == "obj" || name == "bin" || name == "target")
    })
}

fn load_compile_unit_inner(roots: &[PathBuf]) -> Result<CompileUnit, LoadError> {
    let first_root = roots.first().ok_or_else(|| LoadError::Parse {
        path: PathBuf::from("."),
        message: "no source files provided".to_string(),
    })?;
    // Canonicalize before workspace/discovery — find_workspace_root can't walk past
    // a relative path's implicit root (e.g. `./obj/Debug/code/Program.as` → stays at `.`).
    let canonical_root = first_root
        .canonicalize()
        .unwrap_or_else(|_| first_root.to_path_buf());
    let workspace = find_workspace_root(&canonical_root);
    // 项目根：从 root 文件向上找 arc.toml（对标 MSBuild .csproj）。
    // 多文件加载时 root_dir 必须是项目根，而非某个子文件的父目录，
    // 否则 GlobalUsings.as 和 namespace 索引都定位错误。
    let (root_dir, project_manifest) =
        if let Some((manifest_dir, m)) = find_arc_manifest(&canonical_root) {
            (manifest_dir, Some(m))
        } else {
            (
                canonical_root
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf(),
                None,
            )
        };

    // RFC 031 §8：`[std].path` 覆盖 → 完整 std 解析链（`[std].path` → SDK 捆绑
    // std → `ARC_STD_ROOT` → workspace 兜底），统一 std 根（namespace 索引 /
    // 包图 / UI 阴影）。
    let std_root = crate::manifest::resolve_effective_std_root(
        &workspace,
        project_manifest.as_ref().map(|_| root_dir.as_path()),
        project_manifest.as_ref().and_then(|m| m.std.as_ref()),
    );

    // C# 铁律：构建全局 namespace 索引（文件内 namespace 声明即真理）
    eprintln!("[LOAD] build_namespace_index start");
    let mut ns_index = build_namespace_index(&std_root);
    eprintln!("[LOAD] build_namespace_index done, dirs indexed");
    // RFC 032 Phase 4: 同时索引项目目录的 namespace 声明，
    // 确保项目内的 `using`（如 `using UnitTest.Core.Partial;`）可解析。
    // 排除 std/ 本身已被 build_namespace_index 索引。
    if workspace != root_dir && std_root != root_dir {
        index_directory(&root_dir, &mut ns_index);
    }
    // RFC 017：索引 `[dependencies]` path 依赖的源码目录（相对项目根），
    // 使 `using <dep>;` 可经依赖源码解析（源码打包：依赖源码合并进单一编译单元）。
    if let Some(m) = project_manifest.as_ref() {
        for spec in m.dependencies.values() {
            let dep_dir = root_dir.join(&spec.path);
            if dep_dir.is_dir() {
                index_directory(&dep_dir, &mut ns_index);
            }
        }
    }

    // Init pending with all root files.
    // 关键：多根加载时所有文件的 base_dir 统一为项目根（root_dir），
    // 确保非 Arc 命名空间的 `using`（如 `using UnitTest.Core.Partial;`）
    // 从项目根开始解析，而非从子目录相对解析。
    let mut pending: Vec<(PathBuf, PathBuf)> = Vec::new();
    let is_multi_root = roots.len() > 1;
    for root in roots {
        let cr = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let base = if is_multi_root {
            root_dir.clone()
        } else {
            cr.parent().unwrap_or(&root_dir).to_path_buf()
        };
        if !pending.iter().any(|(p, _)| p == &cr) {
            pending.push((cr, base));
        }
    }
    // RFC 003：项目根 `GlobalUsings.as` 优先纳入依赖图。
    let global_usings = root_dir.join("GlobalUsings.as");
    if global_usings.exists() {
        let g = global_usings
            .canonicalize()
            .unwrap_or_else(|_| global_usings.clone());
        // GlobalUsings.as 需要排在前面以便其 `global using` 先生效
        if !pending.iter().any(|(p, _)| p == &g) {
            pending.insert(0, (g, root_dir.clone()));
        }
    }
    // RFC 003 M2：`arc.toml` `[package].global_usings` 合成导入的依赖也纳入 pending。
    let synthetic_global_uses = enqueue_manifest_global_usings(
        project_manifest.as_ref(),
        &root_dir,
        &ns_index,
        &mut pending,
    )?;

    // 单文件入口：同目录兄弟 `.as` 文件一并纳入编译单元（多文件直接编译）。
    //
    // 对标 C# csproj「项目内全部 .cs 一并编译」——单文件只是编译入口，同目录兄弟
    // 文件定义的同命名空间类型/函数需一并加载，否则 `arc check/build/run <file>.as`
    // 会报 undefined type。子目录文件仍由 `using` 经 namespace 索引（resolve_use_deps）
    // 按需拉取。
    //
    // 仅纳入**声明了 namespace** 的兄弟文件：命名空间型文件是结构化项目源码；无
    // namespace 的扁平文件（如共享 scratch 目录中的独立脚本）不自动纳入，避免误拉
    // 同目录无关文件（如 target/e2e 遗留脚本）导致重复定义冲突。
    //
    // 保持 is_multi_root=false（单根语义）：base_dir 沿用入口文件父目录，且 namespace
    // 校验照常执行——兄弟文件属同一项目包，其 namespace 须匹配包根命名空间。
    //
    // 排除生成物目录（obj/ / bin/）：codegen 合并产物（如 obj/<config>/code/Program.as）
    // 已把全部 `.g.as` 兄弟文件内容合并进入口文件，若再经兄弟扫描自动纳入 `.g.as`
    // 会导致 partial class 字段重复定义。与 collect_as_files 的目录模式口径一致。
    let root_is_generated = root_under_output_dir(&roots[0]);
    if !is_multi_root && !root_is_generated {
        let root_file = roots[0].canonicalize().unwrap_or_else(|_| roots[0].clone());
        if root_file.is_file() {
            if let Some(dir) = root_file.parent() {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("as")
                        {
                            let pc = path.canonicalize().unwrap_or_else(|_| path.clone());
                            if pc != root_file && !pending.iter().any(|(p, _)| p == &pc) {
                                if let Ok(src) = std::fs::read_to_string(&path) {
                                    if declares_namespace(&src) {
                                        pending.push((pc, dir.to_path_buf()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    let mut file_registry = FileRegistry::new();

    while let Some((path, base_dir)) = pending.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }

        let source = fs::read_to_string(&path).map_err(|source| LoadError::Read {
            path: path.clone(),
            source,
        })?;
        let file_id = file_registry.allocate(path.clone());
        let program = parse::Parser::parse_program_in_file(&source, file_id).map_err(|e| {
            LoadError::Parse {
                path: path.clone(),
                message: e.to_string(),
            }
        })?;

        // C# 铁律：文件内 namespace 声明即真理，不校验与目录结构的对应关系
        // （已删除 validate_namespace / expected_namespace）

        for use_item in collect_use_items(&program.items) {
            for dep in resolve_use_deps(&use_item.path, &base_dir, &ns_index)? {
                if !dep.exists() {
                    return Err(LoadError::NotFound {
                        path: use_item
                            .path
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("."),
                        resolved: dep,
                    });
                }
                // 统一 canonicalize 依赖路径以匹配 seen HashSet 中的路径格式。
                // Windows 上 fs::read_dir 返回的路径可能与 canonicalize 后的路径
                // 格式不同（如 \\?\ 前缀差异），导致 seen.contains() 误判为未见过
                // 而重复加载同一文件。
                let dep = dep.canonicalize().unwrap_or_else(|_| dep.clone());
                if seen.contains(&dep) {
                    continue;
                }
                let dep_base = dep.parent().unwrap_or(&base_dir).to_path_buf();
                pending.push((dep, dep_base));
            }
        }

        parsed.push(ParsedFile { path, program });
    }

    let mut merged_items = Vec::new();
    let mut public_defs: HashMap<String, PathBuf> = HashMap::new();

    // RFC 003 M2：合成 `global using` 排在最前，与 GlobalUsings.as 同语义并入 TU。
    for use_item in synthetic_global_uses {
        merged_items.push(Spanned::new(Item::Use(use_item), Span::DUMMY));
    }

    // RFC 037：UI codegen inline 框架类型时，std/UI/Core 依赖副本与 root 同名则跳过合并。
    let mut root_type_names = std::collections::HashSet::new();
    if let Some(root_file) = parsed.iter().find(|f| f.path == canonical_root) {
        collect_top_level_type_names(&root_file.program.items, &mut root_type_names);
    }
    let std_ui_root = std_root.join("UI");
    let canonical_std_ui_root = std_ui_root.canonicalize().unwrap_or(std_ui_root);

    for file in &parsed {
        if file.path == canonical_root {
            continue;
        }
        let shadow_std_ui_dupes = file.path.starts_with(&canonical_std_ui_root);
        for item in flatten_dependency_items(&file.program.items) {
            let merged_item = if shadow_std_ui_dupes {
                filter_shadowed_dependency_item(item, &root_type_names)
            } else {
                Some(item)
            };
            if let Some(filtered) = merged_item {
                check_duplicate_public(&filtered, &file.path, &mut public_defs)?;
                merged_items.push(filtered);
            }
        }
    }
    if let Some(root_file) = parsed.iter().find(|f| f.path == canonical_root) {
        for item in flatten_dependency_items(&root_file.program.items) {
            merged_items.push(item);
        }
    }

    // RFC 025 M2：先发现包图与入口包名，供 namespace 校验与 file_packages 共用。
    // RFC 039 §1.7.1 P1：discover = std + 入口 path 依赖吸收，跨库聚合包可入图。
    let entry_deps = project_manifest
        .as_ref()
        .map(|m| &m.dependencies)
        .cloned()
        .unwrap_or_default();
    let package_graph = crate::package_graph::PackageGraph::discover_with_std(
        &workspace,
        &root_dir,
        &entry_deps,
        Some(&std_root),
    )
    .map_err(|message| LoadError::Parse {
        path: std_root.clone(),
        message,
    })?;
    let entry_package = project_manifest
        .as_ref()
        .map(|m| m.package.name.clone())
        .unwrap_or_else(|| "App".to_string());

    if let Some(manifest) = project_manifest.as_ref() {
        // 多根加载时跳过包命名空间校验：项目内所有文件的命名空间不需要
        // 与 package.name 一致（对标 C# 程序集 vs 根命名空间解耦）。
        let skip_ns_validation = roots.len() > 1;
        if !skip_ns_validation {
            let root_program = parsed
                .iter()
                .find(|f| f.path == canonical_root)
                .map(|f| &f.program)
                .expect("root file must be parsed");
            validate_entry_package_namespace(manifest, root_program)?;
        }
        // 规范化 std_root 以匹配 parsed 中已 canonicalize 的文件路径
        let canonical_std_root = std_root.canonicalize().unwrap_or_else(|_| std_root.clone());
        for file in &parsed {
            if file.path == canonical_root {
                continue;
            }
            if file.path.starts_with(&canonical_std_root) {
                continue;
            }
            // RFC 003：GlobalUsings.as 可无 namespace（仅 global using / using）。
            if file.path.file_name().and_then(|n| n.to_str()) == Some("GlobalUsings.as") {
                continue;
            }
            if skip_ns_validation {
                continue; // 多根加载跳过项目内 namespace 校验
            }
            // RFC 025 M2：附属 path 包（最近 arc.toml 包名 ≠ 入口包）按其自身
            // namespace 根校验，不套用入口项目包名。
            let file_pkg = package_graph.package_for_file(&file.path, &entry_package);
            if file_pkg != entry_package {
                if let Some((_, dep_manifest)) = find_arc_manifest(&file.path) {
                    validate_library_package_namespace(&dep_manifest, &file.path, &file.program)?;
                }
                continue;
            }
            validate_library_package_namespace(manifest, &file.path, &file.program)?;
        }
    }

    let native_modules = load_native_contracts(&workspace, &mut file_registry)?;

    let mut file_packages = std::collections::HashMap::new();
    for file_id in 1..=file_registry.len() as FileId {
        if let Some(path) = file_registry.path_of(file_id) {
            let pkg = package_graph.package_for_file(path, &entry_package);
            file_packages.insert(file_id, pkg);
        }
    }

    // RFC 038 M2-G4：合并项按「包依赖拓扑序」重排——依赖包在前、消费者在后。
    //
    // typeck 对 `program.items` 做顺序单趟处理：依赖包的 `static class` 泛型扩展
    // 方法模板（如 Arc.DI `ServiceCollectionExtensions.AddSingleton<TService>`）在
    // `check_static_class` 时注册进 `extension_fn_templates`；消费者（如 Arc.Logging
    // 的 `AddSingleton<ILoggerFactory>(factory)`）方法体若被**先**检查，其跨库泛型
    // 扩展调用点查不到模板 → `undefined name`。按包依赖拓扑重排（依赖先于被依赖，
    // 入口包最后）保证模板先注册、调用点可单态化。入口/外部包不在图内 → 排最后。
    let package_topo_rank =
        package_topo_ranks(&package_graph, &file_packages.values().cloned().collect());
    merged_items.sort_by_key(|item| {
        let pkg = file_packages
            .get(&item.span.file_id)
            .map(|s| s.as_str())
            .unwrap_or(&entry_package);
        package_topo_rank.get(pkg).copied().unwrap_or(usize::MAX)
    });

    // RFC 026 M2+：收集包图（std 子库）中各包的 InternalsVisibleTo 声明；
    // 入口项目清单自身的声明也并入（用户库包可对测试项目开放 internal）。
    let mut internals_visible_to = std::collections::HashMap::new();
    for (name, node) in &package_graph.packages {
        if !node.internals_visible_to.is_empty() {
            internals_visible_to.insert(name.clone(), node.internals_visible_to.clone());
        }
    }
    if let Some(manifest) = project_manifest.as_ref() {
        if !manifest.package.internals_visible_to.is_empty() {
            internals_visible_to.insert(
                manifest.package.name.clone(),
                manifest.package.internals_visible_to.clone(),
            );
        }
    }

    // RFC 025 M2：入口项目文件的 `using` 须声明非 Arc 的 std 子库依赖。
    if let Some(manifest) = project_manifest.as_ref() {
        validate_explicit_std_dependencies(
            &parsed,
            &canonical_root,
            &package_graph,
            manifest,
            &entry_package,
            &file_packages,
            &file_registry,
        )?;
    }

    Ok(CompileUnit {
        program: Program {
            items: merged_items,
        },
        root: canonical_root,
        file_registry,
        native_modules,
        external_symbols: Vec::new(),
        file_packages,
        internals_visible_to,
        entry_package,
    })
}

/// RFC 026 M2+：校验入口包源文件的 `using` 是否落在依赖传递闭包内。
///
/// `Arc` 隐式引入；其余在包图中的子库须出现在项目 `[dependencies]` **或其传递
/// 依赖闭包**中（例如仅声明 `Arc.Net.P2P` 即可 `using Arc.Security`）。
/// 仅检查入口包文件（不含 std 子库自身的传递 `using`）。
fn validate_explicit_std_dependencies(
    parsed: &[ParsedFile],
    canonical_root: &Path,
    package_graph: &crate::package_graph::PackageGraph,
    manifest: &ArcManifest,
    entry_package: &str,
    file_packages: &std::collections::HashMap<FileId, String>,
    file_registry: &FileRegistry,
) -> Result<(), LoadError> {
    let allowed = package_graph
        .allowed_for_entry(&manifest.dependencies)
        .map_err(|e| LoadError::DependencyClosure {
            message: e.to_string(),
        })?;
    for file in parsed {
        let Some(file_id) = file_registry.find_file_id(&file.path) else {
            continue;
        };
        let Some(pkg) = file_packages.get(&file_id) else {
            continue;
        };
        if pkg != entry_package {
            continue;
        }
        // 根文件与项目内文件均检查；std 已由 pkg != entry 过滤。
        let _ = canonical_root;
        for use_item in collect_use_items(&file.program.items) {
            let using_path = use_item
                .path
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(".");
            let Some(matched) = package_graph.match_namespace(&using_path) else {
                continue;
            };
            if crate::package_graph::PackageGraph::is_implicit_package(&matched.name) {
                continue;
            }
            // 自身包豁免：包内文件 `using` 自己包的根/子命名空间（如 std/Net/Core
            // 文件 `using Arc.Net;`）是合法书写形态——目录与命名空间解耦下，
            // 子目录文件引用包根类型不可能（也无必要）向自身声明依赖。
            if matched.name == entry_package {
                continue;
            }
            if allowed.contains(&matched.name) {
                continue;
            }
            return Err(LoadError::UndeclaredDependency {
                package: matched.name.clone(),
                using_path,
            });
        }
    }
    Ok(())
}

/// 扫描编译器内置契约目录下的 `*.ani` 契约文件并解析（RFC 016 M1）。
///
/// 契约是**编译期输入**，由编译器侧归属：内置契约随 SDK 分发（安装态
/// `<sdk>/lib/native`；开发态仓库 `crates/arc/native`，运行期自定位）。workspace
/// 级 `native/` 目录仍作为用户项目自定义契约的补充来源（同模块名以用户项目
/// 覆盖内置）。两目录均不存在时返回空向量（非错误）。文件按路径名排序以保证
/// 确定性。
/// 每个 `.ani` 文件分配独立 FileId，支持跨文件 span 定位。
pub fn load_native_contracts(
    workspace: &Path,
    file_registry: &mut FileRegistry,
) -> Result<Vec<NativeModule>, LoadError> {
    // 编译器内置契约：随 SDK 分发（安装态 `lib/native`，开发态 `crates/arc/native`）。
    let builtin_dir = codegen::sdk_layout::sdk_native_dir();
    let mut modules = scan_contract_dir(&builtin_dir, file_registry)?;

    // 用户项目自定义契约（workspace 级 `native/`），同模块名覆盖内置。
    let project_dir = workspace.join("native");
    if project_dir.is_dir() && project_dir != builtin_dir {
        let project_modules = scan_contract_dir(&project_dir, file_registry)?;
        for m in project_modules {
            if let Some(slot) = modules.iter_mut().find(|e| e.name == m.name) {
                *slot = m;
            } else {
                modules.push(m);
            }
        }
    }

    // 按模块名排序，保证跨目录合并后的确定性。
    modules.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(modules)
}

/// 扫描单个契约目录下的 `*.ani` 文件并解析为 [`NativeModule`]。
fn scan_contract_dir(
    native_dir: &Path,
    file_registry: &mut FileRegistry,
) -> Result<Vec<NativeModule>, LoadError> {
    if !native_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(native_dir)
        .map_err(|source| LoadError::Read {
            path: native_dir.to_path_buf(),
            source,
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "ani"))
        .collect();
    entries.sort();

    let mut modules = Vec::new();
    for path in entries {
        let source = fs::read_to_string(&path).map_err(|source| LoadError::Read {
            path: path.clone(),
            source,
        })?;
        let file_id = file_registry.allocate(path.clone());
        let module =
            parse::Parser::parse_native_module(&source, file_id).map_err(|e| LoadError::Parse {
                path: path.clone(),
                message: e.to_string(),
            })?;
        // RFC 016 M4（用户裁决简化 2026-08-03）：契约内 `library` 相对路径的
        // **基准 = 执行程序根目录**（`-o` 输出可执行文件所在目录），由 codegen
        // 在编译期解析为绝对路径。此处**保持相对原样**，不再按 workspace 根解析
        // ——基准统一收敛到 codegen 单一事实来源（`build_runtime_infos` /
        // `module_lib_search_paths` / `effective_native_lib_paths` 的调用方在
        // `compile_via_llvm_ir` 等入口先按 `output.parent()` 解析）。
        // RFC 016（native 源实现增补）：`source` 的**基准 = 本 `.ani` 契约文件所在
        // 目录**——C 源码是随项目编译纳入的输入，非部署期库目录；载入时据此解析
        // 为绝对路径，codegen 直接消费（无需再知基准）。
        //
        // **同目录同名回退发现**（显式声明缺失时的处理规则）：`.ani` 未声明
        // `source` 也未声明 `library` 时，按契约文件所在目录查找同名词源/词库。
        // 该模块名在契约目录存在同名 `.c` → 回退为源实现（`source`）；否则存在
        // 同名平台库变体 → 回退为从该目录链接（`library` 填契约目录，作为 -L 与
        // 运行时候选目录）。显式声明（`source`/`library`）优先于回退；二者皆空且
        // 无配对文件 → 保持原设计（经全局 `ani-native-lib` 搜索列表 / 系统路径）。
        let mut module = module;
        if let Some(src) = module.source.take() {
            module.source = Some(
                path.parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(src),
            );
        } else if module.library.is_none() {
            apply_same_dir_native_fallback(&mut module, &path);
        }
        modules.push(module);
    }
    Ok(modules)
}

/// 同目录同名配对回退发现（RFC 016，显式声明缺失时的处理规则）。
///
/// `.ani` 未声明 `source` 也未声明 `library` 时，按契约文件所在目录查找同名词源
/// /词库，命中即把该模块判定为本地配对实现：
/// - 同目录同名 `.c` → 回退为**源实现**（`source` 填该 C 源绝对路径，编译器经
///   `prepare_user_native_objects` 编译链接、跳过外部 -l/验证）。
/// - 否则同目录存在同名**平台库变体** → 回退为**从该目录链接**（`library` 填契约
///   目录，作 -L 与运行时候选目录；`library` 相对基准仍由 codegen 按 exe_dir 处理，
///   绝对目录则原样保留）。
///
/// 显式声明优先于回退；二者皆空且无配对文件 → 保持原设计（经全局 `ani-native-lib`
/// 搜索列表 / 系统路径链接）。平台库名变体尽力探测（DLL/so/dylib/lib/a），不依赖
/// 编译目标平台，命中以真实文件存在为准。
fn apply_same_dir_native_fallback(module: &mut NativeModule, contract: &Path) {
    let dir = contract.parent().unwrap_or_else(|| Path::new("."));
    let Some(stem) = contract
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
    else {
        return;
    };

    // 1) 同目录同名 `.c` → 源实现模块。
    let c_src = dir.join(format!("{stem}.c"));
    if c_src.is_file() {
        module.source = Some(c_src);
        return;
    }

    // 2) 同目录同名平台库（DLL/so/dylib/lib/a）→ 从契约目录链接。
    let names = [
        format!("{stem}.dll"),    // Windows
        format!("lib{stem}.dll"), // MinGW 命名变体
        format!("lib{stem}.so"),  // Linux
        format!("{stem}.so"),
        format!("lib{stem}.dylib"), // macOS
        format!("{stem}.dylib"),
        format!("{stem}.lib"),  // MSVC 导入库
        format!("lib{stem}.a"), // 静态库
        format!("{stem}.a"),
    ];
    if names.iter().any(|n| dir.join(n).is_file()) {
        module.library = Some(dir.to_path_buf());
    }
}

fn flatten_dependency_items(items: &[Spanned<Item>]) -> Vec<Spanned<Item>> {
    let mut out = Vec::new();
    for item in items {
        match &item.node {
            Item::Use(_)
            | Item::Fn(_)
            | Item::Class(_)
            | Item::Struct(_)
            | Item::Interface(_)
            | Item::Enum(_)
            | Item::Namespace(_)
            | Item::Variant(_)
            | Item::Delegate(_) => out.push(item.clone()),
            // .ani 契约不参与 .as 依赖合并；由管线层单独路由到 typeck/codegen。
            Item::Native(_) => {}
        }
    }
    out
}

fn collect_top_level_type_names(
    items: &[Spanned<Item>],
    out: &mut std::collections::HashSet<String>,
) {
    for item in items {
        match &item.node {
            Item::Namespace(ns) => collect_top_level_type_names(&ns.items, out),
            other => {
                if let Some(name) = item_type_def_name(other) {
                    out.insert(name);
                }
            }
        }
    }
}

fn item_type_def_name(item: &Item) -> Option<String> {
    match item {
        Item::Class(c) => Some(c.name.to_string()),
        Item::Struct(s) => Some(s.name.to_string()),
        Item::Interface(i) => Some(i.name.to_string()),
        Item::Enum(e) => Some(e.name.to_string()),
        Item::Variant(v) => Some(v.name.to_string()),
        Item::Delegate(d) => Some(d.name.to_string()),
        _ => None,
    }
}

fn item_defines_shadowed_type(item: &Item, root_names: &std::collections::HashSet<String>) -> bool {
    item_type_def_name(item).is_some_and(|name| root_names.contains(&name))
}

fn filter_shadowed_dependency_item(
    item: Spanned<Item>,
    root_names: &std::collections::HashSet<String>,
) -> Option<Spanned<Item>> {
    match item.node {
        Item::Namespace(mut ns) => {
            let mut kept = Vec::new();
            for inner in ns.items {
                if let Some(filtered) = filter_shadowed_dependency_item(inner, root_names) {
                    kept.push(filtered);
                }
            }
            if kept.is_empty() {
                return None;
            }
            ns.items = kept;
            Some(Spanned::new(Item::Namespace(ns), item.span))
        }
        other => {
            if item_defines_shadowed_type(&other, root_names) {
                None
            } else {
                Some(Spanned::new(other, item.span))
            }
        }
    }
}

fn collect_use_items(items: &[Spanned<Item>]) -> Vec<UseItem> {
    let mut uses = Vec::new();
    for item in items {
        match &item.node {
            Item::Use(u) => uses.push(u.clone()),
            Item::Namespace(ns) => uses.extend(collect_use_items(&ns.items)),
            _ => {}
        }
    }
    uses
}

/// RFC 003 M2：将 `arc.toml` `[package].global_usings` 合成 `UseItem`，
/// 并把对应依赖文件加入 pending（与 `GlobalUsings.as` 同路径解析）。
fn enqueue_manifest_global_usings(
    manifest: Option<&ArcManifest>,
    root_dir: &Path,
    ns_index: &NamespaceIndex,
    pending: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<Vec<UseItem>, LoadError> {
    let Some(manifest) = manifest else {
        return Ok(Vec::new());
    };
    let mut synthetic = Vec::new();
    for path_str in &manifest.package.global_usings {
        let path = parse_global_using_path(path_str)?;
        for dep in resolve_use_deps(&path, root_dir, ns_index)? {
            if !dep.exists() {
                return Err(LoadError::NotFound {
                    path: path_str.clone(),
                    resolved: dep,
                });
            }
            let dep = dep.canonicalize().unwrap_or_else(|_| dep.clone());
            if pending.iter().any(|(p, _)| p == &dep) {
                continue;
            }
            let dep_base = dep.parent().unwrap_or(root_dir).to_path_buf();
            pending.push((dep, dep_base));
        }
        synthetic.push(UseItem {
            alias: None,
            path,
            is_global: true,
        });
    }
    Ok(synthetic)
}

/// 将 `"Arc.QIF"` 拆成 `["Arc", "QIF"]`；拒绝空串与空段。
fn parse_global_using_path(path_str: &str) -> Result<Vec<Ident>, LoadError> {
    if path_str.is_empty() || path_str.split('.').any(|s| s.is_empty()) {
        return Err(LoadError::Parse {
            path: PathBuf::from("arc.toml"),
            message: format!(
                "[package].global_usings entry must be a non-empty dotted path, got {path_str:?}"
            ),
        });
    }
    Ok(path_str.split('.').map(Ident::from).collect())
}

fn check_duplicate_public(
    item: &Spanned<Item>,
    path: &Path,
    seen: &mut HashMap<String, PathBuf>,
) -> Result<(), LoadError> {
    let (name, is_public) = match &item.node {
        Item::Class(c) => (c.name.to_string(), matches!(c.vis, Visibility::Public)),
        Item::Struct(s) => (s.name.to_string(), matches!(s.vis, Visibility::Public)),
        Item::Interface(i) => (i.name.to_string(), matches!(i.vis, Visibility::Public)),
        Item::Enum(e) => (e.name.to_string(), matches!(e.vis, Visibility::Public)),
        Item::Fn(f) => (f.name.to_string(), matches!(f.vis, Visibility::Public)),
        _ => return Ok(()),
    };
    if !is_public {
        return Ok(());
    }
    // RFC 037：partial class 允许跨文件重复声明——同 key 的多个 partial 声明
    // 由 typeck `merge_partials_in_hir` 在 HIR 阶段合并为单一 ClassDef。
    // 不在 loader 层做重复检测，避免误拒合法的 partial 多文件场景。
    if let Item::Class(c) = &item.node {
        if c.is_partial {
            return Ok(());
        }
    }
    if let Some(first) = seen.get(&name) {
        return Err(LoadError::DuplicatePublic {
            name,
            first: first.clone(),
            second: path.to_path_buf(),
        });
    }
    seen.insert(name, path.to_path_buf());
    Ok(())
}

/// 按「包依赖拓扑序」计算 `used` 中每个包在合并项中的排序权重：依赖包权重小
/// （排前）、被依赖包权重大（排后），保证依赖包的泛型模板先于消费者注册。
///
/// 仅对 `graph.packages` 内已发现、且 `used` 中出现的包赋权；不在图内的包
/// （入口项目、外部 path/version 包）不赋权 → 调用方回退 `usize::MAX` 排最后。
/// 环安全：DFS 以「已访问」集合去重，环被安全跳过（拓扑序仍完整）。
fn package_topo_ranks(
    graph: &crate::package_graph::PackageGraph,
    used: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, usize> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut names: Vec<&String> = used
        .iter()
        .filter(|n| graph.packages.contains_key(*n))
        .collect();
    names.sort();
    for name in names {
        package_topo_dfs(name, graph, used, &mut visited, &mut order);
    }
    order
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect()
}

fn package_topo_dfs(
    name: &str,
    graph: &crate::package_graph::PackageGraph,
    used: &std::collections::HashSet<String>,
    visited: &mut HashSet<String>,
    order: &mut Vec<String>,
) {
    if visited.contains(name) {
        return;
    }
    visited.insert(name.to_string());
    let Some(node) = graph.packages.get(name) else {
        return;
    };
    let mut deps: Vec<&String> = node
        .dependencies
        .keys()
        .filter(|d| used.contains(*d) && graph.packages.contains_key(*d))
        .collect();
    deps.sort();
    for dep in deps {
        package_topo_dfs(dep, graph, used, visited, order);
    }
    order.push(name.to_string());
}

fn declared_namespace(program: &Program) -> Option<Vec<Ident>> {
    // 块状 namespace：`namespace X { ... }` → 程序仅 1 个 Namespace item
    if program.items.len() == 1 {
        if let Item::Namespace(ns) = &program.items[0].node {
            return Some(ns.path.clone());
        }
    }
    // 文件作用域 namespace：`namespace X;` → 程序可能有多 items，首个为 Namespace
    if let Some(first) = program.items.first() {
        if let Item::Namespace(ns) = &first.node {
            return Some(ns.path.clone());
        }
    }
    None
}

/// Entry file may omit `namespace` (C# `Program.cs` convention). When declared, root must match `[package].name`.
fn validate_entry_package_namespace(
    manifest: &ArcManifest,
    root_program: &Program,
) -> Result<(), LoadError> {
    let package = &manifest.package.name;
    let Some(ns) = declared_namespace(root_program) else {
        return Ok(());
    };
    let full_ns = ns.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(".");
    if full_ns == package.as_str() || full_ns.starts_with(&format!("{}.", package)) {
        Ok(())
    } else {
        Err(LoadError::PackageNamespaceMismatch {
            package: package.clone(),
            expected: package.clone(),
            found: full_ns,
        })
    }
}

/// Library modules (non-entry) must declare a namespace whose root matches `[package].name`.
fn validate_library_package_namespace(
    manifest: &ArcManifest,
    path: &Path,
    program: &Program,
) -> Result<(), LoadError> {
    let package = &manifest.package.name;
    let Some(ns) = declared_namespace(program) else {
        return Err(LoadError::NamespaceMismatch {
            path: path.to_path_buf(),
            expected: package.clone(),
            found: "(none)".into(),
        });
    };
    // 与入口包校验一致：全量命名空间 `A.B.C` 命中包名或为其子命名空间即通过。
    // 早期仅比较首个分段 root，导致包名含多分段（如 `Arc.Agent.Testing` 声明
    // `namespace Arc.Agent.Testing;` → root 为 `Arc`）时误报 mismatch。
    let full_ns = ns.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(".");
    if full_ns == package.as_str() || full_ns.starts_with(&format!("{}.", package)) {
        Ok(())
    } else {
        Err(LoadError::PackageNamespaceMismatch {
            package: package.clone(),
            expected: package.clone(),
            found: full_ns,
        })
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// C# 铁律 namespace 解析
// ══════════════════════════════════════════════════════════════════════════════

/// C# 铁律：解析 `using` 路径 → 返回所有匹配的 `.as` 文件。
///
/// - `Arc.*` 路径：查询全局 namespace 索引（`namespace` 声明即真理）
/// - 非 `Arc` 路径：`base_dir` 相对目录解析（项目本地文件）
fn resolve_use_deps(
    path: &[Ident],
    base_dir: &Path,
    ns_index: &NamespaceIndex,
) -> Result<Vec<PathBuf>, LoadError> {
    if path.is_empty() {
        return Err(LoadError::NotFound {
            path: String::new(),
            resolved: base_dir.to_path_buf(),
        });
    }

    // Arc.* 命名空间查全局索引
    if path.first().map(|s| s.as_str()) == Some("Arc") {
        let key = path
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(".");
        if let Some(files) = namespace_files(path, ns_index) {
            if files.is_empty() {
                return Err(LoadError::NotFound {
                    path: key,
                    resolved: PathBuf::new(),
                });
            }
            let mut result: Vec<PathBuf> = files.to_vec();
            result.sort();
            return Ok(result);
        }
        return Err(LoadError::NotFound {
            path: key,
            resolved: PathBuf::new(),
        });
    }

    // 非 Arc 路径：先查全局索引（含项目本地 namespace），再退回目录相对解析。
    if let Some(files) = namespace_files(path, ns_index) {
        if !files.is_empty() {
            let mut result: Vec<PathBuf> = files.to_vec();
            result.sort();
            return Ok(result);
        }
    }
    resolve_local_deps(path, base_dir)
}

/// 非 `Arc` 命名空间的本地文件解析：`using Foo.Bar;` → `base_dir/Foo/Bar.as` 或目录。
fn resolve_local_deps(path: &[Ident], base_dir: &Path) -> Result<Vec<PathBuf>, LoadError> {
    // 先尝试单文件：base_dir/Foo/Bar.as
    let mut resolved = base_dir.to_path_buf();
    for seg in &path[..path.len() - 1] {
        resolved = resolved.join(seg.as_str());
    }
    let file_path = resolved.join(format!("{}.as", path.last().unwrap()));
    if file_path.is_file() {
        return Ok(vec![file_path]);
    }

    // 再尝试目录：base_dir/Foo/Bar/
    let mut dir = base_dir.to_path_buf();
    for seg in path {
        dir = dir.join(seg.as_str());
    }
    if dir.is_dir() {
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(|source| LoadError::Read {
                path: dir.clone(),
                source,
            })?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "as"))
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(LoadError::NotFound {
                path: path
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("."),
                resolved: dir,
            });
        }
        return Ok(files);
    }

    Err(LoadError::NotFound {
        path: path
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("."),
        resolved: file_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    // ════════════════════════════════════════════════════════════════════════
    // 全局 namespace 索引测试
    // ════════════════════════════════════════════════════════════════════════

    #[test]
    fn std_path_override_indexes_alternate_tree() {
        // `[std].path` 覆盖后，namespace 索引必须扫覆盖树而非默认 workspace/std。
        let root = std::env::temp_dir().join(format!("arc-std-ovl-idx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let alt = root.join("AltStd");
        std::fs::create_dir_all(alt.join("Probe")).unwrap();
        std::fs::write(
            alt.join("Probe/Marker.as"),
            "namespace Arc.Probe;\npublic class Marker { }\n",
        )
        .unwrap();
        let index = build_namespace_index(&alt);
        let files = index
            .get("Arc.Probe")
            .expect("override std must index Arc.Probe");
        assert!(
            files.iter().any(|p| p.ends_with("Marker.as")),
            "expected Marker.as in override index, got {files:?}"
        );
        assert!(
            !index.contains_key("Arc.Collections"),
            "override tree must not silently fall back to workspace std"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn namespace_index_contains_arc_root() {
        let ws = workspace();
        let index = build_namespace_index(&ws.join("std"));
        // Arc 根命名空间应包含 Console.as / String.as / Signal.as 等核心文件
        let arc_files = index
            .get("Arc")
            .expect("index should contain 'Arc' namespace");
        assert!(arc_files
            .iter()
            .any(|p| p.ends_with("Console.as") || p.ends_with("std\\Arc\\Console.as")));
        assert!(arc_files
            .iter()
            .any(|p| p.ends_with("Signal.as") || p.ends_with("std\\Arc\\Signal.as")));
    }

    #[test]
    fn namespace_index_contains_arc_collections() {
        let ws = workspace();
        let index = build_namespace_index(&ws.join("std"));
        let files = index
            .get("Arc.Collections")
            .expect("index should contain 'Arc.Collections'");
        assert!(files
            .iter()
            .any(|p| p.ends_with("List.as") || p.ends_with("std\\Arc\\Collections\\List.as")));
        assert!(files
            .iter()
            .any(|p| p.ends_with("Dictionary.as")
                || p.ends_with("std\\Arc\\Collections\\Dictionary.as")));
    }

    #[test]
    fn namespace_index_contains_arc_net() {
        let ws = workspace();
        let index = build_namespace_index(&ws.join("std"));
        // Arc.Net 子库应被索引（std/Net/Core/ 下的文件声明 namespace Arc.Net）
        assert!(
            index.contains_key("Arc.Net"),
            "index should contain 'Arc.Net' namespace"
        );
    }

    #[test]
    fn namespace_index_contains_arc_data() {
        let ws = workspace();
        let index = build_namespace_index(&ws.join("std"));
        // Arc.Data 独立库应被索引（std/Data/IDbProvider.as 声明 namespace Arc.Data）
        let files = index
            .get("Arc.Data")
            .expect("index should contain 'Arc.Data' namespace");
        assert!(
            files
                .iter()
                .any(|p| p.ends_with("IDbProvider.as") || p.ends_with("std\\Data\\IDbProvider.as")),
            "expected Arc.Data to index std/Data/IDbProvider.as, got {files:?}"
        );
    }

    #[test]
    fn namespace_index_contains_arc_ui() {
        let ws = workspace();
        let index = build_namespace_index(&ws.join("std"));
        // Arc.UI 应被索引（扁平化目录的文件声明 namespace Arc.UI）
        assert!(
            index.contains_key("Arc.UI"),
            "index should contain 'Arc.UI' namespace"
        );
    }

    #[test]
    fn namespace_index_exact_match_only() {
        // C# 铁律：using Arc.Net 不加载 Arc.Net.P2P 的文件
        let ws = workspace();
        let index = build_namespace_index(&ws.join("std"));
        let net_files = index.get("Arc.Net").map(|v| v.as_slice()).unwrap_or(&[]);
        // Arc.Net 的文件不应包含 P2P 子命名空间的文件
        for f in net_files {
            let ns = extract_namespace_from_file(f);
            if let Some(ns) = ns {
                let ns_str = ns.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(".");
                assert_eq!(ns_str, "Arc.Net", "Arc.Net index should only contain files declaring 'namespace Arc.Net;', found {ns_str} in {f:?}");
            }
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // resolve_use_deps 测试（C# 铁律 namespace 索引解析）
    // ════════════════════════════════════════════════════════════════════════

    #[test]
    fn resolve_using_arc_collections_via_index() {
        let ws = workspace();
        let ns_index = build_namespace_index(&ws.join("std"));
        let files = resolve_use_deps(
            &["Arc".into(), "Collections".into()],
            Path::new("."),
            &ns_index,
        )
        .unwrap();
        assert!(files
            .iter()
            .any(|p| p.ends_with("List.as") || p.ends_with("std\\Arc\\Collections\\List.as")));
        assert!(files
            .iter()
            .any(|p| p.ends_with("Dictionary.as")
                || p.ends_with("std\\Arc\\Collections\\Dictionary.as")));
    }

    #[test]
    fn resolve_using_arc_linq_via_index() {
        // C# 铁律：using Arc.Linq → 查索引找 namespace Arc.Linq 的文件
        let ws = workspace();
        let ns_index = build_namespace_index(&ws.join("std"));
        let files =
            resolve_use_deps(&["Arc".into(), "Linq".into()], Path::new("."), &ns_index).unwrap();
        assert!(files
            .iter()
            .any(|p| p.ends_with("Queryable.as") || p.ends_with("std\\Arc\\Linq\\Queryable.as")));
        assert!(files
            .iter()
            .any(|p| p.ends_with("Enumerable.as") || p.ends_with("std\\Arc\\Linq\\Enumerable.as")));
    }

    #[test]
    fn resolve_using_arc_ui_loads_all_ui_files() {
        // C# 铁律：using Arc.UI → 查索引返回所有 namespace Arc.UI 的文件
        // （包括扁平化目录 std/UI/Core/Data/*.as / std/UI/Core/Markup/*.as，因为它们声明 namespace Arc.UI）
        let ws = workspace();
        let ns_index = build_namespace_index(&ws.join("std"));
        let files = resolve_use_deps(&["Arc".into(), "UI".into()], Path::new("."), &ns_index)
            .expect("using Arc.UI should resolve");
        assert!(
            files
                .iter()
                .any(|p| p.ends_with("std\\UI\\Core\\Data\\Binding.as")
                    || p.ends_with("std/UI/Core/Data/Binding.as")),
            "expected Binding.as to be loaded via namespace index"
        );
        assert!(
            files
                .iter()
                .any(|p| p.ends_with("std\\UI\\Core\\Data\\DataContext.as")
                    || p.ends_with("std/UI/Core/Data/DataContext.as")),
            "expected DataContext.as to be loaded via namespace index"
        );
    }

    #[test]
    fn resolve_using_arc_loads_signal() {
        // RFC 037 M1.1：Signal.as 声明 namespace Arc，using Arc 应通过索引加载
        let ws = workspace();
        let ns_index = build_namespace_index(&ws.join("std"));
        let files = resolve_use_deps(&["Arc".into()], Path::new("."), &ns_index)
            .expect("using Arc should resolve");
        assert!(
            files
                .iter()
                .any(|p| p.ends_with("std\\Arc\\Signal.as") || p.ends_with("std/Arc/Signal.as")),
            "expected Signal.as to be loaded via namespace index"
        );
    }

    #[test]
    fn resolve_using_arc_net_loads_net_files() {
        // std/Net/Core/ 下的文件声明 namespace Arc.Net → using Arc.Net 通过索引加载
        let ws = workspace();
        let ns_index = build_namespace_index(&ws.join("std"));
        let result = resolve_use_deps(&["Arc".into(), "Net".into()], Path::new("."), &ns_index);
        // 如果 std/Net/Core/ 下有文件声明 namespace Arc.Net，应成功
        // 如果没有（所有文件都在子命名空间），则为 NotFound
        match result {
            Ok(files) => {
                for f in &files {
                    let ns = extract_namespace_from_file(f);
                    if let Some(ns) = ns {
                        let ns_str = ns.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(".");
                        assert!(ns_str == "Arc.Net" || ns_str.starts_with("Arc.Net."),
                            "files loaded by 'using Arc.Net;' must declare Arc.Net namespace, got {ns_str} in {f:?}");
                    }
                }
            }
            Err(LoadError::NotFound { .. }) => {
                // 可接受：没有文件直接声明 namespace Arc.Net（全在子命名空间）
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // 本地文件解析测试（非 Arc 路径 → 目录相对）
    // ════════════════════════════════════════════════════════════════════════

    #[test]
    fn resolve_local_module_path() {
        let base = workspace().join("examples/UnitTest/Core/MultiFile");
        let ns_index = build_namespace_index(&workspace().join("std"));
        let files = resolve_use_deps(
            &["Services".into(), "GreetingService".into()],
            &base,
            &ns_index,
        )
        .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], base.join("Services/GreetingService.as"));
    }

    #[test]
    fn load_multi_file() {
        // Verify the UnitTest MultiFile test compiles via load_compile_unit.
        let svc = workspace().join("examples/UnitTest/Core/MultiFile/Services/GreetingService.as");
        let unit = load_compile_unit(&svc).unwrap();
        assert!(
            program_has_class(&unit.program.items, "GreetingService"),
            "expected GreetingService in GreetingService.as"
        );
    }

    fn program_has_class(items: &[Spanned<Item>], name: &str) -> bool {
        for item in items {
            match &item.node {
                Item::Class(c) if c.name.as_str() == name => return true,
                Item::Namespace(ns) if program_has_class(&ns.items, name) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    // ════════════════════════════════════════════════════════════════════════
    // 包命名空间校验测试（arc.toml 相关，不受铁律影响）
    // ════════════════════════════════════════════════════════════════════════

    #[test]
    fn package_namespace_mismatch_fails() {
        let dir = std::env::temp_dir().join(format!("arc-pkg-mismatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "myapp"
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.as"),
            r#"
namespace other;

void Main() { }
"#,
        )
        .unwrap();
        let err = load_compile_unit(&dir.join("main.as")).unwrap_err();
        assert!(
            matches!(err, LoadError::PackageNamespaceMismatch { .. }),
            "expected package namespace mismatch, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_namespace_match_succeeds() {
        let dir = std::env::temp_dir().join(format!("arc-pkg-match-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "myapp"
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.as"),
            r#"
namespace myapp.controllers;

void Main() { }
"#,
        )
        .unwrap();
        let r = load_compile_unit(&dir.join("main.as"));
        eprintln!("package_namespace_match_succeeds: {:?}", r);
        assert!(r.is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_without_namespace_succeeds() {
        let dir = std::env::temp_dir().join(format!("arc-pkg-no-ns-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "myapp"
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.as"),
            r#"
void Main() { }
"#,
        )
        .unwrap();
        let r = load_compile_unit(&dir.join("main.as"));
        eprintln!("package_namespace_match_succeeds: {:?}", r);
        assert!(r.is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_namespace_mismatch_fails() {
        let dir = std::env::temp_dir().join(format!("arc-lib-mismatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("arc.toml"),
            r#"
[package]
name = "myapp"
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.as"),
            r#"
using Helper;

void Main() { }
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("Helper.as"),
            r#"
namespace other;

public string Help() { return "x"; }
"#,
        )
        .unwrap();
        let err = load_compile_unit(&dir.join("main.as")).unwrap_err();
        assert!(
            matches!(err, LoadError::PackageNamespaceMismatch { .. }),
            "expected library package namespace mismatch, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ════════════════════════════════════════════════════════════════════════
    // native 契约加载测试（无关）
    // ════════════════════════════════════════════════════════════════════════
    #[test]
    fn load_native_contracts_finds_libc() {
        let ws = workspace();
        let mut registry = FileRegistry::new();
        let modules = load_native_contracts(&ws, &mut registry).unwrap();
        let libc = modules
            .iter()
            .find(|m| m.name == "libc")
            .expect("expected libc contract in crates/arc/native/libc.ani");
        // RFC 016 v2 M2 / RFC 016 M3：libc.ani 新增 memcmp/memcpy（object 形参 FFI 装箱测试）
        // RFC 016 M3 §3.3：libc.ani 新增 div（契约 struct div_t 按值传递测试）
        assert_eq!(libc.functions.len(), 6);
        let puts = libc
            .functions
            .iter()
            .find(|f| f.name == "puts")
            .expect("expected puts fn");
        assert_eq!(puts.params.len(), 1);
        assert_eq!(puts.params[0].name, "s");
        assert!(puts.ret.is_some());
        assert!(puts.symbol.is_none());
        // RFC 016 M2: frexp 带 out 参数方向
        let frexp = libc
            .functions
            .iter()
            .find(|f| f.name == "frexp")
            .expect("expected frexp fn");
        assert_eq!(frexp.params.len(), 2);
        assert_eq!(frexp.params[1].direction, ast::ParamDirection::Out);
    }

    #[test]
    fn load_native_contracts_finds_builtin_without_project_native() {
        // 编译器内置契约（crates/arc/native/）恒加载：workspace 无 native/ 也非空。
        let dir = std::env::temp_dir().join(format!("arc-no-native-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut registry = FileRegistry::new();
        let modules = load_native_contracts(&dir, &mut registry).unwrap();
        assert!(
            modules.iter().any(|m| m.name == "libc"),
            "expected built-in libc contract even without workspace native/ dir"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_native_contracts_skips_non_ani_files() {
        let dir = std::env::temp_dir().join(format!("arc-skip-nonani-{}", std::process::id()));
        let native = dir.join("native");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&native).unwrap();
        std::fs::write(native.join("readme.md"), "not a contract").unwrap();
        std::fs::write(
            native.join("browser.ani"),
            "native module browser { fn launch() -> int; }",
        )
        .unwrap();
        let mut registry = FileRegistry::new();
        let modules = load_native_contracts(&dir, &mut registry).unwrap();
        // 用户项目契约纳入，且 readme.md 被跳过；内置契约仍存在。
        assert!(
            modules.iter().any(|m| m.name == "browser"),
            "expected project browser contract"
        );
        assert!(
            modules.iter().any(|m| m.name == "libc"),
            "expected built-in libc contract"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RFC 016 M4（用户裁决简化 2026-08-03）：契约内 `library` 相对路径**保持相对**
    /// ——基准 = 执行程序根目录（`-o` 输出可执行文件所在目录），由 codegen 编译期
    /// 解析为绝对路径；loader 不再按 workspace 根解析。
    #[test]
    fn load_native_contracts_keeps_library_relative() {
        let dir = std::env::temp_dir().join(format!("arc-libdir-{}", std::process::id()));
        let native = dir.join("native");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&native).unwrap();
        std::fs::write(
            native.join("browser.ani"),
            "native module browser { library = \"vendor/chromium/lib\"; fn launch() -> int; }",
        )
        .unwrap();
        let mut registry = FileRegistry::new();
        let modules = load_native_contracts(&dir, &mut registry).unwrap();
        let browser = modules
            .iter()
            .find(|m| m.name == "browser")
            .expect("expected project browser contract");
        assert_eq!(
            browser.library.as_deref(),
            Some(std::path::Path::new("vendor/chromium/lib"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 构造临时项目（`<tmp>/native/`），写入契约与可选同名录，返回契约模块列表。
    fn contracts_with_pairs(
        tag: &str,
        files: &[(&str, &str)], // (相对 native/ 文件名, 内容)
    ) -> Vec<NativeModule> {
        let dir = std::env::temp_dir().join(format!("arc-pair-{tag}-{}", std::process::id()));
        let native = dir.join("native");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&native).unwrap();
        for (name, content) in files {
            std::fs::write(native.join(name), content).unwrap();
        }
        let registry = &mut FileRegistry::new();
        let modules = load_native_contracts(&dir, registry).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        modules
    }

    /// 同目录同名回退发现（RFC 016，显式声明缺失时的处理规则）·`.c` 源实现：
    /// `foo.ani` 未声明 source/library + 同目录同名 `foo.c` → 回退为源实现，
    /// `source` 填该 `.c` 的绝对路径（契约目录解析）。
    #[test]
    fn same_dir_pair_fallback_c_source() {
        let modules = contracts_with_pairs(
            "csrc",
            &[
                ("foo.ani", "native module foo { fn ping() -> int; }"),
                ("foo.c", "int ping(void) { return 1; }"),
            ],
        );
        let foo = modules
            .iter()
            .find(|m| m.name == "foo")
            .expect("foo contract");
        let expected = std::env::temp_dir()
            .join(format!("arc-pair-csrc-{}", std::process::id()))
            .join("native")
            .join("foo.c");
        assert_eq!(
            foo.source.as_deref(),
            Some(expected.as_path()),
            "source→foo.c 回退"
        );
        assert!(foo.library.is_none(), "c 源实现不应回退 library");
    }

    /// 同目录同名配对·DLL 发现：`foo.ani` 未声明位置 + 同目录 `foo.dll` →
    /// 回退为从契约目录链接（`library` 填契约目录绝对路径，作 -L/运行时候选）。
    #[test]
    fn same_dir_pair_fallback_dll() {
        let modules = contracts_with_pairs(
            "dll",
            &[
                ("foo.ani", "native module foo { fn ping() -> int; }"),
                (
                    "foo.dll",
                    "not a real dll; existence suffices for discovery",
                ),
            ],
        );
        let foo = modules
            .iter()
            .find(|m| m.name == "foo")
            .expect("foo contract");
        let dir = std::env::temp_dir()
            .join(format!("arc-pair-dll-{}", std::process::id()))
            .join("native");
        assert_eq!(
            foo.library.as_deref(),
            Some(dir.as_path()),
            "dll 同目录 → library=契约目录"
        );
        assert!(foo.source.is_none(), "dll 发现不应回退 source");
    }

    /// 同目录同名配对·无配对：`foo.ani` 既无 `.c` 也无同名词库 →
    /// source/library 均保持 None（经全局 `ani-native-lib` 搜索列表 / 系统路径）。
    #[test]
    fn same_dir_pair_fallback_no_pair() {
        let modules = contracts_with_pairs(
            "none",
            &[("foo.ani", "native module foo { fn ping() -> int; }")],
        );
        let foo = modules
            .iter()
            .find(|m| m.name == "foo")
            .expect("foo contract");
        assert!(foo.source.is_none(), "无配对 .c → source 保持 None");
        assert!(foo.library.is_none(), "无配对库 → library 保持 None");
    }

    #[test]
    fn namespace_extraction_matches_declaration() {
        // 验证关键文件的 namespace 声明能被正确提取并与预期一致
        let ws = workspace();

        let signal_path = ws.join("std/Arc/Signal.as");
        let ns =
            extract_namespace_from_file(&signal_path).expect("Signal.as should have namespace");
        let ns_str: Vec<&str> = ns.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            ns_str,
            vec!["Arc"],
            "Signal.as should declare namespace Arc"
        );

        let console_path = ws.join("std/Arc/Console.as");
        let ns =
            extract_namespace_from_file(&console_path).expect("Console.as should have namespace");
        let ns_str: Vec<&str> = ns.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            ns_str,
            vec!["Arc"],
            "Console.as should declare namespace Arc"
        );
    }

    #[test]
    fn namespace_index_self_consistent() {
        // 铁律自证：索引中每个文件提取出的 namespace 必须与索引 key 一致
        let ws = workspace();
        let index = build_namespace_index(&ws.join("std"));
        for (ns_key, files) in &index {
            for file in files {
                let extracted = extract_namespace_from_file(file)
                    .expect("indexed file must have namespace declaration");
                let extracted_str = extracted
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                assert_eq!(extracted_str, *ns_key,
                    "namespace index inconsistency: file {file:?} indexed under '{ns_key}' but declares '{extracted_str}'");
            }
        }
    }

    #[test]
    fn no_hardcoded_constants_in_resolution() {
        // 铁律验证：新增一个任意 namespace 的子库不需要修改编译器
        // 只要在 std/ 下创建文件并声明 namespace，namespace 索引自动收录。
        let ws = workspace();
        let index = build_namespace_index(&ws.join("std"));

        // 已有子库应被自动索引（无需硬编码）
        assert!(
            index.contains_key("Arc"),
            "Arc root namespace must be auto-indexed"
        );
        assert!(
            index.contains_key("Arc.Collections"),
            "Arc.Collections must be auto-indexed"
        );
        assert!(
            index.contains_key("Arc.Linq"),
            "Arc.Linq must be auto-indexed"
        );

        // 新增子库（如 Arc.Net.P2P）只要有文件声明对应 namespace 就自动可用
        if index.contains_key("Arc.Net.P2P") {
            let files = &index["Arc.Net.P2P"];
            for f in files {
                let ns = extract_namespace_from_file(f).unwrap();
                let ns_str = ns.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(".");
                assert_eq!(ns_str, "Arc.Net.P2P");
            }
        }
        // 如果还没有 P2P 文件，这个测试不会失败——只要索引机制正确即可。
        // 新增文件后无需修改 loader.rs 就能被 using 引用，这就是铁律的核心价值。
    }
}
