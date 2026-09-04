//! 活跃变量分析（后向 dataflow）。
//!
//! 为 NLL `BorrowAnalysis` 提供「last use」信息：某 local 的最后使用点即
//! 其上 loan 的 kill 点（近似；精确化在 S4）。
//!
//! - gen = 语句中**使用**的 local（读）
//! - kill = 语句中**定义**的 local（写）
//! - 后向、并集 meet：`IN = (OUT − kill) ∪ gen`
//! - 边界（函数出口）= 空集

use std::collections::HashSet;

use crate::dataflow::{DataflowAnalysis, Direction};
use crate::types::*;

/// 活跃变量分析。Fact = `HashSet<LocalId>`。
pub struct LiveVarAnalysis;

impl DataflowAnalysis for LiveVarAnalysis {
    type Fact = HashSet<LocalId>;

    fn direction(&self) -> Direction {
        Direction::Backward
    }

    fn boundary_fact(&self) -> Self::Fact {
        HashSet::new()
    }

    fn meet_identity(&self) -> Self::Fact {
        HashSet::new()
    }

    fn meet(&self, a: &Self::Fact, b: &Self::Fact) -> Self::Fact {
        a.union(b).copied().collect()
    }

    fn transfer_statement(
        &self,
        _block: BlockId,
        _idx: usize,
        stmt: &MirStatement,
        out_fact: &Self::Fact,
    ) -> Self::Fact {
        // IN = (OUT − kill) ∪ gen
        let mut fact = out_fact.clone();
        for d in stmt_defs(stmt) {
            fact.remove(&d);
        }
        for u in stmt_uses(stmt) {
            fact.insert(u);
        }
        fact
    }

    fn transfer_terminator(
        &self,
        _block: BlockId,
        term: &MirTerminator,
        out_fact: &Self::Fact,
    ) -> Self::Fact {
        let mut fact = out_fact.clone();
        for u in terminator_uses(term) {
            fact.insert(u);
        }
        fact
    }
}

/// 提取 operand 中引用的所有 local（读）。
pub fn operand_locals(op: &MirOperand) -> Vec<LocalId> {
    match op {
        MirOperand::Local(l) => vec![*l],
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. }
        | MirOperand::UnboxString { object }
        | MirOperand::UnboxGeneric { object, .. } => operand_locals(object),
        MirOperand::AddrOf(l) => vec![*l],
        MirOperand::Closure { env, .. } => {
            env.iter().flat_map(|(_, o)| operand_locals(o)).collect()
        }
        MirOperand::ConstInt(_)
        | MirOperand::ConstFloat(_)
        | MirOperand::ConstString(_)
        | MirOperand::ConstBool(_)
        | MirOperand::ConstNull
        | MirOperand::ConstDefault { .. }
        | MirOperand::FnPtr { .. }
        | MirOperand::TypeId { .. }
        | MirOperand::TypeInfoPtr { .. }
        | MirOperand::StaticField { .. } => Vec::new(),
    }
}

