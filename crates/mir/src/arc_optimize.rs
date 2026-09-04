//! 跨块 ARC 优化：dead-copy（arc-neutral 局部）消除（RFC 015 Phase C · 刀 2.2）。
//!
//! ## 背景
//!
//! codegen 在 class 局部做**拷贝赋值**（`t = a` / `t = obj.Field` …）时发射
//! `rt_arc_inc(源)`，并在函数 epilogue 对每个 class 局部发射 `rt_arc_dec(局部)`
//!（见 `emit_cfg::assign_needs_arc_retain` / `arc_drop::emit_sync_epilogue_drops`）。
//!
//! 文本级 `codegen/llvm_ir/arc_optimize.rs` 只能消除**同基本块相邻**的 inc/dec 对；
//! 本 pass 在 MIR 层面做**跨块**分析——拷贝赋值点可能在 A 块、epilogue dec 在
//! 返回块，二者不在同一基本块，文本 pass 无能为力。
//!
//! ## 判据（dead-copy）
//!
//! 一个 class 局部 `t` 可标记为 dead-copy（其 inc/dec 整对跨块消除），当且仅当：
//!
//! 1. class 类型（`layouts.classes` 命中）；—— 值类型无 ARC，标记无副作用
//! 2. 非参数、非捕获（借用，所有权在外层）；非异步函数（见下）；
//! 3. 函数内**从未被读取**（不含合成的 `Drop` 语句——它们被 codegen 的
//!    epilogue drop 取代，见 `arc_drop.rs`；`Drop(id)` 仅是「引用释放」标记）；
//! 4. 仅以**拷贝语义** rvalue 赋值（`new`/`Call` 等所有权移交会带来 `rc=1`
//!    新对象，epilogue dec 必须保留以释放之 → 不满足则不可消除）。
//!
//! ## 安全性论证
//!
//! - 被消除的对象引用仍由**源**持有（拷贝来源局部/字段/静态字段），源自身的
//!   所有权链不受影响；`t` 从不被 deref（未读取），其槽内指针无实义。
//! - 消除后 `t` 相关引用计数**净变化为零**（inc 与 dec 一并去掉），与未优化
//!   语义完全等价，不改变任何对象的释放时机。
//! - 异步/状态机函数：局部生命周期由 env struct + dtor/EH cleanup 管理，跳过
//!   epilogue dec 会让 `rc=1` 对象提前释放 → UAF。**仅同步函数可消除**。

use std::collections::HashSet;

use crate::dataflow::live_var::{operand_locals, rvalue_locals};
use crate::types::*;
use ast::TypeId;

/// 计算可跨块消除 ARC inc/dec 对的 dead-copy 局部集合。
pub fn find_dead_arc_locals(
    cfg: &MirCfgBody,
    layouts: &typeck::ProgramLayouts,
) -> HashSet<LocalId> {
    // 仅同步函数可消除（见模块文档安全性论证）。
    if cfg.is_async {
        return HashSet::new();
    }

    let param_count = cfg.param_count;
    let captured: HashSet<LocalId> = cfg.captures.iter().map(|(cid, _, _)| *cid).collect();

    let mut reads: HashSet<LocalId> = HashSet::new();
    let mut non_copy_assign: HashSet<LocalId> = HashSet::new();

    for block in cfg.blocks.values() {
        for stmt in &block.statements {
            collect_reads(stmt, &mut reads);
            collect_non_copy_assign(stmt, &mut non_copy_assign);
        }
        for u in terminator_reads(&block.terminator) {
            reads.insert(u);
        }
    }

    let mut dead = HashSet::new();
    for (id, (_, ty)) in &cfg.locals {
        let idx = id.0 as usize;
        if idx < param_count || captured.contains(id) {
            continue;
        }
        if !is_class_local(ty, layouts) {
            continue;
        }
        // 被读取（值流入真程序操作）或含所有权移交赋值 → 不可消除。
        if reads.contains(id) || non_copy_assign.contains(id) {
            continue;
        }
        dead.insert(*id);
    }
    dead
}

