//! `.arcgr` 文件整体编解码（RFC 034）。
//!
//! ## 文件结构
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │ Header (76 bytes)                    │
//! │   Magic "AIDX" + Version(2) + Flags   │
//! │   + 8 section 偏移/大小 + CRC32       │
//! ├──────────────────────────────────────┤
//! │ FileTable section                    │  // M2 必填
//! ├──────────────────────────────────────┤
//! │ SymbolTable section                  │  // M2 必填
//! ├──────────────────────────────────────┤
//! │ ReferenceTable section               │  // M2 必填
//! ├──────────────────────────────────────┤
//! │ ReferenceGraph section               │  // M2 必填
//! ├──────────────────────────────────────┤
//! │ ContextManifest section (optional)   │  // M4，缺失则跳过
//! ├──────────────────────────────────────┤
//! │ TypeRelationGraph section (optional) │  // M3+，缺失则跳过
//! ├──────────────────────────────────────┤
//! │ CompletionTable section (optional)    │  // M3+，缺失则跳过
//! ├──────────────────────────────────────┤
//! │ DiagnosticCache section (optional)    │  // M3+，缺失则跳过
//! └──────────────────────────────────────┘
//! ```
//!
//! M2 阶段仅产出前 4 个必填 section；后 4 个可选 section 偏移/大小为 0
//! （在 Header 中标记为不存在）。M3/M4/M5 各自里程碑填充对应 section。

use crate::context_manifest::ContextManifest;
use crate::error::{ArcgrError, Result};
use crate::file_table::FileTable;
use crate::header::{ArcgrHeader, HEADER_SIZE};
use crate::reference_graph::ReferenceGraph;
use crate::reference_table::ReferenceTable;
use crate::symbol_table::SymbolTable;

/// 完整的 `.arcgr` 文件内容。
///
/// M2 阶段仅前 4 个字段有数据；`context_manifest` 为 `None`（M4 填充）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ArcgrFile {
    pub file_table: FileTable,
    pub symbol_table: SymbolTable,
    pub reference_table: ReferenceTable,
    pub reference_graph: ReferenceGraph,
    /// ContextManifest 子表（M4 实施，M2 阶段为 `None`）。
    pub context_manifest: Option<ContextManifest>,
}

impl ArcgrFile {
    pub fn new() -> Self {
        Self::default()
    }

    /// 序列化为字节向量（包含 Header + 全部 section）。
    ///
    /// `context_manifest=None` 时仅产出 4 个 M2 必填 section；
    /// `Some(...)` 时追加 ContextManifest section 并填充 Header 偏移。
    pub fn serialize(&self) -> Vec<u8> {
        // 先序列化 4 个必填 section
        let mut file_table_buf = Vec::new();
        self.file_table.serialize(&mut file_table_buf);

        let mut symbol_table_buf = Vec::new();
        self.symbol_table.serialize(&mut symbol_table_buf);

        let mut reference_table_buf = Vec::new();
        self.reference_table.serialize(&mut reference_table_buf);

        let mut reference_graph_buf = Vec::new();
        self.reference_graph.serialize(&mut reference_graph_buf);

        // 可选 ContextManifest section
        let context_manifest_buf = match &self.context_manifest {
            Some(m) => {
                let mut buf = Vec::new();
                m.serialize(&mut buf);
                buf
            }
            None => Vec::new(),
        };

        // 计算各 section 偏移
        let file_table_off = HEADER_SIZE;
        let symbol_table_off = file_table_off + file_table_buf.len() as u32;
        let reference_table_off = symbol_table_off + symbol_table_buf.len() as u32;
        let reference_graph_off = reference_table_off + reference_table_buf.len() as u32;
        let context_manifest_off = if context_manifest_buf.is_empty() {
            0
        } else {
            reference_graph_off + reference_graph_buf.len() as u32
        };

        let header = ArcgrHeader {
            version: crate::header::VERSION,
            flags: 0,
            file_table_off,
            file_table_size: file_table_buf.len() as u32,
            symbol_table_off,
            symbol_table_size: symbol_table_buf.len() as u32,
            reference_table_off,
            reference_table_size: reference_table_buf.len() as u32,
            reference_graph_off,
            reference_graph_size: reference_graph_buf.len() as u32,
            context_manifest_off,
            context_manifest_size: context_manifest_buf.len() as u32,
            type_relation_graph_off: 0,
            type_relation_graph_size: 0,
            completion_table_off: 0,
            completion_table_size: 0,
            diagnostic_cache_off: 0,
            diagnostic_cache_size: 0,
        };

        let mut buf = Vec::with_capacity(
            HEADER_SIZE as usize
                + file_table_buf.len()
                + symbol_table_buf.len()
                + reference_table_buf.len()
                + reference_graph_buf.len()
                + context_manifest_buf.len(),
        );
        buf.extend_from_slice(&header.serialize());
        buf.extend_from_slice(&file_table_buf);
        buf.extend_from_slice(&symbol_table_buf);
        buf.extend_from_slice(&reference_table_buf);
        buf.extend_from_slice(&reference_graph_buf);
        if !context_manifest_buf.is_empty() {
            buf.extend_from_slice(&context_manifest_buf);
        }
        buf
    }

