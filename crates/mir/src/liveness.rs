//! 跨 await 存活分析（RFC 016 §2.1）。
//!
//! 在 MIR 层对每个局部做「跨 await 存活」判定：局部在某个 suspend 点
//! （`Await`）**之前**被定义、且在**同一个 suspend 点之后**（resume 侧或后续
//! await 后）被读取，则该局部跨该 await 存活 → 应提升为 async 状态机 env 字段。
//!
//! 判定算法（精确于「∃ 定义 d、读取 r、await a：d < a < r」，程序序）：
//! 以发射序（块排序 + 块内语句序 + region 嵌套序）遍历语句树，为每个语句
//! 分配单调递增的位置号，记录：
//! - 每个局部的定义位置集合（`Assign` 的 place、`Await` 的 place）；
//! - 每个局部的读取位置集合（rvalue/operand 中的 `Local`）；
//! - 全部 await 的位置集合。
//!
//! 局部 `L` 跨 await 存活 ⇔ ∃ def d, read r, await a：d < a < r。
//!
//! 该判定对分支/循环为保守（可能过近似提升，但绝不漏提升真正存活局部），
//! 与 codegen 收敛 env 字段/ARC 配对面共用同一份输出（单一事实来源）。

use std::collections::{HashMap, HashSet};

use crate::types::*;

/// 计算跨 await 存活的局部集合。
///
/// 供 codegen `emit_async_sm` 收敛 env 字段与 ARC 配对面使用。只对含 await
/// 的状态机（M2）有意义；无 await 的 async 纯同步走 M1，不消费此结果。
pub fn cross_await_live_locals(cfg: &MirCfgBody) -> HashSet<LocalId> {
    if !cfg.is_async {
        return HashSet::new();
    }
    let mut walker = AwaitLivenessWalker::default();
    walker.walk_cfg(cfg);
    walker.finish()
}

/// 程序序遍历的收集器。
#[derive(Default)]
struct AwaitLivenessWalker {
    pos: u64,
    awaits: Vec<u64>,
    defs: HashMap<LocalId, Vec<u64>>,
    reads: HashMap<LocalId, Vec<u64>>,
}

