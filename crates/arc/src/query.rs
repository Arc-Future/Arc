//! `arc locate` / `arc explain` / `arc query` 查询层（RFC 034 M3）。
//!
//! 渐进式披露 L2 层落地——查询直接读取 `.arcgr` 二进制，不重新编译。
//!
//! ## 子命令一览
//!
//! - `arc locate <symbol>`：定义定位（file:line:col + 签名）
//! - `arc explain <symbol>`：L2 符号卡片（签名 + 引用数 + 直接 callers/callees）
//! - `arc query callers|callees|impls|references <symbol>`：意图查询
//!
//! ## 设计原则
//!
//! - 复用 `arcgr::SymbolTable::find_by_name` / `ReferenceGraph::edges` 等 API
//! - 单文件单职责，每个查询函数 ≤30 行
//! - 结构化结果 + 格式化分离（serde JSON 派生 + human 格式化函数）

use std::path::Path;

use serde::Serialize;

use arcgr::{ArcgrFile, EdgeKind, ReferenceEdge, SymbolKind, TypeSig, Visibility};

// ============================================================================
// 加载层
// ============================================================================

/// 从 `.arcgr` 二进制文件加载 [`ArcgrFile`]。
///
/// 查询层入口——所有 `arc locate/explain/query` 通过此函数复用同一份语义索引。
pub fn load_arcgr(path: &Path) -> Result<ArcgrFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {} failed: {e}", path.display()))?;
    arcgr::read_arcgr(&bytes).map_err(|e| format!("parse .arcgr failed: {e}"))
}

// ============================================================================
// locate —— 定义定位
// ============================================================================

/// `arc locate` 结果——file:line:col + 签名。
#[derive(Serialize)]
pub struct LocateResult {
    pub name: String,
    pub kind: &'static str,
    pub visibility: &'static str,
    pub file: String,
    pub file_id: u32,
    pub span_start: u32,
    pub span_end: u32,
    pub signature: String,
}

/// 按精确符号名查找定义位置。
///
/// 返回首个匹配（RFC 034 M3 非目标：不实现模糊匹配）。
/// 若需查找所有同名符号，使用 `arc query references`。
pub fn locate(file: &ArcgrFile, name: &str) -> Option<LocateResult> {
    let sym = file.symbol_table.find_by_name(name).into_iter().next()?;
    let path = file
        .file_table
        .find(sym.file_id)
        .map(|e| e.path.clone())
        .unwrap_or_default();
    Some(LocateResult {
        name: sym.name.clone(),
        kind: kind_name(sym.kind),
        visibility: visibility_name(sym.visibility),
        file: path,
        file_id: sym.file_id,
        span_start: sym.span_start,
        span_end: sym.span_end,
        signature: format_type_sig(&sym.type_sig),
    })
}

// ============================================================================
// explain —— L2 符号卡片
// ============================================================================

/// `arc explain` 结果——L2 符号卡片（token 预算 4K）。
#[derive(Serialize)]
pub struct ExplainResult {
    pub name: String,
    pub kind: &'static str,
    pub visibility: &'static str,
    pub file: String,
    pub span_start: u32,
    pub span_end: u32,
    pub signature: String,
    pub doc_summary: Option<String>,
    pub callers: Vec<SymbolCard>,
    pub callees: Vec<SymbolCard>,
    pub reference_count: usize,
    pub is_entry_point: bool,
    pub is_reachable: bool,
}

/// 紧凑符号卡片——explain / query 共用。
#[derive(Serialize)]
pub struct SymbolCard {
    pub name: String,
    pub kind: &'static str,
    pub signature: String,
    pub edge_kind: Option<&'static str>,
}

