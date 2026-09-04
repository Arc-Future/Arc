//! ARC drop instrumentation (RFC 015 Phase B).
//!
//! Header-only class drop: emit `rt_arc_dec` on the object pointer, **without**
//! walking class-typed fields.
//!
//! Why not field-walk on last-ref (`rt_arc_count == 1`)?
//! - `List` element release (`rt_list_arc_dec_ref`) is also header-only.
//! - A load-then-act "last ref" check is racy with other owners and can false-positive
//!   under heap noise; releasing `QIFResult.Traits` while `List<QIFResult>` still
//!   holds the object left dangling field pointers → flaky `0xC0000005` /
//!   `0xC0000374` in `WriteResults` (H1 residual after tip `608a0004`).
//! - Prefer leak nested handles until a vtable finalizer unifies List + codegen.
//!
//! Opaque runtime handles (`ThreadPoolScheduler`, `Lock`, …) are not ArcBoxes;
//! drop is a no-op for those.
//!
//! `rt_arc_dec` is a no-op on `null`.

use super::*;
use ast::TypeId;
use mir::MirOperand;

impl<'a> FnEmitter<'a> {
    /// Emit a drop sequence for a local of class type.
    ///
    /// Falls back to a plain `rt_arc_dec` call for non-class types (preserving
    /// the previous behaviour for opaque handles).
    pub fn emit_drop(&mut self, id: mir::LocalId) {
        let ty = self.local_type(id);
        if matches!(ty, TypeId::Ref { .. }) {
            // Ref params are owned by the caller — do not decrement.
            return;
        }
        // RFC 004 M1：variant 类型按 tag 分派 drop class payload
        if let TypeId::Named(name) = &ty {
            if self.layouts.variants.contains_key(name) {
                self.emit_variant_drop(id, name);
                return;
            }
        }
        // string 常为 rodata / 非 ArcHeader；arc_dec 会误读字节为 refcount → 堆损坏。
        if matches!(ty, TypeId::String) {
            return;
        }
        let (_, val) = self.emit_operand(&MirOperand::Local(id));
        match class_name(&ty) {
            Some(class) => self.emit_class_drop(&val, class),
            None => self.emit(&format!("call void @rt_arc_dec(ptr {val})")),
        }
    }

    /// Emit epilogue `rt_arc_dec` for all class-typed non-param locals on a
    /// sync return path.
    ///
    /// `mir::lower` appends `MirStatement::Drop` for every class local *after*
    /// the body's `Return` statements. `cfg::flatten_stmts` turns each
    /// `Return` into a block terminator, so those trailing Drops land in an
    /// unreachable CFG block and never execute — local strong refs leak and
    /// `Weak<T>` targets never reclaim (refcount stuck > 0 → `TryGet` never
    /// returns null).
    ///
    /// This closes that gap by emitting the drops on the return path itself,
    /// right before `ret`. It is safe because the entry block zero-inits every
    /// `ptr` slot to `null` (`emit_fn`), and `rt_arc_dec` is null-safe —
    /// dropping an as-yet-unassigned local is a no-op.
    ///
    /// `returned_local` is excluded: `return <local>;` transfers that strong
    /// ref to the caller; dec'ing it here would free the object underneath the
    /// returned pointer (double-free when rc == 1).
    ///
    /// Sync functions only. Async M1 fallback and M2 state machines manage
    /// local lifetimes via the env struct / task handle; their return paths
    /// (`emit_sm_return*`) are left untouched.
    pub(crate) fn emit_sync_epilogue_drops(&mut self, returned_local: Option<LocalId>) {
        if self.in_state_machine || self.cfg.is_async {
            return;
        }
        let to_drop = self.collect_exit_drop_locals(returned_local);
        for id in to_drop {
            self.emit_drop(id);
        }
    }