impl AwaitLivenessWalker {
    /// 以发射序遍历 CFG（块按 id 升序，块内语句按序，region 嵌套按序）。
    fn walk_cfg(&mut self, cfg: &MirCfgBody) {
        // 参数在函数入口隐式「已定义」——位置 0 早于一切 await（首个语句自增后 ≥ 1）。
        // 此前未登记参数 def，导致参数即使在某 await 之后被读取也不被判为跨 await
        // 存活：class 参数无法提升为 env 唯一 owner，而 ctor 又为其 env 字段 inc 了
        // 独立 +1，形成「无人释放」的不一致（泄漏 / 配对面错位）。登记位置 0 后，
        // 凡在任一 await 之后被读取的 class 参数即跨 await 存活 → env 唯一 owner，
        // 由 dtor 释放恰一次，与 ctor 的 inc 配对。
        for i in 0..cfg.param_count {
            let id = LocalId(i as u32);
            if cfg.locals.contains_key(&id) {
                self.defs.entry(id).or_default().push(0);
            }
        }
        let mut blocks: Vec<&MirBlock> =
            cfg.blocks.values().filter(|b| !is_dead_block(b)).collect();
        blocks.sort_by_key(|b| b.id.0);
        // 记录每块 def 的局部（Assign/Await place），供 backedge 处理区分
        // 「循环内重新定义」的局部（如循环变量 i++）与「循环外定义、header
        // 跨迭代读取」的局部（如 while 标志）。
        let mut block_defs: HashMap<BlockId, HashSet<LocalId>> = HashMap::new();
        for block in &blocks {
            let mut defs = HashSet::new();
            for stmt in &block.statements {
                Self::collect_defs_stmt(stmt, &mut defs);
            }
            block_defs.insert(block.id, defs);
        }
        for block in blocks {
            for stmt in &block.statements {
                self.walk_stmt(stmt);
            }
            self.walk_terminator(&block.terminator);
        }

        // 循环回边（loop_backedges）：backedge 目标（loop header）在每次迭代
        // 都会被重新读取。线性位置号无法表达回边语义——header 中读取的局部若
        // 在循环体内任一 await 之后仍被读取（下一次迭代），应判为跨 await 存活，
        // 否则该局部不提升为 env 字段 → resume 后读取未初始化 alloca
        // （`while (flag) { await; … }` 标志型循环只执行 1 次迭代的根因，
        // arc_ai_turn_loop multi_round 的 RunLoopAsync 提前返回 null 同源）。
        //
        // **仅对「循环外定义、header 跨迭代读取」的控制局部追加**（while 标志
        // 等）。循环内被迭代重写（计数 `i++`、标志翻转）的局部**刻意不追加**：
        // 其每次迭代新值已由循环体赋值，追加会过度提升 async 状态机 env 布局
        // （TLS 握手等复杂 async 循环 → 0xC0000409，http11_https_e2e 实测）。
        // 因此**计数型 async 循环（`while (i<n) { await; i++; }`）的 `i` 仍
        // 不提升**——resume 后读取未初始化 alloca，属已知技术债（与 TLS
        // env 布局副作用同源）；本修复只覆盖标志型/循环外定义控制量场景。
        let loop_end_pos = self.pos;
        for bj in &cfg.loop_backedges {
            let Some(bj_block) = cfg.blocks.get(bj) else {
                continue;
            };
            let MirTerminator::Goto(header) = &bj_block.terminator else {
                continue;
            };
            let Some(hblock) = cfg.blocks.get(header) else {
                continue;
            };
            // 循环内 defs = 从 header 到 backedge（含）所有块的 def 并集。
            let mut loop_defs = HashSet::new();
            let mut cur = *header;
            let mut guard = 0usize;
            while guard < 1000 && cfg.blocks.contains_key(&cur) {
                if let Some(d) = block_defs.get(&cur) {
                    loop_defs.extend(d.iter().copied());
                }
                if cur == *bj {
                    break;
                }
                // 沿 backedge 路径前推：取当前块 terminator 的下一块（单后继）。
                let next = match &cfg.blocks[&cur].terminator {
                    MirTerminator::Goto(n) => *n,
                    MirTerminator::CondBr {
                        then_bb, else_bb, ..
                    } if cur == *header => *then_bb,
                    _ => break,
                };
                cur = next;
                guard += 1;
            }
            for stmt in &hblock.statements {
                for id in read_locals_stmt(stmt) {
                    if !loop_defs.contains(&id) {
                        self.reads.entry(id).or_default().push(loop_end_pos + 1);
                    }
                }
            }
            for id in read_locals_terminator(&hblock.terminator) {
                if !loop_defs.contains(&id) {
                    self.reads.entry(id).or_default().push(loop_end_pos + 1);
                }
            }
        }
    }

    /// 收集语句中的 def 局部（Assign/Await place）。供循环回边处理判断
    /// 「局部是否在循环内被重新定义」。
    fn collect_defs_stmt(stmt: &MirStatement, out: &mut HashSet<LocalId>) {
        match stmt {
            MirStatement::Assign { place, .. } => {
                out.insert(*place);
            }
            MirStatement::Await { place, .. } => {
                out.insert(*place);
            }
            MirStatement::If {
                then_body,
                else_body,
                ..
            } => {
                for s in then_body {
                    Self::collect_defs_stmt(s, out);
                }
                for s in else_body {
                    Self::collect_defs_stmt(s, out);
                }
            }
            MirStatement::While { body, .. } | MirStatement::LinqForeach { body, .. } => {
                for s in body {
                    Self::collect_defs_stmt(s, out);
                }
            }
            MirStatement::TryCatch {
                try_body,
                catch_body,
                ..
            } => {
                for s in try_body {
                    Self::collect_defs_stmt(s, out);
                }
                for s in catch_body {
                    Self::collect_defs_stmt(s, out);
                }
            }
            MirStatement::TryFinally { body, finally } => {
                for s in body {
                    Self::collect_defs_stmt(s, out);
                }
                for s in finally {
                    Self::collect_defs_stmt(s, out);
                }
            }
            _ => {}
        }
    }