/// 生成 L2 符号卡片。
///
/// 含签名 + doc_summary + 直接 callers/callees + 引用数 + 入口/可达性标记。
/// token 预算 ~4K（RFC 034 M3 §448）。
pub fn explain(file: &ArcgrFile, name: &str) -> Option<ExplainResult> {
    let sym = file.symbol_table.find_by_name(name).into_iter().next()?;
    let id = sym.symbol_id;
    let path = file
        .file_table
        .find(sym.file_id)
        .map(|e| e.path.clone())
        .unwrap_or_default();

    let callers = collect_callers(file, id);
    let callees = collect_callees(file, id);
    let reference_count = count_references(file, id);
    let is_entry_point = file
        .reference_graph
        .entry_points
        .iter()
        .any(|ep| ep.symbol_id == id);
    let is_reachable = file.reference_graph.reachable_symbols.contains(&id);

    Some(ExplainResult {
        name: sym.name.clone(),
        kind: kind_name(sym.kind),
        visibility: visibility_name(sym.visibility),
        file: path,
        span_start: sym.span_start,
        span_end: sym.span_end,
        signature: format_type_sig(&sym.type_sig),
        doc_summary: sym.doc_summary.clone(),
        callers,
        callees,
        reference_count,
        is_entry_point,
        is_reachable,
    })
}

// ============================================================================
// query —— 意图查询
// ============================================================================

/// 查询意图（RFC 034 M3 §455）。
#[derive(Clone, Copy)]
pub enum QueryKind {
    Callers,
    Callees,
    Impls,
    References,
}

impl QueryKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "callers" => Some(Self::Callers),
            "callees" => Some(Self::Callees),
            "impls" => Some(Self::Impls),
            "references" => Some(Self::References),
            _ => None,
        }
    }
}

/// `arc query` 结果——符号集合（callers/callees/impls）或引用点集合（references）。
#[derive(Serialize)]
pub struct QueryResult {
    pub kind: &'static str,
    pub target: String,
    pub symbols: Vec<SymbolCard>,
    pub references: Vec<RefSite>,
}

/// 引用点——`arc query references` 输出条目。
#[derive(Serialize)]
pub struct RefSite {
    pub symbol: String,
    pub kind: &'static str,
    pub file: String,
    pub file_id: u32,
    pub span_start: u32,
    pub span_end: u32,
}

/// 执行意图查询。
///
/// - `callers`：调用 `symbol` 的所有符号
/// - `callees`：`symbol` 调用的所有符号
/// - `impls`：实现 `symbol`（接口）的所有类型
/// - `references`：所有引用 `symbol` 的位置（callers + 字段访问 + 属性访问等）
pub fn query(file: &ArcgrFile, kind: QueryKind, name: &str) -> Option<QueryResult> {
    let sym = file.symbol_table.find_by_name(name).into_iter().next()?;
    let id = sym.symbol_id;
    let kind_str = match kind {
        QueryKind::Callers => "callers",
        QueryKind::Callees => "callees",
        QueryKind::Impls => "impls",
        QueryKind::References => "references",
    };
    let (symbols, references) = match kind {
        QueryKind::Callers => (collect_callers(file, id), vec![]),
        QueryKind::Callees => (collect_callees(file, id), vec![]),
        QueryKind::Impls => (collect_impls(file, id), vec![]),
        QueryKind::References => (vec![], collect_references(file, id)),
    };
    Some(QueryResult {
        kind: kind_str,
        target: sym.name.clone(),
        symbols,
        references,
    })
}

// ============================================================================
// 查询辅助（≤20 行 / 函数）
// ============================================================================

fn collect_callers(file: &ArcgrFile, callee_id: u32) -> Vec<SymbolCard> {
    file.reference_graph
        .edges
        .iter()
        .filter(|e| e.callee_symbol_id == callee_id)
        .filter_map(|e| file.symbol_table.find(e.caller_symbol_id))
        .map(|s| SymbolCard {
            name: s.name.clone(),
            kind: kind_name(s.kind),
            signature: format_type_sig(&s.type_sig),
            edge_kind: None,
        })
        .collect()
}

fn collect_callees(file: &ArcgrFile, caller_id: u32) -> Vec<SymbolCard> {
    file.reference_graph
        .edges
        .iter()
        .filter(|e| e.caller_symbol_id == caller_id)
        .filter_map(|e| {
            file.symbol_table
                .find(e.callee_symbol_id)
                .map(|s| SymbolCard {
                    name: s.name.clone(),
                    kind: kind_name(s.kind),
                    signature: format_type_sig(&s.type_sig),
                    edge_kind: Some(edge_kind_name(e.edge_kind)),
                })
        })
        .collect()
}

