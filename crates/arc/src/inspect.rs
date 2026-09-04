//! `arc inspect` CLI 实现（RFC 034 M2 Step 4）。
//!
//! 运行 `parse → hir → typeck → collect_arcgr_file` 产出 `.arcgr` 语义索引，
//! 按指定格式输出供 AI 工具链、LSP、调试器消费（RFC 034/RFC 038/RFC 039 共享数据底座）。
//!
//! ## 输出格式
//!
//! - `human`（默认）：人类可读树形摘要——文件/符号/入口/边/可达性集合
//! - `json`：结构化 JSON（机器可读，供下游工具链消费）
//!
//! ## 可选 `--emit <path>`
//!
//! 同时将 `.arcgr` 二进制写入磁盘，用于跨工具链共享同一份语义索引
//! （RFC 034 M2 Step 5：`.arcgr` 落盘 + 跨工具链消费）。

use std::path::Path;

use serde::Serialize;

use crate::arcgr::collect_arcgr_file;
use crate::equipment::PackageContext;
use crate::manifest::find_arc_manifest;
use arcgr::{ArcgrFile, EdgeKind, EntryPointKind, SymbolEntry, SymbolKind, TypeSig, Visibility};

/// `.arcgr` 收集结果——包含二进制产物与源码路径（供格式化输出使用）。
pub struct InspectReport {
    pub arcgr_file: ArcgrFile,
    pub source_path: String,
}