    fn walk_stmt(&mut self, stmt: &MirStatement) {
        // 位置号在语句处理前分配（自增），保证语句之间严格递增。
        match stmt {
            MirStatement::Assign { place, rvalue } => {
                self.pos += 1;
                let p = self.pos;
                self.defs.entry(*place).or_default().push(p);
                for id in read_locals_rvalue(rvalue) {
                    self.reads.entry(id).or_default().push(p);
                }
            }
            MirStatement::Await { place, task } => {
                self.pos += 1;
                let p = self.pos;
                self.awaits.push(p);
                self.defs.entry(*place).or_default().push(p);
                for id in read_locals_rvalue(task) {
                    self.reads.entry(id).or_default().push(p);
                }
            }
            MirStatement::Drop(id) => {
                self.pos += 1;
                let p = self.pos;
                self.reads.entry(*id).or_default().push(p);
            }
            MirStatement::FieldSet { object, value, .. } => {
                self.pos += 1;
                let p = self.pos;
                for id in read_locals_operand(object) {
                    self.reads.entry(id).or_default().push(p);
                }
                for id in read_locals_rvalue(value) {
                    self.reads.entry(id).or_default().push(p);
                }
            }
            MirStatement::StaticFieldSet { value, .. } => {
                self.pos += 1;
                let p = self.pos;
                for id in read_locals_rvalue(value) {
                    self.reads.entry(id).or_default().push(p);
                }
            }
            MirStatement::IndexSet {
                array,
                index,
                value,
                ..
            } => {
                self.pos += 1;
                let p = self.pos;
                for id in read_locals_operand(array)
                    .into_iter()
                    .chain(read_locals_operand(index))
                    .chain(read_locals_rvalue(value))
                {
                    self.reads.entry(id).or_default().push(p);
                }
            }
            MirStatement::Throw { value } => {
                self.pos += 1;
                let p = self.pos;
                for id in read_locals_rvalue(value) {
                    self.reads.entry(id).or_default().push(p);
                }
            }
            MirStatement::Return(Some(rv)) => {
                self.pos += 1;
                let p = self.pos;
                for id in read_locals_rvalue(rv) {
                    self.reads.entry(id).or_default().push(p);
                }
            }
            // region 语句：递归遍历嵌套 body（位置号继续递增）。
            MirStatement::TryCatch {
                try_body,
                catch_body,
                ..
            } => {
                for s in try_body {
                    self.walk_stmt(s);
                }
                for s in catch_body {
                    self.walk_stmt(s);
                }
            }
            MirStatement::TryFinally { body, finally } => {
                for s in body {
                    self.walk_stmt(s);
                }
                for s in finally {
                    self.walk_stmt(s);
                }
            }
            MirStatement::LinqForeach { body, .. } => {
                for s in body {
                    self.walk_stmt(s);
                }
            }
            MirStatement::If {
                then_body,
                else_body,
                ..
            } => {
                for s in then_body {
                    self.walk_stmt(s);
                }
                for s in else_body {
                    self.walk_stmt(s);
                }
            }
            MirStatement::While { body, .. } => {
                for s in body {
                    self.walk_stmt(s);
                }
            }
            MirStatement::Return(None) | MirStatement::Break | MirStatement::Continue => {
                self.pos += 1;
            }
        }
    }

    fn walk_terminator(&mut self, term: &MirTerminator) {
        self.pos += 1;
        let p = self.pos;
        for id in read_locals_terminator(term) {
            self.reads.entry(id).or_default().push(p);
        }
    }