fn collect_impls(file: &ArcgrFile, iface_id: u32) -> Vec<SymbolCard> {
    file.reference_graph
        .edges
        .iter()
        .filter(|e| e.edge_kind == EdgeKind::Implement && e.callee_symbol_id == iface_id)
        .filter_map(|e| file.symbol_table.find(e.caller_symbol_id))
        .map(|s| SymbolCard {
            name: s.name.clone(),
            kind: kind_name(s.kind),
            signature: format_type_sig(&s.type_sig),
            edge_kind: Some("Implement"),
        })
        .collect()
}

fn collect_references(file: &ArcgrFile, target_id: u32) -> Vec<RefSite> {
    file.reference_graph
        .edges
        .iter()
        .filter(|e| e.callee_symbol_id == target_id)
        .map(|e| {
            let sym_name = file
                .symbol_table
                .find(e.caller_symbol_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| format!("#{}", e.caller_symbol_id));
            let path = file
                .file_table
                .find(e.file_id)
                .map(|f| f.path.clone())
                .unwrap_or_default();
            RefSite {
                symbol: sym_name,
                kind: edge_kind_name(e.edge_kind),
                file: path,
                file_id: e.file_id,
                span_start: e.span_start,
                span_end: e.span_end,
            }
        })
        .collect()
}

fn count_references(file: &ArcgrFile, target_id: u32) -> usize {
    file.reference_graph
        .edges
        .iter()
        .filter(|e| e.callee_symbol_id == target_id)
        .count()
}

// ============================================================================
// 格式化（human-readable）
// ============================================================================

/// 渲染 `arc locate` 人类可读输出。
pub fn format_locate_human(r: &LocateResult) -> String {
    format!(
        "{} ({}, {}) — {}:{}-{}\n  signature: {}",
        r.name, r.kind, r.visibility, r.file, r.span_start, r.span_end, r.signature
    )
}

/// 渲染 `arc explain` 人类可读输出（L2 符号卡片）。
pub fn format_explain_human(r: &ExplainResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "=== {} ({}, {}) ===\n",
        r.name, r.kind, r.visibility
    ));
    out.push_str(&format!(
        "location: {}:{}-{}\n",
        r.file, r.span_start, r.span_end
    ));
    out.push_str(&format!("signature: {}\n", r.signature));
    if let Some(doc) = &r.doc_summary {
        out.push_str(&format!("doc: {doc}\n"));
    }
    out.push_str(&format!(
        "references: {} (entry_point: {}, reachable: {})\n",
        r.reference_count, r.is_entry_point, r.is_reachable
    ));
    out.push_str(&format!("\nCallers ({}):\n", r.callers.len()));
    for c in &r.callers {
        out.push_str(&format!("  {} ({}) — {}\n", c.name, c.kind, c.signature));
    }
    out.push_str(&format!("\nCallees ({}):\n", r.callees.len()));
    for c in &r.callees {
        let edge = c.edge_kind.map(|k| format!(" [{k}]")).unwrap_or_default();
        out.push_str(&format!(
            "  {} ({}){} — {}\n",
            c.name, c.kind, edge, c.signature
        ));
    }
    out
}

/// 渲染 `arc query` 人类可读输出。
pub fn format_query_human(r: &QueryResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== {} of '{}' ===\n", r.kind, r.target));
    if !r.symbols.is_empty() {
        out.push_str(&format!("symbols ({}):\n", r.symbols.len()));
        for c in &r.symbols {
            let edge = c.edge_kind.map(|k| format!(" [{k}]")).unwrap_or_default();
            out.push_str(&format!(
                "  {} ({}){} — {}\n",
                c.name, c.kind, edge, c.signature
            ));
        }
    }
    if !r.references.is_empty() {
        out.push_str(&format!("references ({}):\n", r.references.len()));
        for rf in &r.references {
            out.push_str(&format!(
                "  {} [{}] {}:{}-{}\n",
                rf.symbol, rf.kind, rf.file, rf.span_start, rf.span_end
            ));
        }
    }
    out
}

