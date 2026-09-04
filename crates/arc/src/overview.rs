//! `arc overview` CLI 实现（RFC 034 M4）。
//!
//! AI 首触入口——通过 `arc.toml` + `.arcgr` 输出项目骨架（L0 项目概览 / L1 模块面），
//! 让 AI 无需读源码即知项目结构。对应 RFC 034 渐进式披露的 L0/L1 层级。
//!
//! ## 输出格式
//!
//! - `human`（默认）：人类可读树形摘要
//! - `json`：结构化 JSON（机器可读，供下游工具链消费）
//!
//! ## `--detail` 旗标
//!
//! - 默认：仅输出 L0 项目概览（~500 token）
//! - `--detail`：输出 L0 + L1 完整模块面（~2K token）
//!
//! ## 工作流
//!
//! 1. 从 `file_path` 向上查找 `arc.toml`
//! 2. 运行 `parse → hir → typeck → collect_arcgr_file`（传入 manifest）
//! 3. 提取 `arcgr_file.context_manifest` 格式化输出
//!
//! 与 [`crate::inspect`] 的区别：`inspect` 输出语义索引（符号/边/可达性），
//! `overview` 输出项目骨架（L0/L1）——前者面向工具链消费，后者面向 AI 首触理解。

use std::path::Path;

use serde::Serialize;

use crate::arcgr::collect_arcgr_file;
use crate::equipment::PackageContext;
use crate::manifest::find_arc_manifest;
use arcgr::{ContextManifest, ProjectKind};

/// `arc overview` 报告——包含 ContextManifest 与源码路径。
pub struct OverviewReport {
    pub manifest: ContextManifest,
    pub source_path: String,
}

/// 运行 `parse → hir → typeck → collect_arcgr_file`，提取 ContextManifest。
///
/// 与 [`crate::inspect::inspect_source`] 共享前置流程，但仅提取 ContextManifest，
/// 不输出符号/边/可达性。要求 `arc.toml` 存在——否则返回错误。
pub fn overview_source(
    file_path: &Path,
    context: &dyn PackageContext,
) -> Result<OverviewReport, String> {
    // 1. 必须存在 arc.toml——overview 的核心数据源
    let (_, manifest) = find_arc_manifest(file_path).ok_or_else(|| {
        format!(
            "arc.toml not found (searched upward from {})",
            file_path.display()
        )
    })?;

    // 2. 与 inspect_source 共享前置流程
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
    let project_files = crate::arcgr::collect_project_files(&unit, user_file_id);

    // 3. collect_arcgr_file 传入 manifest 填充 ContextManifest
    let arcgr_file = collect_arcgr_file(
        &typeck,
        &typed_fns,
        &program,
        &project_files,
        Some(&manifest),
    );

    let manifest_out = arcgr_file.context_manifest.ok_or_else(|| {
        "ContextManifest not filled (manifest=Some but context_manifest=None)".to_string()
    })?;

    Ok(OverviewReport {
        manifest: manifest_out,
        source_path: unit.root.display().to_string(),
    })
}

// ============================================================================
// Human-readable 格式
// ============================================================================

/// 渲染 L0 项目概览（人类可读，~500 token）。
pub fn format_l0_human(report: &OverviewReport) -> String {
    let l0 = &report.manifest.l0_project;
    let mut out = String::new();
    out.push_str(&format!("arc overview (L0): {}\n\n", report.source_path));

    out.push_str(&format!("Project: {}\n", l0.name));
    out.push_str(&format!(
        "Kind: {} | Version: {}.{}.{} | Edition: {}\n",
        project_kind_name(l0.kind),
        l0.version_major,
        l0.version_minor,
        l0.version_patch,
        l0.edition
    ));
    out.push_str(&format!(
        "ABI: v{} | LLVM: v{} | Target: {}\n",
        l0.arc_abi_version,
        l0.llvm_version,
        if l0.target_triple.is_empty() {
            "(host)"
        } else {
            &l0.target_triple
        }
    ));

    out.push_str(&format!("\nNamespaces ({}):\n", l0.namespaces.len()));
    for ns in &l0.namespaces {
        out.push_str(&format!("  [{}] {}\n", ns.source_crate, ns.name));
    }

    out.push_str(&format!("\nDependencies ({}):\n", l0.dependencies.len()));
    for dep in &l0.dependencies {
        out.push_str(&format!(
            "  {} {}.{}.{} ({})\n",
            dep.name,
            dep.version_major,
            dep.version_minor,
            dep.version_patch,
            dependency_source_name(dep.source)
        ));
    }

    out.push_str(&format!("\nCapabilities ({}):\n", l0.capabilities.len()));
    for cap in &l0.capabilities {
        out.push_str(&format!("  id={} flags={}\n", cap.capability_id, cap.flags));
    }

    out.push_str(&format!(
        "\nCrate DAG: {} crates, {} edges\n",
        l0.crate_dag_summary.crate_count, l0.crate_dag_summary.edge_count
    ));

    if !l0.architecture_redlines.is_empty() {
        out.push_str(&format!(
            "\nArchitecture Redlines ({}):\n",
            l0.architecture_redlines.len()
        ));
        for rl in &l0.architecture_redlines {
            out.push_str(&format!(
                "  [{}] {}: {}\n",
                rl.rule_id,
                if rl.severity == 1 { "error" } else { "warning" },
                rl.message
            ));
        }
    }

    out
}

