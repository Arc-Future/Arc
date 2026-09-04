//! `.arcgr` ContextManifest 子表（RFC 034）。
//!
//! AI 上下文清单——L0 项目概览（~500 token）+ L1 模块面（~2K token）双层结构。
//! 对齐 RFC 034 渐进式披露原则：L0 给出项目概览，L1 给出模块面。
//!
//! ## 二进制布局
//!
//! ```text
//! ContextManifest section:
//!   has_manifest: 1 byte (0=无 manifest, 1=有 manifest)
//!   if has_manifest == 1:
//!     L0ProjectOverview: 变长
//!     L1ModuleSurface:  变长
//! ```
//!
//! ## M2 占位策略
//!
//! M2 阶段不产出 ContextManifest（`has_manifest=0`，1 字节占位），
//! M4 实施时填充真实清单。schema 完整定义先行——遵循 R1「前置 schema 先行」原则。

use crate::error::{ArcgrError, Result};
use crate::io::{read_u16, read_u32, read_u8, write_u16, write_u32};

// ============================================================================
// L0 项目概览（~500 token）
// ============================================================================

/// 项目类别（1 字节枚举）。
///
/// RFC 017 D8 v1.0：删除 `Plugin` 变体——框架无 plugin 概念，
/// 动态库由 `DynamicLibrary` 表达（`kind = "library"` + `dynamic = true`）。
/// 值 `3` 保留为预留位（不再映射到任何变体），保证二进制前向兼容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ProjectKind {
    /// `arc.toml [package].kind = "executable"`
    #[default]
    Executable = 0,
    /// `kind = "library"`
    Library = 1,
    /// `kind = "library"` + `dynamic = true`（RFC 017 D8 v1.0 动态库）
    DynamicLibrary = 2,
    /// 测试 harness 二进制
    Test = 4,
}

impl ProjectKind {
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Executable,
            1 => Self::Library,
            2 => Self::DynamicLibrary,
            4 => Self::Test,
            other => return Err(ArcgrError::InvalidProjectKind(other)),
        })
    }
}

/// 依赖来源（1 字节枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum DependencySource {
    /// git + semver
    #[default]
    Git = 0,
    /// 本地路径依赖
    Path = 1,
    /// `.ao` 全局缓存
    Precompiled = 2,
}

impl DependencySource {
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Git,
            1 => Self::Path,
            2 => Self::Precompiled,
            other => return Err(ArcgrError::InvalidDependencySource(other)),
        })
    }
}

/// 依赖条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEntry {
    pub name: String,
    pub version_major: u16,
    pub version_minor: u16,
    pub version_patch: u16,
    pub source: DependencySource,
}

impl DependencyEntry {
    pub fn new(
        name: impl Into<String>,
        major: u16,
        minor: u16,
        patch: u16,
        source: DependencySource,
    ) -> Self {
        Self {
            name: name.into(),
            version_major: major,
            version_minor: minor,
            version_patch: patch,
            source,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_str_u8(w, &self.name);
        write_u16(w, self.version_major);
        write_u16(w, self.version_minor);
        write_u16(w, self.version_patch);
        w.push(self.source as u8);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let name = read_str_u8(r)?;
        let version_major = read_u16(r)?;
        let version_minor = read_u16(r)?;
        let version_patch = read_u16(r)?;
        let source = DependencySource::from_u8(read_u8(r)?)?;
        Ok(Self {
            name,
            version_major,
            version_minor,
            version_patch,
            source,
        })
    }
}

/// 能力声明（RFC 016/4.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityDecl {
    pub capability_id: u16,
    /// 0=声明 / 1=使用 / 2=请求
    pub flags: u8,
}

impl CapabilityDecl {
    pub fn new(capability_id: u16, flags: u8) -> Self {
        Self {
            capability_id,
            flags,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u16(w, self.capability_id);
        w.push(self.flags);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let capability_id = read_u16(r)?;
        let flags = read_u8(r)?;
        Ok(Self {
            capability_id,
            flags,
        })
    }
}

/// 命名空间根条目（RFC 025）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceEntry {
    pub name: String,
    /// 来源 crate 索引（关联 L1 crate_dag）。
    pub source_crate: u8,
}

impl NamespaceEntry {
    pub fn new(name: impl Into<String>, source_crate: u8) -> Self {
        Self {
            name: name.into(),
            source_crate,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_str_u8(w, &self.name);
        w.push(self.source_crate);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let name = read_str_u8(r)?;
        let source_crate = read_u8(r)?;
        Ok(Self { name, source_crate })
    }
}

/// 架构红线违反记录（编译期 lint 产出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedlineEntry {
    pub rule_id: u16,
    /// 0=warning / 1=error
    pub severity: u8,
    pub message: String,
}