// ============================================================================
// 枚举名映射（与 inspect.rs 对齐，避免重复实现）
// ============================================================================

fn kind_name(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::StaticMethod => "static-method",
        SymbolKind::Property => "property",
        SymbolKind::Field => "field",
        SymbolKind::Class => "class",
        SymbolKind::Struct => "struct",
        SymbolKind::Interface => "interface",
        SymbolKind::Enum => "enum",
        SymbolKind::Variant => "variant",
        SymbolKind::Constant => "constant",
        SymbolKind::Module => "module",
    }
}

fn visibility_name(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Internal => "internal",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    }
}

fn edge_kind_name(k: EdgeKind) -> &'static str {
    match k {
        EdgeKind::Call => "Call",
        EdgeKind::MethodCall => "MethodCall",
        EdgeKind::New => "New",
        EdgeKind::Implement => "Implement",
        EdgeKind::FieldAccess => "FieldAccess",
        EdgeKind::PropertyAccess => "PropertyAccess",
        EdgeKind::VariantMatch => "VariantMatch",
        EdgeKind::GenericInstantiation => "GenericInstantiation",
    }
}

/// 将 [`TypeSig`] 渲染为简短可读字符串（与 inspect.rs `format_type_sig` 对齐）。
///
/// M2 阶段简化输出——基元直接名称，命名类型 FQN，复合类型按形如 `List<T>`、
/// `Func<T1, T2 -> R>` 渲染。
fn format_type_sig(sig: &TypeSig) -> String {
    match sig {
        TypeSig::Int => "int".into(),
        TypeSig::Long => "long".into(),
        TypeSig::Float => "float".into(),
        TypeSig::Double => "double".into(),
        TypeSig::Bool => "bool".into(),
        TypeSig::String => "string".into(),
        TypeSig::Unit => "void".into(),
        TypeSig::Null => "null".into(),
        TypeSig::Object => "object".into(),
        TypeSig::UInt => "uint".into(),
        TypeSig::ULong => "ulong".into(),
        TypeSig::UShort => "ushort".into(),
        TypeSig::SByte => "sbyte".into(),
        TypeSig::Named {
            fully_qualified_name,
            generic_args,
        } => {
            if generic_args.is_empty() {
                fully_qualified_name.clone()
            } else {
                let args: Vec<String> = generic_args.iter().map(format_type_sig).collect();
                format!("{}<{}>", fully_qualified_name, args.join(", "))
            }
        }
        TypeSig::Func { params, ret, .. } => {
            let p: Vec<String> = params.iter().map(format_type_sig).collect();
            format!("Func<{} -> {}>", p.join(", "), format_type_sig(ret))
        }
        TypeSig::Method {
            receiver,
            params,
            ret,
            ..
        } => {
            let p: Vec<String> = params.iter().map(format_type_sig).collect();
            format!(
                "{}::Method<({}) -> {}>",
                format_type_sig(receiver),
                p.join(", "),
                format_type_sig(ret)
            )
        }
        TypeSig::Property { prop_type, .. } => {
            format!("Property<{}>", format_type_sig(prop_type))
        }
        TypeSig::GenericParam { param_index } => format!("T{param_index}"),
        TypeSig::Nullable { inner } => format!("{}?", format_type_sig(inner)),
        TypeSig::List { element_type } => {
            format!("List<{}>", format_type_sig(element_type))
        }
        TypeSig::Array {
            element_type,
            length,
        } => {
            format!("{}[{length}]", format_type_sig(element_type))
        }
        TypeSig::Tuple { elements } => {
            let e: Vec<String> = elements.iter().map(format_type_sig).collect();
            format!("({})", e.join(", "))
        }
        TypeSig::Closure { fn_sig, env_type } => {
            format!(
                "Closure<fn={}, env={}>",
                format_type_sig(fn_sig),
                format_type_sig(env_type)
            )
        }
        TypeSig::Variant {
            fully_qualified_name,
            cases,
        } => {
            let c: Vec<String> = cases
                .iter()
                .map(|c| format!("{}: {}", c.case_name, format_type_sig(&c.payload_type)))
                .collect();
            format!("Variant {fully_qualified_name} {{ {} }}", c.join(", "))
        }
        TypeSig::TaskHandle { result_type } => {
            format!("Task<{}>", format_type_sig(result_type))
        }
        TypeSig::Span { element_type } => {
            format!("Span<{}>", format_type_sig(element_type))
        }
        TypeSig::Expression { delegate_type } => {
            format!("Expression<{}>", format_type_sig(delegate_type))
        }
    }
}

