//! L2 入口可达性分析核心算法（RFC 034 M2）。
//!
//! ## 算法概要
//!
//! 1. **入口标记**：从 `entry_points` 初始化 BFS 队列
//! 2. **BFS 遍历**：沿 `edges` 边传播可达性
//! 3. **虚分派保守**：当 `interface_method_id` 被引用时，所有
//!    `impl_method_ids` 保守标记可达
//! 4. **全集划分**：`universe ∩ reachable` = 可达集合；
//!    `universe - reachable` = 不可达集合
//!
//! ## 输入契约
//!
//! - `universe` 必须排序且去重（用于差集与二分查找）
//! - `edges` 中的 `caller_symbol_id` 必须在 `universe` 中
//!   （外部符号不会作为 caller 出现在本包 HIR 中）
//! - `edges` 中的 `callee_symbol_id` 可能在 `universe` 之外
//!   （外部 .ao 符号——BFS 到达后停止，无后续边可走）
//! - `entry_points.symbol_id` 必须在 `universe` 中
//! - `virtual_dispatch_groups.impl_method_ids` 必须在 `universe` 中
//!
//! ## 复杂度
//!
//! - 时间：O(V + E) 其中 V=universe 大小，E=edges 大小
//! - 空间：O(V + E) 用于 BFS 队列与访问集合

use std::collections::{HashMap, HashSet, VecDeque};

use arcgr::{EdgeKind, EntryPoint, EntryPointKind, ReferenceEdge as Edge, ReferenceGraph};

/// 虚分派保守组——接口方法被引用时，所有实现方法保守标记可达。
///
/// RFC 034 M2 保守策略：接口分派场景，被实现的接口及其所有方法
/// 保守标记为 reachable（typeck 后符号表完整，虚分派候选从实现链推导）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualDispatchGroup {
    /// 接口方法符号 ID（虚分派目标）。
    pub interface_method_id: u32,
    /// 所有实现该方法的具体类的方法符号 ID（保守全部标记可达）。
    pub impl_method_ids: Vec<u32>,
}

impl VirtualDispatchGroup {
    pub fn new(interface_method_id: u32, impl_method_ids: Vec<u32>) -> Self {
        Self {
            interface_method_id,
            impl_method_ids,
        }
    }
}

/// 分析输入——IR-agnostic 抽象输入。
///
/// 由 `arc` crate 的 `arcgr.rs` 模块（Step 3）从带类型 HIR + 跨包
/// `.ao` metadata 收集并填充，再传入 [`analyze`] 执行。
#[derive(Debug, Clone, Default)]
pub struct AnalysisInput {
    /// 入口点集合（`[Entry]` 属性 / main / library exports / FFI exports）。
    pub entry_points: Vec<EntryPoint>,
    /// 引用图边集合（caller → callee）。
    pub edges: Vec<Edge>,
    /// 本包所有符号 ID 全集（必须排序且去重）。
    pub universe: Vec<u32>,
    /// 虚分派保守组（接口方法 → 所有实现方法）。
    pub virtual_dispatch_groups: Vec<VirtualDispatchGroup>,
}

impl AnalysisInput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_entry_points(mut self, entry_points: Vec<EntryPoint>) -> Self {
        self.entry_points = entry_points;
        self
    }

    pub fn with_edges(mut self, edges: Vec<Edge>) -> Self {
        self.edges = edges;
        self
    }

    pub fn with_universe(mut self, universe: Vec<u32>) -> Self {
        let mut u = universe;
        u.sort_unstable();
        u.dedup();
        self.universe = u;
        self
    }

    pub fn with_virtual_dispatch_groups(mut self, groups: Vec<VirtualDispatchGroup>) -> Self {
        self.virtual_dispatch_groups = groups;
        self
    }
}

/// 分析报告——`analyze` 的产出，封装 `ReferenceGraph` 与统计元数据。
///
/// `reference_graph` 字段是 `.arcgr` 二进制产出的直接数据源；
/// 统计字段供 `arc inspect --format human` 等 CLI 场景消费。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisReport {
    pub reference_graph: ReferenceGraph,
    pub entry_point_count: usize,
    pub reachable_count: usize,
    pub unreachable_count: usize,
    pub edge_count: usize,
}

