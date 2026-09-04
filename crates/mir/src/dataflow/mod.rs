//! MIR dataflow 分析框架（RFC 036 §2.1 / 计划文档 刀 1.2）。
//!
//! 提供 `DataflowAnalysis` trait + worklist 求解器，支持前向/后向、
//! 并集/交集 meet。NLL `BorrowAnalysis`（前向）与 `LiveVarAnalysis`（后向）
//! 均建立在此框架之上。
//!
//! **设计约束**（AGENTS.md / RFC 036）：
//! - 编译器核心禁领域逻辑；本模块仅做通用 dataflow，不含具体诊断措辞。
//! - 单文件单职责：trait + 求解器在此；具体分析在 `live_var.rs` / `borrow.rs`。
//! - 不过度工程：fact 域为有限集（`LocalId` / `LoanId`），worklist 必然终止。

use crate::types::*;
use indexmap::IndexMap;

pub mod borrow;
pub mod diagnostics;
pub mod live_var;
pub mod nll;

pub use borrow::{BorrowAnalysis, BorrowConflict, ConflictKind, Loan, LoanId, LoanKind};
pub use diagnostics::{scan_for_forbidden_terms, NllDiagnostic, NllDiagnosticCode};
pub use live_var::LiveVarAnalysis;
pub use nll::{run_nll_check, run_nll_check_module};

/// 数据流方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// 前向：IN = meet(OUT[preds])；OUT = transfer(IN)。
    Forward,
    /// 后向：OUT = meet(IN[succs])；IN = transfer(OUT)。
    Backward,
}

/// 单个基本块的 dataflow 结果。
#[derive(Clone, Debug)]
pub struct BlockResult<F> {
    /// 块入口事实（前向）/ 块出口事实（后向计算后回填的 IN）。
    pub in_fact: F,
    /// 块出口事实（前向）/ 块入口事实（后向）。
    pub out_fact: F,
}

/// dataflow 分析契约。
///
/// `Fact` 须为有限域上的可克隆/可比较集合（如 `HashSet<LocalId>`）。
/// meet 在并集语义下单调递增、在交集语义下单调递减，配合有限域保证 worklist 终止。
pub trait DataflowAnalysis {
    type Fact: Clone + PartialEq;

    /// 分析方向（前向 / 后向）。
    fn direction(&self) -> Direction;

    /// 边界事实：前向 = 函数入口；后向 = 函数出口（无后继块）。
    /// 并集 meet 下通常为空集。
    fn boundary_fact(&self) -> Self::Fact;

    /// meet 单位元：并集下为空集；交集下为全集。
    /// 用于无前驱/后继的块。
    fn meet_identity(&self) -> Self::Fact;

    /// 二元 meet：并集 = union；交集 = intersection。
    fn meet(&self, a: &Self::Fact, b: &Self::Fact) -> Self::Fact;

    /// 语句 transfer。`block` + `idx` 标识语句在 CFG 中的位置（BorrowAnalysis
    /// 据此查 gen/kill 表；LiveVar 仅依赖语句本身，忽略位置）。
    ///
    /// - 前向：`fact` = 该语句入口事实，返回出口事实。
    /// - 后向：`fact` = 该语句出口事实，返回入口事实。
    fn transfer_statement(
        &self,
        block: BlockId,
        idx: usize,
        stmt: &MirStatement,
        fact: &Self::Fact,
    ) -> Self::Fact;

    /// 终结符 transfer。语义同 `transfer_statement`，作用于 `MirTerminator`。
    fn transfer_terminator(
        &self,
        block: BlockId,
        term: &MirTerminator,
        fact: &Self::Fact,
    ) -> Self::Fact;
}

/// 计算块的后继集合（依据终结符）。
pub fn successors_of(block: BlockId, cfg: &MirCfgBody) -> Vec<BlockId> {
    match &cfg.blocks[&block].terminator {
        MirTerminator::Goto(t) => vec![*t],
        MirTerminator::CondBr {
            then_bb, else_bb, ..
        } => vec![*then_bb, *else_bb],
        MirTerminator::Return(_) | MirTerminator::Throw(_) | MirTerminator::Unreachable => vec![],
    }
}