/// 收集宿主函数内被嵌套闭包 **ByRef** 捕获的局部（闭包逃逸安全 → 堆槽提升）。
///
/// 闭包捕获按 `CaptureMode` 分为 ByRef（引用类型变量捕获：env 存外层变量槽地址）
/// 与 ByValue（值类型快照）。ByRef 捕获存的是**栈槽地址**——同步宿主函数返回后
/// 该槽悬垂，闭包延迟调用读到死栈槽 → 垃圾值/崩溃（DI `AddSingleton(instance)`
/// 工厂闭包捕获参数，`closure` 探针 `escaped-closure-v=890073456` 实测根因）。
///
/// 因此 ByRef 捕获的宿主局部须在 codegen 期**堆槽提升**（malloc 槽替代 alloca），
/// 使闭包与宿主函数共享跨帧存活的堆槽。本 pass 识别这些局部（`MirOperand::Closure`
/// 的 env 中 `mode == ByRef` 且 `src == Local(id)` 的 `id`）。
pub fn find_byref_captured_locals(cfg: &MirCfgBody) -> HashSet<LocalId> {
    let mut out = HashSet::new();
    for block in cfg.blocks.values() {
        for stmt in &block.statements {
            collect_byref_captures_stmt(stmt, &mut out);
        }
        collect_byref_captures_terminator(&block.terminator, &mut out);
    }
    out
}

fn collect_byref_captures_terminator(term: &MirTerminator, out: &mut HashSet<LocalId>) {
    match term {
        MirTerminator::CondBr { cond, .. } => collect_byref_captures_operand(cond, out),
        MirTerminator::Return(Some(op)) => collect_byref_captures_operand(op, out),
        MirTerminator::Throw(op) => collect_byref_captures_operand(op, out),
        MirTerminator::Return(None) | MirTerminator::Goto(_) | MirTerminator::Unreachable => {}
    }
}

fn collect_byref_captures_operand(op: &MirOperand, out: &mut HashSet<LocalId>) {
    match op {
        MirOperand::Closure { env, .. } => {
            for (cap, src) in env {
                if cap.mode == ast::CaptureMode::ByRef {
                    if let MirOperand::Local(id) = src {
                        out.insert(*id);
                    }
                }
            }
        }
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. }
        | MirOperand::UnboxString { object }
        | MirOperand::UnboxGeneric { object, .. } => collect_byref_captures_operand(object, out),
        MirOperand::Local(_)
        | MirOperand::ConstInt(_)
        | MirOperand::ConstFloat(_)
        | MirOperand::ConstString(_)
        | MirOperand::ConstBool(_)
        | MirOperand::AddrOf(_)
        | MirOperand::ConstNull
        | MirOperand::ConstDefault { .. }
        | MirOperand::FnPtr { .. }
        | MirOperand::TypeId { .. }
        | MirOperand::TypeInfoPtr { .. }
        | MirOperand::StaticField { .. } => {}
    }
}