/// 执行 L2 入口可达性分析。
///
/// 算法步骤详见[模块文档](self)。
///
/// # 输入契约
///
/// 调用方负责保证：
/// - `universe` 已排序去重（构造时通过 [`AnalysisInput::with_universe`] 自动处理）
/// - 所有 `entry_points.symbol_id` ∈ `universe`
/// - 所有 `edges.caller_symbol_id` ∈ `universe`
/// - 所有 `virtual_dispatch_groups.impl_method_ids` ∈ `universe`
///
/// # 输出
///
/// `AnalysisReport.reference_graph` 已就绪，可直接写入 `.arcgr` 二进制。
/// `reachable_symbols` / `unreachable_symbols` 已排序（满足
/// [`ReferenceGraph::is_reachable`] 二分查找前提）。
pub fn analyze(input: &AnalysisInput) -> AnalysisReport {
    // 1. 构建 caller → [callee] 索引
    let mut edge_map: HashMap<u32, Vec<u32>> = HashMap::new();
    for edge in &input.edges {
        edge_map
            .entry(edge.caller_symbol_id)
            .or_default()
            .push(edge.callee_symbol_id);
    }

    // 2. 构建 interface_method → [impl_method] 索引（虚分派保守）
    let mut vdispatch_map: HashMap<u32, Vec<u32>> = HashMap::new();
    for group in &input.virtual_dispatch_groups {
        vdispatch_map
            .entry(group.interface_method_id)
            .or_default()
            .extend_from_slice(&group.impl_method_ids);
    }

    // 3. BFS 从入口点出发
    let mut reachable: HashSet<u32> = HashSet::new();
    let mut queue: VecDeque<u32> = VecDeque::new();

    for ep in &input.entry_points {
        if reachable.insert(ep.symbol_id) {
            queue.push_back(ep.symbol_id);
        }
    }

    while let Some(caller) = queue.pop_front() {
        // 沿引用边传播
        if let Some(callees) = edge_map.get(&caller) {
            for &callee in callees {
                if reachable.insert(callee) {
                    queue.push_back(callee);
                }
            }
        }
        // 虚分派保守：接口方法被引用时，所有实现方法保守可达
        if let Some(impls) = vdispatch_map.get(&caller) {
            for &impl_id in impls {
                if reachable.insert(impl_id) {
                    queue.push_back(impl_id);
                }
            }
        }
    }

    // 4. 划分 reachable / unreachable（仅限 universe 内符号）
    let mut reachable_symbols: Vec<u32> = input
        .universe
        .iter()
        .copied()
        .filter(|id| reachable.contains(id))
        .collect();
    // universe 已排序，filter 保序；但保险起见再排一次
    reachable_symbols.sort_unstable();

    let mut unreachable_symbols: Vec<u32> = input
        .universe
        .iter()
        .copied()
        .filter(|id| !reachable.contains(id))
        .collect();
    unreachable_symbols.sort_unstable();

    let reference_graph = ReferenceGraph {
        entry_points: input.entry_points.clone(),
        reachable_symbols,
        unreachable_symbols,
        edges: input.edges.clone(),
    };

    AnalysisReport {
        entry_point_count: input.entry_points.len(),
        reachable_count: reference_graph.reachable_symbols.len(),
        unreachable_count: reference_graph.unreachable_symbols.len(),
        edge_count: input.edges.len(),
        reference_graph,
    }
}

/// 构造边的便捷辅助函数（测试与上游收集器共用）。
pub fn make_edge(caller: u32, callee: u32, kind: EdgeKind) -> Edge {
    Edge::new(caller, callee, kind, 0, 0, 0, true)
}