/// 提取 rvalue 中读取的所有 local。
pub fn rvalue_locals(rv: &MirRvalue) -> Vec<LocalId> {
    let mut v: Vec<LocalId> = Vec::new();
    match rv {
        MirRvalue::Use(op) => v.extend(operand_locals(op)),
        MirRvalue::Binary { left, right, .. } => {
            v.extend(operand_locals(left));
            v.extend(operand_locals(right));
        }
        MirRvalue::Call { args, .. } | MirRvalue::New { args, .. } => {
            v.extend(args.iter().flat_map(operand_locals));
        }
        MirRvalue::FieldGet { object, .. } => v.extend(operand_locals(object)),
        MirRvalue::MethodCall { receiver, args, .. } => {
            v.extend(operand_locals(receiver));
            v.extend(args.iter().flat_map(operand_locals));
        }
        MirRvalue::MakeIface { object, .. }
        | MirRvalue::MakeIfaceDyn { object, .. }
        | MirRvalue::AdaptIface { object, .. } => v.extend(operand_locals(object)),
        MirRvalue::StructLit { fields, .. } => {
            v.extend(fields.iter().flat_map(|(_, o)| operand_locals(o)));
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for e in elements {
                match e {
                    ArrayLitElement::Value(rv) => v.extend(rvalue_locals(rv)),
                    ArrayLitElement::Spread(op) => v.extend(operand_locals(op)),
                }
            }
        }
        MirRvalue::IndexGet { array, index, .. } => {
            v.extend(operand_locals(array));
            v.extend(operand_locals(index));
        }
        MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            v.extend(operand_locals(array));
            if let Some(s) = start {
                v.extend(operand_locals(s));
            }
            if let Some(l) = length {
                v.extend(operand_locals(l));
            }
        }
        MirRvalue::SpanFromStack { elements, .. } => {
            v.extend(elements.iter().flat_map(operand_locals));
        }
        MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            v.extend(operand_locals(span));
            v.extend(operand_locals(start));
            if let Some(l) = length {
                v.extend(operand_locals(l));
            }
        }
        MirRvalue::SpanFill { span, value, .. } => {
            v.extend(operand_locals(span));
            v.extend(operand_locals(value));
        }
        MirRvalue::SpanClear { span, .. } => v.extend(operand_locals(span)),
        MirRvalue::SpanCopyTo { src, dest, .. } | MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            v.extend(operand_locals(src));
            v.extend(operand_locals(dest));
        }
        MirRvalue::SpanToArray { span, .. } => v.extend(operand_locals(span)),
        MirRvalue::SoaFieldGet { array, index, .. } => {
            v.extend(operand_locals(array));
            v.extend(operand_locals(index));
        }
        MirRvalue::LinqChain(chain) => {
            v.extend(operand_locals(&chain.source));
            // LambdaExpr 体属 AST，不引用 MIR local；忽略 operators。
        }
        MirRvalue::ExpressionTreeConst { .. } | MirRvalue::FnPtr { .. } => {}
        MirRvalue::IndirectCall { func, args } => {
            v.extend(operand_locals(func));
            v.extend(args.iter().flat_map(operand_locals));
        }
        MirRvalue::Coalesce { left, right } => {
            v.extend(operand_locals(left));
            v.extend(operand_locals(right));
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            v.extend(operand_locals(cond));
            v.extend(operand_locals(then_val));
            v.extend(operand_locals(else_val));
        }
        MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            v.extend(operand_locals(receiver));
            v.extend(operand_locals(default));
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            v.extend(operand_locals(receiver));
            v.extend(args.iter().flat_map(operand_locals));
            v.extend(operand_locals(default));
        }
        MirRvalue::ForceDerefField { receiver, .. } => v.extend(operand_locals(receiver)),
        MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            v.extend(operand_locals(receiver));
            v.extend(args.iter().flat_map(operand_locals));
        }
        MirRvalue::Box { src, .. } | MirRvalue::Unbox { src, .. } => v.extend(operand_locals(src)),
        MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(p) = payload {
                v.extend(operand_locals(p));
            }
        }
        MirRvalue::VariantTag { scrutinee, .. } | MirRvalue::VariantExtract { scrutinee, .. } => {
            v.extend(operand_locals(scrutinee))
        }
        MirRvalue::NewArray { length, .. } => v.extend(operand_locals(length)),
    }
    v
}