fn collect_byref_captures_rvalue(rv: &MirRvalue, out: &mut HashSet<LocalId>) {
    match rv {
        MirRvalue::Use(op) => collect_byref_captures_operand(op, out),
        MirRvalue::Binary { left, right, .. } => {
            collect_byref_captures_operand(left, out);
            collect_byref_captures_operand(right, out);
        }
        MirRvalue::Call { args, .. } | MirRvalue::New { args, .. } => {
            for a in args {
                collect_byref_captures_operand(a, out);
            }
        }
        MirRvalue::FieldGet { object, .. } => collect_byref_captures_operand(object, out),
        MirRvalue::MethodCall { receiver, args, .. } => {
            collect_byref_captures_operand(receiver, out);
            for a in args {
                collect_byref_captures_operand(a, out);
            }
        }
        MirRvalue::MakeIface { object, .. }
        | MirRvalue::MakeIfaceDyn { object, .. }
        | MirRvalue::AdaptIface { object, .. } => collect_byref_captures_operand(object, out),
        MirRvalue::StructLit { fields, .. } => {
            for (_, o) in fields {
                collect_byref_captures_operand(o, out);
            }
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for e in elements {
                match e {
                    ArrayLitElement::Value(rv) => collect_byref_captures_rvalue(rv, out),
                    ArrayLitElement::Spread(op) => collect_byref_captures_operand(op, out),
                }
            }
        }
        MirRvalue::IndexGet { array, index, .. } => {
            collect_byref_captures_operand(array, out);
            collect_byref_captures_operand(index, out);
        }
        MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            collect_byref_captures_operand(array, out);
            if let Some(s) = start {
                collect_byref_captures_operand(s, out);
            }
            if let Some(l) = length {
                collect_byref_captures_operand(l, out);
            }
        }
        MirRvalue::SpanFromStack { elements, .. } => {
            for e in elements {
                collect_byref_captures_operand(e, out);
            }
        }
        MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            collect_byref_captures_operand(span, out);
            collect_byref_captures_operand(start, out);
            if let Some(l) = length {
                collect_byref_captures_operand(l, out);
            }
        }
        MirRvalue::SpanFill { span, value, .. } => {
            collect_byref_captures_operand(span, out);
            collect_byref_captures_operand(value, out);
        }
        MirRvalue::SpanClear { span, .. } => collect_byref_captures_operand(span, out),
        MirRvalue::SpanCopyTo { src, dest, .. } | MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            collect_byref_captures_operand(src, out);
            collect_byref_captures_operand(dest, out);
        }
        MirRvalue::SpanToArray { span, .. } => collect_byref_captures_operand(span, out),
        MirRvalue::SoaFieldGet { array, index, .. } => {
            collect_byref_captures_operand(array, out);
            collect_byref_captures_operand(index, out);
        }
        MirRvalue::LinqChain(chain) => collect_byref_captures_operand(&chain.source, out),
        MirRvalue::ExpressionTreeConst { .. } | MirRvalue::FnPtr { .. } => {}
        MirRvalue::IndirectCall { func, args } => {
            collect_byref_captures_operand(func, out);
            for a in args {
                collect_byref_captures_operand(a, out);
            }
        }
        MirRvalue::Coalesce { left, right } => {
            collect_byref_captures_operand(left, out);
            collect_byref_captures_operand(right, out);
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            collect_byref_captures_operand(cond, out);
            collect_byref_captures_operand(then_val, out);
            collect_byref_captures_operand(else_val, out);
        }
        MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            collect_byref_captures_operand(receiver, out);
            collect_byref_captures_operand(default, out);
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            collect_byref_captures_operand(receiver, out);
            for a in args {
                collect_byref_captures_operand(a, out);
            }
            collect_byref_captures_operand(default, out);
        }
        MirRvalue::ForceDerefField { receiver, .. } => {
            collect_byref_captures_operand(receiver, out)
        }
        MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            collect_byref_captures_operand(receiver, out);
            for a in args {
                collect_byref_captures_operand(a, out);
            }
        }
        MirRvalue::Box { src, .. } | MirRvalue::Unbox { src, .. } => {
            collect_byref_captures_operand(src, out)
        }
        MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(p) = payload {
                collect_byref_captures_operand(p, out);
            }
        }
        MirRvalue::VariantTag { scrutinee, .. } | MirRvalue::VariantExtract { scrutinee, .. } => {
            collect_byref_captures_operand(scrutinee, out)
        }
        MirRvalue::NewArray { length, .. } => collect_byref_captures_operand(length, out),
    }
}