    fn finish(self) -> HashSet<LocalId> {
        let mut live = HashSet::new();
        // 局部存活 ⇔ ∃ def、read、await：def_pos < await_pos < read_pos。
        // 对每个 await，取「最晚的 < await 的 def」与「最早的 > await 的 read」。
        let mut awaits = self.awaits;
        awaits.sort_unstable();
        for (id, def_positions) in &self.defs {
            let Some(read_positions) = self.reads.get(id) else {
                continue;
            };
            for &a in &awaits {
                let def_before = def_positions.iter().any(|&d| d < a);
                if !def_before {
                    continue;
                }
                let read_after = read_positions.iter().any(|&r| r > a);
                if read_after {
                    live.insert(*id);
                    break;
                }
            }
        }
        live
    }
}

fn is_dead_block(block: &MirBlock) -> bool {
    matches!(block.terminator, MirTerminator::Unreachable) && block.statements.is_empty()
}

/// 终结符中读取的 local（不递归 region）。
fn read_locals_terminator(term: &MirTerminator) -> Vec<LocalId> {
    match term {
        MirTerminator::CondBr { cond, .. } => read_locals_operand(cond),
        MirTerminator::Return(Some(op)) => read_locals_operand(op),
        MirTerminator::Throw(op) => read_locals_operand(op),
        MirTerminator::Return(None) | MirTerminator::Goto(_) | MirTerminator::Unreachable => {
            Vec::new()
        }
    }
}

/// 语句中读取的 local（含 region 嵌套 body 递归）。供循环回边处理收集
/// loop header 的读取（与 [`AwaitLivenessWalker::walk_stmt`] 的 reads 侧同构）。
fn read_locals_stmt(stmt: &MirStatement) -> Vec<LocalId> {
    match stmt {
        MirStatement::Assign { rvalue, .. } => read_locals_rvalue(rvalue),
        MirStatement::Await { task, .. } => read_locals_rvalue(task),
        MirStatement::Drop(id) => vec![*id],
        MirStatement::FieldSet { object, value, .. } => {
            let mut v = read_locals_operand(object);
            v.extend(read_locals_rvalue(value));
            v
        }
        MirStatement::StaticFieldSet { value, .. } => read_locals_rvalue(value),
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            let mut v = read_locals_operand(array);
            v.extend(read_locals_operand(index));
            v.extend(read_locals_rvalue(value));
            v
        }
        MirStatement::Throw { value } => read_locals_rvalue(value),
        MirStatement::Return(Some(rv)) => read_locals_rvalue(rv),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            let mut v = Vec::new();
            for s in try_body {
                v.extend(read_locals_stmt(s));
            }
            for s in catch_body {
                v.extend(read_locals_stmt(s));
            }
            v
        }
        MirStatement::TryFinally { body, finally } => {
            let mut v = Vec::new();
            for s in body {
                v.extend(read_locals_stmt(s));
            }
            for s in finally {
                v.extend(read_locals_stmt(s));
            }
            v
        }
        MirStatement::LinqForeach { body, .. } => {
            let mut v = Vec::new();
            for s in body {
                v.extend(read_locals_stmt(s));
            }
            v
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            let mut v = Vec::new();
            for s in then_body {
                v.extend(read_locals_stmt(s));
            }
            for s in else_body {
                v.extend(read_locals_stmt(s));
            }
            v
        }
        MirStatement::While { body, .. } => {
            let mut v = Vec::new();
            for s in body {
                v.extend(read_locals_stmt(s));
            }
            v
        }
        MirStatement::Return(None) | MirStatement::Break | MirStatement::Continue => Vec::new(),
    }
}