/// 构造入口点的便捷辅助函数。
pub fn make_entry(symbol_id: u32, kind: EntryPointKind) -> EntryPoint {
    EntryPoint::new(symbol_id, kind, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空输入：无入口、无边、空 universe → 空输出。
    #[test]
    fn empty_input_produces_empty_graph() {
        let input = AnalysisInput::new();
        let report = analyze(&input);

        assert_eq!(report.entry_point_count, 0);
        assert_eq!(report.reachable_count, 0);
        assert_eq!(report.unreachable_count, 0);
        assert_eq!(report.edge_count, 0);
        assert!(report.reference_graph.reachable_symbols.is_empty());
        assert!(report.reference_graph.unreachable_symbols.is_empty());
        assert!(report.reference_graph.entry_points.is_empty());
        assert!(report.reference_graph.edges.is_empty());
    }

    /// 单入口无边：入口可达，其他符号不可达。
    #[test]
    fn single_entry_no_edges() {
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_universe(vec![0, 1, 2, 3]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 1);
        assert_eq!(report.unreachable_count, 3);
        assert!(report.reference_graph.is_reachable(0));
        assert!(report.reference_graph.is_unreachable(1));
        assert!(report.reference_graph.is_unreachable(2));
        assert!(report.reference_graph.is_unreachable(3));
    }

    /// 单入口 + 单链边：BFS 沿链传播。
    #[test]
    fn bfs_propagates_along_chain() {
        // main(0) → A(1) → B(2) → C(3)，D(4) 不可达
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![
                make_edge(0, 1, EdgeKind::Call),
                make_edge(1, 2, EdgeKind::Call),
                make_edge(2, 3, EdgeKind::Call),
            ])
            .with_universe(vec![0, 1, 2, 3, 4]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 4);
        assert_eq!(report.unreachable_count, 1);
        assert!(report.reference_graph.is_reachable(0));
        assert!(report.reference_graph.is_reachable(1));
        assert!(report.reference_graph.is_reachable(2));
        assert!(report.reference_graph.is_reachable(3));
        assert!(report.reference_graph.is_unreachable(4));
    }

    /// 多入口：每个入口独立 BFS，可达集合并集。
    #[test]
    fn multiple_entry_points_merge_reachable() {
        // 入口 0 → 1；入口 2 → 3；4 不可达
        let input = AnalysisInput::new()
            .with_entry_points(vec![
                make_entry(0, EntryPointKind::Main),
                make_entry(2, EntryPointKind::LibraryExport),
            ])
            .with_edges(vec![
                make_edge(0, 1, EdgeKind::Call),
                make_edge(2, 3, EdgeKind::Call),
            ])
            .with_universe(vec![0, 1, 2, 3, 4]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 4);
        assert_eq!(report.unreachable_count, 1);
        assert!(report.reference_graph.is_reachable(0));
        assert!(report.reference_graph.is_reachable(1));
        assert!(report.reference_graph.is_reachable(2));
        assert!(report.reference_graph.is_reachable(3));
        assert!(report.reference_graph.is_unreachable(4));
    }

    /// 虚分派保守：接口方法被引用时，所有实现方法保守可达。
    #[test]
    fn virtual_dispatch_conservative_propagation() {
        // main(0) → IFace.Method(1) [MethodCall 虚分派]
        // IFace.Method(1) 被实现 by ClassA.Method(2), ClassB.Method(3)
        // 实现方法 2/3 保守可达；4 是孤立方法（不可达）
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![make_edge(0, 1, EdgeKind::MethodCall)])
            .with_universe(vec![0, 1, 2, 3, 4])
            .with_virtual_dispatch_groups(vec![VirtualDispatchGroup::new(1, vec![2, 3])]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 4);
        assert_eq!(report.unreachable_count, 1);
        assert!(report.reference_graph.is_reachable(0));
        assert!(report.reference_graph.is_reachable(1));
        assert!(report.reference_graph.is_reachable(2));
        assert!(report.reference_graph.is_reachable(3));
        assert!(report.reference_graph.is_unreachable(4));
    }

    /// 虚分派保守组不触发：接口方法未被引用时，实现方法不可达。
    #[test]
    fn virtual_dispatch_group_not_triggered_when_interface_unreached() {
        // main(0) → 1；interface.method=2 的虚分派组未触发
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![make_edge(0, 1, EdgeKind::Call)])
            .with_universe(vec![0, 1, 2, 3, 4])
            .with_virtual_dispatch_groups(vec![VirtualDispatchGroup::new(2, vec![3, 4])]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 2);
        assert_eq!(report.unreachable_count, 3);
        assert!(report.reference_graph.is_reachable(0));
        assert!(report.reference_graph.is_reachable(1));
        assert!(report.reference_graph.is_unreachable(2));
        assert!(report.reference_graph.is_unreachable(3));
        assert!(report.reference_graph.is_unreachable(4));
    }

    /// 环检测：BFS 不应陷入死循环。
    #[test]
    fn cycles_do_not_cause_infinite_loop() {
        // 0 → 1 → 2 → 0 (环) + 0 → 3
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![
                make_edge(0, 1, EdgeKind::Call),
                make_edge(1, 2, EdgeKind::Call),
                make_edge(2, 0, EdgeKind::Call),
                make_edge(0, 3, EdgeKind::Call),
            ])
            .with_universe(vec![0, 1, 2, 3, 4]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 4);
        assert_eq!(report.unreachable_count, 1);
        assert!(report.reference_graph.is_reachable(0));
        assert!(report.reference_graph.is_reachable(1));
        assert!(report.reference_graph.is_reachable(2));
        assert!(report.reference_graph.is_reachable(3));
        assert!(report.reference_graph.is_unreachable(4));
    }

    /// 自环：caller → caller 不应死循环。
    #[test]
    fn self_loop_terminates() {
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![make_edge(0, 0, EdgeKind::Call)])
            .with_universe(vec![0, 1]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 1);
        assert!(report.reference_graph.is_reachable(0));
        assert!(report.reference_graph.is_unreachable(1));
    }

    /// 外部符号（不在 universe）：可达但不写入 reachable_symbols 输出。
    #[test]
    fn external_symbols_reachable_but_not_in_output() {
        // main(0) → external(999)；universe 仅含 0, 1
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![make_edge(0, 999, EdgeKind::Call)])
            .with_universe(vec![0, 1]);

        let report = analyze(&input);

        // 999 被 BFS 访问，但不在 universe 中，不写入 reachable_symbols
        assert_eq!(report.reachable_count, 1);
        assert!(report.reference_graph.is_reachable(0));
        assert!(!report.reference_graph.is_reachable(999));
        assert!(report.reference_graph.is_unreachable(1));
    }

    /// 多种 EdgeKind 混合：BFS 不区分类别，全部沿边传播。
    #[test]
    fn mixed_edge_kinds_all_propagate() {
        // 0 -Call→ 1 -MethodCall→ 2 -New→ 3 -FieldAccess→ 4 -Implement→ 5
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![
                make_edge(0, 1, EdgeKind::Call),
                make_edge(1, 2, EdgeKind::MethodCall),
                make_edge(2, 3, EdgeKind::New),
                make_edge(3, 4, EdgeKind::FieldAccess),
                make_edge(4, 5, EdgeKind::Implement),
            ])
            .with_universe(vec![0, 1, 2, 3, 4, 5, 6]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 6);
        assert_eq!(report.unreachable_count, 1);
        for id in 0..=5 {
            assert!(
                report.reference_graph.is_reachable(id),
                "symbol {} should be reachable",
                id
            );
        }
        assert!(report.reference_graph.is_unreachable(6));
    }

    /// 全部 EntryPointKind 作为入口可达。
    #[test]
    fn all_entry_point_kinds_are_reachable() {
        let input = AnalysisInput::new()
            .with_entry_points(vec![
                make_entry(0, EntryPointKind::Main),
                make_entry(1, EntryPointKind::LibraryExport),
                make_entry(2, EntryPointKind::TestFunction),
                make_entry(3, EntryPointKind::DynamicLibEntry),
                make_entry(4, EntryPointKind::FFIExport),
                make_entry(5, EntryPointKind::CGMain),
            ])
            .with_universe(vec![0, 1, 2, 3, 4, 5, 6]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 6);
        assert_eq!(report.unreachable_count, 1);
        for id in 0..=5 {
            assert!(report.reference_graph.is_reachable(id));
        }
        assert!(report.reference_graph.is_unreachable(6));
    }

    /// 重复入口 / 重复边：去重（HashSet 自动处理）。
    #[test]
    fn duplicate_entries_are_deduplicated() {
        let input = AnalysisInput::new()
            .with_entry_points(vec![
                make_entry(0, EntryPointKind::Main),
                make_entry(0, EntryPointKind::Main),
            ])
            .with_edges(vec![
                make_edge(0, 1, EdgeKind::Call),
                make_edge(0, 1, EdgeKind::Call),
            ])
            .with_universe(vec![0, 1]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 2);
        // edges 不去重（保留原始边以供 LSP Find All References 计数）
        assert_eq!(report.edge_count, 2);
    }

    /// universe 排序与去重契约：构造时自动处理。
    #[test]
    fn universe_is_sorted_and_deduplicated() {
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_universe(vec![3, 1, 2, 1, 0, 3]);

        assert_eq!(input.universe, vec![0, 1, 2, 3]);
    }

    /// 输出 reachable/unreachable 已排序（满足二分查找前提）。
    #[test]
    fn output_arrays_are_sorted() {
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![
                make_edge(0, 5, EdgeKind::Call),
                make_edge(0, 2, EdgeKind::Call),
                make_edge(0, 8, EdgeKind::Call),
            ])
            .with_universe(vec![8, 5, 2, 0, 1, 3, 4, 6, 7, 9]);

        let report = analyze(&input);

        // reachable 已排序
        let mut sorted = report.reference_graph.reachable_symbols.clone();
        sorted.sort_unstable();
        assert_eq!(report.reference_graph.reachable_symbols, sorted);

        // unreachable 已排序
        let mut sorted = report.reference_graph.unreachable_symbols.clone();
        sorted.sort_unstable();
        assert_eq!(report.reference_graph.unreachable_symbols, sorted);
    }

    /// 钻石形调用图：A→B, A→C, B→D, C→D，D 不应被重复访问。
    #[test]
    fn diamond_shaped_graph_terminates() {
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![
                make_edge(0, 1, EdgeKind::Call),
                make_edge(0, 2, EdgeKind::Call),
                make_edge(1, 3, EdgeKind::Call),
                make_edge(2, 3, EdgeKind::Call),
            ])
            .with_universe(vec![0, 1, 2, 3, 4]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 4);
        assert_eq!(report.unreachable_count, 1);
        assert!(report.reference_graph.is_reachable(3));
    }

    /// 虚分派保守级联：接口方法 A → 实现方法 B → 接口方法 C → 实现方法 D。
    #[test]
    fn virtual_dispatch_cascades_through_bfs() {
        // main(0) → IFaceA.method(1) [vdispatch] → implB(2)
        // implB(2) → IFaceC.method(3) [vdispatch] → implD(4)
        // 5 孤立
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(0, EntryPointKind::Main)])
            .with_edges(vec![
                make_edge(0, 1, EdgeKind::MethodCall),
                make_edge(2, 3, EdgeKind::MethodCall),
            ])
            .with_universe(vec![0, 1, 2, 3, 4, 5])
            .with_virtual_dispatch_groups(vec![
                VirtualDispatchGroup::new(1, vec![2]),
                VirtualDispatchGroup::new(3, vec![4]),
            ]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 5);
        assert_eq!(report.unreachable_count, 1);
        for id in 0..=4 {
            assert!(report.reference_graph.is_reachable(id));
        }
        assert!(report.reference_graph.is_unreachable(5));
    }

    /// 入口不在 universe 中：仍触发 BFS，但不出现在 reachable_symbols 输出。
    #[test]
    fn entry_point_outside_universe_still_seeds_bfs() {
        // 入口 100 不在 universe [0,1,2] 中，但 100 → 1，故 1 可达
        let input = AnalysisInput::new()
            .with_entry_points(vec![make_entry(100, EntryPointKind::Main)])
            .with_edges(vec![make_edge(100, 1, EdgeKind::Call)])
            .with_universe(vec![0, 1, 2]);

        let report = analyze(&input);

        assert_eq!(report.reachable_count, 1);
        assert!(report.reference_graph.is_reachable(1));
        assert!(report.reference_graph.is_unreachable(0));
        assert!(report.reference_graph.is_unreachable(2));
    }
}