    /// 从字节切片反序列化。
    ///
    /// 解析 Header 后按 section 偏移逐段反序列化。可选 section 缺失
    /// （`off == 0 && size == 0`）时对应字段置为 `None`。
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        let header = ArcgrHeader::deserialize(bytes)?;

        // 校验必填 section 偏移量边界
        let check = |section: &'static str, off: u32, size: u32| -> Result<()> {
            let end = off as usize + size as usize;
            if end > bytes.len() {
                return Err(ArcgrError::OffsetOutOfBounds {
                    section,
                    offset: off,
                    file_size: bytes.len(),
                });
            }
            Ok(())
        };
        check("file_table", header.file_table_off, header.file_table_size)?;
        check(
            "symbol_table",
            header.symbol_table_off,
            header.symbol_table_size,
        )?;
        check(
            "reference_table",
            header.reference_table_off,
            header.reference_table_size,
        )?;
        check(
            "reference_graph",
            header.reference_graph_off,
            header.reference_graph_size,
        )?;

        // 解析 4 个必填 section
        let file_table = {
            let start = header.file_table_off as usize;
            let end = start + header.file_table_size as usize;
            let mut slice = &bytes[start..end];
            FileTable::deserialize(&mut slice)?
        };
        let symbol_table = {
            let start = header.symbol_table_off as usize;
            let end = start + header.symbol_table_size as usize;
            let mut slice = &bytes[start..end];
            SymbolTable::deserialize(&mut slice)?
        };
        let reference_table = {
            let start = header.reference_table_off as usize;
            let end = start + header.reference_table_size as usize;
            let mut slice = &bytes[start..end];
            ReferenceTable::deserialize(&mut slice)?
        };
        let reference_graph = {
            let start = header.reference_graph_off as usize;
            let end = start + header.reference_graph_size as usize;
            let mut slice = &bytes[start..end];
            ReferenceGraph::deserialize(&mut slice)?
        };

        // 解析可选 ContextManifest section
        let context_manifest =
            if header.context_manifest_off != 0 || header.context_manifest_size != 0 {
                check(
                    "context_manifest",
                    header.context_manifest_off,
                    header.context_manifest_size,
                )?;
                let start = header.context_manifest_off as usize;
                let end = start + header.context_manifest_size as usize;
                let mut slice = &bytes[start..end];
                Some(ContextManifest::deserialize(&mut slice)?)
            } else {
                None
            };

        Ok(Self {
            file_table,
            symbol_table,
            reference_table,
            reference_graph,
            context_manifest,
        })
    }
}

/// 便捷函数：序列化 `ArcgrFile` 为字节。
pub fn write_arcgr(file: &ArcgrFile) -> Vec<u8> {
    file.serialize()
}