/// 语句中**使用**的 local（gen）。
pub fn stmt_uses(stmt: &MirStatement) -> Vec<LocalId> {
    let mut v: Vec<LocalId> = Vec::new();
    match stmt {
        MirStatement::Assign { rvalue, .. } => v.extend(rvalue_locals(rvalue)),
        MirStatement::Drop(l) => v.push(*l),
        MirStatement::Return(Some(rv)) => v.extend(rvalue_locals(rv)),
        MirStatement::Return(None) => {}
        MirStatement::FieldSet { object, value, .. } => {
            v.extend(operand_locals(object));
            v.extend(rvalue_locals(value));
        }
        MirStatement::StaticFieldSet { value, .. } => v.extend(rvalue_locals(value)),
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            v.extend(operand_locals(array));
            v.extend(operand_locals(index));
            v.extend(rvalue_locals(value));
        }
        MirStatement::Await { task, .. } => v.extend(rvalue_locals(task)),
        MirStatement::Throw { value } => v.extend(rvalue_locals(value)),
        // region 语句：保守地把嵌套体的 uses 收集到当前点（可能过近似，但安全）。
        MirStatement::LinqForeach { chain, body, .. } => {
            v.extend(operand_locals(&chain.source));
            for s in body {
                v.extend(stmt_uses(s));
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                v.extend(stmt_uses(s));
            }
            for s in catch_body {
                v.extend(stmt_uses(s));
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                v.extend(stmt_uses(s));
            }
            for s in finally {
                v.extend(stmt_uses(s));
            }
        }
        // If/While/Break/Continue 不应出现在 MirCfgBody 顶层（to_cfg 已展平）。
        MirStatement::If { .. }
        | MirStatement::While { .. }
        | MirStatement::Break
        | MirStatement::Continue => {}
    }
    v
}

/// 语句中**定义**的 local（kill）。
pub fn stmt_defs(stmt: &MirStatement) -> Vec<LocalId> {
    let mut v: Vec<LocalId> = Vec::new();
    match stmt {
        MirStatement::Assign { place, .. } => v.push(*place),
        MirStatement::Await { place, .. } => v.push(*place),
        // region 语句：收集嵌套体的 defs（保守）。
        MirStatement::LinqForeach { body, .. } => {
            for s in body {
                v.extend(stmt_defs(s));
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                v.extend(stmt_defs(s));
            }
            for s in catch_body {
                v.extend(stmt_defs(s));
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                v.extend(stmt_defs(s));
            }
            for s in finally {
                v.extend(stmt_defs(s));
            }
        }
        _ => {}
    }
    v
}