fn collect_byref_captures_stmt(stmt: &MirStatement, out: &mut HashSet<LocalId>) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => collect_byref_captures_rvalue(rvalue, out),
        MirStatement::Drop(_) => {}
        MirStatement::Return(Some(rv)) => collect_byref_captures_rvalue(rv, out),
        MirStatement::Return(None) => {}
        MirStatement::FieldSet { object, value, .. } => {
            collect_byref_captures_operand(object, out);
            collect_byref_captures_rvalue(value, out);
        }
        MirStatement::StaticFieldSet { value, .. } => collect_byref_captures_rvalue(value, out),
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            collect_byref_captures_operand(array, out);
            collect_byref_captures_operand(index, out);
            collect_byref_captures_rvalue(value, out);
        }
        MirStatement::LinqForeach { chain, body, .. } => {
            collect_byref_captures_operand(&chain.source, out);
            for s in body {
                collect_byref_captures_stmt(s, out);
            }
        }
        MirStatement::Await { task, .. } => collect_byref_captures_rvalue(task, out),
        MirStatement::Throw { value } => collect_byref_captures_rvalue(value, out),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_byref_captures_stmt(s, out);
            }
            for s in catch_body {
                collect_byref_captures_stmt(s, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_byref_captures_stmt(s, out);
            }
            for s in finally {
                collect_byref_captures_stmt(s, out);
            }
        }
        MirStatement::If { .. }
        | MirStatement::While { .. }
        | MirStatement::Break
        | MirStatement::Continue => {}
    }
}

/// 是否 class 类型（镜像 `codegen::is_arc_class_slot`）。
fn is_class_local(ty: &TypeId, layouts: &typeck::ProgramLayouts) -> bool {
    matches!(ty, TypeId::Named(n) if layouts.classes.contains_key(n.as_str()))
}

/// 赋值 rvalue 是否为**拷贝语义**（codegen 会因此发射 retain）。
/// 镜像 `emit_cfg::assign_needs_arc_retain` 的 rvalue 匹配分支。
fn is_copy_retain_rvalue(rv: &MirRvalue) -> bool {
    matches!(
        rv,
        MirRvalue::Use(MirOperand::Local(_))
            | MirRvalue::Use(MirOperand::Field { .. })
            | MirRvalue::Use(MirOperand::StaticField { .. })
            | MirRvalue::Use(MirOperand::UnboxIface { .. })
            | MirRvalue::FieldGet { .. }
            | MirRvalue::Ternary { .. }
            | MirRvalue::Coalesce { .. }
            | MirRvalue::IndexGet { .. }
    )
}

/// 收集语句的**真实读取**（不含 `Drop`——它是引用释放标记，非值读取），
/// 递归进入 region 语句（TryCatch/TryFinally/LinqForeach）嵌套体。
fn collect_reads(stmt: &MirStatement, out: &mut HashSet<LocalId>) {
    match stmt {
        MirStatement::Drop(_) => {}
        MirStatement::Assign { rvalue, .. } => out.extend(rvalue_locals(rvalue)),
        MirStatement::Return(Some(rv)) => out.extend(rvalue_locals(rv)),
        MirStatement::Return(None) => {}
        MirStatement::FieldSet { object, value, .. } => {
            out.extend(operand_locals(object));
            out.extend(rvalue_locals(value));
        }
        MirStatement::StaticFieldSet { value, .. } => out.extend(rvalue_locals(value)),
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            out.extend(operand_locals(array));
            out.extend(operand_locals(index));
            out.extend(rvalue_locals(value));
        }
        MirStatement::Await { task, .. } => out.extend(rvalue_locals(task)),
        MirStatement::Throw { value } => out.extend(rvalue_locals(value)),
        MirStatement::LinqForeach { chain, body, .. } => {
            out.extend(operand_locals(&chain.source));
            for s in body {
                collect_reads(s, out);
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_reads(s, out);
            }
            for s in catch_body {
                collect_reads(s, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_reads(s, out);
            }
            for s in finally {
                collect_reads(s, out);
            }
        }
        // If/While/Break/Continue 不应出现在 CFG 顶层（to_cfg 已展平）。
        MirStatement::If { .. }
        | MirStatement::While { .. }
        | MirStatement::Break
        | MirStatement::Continue => {}
    }
}