/// 渲染 L0 + L1 完整模块面（人类可读，~2K token）。
pub fn format_l1_human(report: &OverviewReport) -> String {
    let mut out = format_l0_human(report);
    let l1 = &report.manifest.l1_module_surface;

    out.push_str(&format!("\nModules ({} crates):\n", l1.crates.len()));
    for c in &l1.crates {
        out.push_str(&format!(
            "\n  [{}] {} ({})\n",
            c.crate_id,
            c.name,
            if c.path.is_empty() { "(root)" } else { &c.path }
        ));
        if !c.responsibility.is_empty() {
            out.push_str(&format!("    Responsibility: {}\n", c.responsibility));
        }
        out.push_str(&format!("    Public APIs ({}):\n", c.public_apis.len()));
        for api in &c.public_apis {
            out.push_str(&format!(
                "      symbol_id={} kind={} visibility={}\n",
                api.symbol_id,
                public_api_kind_name(api.api_kind),
                api.visibility
            ));
        }
        if !c.namespaces.is_empty() {
            out.push_str(&format!("    Namespaces: {:?}\n", c.namespaces));
        }
    }

    if !l1.dag_edges.is_empty() {
        out.push_str(&format!("\nDAG Edges ({}):\n", l1.dag_edges.len()));
        for e in &l1.dag_edges {
            out.push_str(&format!(
                "  crate {} -> crate {} ({})\n",
                e.from_crate_id,
                e.to_crate_id,
                dag_edge_kind_name(e.edge_kind)
            ));
        }
    }

    out
}

fn project_kind_name(k: ProjectKind) -> &'static str {
    match k {
        ProjectKind::Executable => "executable",
        ProjectKind::Library => "library",
        ProjectKind::DynamicLibrary => "dynamic-library",
        ProjectKind::Test => "test",
    }
}

fn dependency_source_name(s: arcgr::DependencySource) -> &'static str {
    match s {
        arcgr::DependencySource::Git => "git",
        arcgr::DependencySource::Path => "path",
        arcgr::DependencySource::Precompiled => "precompiled",
    }
}

fn public_api_kind_name(k: arcgr::PublicApiKind) -> &'static str {
    match k {
        arcgr::PublicApiKind::Function => "function",
        arcgr::PublicApiKind::Method => "method",
        arcgr::PublicApiKind::StaticMethod => "static-method",
        arcgr::PublicApiKind::Property => "property",
        arcgr::PublicApiKind::Class => "class",
        arcgr::PublicApiKind::Struct => "struct",
        arcgr::PublicApiKind::Interface => "interface",
        arcgr::PublicApiKind::Enum => "enum",
        arcgr::PublicApiKind::Variant => "variant",
        arcgr::PublicApiKind::Module => "module",
    }
}

fn dag_edge_kind_name(k: arcgr::DagEdgeKind) -> &'static str {
    match k {
        arcgr::DagEdgeKind::CompileDep => "compile",
        arcgr::DagEdgeKind::LinkDep => "link",
        arcgr::DagEdgeKind::DevDep => "dev",
    }
}

// ============================================================================
// JSON 格式——轻量序列化视图
// ============================================================================

#[derive(Serialize)]
struct JsonL0Overview<'a> {
    name: &'a str,
    kind: &'static str,
    version: [u16; 3],
    edition: u16,
    arc_abi_version: u16,
    llvm_version: u16,
    target_triple: &'a str,
    namespaces: Vec<JsonNamespace>,
    dependencies: Vec<JsonDependency>,
    capabilities: Vec<JsonCapability>,
    architecture_redlines: Vec<JsonRedline>,
    crate_dag_summary: [u32; 2],
}

#[derive(Serialize)]
struct JsonNamespace {
    name: String,
    source_crate: u8,
}

#[derive(Serialize)]
struct JsonDependency {
    name: String,
    version: [u16; 3],
    source: &'static str,
}

#[derive(Serialize)]
struct JsonCapability {
    id: u16,
    flags: u8,
}

#[derive(Serialize)]
struct JsonRedline {
    rule_id: u16,
    severity: &'static str,
    message: String,
}