impl RedlineEntry {
    pub fn new(rule_id: u16, severity: u8, message: impl Into<String>) -> Self {
        Self {
            rule_id,
            severity,
            message: message.into(),
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u16(w, self.rule_id);
        w.push(self.severity);
        write_str_u16(w, &self.message);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let rule_id = read_u16(r)?;
        let severity = read_u8(r)?;
        let message = read_str_u16(r)?;
        Ok(Self {
            rule_id,
            severity,
            message,
        })
    }
}

/// crate DAG 摘要（L0 级别概览数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CrateDagSummary {
    pub crate_count: u8,
    pub edge_count: u16,
}

impl CrateDagSummary {
    pub fn new(crate_count: u8, edge_count: u16) -> Self {
        Self {
            crate_count,
            edge_count,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        w.push(self.crate_count);
        write_u16(w, self.edge_count);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let crate_count = read_u8(r)?;
        let edge_count = read_u16(r)?;
        Ok(Self {
            crate_count,
            edge_count,
        })
    }
}

/// L0 项目概览（~500 token）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct L0ProjectOverview {
    pub name: String,
    pub kind: ProjectKind,
    pub version_major: u16,
    pub version_minor: u16,
    pub version_patch: u16,
    /// Arc edition（RFC 008）。
    pub edition: u16,
    /// Arc ABI 版本。
    pub arc_abi_version: u16,
    /// LLVM 版本锁定（RFC 015）。
    pub llvm_version: u16,
    pub target_triple: String,
    pub dependencies: Vec<DependencyEntry>,
    pub capabilities: Vec<CapabilityDecl>,
    pub namespaces: Vec<NamespaceEntry>,
    pub architecture_redlines: Vec<RedlineEntry>,
    pub crate_dag_summary: CrateDagSummary,
}

impl L0ProjectOverview {
    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_str_u16(w, &self.name);
        w.push(self.kind as u8);
        write_u16(w, self.version_major);
        write_u16(w, self.version_minor);
        write_u16(w, self.version_patch);
        write_u16(w, self.edition);
        write_u16(w, self.arc_abi_version);
        write_u16(w, self.llvm_version);
        write_str_u8(w, &self.target_triple);

        write_u16(w, self.dependencies.len() as u16);
        for dep in &self.dependencies {
            dep.serialize(w);
        }

        w.push(self.capabilities.len() as u8);
        for cap in &self.capabilities {
            cap.serialize(w);
        }

        w.push(self.namespaces.len() as u8);
        for ns in &self.namespaces {
            ns.serialize(w);
        }

        w.push(self.architecture_redlines.len() as u8);
        for redline in &self.architecture_redlines {
            redline.serialize(w);
        }

        self.crate_dag_summary.serialize(w);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let name = read_str_u16(r)?;
        let kind = ProjectKind::from_u8(read_u8(r)?)?;
        let version_major = read_u16(r)?;
        let version_minor = read_u16(r)?;
        let version_patch = read_u16(r)?;
        let edition = read_u16(r)?;
        let arc_abi_version = read_u16(r)?;
        let llvm_version = read_u16(r)?;
        let target_triple = read_str_u8(r)?;

        let dep_count = read_u16(r)? as usize;
        let mut dependencies = Vec::with_capacity(dep_count);
        for _ in 0..dep_count {
            dependencies.push(DependencyEntry::deserialize(r)?);
        }

        let cap_count = read_u8(r)? as usize;
        let mut capabilities = Vec::with_capacity(cap_count);
        for _ in 0..cap_count {
            capabilities.push(CapabilityDecl::deserialize(r)?);
        }

        let ns_count = read_u8(r)? as usize;
        let mut namespaces = Vec::with_capacity(ns_count);
        for _ in 0..ns_count {
            namespaces.push(NamespaceEntry::deserialize(r)?);
        }

        let redline_count = read_u8(r)? as usize;
        let mut architecture_redlines = Vec::with_capacity(redline_count);
        for _ in 0..redline_count {
            architecture_redlines.push(RedlineEntry::deserialize(r)?);
        }

        let crate_dag_summary = CrateDagSummary::deserialize(r)?;

        Ok(Self {
            name,
            kind,
            version_major,
            version_minor,
            version_patch,
            edition,
            arc_abi_version,
            llvm_version,
            target_triple,
            dependencies,
            capabilities,
            namespaces,
            architecture_redlines,
            crate_dag_summary,
        })
    }
}

