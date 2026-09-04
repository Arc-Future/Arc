//! `.arcgr` ReferenceGraph 子表（RFC 034）。
//!
//! ## 二进制布局
//!
//! ```text
//! ReferenceGraph section:
//!   entry_points_count: u32 LE
//!   entry_points[]:
//!     EntryPoint[i]:
//!       symbol_id: 4 bytes u32 LE
//!       kind: 1 byte (EntryPointKind enum)
//!       priority: 1 byte u8
//!   reachable_count: u32 LE
//!   reachable_symbol_ids[]: 4 bytes u32 LE × reachable_count（排序）
//!   unreachable_count: u32 LE
//!   unreachable_symbol_ids[]: 4 bytes u32 LE × unreachable_count（排序）
//!   edge_count: u32 LE
//!   edges[]:
//!     ReferenceEdge[i]:
//!       caller_symbol_id: 4 bytes u32 LE
//!       callee_symbol_id: 4 bytes u32 LE
//!       edge_kind: 1 byte (EdgeKind enum)
//!       file_id: 4 bytes u32 LE
//!       span_start: 4 bytes u32 LE
//!       span_end: 4 bytes u32 LE
//!       is_direct: 1 byte u8
//! ```

use crate::error::{ArcgrError, Result};
use crate::io::{read_u32, write_u32};

/// 入口点类别（1 字节，6 种）。
///
/// 与 LSP/调试器/DAP 共享——`[RFC 039](049-debugger-and-dap.md) M1` 断点可达性校验消费。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntryPointKind {
    /// `fn main()` 入口。
    Main = 0,
    /// `library` kind 的公共导出（`exports[]` Public 符号）。
    LibraryExport = 1,
    /// `[Test]` 标记的测试函数（RFC 032）。
    TestFunction = 2,
    /// 动态库约定符号入口（如 `__qif_init`，`rt_library_load` ABI——RFC 017 D8 v1.0 重命名）。
    DynamicLibEntry = 3,
    /// `extern "C"` 导出符号（RFC 016）。
    FFIExport = 4,
    /// codegen 生成的内部入口（如 `__arc_init`）。
    CGMain = 5,
}

impl EntryPointKind {
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Main,
            1 => Self::LibraryExport,
            2 => Self::TestFunction,
            3 => Self::DynamicLibEntry,
            4 => Self::FFIExport,
            5 => Self::CGMain,
            other => return Err(ArcgrError::InvalidEntryPointKind(other)),
        })
    }
}

/// 边类别（1 字节，8 种，覆盖 Arc 全部引用关系）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EdgeKind {
    /// 函数调用（含静态方法调用）。
    Call = 0,
    /// 实例方法调用（虚分派或非虚）。
    MethodCall = 1,
    /// 对象实例化（class 实例创建）。
    New = 2,
    /// 接口实现/继承（`class : Interface` / `class : BaseClass`）。
    Implement = 3,
    /// 字段访问（struct/class 实例字段读写）。
    FieldAccess = 4,
    /// 属性访问（get/set 调用）。
    PropertyAccess = 5,
    /// variant 模式匹配（RFC 004）。
    VariantMatch = 6,
    /// 泛型实例化（单态化触发的隐式边）。
    GenericInstantiation = 7,
}

impl EdgeKind {
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Call,
            1 => Self::MethodCall,
            2 => Self::New,
            3 => Self::Implement,
            4 => Self::FieldAccess,
            5 => Self::PropertyAccess,
            6 => Self::VariantMatch,
            7 => Self::GenericInstantiation,
            other => return Err(ArcgrError::InvalidEdgeKind(other)),
        })
    }
}

/// 入口点条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPoint {
    pub symbol_id: u32,
    pub kind: EntryPointKind,
    /// 优先级（0-255，0=最高；用于多入口排序）。
    pub priority: u8,
}

impl EntryPoint {
    pub fn new(symbol_id: u32, kind: EntryPointKind, priority: u8) -> Self {
        Self {
            symbol_id,
            kind,
            priority,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u32(w, self.symbol_id);
        w.push(self.kind as u8);
        w.push(self.priority);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let symbol_id = read_u32(r)?;
        let kind = EntryPointKind::from_u8(crate::io::read_u8(r)?)?;
        let priority = crate::io::read_u8(r)?;
        Ok(Self {
            symbol_id,
            kind,
            priority,
        })
    }
}

/// 引用边条目（caller → callee）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub caller_symbol_id: u32,
    pub callee_symbol_id: u32,
    pub edge_kind: EdgeKind,
    pub file_id: u32,
    pub span_start: u32,
    pub span_end: u32,
    /// 1=直接调用/引用，0=间接（如通过虚分派）。
    pub is_direct: bool,
}

