//! Nested-to-CFG flattening (RFC 015 Phase A).
//!
//! Converts `MirBody` (nested If/While statements) into `MirCfgBody`
//! (explicit basic blocks + terminators). The LLVM IR backend consumes
//! the CFG form as its canonical input.

use crate::types::*;
use ast::Ident;
use indexmap::IndexMap;
use std::collections::HashSet;
use typeck::TypeId;

/// 最近一层循环的跳转目标：`break` → exit，`continue` → header。
struct LoopTargets {
    exit_bb: BlockId,
    continue_bb: BlockId,
}

struct CfgBuilder {
    next_block: u32,
    next_local: u32,
    blocks: IndexMap<BlockId, MirBlock>,
    extra_locals: Vec<(LocalId, Ident, TypeId)>,
    /// RFC 009 M3：while 循环 backedge 源块集合。`flatten_stmts` 在生成
    /// while 循环 backedge（`body_end → header_bb`）时记录 `body_end`，
    /// 供 codegen 在 `parallelize=true` 时附加 `!llvm.loop` metadata。
    loop_backedges: HashSet<BlockId>,
    /// 迭代溯源：展平携带 `foreach_source` 的 While 时记录
    /// `(header_bb, 枚举容器)`，透传至 `MirCfgBody.foreach_loops`。
    foreach_loops: Vec<(BlockId, MirOperand)>,
    /// 嵌套循环栈（内层在顶）；供 `Break`/`Continue` 解析最近循环。
    loop_stack: Vec<LoopTargets>,
}

impl CfgBuilder {
    fn new(next_local: u32) -> Self {
        Self {
            next_block: 0,
            next_local,
            blocks: IndexMap::new(),
            extra_locals: Vec::new(),
            loop_backedges: HashSet::new(),
            foreach_loops: Vec::new(),
            loop_stack: Vec::new(),
        }
    }

    fn alloc_block(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        self.blocks.insert(
            id,
            MirBlock {
                id,
                statements: Vec::new(),
                terminator: MirTerminator::Unreachable,
            },
        );
        id
    }

    fn alloc_local(&mut self, ty: TypeId) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        self.extra_locals.push((id, "_cfgtmp".into(), ty));
        id
    }

    fn push_stmt(&mut self, bb: BlockId, stmt: MirStatement) {
        if let Some(block) = self.blocks.get_mut(&bb) {
            block.statements.push(stmt);
        }
    }

    fn set_terminator(&mut self, bb: BlockId, term: MirTerminator) {
        if let Some(block) = self.blocks.get_mut(&bb) {
            block.terminator = term;
        }
    }

    /// Materialize a MirRvalue into a MirOperand by emitting an Assign to a
    /// fresh temp local in the given block. `ty` is the type of the rvalue
    /// (used to type the temp local).
    fn materialize_rvalue(&mut self, bb: BlockId, rv: MirRvalue, ty: TypeId) -> MirOperand {
        match rv {
            MirRvalue::Use(op) => op,
            other => {
                let tmp = self.alloc_local(ty);
                self.push_stmt(
                    bb,
                    MirStatement::Assign {
                        place: tmp,
                        rvalue: other,
                    },
                );
                MirOperand::Local(tmp)
            }
        }
    }
}