    /// 收集函数出口（return/destroy）需释放的 class 局部集合。供同步 epilogue
    /// 与 coroutine 的 final/destroy 双路径共用——所有权释放语义一致，避免冗余。
    pub(crate) fn collect_exit_drop_locals(
        &mut self,
        returned_local: Option<LocalId>,
    ) -> Vec<LocalId> {
        let param_count = self.cfg.param_count;
        let to_drop: Vec<LocalId> = self
            .cfg
            .locals
            .iter()
            .filter(|(id, (_, ty))| {
                (id.0 as usize) >= param_count
                    && is_arc_class_slot(ty, self.layouts)
                    && Some(**id) != returned_local
                    // 刀 2.2 跨块 ARC：dead-copy 局部（从不读取、仅拷贝赋值）的
                    // epilogue dec 与其赋值的 inc 整对消除（见 mir::find_dead_arc_locals
                    // / emit_cfg 普通赋值路径）。此类局部引用对象仍由源持有，跳过
                    // dec 后引用计数净变化为零。
                    && !self.dead_arc_locals.contains(id)
                    // 捕获局部（闭包 env 字段）为借用——所有权在外层局部。
                    // 事件 handler lambda 会被多次调用，每次出口 dec 会把
                    // rc=1 捕获对象提前释放 → 外层再 inc/用 UAF（实测
                    // `session.ToolInvoked = c => ...` 捕获 Counter → 释放）。
                    && !self.cfg.captures.iter().any(|(cid, _, _)| *cid == **id)
                    // ByRef 捕获局部已堆槽提升：堆槽是变量的唯一权威存储（leak-until-exit），
                    // 宿主 epilogue 不再 dec——否则对象 rc 归零释放，闭包经堆槽读已释放
                    // 指针 → 垃圾值/UAF（闭包逃逸探针 escaped-closure 实测根因）。
                    && !self.byref_captured_locals.contains(id)
            })
            .map(|(id, _)| *id)
            .collect();
        to_drop
    }

    /// RFC 039 M2：收集尺寸精确可知的栈局部到 `stack_lifetime`。
    ///
    /// 必须在 **CFG 块发射之前** 调用——return 路径的 `emit_stack_lifetime_ends`
    /// 在 CFG 块发射期间执行，需依赖本表；而 `emit_stack_lifetime_starts` 只在
    /// CFG 完成后把 start 标记写入 entry 块。二者用同一张表保证 start/end 成对。
    pub(crate) fn collect_stack_lifetime(&mut self) {
        self.stack_lifetime.clear();
        let slots: Vec<(String, u64)> = self
            .cfg
            .locals
            .iter()
            .filter(|(_, (_, ty))| !matches!(ty, TypeId::Void))
            // ByRef 捕获局部为堆槽（malloc），非栈 alloca——`llvm.lifetime.start/end`
            // 只对 alloca 有效，对 malloc 指针发射会误编译（LLVM 据 lifetime 删 store）。
            .filter(|(id, _)| !self.byref_captured_locals.contains(id))
            .filter_map(|(id, (_, ty))| {
                stack_slot_size(ty, self.layouts).map(|sz| (self.local_ptr(*id), sz))
            })
            .collect();
        self.stack_lifetime = slots;
    }

    /// RFC 039 M2：在 entry 块为 `stack_lifetime` 中的栈局部发射
    /// `!llvm.lifetime.start`。须在 `collect_stack_lifetime` 之后调用。
    ///
    /// 仅在 **同步** 函数执行（async/SM 局部在 env struct，非本帧栈 alloca）。
    /// struct / vector 等未知尺寸槽返回 `None` 被跳过——发射一个低估的尺寸
    /// 会让 LLVM 把相邻活内存 slot 误判为已死而删除合法 store（误编译）。
    pub(crate) fn emit_stack_lifetime_starts(&mut self) {
        if self.in_state_machine || self.cfg.is_async {
            return;
        }
        let slots: Vec<(String, u64)> = self.stack_lifetime.clone();
        for (slot, size) in slots {
            self.emit(&format!(
                "call void @llvm.lifetime.start.p0(i64 {size}, ptr {slot})"
            ));
        }
    }