#[derive(Serialize)]
struct JsonL1Surface {
    crates: Vec<JsonCrate>,
    dag_edges: Vec<JsonDagEdge>,
}

#[derive(Serialize)]
struct JsonCrate {
    id: u8,
    name: String,
    path: String,
    responsibility: String,
    public_apis: Vec<JsonPublicApi>,
    namespaces: Vec<u8>,
}

#[derive(Serialize)]
struct JsonPublicApi {
    symbol_id: u32,
    kind: &'static str,
    visibility: u8,
}

#[derive(Serialize)]
struct JsonDagEdge {
    from: u8,
    to: u8,
    kind: &'static str,
}

/// 渲染 L0 JSON 输出。
pub fn format_l0_json(report: &OverviewReport) -> Result<String, String> {
    let l0 = &report.manifest.l0_project;
    let json = JsonL0Overview {
        name: &l0.name,
        kind: project_kind_name(l0.kind),
        version: [l0.version_major, l0.version_minor, l0.version_patch],
        edition: l0.edition,
        arc_abi_version: l0.arc_abi_version,
        llvm_version: l0.llvm_version,
        target_triple: &l0.target_triple,
        namespaces: l0
            .namespaces
            .iter()
            .map(|n| JsonNamespace {
                name: n.name.clone(),
                source_crate: n.source_crate,
            })
            .collect(),
        dependencies: l0
            .dependencies
            .iter()
            .map(|d| JsonDependency {
                name: d.name.clone(),
                version: [d.version_major, d.version_minor, d.version_patch],
                source: dependency_source_name(d.source),
            })
            .collect(),
        capabilities: l0
            .capabilities
            .iter()
            .map(|c| JsonCapability {
                id: c.capability_id,
                flags: c.flags,
            })
            .collect(),
        architecture_redlines: l0
            .architecture_redlines
            .iter()
            .map(|r| JsonRedline {
                rule_id: r.rule_id,
                severity: if r.severity == 1 { "error" } else { "warning" },
                message: r.message.clone(),
            })
            .collect(),
        crate_dag_summary: [
            l0.crate_dag_summary.crate_count as u32,
            l0.crate_dag_summary.edge_count as u32,
        ],
    };
    serde_json::to_string_pretty(&json).map_err(|e| format!("serialize JSON failed: {e}"))
}

/// 渲染 L0 + L1 完整 JSON 输出。
pub fn format_l1_json(report: &OverviewReport) -> Result<String, String> {
    let l0_json = format_l0_json(report)?;
    let l1 = &report.manifest.l1_module_surface;

    let l1_json = JsonL1Surface {
        crates: l1
            .crates
            .iter()
            .map(|c| JsonCrate {
                id: c.crate_id,
                name: c.name.clone(),
                path: c.path.clone(),
                responsibility: c.responsibility.clone(),
                public_apis: c
                    .public_apis
                    .iter()
                    .map(|a| JsonPublicApi {
                        symbol_id: a.symbol_id,
                        kind: public_api_kind_name(a.api_kind),
                        visibility: a.visibility,
                    })
                    .collect(),
                namespaces: c.namespaces.clone(),
            })
            .collect(),
        dag_edges: l1
            .dag_edges
            .iter()
            .map(|e| JsonDagEdge {
                from: e.from_crate_id,
                to: e.to_crate_id,
                kind: dag_edge_kind_name(e.edge_kind),
            })
            .collect(),
    };

    let l1_str = serde_json::to_string_pretty(&l1_json)
        .map_err(|e| format!("serialize JSON failed: {e}"))?;

    // 合并 L0 + L1 为单个 JSON 对象
    Ok(format!(
        "{{\n  \"l0\": {},\n  \"l1\": {}\n}}",
        l0_json, l1_str
    ))
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_api_kind_name_covers_all_variants() {
        for k in [
            arcgr::PublicApiKind::Function,
            arcgr::PublicApiKind::Method,
            arcgr::PublicApiKind::StaticMethod,
            arcgr::PublicApiKind::Property,
            arcgr::PublicApiKind::Class,
            arcgr::PublicApiKind::Struct,
            arcgr::PublicApiKind::Interface,
            arcgr::PublicApiKind::Enum,
            arcgr::PublicApiKind::Variant,
            arcgr::PublicApiKind::Module,
        ] {
            assert!(!public_api_kind_name(k).is_empty());
        }
    }

    #[test]
    fn project_kind_name_covers_all_variants() {
        for k in [
            ProjectKind::Executable,
            ProjectKind::Library,
            ProjectKind::DynamicLibrary,
            ProjectKind::Test,
        ] {
            assert!(!project_kind_name(k).is_empty());
        }
    }
}