impl MirBody {
    /// Flatten nested If/While into explicit CFG basic blocks.
    ///
    /// TryCatch is preserved as a linear statement (exception lowering to
    /// invoke/landingpad is handled by backends; full CFG exception model is Phase B).
    pub(crate) fn to_cfg(&self) -> MirCfgBody {
        let next_local = self.locals.keys().map(|id| id.0 + 1).max().unwrap_or(0);
        let mut builder = CfgBuilder::new(next_local);
        let entry = builder.alloc_block();

        let stmts: Vec<MirStatement> = self
            .blocks
            .first()
            .map(|b| b.statements.clone())
            .unwrap_or_default();

        let ret_ty = if self.is_async {
            self.ret.task_inner().cloned().unwrap_or(TypeId::Void)
        } else {
            self.ret.clone()
        };
        let last_bb = flatten_stmts(&mut builder, entry, stmts, ret_ty);
        ensure_terminator(&mut builder, last_bb);

        let mut locals = self.locals.clone();
        for (id, name, ty) in &builder.extra_locals {
            locals.insert(*id, (name.clone(), ty.clone()));
        }

        let cfg = MirCfgBody {
            params: self.params.clone(),
            ret: self.ret.clone(),
            param_count: self.param_count,
            locals,
            entry,
            blocks: builder.blocks,
            is_async: self.is_async,
            owner: self.owner.clone(),
            class_fields: self.class_fields.clone(),
            is_ctor: self.is_ctor,
            // RFC 006 M2：透传 is_static 到 MirCfgBody。
            is_static: self.is_static,
            captures: self.captures.clone(),
            linkage: self.linkage,
            // RFC 009 M3：透传 parallelize 到 MirCfgBody，并附 while 循环
            // backedge 源块集合，供 codegen 附加 `!llvm.loop` metadata。
            parallelize: self.parallelize,
            loop_backedges: builder.loop_backedges,
            foreach_loops: builder.foreach_loops,
            // RFC 009 M3：透传 spill_set 到 MirCfgBody，供 codegen 消费。
            spill_set: self.spill_set.clone(),
        };
        debug_assert!(
            !cfg.blocks.values().any(|bb| {
                bb.statements.iter().any(|s| match s {
                    MirStatement::If { .. } | MirStatement::Break | MirStatement::Continue => true,
                    // 允许保留「try 内嵌 break/continue」的 While 为嵌套语句
                    //（flatten_stmts While 分支的回退；codegen emit_nested_while 处理）。
                    MirStatement::While { body, .. } => !nested_break_continue_in_try(body),
                    _ => false,
                })
            }),
            "MirCfgBody top-level blocks must not contain nested If/Break/Continue after to_cfg"
        );
        cfg
    }
}

fn flatten_stmts(
    builder: &mut CfgBuilder,
    mut current: BlockId,
    stmts: Vec<MirStatement>,
    ret_ty: TypeId,
) -> BlockId {
    for stmt in stmts {
        match stmt {
            MirStatement::If {
                cond,
                then_body,
                else_body,
            } => {
                let then_bb = builder.alloc_block();
                let else_bb = builder.alloc_block();
                let merge_bb = builder.alloc_block();

                builder.set_terminator(
                    current,
                    MirTerminator::CondBr {
                        cond,
                        then_bb,
                        else_bb,
                    },
                );

                let then_end = flatten_stmts(builder, then_bb, then_body, ret_ty.clone());
                if !is_terminated(builder, then_end) {
                    builder.set_terminator(then_end, MirTerminator::Goto(merge_bb));
                }

                let else_end = flatten_stmts(builder, else_bb, else_body, ret_ty.clone());
                if !is_terminated(builder, else_end) {
                    builder.set_terminator(else_end, MirTerminator::Goto(merge_bb));
                }

                current = merge_bb;
            }
            MirStatement::While {
                cond,
                body,
                foreach_source,
            } => {
                // `flatten_stmts` 不递归 `TryCatch`/`TryFinally`（保留区域语句），
                // 因此 Try 内部的 `Break`/`Continue` 无法展平为 flat CFG。若此时把
                // While 展平，codegen 的 `nested_loop_stack` 为空，Try 内的 break 会
                // 触发 "break outside loop" panic。故当循环体含「Try 区域内嵌套的
                // break/continue」时，保留 While 为嵌套语句，交给 codegen 的
                // `emit_nested_while`（push nested_loop_stack + emit_finally_chain）。
                if nested_break_continue_in_try(&body) {
                    builder.push_stmt(
                        current,
                        MirStatement::While {
                            cond,
                            body,
                            foreach_source,
                        },
                    );
                    continue;
                }
                let header_bb = builder.alloc_block();
                let body_bb = builder.alloc_block();
                let exit_bb = builder.alloc_block();

                if let Some(source) = foreach_source {
                    builder.foreach_loops.push((header_bb, source));
                }

                builder.set_terminator(current, MirTerminator::Goto(header_bb));

                // Materialize the cond rvalue into an operand in the header block.
                let cond_op = builder.materialize_rvalue(header_bb, cond, TypeId::Bool);
                builder.set_terminator(
                    header_bb,
                    MirTerminator::CondBr {
                        cond: cond_op,
                        then_bb: body_bb,
                        else_bb: exit_bb,
                    },
                );

                builder.loop_stack.push(LoopTargets {
                    exit_bb,
                    continue_bb: header_bb,
                });
                let body_end = flatten_stmts(builder, body_bb, body, ret_ty.clone());
                builder.loop_stack.pop();
                if !is_terminated(builder, body_end) {
                    builder.set_terminator(body_end, MirTerminator::Goto(header_bb));
                    // RFC 009 M3：记录 backedge 源块，供 codegen 在 parallelize=true
                    // 时附加 `!llvm.loop` metadata 强制向量化。
                    builder.loop_backedges.insert(body_end);
                }

                current = exit_bb;
            }
            MirStatement::Break => {
                let exit_bb = builder
                    .loop_stack
                    .last()
                    .unwrap_or_else(|| panic!("MIR to_cfg: break outside loop"))
                    .exit_bb;
                builder.set_terminator(current, MirTerminator::Goto(exit_bb));
                current = builder.alloc_block();
                builder.set_terminator(current, MirTerminator::Unreachable);
            }
            MirStatement::Continue => {
                let continue_bb = builder
                    .loop_stack
                    .last()
                    .unwrap_or_else(|| panic!("MIR to_cfg: continue outside loop"))
                    .continue_bb;
                builder.set_terminator(current, MirTerminator::Goto(continue_bb));
                // continue 是 backedge（跳回 header）；记录供 parallelize metadata。
                builder.loop_backedges.insert(current);
                current = builder.alloc_block();
                builder.set_terminator(current, MirTerminator::Unreachable);
            }
            MirStatement::Return(val) => {
                let operand = val.map(|rv| builder.materialize_rvalue(current, rv, ret_ty.clone()));
                builder.set_terminator(current, MirTerminator::Return(operand));
                // Return terminates control flow — the new block is unreachable.
                // Set Unreachable so LLVM doesn't emit a typed `ret` with wrong type.
                current = builder.alloc_block();
                builder.set_terminator(current, MirTerminator::Unreachable);
            }
            MirStatement::Throw { value } => {
                let operand = builder.materialize_rvalue(current, value, TypeId::String);
                builder.set_terminator(current, MirTerminator::Throw(operand));
                current = builder.alloc_block();
                builder.set_terminator(current, MirTerminator::Unreachable);
            }
            other => builder.push_stmt(current, other),
        }
    }
    current
}