/// 计算全 CFG 的前驱表（后继关系反转）。
pub fn compute_predecessors(cfg: &MirCfgBody) -> IndexMap<BlockId, Vec<BlockId>> {
    let mut preds: IndexMap<BlockId, Vec<BlockId>> =
        cfg.blocks.keys().map(|&id| (id, Vec::new())).collect();
    for &from in cfg.blocks.keys() {
        for to in successors_of(from, cfg) {
            if let Some(v) = preds.get_mut(&to) {
                v.push(from);
            }
        }
    }
    preds
}

/// Worklist 求解器：迭代至不动点，返回每块的 `BlockResult { in_fact, out_fact }`。
///
/// 对前向分析：`in_fact` = 块入口事实，`out_fact` = 块出口事实。
/// 对后向分析：`out_fact` = 块出口事实，`in_fact` = 块入口事实。
///
/// 终止性：有限域 + 单调 meet（并集递增 / 交集递减）。
pub fn run_worklist<A: DataflowAnalysis>(
    analysis: &A,
    cfg: &MirCfgBody,
) -> IndexMap<BlockId, BlockResult<A::Fact>> {
    let preds = compute_predecessors(cfg);
    let identity = analysis.meet_identity();
    let boundary = analysis.boundary_fact();

    let mut in_facts: IndexMap<BlockId, A::Fact> = IndexMap::new();
    let mut out_facts: IndexMap<BlockId, A::Fact> = IndexMap::new();

    // 初始化：前向 entry 的 IN = boundary，其余 = identity；OUT 全 = identity。
    // 后向 exit（无后继）的 OUT = boundary，其余 = identity；IN 全 = identity。
    for &id in cfg.blocks.keys() {
        let init_in = if matches!(analysis.direction(), Direction::Forward) && id == cfg.entry {
            boundary.clone()
        } else {
            identity.clone()
        };
        let init_out = if matches!(analysis.direction(), Direction::Backward)
            && successors_of(id, cfg).is_empty()
        {
            boundary.clone()
        } else {
            identity.clone()
        };
        in_facts.insert(id, init_in);
        out_facts.insert(id, init_out);
    }

    let mut worklist: Vec<BlockId> = cfg.blocks.keys().copied().collect();

    while let Some(id) = worklist.pop() {
        match analysis.direction() {
            Direction::Forward => {
                // IN = meet(OUT[preds])，entry 强制为 boundary。
                let new_in = if id == cfg.entry {
                    boundary.clone()
                } else {
                    preds[&id]
                        .iter()
                        .filter_map(|p| out_facts.get(p))
                        .fold(identity.clone(), |acc, f| analysis.meet(&acc, f))
                };
                // OUT = transfer over statements（顺序）+ terminator。
                let new_out = transfer_forward(analysis, id, cfg, &new_in);

                let out_changed = out_facts[&id] != new_out;
                let in_changed = in_facts[&id] != new_in;
                in_facts.insert(id, new_in);
                out_facts.insert(id, new_out);

                if out_changed {
                    for s in successors_of(id, cfg) {
                        if !worklist.contains(&s) {
                            worklist.push(s);
                        }
                    }
                } else if in_changed {
                    // OUT 未变则不传播；IN 仅作记录。
                }
            }
            Direction::Backward => {
                // OUT = meet(IN[succs])，exit（无后继）强制为 boundary。
                let new_out = if successors_of(id, cfg).is_empty() {
                    boundary.clone()
                } else {
                    successors_of(id, cfg)
                        .iter()
                        .filter_map(|s| in_facts.get(s))
                        .fold(identity.clone(), |acc, f| analysis.meet(&acc, f))
                };
                // IN = transfer over terminator + statements（逆序）。
                let new_in = transfer_backward(analysis, id, cfg, &new_out);

                let in_changed = in_facts[&id] != new_in;
                let out_changed = out_facts[&id] != new_out;
                in_facts.insert(id, new_in);
                out_facts.insert(id, new_out);

                if in_changed {
                    for &p in &preds[&id] {
                        if !worklist.contains(&p) {
                            worklist.push(p);
                        }
                    }
                } else if out_changed {
                    // IN 未变则不传播；OUT 仅作记录。
                }
            }
        }
    }

    cfg.blocks
        .keys()
        .map(|&id| {
            (
                id,
                BlockResult {
                    in_fact: in_facts[&id].clone(),
                    out_fact: out_facts[&id].clone(),
                },
            )
        })
        .collect()
}