/// 便捷函数：从字节解析 `ArcgrFile`。
pub fn read_arcgr(bytes: &[u8]) -> Result<ArcgrFile> {
    ArcgrFile::deserialize(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_manifest::{
        CapabilityDecl, CrateDagSummary, CrateModule, DagEdge, DagEdgeKind, DependencyEntry,
        DependencySource, L0ProjectOverview, L1ModuleSurface, NamespaceEntry, ProjectKind,
        PublicApiEntry, PublicApiKind, RedlineEntry,
    };
    use crate::file_table::FileEntry;
    use crate::reference_graph::{Edge, EdgeKind, EntryPoint, EntryPointKind};
    use crate::reference_table::{ReferenceContext, ReferenceEntry};
    use crate::symbol_table::{SymbolEntry, SymbolKind, Visibility};
    use crate::typesig::TypeSig;

    fn sample_file() -> ArcgrFile {
        let mut file = ArcgrFile::new();

        // FileTable
        file.file_table
            .push(FileEntry::new(0, "/proj/main.as".into(), 0xABCD, 10));
        file.file_table
            .push(FileEntry::new(1, "/proj/util.as".into(), 0xDCBA, 20));

        // SymbolTable
        file.symbol_table.push(SymbolEntry::new(
            0,
            "main",
            SymbolKind::Function,
            Visibility::Public,
            0,
            0,
            50,
            TypeSig::Func {
                params: vec![],
                ret: Box::new(TypeSig::Unit),
                captures: false,
            },
            None,
        ));
        file.symbol_table.push(SymbolEntry::new(
            1,
            "Calculator",
            SymbolKind::Class,
            Visibility::Public,
            1,
            0,
            100,
            TypeSig::Named {
                fully_qualified_name: "Calculator".into(),
                generic_args: vec![],
            },
            Some("Calculator class".into()),
        ));
        file.symbol_table.push(SymbolEntry::new(
            2,
            "add",
            SymbolKind::Method,
            Visibility::Public,
            1,
            30,
            80,
            TypeSig::Method {
                receiver: Box::new(TypeSig::Named {
                    fully_qualified_name: "Calculator".into(),
                    generic_args: vec![],
                }),
                params: vec![TypeSig::Int, TypeSig::Int],
                ret: Box::new(TypeSig::Int),
                is_virtual: false,
                vtable_slot: 0,
            },
            None,
        ));

        // ReferenceTable
        file.reference_table.push(ReferenceEntry::new(
            0,
            2,
            0,
            100,
            110,
            ReferenceContext::Call,
        ));
        file.reference_table.push(ReferenceEntry::new(
            1,
            1,
            0,
            200,
            210,
            ReferenceContext::TypeAnnotation,
        ));

        // ReferenceGraph
        file.reference_graph
            .entry_points
            .push(EntryPoint::new(0, EntryPointKind::Main, 0));
        file.reference_graph.reachable_symbols = vec![0, 1, 2];
        file.reference_graph.unreachable_symbols = vec![3, 4];
        file.reference_graph
            .edges
            .push(Edge::new(0, 2, EdgeKind::MethodCall, 0, 100, 110, true));

        file
    }

    fn sample_context_manifest() -> ContextManifest {
        let l0 = L0ProjectOverview {
            name: "TestProject".into(),
            kind: ProjectKind::Executable,
            version_major: 1,
            version_minor: 2,
            version_patch: 3,
            edition: 2024,
            arc_abi_version: 1,
            llvm_version: 22,
            target_triple: "x86_64-pc-windows-msvc".into(),
            dependencies: vec![DependencyEntry::new(
                "Arc.Runtime",
                1,
                0,
                0,
                DependencySource::Precompiled,
            )],
            capabilities: vec![CapabilityDecl::new(1, 0)],
            namespaces: vec![NamespaceEntry::new("Arc", 0)],
            architecture_redlines: vec![RedlineEntry::new(101, 1, "lib.rs too long")],
            crate_dag_summary: CrateDagSummary::new(1, 0),
        };
        let l1 = L1ModuleSurface {
            crates: vec![CrateModule {
                crate_id: 0,
                name: "arc".into(),
                path: "crates/arc".into(),
                responsibility: "Arc compiler".into(),
                public_apis: vec![PublicApiEntry::new(0, PublicApiKind::Function, 0)],
                namespaces: vec![0],
            }],
            dag_edges: vec![DagEdge::new(0, 1, DagEdgeKind::CompileDep)],
        };
        ContextManifest::new(l0, l1)
    }

    #[test]
    fn full_file_round_trip() {
        let file = sample_file();
        let bytes = file.serialize();
        assert!(bytes.len() > HEADER_SIZE as usize);

        let file2 = read_arcgr(&bytes).unwrap();
        assert_eq!(file, file2);
    }

    #[test]
    fn empty_file_round_trip() {
        let file = ArcgrFile::new();
        let bytes = file.serialize();
        // Header(76) + 4 个表各 count(4 字节) + ReferenceGraph 额外 3 个 count 字段
        // FileTable(4) + SymbolTable(4) + ReferenceTable(4) + ReferenceGraph(16) = 28
        // 总计 76 + 28 = 104
        assert_eq!(bytes.len(), 104);

        let file2 = read_arcgr(&bytes).unwrap();
        assert_eq!(file, file2);
    }

    #[test]
    fn file_with_context_manifest_round_trip() {
        let mut file = sample_file();
        file.context_manifest = Some(sample_context_manifest());

        let bytes = file.serialize();
        let file2 = read_arcgr(&bytes).unwrap();
        assert_eq!(file, file2);
        assert!(file2.context_manifest.is_some());
    }

    #[test]
    fn file_without_context_manifest_has_zero_offset() {
        let file = sample_file();
        let bytes = file.serialize();
        let header = ArcgrHeader::deserialize(&bytes).unwrap();

        assert_eq!(header.context_manifest_off, 0);
        assert_eq!(header.context_manifest_size, 0);
    }

    #[test]
    fn corrupted_header_rejected() {
        let file = sample_file();
        let mut bytes = file.serialize();
        bytes[0] = b'X'; // 破坏 magic
        let err = read_arcgr(&bytes).unwrap_err();
        assert!(matches!(err, ArcgrError::BadMagic(_)));
    }

    #[test]
    fn truncated_buffer_rejected() {
        let file = sample_file();
        let mut bytes = file.serialize();
        // 截断到不足以容纳所有 section（保留完整 Header 76 字节，但截断 FileTable 之后）
        bytes.truncate(120);
        let err = read_arcgr(&bytes).unwrap_err();
        // 截断会触发 OffsetOutOfBounds 或 SectionTruncated（Header 仍完整）
        match err {
            ArcgrError::OffsetOutOfBounds { .. } | ArcgrError::SectionTruncated(_) => {}
            other => panic!("expected OffsetOutOfBounds or SectionTruncated, got {other:?}"),
        }
    }

    #[test]
    fn header_offset_consistency() {
        let file = sample_file();
        let bytes = file.serialize();
        let header = ArcgrHeader::deserialize(&bytes).unwrap();

        // 表偏移量必须连续，无空隙（仅 4 个必填 section）
        assert_eq!(
            header.symbol_table_off,
            header.file_table_off + header.file_table_size
        );
        assert_eq!(
            header.reference_table_off,
            header.symbol_table_off + header.symbol_table_size
        );
        assert_eq!(
            header.reference_graph_off,
            header.reference_table_off + header.reference_table_size
        );

        // 总长度 = 最后一张表末尾（无 ContextManifest 时）
        let expected_len = header.reference_graph_off + header.reference_graph_size;
        assert_eq!(bytes.len() as u32, expected_len);
    }

    #[test]
    fn header_offset_consistency_with_context_manifest() {
        let mut file = sample_file();
        file.context_manifest = Some(sample_context_manifest());
        let bytes = file.serialize();
        let header = ArcgrHeader::deserialize(&bytes).unwrap();

        // 5 个 section 偏移量必须连续
        assert_eq!(
            header.context_manifest_off,
            header.reference_graph_off + header.reference_graph_size
        );

        let expected_len = header.context_manifest_off + header.context_manifest_size;
        assert_eq!(bytes.len() as u32, expected_len);
    }
}