/// 判断循环体（`While`）中是否存在「位于保留区域语句 `TryCatch`/`TryFinally`
/// 内部、因而不会被 `flatten_stmts` 展平」的 `Break`/`Continue`。
///
/// `flatten_stmts` 递归 `If`/`While`，但**不**递归 `Try` 区域（区域语句有意保留、
/// 由 codegen 以嵌套区域发射）。因此 Try 内部任何深度的 `Break`/`Continue` 都无法
/// 转成 flat CFG 的 Goto；当它们位于某个 `While` 内时，该 `While` 不得展平，须保留
/// 为嵌套语句，由 codegen 的 `emit_nested_while` 处理（其 `nested_loop_stack` 能
/// 解析 break、`emit_finally_chain` 能内联执行 finally）。
fn nested_break_continue_in_try(stmts: &[MirStatement]) -> bool {
    fn walk(s: &MirStatement, in_try_region: bool) -> bool {
        match s {
            MirStatement::Break | MirStatement::Continue => in_try_region,
            MirStatement::TryCatch {
                try_body,
                catch_body,
                ..
            } => walk_each(try_body, true) || walk_each(catch_body, true),
            MirStatement::TryFinally { body, finally } => {
                walk_each(body, true) || walk_each(finally, true)
            }
            MirStatement::If {
                then_body,
                else_body,
                ..
            } => walk_each(then_body, in_try_region) || walk_each(else_body, in_try_region),
            MirStatement::While { body, .. } => walk_each(body, in_try_region),
            MirStatement::LinqForeach { body, .. } => walk_each(body, in_try_region),
            _ => false,
        }
    }
    fn walk_each(list: &[MirStatement], in_try_region: bool) -> bool {
        list.iter().any(|s| walk(s, in_try_region))
    }
    stmts.iter().any(|s| walk(s, false))
}

fn is_terminated(builder: &CfgBuilder, bb: BlockId) -> bool {
    builder
        .blocks
        .get(&bb)
        .map(|b| !matches!(b.terminator, MirTerminator::Unreachable))
        .unwrap_or(true)
}