/// rvalue 中读取的 local。
fn read_locals_rvalue(rv: &MirRvalue) -> Vec<LocalId> {
    let mut v: Vec<LocalId> = Vec::new();
    match rv {
        MirRvalue::Use(op) => v.extend(read_locals_operand(op)),
        MirRvalue::Binary { left, right, .. } => {
            v.extend(read_locals_operand(left));
            v.extend(read_locals_operand(right));
        }
        MirRvalue::Call { args, .. } | MirRvalue::New { args, .. } => {
            v.extend(args.iter().flat_map(read_locals_operand));
        }
        MirRvalue::FieldGet { object, .. } => v.extend(read_locals_operand(object)),
        MirRvalue::MethodCall { receiver, args, .. } => {
            v.extend(read_locals_operand(receiver));
            v.extend(args.iter().flat_map(read_locals_operand));
        }
        MirRvalue::MakeIface { object, .. }
        | MirRvalue::MakeIfaceDyn { object, .. }
        | MirRvalue::AdaptIface { object, .. } => v.extend(read_locals_operand(object)),
        MirRvalue::StructLit { fields, .. } => {
            v.extend(fields.iter().flat_map(|(_, o)| read_locals_operand(o)));
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for e in elements {
                match e {
                    ArrayLitElement::Value(rv) => v.extend(read_locals_rvalue(rv)),
                    ArrayLitElement::Spread(op) => v.extend(read_locals_operand(op)),
                }
            }
        }
        MirRvalue::IndexGet { array, index, .. } => {
            v.extend(read_locals_operand(array));
            v.extend(read_locals_operand(index));
        }
        MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            v.extend(read_locals_operand(array));
            if let Some(s) = start {
                v.extend(read_locals_operand(s));
            }
            if let Some(l) = length {
                v.extend(read_locals_operand(l));
            }
        }
        MirRvalue::SpanFromStack { elements, .. } => {
            v.extend(elements.iter().flat_map(read_locals_operand));
        }
        MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            v.extend(read_locals_operand(span));
            v.extend(read_locals_operand(start));
            if let Some(l) = length {
                v.extend(read_locals_operand(l));
            }
        }
        MirRvalue::SpanFill { span, value, .. } => {
            v.extend(read_locals_operand(span));
            v.extend(read_locals_operand(value));
        }
        MirRvalue::SpanClear { span, .. } => v.extend(read_locals_operand(span)),
        MirRvalue::SpanCopyTo { src, dest, .. } | MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            v.extend(read_locals_operand(src));
            v.extend(read_locals_operand(dest));
        }
        MirRvalue::SpanToArray { span, .. } => v.extend(read_locals_operand(span)),
        MirRvalue::SoaFieldGet { array, index, .. } => {
            v.extend(read_locals_operand(array));
            v.extend(read_locals_operand(index));
        }
        MirRvalue::LinqChain(chain) => v.extend(read_locals_operand(&chain.source)),
        MirRvalue::ExpressionTreeConst { .. } | MirRvalue::FnPtr { .. } => {}
        MirRvalue::IndirectCall { func, args } => {
            v.extend(read_locals_operand(func));
            v.extend(args.iter().flat_map(read_locals_operand));
        }
        MirRvalue::Coalesce { left, right } => {
            v.extend(read_locals_operand(left));
            v.extend(read_locals_operand(right));
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            v.extend(read_locals_operand(cond));
            v.extend(read_locals_operand(then_val));
            v.extend(read_locals_operand(else_val));
        }
        MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            v.extend(read_locals_operand(receiver));
            v.extend(read_locals_operand(default));
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            v.extend(read_locals_operand(receiver));
            v.extend(args.iter().flat_map(read_locals_operand));
            v.extend(read_locals_operand(default));
        }
        MirRvalue::ForceDerefField { receiver, .. } => v.extend(read_locals_operand(receiver)),
        MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            v.extend(read_locals_operand(receiver));
            v.extend(args.iter().flat_map(read_locals_operand));
        }
        MirRvalue::Box { src, .. } | MirRvalue::Unbox { src, .. } => {
            v.extend(read_locals_operand(src))
        }
        MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(p) = payload {
                v.extend(read_locals_operand(p));
            }
        }
        MirRvalue::VariantTag { scrutinee, .. } | MirRvalue::VariantExtract { scrutinee, .. } => {
            v.extend(read_locals_operand(scrutinee))
        }
        MirRvalue::NewArray { length, .. } => v.extend(read_locals_operand(length)),
    }
    v
}