// ============================================================================
// L1 模块面（~2K token）
// ============================================================================

/// 公共 API 种类（1 字节枚举，与 SymbolKind 大致对齐但有差异）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum PublicApiKind {
    #[default]
    Function = 0,
    Method = 1,
    StaticMethod = 2,
    Property = 3,
    Class = 4,
    Struct = 5,
    Interface = 6,
    Enum = 7,
    Variant = 8,
    /// namespace 级别
    Module = 9,
}

impl PublicApiKind {
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Function,
            1 => Self::Method,
            2 => Self::StaticMethod,
            3 => Self::Property,
            4 => Self::Class,
            5 => Self::Struct,
            6 => Self::Interface,
            7 => Self::Enum,
            8 => Self::Variant,
            9 => Self::Module,
            other => return Err(ArcgrError::InvalidPublicApiKind(other)),
        })
    }
}

/// DAG 边种类（1 字节枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum DagEdgeKind {
    /// 编译期依赖（`use crate::Symbol`）
    #[default]
    CompileDep = 0,
    /// 链接期依赖（`.ao` 链接）
    LinkDep = 1,
    /// 开发依赖（test/profiling）
    DevDep = 2,
}

impl DagEdgeKind {
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::CompileDep,
            1 => Self::LinkDep,
            2 => Self::DevDep,
            other => return Err(ArcgrError::InvalidDagEdgeKind(other)),
        })
    }
}

/// 公共 API 条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicApiEntry {
    /// 关联 SymbolTable SymbolId。
    pub symbol_id: u32,
    pub api_kind: PublicApiKind,
    /// 同 ExportEntry.Visibility。
    pub visibility: u8,
}

impl PublicApiEntry {
    pub fn new(symbol_id: u32, api_kind: PublicApiKind, visibility: u8) -> Self {
        Self {
            symbol_id,
            api_kind,
            visibility,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u32(w, self.symbol_id);
        w.push(self.api_kind as u8);
        w.push(self.visibility);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let symbol_id = read_u32(r)?;
        let api_kind = PublicApiKind::from_u8(read_u8(r)?)?;
        let visibility = read_u8(r)?;
        Ok(Self {
            symbol_id,
            api_kind,
            visibility,
        })
    }
}

/// crate 模块条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateModule {
    pub crate_id: u8,
    pub name: String,
    /// crate 路径（相对项目根）。
    pub path: String,
    /// 模块职责描述。
    pub responsibility: String,
    pub public_apis: Vec<PublicApiEntry>,
    /// namespaces 索引数组（关联 L0 namespaces）。
    pub namespaces: Vec<u8>,
}

impl CrateModule {
    pub fn serialize(&self, w: &mut Vec<u8>) {
        w.push(self.crate_id);
        write_str_u8(w, &self.name);
        write_str_u16(w, &self.path);
        write_str_u16(w, &self.responsibility);

        write_u16(w, self.public_apis.len() as u16);
        for api in &self.public_apis {
            api.serialize(w);
        }

        w.push(self.namespaces.len() as u8);
        for idx in &self.namespaces {
            w.push(*idx);
        }
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let crate_id = read_u8(r)?;
        let name = read_str_u8(r)?;
        let path = read_str_u16(r)?;
        let responsibility = read_str_u16(r)?;

        let api_count = read_u16(r)? as usize;
        let mut public_apis = Vec::with_capacity(api_count);
        for _ in 0..api_count {
            public_apis.push(PublicApiEntry::deserialize(r)?);
        }

        let ns_count = read_u8(r)? as usize;
        let mut namespaces = Vec::with_capacity(ns_count);
        for _ in 0..ns_count {
            namespaces.push(read_u8(r)?);
        }

        Ok(Self {
            crate_id,
            name,
            path,
            responsibility,
            public_apis,
            namespaces,
        })
    }
}

/// crate DAG 边。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DagEdge {
    pub from_crate_id: u8,
    pub to_crate_id: u8,
    pub edge_kind: DagEdgeKind,
}

impl DagEdge {
    pub fn new(from: u8, to: u8, kind: DagEdgeKind) -> Self {
        Self {
            from_crate_id: from,
            to_crate_id: to,
            edge_kind: kind,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        w.push(self.from_crate_id);
        w.push(self.to_crate_id);
        w.push(self.edge_kind as u8);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let from_crate_id = read_u8(r)?;
        let to_crate_id = read_u8(r)?;
        let edge_kind = DagEdgeKind::from_u8(read_u8(r)?)?;
        Ok(Self {
            from_crate_id,
            to_crate_id,
            edge_kind,
        })
    }
}

/// L1 模块面（~2K token）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct L1ModuleSurface {
    pub crates: Vec<CrateModule>,
    pub dag_edges: Vec<DagEdge>,
}