/// 记录含**所有权移交**（非拷贝语义）赋值的局部。此类局部必须保留 epilogue
/// dec 释放 `rc=1` 新对象，不可标记为 dead-copy。
fn collect_non_copy_assign(stmt: &MirStatement, out: &mut HashSet<LocalId>) {
    match stmt {
        MirStatement::Assign { place, rvalue } => {
            if !is_copy_retain_rvalue(rvalue) {
                out.insert(*place);
            }
        }
        MirStatement::LinqForeach { body, .. } => {
            for s in body {
                collect_non_copy_assign(s, out);
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_non_copy_assign(s, out);
            }
            for s in catch_body {
                collect_non_copy_assign(s, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_non_copy_assign(s, out);
            }
            for s in finally {
                collect_non_copy_assign(s, out);
            }
        }
        _ => {}
    }
}

/// 终结符读取（Return/CondBr/Throw）。
fn terminator_reads(term: &MirTerminator) -> Vec<LocalId> {
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
    use ast::TypeId;
    use indexmap::{IndexMap, IndexSet};
    use typeck::{ClassLayout, ProgramLayouts};

    fn layouts_with_class(name: &str) -> ProgramLayouts {
        let mut classes = IndexMap::new();
        classes.insert(
            name.into(),
            ClassLayout {
                name: name.into(),
                fields: vec![],
                parent: None,
                interfaces: vec![],
                method_impl: Default::default(),
                virtual_slots: vec![],
                has_vtable: false,
                constructors: vec![],
                declared_methods: vec![],
                declared_properties: vec![],
            },
        );
        ProgramLayouts {
            classes,
            structs: IndexMap::new(),
            enums: IndexSet::new(),
            enum_variants: IndexMap::new(),
            interfaces: IndexMap::new(),
            variants: IndexMap::new(),
            static_fields: vec![],
            observable_properties: IndexSet::new(),
            type_full_names: Default::default(),
        }
    }

    fn class_cfg(stmts: Vec<MirStatement>, locals: Vec<(LocalId, TypeId)>) -> MirCfgBody {
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
        for (l, ty) in locals {
            locals_map.insert(l, ("_".into(), ty));
        }
        MirCfgBody {
            params: vec![],
            ret: TypeId::Void,
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

    fn local(id: u32) -> MirOperand {
        MirOperand::Local(LocalId(id))
    }

    #[test]
    fn dead_copy_cross_block_eliminated() {
        // L1 = L0; 且 L1 从未读取 → L1 为 dead-copy，应被标记。
        // 拷贝赋值点在 entry 块，epilogue dec 在返回路径——跨块。
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let stmts = vec![MirStatement::Assign {
            place: l1,
            rvalue: MirRvalue::Use(local(0)),
        }];
        let cfg = class_cfg(
            stmts,
            vec![
                (l0, TypeId::Named("Foo".into())),
                (l1, TypeId::Named("Foo".into())),
            ],
        );
        let dead = find_dead_arc_locals(&cfg, &layouts_with_class("Foo"));
        assert!(dead.contains(&l1), "never-read copy local should be dead");
        assert!(!dead.contains(&l0), "source local (read) must stay live");
    }

    #[test]
    fn read_local_not_eliminated() {
        // L1 = L0; 然后 L2 = L1（读取 L1）→ L1 非 dead-copy。
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let l2 = LocalId(2);
        let stmts = vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(local(0)),
            },
            MirStatement::Assign {
                place: l2,
                rvalue: MirRvalue::Use(local(1)),
            },
        ];
        let cfg = class_cfg(
            stmts,
            vec![
                (l0, TypeId::Named("Foo".into())),
                (l1, TypeId::Named("Foo".into())),
                (l2, TypeId::Named("Foo".into())),
            ],
        );
        let dead = find_dead_arc_locals(&cfg, &layouts_with_class("Foo"));
        assert!(
            !dead.contains(&l1),
            "local read by later copy must stay live"
        );
    }

    #[test]
    fn ownership_transfer_not_eliminated() {
        // L1 = Call(...)（所有权移交，rc=1 新对象）→ 必须保留 epilogue dec。
        let l1 = LocalId(1);
        let stmts = vec![MirStatement::Assign {
            place: l1,
            rvalue: MirRvalue::Call {
                func: "F".into(),
                args: vec![],
            },
        }];
        let cfg = class_cfg(stmts, vec![(l1, TypeId::Named("Foo".into()))]);
        let dead = find_dead_arc_locals(&cfg, &layouts_with_class("Foo"));
        assert!(
            !dead.contains(&l1),
            "ownership-transfer local must keep epilogue dec"
        );
    }

    #[test]
    fn async_fn_not_optimized() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let stmts = vec![MirStatement::Assign {
            place: l1,
            rvalue: MirRvalue::Use(local(0)),
        }];
        let mut cfg = class_cfg(
            stmts,
            vec![
                (l0, TypeId::Named("Foo".into())),
                (l1, TypeId::Named("Foo".into())),
            ],
        );
        cfg.is_async = true;
        let dead = find_dead_arc_locals(&cfg, &layouts_with_class("Foo"));
        assert!(dead.is_empty(), "async functions must not be optimized");
    }

    #[test]
    fn param_and_capture_not_eliminated() {
        // L1 是捕获局部：即便未读取也不得标记（借用，所有权在外层）。
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let stmts = vec![MirStatement::Assign {
            place: l1,
            rvalue: MirRvalue::Use(local(0)),
        }];
        let mut cfg = class_cfg(
            stmts,
            vec![
                (l0, TypeId::Named("Foo".into())),
                (l1, TypeId::Named("Foo".into())),
            ],
        );
        cfg.captures = vec![(
            l1,
            0,
            ast::LambdaCapture {
                name: "L1".into(),
                ty: TypeId::Named("Foo".into()),
                mode: ast::CaptureMode::ByRef,
            },
        )];
        let dead = find_dead_arc_locals(&cfg, &layouts_with_class("Foo"));
        assert!(!dead.contains(&l1), "captured local must not be eliminated");
    }

    #[test]
    fn byref_captured_locals_identified_for_heap_promotion() {
        // `() => b`（b: Foo 引用类型 ByRef 捕获）与 `() => n`（n: int 值类型 ByValue
        // 捕获）——仅 ByRef 捕获的 b 须堆槽提升。
        let b = LocalId(0);
        let n = LocalId(1);
        let cap_b = ast::LambdaCapture {
            name: "b".into(),
            ty: TypeId::Named("Foo".into()),
            mode: ast::CaptureMode::ByRef,
        };
        let cap_n = ast::LambdaCapture {
            name: "n".into(),
            ty: TypeId::Int,
            mode: ast::CaptureMode::ByValue,
        };
        let stmts = vec![MirStatement::Assign {
            place: LocalId(2),
            rvalue: MirRvalue::Use(MirOperand::Closure {
                fn_name: "__lambda_x".into(),
                env: vec![(cap_b, MirOperand::Local(b)), (cap_n, MirOperand::Local(n))],
            }),
        }];
        let cfg = class_cfg(
            stmts,
            vec![
                (b, TypeId::Named("Foo".into())),
                (n, TypeId::Int),
                (
                    LocalId(2),
                    TypeId::Func {
                        params: vec![],
                        ret: Box::new(TypeId::Named("Foo".into())),
                    },
                ),
            ],
        );
        let captured = find_byref_captured_locals(&cfg);
        assert!(
            captured.contains(&b),
            "ByRef-captured reference local must be heap-promoted"
        );
        assert!(
            !captured.contains(&n),
            "ByValue-captured value local must NOT be heap-promoted"
        );
    }
}