impl Edge {
    pub fn new(
        caller_symbol_id: u32,
        callee_symbol_id: u32,
        edge_kind: EdgeKind,
        file_id: u32,
        span_start: u32,
        span_end: u32,
        is_direct: bool,
    ) -> Self {
        Self {
            caller_symbol_id,
            callee_symbol_id,
            edge_kind,
            file_id,
            span_start,
            span_end,
            is_direct,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u32(w, self.caller_symbol_id);
        write_u32(w, self.callee_symbol_id);
        w.push(self.edge_kind as u8);
        write_u32(w, self.file_id);
        write_u32(w, self.span_start);
        write_u32(w, self.span_end);
        w.push(if self.is_direct { 1u8 } else { 0u8 });
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let caller_symbol_id = read_u32(r)?;
        let callee_symbol_id = read_u32(r)?;
        let edge_kind = EdgeKind::from_u8(crate::io::read_u8(r)?)?;
        let file_id = read_u32(r)?;
        let span_start = read_u32(r)?;
        let span_end = read_u32(r)?;
        let is_direct = crate::io::read_u8(r)? != 0;
        Ok(Self {
            caller_symbol_id,
            callee_symbol_id,
            edge_kind,
            file_id,
            span_start,
            span_end,
            is_direct,
        })
    }
}

/// ReferenceGraph 子表——可达性分析多维输出。
///
/// 由 RFC 034 M2 实施产出（含 L2 入口可达性 Pass）；
/// 由 RFC 038 M6 增量维护；由 RFC 039 M1 entry_points 校验消费。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferenceGraph {
    pub entry_points: Vec<EntryPoint>,
    /// 可达符号 ID 数组（必须排序）。
    pub reachable_symbols: Vec<u32>,
    /// 不可达符号 ID 数组（必须排序）。
    pub unreachable_symbols: Vec<u32>,
    pub edges: Vec<Edge>,
}