impl L1ModuleSurface {
    pub fn serialize(&self, w: &mut Vec<u8>) {
        w.push(self.crates.len() as u8);
        for c in &self.crates {
            c.serialize(w);
        }
        write_u16(w, self.dag_edges.len() as u16);
        for e in &self.dag_edges {
            e.serialize(w);
        }
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let crate_count = read_u8(r)? as usize;
        let mut crates = Vec::with_capacity(crate_count);
        for _ in 0..crate_count {
            crates.push(CrateModule::deserialize(r)?);
        }
        let edge_count = read_u16(r)? as usize;
        let mut dag_edges = Vec::with_capacity(edge_count);
        for _ in 0..edge_count {
            dag_edges.push(DagEdge::deserialize(r)?);
        }
        Ok(Self { crates, dag_edges })
    }
}

// ============================================================================
// ContextManifest——L0 + L1 顶层组合
// ============================================================================

/// ContextManifest 子表——L0 项目概览 + L1 模块面。
///
/// M2 阶段不产出（`Option::None`），M4 实施期填充。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextManifest {
    pub l0_project: L0ProjectOverview,
    pub l1_module_surface: L1ModuleSurface,
}

impl ContextManifest {
    pub fn new(l0: L0ProjectOverview, l1: L1ModuleSurface) -> Self {
        Self {
            l0_project: l0,
            l1_module_surface: l1,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        self.l0_project.serialize(w);
        self.l1_module_surface.serialize(w);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let l0_project = L0ProjectOverview::deserialize(r)?;
        let l1_module_surface = L1ModuleSurface::deserialize(r)?;
        Ok(Self {
            l0_project,
            l1_module_surface,
        })
    }
}

// ============================================================================
// 内部 IO 辅助——长度前缀为 u8 / u16 的 UTF-8 字符串
// ============================================================================

fn write_str_u8(w: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u8::MAX as usize) as u8;
    w.push(len);
    w.extend_from_slice(&bytes[..len as usize]);
}

fn read_str_u8(r: &mut &[u8]) -> Result<String> {
    let len = read_u8(r)? as usize;
    if r.len() < len {
        return Err(ArcgrError::SectionTruncated("ContextManifest str_u8"));
    }
    let s = std::str::from_utf8(&r[..len])
        .map_err(|_| ArcgrError::Utf8Error("ContextManifest str_u8"))?
        .to_string();
    *r = &r[len..];
    Ok(s)
}

fn write_str_u16(w: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    write_u16(w, len);
    w.extend_from_slice(&bytes[..len as usize]);
}