    /// RFC 039 M2：同步 return 路径的 `!llvm.lifetime.end` 配套发射。
    ///
    /// 必须在返回表达式的值已读入寄存器之后调用（`emit_cfg` 的 Return 分支把
    /// 值加载交给 `pre_ret` / `emit_operand` 完成后才抵达 ret）。`returned_local`
    /// 的槽被排除：其值可能仍作为 ptr 返回值被引用，标记已死会触发误编译。
    /// async/SM 路径 no-op（与 `emit_stack_lifetime_starts` 对称）。
    pub(crate) fn emit_stack_lifetime_ends(&mut self, returned_local: Option<LocalId>) {
        if self.in_state_machine || self.cfg.is_async {
            return;
        }
        // 排除返回局部：`return <local>;` 的槽在 epilogue 后仍被调用方语义引用。
        let excluded: Option<String> = returned_local.map(|rl| self.local_ptr(rl));
        let slots: Vec<(String, u64)> = self
            .stack_lifetime
            .iter()
            .filter(|(slot, _)| excluded.as_ref() != Some(slot))
            .cloned()
            .collect();
        for (slot, size) in slots {
            self.emit(&format!(
                "call void @llvm.lifetime.end.p0(i64 {size}, ptr {slot})"
            ));
        }
    }

    /// Emit header-only drop for a known class (see module docs).
    pub(crate) fn emit_class_drop(&mut self, obj_val: &str, class: &str) {
        // ThreadPoolScheduler 句柄是 rt_threadpool*（calloc 池结构），不是 ArcBox。
        // 生命周期由 Shutdown/Destroy 管理；禁止 rt_arc_dec（会误读 n_workers 为 refcount）。
        if class == "ThreadPoolScheduler" {
            return;
        }
        // H1: Thread 是 opaque 句柄，但必须走 destroy——否则未 Join 的 OS 线程
        // 会跑进 WriteResults。destroy 将未 Join 者挂入 live 表，报告前统一 Join。
        if class == "Thread" {
            self.emit(&format!(
                "call void @rt_thread_handle_destroy(ptr {obj_val})"
            ));
            return;
        }
        // RFC 005 §2.2: Weak<T> 析构——_target 槽位（RtWeak*，offset 16）由
        // Weak<T> 对象本身拥有，须在「最后一次引用释放」时销毁一次，而不是
        // 每次引用 drop 都销毁：容器（List<Weak<T>> 元素槽 / 字段）与临时
        // 引用（栈上局部 / 按值副本）共享同一 Weak<T> 对象，若任一临时引用
        // drop 都执行 rt_arc_weak_destroy，共享槽位会被二次释放 → UAF/堆损坏
        // （RFC 005 里程碑②记录：List<Weak<T>> 元素访问 + 容器析构 0xC0000374）。
        // 这里先查 refcount：== 1 说明本次 drop 是唯一持有者 → destroy 槽位
        // → rt_arc_dec（对象归零释放）；> 1 只 dec header，槽位留给后续
        // 最后一次引用释放处理。顺序不可颠倒：rt_arc_dec 在 1→0 时 free 对象，
        // 之后读 offset 16 是 UAF。
        if class.starts_with("Weak_") {
            // null 守卫：未赋值 Weak 局部/空字段早退路径（如 Element.get_Parent 的
            // `_weakParent == null` return）会经 sync epilogue drops 走到本序列；
            // rt_arc_dec 契约为 null-safe，本分支必须同样先判空——否则下面的
            // `[obj_val+16]` 槽位加载在 null 上解引用 → 0xC0000005（读地址 0x10）。
            let is_null = self.fresh_temp();
            self.emit(&format!("{is_null} = icmp eq ptr {obj_val}, null"));
            let live_bb = self.fresh_label();
            let null_done_bb = self.fresh_label();
            self.emit(&format!(
                "br i1 {is_null}, label %{null_done_bb}, label %{live_bb}"
            ));
            self.emit_label(&live_bb);
            let hp = self.fresh_temp();
            let slot = self.fresh_temp();
            self.emit(&format!(
                "{hp} = getelementptr inbounds i8, ptr {obj_val}, i32 16"
            ));
            self.emit(&format!("{slot} = load ptr, ptr {hp}"));
            let rc = self.fresh_temp();
            self.emit(&format!("{rc} = call i32 @rt_arc_count(ptr {obj_val})"));
            let is_last = self.fresh_temp();
            self.emit(&format!("{is_last} = icmp eq i32 {rc}, 1"));
            let last_bb = self.fresh_label();
            let shared_bb = self.fresh_label();
            let done_bb = self.fresh_label();
            self.emit(&format!(
                "br i1 {is_last}, label %{last_bb}, label %{shared_bb}"
            ));
            self.emit_label(&last_bb);
            self.emit(&format!("call void @rt_arc_weak_destroy(ptr {slot})"));
            self.emit(&format!("call void @rt_arc_dec(ptr {obj_val})"));
            self.emit(&format!("br label %{done_bb}"));
            self.emit_label(&shared_bb);
            self.emit(&format!("call void @rt_arc_dec(ptr {obj_val})"));
            self.emit(&format!("br label %{done_bb}"));
            self.emit_label(&done_bb);
            self.emit(&format!("br label %{null_done_bb}"));
            self.emit_label(&null_done_bb);
            return;
        }
        // LinkedListNode_*：RtLinkedListNode* 透传，无 ArcHeader；由链表拥有。
        if is_opaque_runtime_handle(class) {
            return;
        }
        self.emit(&format!("call void @rt_arc_dec(ptr {obj_val})"));
    }
}

