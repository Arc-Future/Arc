//! `arcgr`：`.arcgr` 语义索引二进制格式（RFC 034）。
//!
//! ## 概述
//!
//! `.arcgr` 是 Arc 的全项目语义索引二进制产物，由 [RFC 034](../../../docs/rfc/034-ai-toolchain-arcgr.md)
//! 定义格式权威并实施产出。
//! 三 RFC 共享此数据底座：
//!
//! - **RFC 034**：AI 工具链消费（`arc inspect` / `arc query` / `arc overview`）
//! - **RFC 038**：LSP 服务消费（hover / definition / references / workspaceSymbol）
//! - **RFC 039**：调试器消费（`entry_points` 断点可达性校验）
//!
//! ## 二进制布局
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │ Header (76 bytes)                    │
//! │   Magic "AIDX" + Version(2) + Flags  │
//! │   + 8 section 偏移/大小 + CRC32      │
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
//! │ DiagnosticCache section (optional)   │  // M3+，缺失则跳过
//! └──────────────────────────────────────┘
//! ```
//!
//! ## M2 范围
//!
//! - Header (76 字节, VERSION=2) + FileTable + SymbolTable + ReferenceTable + ReferenceGraph 必填 section
//! - IntentMeta schema 完整定义（5 IntentRole + 5 IntentMetadata 变体），
//!   M2 阶段产出占位 `role=None, metadata=None`，M5 实施期填充真实数据
//! - ContextManifest schema 完整定义（L0 ProjectOverview + L1 ModuleSurface），
//!   M2 阶段为 `None` 占位，M4 实施期填充真实数据
//! - Header 已锁定全部 8 个 section 偏移字段，后 4 个 section M2 阶段 off=0/size=0 表示不存在
//! - 不实施 TypeRelationGraph / CompletionTable / DiagnosticCache（M3+）

pub mod context_manifest;
pub mod error;
pub mod file_table;
pub mod format;
pub mod header;
pub mod intent_meta;
pub mod io;
pub mod reference_graph;
pub mod reference_table;
pub mod symbol_table;
pub mod typesig;

pub use context_manifest::{
    CapabilityDecl, ContextManifest, CrateDagSummary, CrateModule, DagEdge, DagEdgeKind,
    DependencyEntry, DependencySource, L0ProjectOverview, L1ModuleSurface, NamespaceEntry,
    ProjectKind, PublicApiEntry, PublicApiKind, RedlineEntry,
};
pub use error::{ArcgrError, Result};
pub use file_table::{FileEntry, FileTable};
pub use format::{read_arcgr, write_arcgr, ArcgrFile};
pub use header::{ArcgrHeader, HeaderSection, HEADER_SIZE, MAGIC, VERSION};
pub use intent_meta::{IntentMeta, IntentMetadata, IntentRole};
pub use reference_graph::{
    Edge as ReferenceEdge, EdgeKind, EntryPoint, EntryPointKind, ReferenceGraph,
};
pub use reference_table::{ReferenceContext, ReferenceEntry, ReferenceTable};
pub use symbol_table::{SymbolEntry, SymbolKind, SymbolTable, Visibility};
pub use typesig::{TypeSig, TypeSigTag, VariantCase};
