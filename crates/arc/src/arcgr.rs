//! `.arcgr` 收集器与产物生成器（RFC 034 M2 Step 3）。
//!
//! ## 职责
//!
//! 桥接 typeck 产物与 [`reachability`] 算法库，最终产出可写入磁盘的
//! `.arcgr` 二进制文件。本模块是 HIR/TypeRegistry → `.arcgr` 的转换层：
//!
//! 1. **符号收集**：从 [`TypeRegistry`] 提取本包所有符号（类型/方法/字段）
//! 2. **入口标记**：识别 `fn main()` / `library` 公共导出
//! 3. **EdgeKind 边收集**：遍历 [`TypedFn`] bodies 提取调用/方法/实例化/实现边
//! 4. **虚分派保守组**：从接口 → 实现类推导
//! 5. **可达性分析**：调用 [`reachability::analyze`] 产出 `ReferenceGraph`
//! 6. **二进制产出**：组装 [`ArcgrFile`] 并序列化为字节
//!
//! ## 范围（M2）
//!
//! EdgeKind 覆盖：
//! - `Call`：函数调用（含静态方法直接调用）
//! - `MethodCall`：实例方法调用（虚分派保守策略触发）
//! - `New`：`new T(...)` 实例化
//! - `Implement`：`class : Interface` / `class : BaseClass`
//!
//! 未覆盖（M3+ 扩展）：
//! - `FieldAccess`/`PropertyAccess`/`VariantMatch`/`GenericInstantiation`
//!
//! EntryPoint 覆盖：
//! - `Main`：`fn main()` / `fn Main()`
//! - `LibraryExport`：用户文件中 public 类型
//! - `FFIExport`/`TestFunction`/`DynamicLibEntry`/`CGMain`：M3+ 扩展

use std::collections::HashMap;

use arcgr::{
    ArcgrFile, ContextManifest, CrateDagSummary, CrateModule, EdgeKind, EntryPoint, EntryPointKind,
    FileEntry, FileTable, L0ProjectOverview, L1ModuleSurface, NamespaceEntry, ProjectKind,
    PublicApiEntry, PublicApiKind, ReferenceContext, ReferenceEdge as Edge, ReferenceEntry,
    ReferenceTable, SymbolEntry, SymbolKind, SymbolTable, TypeSig, Visibility,
};
// RFC 017：重导出编解码函数供外部消费（测试/工具链等）。
pub use arcgr::{read_arcgr, write_arcgr};
use ast::{Expr, FileId, Ident, Item, Program, Span, Spanned, Type};
use reachability::{AnalysisInput, VirtualDispatchGroup};
use typeck::{TypeChecker, TypeKind, TypeRegistry, TypedFn};

use crate::manifest::ArcManifest;

/// 收集上下文——贯穿符号/入口/边收集过程。
struct CollectContext {
    /// 符号名（短名或 `Class.member` 形式）→ symbol_id 映射。
    name_to_symbol_id: HashMap<String, u32>,
    /// 已分配的 symbol_id 全集（用于可达性分析的 universe）。
    universe: Vec<u32>,
}

impl CollectContext {
    fn new() -> Self {
        Self {
            name_to_symbol_id: HashMap::new(),
            universe: Vec::new(),
        }
    }

    /// 注册一个符号到 name 映射，返回分配的 symbol_id。
    fn register(&mut self, name: String) -> u32 {
        let id = self.universe.len() as u32;
        self.name_to_symbol_id.insert(name, id);
        self.universe.push(id);
        id
    }

    /// 按名称查找 symbol_id。
    fn lookup(&self, name: &str) -> Option<u32> {
        self.name_to_symbol_id.get(name).copied()
    }
}

/// 函数体局部作用域——变量名 → 类型名（含 `this`/`base` 属主）。
///
/// K3：MethodCall 接收者类型解析依赖此映射（`Helper h = new Helper();
/// h.Double(21)` → `h` → `Helper`，据此拼 `"Helper.Double"` 查符号表）。
/// 词法作用域按块压栈：进入嵌套块 `push`、退出 `pop`，同名变量按最近绑定
/// 解析（shadowing 取栈顶）。
struct LocalScope {
    scopes: Vec<HashMap<String, String>>,
    /// 当前方法属主类（`this`/`base` 接收者解析用）；自由函数为 None。
    owner: Option<String>,
}

impl LocalScope {
    fn new(owner: Option<String>) -> Self {
        Self {
            scopes: vec![HashMap::new()],
            owner,
        }
    }

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: &str, ty: &str) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), ty.to_string());
        }
    }

    fn lookup(&self, name: &str) -> Option<&str> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.as_str());
            }
        }
        None
    }
}

/// 收集 `.arcgr` 全部数据并执行可达性分析。
///
/// # 输入
///
/// - `typeck`：typeck 完成后的 TypeChecker（提供 TypeRegistry）
/// - `typed_fns`：typeck `check_module()` 返回的 typed functions
/// - `program`：用户源码 AST Program（用于自由函数来源过滤——
///   `typed_fns` 包含 `using` 导入的所有文件函数，需通过 AST span.file_id
///   仅保留项目文件集合中声明的函数）
/// - `project_files`：项目文件表（入口源文件 + 同目录声明 namespace 的兄弟文件，
///   K2 项目级 inspect 的多文件符号合并 + 跨文件 edges 前置；来自 loader 合并后的
///   同一编译单元 `file_registry`，经包过滤排除 std 依赖与 `.ani` 契约）
/// - `manifest`：可选的 `arc.toml` 项目清单（RFC 034 M4）——
///   `Some` 时填充 `ContextManifest`（L0 项目概览 + L1 模块面），
///   `None` 时跳过填充（保持向后兼容，单文件无 manifest 场景）
///
/// # 输出
///
/// [`ArcgrFile`]——已填充 4 张表（FileTable/SymbolTable/ReferenceTable/ReferenceGraph），
/// 可直接调用 `arcgr::write_arcgr` 序列化为字节。当 `manifest=Some` 时
/// `context_manifest` 字段填充 L0+L1 双层结构。
pub fn collect_arcgr_file(
    typeck: &TypeChecker,
    typed_fns: &[TypedFn],
    program: &Program,
    project_files: &[FileEntry],
    manifest: Option<&ArcManifest>,
) -> ArcgrFile {
    let registry = typeck.registry();
    let mut ctx = CollectContext::new();

    // 项目文件集合（入口 + 兄弟文件）。K2：所有来源过滤（符号/入口/边/虚分派）
    // 以「文件 ∈ 项目文件集合」为准，不再只认用户入口文件——跨文件符号合并、
    // 跨文件 edges 依赖此集合。
    let included: std::collections::HashSet<FileId> =
        project_files.iter().map(|f| f.file_id).collect();

    // 1. FileTable——项目文件表（入口 + 兄弟文件）
    let file_table = collect_file_table(project_files);

    // 2. SymbolTable——从 TypeRegistry + typed_fns 收集所有项目源码符号
    let user_fn_names = collect_user_source_fn_names(program, &included);
    let symbol_table =
        collect_symbol_table(registry, typed_fns, &user_fn_names, &included, &mut ctx);

    // 3. 入口点——Main + LibraryExport
    let entry_points = collect_entry_points(typeck, typed_fns, &included, &ctx);

    // 4. EdgeKind 边 + ReferenceTable 引用——同步收集（共享 AST walker）
    let (edges, reference_table) = collect_edges_and_refs(registry, typed_fns, &included, &ctx);

    // 5. 虚分派保守组——接口方法 → 所有实现方法
    let vdispatch_groups = collect_virtual_dispatch_groups(registry, &included, &ctx);

    // 6. 可达性分析
    let input = AnalysisInput::new()
        .with_entry_points(entry_points)
        .with_edges(edges)
        .with_universe(ctx.universe.clone())
        .with_virtual_dispatch_groups(vdispatch_groups);
    let report = reachability::analyze(&input);

    // 7. ContextManifest——RFC 034 M4：从 ArcManifest + SymbolTable 填充
    let context_manifest = manifest.map(|m| collect_context_manifest(m, &symbol_table));

    ArcgrFile {
        file_table,
        symbol_table,
        reference_table,
        reference_graph: report.reference_graph,
        context_manifest,
    }
}

/// 收集项目文件表——入口文件 + 同目录声明 namespace 的兄弟文件（K2 项目级 inspect）。
///
/// 过滤口径：文件须属入口包（`file_packages[file_id] == entry_package`）且为
/// `.as` 源文件（排除 `.ani` 契约与 std 依赖——后者属各自 std 包，非入口包）。
/// 退化兜底：集合为空时至少含入口文件（manifest 缺失等异常场景）。排序按
/// `file_id` 保证确定性输出。
pub fn collect_project_files(
    unit: &crate::loader::CompileUnit,
    user_file_id: FileId,
) -> Vec<FileEntry> {
    let mut files = Vec::new();
    for id in 1..=unit.file_registry.len() as FileId {
        let pkg = unit
            .file_packages
            .get(&id)
            .map(String::as_str)
            .unwrap_or(unit.entry_package.as_str());
        if pkg != unit.entry_package.as_str() {
            continue;
        }
        let Some(path) = unit.file_registry.path_of(id) else {
            continue;
        };
        if path.extension().and_then(|e| e.to_str()) != Some("as") {
            continue;
        }
        files.push(FileEntry::new(id, path.display().to_string(), 0, 0));
    }
    files.sort_by_key(|f| f.file_id);
    if files.is_empty() {
        files.push(FileEntry::new(
            user_file_id,
            unit.root.display().to_string(),
            0,
            0,
        ));
    }
    files
}

/// 收集 FileTable——项目文件表（入口 + 兄弟文件，K2 多文件并入）。
fn collect_file_table(project_files: &[FileEntry]) -> FileTable {
    let mut table = FileTable::new();
    for entry in project_files {
        // M2 阶段：content_hash/line_count 待 Step 4 填充（CLI 侧当前传 0）。
        table.push(entry.clone());
    }
    table
}