fn transfer_forward<A: DataflowAnalysis>(
    analysis: &A,
    block: BlockId,
    cfg: &MirCfgBody,
    in_fact: &A::Fact,
) -> A::Fact {
    let blk = &cfg.blocks[&block];
    let mut f = in_fact.clone();
    for (i, stmt) in blk.statements.iter().enumerate() {
        f = analysis.transfer_statement(block, i, stmt, &f);
    }
    f = analysis.transfer_terminator(block, &blk.terminator, &f);
    f
}

fn transfer_backward<A: DataflowAnalysis>(
    analysis: &A,
    block: BlockId,
    cfg: &MirCfgBody,
    out_fact: &A::Fact,
) -> A::Fact {
    let blk = &cfg.blocks[&block];
    let mut f = out_fact.clone();
    // 后向：先处理终结符（块的「出口」），再逆序处理语句。
    f = analysis.transfer_terminator(block, &blk.terminator, &f);
    for (i, stmt) in blk.statements.iter().enumerate().rev() {
        f = analysis.transfer_statement(block, i, stmt, &f);
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 简易前向「并集」分析：Fact = HashSet<u32>，gen 给定，无 kill。
    /// 用于验证 worklist 在菱形 CFG 上的并集传播。
    struct ConstForward {
        gen: IndexMap<BlockId, std::collections::HashSet<u32>>,
    }

    impl DataflowAnalysis for ConstForward {
        type Fact = std::collections::HashSet<u32>;
        fn direction(&self) -> Direction {
            Direction::Forward
        }
        fn boundary_fact(&self) -> Self::Fact {
            std::collections::HashSet::new()
        }
        fn meet_identity(&self) -> Self::Fact {
            std::collections::HashSet::new()
        }
        fn meet(&self, a: &Self::Fact, b: &Self::Fact) -> Self::Fact {
            a.union(b).copied().collect()
        }
        fn transfer_statement(
            &self,
            _block: BlockId,
            _idx: usize,
            _stmt: &MirStatement,
            fact: &Self::Fact,
        ) -> Self::Fact {
            fact.clone()
        }
        fn transfer_terminator(
            &self,
            block: BlockId,
            _term: &MirTerminator,
            fact: &Self::Fact,
        ) -> Self::Fact {
            let mut f = fact.clone();
            if let Some(g) = self.gen.get(&block) {
                f.extend(g.iter().copied());
            }
            f
        }
    }

    /// 菱形 CFG：entry --CondBr--> then / else --> merge --> exit。
    fn build_diamond_cfg() -> MirCfgBody {
        let mut blocks = IndexMap::new();
        let entry = BlockId(0);
        let then = BlockId(1);
        let els = BlockId(2);
        let merge = BlockId(3);
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: vec![],
                terminator: MirTerminator::CondBr {
                    cond: MirOperand::ConstBool(true),
                    then_bb: then,
                    else_bb: els,
                },
            },
        );
        blocks.insert(
            then,
            MirBlock {
                id: then,
                statements: vec![],
                terminator: MirTerminator::Goto(merge),
            },
        );
        blocks.insert(
            els,
            MirBlock {
                id: els,
                statements: vec![],
                terminator: MirTerminator::Goto(merge),
            },
        );
        blocks.insert(
            merge,
            MirBlock {
                id: merge,
                statements: vec![],
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: typeck::TypeId::Void,
            param_count: 0,
            locals: IndexMap::new(),
            entry,
            blocks,
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: Linkage::External,
            parallelize: false,
            loop_backedges: Default::default(),
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        }
    }

    /// 验证 worklist 在菱形 CFG 上的前向并集传播：
    /// entry gen {1}，then gen {2}，else gen {3} → merge IN = {1,2,3}。
    #[test]
    fn worklist_forward_union_diamond() {
        use std::collections::HashSet;
        let cfg = build_diamond_cfg();
        let mut gen = IndexMap::new();
        gen.insert(BlockId(0), HashSet::from([1u32]));
        gen.insert(BlockId(1), HashSet::from([2u32]));
        gen.insert(BlockId(2), HashSet::from([3u32]));
        let analysis = ConstForward { gen };
        let result = run_worklist(&analysis, &cfg);
        let merge_in = &result[&BlockId(3)].in_fact;
        assert!(
            merge_in.contains(&1) && merge_in.contains(&2) && merge_in.contains(&3),
            "merge IN must aggregate gen from all predecessors, got {:?}",
            merge_in
        );
    }
}