/// 终结符中使用的 local（gen；无 def）。
pub fn terminator_uses(term: &MirTerminator) -> Vec<LocalId> {
    match term {
        MirTerminator::CondBr { cond, .. } => operand_locals(cond),
        MirTerminator::Return(Some(op)) => operand_locals(op),
        MirTerminator::Throw(op) => operand_locals(op),
        MirTerminator::Return(None) | MirTerminator::Goto(_) | MirTerminator::Unreachable => {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataflow::run_worklist;
    use ast::BinOp;
    use indexmap::IndexMap;

    fn local_cfg(stmts: Vec<MirStatement>, locals: Vec<LocalId>) -> MirCfgBody {
        let entry = BlockId(0);
        let mut blocks = IndexMap::new();
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: stmts,
                terminator: MirTerminator::Return(None),
            },
        );
        let mut locals_map = IndexMap::new();
        for l in locals {
            locals_map.insert(l, ("_".into(), typeck::TypeId::Void));
        }
        MirCfgBody {
            params: vec![],
            ret: typeck::TypeId::Void,
            param_count: 0,
            locals: locals_map,
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

    /// 直链：`L1 = 1; L2 = L1; return` → L2 在 return 前活跃，L1 在第二句后死亡。
    #[test]
    fn live_var_straight_line() {
        let l1 = LocalId(1);
        let l2 = LocalId(2);
        let stmts = vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
            },
            MirStatement::Assign {
                place: l2,
                rvalue: MirRvalue::Use(MirOperand::Local(l1)),
            },
        ];
        let cfg = local_cfg(stmts, vec![l1, l2]);
        let result = run_worklist(&LiveVarAnalysis, &cfg);
        let entry_in = &result[&cfg.entry].in_fact;
        // 第一句前：L2 未定义不活跃；L1 未定义不活跃 → 入口应为空。
        // 但第二句使用 L1，故 L1 在第一句后活跃，在第一句前不活跃（第一句定义 L1）。
        // 入口（块 IN）= 第一句前的 fact = 空（L1 由第一句定义，此前无使用）。
        assert!(
            !entry_in.contains(&l1),
            "L1 not live at entry (defined before any use), got {:?}",
            entry_in
        );
        assert!(!entry_in.contains(&l2));
    }

    /// `L2 = L1 + L1; return L2` → L1 在加法后死亡，L2 活跃到 return。
    #[test]
    fn live_var_use_then_die() {
        let l1 = LocalId(1);
        let l2 = LocalId(2);
        let stmts = vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
            },
            MirStatement::Assign {
                place: l2,
                rvalue: MirRvalue::Binary {
                    op: BinOp::Add,
                    left: MirOperand::Local(l1),
                    right: MirOperand::Local(l1),
                },
            },
            MirStatement::Return(Some(MirRvalue::Use(MirOperand::Local(l2)))),
        ];
        let cfg = local_cfg(stmts, vec![l1, l2]);
        let result = run_worklist(&LiveVarAnalysis, &cfg);
        let entry_in = &result[&cfg.entry].in_fact;
        assert!(!entry_in.contains(&l1), "L1 not live at entry");
        assert!(!entry_in.contains(&l2), "L2 not live at entry");
    }

    /// if-else：两条分支各自使用不同 local，merge 前都应被 kill 掉。
    #[test]
    fn live_var_if_else_merge() {
        // entry: cond = true; CondBr → then/else
        // then:  L3 = 1; goto merge
        // else:  L4 = 2; goto merge
        // merge: return
        let l3 = LocalId(3);
        let l4 = LocalId(4);
        let entry = BlockId(0);
        let then = BlockId(1);
        let els = BlockId(2);
        let merge = BlockId(3);
        let mut blocks = IndexMap::new();
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
                statements: vec![MirStatement::Assign {
                    place: l3,
                    rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
                }],
                terminator: MirTerminator::Goto(merge),
            },
        );
        blocks.insert(
            els,
            MirBlock {
                id: els,
                statements: vec![MirStatement::Assign {
                    place: l4,
                    rvalue: MirRvalue::Use(MirOperand::ConstInt(2)),
                }],
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
        let cfg = MirCfgBody {
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
        };
        let result = run_worklist(&LiveVarAnalysis, &cfg);
        // merge 入口：then/else 都不返回活跃变量（L3/L4 在各自分支内定义后未使用）。
        let merge_in = &result[&merge].in_fact;
        assert!(!merge_in.contains(&l3) && !merge_in.contains(&l4));
    }

    /// while 循环：循环体内使用的变量在 header 入口活跃（backedge 携带）。
    #[test]
    fn live_var_loop_back_edge() {
        // entry → header (CondBr cond=L0) → body / exit
        // body: L1 = L0 + 1; goto header
        // L0 在 body 中被使用，故 header 入口 L0 活跃。
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let entry = BlockId(0);
        let header = BlockId(1);
        let body = BlockId(2);
        let exit = BlockId(3);
        let mut blocks = IndexMap::new();
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: vec![MirStatement::Assign {
                    place: l0,
                    rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
                }],
                terminator: MirTerminator::Goto(header),
            },
        );
        blocks.insert(
            header,
            MirBlock {
                id: header,
                statements: vec![],
                terminator: MirTerminator::CondBr {
                    cond: MirOperand::Local(l0),
                    then_bb: body,
                    else_bb: exit,
                },
            },
        );
        blocks.insert(
            body,
            MirBlock {
                id: body,
                statements: vec![MirStatement::Assign {
                    place: l1,
                    rvalue: MirRvalue::Binary {
                        op: BinOp::Add,
                        left: MirOperand::Local(l0),
                        right: MirOperand::ConstInt(1),
                    },
                }],
                terminator: MirTerminator::Goto(header),
            },
        );
        blocks.insert(
            exit,
            MirBlock {
                id: exit,
                statements: vec![],
                terminator: MirTerminator::Return(None),
            },
        );
        let cfg = MirCfgBody {
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
        };
        let result = run_worklist(&LiveVarAnalysis, &cfg);
        let header_in = &result[&header].in_fact;
        assert!(
            header_in.contains(&l0),
            "L0 live at loop header (used in body + cond), got {:?}",
            header_in
        );
    }
}