impl ReferenceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        // entry_points
        write_u32(w, self.entry_points.len() as u32);
        for ep in &self.entry_points {
            ep.serialize(w);
        }
        // reachable_symbols
        write_u32(w, self.reachable_symbols.len() as u32);
        for &id in &self.reachable_symbols {
            write_u32(w, id);
        }
        // unreachable_symbols
        write_u32(w, self.unreachable_symbols.len() as u32);
        for &id in &self.unreachable_symbols {
            write_u32(w, id);
        }
        // edges
        write_u32(w, self.edges.len() as u32);
        for edge in &self.edges {
            edge.serialize(w);
        }
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let ep_count = read_u32(r)? as usize;
        let mut entry_points = Vec::with_capacity(ep_count);
        for _ in 0..ep_count {
            entry_points.push(EntryPoint::deserialize(r)?);
        }

        let reach_count = read_u32(r)? as usize;
        let mut reachable_symbols = Vec::with_capacity(reach_count);
        for _ in 0..reach_count {
            reachable_symbols.push(read_u32(r)?);
        }

        let unreach_count = read_u32(r)? as usize;
        let mut unreachable_symbols = Vec::with_capacity(unreach_count);
        for _ in 0..unreach_count {
            unreachable_symbols.push(read_u32(r)?);
        }

        let edge_count = read_u32(r)? as usize;
        let mut edges = Vec::with_capacity(edge_count);
        for _ in 0..edge_count {
            edges.push(Edge::deserialize(r)?);
        }

        Ok(Self {
            entry_points,
            reachable_symbols,
            unreachable_symbols,
            edges,
        })
    }

    /// 检查 symbol_id 是否在可达集合中（二分查找，要求 sorted）。
    pub fn is_reachable(&self, symbol_id: u32) -> bool {
        self.reachable_symbols.binary_search(&symbol_id).is_ok()
    }

    /// 检查 symbol_id 是否在不可达集合中（二分查找，要求 sorted）。
    pub fn is_unreachable(&self, symbol_id: u32) -> bool {
        self.unreachable_symbols.binary_search(&symbol_id).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph_round_trip() {
        let graph = ReferenceGraph::new();
        let mut buf = Vec::new();
        graph.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let graph2 = ReferenceGraph::deserialize(&mut slice).unwrap();
        assert_eq!(graph, graph2);
        assert!(slice.is_empty());
        // 4 个 count 字段，每个 4 字节
        assert_eq!(buf.len(), 16);
    }

    #[test]
    fn entry_points_round_trip() {
        let mut graph = ReferenceGraph::new();
        graph
            .entry_points
            .push(EntryPoint::new(0, EntryPointKind::Main, 0));
        graph
            .entry_points
            .push(EntryPoint::new(10, EntryPointKind::LibraryExport, 100));
        graph
            .entry_points
            .push(EntryPoint::new(20, EntryPointKind::FFIExport, 200));

        let mut buf = Vec::new();
        graph.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let graph2 = ReferenceGraph::deserialize(&mut slice).unwrap();
        assert_eq!(graph, graph2);
        assert_eq!(graph2.entry_points.len(), 3);
        assert!(slice.is_empty());
    }

    #[test]
    fn all_entry_point_kinds_round_trip() {
        let kinds = [
            EntryPointKind::Main,
            EntryPointKind::LibraryExport,
            EntryPointKind::TestFunction,
            EntryPointKind::DynamicLibEntry,
            EntryPointKind::FFIExport,
            EntryPointKind::CGMain,
        ];
        let mut graph = ReferenceGraph::new();
        for (i, kind) in kinds.iter().enumerate() {
            graph
                .entry_points
                .push(EntryPoint::new(i as u32, *kind, i as u8));
        }
        let mut buf = Vec::new();
        graph.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let graph2 = ReferenceGraph::deserialize(&mut slice).unwrap();
        assert_eq!(graph, graph2);
        for (i, ep) in graph2.entry_points.iter().enumerate() {
            assert_eq!(ep.kind, kinds[i]);
        }
    }

    #[test]
    fn all_edge_kinds_round_trip() {
        let kinds = [
            EdgeKind::Call,
            EdgeKind::MethodCall,
            EdgeKind::New,
            EdgeKind::Implement,
            EdgeKind::FieldAccess,
            EdgeKind::PropertyAccess,
            EdgeKind::VariantMatch,
            EdgeKind::GenericInstantiation,
        ];
        let mut graph = ReferenceGraph::new();
        for (i, kind) in kinds.iter().enumerate() {
            graph
                .edges
                .push(Edge::new(i as u32, (i + 1) as u32, *kind, 0, 0, 10, true));
        }
        let mut buf = Vec::new();
        graph.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let graph2 = ReferenceGraph::deserialize(&mut slice).unwrap();
        assert_eq!(graph, graph2);
        for (i, edge) in graph2.edges.iter().enumerate() {
            assert_eq!(edge.edge_kind, kinds[i]);
        }
    }

    #[test]
    fn reachable_unreachable_round_trip() {
        let mut graph = ReferenceGraph::new();
        graph.reachable_symbols = vec![0, 1, 2, 5, 10];
        graph.unreachable_symbols = vec![3, 4, 6, 7, 8, 9];

        let mut buf = Vec::new();
        graph.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let graph2 = ReferenceGraph::deserialize(&mut slice).unwrap();
        assert_eq!(graph, graph2);
        assert!(slice.is_empty());
    }

    #[test]
    fn is_reachable_binary_search() {
        let mut graph = ReferenceGraph::new();
        graph.reachable_symbols = vec![0, 1, 2, 5, 10];
        graph.unreachable_symbols = vec![3, 4, 6, 7, 8, 9];

        assert!(graph.is_reachable(0));
        assert!(graph.is_reachable(5));
        assert!(graph.is_reachable(10));
        assert!(!graph.is_reachable(3));
        assert!(!graph.is_reachable(99));

        assert!(graph.is_unreachable(3));
        assert!(graph.is_unreachable(9));
        assert!(!graph.is_unreachable(0));
    }

    #[test]
    fn invalid_edge_kind_rejected() {
        let mut buf = Vec::new();
        Edge::new(0, 1, EdgeKind::Call, 0, 0, 0, true).serialize(&mut buf);
        // edge_kind 在第 8 字节（caller=4 + callee=4 = offset 8）
        buf[8] = 0xFF;
        let mut slice = buf.as_slice();
        let err = Edge::deserialize(&mut slice).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidEdgeKind(0xFF)));
    }

    #[test]
    fn invalid_entry_point_kind_rejected() {
        let mut buf = Vec::new();
        EntryPoint::new(0, EntryPointKind::Main, 0).serialize(&mut buf);
        // kind 在第 4 字节（symbol_id=4 = offset 4）
        buf[4] = 0xFF;
        let mut slice = buf.as_slice();
        let err = EntryPoint::deserialize(&mut slice).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidEntryPointKind(0xFF)));
    }

    #[test]
    fn full_graph_with_all_components_round_trip() {
        let mut graph = ReferenceGraph::new();
        graph
            .entry_points
            .push(EntryPoint::new(0, EntryPointKind::Main, 0));
        graph
            .entry_points
            .push(EntryPoint::new(10, EntryPointKind::LibraryExport, 50));
        graph.reachable_symbols = vec![0, 1, 2, 10, 11];
        graph.unreachable_symbols = vec![3, 4, 5, 6, 7, 8, 9];
        graph
            .edges
            .push(Edge::new(0, 1, EdgeKind::Call, 0, 100, 110, true));
        graph
            .edges
            .push(Edge::new(0, 2, EdgeKind::Call, 0, 120, 130, true));
        graph
            .edges
            .push(Edge::new(1, 10, EdgeKind::MethodCall, 0, 200, 210, false));
        graph
            .edges
            .push(Edge::new(2, 11, EdgeKind::New, 1, 300, 310, true));

        let mut buf = Vec::new();
        graph.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let graph2 = ReferenceGraph::deserialize(&mut slice).unwrap();
        assert_eq!(graph, graph2);
        assert!(slice.is_empty());
    }
}