/// 从 TypeRegistry + typed_fns 收集符号表。
///
/// 符号来源：
/// 1. `registry.types` —— class/struct/interface/enum/variant 类型及其方法/字段
/// 2. `typed_fns` 中 `owner == None` 的自由函数（如 `fn main()` / `fn Main()`），
///    仅当函数名出现在 `user_fn_names` 中（即声明于项目文件集合）时纳入。
fn collect_symbol_table(
    registry: &TypeRegistry,
    typed_fns: &[TypedFn],
    user_fn_names: &HashMap<String, Span>,
    included: &std::collections::HashSet<FileId>,
    ctx: &mut CollectContext,
) -> SymbolTable {
    let mut table = SymbolTable::new();

    for (name, nominal) in &registry.types {
        if !included.contains(&nominal.span.file_id) {
            continue;
        }

        // 1. 类型本身
        let kind = match nominal.kind {
            TypeKind::Class => SymbolKind::Class,
            TypeKind::Struct => SymbolKind::Struct,
            TypeKind::StaticClass => SymbolKind::Module,
            TypeKind::Interface => SymbolKind::Interface,
            TypeKind::Enum => SymbolKind::Enum,
            TypeKind::Variant => SymbolKind::Variant,
        };

        let type_sig = build_named_type_sig(name, registry, nominal);
        let entry = SymbolEntry::new(
            ctx.register(name.to_string()),
            name.to_string(),
            kind,
            Visibility::Public,
            nominal.span.file_id,
            nominal.span.start,
            nominal.span.end,
            type_sig,
            None,
        );
        table.push(entry);

        // 2. 所有方法（含 private——内部可达性需考虑）
        for (_method_name, overloads) in &nominal.methods {
            for sig in overloads {
                let method_kind = if sig.modifier == ast::MethodModifier::Static {
                    SymbolKind::StaticMethod
                } else {
                    SymbolKind::Method
                };
                let method_visibility = map_visibility(sig.vis);
                let method_name_full = format!("{}.{}", name, sig.name);
                let method_sig = build_method_type_sig(name, sig, registry);
                let method_entry = SymbolEntry::new(
                    ctx.register(method_name_full),
                    format!("{}.{}", name, sig.name),
                    method_kind,
                    method_visibility,
                    nominal.span.file_id,
                    nominal.span.start,
                    nominal.span.end,
                    method_sig,
                    None,
                );
                table.push(method_entry);
            }
        }

        // 3. 所有字段（含 private）
        for (field_name, field) in &nominal.fields {
            let field_kind = if field.is_const {
                SymbolKind::Constant
            } else {
                SymbolKind::Field
            };
            let field_visibility = map_visibility(field.vis);
            let field_full_name = format!("{}.{}", name, field_name);
            let field_sig = build_type_sig_from_name(&field.ty, registry);
            let field_entry = SymbolEntry::new(
                ctx.register(field_full_name.clone()),
                field_full_name,
                field_kind,
                field_visibility,
                nominal.span.file_id,
                nominal.span.start,
                nominal.span.end,
                field_sig,
                None,
            );
            table.push(field_entry);
        }
    }

    // 4. 自由函数——`typed_fns` 中 owner == None 的项（如 `fn Main()`）
    //
    // 与类型方法不同，自由函数不在 `registry.types` 中，必须从 `typed_fns`
    // 单独收集，否则 `fn main()` 入口不会出现在符号表 universe 中，
    // 导致可达性分析无法从 Main 出发传播。
    //
    // 来源过滤：`typed_fns` 包含 `using` 导入的所有文件函数（如 Extensions.as
    // 中的 `Identity_Rectangle`），需通过 `user_fn_names`（AST 来源映射）
    // 仅保留项目文件集合中声明的函数。
    for typed_fn in typed_fns {
        if typed_fn.owner.is_some() {
            continue;
        }
        let fn_name = typed_fn.name.to_string();
        // 必须在项目文件集合中声明——通过 AST 来源映射过滤
        let span = match user_fn_names.get(&fn_name) {
            Some(s) => *s,
            None => continue,
        };
        // 避免与已注册的同名符号冲突（理论上自由函数不会与类型重名）
        if ctx.lookup(&fn_name).is_some() {
            continue;
        }
        let fn_sig = build_fn_type_sig(typed_fn, registry);
        let entry = SymbolEntry::new(
            ctx.register(fn_name.clone()),
            fn_name,
            SymbolKind::Function,
            Visibility::Public,
            span.file_id,
            span.start,
            span.end,
            fn_sig,
            None,
        );
        table.push(entry);
    }

    table
}

/// 遍历 AST Program，收集项目文件集合中声明的自由函数名 → span 映射。
///
/// 递归处理 `namespace` 嵌套。`Fn` 项外的其他项（class/struct/interface/enum）
/// 不在此收集——它们由 `registry.types` 提供。
///
/// 函数名使用**裸名**（与 typeck `typed_fns.name` 一致——typeck 对自由函数
/// 一律以裸 `f.name` 入表，不带 namespace 前缀）。若带前缀，跨命名空间调用点
/// 的 `Call` 边（`extract_callee_name` 取裸名）将查无符号而静默丢弃：
/// - 顶层 `void Main()` → "Main"
/// - `namespace Foo { void Bar() }` → "Bar"（typed_fns.name 形式）
///
/// K2：`included` 为项目文件集合（入口 + 同目录声明 namespace 的兄弟文件），
/// 跨文件自由函数符号据此并入符号表。
fn collect_user_source_fn_names(
    program: &Program,
    included: &std::collections::HashSet<FileId>,
) -> HashMap<String, Span> {
    let mut map = HashMap::new();
    collect_user_fn_names_in_items(&program.items, included, &mut map);
    map
}

fn collect_user_fn_names_in_items(
    items: &[Spanned<Item>],
    included: &std::collections::HashSet<FileId>,
    out: &mut HashMap<String, Span>,
) {
    for item in items {
        if !included.contains(&item.span.file_id) {
            continue;
        }
        match &item.node {
            Item::Fn(f) => {
                out.entry(f.name.to_string()).or_insert(item.span);
            }
            Item::Namespace(ns) => {
                collect_user_fn_names_in_items(&ns.items, included, out);
            }
            _ => {}
        }
    }
}

/// 收集入口点——Main + LibraryExport。
fn collect_entry_points(
    typeck: &TypeChecker,
    typed_fns: &[TypedFn],
    included: &std::collections::HashSet<FileId>,
    ctx: &CollectContext,
) -> Vec<EntryPoint> {
    let mut entries = Vec::new();

    // 1. Main——遍历 typed_fns 查找 main / Main 函数（顶层自由函数）
    //
    // 入口 ID 必须与符号表中 Main 的 symbol_id 一致（由 collect_symbol_table
    // 注册），否则 reachability::analyze 的 BFS 起点不在 universe 中，
    // 无法传播可达性。
    for typed_fn in typed_fns {
        if typed_fn.owner.is_some() {
            continue; // 跳过类方法（仅顶层 main/Main 作为入口）
        }
        let fn_name = typed_fn.name.as_str();
        if fn_name.eq_ignore_ascii_case("main") {
            if let Some(sym_id) = ctx.lookup(fn_name) {
                entries.push(EntryPoint::new(sym_id, EntryPointKind::Main, 0));
            }
        }
    }

    // 2. LibraryExport——用户源文件中 public 类型作为入口（保守策略）
    //
    // 通过 typed_fns.owner 反查类型名，再通过 ctx.lookup 找到 symbol_id。
    // 避免重复添加（同一类型可能被多个 method 持有）。
    for typed_fn in typed_fns {
        if let Some(owner) = &typed_fn.owner {
            if let Some(sym_id) = ctx.lookup(owner.as_str()) {
                let exists = entries.iter().any(|e| e.symbol_id == sym_id);
                if !exists {
                    entries.push(EntryPoint::new(sym_id, EntryPointKind::LibraryExport, 100));
                }
            }
        }
    }

    // 3. RFC 006 A3 S3：静态字段初始化器引用的方法作为可达性入口根。
    //
    // 静态字段初值（尤其惰性 `static readonly`，以及急切 `static`）由 codegen
    // 注入的 `__sinit_<Class>` / `__lazy_init_<Class>` helper 在模块初始化 /
    // 首次访问时调用。这些 helper 是 codegen 生成的，reachability（仅能看见
    // 函数体边）不可见——若不把初值中调用的方法标记为根，它们会被判为不可达而
    // 未发射，导致 LLVM `use of undefined value`（如 `static readonly int X =
    // Construct();` 的 `Construct`）。此处把它们加入入口根，保证始终发射。
    let registry = typeck.registry();
    for (class_name, nominal) in &registry.types {
        if !included.contains(&nominal.span.file_id) {
            continue;
        }
        for field in nominal.fields.values() {
            if !field.is_static || field.is_const {
                continue;
            }
            if let Some(init) = &field.init {
                collect_static_init_entry_points(class_name, &init.node, ctx, &mut entries);
            }
        }
    }

    entries
}

/// 递归遍历静态字段初始化器表达式，收集其调用的方法符号并作为可达性入口根。
///
/// 与函数体内的调用不同，静态字段初值没有宿主函数符号；codegen 生成的
/// `__sinit_<Class>` / `__lazy_init_<Class>` 会调用初值中的方法，故须把它们
/// 作为根保活。仅处理静态字段初值中常见的表达式形态；未覆盖形态保守跳过
/// （不做递归，避免过度保活）。
fn collect_static_init_entry_points(
    class: &Ident,
    expr: &Expr,
    ctx: &CollectContext,
    entries: &mut Vec<EntryPoint>,
) {
    match expr {
        // 裸静态方法调用：`Construct()` 在类 C 的字段初值中 → 符号 `C.Construct`。
        Expr::Call { func, args, .. } => {
            if let Expr::Ident(callee) = &func.node {
                let sym_name = format!("{class}.{callee}");
                if let Some(sym_id) = ctx.lookup(&sym_name) {
                    let exists = entries.iter().any(|e| e.symbol_id == sym_id);
                    if !exists {
                        entries.push(EntryPoint::new(sym_id, EntryPointKind::CGMain, 50));
                    }
                }
            }
            for arg in args {
                collect_static_init_entry_points(class, &arg.node, ctx, entries);
            }
            collect_static_init_entry_points(class, &func.node, ctx, entries);
        }
        Expr::MethodCall { receiver, args, .. } => {
            // 接收者上的方法调用——递归接收者与实参（方法属主经接收者类型解析，
            // 静态字段初值中少见，保守递归即可）。
            collect_static_init_entry_points(class, &receiver.node, ctx, entries);
            for arg in args {
                collect_static_init_entry_points(class, &arg.node, ctx, entries);
            }
        }
        Expr::New { args, .. } => {
            for arg in args {
                collect_static_init_entry_points(class, &arg.node, ctx, entries);
            }
        }
        Expr::Field { receiver, .. } => {
            collect_static_init_entry_points(class, &receiver.node, ctx, entries);
        }
        Expr::Binary { left, right, .. } => {
            collect_static_init_entry_points(class, &left.node, ctx, entries);
            collect_static_init_entry_points(class, &right.node, ctx, entries);
        }
        Expr::Unary { expr: inner, .. } => {
            collect_static_init_entry_points(class, &inner.node, ctx, entries);
        }
        Expr::Index { receiver, index } => {
            collect_static_init_entry_points(class, &receiver.node, ctx, entries);
            collect_static_init_entry_points(class, &index.node, ctx, entries);
        }
        Expr::Await(inner) => {
            collect_static_init_entry_points(class, &inner.node, ctx, entries);
        }
        _ => {}
    }
}