/// operand 中读取的 local（含闭包 env 捕获）。
fn read_locals_operand(op: &MirOperand) -> Vec<LocalId> {
    match op {
        MirOperand::Local(l) => vec![*l],
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. }
        | MirOperand::UnboxString { object }
        | MirOperand::UnboxGeneric { object, .. } => read_locals_operand(object),
        MirOperand::AddrOf(l) => vec![*l],
        MirOperand::Closure { env, .. } => env
            .iter()
            .flat_map(|(_, o)| read_locals_operand(o))
            .collect(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn local_cfg(stmts: Vec<MirStatement>, locals: Vec<LocalId>) -> MirCfgBody {
        local_cfg_param_count(stmts, locals, 0)
    }

    fn local_cfg_param_count(
        stmts: Vec<MirStatement>,
        locals: Vec<LocalId>,
        param_count: usize,
    ) -> MirCfgBody {
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
            param_count,
            locals: locals_map,
            entry,
            blocks,
            is_async: true,
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

    #[test]
    fn live_across_await_promoted() {
        let l = LocalId(1);
        let stmts = vec![
            MirStatement::Assign {
                place: l,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
            },
            MirStatement::Await {
                place: LocalId(2),
                task: MirRvalue::Call {
                    func: "A".into(),
                    args: vec![],
                },
            },
            MirStatement::Assign {
                place: LocalId(3),
                rvalue: MirRvalue::Use(MirOperand::Local(l)),
            },
        ];
        let cfg = local_cfg(stmts, vec![l, LocalId(2), LocalId(3)]);
        let live = cross_await_live_locals(&cfg);
        assert!(
            live.contains(&l),
            "local defined before await & read after await must be live"
        );
    }

    #[test]
    fn dead_before_await_not_promoted() {
        let l = LocalId(1);
        // l 在 await 之后定义、之后读取：不跨 await（无「定义在 await 前」）。
        let stmts = vec![
            MirStatement::Await {
                place: LocalId(2),
                task: MirRvalue::Call {
                    func: "A".into(),
                    args: vec![],
                },
            },
            MirStatement::Assign {
                place: l,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
            },
            MirStatement::Assign {
                place: LocalId(3),
                rvalue: MirRvalue::Use(MirOperand::Local(l)),
            },
        ];
        let cfg = local_cfg(stmts, vec![l, LocalId(2), LocalId(3)]);
        let live = cross_await_live_locals(&cfg);
        assert!(
            !live.contains(&l),
            "local defined after await must not be promoted"
        );
    }

    #[test]
    fn no_await_no_promotion() {
        let l = LocalId(1);
        let stmts = vec![
            MirStatement::Assign {
                place: l,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
            },
            MirStatement::Assign {
                place: LocalId(3),
                rvalue: MirRvalue::Use(MirOperand::Local(l)),
            },
        ];
        let cfg = local_cfg(stmts, vec![l, LocalId(3)]);
        let live = cross_await_live_locals(&cfg);
        assert!(live.is_empty(), "no await → nothing cross-await-live");
    }

    /// 参数在 await 之后被读取 → 必须判为跨 await 存活（env 唯一 owner），
    /// 否则 ctor 已 inc 的 class 参数 +1 无人释放（泄漏 / 配对面错位）。
    #[test]
    fn param_read_after_await_promoted() {
        // param = LocalId(0)；await 之后读取 → 存活。
        let param = LocalId(0);
        let stmts = vec![
            MirStatement::Await {
                place: LocalId(1),
                task: MirRvalue::Call {
                    func: "A".into(),
                    args: vec![],
                },
            },
            MirStatement::Assign {
                place: LocalId(2),
                rvalue: MirRvalue::Use(MirOperand::Local(param)),
            },
        ];
        let cfg = local_cfg_param_count(stmts, vec![param, LocalId(1), LocalId(2)], 1);
        let live = cross_await_live_locals(&cfg);
        assert!(
            live.contains(&param),
            "param read after await must be cross-await-live"
        );
    }

    /// 参数仅在 await 之前被读取 → 不跨 await 存活（不得提升为 env 唯一 owner）。
    #[test]
    fn param_read_before_await_not_promoted() {
        let param = LocalId(0);
        let stmts = vec![
            MirStatement::Assign {
                place: LocalId(1),
                rvalue: MirRvalue::Use(MirOperand::Local(param)),
            },
            MirStatement::Await {
                place: LocalId(2),
                task: MirRvalue::Call {
                    func: "A".into(),
                    args: vec![],
                },
            },
        ];
        let cfg = local_cfg_param_count(stmts, vec![param, LocalId(1), LocalId(2)], 1);
        let live = cross_await_live_locals(&cfg);
        assert!(
            !live.contains(&param),
            "param read only before await must not be cross-await-live"
        );
    }

    /// 循环回边：loop header 中读取的局部（如 while 标志/循环变量）在迭代间被
    /// **重新**读取。线性位置号（块序单调）把 header 读取排在循环体 await 之前 →
    /// 修复前漏提升 → resume 后读取未初始化 alloca（`while (i<n) { await … }`
    /// 只执行 1 次迭代的根因）。backedge header 读取须追加末尾位置判为存活。
    #[test]
    fn loop_header_read_promoted_across_await() {
        let flag = LocalId(1);
        let tmp = LocalId(2);
        let entry = BlockId(0);
        let header = BlockId(1);
        let body = BlockId(2);
        let exit = BlockId(3);
        let backedge = BlockId(4);
        let mut blocks = IndexMap::new();
        // bb0: flag = 1; goto header
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: vec![MirStatement::Assign {
                    place: flag,
                    rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
                }],
                terminator: MirTerminator::Goto(header),
            },
        );
        // bb1 (header): CondBr flag → body / exit（header 读取 flag）
        blocks.insert(
            header,
            MirBlock {
                id: header,
                statements: vec![],
                terminator: MirTerminator::CondBr {
                    cond: MirOperand::Local(flag),
                    then_bb: body,
                    else_bb: exit,
                },
            },
        );
        // bb2 (body): await; goto backedge
        blocks.insert(
            body,
            MirBlock {
                id: body,
                statements: vec![MirStatement::Await {
                    place: tmp,
                    task: MirRvalue::Call {
                        func: "A".into(),
                        args: vec![],
                    },
                }],
                terminator: MirTerminator::Goto(backedge),
            },
        );
        // bb3 (exit): return
        blocks.insert(
            exit,
            MirBlock {
                id: exit,
                statements: vec![],
                terminator: MirTerminator::Return(None),
            },
        );
        // bb4 (backedge): goto header
        blocks.insert(
            backedge,
            MirBlock {
                id: backedge,
                statements: vec![],
                terminator: MirTerminator::Goto(header),
            },
        );
        let mut locals_map = IndexMap::new();
        locals_map.insert(flag, ("_".into(), typeck::TypeId::Void));
        locals_map.insert(tmp, ("_".into(), typeck::TypeId::Void));
        let cfg = MirCfgBody {
            params: vec![],
            ret: typeck::TypeId::Void,
            param_count: 0,
            locals: locals_map,
            entry,
            blocks,
            is_async: true,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: Linkage::External,
            parallelize: false,
            loop_backedges: [backedge].into_iter().collect(),
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        };
        let live = cross_await_live_locals(&cfg);
        assert!(
            live.contains(&flag),
            "loop header read must be cross-await-live (backedge re-read), got {live:?}"
        );
    }
}