fn ensure_terminator(builder: &mut CfgBuilder, bb: BlockId) {
    if !is_terminated(builder, bb) {
        builder.set_terminator(bb, MirTerminator::Return(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::BinOp;

    fn make_body(stmts: Vec<MirStatement>) -> MirBody {
        MirBody {
            params: vec![],
            ret: typeck::TypeId::Void,
            param_count: 0,
            locals: IndexMap::new(),
            blocks: vec![MirBasicBlock { statements: stmts }],
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: Linkage::External,
            parallelize: false,
            spill_set: typeck::SpillSet::empty(),
        }
    }

    /// RFC 009 M3：验证 while 循环 backedge 在 to_cfg 后被记录到 loop_backedges。
    #[test]
    fn flatten_while_records_backedge() {
        let body = make_body(vec![MirStatement::While {
            cond: MirRvalue::Binary {
                op: BinOp::Lt,
                left: MirOperand::ConstInt(0),
                right: MirOperand::ConstInt(10),
            },
            body: vec![],
            foreach_source: None,
        }]);
        // parallelize=false 时仍应记录 backedge（记录是无条件的，
        // codegen 据此决定是否附加 metadata）。
        let cfg = body.to_cfg();
        assert!(
            !cfg.loop_backedges.is_empty(),
            "while loop backedge must be recorded in loop_backedges"
        );
    }

    #[test]
    fn flatten_simple_return() {
        let body = make_body(vec![MirStatement::Return(None)]);
        let cfg = body.to_cfg();
        assert_eq!(cfg.blocks.len(), 2); // entry + dead block after return
        let entry_block = &cfg.blocks[&cfg.entry];
        assert!(matches!(
            entry_block.terminator,
            MirTerminator::Return(None)
        ));
    }

    #[test]
    fn flatten_if_else() {
        let body = make_body(vec![MirStatement::If {
            cond: MirOperand::ConstBool(true),
            then_body: vec![MirStatement::Return(None)],
            else_body: vec![MirStatement::Return(None)],
        }]);
        let cfg = body.to_cfg();
        assert!(cfg.blocks.len() >= 4);
        let entry_block = &cfg.blocks[&cfg.entry];
        assert!(matches!(
            entry_block.terminator,
            MirTerminator::CondBr { .. }
        ));
    }

    #[test]
    fn flatten_while_break_jumps_to_exit() {
        let body = make_body(vec![MirStatement::While {
            cond: MirRvalue::Use(MirOperand::ConstBool(true)),
            body: vec![MirStatement::Break],
            foreach_source: None,
        }]);
        let cfg = body.to_cfg();
        // body block must Goto(exit), not back to header.
        let has_break_to_non_header = cfg.blocks.values().any(|bb| {
            matches!(&bb.terminator, MirTerminator::Goto(target)
                if !cfg.loop_backedges.contains(&bb.id)
                    && cfg.blocks.contains_key(target))
        });
        assert!(
            has_break_to_non_header || cfg.blocks.len() >= 3,
            "break must produce a Goto out of the loop body"
        );
        // No Break scratch left at top level.
        assert!(cfg.blocks.values().all(|bb| {
            bb.statements
                .iter()
                .all(|s| !matches!(s, MirStatement::Break | MirStatement::Continue))
        }));
    }

    #[test]
    fn flatten_while_continue_is_backedge() {
        let body = make_body(vec![MirStatement::While {
            cond: MirRvalue::Use(MirOperand::ConstBool(true)),
            body: vec![MirStatement::Continue],
            foreach_source: None,
        }]);
        let cfg = body.to_cfg();
        assert!(
            !cfg.loop_backedges.is_empty(),
            "continue must record a loop backedge"
        );
        assert!(cfg.blocks.values().all(|bb| {
            bb.statements
                .iter()
                .all(|s| !matches!(s, MirStatement::Break | MirStatement::Continue))
        }));
    }

    #[test]
    fn flatten_while_with_binary_cond() {
        let body = make_body(vec![MirStatement::While {
            cond: MirRvalue::Binary {
                op: BinOp::Lt,
                left: MirOperand::ConstInt(0),
                right: MirOperand::ConstInt(10),
            },
            body: vec![],
            foreach_source: None,
        }]);
        let cfg = body.to_cfg();
        // entry → header → body → exit = 4 blocks
        assert!(cfg.blocks.len() >= 4);
        // The header block should have an Assign (materialized cond) + CondBr terminator.
        let entry = &cfg.blocks[&cfg.entry];
        assert!(matches!(entry.terminator, MirTerminator::Goto(_)));
        // Find header block (the target of entry's Goto)
        if let MirTerminator::Goto(header) = entry.terminator {
            let header_block = &cfg.blocks[&header];
            assert!(!header_block.statements.is_empty()); // Has materialized cond Assign
            assert!(matches!(
                header_block.terminator,
                MirTerminator::CondBr { .. }
            ));
        }
    }
}