/// 收集 EdgeKind 边 + ReferenceTable 引用——共享 AST walker 同步产出。
///
/// - `edges`：可达性分析使用的语义关系边（Call/MethodCall/New/Implement 等）
/// - `refs`：LSP `textDocument/references` 使用的引用清单（覆盖更广的引用上下文）
///
/// 两者共享 AST walker——避免重复遍历 typed_fn bodies。ReferenceTable 与 Edge
/// 不同点：ReferenceTable 记录**所有**引用点（含类型标注、字段读写等），Edge
/// 仅记录语义关系边（用于可达性分析）。
fn collect_edges_and_refs(
    registry: &TypeRegistry,
    typed_fns: &[TypedFn],
    included: &std::collections::HashSet<FileId>,
    ctx: &CollectContext,
) -> (Vec<Edge>, ReferenceTable) {
    let mut edges = Vec::new();
    let mut refs = ReferenceTable::new();
    let mut next_ref_id = 0u32;

    // 1. Implement 边 + Inherit/Implement 引用——从 TypeRegistry.bases 推导
    for (class_name, nominal) in &registry.types {
        if !included.contains(&nominal.span.file_id) {
            continue;
        }
        let caller_id = match ctx.lookup(class_name.as_str()) {
            Some(id) => id,
            None => continue,
        };
        for base in &nominal.bases {
            if let Some(base_id) = ctx.lookup(base.as_str()) {
                edges.push(Edge::new(
                    caller_id,
                    base_id,
                    EdgeKind::Implement,
                    nominal.span.file_id,
                    nominal.span.start,
                    nominal.span.end,
                    true,
                ));
                // 同步记录为 ReferenceContext::Implement 引用
                refs.push(ReferenceEntry::new(
                    next_ref_id,
                    base_id,
                    nominal.span.file_id,
                    nominal.span.start,
                    nominal.span.end,
                    ReferenceContext::Implement,
                ));
                next_ref_id += 1;
            }
        }
    }

    // 2. Call / MethodCall / New / FieldAccess 边 + Read/Write/Call 引用——遍历 typed_fn bodies
    for typed_fn in typed_fns {
        let caller_name = typed_fn_symbol_name(typed_fn, registry);
        let caller_id = match ctx.lookup(&caller_name) {
            Some(id) => id,
            None => continue,
        };

        if let Some(body) = &typed_fn.body {
            // 调用点所在文件（K2 跨文件边归属）：优先取函数体首语句 span 的 file_id；
            // 空体方法回退到属主类型文件（空体无任何边，纯兜底）。
            let body_file_id = body
                .stmts
                .first()
                .map(|s| s.span.file_id)
                .unwrap_or_else(|| {
                    typed_fn
                        .owner
                        .as_ref()
                        .and_then(|o| registry.types.get(o))
                        .map(|n| n.span.file_id)
                        .unwrap_or(0)
                });

            // K3：局部作用域——`this`/`base` 属主类 + 形参类型（`Helper h = …`
            // 的 `h` 类型由 `collect_stmt_edges_and_refs` 的 Let 分支动态登记）。
            let mut locals = LocalScope::new(typed_fn.owner.as_ref().map(|o| o.to_string()));
            for (param_name, param_ty) in &typed_fn.params {
                if let Some(tname) = type_id_type_name(param_ty) {
                    locals.define(param_name.as_str(), &tname);
                }
            }
            collect_expr_edges_and_refs(
                &body.stmts,
                caller_id,
                body_file_id,
                ctx,
                &mut locals,
                &mut edges,
                &mut refs,
                &mut next_ref_id,
            );
        }
    }

    (edges, refs)
}

/// 从 typed_fn 推导 arcgr 符号表名（`Class.Method`）。
///
/// `typed_fn.name` 为 typeck 的 link 名——类方法形如 `Class::Method`（重载时带
/// `_<param>` 后缀，如 `Class::Add_int`），自由函数为裸名。arcgr 符号表以
/// `Class.Method`（裸方法名）注册，故此处剥离 `Class::` 前缀并按**最长前缀**
/// 匹配属主类方法名，把重载后缀折回裸方法名。K3 修复：原 `format!("{}.{}",
/// owner, typed_fn.name)` 产出 `Helper.Helper::Double` 查无符号，类方法体从未
/// 被遍历——方法→方法 MethodCall 边（含类内裸调用）因此恒缺。
fn typed_fn_symbol_name(typed_fn: &TypedFn, registry: &TypeRegistry) -> String {
    let Some(owner) = &typed_fn.owner else {
        return typed_fn.name.to_string();
    };
    let link = typed_fn.name.as_str();
    let bare = link.strip_prefix(&format!("{owner}::")).unwrap_or(link);
    if let Some(nominal) = registry.types.get(owner) {
        let exact = nominal.methods.keys().find(|k| k.as_str() == bare);
        let prefix = nominal
            .methods
            .keys()
            .filter(|k| {
                let k = k.as_str();
                k.len() < bare.len() && bare.starts_with(k) && bare.as_bytes()[k.len()] == b'_'
            })
            .max_by_key(|k| k.as_str().len());
        if let Some(k) = exact.or(prefix) {
            return format!("{owner}.{k}");
        }
    }
    format!("{owner}.{bare}")
}

/// 递归遍历语句块中的表达式，收集边 + 引用。
fn collect_expr_edges_and_refs(
    stmts: &[Spanned<ast::Stmt>],
    caller_id: u32,
    file_id: FileId,
    ctx: &CollectContext,
    locals: &mut LocalScope,
    edges: &mut Vec<Edge>,
    refs: &mut ReferenceTable,
    next_ref_id: &mut u32,
) {
    for stmt in stmts {
        collect_stmt_edges_and_refs(
            &stmt.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            stmt.span,
        );
    }
}