/// 运行 `parse → hir → typeck → collect_arcgr_file`，产出 `InspectReport`。
///
/// 与 [`crate::pipeline::compile_file`] 共享前置流程（parse/hir/typeck），
/// 但跳过 borrowck / mir / codegen——`arc inspect` 只关心语义索引，
/// 不需要 MIR 或可执行产物。
pub fn inspect_source(
    file_path: &Path,
    context: &dyn PackageContext,
) -> Result<InspectReport, String> {
    let unit = context.load(file_path)?;

    let mut program = unit.program.clone();
    let desugar_errors = hir::desugar_program(&mut program);
    if !desugar_errors.is_empty() {
        return Err(desugar_errors.join("\n"));
    }
    let yield_errors = hir::desugar_yield_program(&mut program);
    if !yield_errors.is_empty() {
        return Err(yield_errors.join("\n"));
    }

    let mut hir_builder = hir::HirBuilder::new();
    let module = hir_builder
        .lower_program(&program)
        .map_err(|e| format!("hir error: {e}"))?;

    let mut typeck = typeck::TypeChecker::new();
    // 与 prepare_compilation 一致：注册 native + 外部符号，使 typeck 能解析跨包类型引用。
    typeck.register_native_modules(&unit.native_modules);
    typeck.register_external_symbols(&unit.external_symbols);
    let typed_fns = typeck.check_module(&module).map_err(|es| {
        es.iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    let user_file_id = unit
        .file_registry
        .find_file_id(&unit.root)
        .expect("user entry source must be registered in FileRegistry");

    // K2：项目文件表 = 入口文件 + 同目录声明 namespace 的兄弟文件（包过滤）。
    // `collect_arcgr_file` 据此做多文件符号合并 + 跨文件 edges。
    let project_files = crate::arcgr::collect_project_files(&unit, user_file_id);

    // RFC 034 M4：尝试加载 `arc.toml` 用于填充 ContextManifest。
    // 单文件无 manifest 场景（如 std/ 单文件 inspect）保持 None——
    // `collect_arcgr_file` 会跳过 ContextManifest 填充。
    let manifest = find_arc_manifest(file_path).map(|(_, m)| m);

    let arcgr_file = collect_arcgr_file(
        &typeck,
        &typed_fns,
        &program,
        &project_files,
        manifest.as_ref(),
    );
    Ok(InspectReport {
        arcgr_file,
        source_path: unit.root.display().to_string(),
    })
}

/// 将 `.arcgr` 二进制写入磁盘（RFC 034 M2 Step 5：跨工具链共享语义索引）。
pub fn emit_arcgr(report: &InspectReport, output: &Path) -> Result<(), String> {
    let bytes = arcgr::write_arcgr(&report.arcgr_file);
    std::fs::write(output, &bytes)
        .map_err(|e| format!("write .arcgr to {} failed: {e}", output.display()))?;
    Ok(())
}

// ============================================================================
// Human-readable 格式
// ============================================================================

/// 渲染人类可读摘要——文件/符号/入口/边/可达性集合。
pub fn format_human(report: &InspectReport) -> String {
    let file = &report.arcgr_file;
    let mut out = String::new();
    out.push_str(&format!("arc inspect: {}\n\n", report.source_path));

    // Files
    out.push_str(&format!("Files ({}):\n", file.file_table.entries.len()));
    for entry in &file.file_table.entries {
        out.push_str(&format!(
            "  [{}] {} (lines: {}, hash: {:#018x})\n",
            entry.file_id, entry.path, entry.line_count, entry.content_hash
        ));
    }

    // Symbols
    let symbols = &file.symbol_table.entries;
    out.push_str(&format!("\nSymbols ({}):\n", symbols.len()));
    for sym in symbols {
        out.push_str(&format!(
            "  [{}] {} ({}, {}) [{}:{}-{}]\n",
            sym.symbol_id,
            sym.name,
            kind_name(sym.kind),
            visibility_name(sym.visibility),
            sym.file_id,
            sym.span_start,
            sym.span_end
        ));
    }

    // Entry points
    let graph = &file.reference_graph;
    out.push_str(&format!("\nEntry points ({}):\n", graph.entry_points.len()));
    for ep in &graph.entry_points {
        let name = symbol_name_by_id(symbols, ep.symbol_id)
            .unwrap_or_else(|| format!("#{}", ep.symbol_id));
        out.push_str(&format!(
            "  [{}] {} ({}, priority {})\n",
            ep.symbol_id,
            name,
            entry_point_kind_name(ep.kind),
            ep.priority
        ));
    }

    // Edges
    out.push_str(&format!("\nEdges ({}):\n", graph.edges.len()));
    for edge in &graph.edges {
        let caller = symbol_name_by_id(symbols, edge.caller_symbol_id)
            .unwrap_or_else(|| format!("#{}", edge.caller_symbol_id));
        let callee = symbol_name_by_id(symbols, edge.callee_symbol_id)
            .unwrap_or_else(|| format!("#{}", edge.callee_symbol_id));
        out.push_str(&format!(
            "  {} -> {} ({}, {}) [{}:{}-{}]\n",
            caller,
            callee,
            edge_kind_name(edge.edge_kind),
            if edge.is_direct { "direct" } else { "indirect" },
            edge.file_id,
            edge.span_start,
            edge.span_end
        ));
    }

    // Reachability summary
    out.push_str(&format!(
        "\nReachability:\n  reachable: {} symbols\n  unreachable: {} symbols\n",
        graph.reachable_symbols.len(),
        graph.unreachable_symbols.len()
    ));
    if !graph.unreachable_symbols.is_empty() {
        out.push_str("  unreachable symbols:\n");
        for &id in &graph.unreachable_symbols {
            let name = symbol_name_by_id(symbols, id).unwrap_or_else(|| format!("#{}", id));
            out.push_str(&format!("    [{}] {}\n", id, name));
        }
    }

    out
}

fn symbol_name_by_id(symbols: &[SymbolEntry], id: u32) -> Option<String> {
    symbols
        .iter()
        .find(|s| s.symbol_id == id)
        .map(|s| s.name.clone())
}

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

fn entry_point_kind_name(k: EntryPointKind) -> &'static str {
    match k {
        EntryPointKind::Main => "Main",
        EntryPointKind::LibraryExport => "LibraryExport",
        EntryPointKind::TestFunction => "TestFunction",
        EntryPointKind::DynamicLibEntry => "DynamicLibEntry",
        EntryPointKind::FFIExport => "FFIExport",
        EntryPointKind::CGMain => "CGMain",
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

// ============================================================================
// JSON 格式——轻量序列化视图（避免在 arcgr crate 引入 serde 依赖）
// ============================================================================

#[derive(Serialize)]
struct JsonReport<'a> {
    source_path: &'a str,
    files: Vec<JsonFile>,
    symbols: Vec<JsonSymbol>,
    entry_points: Vec<JsonEntryPoint>,
    edges: Vec<JsonEdge>,
    reachable: &'a [u32],
    unreachable: &'a [u32],
}

#[derive(Serialize)]
struct JsonFile {
    id: u32,
    path: String,
    content_hash: u64,
    line_count: u32,
}

#[derive(Serialize)]
struct JsonSymbol {
    id: u32,
    name: String,
    kind: &'static str,
    visibility: &'static str,
    file_id: u32,
    span_start: u32,
    span_end: u32,
}

#[derive(Serialize)]
struct JsonEntryPoint {
    symbol_id: u32,
    kind: &'static str,
    priority: u8,
}

#[derive(Serialize)]
struct JsonEdge {
    caller: u32,
    callee: u32,
    kind: &'static str,
    file_id: u32,
    span_start: u32,
    span_end: u32,
    direct: bool,
}

/// 渲染 JSON 输出。`type_sig` 字段省略（递归结构序列化留给二进制 `.arcgr`）。
pub fn format_json(report: &InspectReport) -> Result<String, String> {
    let file = &report.arcgr_file;
    let json = JsonReport {
        source_path: &report.source_path,
        files: file
            .file_table
            .entries
            .iter()
            .map(|f| JsonFile {
                id: f.file_id,
                path: f.path.clone(),
                content_hash: f.content_hash,
                line_count: f.line_count,
            })
            .collect(),
        symbols: file
            .symbol_table
            .entries
            .iter()
            .map(|s| JsonSymbol {
                id: s.symbol_id,
                name: s.name.clone(),
                kind: kind_name(s.kind),
                visibility: visibility_name(s.visibility),
                file_id: s.file_id,
                span_start: s.span_start,
                span_end: s.span_end,
            })
            .collect(),
        entry_points: file
            .reference_graph
            .entry_points
            .iter()
            .map(|ep| JsonEntryPoint {
                symbol_id: ep.symbol_id,
                kind: entry_point_kind_name(ep.kind),
                priority: ep.priority,
            })
            .collect(),
        edges: file
            .reference_graph
            .edges
            .iter()
            .map(|e| JsonEdge {
                caller: e.caller_symbol_id,
                callee: e.callee_symbol_id,
                kind: edge_kind_name(e.edge_kind),
                file_id: e.file_id,
                span_start: e.span_start,
                span_end: e.span_end,
                direct: e.is_direct,
            })
            .collect(),
        reachable: &file.reference_graph.reachable_symbols,
        unreachable: &file.reference_graph.unreachable_symbols,
    };
    serde_json::to_string_pretty(&json).map_err(|e| format!("serialize JSON failed: {e}"))
}

// 占位抑制未使用警告（TypeSig 在 M3+ JSON 详细类型签名时启用）
#[allow(dead_code)]
fn _typesig_placeholder(_t: &TypeSig) {}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_visibility_names_cover_all_variants() {
        // 简单烟雾测试：枚举名函数能覆盖所有变体，不会因新增变体失配。
        for k in [
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::StaticMethod,
            SymbolKind::Property,
            SymbolKind::Field,
            SymbolKind::Class,
            SymbolKind::Struct,
            SymbolKind::Interface,
            SymbolKind::Enum,
            SymbolKind::Variant,
            SymbolKind::Constant,
            SymbolKind::Module,
        ] {
            assert!(!kind_name(k).is_empty());
        }
        for v in [
            Visibility::Public,
            Visibility::Internal,
            Visibility::Protected,
            Visibility::Private,
        ] {
            assert!(!visibility_name(v).is_empty());
        }
    }
}