/// Extract the class name from a `TypeId::Named`.
pub(crate) fn class_name(ty: &TypeId) -> Option<&str> {
    match ty {
        TypeId::Named(n) => Some(n.as_str()),
        _ => None,
    }
}

/// Whether a local slot holds a class reference that `mir::lower` would have
/// appended a `Drop` for (mirrors `is_class_type` in lower_type.rs). The
/// codegen epilogue must drop exactly this set so the unreachable trailing
/// Drops are substituted 1:1. `Weak_<T>`, `Thread` and opaque handles are all
/// `Named` classes; `emit_drop` / `emit_class_drop` specialise their release.
pub(crate) fn is_arc_class_slot(ty: &TypeId, layouts: &typeck::ProgramLayouts) -> bool {
    matches!(ty, TypeId::Named(n) if layouts.classes.contains_key(n.as_str()))
}

/// RFC 039 M2：局部 alloca 槽的 byte size（配合 `!llvm.lifetime.start/end`）。
///
/// 仅对**标量 / ptr 槽**返回精确尺寸；struct / vector 等未知尺寸槽返回 `None`。
/// 槽类型来自 `llvm_type_of`：scalar 精确映射，class/struct/variant/array 等
/// 引用型统一为 `ptr`（8 字节），vector 为 `<n x T>`（跳过）。发射一个低估的
/// 尺寸会让 LLVM 把相邻活内存误判为已死而删除合法 store（误编译），故未知
/// 尺寸一律跳过以保证正确性优先。
fn stack_slot_size(ty: &TypeId, layouts: &typeck::ProgramLayouts) -> Option<u64> {
    let slot_ty = llvm_type_of(ty, layouts);
    match slot_ty.as_str() {
        "i1" | "i8" => Some(1),
        "i16" => Some(2),
        "i32" | "float" => Some(4),
        "i64" | "double" | "ptr" => Some(8),
        _ => None,
    }
}