/// 遍历单条语句中的表达式。
fn collect_stmt_edges_and_refs(
    stmt: &ast::Stmt,
    caller_id: u32,
    file_id: FileId,
    ctx: &CollectContext,
    locals: &mut LocalScope,
    edges: &mut Vec<Edge>,
    refs: &mut ReferenceTable,
    next_ref_id: &mut u32,
    span: Span,
) {
    use ast::Stmt;
    match stmt {
        Stmt::Let { name, ty, init, .. } => {
            // K3：局部变量类型登记——`Helper h = new Helper()`（显式注解）与
            // `var q = new Quad()`（从初始化表达式推断）。
            if let Some(tname) = let_binding_type(
                ty.as_ref().map(|t| &t.node),
                init.as_ref().map(|i| &i.node),
                ctx,
                locals,
            ) {
                locals.define(name.as_str(), &tname);
            }
            if let Some(expr) = init {
                collect_one_expr_and_refs(
                    &expr.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    expr.span,
                );
            }
        }
        Stmt::Expr(expr) => collect_one_expr_and_refs(
            &expr.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            expr.span,
        ),
        Stmt::Return(Some(expr)) => collect_one_expr_and_refs(
            &expr.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            expr.span,
        ),
        Stmt::Return(None) => {}
        Stmt::While { cond, body, .. } => {
            locals.push();
            collect_one_expr_and_refs(
                &cond.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                cond.span,
            );
            collect_expr_edges_and_refs(
                &body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            locals.pop();
        }
        Stmt::For {
            var, iter, body, ..
        } => {
            locals.push();
            collect_one_expr_and_refs(
                &iter.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                iter.span,
            );
            collect_expr_edges_and_refs(
                &body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            locals.pop();
            let _ = var;
        }
        Stmt::Assign { value, .. } => collect_one_expr_and_refs(
            &value.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            value.span,
        ),
        Stmt::Throw { expr, .. } => collect_one_expr_and_refs(
            &expr.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            expr.span,
        ),
        Stmt::TryCatch {
            try_body,
            when_cond,
            catch_ty,
            catch_name,
            catch_body,
            finally,
            ..
        } => {
            locals.push();
            collect_expr_edges_and_refs(
                &try_body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            if let Some(w) = when_cond {
                collect_one_expr_and_refs(
                    &w.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    w.span,
                );
            }
            // catch 变量仅在 catch 体内可见——在本层作用域登记。
            if let Some(tname) = extract_type_name(&catch_ty.node) {
                locals.define(catch_name.as_str(), &tname);
            }
            collect_expr_edges_and_refs(
                &catch_body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            if let Some(f) = finally {
                collect_expr_edges_and_refs(
                    &f.stmts,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                );
            }
            locals.pop();
        }
        Stmt::TryFinally { body, finally, .. } => {
            locals.push();
            collect_expr_edges_and_refs(
                &body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            collect_expr_edges_and_refs(
                &finally.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            locals.pop();
        }
        Stmt::Using {
            name,
            ty,
            init,
            body,
            ..
        } => {
            locals.push();
            if let Some(tname) =
                let_binding_type(ty.as_ref().map(|t| &t.node), Some(&init.node), ctx, locals)
            {
                locals.define(name.as_str(), &tname);
            }
            collect_one_expr_and_refs(
                &init.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                init.span,
            );
            collect_expr_edges_and_refs(
                &body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            locals.pop();
        }
        Stmt::UsingVar { name, ty, init, .. } => {
            if let Some(tname) =
                let_binding_type(ty.as_ref().map(|t| &t.node), Some(&init.node), ctx, locals)
            {
                locals.define(name.as_str(), &tname);
            }
            collect_one_expr_and_refs(
                &init.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                init.span,
            );
        }
        Stmt::AwaitUsing {
            name,
            ty,
            init,
            body,
            ..
        } => {
            locals.push();
            if let Some(tname) =
                let_binding_type(ty.as_ref().map(|t| &t.node), Some(&init.node), ctx, locals)
            {
                locals.define(name.as_str(), &tname);
            }
            collect_one_expr_and_refs(
                &init.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                init.span,
            );
            collect_expr_edges_and_refs(
                &body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            locals.pop();
        }
        Stmt::AwaitUsingVar { name, ty, init, .. } => {
            if let Some(tname) =
                let_binding_type(ty.as_ref().map(|t| &t.node), Some(&init.node), ctx, locals)
            {
                locals.define(name.as_str(), &tname);
            }
            collect_one_expr_and_refs(
                &init.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                init.span,
            );
        }
        // RFC 044：yield 值表达式照常收集引用（语义索引不关心迭代器脱糖）。
        Stmt::YieldReturn { value } => {
            collect_one_expr_and_refs(
                &value.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                value.span,
            );
        }
        Stmt::YieldBreak => {}
        Stmt::Lock { expr, body } => {
            locals.push();
            collect_one_expr_and_refs(
                &expr.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                expr.span,
            );
            collect_expr_edges_and_refs(
                &body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            locals.pop();
        }
        Stmt::ForC {
            init,
            cond,
            inc,
            body,
        } => {
            locals.push();
            if let Some(i) = init {
                collect_stmt_edges_and_refs(
                    &i.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    i.span,
                );
            }
            if let Some(c) = cond {
                collect_one_expr_and_refs(
                    &c.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    c.span,
                );
            }
            if let Some(i) = inc {
                collect_stmt_edges_and_refs(
                    &i.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    i.span,
                );
            }
            collect_expr_edges_and_refs(
                &body.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            locals.pop();
        }
        Stmt::Break | Stmt::Continue => {}
        Stmt::DeconstructAssign { value, .. } => {
            collect_one_expr_and_refs(
                &value.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                value.span,
            );
        }
    }
    let _ = span;
}

/// 遍历单个表达式，识别 Call/MethodCall/New/FieldAccess 边 + Read/Write/Call 引用。
fn collect_one_expr_and_refs(
    expr: &Expr,
    caller_id: u32,
    file_id: FileId,
    ctx: &CollectContext,
    locals: &mut LocalScope,
    edges: &mut Vec<Edge>,
    refs: &mut ReferenceTable,
    next_ref_id: &mut u32,
    span: Span,
) {
    // 推入一个引用条目（辅助闭包，避免重复样板代码）
    let mut push_ref = |symbol_id: u32, context: ReferenceContext| {
        refs.push(ReferenceEntry::new(
            *next_ref_id,
            symbol_id,
            file_id,
            span.start,
            span.end,
            context,
        ));
        *next_ref_id += 1;
    };

    match expr {
        Expr::Call { func, args, .. } => {
            let mut emitted = false;
            if let Some(callee_name) = extract_callee_name(&func.node) {
                if let Some(callee_id) = ctx.lookup(&callee_name) {
                    edges.push(Edge::new(
                        caller_id,
                        callee_id,
                        EdgeKind::Call,
                        file_id,
                        span.start,
                        span.end,
                        true,
                    ));
                    push_ref(callee_id, ReferenceContext::Call);
                    emitted = true;
                }
            }
            // K3：类内裸方法调用——`Double(21)` 于 Quad 方法体 → `Quad.Double`。
            // typeck 对裸实例方法调用重写为 `this.Double(21)`（typed_body），但
            // 本收集器遍历的是原始 body，故在此补 owner 归属解析。自由函数调用
            // （caller 无 owner）与裸名已命中符号的情况不进入此路径。
            if !emitted {
                if let Some(owner) = &locals.owner {
                    if let Some(callee_name) = extract_callee_name(&func.node) {
                        let full = format!("{owner}.{callee_name}");
                        if let Some(callee_id) = ctx.lookup(&full) {
                            edges.push(Edge::new(
                                caller_id,
                                callee_id,
                                EdgeKind::MethodCall,
                                file_id,
                                span.start,
                                span.end,
                                false,
                            ));
                            push_ref(callee_id, ReferenceContext::Call);
                        }
                    }
                }
            }
            for arg in args {
                collect_one_expr_and_refs(
                    &arg.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    arg.span,
                );
            }
            collect_one_expr_and_refs(
                &func.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                func.span,
            );
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            let callee_name = resolve_method_callee(&receiver.node, method, ctx, locals);
            if let Some(callee_id) = callee_name.and_then(|n| ctx.lookup(&n)) {
                edges.push(Edge::new(
                    caller_id,
                    callee_id,
                    EdgeKind::MethodCall,
                    file_id,
                    span.start,
                    span.end,
                    false,
                ));
                push_ref(callee_id, ReferenceContext::Call);
            }
            collect_one_expr_and_refs(
                &receiver.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                receiver.span,
            );
            for arg in args {
                collect_one_expr_and_refs(
                    &arg.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    arg.span,
                );
            }
        }
        Expr::New { ty, args, .. } => {
            if let Some(type_name) = extract_type_name(&ty.node) {
                if let Some(callee_id) = ctx.lookup(&type_name) {
                    edges.push(Edge::new(
                        caller_id,
                        callee_id,
                        EdgeKind::New,
                        file_id,
                        span.start,
                        span.end,
                        true,
                    ));
                    push_ref(callee_id, ReferenceContext::Read);
                }
            }
            for arg in args {
                collect_one_expr_and_refs(
                    &arg.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    arg.span,
                );
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_one_expr_and_refs(
                &left.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                left.span,
            );
            collect_one_expr_and_refs(
                &right.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                right.span,
            );
        }
        // 赋值表达式：目标与值均递归收集（目标/值内调用与引用均入图）。
        Expr::Assign { target, value } => {
            collect_one_expr_and_refs(
                &target.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                target.span,
            );
            collect_one_expr_and_refs(
                &value.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                value.span,
            );
        }
        Expr::Unary { expr: inner, .. } => collect_one_expr_and_refs(
            &inner.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            inner.span,
        ),
        Expr::Field { receiver, .. } => {
            // 字段读取——记录 Read 引用（具体字段符号解析在 M3+ 接入 typed_hir 后完善）
            collect_one_expr_and_refs(
                &receiver.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                receiver.span,
            );
        }
        Expr::Index { receiver, index } => {
            collect_one_expr_and_refs(
                &receiver.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                receiver.span,
            );
            collect_one_expr_and_refs(
                &index.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                index.span,
            );
        }
        Expr::Await(inner) => collect_one_expr_and_refs(
            &inner.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            inner.span,
        ),
        Expr::Block(b) => {
            locals.push();
            collect_expr_edges_and_refs(
                &b.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            locals.pop();
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_one_expr_and_refs(
                &cond.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                cond.span,
            );
            locals.push();
            collect_expr_edges_and_refs(
                &then_branch.stmts,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
            );
            if let Some(eb) = else_branch {
                collect_expr_edges_and_refs(
                    &eb.stmts,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                );
            }
            locals.pop();
        }
        Expr::Cast { expr: inner, .. } => collect_one_expr_and_refs(
            &inner.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            inner.span,
        ),
        Expr::Comptime(inner) => collect_one_expr_and_refs(
            &inner.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            inner.span,
        ),
        Expr::Coalesce { left, right } => {
            collect_one_expr_and_refs(
                &left.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                left.span,
            );
            collect_one_expr_and_refs(
                &right.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                right.span,
            );
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_one_expr_and_refs(
                &cond.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                cond.span,
            );
            collect_one_expr_and_refs(
                &then_branch.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                then_branch.span,
            );
            collect_one_expr_and_refs(
                &else_branch.node,
                caller_id,
                file_id,
                ctx,
                locals,
                edges,
                refs,
                next_ref_id,
                else_branch.span,
            );
        }
        Expr::NullCond { access } => collect_one_expr_and_refs(
            &access.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            access.span,
        ),
        Expr::ForceDeref { access } => collect_one_expr_and_refs(
            &access.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            access.span,
        ),
        Expr::RefArg { expr: inner, .. } => collect_one_expr_and_refs(
            &inner.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            inner.span,
        ),
        Expr::NamedArg { expr: inner, .. } => collect_one_expr_and_refs(
            &inner.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            inner.span,
        ),
        Expr::Box { expr: inner, .. } => collect_one_expr_and_refs(
            &inner.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            inner.span,
        ),
        Expr::Unbox { expr: inner, .. } => collect_one_expr_and_refs(
            &inner.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            inner.span,
        ),
        // 标识符引用——尝试解析为已知符号，记录 Read 引用
        Expr::Ident(name) => {
            if let Some(sym_id) = ctx.lookup(name.as_str()) {
                push_ref(sym_id, ReferenceContext::Read);
            }
        }
        Expr::Path(path) => {
            if path.len() == 1 {
                if let Some(sym_id) = ctx.lookup(path[0].as_str()) {
                    push_ref(sym_id, ReferenceContext::Read);
                }
            }
        }
        // lambda 体非叶子：体内的调用边/引用属于宿主函数的索引视野（LSP
        // goto-ref / inspect 依赖）——当叶子跳过会造成 lambda 内符号「零引用」
        // 盲区（lambda 内裸调实例/静态方法时尤为常见）。lambda 参数入子作用域
        // 遮蔽同名外层局部，避免把体内标识符误挂到外层局部符号。
        Expr::Lambda(l) => {
            locals.push();
            for p in &l.params {
                locals.define(p.name.as_str(), "");
            }
            match &l.body {
                ast::LambdaBody::Expr(e) => collect_one_expr_and_refs(
                    &e.node,
                    caller_id,
                    file_id,
                    ctx,
                    locals,
                    edges,
                    refs,
                    next_ref_id,
                    e.span,
                ),
                ast::LambdaBody::Block(b) => {
                    collect_expr_edges_and_refs(
                        &b.stmts,
                        caller_id,
                        file_id,
                        ctx,
                        locals,
                        edges,
                        refs,
                        next_ref_id,
                    );
                    if let Some(tail) = &b.tail {
                        collect_one_expr_and_refs(
                            &tail.node,
                            caller_id,
                            file_id,
                            ctx,
                            locals,
                            edges,
                            refs,
                            next_ref_id,
                            tail.span,
                        );
                    }
                }
            }
            locals.pop();
        }
        // 叶子表达式——无递归
        Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::BoolLit(_)
        | Expr::StringLit(_)
        | Expr::CharLit(_)
        | Expr::This
        | Expr::Base
        | Expr::Null
        | Expr::Default { .. }
        | Expr::TypeOf(_)
        | Expr::ExpressionLit(_)
        | Expr::CollectionExpr { .. }
        | Expr::StackSpanLit { .. }
        | Expr::Is { .. }
        | Expr::With { .. } => {}
        Expr::InterpolatedString { parts } => {
            for p in parts {
                if let ast::InterpPart::Expr(hole) = p {
                    collect_one_expr_and_refs(
                        &hole.expr.node,
                        caller_id,
                        file_id,
                        ctx,
                        locals,
                        edges,
                        refs,
                        next_ref_id,
                        hole.expr.span,
                    );
                }
            }
        }
        // Query / Switch 表达式——M3+ 扩展
        Expr::Query(_) | Expr::Switch(_) | Expr::SwitchForm(_) => {}
        // `new T[n]`：长度表达式可能含调用/引用。
        Expr::NewArray { length, .. } => collect_one_expr_and_refs(
            &length.node,
            caller_id,
            file_id,
            ctx,
            locals,
            edges,
            refs,
            next_ref_id,
            length.span,
        ),
    }
}

/// 从 `Expr::Call.func` 提取被调用函数名。
fn extract_callee_name(func: &Expr) -> Option<String> {
    match func {
        Expr::Ident(name) => Some(name.to_string()),
        Expr::Path(path) if path.len() == 1 => Some(path[0].to_string()),
        _ => None,
    }
}

/// 从 `Expr::New.ty` 提取类型名。
fn extract_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Named { path, .. } if !path.is_empty() => Some(path.last().unwrap().to_string()),
        _ => None,
    }
}

/// 解析 MethodCall 的 callee 符号名。
///
/// K3：从「接收者类型 → 方法符号」解析——receiver 的类名 + 方法名拼
/// `"Class.method"` 查符号表（方法符号注册为 `"Class.method"`，原 M2 裸方法名
/// 独立查找对实例方法恒 miss）。实例方法调用（`q.Double(21)`）、静态方法调用
/// （`Helper.Double(21)`）统一经接收者类型解析，端点正确落在符号表。
/// 退化回退：裸方法名独立查找（保持 M2 行为——方法符号为 `"Class.method"`
/// 时一般 miss，仅兜底自由函数同名场景）。
fn resolve_method_callee(
    receiver: &Expr,
    method: &Ident,
    ctx: &CollectContext,
    locals: &LocalScope,
) -> Option<String> {
    if let Some(ty_name) = receiver_type_name(receiver, ctx, locals) {
        let full = format!("{ty_name}.{method}");
        if ctx.lookup(&full).is_some() {
            return Some(full);
        }
    }
    if ctx.lookup(method.as_str()).is_some() {
        return Some(method.to_string());
    }
    None
}

/// 解析接收者表达式的类型名（MethodCall callee 解析前置）。
///
/// K3 覆盖最常见的接收者形态：
/// - `Ident(name)`：name 为已注册类型符号（静态调用 `Helper.Double`）→ 类型名；
///   否则查局部变量表（`Helper h = new Helper(); h.Double(21)`）→ `h` 的类型；
/// - `Path(…末段)`：命名空间限定类型（`NS.Helper.Double`）→ 末段；
/// - `New { ty }`：`new Helper().Double(21)` → `Helper`；
/// - `This`/`Base`：当前方法属主类；
/// - `Field { receiver, field }`：`field` 为已注册类型 → 限定类型访问
///   （`NS.Helper` → `Helper`）；成员字段类型解析需 registry——残余登记；
/// - `Cast { ty }`：显式转换目标类型；
/// - `NullCond`/`ForceDeref`：解包后递归。
///
/// 未覆盖形态（链式调用返回类型、`Factory().Method()` 等）返回 None——残余登记，
/// 不假解析。
fn receiver_type_name(
    receiver: &Expr,
    ctx: &CollectContext,
    locals: &LocalScope,
) -> Option<String> {
    match receiver {
        Expr::Ident(name) => {
            if ctx.lookup(name.as_str()).is_some() {
                return Some(name.to_string());
            }
            locals.lookup(name.as_str()).map(str::to_string)
        }
        Expr::Path(path) if !path.is_empty() => {
            let last = path.last().unwrap();
            if ctx.lookup(last.as_str()).is_some() {
                Some(last.to_string())
            } else {
                None
            }
        }
        Expr::New { ty, .. } => extract_type_name(&ty.node),
        Expr::This | Expr::Base => locals.owner.clone(),
        Expr::Field {
            receiver: inner,
            field,
        } => {
            if ctx.lookup(field.as_str()).is_some() {
                Some(field.to_string())
            } else {
                let _ = receiver_type_name(&inner.node, ctx, locals);
                None
            }
        }
        Expr::Cast {
            expr: inner, ty, ..
        } => receiver_type_name(&inner.node, ctx, locals).or_else(|| extract_type_name(&ty.node)),
        Expr::NullCond { access } | Expr::ForceDeref { access } => {
            receiver_type_name(&access.node, ctx, locals)
        }
        _ => None,
    }
}

/// 推导 `let`/`using` 绑定变量的类型名。
///
/// 显式类型注解（`Helper h = …`）优先；`var q = new Quad()` 形态从初始化
/// 表达式推断（`New`/`Cast` 等）。无法推断返回 None（残余登记）。
fn let_binding_type(
    ty: Option<&Type>,
    init: Option<&Expr>,
    ctx: &CollectContext,
    locals: &LocalScope,
) -> Option<String> {
    if let Some(t) = ty {
        if let Some(name) = extract_type_name(t) {
            return Some(name);
        }
    }
    init.and_then(|e| receiver_type_name(e, ctx, locals))
}

/// 从 [`ast::TypeId`] 提取类型名（K3 局部变量参数类型解析）。
///
/// 仅解析可命名类型（`Named`/`Generic`）；可空、`ref` 解包后递归；基元类型
/// 与容器类型返回 None（其上方法非项目符号，MethodCall 边无意义）。
fn type_id_type_name(ty: &ast::TypeId) -> Option<String> {
    match ty {
        ast::TypeId::Named(n) => Some(n.to_string()),
        ast::TypeId::Generic(n) => Some(n.to_string()),
        ast::TypeId::Nullable { inner } => type_id_type_name(inner),
        ast::TypeId::Ref { inner, .. } => type_id_type_name(inner),
        _ => None,
    }
}

/// 收集虚分派保守组——接口方法 → 所有实现方法。
fn collect_virtual_dispatch_groups(
    registry: &TypeRegistry,
    included: &std::collections::HashSet<FileId>,
    ctx: &CollectContext,
) -> Vec<VirtualDispatchGroup> {
    let mut merged: HashMap<u32, Vec<u32>> = HashMap::new();

    // 收集项目文件集合中的所有接口
    let interfaces: Vec<(&Ident, &typeck::NominalType)> = registry
        .types
        .iter()
        .filter(|(_, n)| n.kind == TypeKind::Interface && included.contains(&n.span.file_id))
        .collect();

    // 对每个实现类，遍历其 bases 中的接口，构造虚分派组
    for (class_name, class_nominal) in &registry.types {
        if !included.contains(&class_nominal.span.file_id) || class_nominal.kind != TypeKind::Class
        {
            continue;
        }

        for base in &class_nominal.bases {
            let iface = interfaces
                .iter()
                .find(|(name, _)| name.as_str() == base.as_str());
            if let Some((_, iface_nominal)) = iface {
                for (method_name, _iface_overloads) in &iface_nominal.methods {
                    let iface_method_full = format!("{}.{}", base, method_name);
                    let class_method_full = format!("{}.{}", class_name, method_name);
                    let iface_method_id = match ctx.lookup(&iface_method_full) {
                        Some(id) => id,
                        None => continue,
                    };
                    let impl_method_id = match ctx.lookup(&class_method_full) {
                        Some(id) => id,
                        None => continue,
                    };
                    merged
                        .entry(iface_method_id)
                        .or_default()
                        .push(impl_method_id);
                }
            }
        }
    }

    merged
        .into_iter()
        .map(|(iface_id, impls)| VirtualDispatchGroup::new(iface_id, impls))
        .collect()
}

/// 将 `ast::Visibility` 映射为 `arcgr::Visibility`。
fn map_visibility(vis: ast::Visibility) -> Visibility {
    match vis {
        ast::Visibility::Public => Visibility::Public,
        ast::Visibility::Internal => Visibility::Internal,
        ast::Visibility::Protected => Visibility::Protected,
        ast::Visibility::Private => Visibility::Private,
    }
}

/// 从类型名字符串构造 `arcgr::TypeSig`。
///
/// 仿照 `export_collector::type_name_to_type_sig` 但返回 `arcgr::TypeSig`。
fn build_type_sig_from_name(name: &str, registry: &TypeRegistry) -> TypeSig {
    match name {
        "int" => TypeSig::Int,
        "long" => TypeSig::Long,
        "float" => TypeSig::Float,
        "double" => TypeSig::Double,
        "bool" => TypeSig::Bool,
        "string" => TypeSig::String,
        "void" => TypeSig::Unit,
        "object" => TypeSig::Object,
        "uint" => TypeSig::UInt,
        "ulong" => TypeSig::ULong,
        "ushort" => TypeSig::UShort,
        "sbyte" => TypeSig::SByte,
        _ => {
            if let Some(suffix) = name.strip_prefix("List_") {
                return TypeSig::List {
                    element_type: Box::new(build_type_sig_from_name(suffix, registry)),
                };
            }
            let generic_args: Vec<TypeSig> = registry
                .types
                .get(name)
                .map(|n| {
                    n.generic_params
                        .iter()
                        .enumerate()
                        .map(|(i, _)| TypeSig::GenericParam {
                            param_index: i as u8,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            TypeSig::Named {
                fully_qualified_name: name.to_string(),
                generic_args,
            }
        }
    }
}

/// 构造类型的 `TypeSig::Named` 或 `TypeSig::Variant`。
fn build_named_type_sig(
    name: &Ident,
    registry: &TypeRegistry,
    nominal: &typeck::NominalType,
) -> TypeSig {
    let fqn = if nominal.namespace.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", nominal.namespace.join("."), name)
    };
    match nominal.kind {
        TypeKind::Variant | TypeKind::Enum => {
            let cases: Vec<arcgr::VariantCase> = nominal
                .variants
                .iter()
                .map(|v| arcgr::VariantCase {
                    case_name: v.name.to_string(),
                    payload_type: match &v.payload {
                        Some(ty) => build_type_sig_from_name(ty, registry),
                        None => TypeSig::Unit,
                    },
                    discriminant: v.discriminant,
                })
                .collect();
            TypeSig::Variant {
                fully_qualified_name: fqn,
                cases,
            }
        }
        _ => {
            let generic_args: Vec<TypeSig> = nominal
                .generic_params
                .iter()
                .enumerate()
                .map(|(i, _)| TypeSig::GenericParam {
                    param_index: i as u8,
                })
                .collect();
            TypeSig::Named {
                fully_qualified_name: fqn,
                generic_args,
            }
        }
    }
}

/// 构造方法的 `TypeSig::Method`。
fn build_method_type_sig(
    class_name: &str,
    sig: &typeck::OopMethodSig,
    registry: &TypeRegistry,
) -> TypeSig {
    let receiver = TypeSig::Named {
        fully_qualified_name: class_name.to_string(),
        generic_args: vec![],
    };
    let params: Vec<TypeSig> = sig
        .params
        .iter()
        .map(|p| build_type_sig_from_name(&p.ty, registry))
        .collect();
    let ret = build_type_sig_from_name(&sig.ret, registry);
    let is_virtual = matches!(
        sig.modifier,
        ast::MethodModifier::Virtual
            | ast::MethodModifier::Override
            | ast::MethodModifier::Abstract
            | ast::MethodModifier::OverrideAbstract
    );
    TypeSig::Method {
        receiver: Box::new(receiver),
        params,
        ret: Box::new(ret),
        is_virtual,
        vtable_slot: 0,
    }
}

/// 从 [`ast::TypeId`] 构造 [`TypeSig`]——用于自由函数的参数/返回类型签名。
///
/// 与 [`build_type_sig_from_name`] 互补：后者从字符串名（AST `Type` 节点）
/// 构造，本函数从语义 [`TypeId`]（typeck 推导结果）构造。
fn build_type_sig_from_type_id(ty: &ast::TypeId, registry: &TypeRegistry) -> TypeSig {
    match ty {
        ast::TypeId::Void => TypeSig::Unit,
        ast::TypeId::Int => TypeSig::Int,
        ast::TypeId::Long => TypeSig::Long,
        ast::TypeId::Short => TypeSig::Int,
        ast::TypeId::Byte => TypeSig::Int,
        ast::TypeId::Char => TypeSig::Int,
        ast::TypeId::Float => TypeSig::Float,
        ast::TypeId::Double => TypeSig::Double,
        ast::TypeId::Bool => TypeSig::Bool,
        ast::TypeId::UInt => TypeSig::UInt,
        ast::TypeId::ULong => TypeSig::ULong,
        ast::TypeId::UShort => TypeSig::UShort,
        ast::TypeId::SByte => TypeSig::SByte,
        ast::TypeId::String => TypeSig::String,
        ast::TypeId::Object => TypeSig::Object,
        ast::TypeId::Named(name) => build_type_sig_from_name(name.as_str(), registry),
        ast::TypeId::Generic(name) => TypeSig::Named {
            fully_qualified_name: name.to_string(),
            generic_args: vec![],
        },
        ast::TypeId::Ref { inner, .. } => build_type_sig_from_type_id(inner, registry),
        ast::TypeId::Func { params, ret } => {
            let p: Vec<TypeSig> = params
                .iter()
                .map(|p| build_type_sig_from_type_id(p, registry))
                .collect();
            let r = build_type_sig_from_type_id(ret, registry);
            TypeSig::Func {
                params: p,
                ret: Box::new(r),
                captures: false,
            }
        }
        ast::TypeId::Task { inner } => TypeSig::TaskHandle {
            result_type: Box::new(build_type_sig_from_type_id(inner, registry)),
        },
        ast::TypeId::IEnumerable { inner } => TypeSig::List {
            element_type: Box::new(build_type_sig_from_type_id(inner, registry)),
        },
        ast::TypeId::IQueryable { inner } => TypeSig::List {
            element_type: Box::new(build_type_sig_from_type_id(inner, registry)),
        },
        ast::TypeId::Array { elem } => TypeSig::List {
            element_type: Box::new(build_type_sig_from_type_id(elem, registry)),
        },
        ast::TypeId::Expression { inner } => TypeSig::Expression {
            delegate_type: Box::new(build_type_sig_from_type_id(inner, registry)),
        },
        ast::TypeId::Nullable { inner } => TypeSig::Nullable {
            inner: Box::new(build_type_sig_from_type_id(inner, registry)),
        },
        ast::TypeId::Vector { elem, .. } => TypeSig::List {
            element_type: Box::new(build_type_sig_from_type_id(elem, registry)),
        },
        // RFC 005 / RFC 024 TypeSig Span=22
        ast::TypeId::Span { elem, .. } => TypeSig::Span {
            element_type: Box::new(build_type_sig_from_type_id(elem, registry)),
        },
        ast::TypeId::Infer | ast::TypeId::Error => TypeSig::Unit,
    }
}

/// 构造自由函数的 `TypeSig::Func`。
fn build_fn_type_sig(fn_: &TypedFn, registry: &TypeRegistry) -> TypeSig {
    let params: Vec<TypeSig> = fn_
        .params
        .iter()
        .map(|(_, ty)| build_type_sig_from_type_id(ty, registry))
        .collect();
    let ret = build_type_sig_from_type_id(&fn_.ret, registry);
    TypeSig::Func {
        params,
        ret: Box::new(ret),
        captures: false,
    }
}

// ============================================================================
// ContextManifest 收集（RFC 034 M4）
// ============================================================================

/// 从 `ArcManifest` + `SymbolTable` 收集 `ContextManifest`（L0 + L1）。
///
/// **L0 ProjectOverview** 来源：
/// - `name` / `version` / `edition` / `kind` / `namespace` ← `arc.toml [package]`
/// - `arc_abi_version` ← 固定 `1`（M2 默认）
/// - `llvm_version` ← 固定 `22`（项目锁定 LLVM 22）
/// - `target_triple` ← `""`（CLI 模式暂不填，由 `arc build --target` 触发）
/// - `dependencies` / `capabilities` ← 空列表（待 `[dependencies]` / `[capabilities]` 段扩展）
/// - `namespaces` ← 单一条目 `[package].namespace`（source_crate=0）
/// - `architecture_redlines` ← 空列表（lint 阶段产出）
/// - `crate_dag_summary` ← `{ crate_count: 1, edge_count: 0 }`（单 crate MVP）
///
/// **L1 ModuleSurface** 来源：
/// - `crates` ← 单一 crate 条目（crate_id=0）
///   - `public_apis` 从 SymbolTable 收集所有 `Visibility::Public` 条目
///   - `namespaces` ← `[0]`（关联 L0 namespaces[0]）
/// - `dag_edges` ← 空列表（M2 单 crate MVP，无跨 crate 依赖）
fn collect_context_manifest(manifest: &ArcManifest, symbol_table: &SymbolTable) -> ContextManifest {
    let l0 = build_l0_project_overview(manifest);
    let l1 = build_l1_module_surface(manifest, symbol_table);
    ContextManifest::new(l0, l1)
}

/// 构造 L0 ProjectOverview——从 `arc.toml [package]` 段提取项目元信息。
fn build_l0_project_overview(manifest: &ArcManifest) -> L0ProjectOverview {
    let pkg = &manifest.package;

    // 解析 "major.minor.patch" 版本字符串
    let (vmaj, vmin, vpatch) = parse_version(&pkg.version);

    // 解析 edition 字符串 → u16
    let edition: u16 = pkg.edition.parse().unwrap_or(1);

    // kind 字符串 → ProjectKind 枚举
    // RFC 017 D8 v1.0：删除 plugin 概念——"plugin" 字符串不再有专属变体，
    // 走默认 Executable 分支（兼容历史 arc.toml 但不再特殊处理）。
    // 动态库由 `kind = "library"` + `dynamic = true` 组合表达，
    // dynamic 字段在 L0 概览层不单独序列化（ProjectKind::DynamicLibrary 已编码此语义）。
    let kind = match pkg.kind.as_str() {
        "library" => ProjectKind::Library,
        "test" => ProjectKind::Test,
        // "executable" / "binary" / "plugin"(legacy) / 其他 → 默认 Executable
        _ => ProjectKind::Executable,
    };

    // namespaces：单一条目（M2 单 crate MVP）
    let namespaces = vec![NamespaceEntry::new(&pkg.namespace, 0)];

    L0ProjectOverview {
        name: pkg.name.clone(),
        kind,
        version_major: vmaj,
        version_minor: vmin,
        version_patch: vpatch,
        edition,
        // M2 默认值：ABI 版本 1 + LLVM 22（项目锁定）
        arc_abi_version: 1,
        llvm_version: 22,
        // CLI 模式暂不填 target_triple，由 `arc build --target` 触发时填充
        target_triple: String::new(),
        // M2 阶段 dependencies / capabilities 空——待 `[dependencies]` / `[capabilities]` 段扩展
        dependencies: Vec::new(),
        capabilities: Vec::new(),
        namespaces,
        // lint 阶段产出（M2 阶段为空）
        architecture_redlines: Vec::new(),
        // 单 crate MVP
        crate_dag_summary: CrateDagSummary::new(1, 0),
    }
}

/// 构造 L1 ModuleSurface——单 crate MVP，public_apis 从 SymbolTable 收集。
fn build_l1_module_surface(manifest: &ArcManifest, symbol_table: &SymbolTable) -> L1ModuleSurface {
    let pkg = &manifest.package;

    // 收集所有 public SymbolEntry 作为公共 API 面
    let public_apis: Vec<PublicApiEntry> = symbol_table
        .entries
        .iter()
        .filter(|s| s.visibility == Visibility::Public)
        .map(|s| PublicApiEntry::new(s.symbol_id, symbol_kind_to_public_api_kind(s.kind), 0))
        .collect();

    let crate_module = CrateModule {
        crate_id: 0,
        name: pkg.name.clone(),
        // 相对项目根路径——M2 单文件 MVP 用 manifest 所在目录的相对表示
        path: String::new(),
        // 模块职责描述——M2 阶段不自动推导（待 README.md / mod.rs 解析扩展）
        responsibility: String::new(),
        public_apis,
        // 关联 L0 namespaces[0]
        namespaces: vec![0],
    };

    L1ModuleSurface {
        crates: vec![crate_module],
        // M2 单 crate MVP——无跨 crate 依赖边
        dag_edges: Vec::new(),
    }
}

/// `SymbolKind` → `PublicApiKind` 映射（一一对应，仅枚举名不同）。
fn symbol_kind_to_public_api_kind(kind: SymbolKind) -> PublicApiKind {
    match kind {
        SymbolKind::Function => PublicApiKind::Function,
        SymbolKind::Method => PublicApiKind::Method,
        SymbolKind::StaticMethod => PublicApiKind::StaticMethod,
        SymbolKind::Property => PublicApiKind::Property,
        SymbolKind::Field => PublicApiKind::Property, // Field 归类为 Property（无独立 PublicApiKind::Field）
        SymbolKind::Class => PublicApiKind::Class,
        SymbolKind::Struct => PublicApiKind::Struct,
        SymbolKind::Interface => PublicApiKind::Interface,
        SymbolKind::Enum => PublicApiKind::Enum,
        SymbolKind::Variant => PublicApiKind::Variant,
        SymbolKind::Constant => PublicApiKind::Function, // Constant 归类为 Function（无独立 PublicApiKind）
        SymbolKind::Module => PublicApiKind::Module,
    }
}

/// 解析 `"major.minor.patch"` 版本字符串。
///
/// 缺失字段补 0；解析失败返回 `(0, 0, 0)`。
fn parse_version(s: &str) -> (u16, u16, u16) {
    let parts: Vec<&str> = s.split('.').collect();
    let parse = |idx: usize| -> u16 { parts.get(idx).and_then(|p| p.parse().ok()).unwrap_or(0) };
    (parse(0), parse(1), parse(2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use std::path::PathBuf;

    /// 单文件项目文件表辅助——collect_arcgr_file 的项目文件表参数（K2 后签名）。
    fn single_file_entry() -> Vec<FileEntry> {
        vec![FileEntry::new(1, "test.as".to_string(), 0, 0)]
    }

    /// 最小冒烟测试——空 TypeRegistry 应产出空 ArcgrFile（仅用户文件占位）。
    #[test]
    fn empty_input_produces_minimal_file() {
        let registry = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            shadowed_types: Default::default(),
            entry_package: None,
            delegate_aliases: std::collections::HashMap::new(),
        };
        let typeck = TypeChecker::with_registry(registry);
        let program = Program { items: vec![] };

        let file = collect_arcgr_file(&typeck, &[], &program, &single_file_entry(), None);

        // 空 FileTable 仍有 1 条用户文件占位
        assert_eq!(file.file_table.entries.len(), 1);
        assert_eq!(file.symbol_table.entries.len(), 0);
        assert_eq!(file.reference_graph.entry_points.len(), 0);
        assert_eq!(file.reference_graph.reachable_symbols.len(), 0);
        assert_eq!(file.reference_graph.unreachable_symbols.len(), 0);
        assert_eq!(file.reference_graph.edges.len(), 0);
    }

    /// 序列化 round-trip 验证——collect 产出的 ArcgrFile 必须可序列化与反序列化。
    #[test]
    fn collect_output_serializes_round_trip() {
        let registry = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            shadowed_types: Default::default(),
            entry_package: None,
            delegate_aliases: std::collections::HashMap::new(),
        };
        let typeck = TypeChecker::with_registry(registry);
        let program = Program { items: vec![] };

        let file = collect_arcgr_file(&typeck, &[], &program, &single_file_entry(), None);
        let bytes = arcgr::write_arcgr(&file);
        let file2 = arcgr::read_arcgr(&bytes).unwrap();
        assert_eq!(file, file2);
    }

    /// Schema 不变量验证——M2 阶段产出的 ArcgrFile 必须满足：
    /// 1. ContextManifest 为 None（M4 才填充）
    /// 2. 所有 SymbolEntry 的 intent_meta.role == None（M5 才填充）
    /// 3. ReferenceTable 字段存在（不再是空表占位）
    /// 4. 序列化字节流 Header 76 字节 + VERSION=2 + 后 4 section 偏移为 0
    #[test]
    fn m2_schema_invariants_hold() {
        let registry = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            shadowed_types: Default::default(),
            entry_package: None,
            delegate_aliases: std::collections::HashMap::new(),
        };
        let typeck = TypeChecker::with_registry(registry);
        let program = Program { items: vec![] };

        let file = collect_arcgr_file(&typeck, &[], &program, &single_file_entry(), None);

        // 1. ContextManifest 必须为 None（M2 不产出，M4 才填充）
        assert!(
            file.context_manifest.is_none(),
            "M2 阶段 ContextManifest 必须为 None"
        );

        // 2. 所有 SymbolEntry 的 intent_meta 必须为 None 占位（M5 才填充真实数据）
        for entry in &file.symbol_table.entries {
            assert_eq!(
                entry.intent_meta.role,
                arcgr::IntentRole::None,
                "M2 阶段符号 {} 的 intent_meta.role 必须为 None 占位",
                entry.name
            );
            assert!(
                entry.intent_meta.metadata.is_none(),
                "M2 阶段符号 {} 的 intent_meta.metadata 必须为 None",
                entry.name
            );
        }

        // 3. ReferenceTable 字段存在（即使为空也不再是占位符）
        // 这里仅验证字段可访问——具体填充由 collect_edges_and_refs 负责
        let _ = &file.reference_table;

        // 4. 序列化字节流 Header 验证
        let bytes = arcgr::write_arcgr(&file);
        assert!(bytes.len() >= arcgr::HEADER_SIZE as usize);

        let header = arcgr::ArcgrHeader::deserialize(&bytes).unwrap();
        assert_eq!(header.version, arcgr::VERSION);
        assert_eq!(arcgr::VERSION, 2, "M2 完整 schema 版本必须是 2");
        assert_eq!(arcgr::HEADER_SIZE, 76, "Header 大小必须是 76 字节");

        // M2 阶段后 4 个 section 偏移/大小必须为 0
        assert_eq!(header.context_manifest_off, 0);
        assert_eq!(header.context_manifest_size, 0);
        assert_eq!(header.type_relation_graph_off, 0);
        assert_eq!(header.type_relation_graph_size, 0);
        assert_eq!(header.completion_table_off, 0);
        assert_eq!(header.completion_table_size, 0);
        assert_eq!(header.diagnostic_cache_off, 0);
        assert_eq!(header.diagnostic_cache_size, 0);

        // has_section API 验证
        assert!(header.has_section(arcgr::HeaderSection::FileTable));
        assert!(!header.has_section(arcgr::HeaderSection::ContextManifest));
        assert!(!header.has_section(arcgr::HeaderSection::TypeRelationGraph));
        assert!(!header.has_section(arcgr::HeaderSection::CompletionTable));
        assert!(!header.has_section(arcgr::HeaderSection::DiagnosticCache));
    }

    /// M4 ContextManifest 填充验证——传入 manifest 时 collect_arcgr_file 必须填充
    /// ContextManifest（L0 + L1），且 L0 字段从 manifest 正确提取。
    #[test]
    fn context_manifest_filled_when_manifest_provided() {
        use crate::manifest::{
            ArcManifest, CompilerSection, NativeSection, PackageSection, QifSection,
        };

        let manifest = ArcManifest {
            package: PackageSection {
                name: "TestApp".into(),
                edition: "1".into(),
                version: "2.3.4".into(),
                kind: "library".into(),
                dynamic: false,
                namespace: "Arc.TestApp".into(),
                global_usings: Vec::new(),
                internals_visible_to: Vec::new(),
            },
            dependencies: std::collections::BTreeMap::new(),
            native: NativeSection::default(),
            ui: None,
            qif: QifSection::default(),
            compiler: CompilerSection::default(),
            workspace: crate::manifest::WorkspaceSection::default(),
            std: None,
            path: PathBuf::from("/tmp/arc.toml"),
        };

        let registry = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            shadowed_types: Default::default(),
            entry_package: None,
            delegate_aliases: std::collections::HashMap::new(),
        };
        let typeck = TypeChecker::with_registry(registry);
        let program = Program { items: vec![] };

        let file = collect_arcgr_file(
            &typeck,
            &[],
            &program,
            &single_file_entry(),
            Some(&manifest),
        );

        // ContextManifest 必须填充
        let cm = file
            .context_manifest
            .as_ref()
            .expect("manifest=Some 时 context_manifest 必须填充");

        // L0 字段从 manifest 正确提取
        let l0 = &cm.l0_project;
        assert_eq!(l0.name, "TestApp");
        assert_eq!(l0.kind, ProjectKind::Library);
        assert_eq!(l0.version_major, 2);
        assert_eq!(l0.version_minor, 3);
        assert_eq!(l0.version_patch, 4);
        assert_eq!(l0.edition, 1);
        assert_eq!(l0.arc_abi_version, 1);
        assert_eq!(l0.llvm_version, 22);
        assert!(l0.target_triple.is_empty(), "CLI 模式 target_triple 应为空");

        // namespaces 单一条目（M2 单 crate MVP）
        assert_eq!(l0.namespaces.len(), 1);
        assert_eq!(l0.namespaces[0].name, "Arc.TestApp");
        assert_eq!(l0.namespaces[0].source_crate, 0);

        // 单 crate DAG summary
        assert_eq!(l0.crate_dag_summary.crate_count, 1);
        assert_eq!(l0.crate_dag_summary.edge_count, 0);

        // L1 模块面——单一 crate，无 DAG 边
        let l1 = &cm.l1_module_surface;
        assert_eq!(l1.crates.len(), 1);
        assert_eq!(l1.crates[0].crate_id, 0);
        assert_eq!(l1.crates[0].name, "TestApp");
        assert_eq!(l1.crates[0].namespaces, vec![0]);
        assert!(l1.dag_edges.is_empty());

        // round-trip 序列化
        let bytes = arcgr::write_arcgr(&file);
        let file2 = arcgr::read_arcgr(&bytes).unwrap();
        assert_eq!(file, file2);
    }

    /// M4 ContextManifest 不填充验证——manifest=None 时 context_manifest 必须为 None
    /// （保持向后兼容，单文件无 manifest 场景）。
    #[test]
    fn context_manifest_none_when_manifest_none() {
        let registry = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            shadowed_types: Default::default(),
            entry_package: None,
            delegate_aliases: std::collections::HashMap::new(),
        };
        let typeck = TypeChecker::with_registry(registry);
        let program = Program { items: vec![] };

        let file = collect_arcgr_file(&typeck, &[], &program, &single_file_entry(), None);
        assert!(
            file.context_manifest.is_none(),
            "manifest=None 时 context_manifest 必须为 None（向后兼容）"
        );
    }

    /// M4 parse_version 单元测试——验证 "major.minor.patch" 解析逻辑。
    #[test]
    fn parse_version_handles_standard_format() {
        assert_eq!(parse_version("1.2.3"), (1, 2, 3));
        assert_eq!(parse_version("0.1.0"), (0, 1, 0));
        assert_eq!(parse_version("1.0"), (1, 0, 0)); // 缺失 patch 补 0
        assert_eq!(parse_version("1"), (1, 0, 0)); // 缺失 minor/patch 补 0
        assert_eq!(parse_version(""), (0, 0, 0)); // 空字符串
        assert_eq!(parse_version("1.x.y"), (1, 0, 0)); // 非数字字段补 0
    }

    /// ReferenceTable 填充验证——当 TypeRegistry 有 class 实现 interface 时，
    /// collect_arcgr_file 必须产出非空 ReferenceTable，至少包含 Implement 引用。
    ///
    /// 这是 Task 1「填充 ReferenceTable 防止半成品」的关键回归测试。
    #[test]
    fn reference_table_filled_when_class_implements_interface() {
        use ast::{Ident, Span};
        use typeck::NominalType;

        let mut registry = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            shadowed_types: Default::default(),
            entry_package: None,
            delegate_aliases: std::collections::HashMap::new(),
        };

        // 构造 Shape interface + Circle class : Shape
        let user_span = Span {
            file_id: 1,
            start: 0,
            end: 100,
        };
        let shape = NominalType {
            name: Ident::from("Shape"),
            kind: TypeKind::Interface,
            vis: ast::Visibility::Public,
            is_abstract: false,
            is_readonly: false,
            is_record: false,
            fields: IndexMap::new(),
            methods: IndexMap::new(),
            bases: vec![],
            base_types: vec![],
            variants: vec![],
            generic_params: vec![],
            namespace: vec![],
            span: user_span,
            const_values: IndexMap::new(),
            constructors: vec![],
            soa: false,
            required_props: Default::default(),
        };
        let circle = NominalType {
            name: Ident::from("Circle"),
            kind: TypeKind::Class,
            vis: ast::Visibility::Public,
            is_abstract: false,
            is_readonly: false,
            is_record: false,
            fields: IndexMap::new(),
            methods: IndexMap::new(),
            bases: vec![Ident::from("Shape")],
            base_types: vec![],
            variants: vec![],
            generic_params: vec![],
            namespace: vec![],
            span: user_span,
            const_values: IndexMap::new(),
            constructors: vec![],
            soa: false,
            required_props: Default::default(),
        };
        registry.types.insert(Ident::from("Shape"), shape);
        registry.types.insert(Ident::from("Circle"), circle);

        let typeck = TypeChecker::with_registry(registry);
        let program = Program { items: vec![] };

        let file = collect_arcgr_file(&typeck, &[], &program, &single_file_entry(), None);

        // ReferenceTable 必须非空——至少包含 Circle -> Shape 的 Implement 引用
        assert!(
            !file.reference_table.entries.is_empty(),
            "ReferenceTable 必须填充——当存在 class 实现 interface 时，至少包含 Implement 引用"
        );

        // 验证至少有一个 Implement 引用
        let has_implement = file
            .reference_table
            .entries
            .iter()
            .any(|r| r.context == arcgr::ReferenceContext::Implement);
        assert!(
            has_implement,
            "ReferenceTable 必须包含至少一个 Implement 引用（Circle -> Shape）"
        );

        // 序列化 round-trip——确保填充后的 ReferenceTable 可正确序列化
        let bytes = arcgr::write_arcgr(&file);
        let file2 = arcgr::read_arcgr(&bytes).unwrap();
        assert_eq!(file, file2);
        assert_eq!(
            file2.reference_table.entries.len(),
            file.reference_table.entries.len(),
            "round-trip 后 ReferenceTable 条目数必须一致"
        );
    }

    /// IntentMeta 完整 schema 验证——所有 5 种 IntentRole 通过 with_intent_meta
    /// 设置后必须正确 round-trip。
    ///
    /// 这是 Task 2「IntentMeta schema 完整」的回归测试。
    #[test]
    fn intent_meta_all_roles_round_trip_through_arcgr_file() {
        use arcgr::{IntentMeta, IntentRole};

        let roles = [
            IntentRole::None,
            IntentRole::Facade,
            IntentRole::AbiBoundary,
            IntentRole::HotPath,
            IntentRole::Stable,
            IntentRole::Internal,
        ];

        for role in roles {
            let mut file = arcgr::ArcgrFile::new();
            file.symbol_table.push(
                arcgr::SymbolEntry::new(
                    0,
                    "test_symbol",
                    arcgr::SymbolKind::Function,
                    arcgr::Visibility::Public,
                    0,
                    0,
                    10,
                    arcgr::TypeSig::Unit,
                    None,
                )
                .with_intent_meta(IntentMeta::role_only(role)),
            );

            let bytes = arcgr::write_arcgr(&file);
            let file2 = arcgr::read_arcgr(&bytes).unwrap();

            assert_eq!(
                file2.symbol_table.entries[0].intent_meta.role, role,
                "IntentRole {:?} 必须 round-trip 一致",
                role
            );
        }
    }

    /// IntentMetadata 全变体验证——5 种 metadata 变体通过 with_intent_meta
    /// 设置后必须正确 round-trip。
    #[test]
    fn intent_metadata_all_variants_round_trip() {
        use arcgr::{IntentMeta, IntentMetadata, IntentRole};

        let cases: Vec<(IntentRole, IntentMetadata)> = vec![
            (
                IntentRole::HotPath,
                IntentMetadata::Hotness {
                    calls_per_sec: 10_000,
                    avg_latency_ns: 500,
                },
            ),
            (
                IntentRole::AbiBoundary,
                IntentMetadata::Boundary {
                    abi_version: 3,
                    contract_name: "Arc.Runtime".into(),
                },
            ),
            (
                IntentRole::Stable,
                IntentMetadata::Stability {
                    since_major: 1,
                    since_minor: 5,
                    deprecated: true,
                    deprecation_msg: "use NewApi instead".into(),
                },
            ),
            (
                IntentRole::Facade,
                IntentMetadata::FacadeLayer {
                    layer_index: 2,
                    parent_facade_symbol_ids: vec![10, 20, 30],
                },
            ),
            (
                IntentRole::Internal,
                IntentMetadata::InternalGroup {
                    group_name: "codegen::lower".into(),
                },
            ),
        ];

        for (role, metadata) in cases {
            let mut file = arcgr::ArcgrFile::new();
            file.symbol_table.push(
                arcgr::SymbolEntry::new(
                    0,
                    "test_symbol",
                    arcgr::SymbolKind::Function,
                    arcgr::Visibility::Public,
                    0,
                    0,
                    10,
                    arcgr::TypeSig::Unit,
                    None,
                )
                .with_intent_meta(IntentMeta::with_metadata(role, metadata.clone())),
            );

            let bytes = arcgr::write_arcgr(&file);
            let file2 = arcgr::read_arcgr(&bytes).unwrap();

            let actual = &file2.symbol_table.entries[0].intent_meta;
            assert_eq!(actual.role, role);
            assert_eq!(actual.metadata, Some(metadata));
        }
    }

    /// ContextManifest round-trip——完整 ContextManifest 通过 ArcgrFile 序列化后
    /// 必须正确还原，且 Header 的 context_manifest_off/size 字段被正确填充。
    ///
    /// 这是 Task 3「ContextManifest schema 完整」的回归测试。
    #[test]
    fn context_manifest_full_round_trip_through_arcgr_file() {
        use arcgr::{
            CapabilityDecl, ContextManifest, CrateDagSummary, CrateModule, DagEdge, DagEdgeKind,
            DependencyEntry, DependencySource, L0ProjectOverview, L1ModuleSurface, NamespaceEntry,
            ProjectKind, PublicApiEntry, PublicApiKind, RedlineEntry,
        };

        let l0 = L0ProjectOverview {
            name: "ArcProject".into(),
            kind: ProjectKind::Executable,
            version_major: 0,
            version_minor: 1,
            version_patch: 0,
            edition: 2024,
            arc_abi_version: 1,
            llvm_version: 22,
            target_triple: "x86_64-pc-windows-msvc".into(),
            dependencies: vec![
                DependencyEntry::new("Arc.Runtime", 1, 0, 0, DependencySource::Precompiled),
                DependencyEntry::new("Arc.IO", 0, 2, 1, DependencySource::Path),
            ],
            capabilities: vec![CapabilityDecl::new(1, 0), CapabilityDecl::new(2, 1)],
            namespaces: vec![NamespaceEntry::new("Arc", 0)],
            architecture_redlines: vec![RedlineEntry::new(101, 1, "lib.rs exceeds 80 lines")],
            crate_dag_summary: CrateDagSummary::new(2, 1),
        };
        let l1 = L1ModuleSurface {
            crates: vec![CrateModule {
                crate_id: 0,
                name: "arc".into(),
                path: "crates/arc".into(),
                responsibility: "Arc compiler driver".into(),
                public_apis: vec![
                    PublicApiEntry::new(0, PublicApiKind::Function, 0),
                    PublicApiEntry::new(1, PublicApiKind::Class, 0),
                ],
                namespaces: vec![0],
            }],
            dag_edges: vec![DagEdge::new(0, 1, DagEdgeKind::CompileDep)],
        };

        let mut file = arcgr::ArcgrFile::new();
        file.context_manifest = Some(ContextManifest::new(l0.clone(), l1.clone()));

        let bytes = arcgr::write_arcgr(&file);
        let file2 = arcgr::read_arcgr(&bytes).unwrap();

        // ContextManifest 必须 round-trip 完整
        assert_eq!(file, file2);
        assert!(file2.context_manifest.is_some());

        let cm = file2.context_manifest.unwrap();
        assert_eq!(cm.l0_project, l0);
        assert_eq!(cm.l1_module_surface, l1);

        // Header 必须正确填充 ContextManifest 偏移/大小
        let header = arcgr::ArcgrHeader::deserialize(&bytes).unwrap();
        assert_ne!(
            header.context_manifest_off, 0,
            "ContextManifest 偏移必须非 0"
        );
        assert_ne!(
            header.context_manifest_size, 0,
            "ContextManifest 大小必须非 0"
        );
        assert!(header.has_section(arcgr::HeaderSection::ContextManifest));

        // M2 阶段其他后 3 个 section 仍为 0（M3+ 才填充）
        assert!(!header.has_section(arcgr::HeaderSection::TypeRelationGraph));
        assert!(!header.has_section(arcgr::HeaderSection::CompletionTable));
        assert!(!header.has_section(arcgr::HeaderSection::DiagnosticCache));
    }

    /// 8 种 ReferenceContext 全覆盖——ReferenceTable 必须能存储所有 8 种引用上下文。
    ///
    /// 这是 Task 1「ReferenceTable 填充 8 种 ReferenceContext」的回归测试。
    #[test]
    fn reference_table_supports_all_8_contexts() {
        use arcgr::{ReferenceContext, ReferenceEntry, ReferenceTable};

        let contexts = [
            ReferenceContext::Read,
            ReferenceContext::Write,
            ReferenceContext::Call,
            ReferenceContext::Implement,
            ReferenceContext::Inherit,
            ReferenceContext::Import,
            ReferenceContext::TypeAnnotation,
            ReferenceContext::PatternMatch,
        ];

        let mut table = ReferenceTable::new();
        for (i, ctx) in contexts.iter().enumerate() {
            table.push(ReferenceEntry::new(
                i as u32,
                i as u32,
                0,
                i as u32 * 10,
                i as u32 * 10 + 5,
                *ctx,
            ));
        }

        // 嵌入 ArcgrFile round-trip
        let mut file = arcgr::ArcgrFile::new();
        file.reference_table = table;

        let bytes = arcgr::write_arcgr(&file);
        let file2 = arcgr::read_arcgr(&bytes).unwrap();

        assert_eq!(file2.reference_table.entries.len(), 8);
        for (i, entry) in file2.reference_table.entries.iter().enumerate() {
            assert_eq!(entry.context, contexts[i]);
        }
    }

    /// K3：`resolve_method_callee` 按「接收者类型 → 方法符号」解析——实例变量、
    /// 静态类名、`new T()`、`this` 四种接收者形态均产出 `"Class.method"` 符号
    /// （原 M2 裸方法名查找对实例方法恒 miss）。
    #[test]
    fn resolve_method_callee_resolves_receiver_to_class_method_symbol() {
        let mut ctx = CollectContext::new();
        for name in ["Helper", "Helper.Double", "Quad", "Quad.Double"] {
            ctx.register(name.to_string());
        }
        let span = Span::DUMMY;

        // 实例变量：`q.Double(21)`（q 经 let 登记为 Quad）→ Quad.Double
        let mut locals = LocalScope::new(Some("Quad".to_string()));
        locals.define("q", "Quad");
        let recv = Expr::Ident("q".into());
        assert_eq!(
            resolve_method_callee(&recv, &"Double".into(), &ctx, &locals),
            Some("Quad.Double".to_string())
        );

        // 静态调用：`Helper.Double(21)`（receiver 为类型名）→ Helper.Double
        let recv = Expr::Ident("Helper".into());
        assert_eq!(
            resolve_method_callee(&recv, &"Double".into(), &ctx, &locals),
            Some("Helper.Double".to_string())
        );

        // `new Quad().Double(21)` → Quad.Double
        let recv = Expr::New {
            ty: Spanned::new(
                Type::Named {
                    path: vec!["Quad".into()],
                    generics: vec![],
                },
                span,
            ),
            args: vec![],
            obj_init: None,
        };
        assert_eq!(
            resolve_method_callee(&recv, &"Double".into(), &ctx, &locals),
            Some("Quad.Double".to_string())
        );

        // `this.Double(21)`（属主 Quad）→ Quad.Double
        let recv = Expr::This;
        assert_eq!(
            resolve_method_callee(&recv, &"Double".into(), &ctx, &locals),
            Some("Quad.Double".to_string())
        );
    }

    /// K3：`LocalScope` 词法作用域——同名变量 shadowing 取最近绑定，嵌套块退出后
    /// 外层变量恢复可见。
    #[test]
    fn local_scope_shadowing_restores_outer_binding() {
        let mut locals = LocalScope::new(None);
        locals.define("q", "Quad");
        assert_eq!(locals.lookup("q"), Some("Quad"));

        locals.push();
        locals.define("q", "Helper");
        assert_eq!(
            locals.lookup("q"),
            Some("Helper"),
            "内层 shadowing 应取最近绑定"
        );
        locals.pop();

        assert_eq!(
            locals.lookup("q"),
            Some("Quad"),
            "退出嵌套块后应恢复外层绑定"
        );
        assert_eq!(locals.lookup("missing"), None);
    }
}