fn read_str_u16(r: &mut &[u8]) -> Result<String> {
    let len = read_u16(r)? as usize;
    if r.len() < len {
        return Err(ArcgrError::SectionTruncated("ContextManifest str_u16"));
    }
    let s = std::str::from_utf8(&r[..len])
        .map_err(|_| ArcgrError::Utf8Error("ContextManifest str_u16"))?
        .to_string();
    *r = &r[len..];
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_l0() -> L0ProjectOverview {
        L0ProjectOverview {
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
            namespaces: vec![
                NamespaceEntry::new("Arc", 0),
                NamespaceEntry::new("Arc.IO", 1),
            ],
            architecture_redlines: vec![RedlineEntry::new(101, 1, "lib.rs exceeds 80 lines")],
            crate_dag_summary: CrateDagSummary::new(2, 1),
        }
    }

    fn sample_l1() -> L1ModuleSurface {
        L1ModuleSurface {
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
            dag_edges: vec![
                DagEdge::new(0, 1, DagEdgeKind::CompileDep),
                DagEdge::new(0, 2, DagEdgeKind::LinkDep),
            ],
        }
    }

    #[test]
    fn l0_round_trip() {
        let l0 = sample_l0();
        let mut buf = Vec::new();
        l0.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let l0_2 = L0ProjectOverview::deserialize(&mut slice).unwrap();
        assert_eq!(l0, l0_2);
        assert!(slice.is_empty());
    }

    #[test]
    fn l1_round_trip() {
        let l1 = sample_l1();
        let mut buf = Vec::new();
        l1.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let l1_2 = L1ModuleSurface::deserialize(&mut slice).unwrap();
        assert_eq!(l1, l1_2);
        assert!(slice.is_empty());
    }

    #[test]
    fn full_manifest_round_trip() {
        let manifest = ContextManifest::new(sample_l0(), sample_l1());
        let mut buf = Vec::new();
        manifest.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let manifest2 = ContextManifest::deserialize(&mut slice).unwrap();
        assert_eq!(manifest, manifest2);
        assert!(slice.is_empty());
    }

    #[test]
    fn empty_manifest_round_trip() {
        let manifest = ContextManifest::default();
        let mut buf = Vec::new();
        manifest.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let manifest2 = ContextManifest::deserialize(&mut slice).unwrap();
        assert_eq!(manifest, manifest2);
    }

    #[test]
    fn all_project_kinds_round_trip() {
        let kinds = [
            ProjectKind::Executable,
            ProjectKind::Library,
            ProjectKind::DynamicLibrary,
            ProjectKind::Test,
        ];
        for kind in kinds {
            let mut l0 = sample_l0();
            l0.kind = kind;
            let mut buf = Vec::new();
            l0.serialize(&mut buf);
            let mut slice = buf.as_slice();
            let l0_2 = L0ProjectOverview::deserialize(&mut slice).unwrap();
            assert_eq!(l0, l0_2);
        }
    }

    #[test]
    fn all_dependency_sources_round_trip() {
        let sources = [
            DependencySource::Git,
            DependencySource::Path,
            DependencySource::Precompiled,
        ];
        for (i, src) in sources.iter().enumerate() {
            let dep = DependencyEntry::new("Test", 1, 0, i as u16, *src);
            let mut buf = Vec::new();
            dep.serialize(&mut buf);
            let mut slice = buf.as_slice();
            let dep2 = DependencyEntry::deserialize(&mut slice).unwrap();
            assert_eq!(dep, dep2);
        }
    }

    #[test]
    fn all_dag_edge_kinds_round_trip() {
        let kinds = [
            DagEdgeKind::CompileDep,
            DagEdgeKind::LinkDep,
            DagEdgeKind::DevDep,
        ];
        for (i, k) in kinds.iter().enumerate() {
            let edge = DagEdge::new(0, i as u8, *k);
            let mut buf = Vec::new();
            edge.serialize(&mut buf);
            let mut slice = buf.as_slice();
            let edge2 = DagEdge::deserialize(&mut slice).unwrap();
            assert_eq!(edge, edge2);
        }
    }

    #[test]
    fn all_public_api_kinds_round_trip() {
        let kinds = [
            PublicApiKind::Function,
            PublicApiKind::Method,
            PublicApiKind::StaticMethod,
            PublicApiKind::Property,
            PublicApiKind::Class,
            PublicApiKind::Struct,
            PublicApiKind::Interface,
            PublicApiKind::Enum,
            PublicApiKind::Variant,
            PublicApiKind::Module,
        ];
        for (i, k) in kinds.iter().enumerate() {
            let api = PublicApiEntry::new(i as u32, *k, 0);
            let mut buf = Vec::new();
            api.serialize(&mut buf);
            let mut slice = buf.as_slice();
            let api2 = PublicApiEntry::deserialize(&mut slice).unwrap();
            assert_eq!(api, api2);
        }
    }

    #[test]
    fn invalid_project_kind_rejected() {
        let err = ProjectKind::from_u8(0xFF).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidProjectKind(0xFF)));
    }

    #[test]
    fn invalid_dependency_source_rejected() {
        let err = DependencySource::from_u8(0xFF).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidDependencySource(0xFF)));
    }

    #[test]
    fn invalid_public_api_kind_rejected() {
        let err = PublicApiKind::from_u8(0xFF).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidPublicApiKind(0xFF)));
    }

    #[test]
    fn invalid_dag_edge_kind_rejected() {
        let err = DagEdgeKind::from_u8(0xFF).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidDagEdgeKind(0xFF)));
    }

    #[test]
    fn crate_module_with_empty_apis_round_trip() {
        let module = CrateModule {
            crate_id: 1,
            name: "empty".into(),
            path: "crates/empty".into(),
            responsibility: String::new(),
            public_apis: vec![],
            namespaces: vec![],
        };
        let mut buf = Vec::new();
        module.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let module2 = CrateModule::deserialize(&mut slice).unwrap();
        assert_eq!(module, module2);
    }

    #[test]
    fn redline_entry_round_trip() {
        let r = RedlineEntry::new(42, 1, "violation: missing doc");
        let mut buf = Vec::new();
        r.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let r2 = RedlineEntry::deserialize(&mut slice).unwrap();
        assert_eq!(r, r2);
    }
}