// 抑制未使用警告（ReferenceEdge 在 M3+ 引用图扩展时启用）
#[allow(dead_code)]
fn _ref_edge_placeholder(_e: &ReferenceEdge) {}

// ============================================================================
// 单元测试（不依赖文件 I/O——直接构造 ArcgrFile）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use arcgr::{
        ArcgrFile, EdgeKind, EntryPoint, EntryPointKind, FileEntry, ReferenceEdge, SymbolEntry,
        SymbolKind, TypeSig, Visibility,
    };

    /// 构造测试用 ArcgrFile：
    /// ```arc
    /// interface IFoo { void Bar(); }
    /// class FooImpl : IFoo { void Bar() { Main(); } }
    /// void Main() { new FooImpl(); }
    /// ```
    fn sample_file() -> ArcgrFile {
        let mut file = ArcgrFile::new();

        // FileTable
        file.file_table.entries.push(FileEntry {
            file_id: 1,
            path: "test.as".into(),
            content_hash: 0,
            line_count: 10,
        });

        // Symbols: IFoo(1), FooImpl(2), Main(3), IFoo.Bar(4), FooImpl.Bar(5)
        let symbols: [(u32, &str, SymbolKind, TypeSig); 5] = [
            (
                1,
                "IFoo",
                SymbolKind::Interface,
                TypeSig::Named {
                    fully_qualified_name: "IFoo".into(),
                    generic_args: vec![],
                },
            ),
            (
                2,
                "FooImpl",
                SymbolKind::Class,
                TypeSig::Named {
                    fully_qualified_name: "FooImpl".into(),
                    generic_args: vec![],
                },
            ),
            (
                3,
                "Main",
                SymbolKind::Function,
                TypeSig::Func {
                    params: vec![],
                    ret: Box::new(TypeSig::Unit),
                    captures: false,
                },
            ),
            (
                4,
                "IFoo.Bar",
                SymbolKind::Method,
                TypeSig::Method {
                    receiver: Box::new(TypeSig::Named {
                        fully_qualified_name: "IFoo".into(),
                        generic_args: vec![],
                    }),
                    params: vec![],
                    ret: Box::new(TypeSig::Unit),
                    is_virtual: true,
                    vtable_slot: 0,
                },
            ),
            (
                5,
                "FooImpl.Bar",
                SymbolKind::Method,
                TypeSig::Method {
                    receiver: Box::new(TypeSig::Named {
                        fully_qualified_name: "FooImpl".into(),
                        generic_args: vec![],
                    }),
                    params: vec![],
                    ret: Box::new(TypeSig::Unit),
                    is_virtual: false,
                    vtable_slot: 0,
                },
            ),
        ];
        for (id, name, kind, sig) in symbols {
            file.symbol_table.entries.push(SymbolEntry::new(
                id,
                name.to_string(),
                kind,
                Visibility::Public,
                1,
                0,
                100,
                sig,
                None,
            ));
        }

        // Edges: FooImpl implements IFoo; Main calls FooImpl.Bar; Main calls FooImpl (New)
        let edges = [
            ReferenceEdge::new(2, 1, EdgeKind::Implement, 1, 10, 20, true),
            ReferenceEdge::new(3, 2, EdgeKind::New, 1, 30, 40, true),
            ReferenceEdge::new(5, 3, EdgeKind::Call, 1, 50, 60, true),
        ];
        file.reference_graph.edges = edges.to_vec();

        // Entry point: Main
        file.reference_graph
            .entry_points
            .push(EntryPoint::new(3, EntryPointKind::Main, 0));
        // Reachable: Main(3), FooImpl(2), FooImpl.Bar(5), Main(call from FooImpl.Bar)
        file.reference_graph.reachable_symbols = vec![2, 3, 5];

        file
    }

    #[test]
    fn locate_returns_first_match() {
        let file = sample_file();
        let r = locate(&file, "Main").expect("Main should exist");
        assert_eq!(r.name, "Main");
        assert_eq!(r.kind, "function");
        assert_eq!(r.file, "test.as");
        assert_eq!(r.signature, "Func< -> void>");
    }

    #[test]
    fn locate_returns_none_for_unknown() {
        let file = sample_file();
        assert!(locate(&file, "NotExist").is_none());
    }

    #[test]
    fn explain_includes_callers_and_callees() {
        let file = sample_file();
        let r = explain(&file, "Main").expect("Main should exist");
        assert!(r.is_entry_point);
        assert!(r.is_reachable);
        // Main 被 FooImpl.Bar 调用
        assert_eq!(r.callers.len(), 1);
        assert_eq!(r.callers[0].name, "FooImpl.Bar");
        // Main 调用 FooImpl (New)
        assert_eq!(r.callees.len(), 1);
        assert_eq!(r.callees[0].name, "FooImpl");
    }

    #[test]
    fn query_callers_returns_expected() {
        let file = sample_file();
        let r = query(&file, QueryKind::Callers, "Main").expect("query should succeed");
        assert_eq!(r.kind, "callers");
        assert_eq!(r.symbols.len(), 1);
        assert_eq!(r.symbols[0].name, "FooImpl.Bar");
    }

    #[test]
    fn query_impls_returns_implementors() {
        let file = sample_file();
        let r = query(&file, QueryKind::Impls, "IFoo").expect("query should succeed");
        assert_eq!(r.kind, "impls");
        assert_eq!(r.symbols.len(), 1);
        assert_eq!(r.symbols[0].name, "FooImpl");
        assert_eq!(r.symbols[0].edge_kind, Some("Implement"));
    }

    #[test]
    fn query_references_returns_all_sites() {
        let file = sample_file();
        let r = query(&file, QueryKind::References, "Main").expect("query should succeed");
        assert_eq!(r.kind, "references");
        // Main 作为 callee 出现在 1 条边（FooImpl.Bar -> Main）
        assert_eq!(r.references.len(), 1);
        assert_eq!(r.references[0].symbol, "FooImpl.Bar");
        assert_eq!(r.references[0].kind, "Call");
    }

    #[test]
    fn format_type_sig_handles_compound_types() {
        let sig = TypeSig::List {
            element_type: Box::new(TypeSig::Named {
                fully_qualified_name: "Foo".into(),
                generic_args: vec![TypeSig::Int, TypeSig::String],
            }),
        };
        assert_eq!(format_type_sig(&sig), "List<Foo<int, string>>");

        let nullable = TypeSig::Nullable {
            inner: Box::new(TypeSig::Int),
        };
        assert_eq!(format_type_sig(&nullable), "int?");
    }

    #[test]
    fn format_outputs_are_non_empty() {
        let file = sample_file();
        let loc = locate(&file, "Main").unwrap();
        assert!(format_locate_human(&loc).contains("Main"));

        let exp = explain(&file, "Main").unwrap();
        assert!(format_explain_human(&exp).contains("Callers"));

        let q = query(&file, QueryKind::Callers, "Main").unwrap();
        assert!(format_query_human(&q).contains("callers"));
    }

    #[test]
    fn json_serialization_round_trip() {
        let file = sample_file();
        let exp = explain(&file, "Main").unwrap();
        let json = serde_json::to_string(&exp).expect("serialize should succeed");
        assert!(json.contains("\"name\":\"Main\""));
        assert!(json.contains("\"is_entry_point\":true"));
    }
}
