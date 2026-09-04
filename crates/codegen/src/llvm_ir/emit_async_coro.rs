//! Async coroutine lowering（RFC 009 / plan.md 阶段 3 断点 I1）。
//!
//! 将「直线体 async 函数」（await 不在循环 / try-finally / try-catch 内、
//! 无捕获）编译为 **pre-split LLVM 协程**：单帧发射 `llvm.coro.*` intrinsic
//! 序列，帧切分（CoroSplit）、跨 suspend 存活局部提升（SROA）、分配消除
//!（CoroElide）全部委托 clang 管线——`presplitcoroutine` 函数属性是
//! CoroSplit 的开关（clang 22 默认管线含 -O0 均跑，经 target/coro_probe/
//! probe2.ll 实证）。
//!
//! ## 发射形态（canonical；intrinsic 签名以本机 clang 22 前端产出
//! target/coro_probe/ref_raw.ll 为准，非 LLVM 经典文档）
//!
//! 每个 async 函数 `F` 编译为三个函数：
//!
//! - **ramp `@F`**（协程函数本体，`presplitcoroutine`）：
//!   `coro.id(0, null, null, null)` → `coro.alloc` → `coro.size.i64` +
//!   `@malloc` → `coro.begin`；`rt_task_from_coroutine(%frame, @__coro_thunk_F,
//!   @__coro_destroy_F)`（I2 收敛：单次调用创建 Task，runtime 直接持帧所有权）；
//!   body CFG 与同步函数同构（alloca 全部位于 entry，跨 suspend 存活者由
//!   CoroSplit 提升为帧字段——无手写 env struct / save / load）。
//!   - await 边界：抢占检查 → `rt_task_poll(inner)`；PENDING 时
//!     `rt_task_register_waker(inner, %task)` + `coro.save(ptr null)` +
//!     `coro.suspend(%save, false)` → switch `0` 续行 / `1` destroy
//!     （cleanup）/ default yield。resume 侧先 re-poll awaiter——事件
//!     循环首轮 tick 经 thunk 无条件 resume 本帧（poll 即推进），awaiter
//!     未完成时回挂起点重等（与状态机 resume 的 re-poll 语义对齐）。
//!   - return：结果经 `rt_task_set_result_*` 写入 Task → final suspend
//!     （`coro.suspend(%save, true)`，此后 `coro.done` 置位）。
//!   - cleanup（destroy 路径）：dec 帧持有的 class 引用 → `coro.free` +
//!     `@free`；coro.ret：`coro.end(ptr null, i1 false, token none)` +
//!     `ret ptr %task`。
//! - **thunk `@__coro_thunk_F`**：桥接 `rt_resume_fn` 契约
//!   （`i32 (ptr env, ptr waker)`）。先 `coro.done` 预检（同步完成的
//!   body 一路跑到 final suspend，ramp 返回的 Task 仍是 PENDING，首次
//!   poll 不得对 final-suspended 帧 resume——UB），未 done 则
//!   `coro.resume` + `coro.done` → READY(0)/PENDING(1)。
//! - **destroy `@__coro_destroy_F`**：`coro.destroy(%frame)`，随
//!   `rt_task_from_coroutine` 传给 runtime 存为 dtor_fn，由 `rt_task_release`
//!   调用；CoroSplit 把 destroy 入口接到各 suspend 点的 `i8 1` 分支（cleanup）。
//!
//! ## 帧槽所有权（与状态机 env 模型同构，禁双轨）
//!
//! 跨 await 存活局部 = `mir::cross_await_live_locals`（单一事实来源）。
//! class 参数在 ramp preamble 无条件 `rt_arc_inc` 授予帧 +1（caller 仍持
//! 自己的引用）；cleanup 路径对帧持有的 class 局部（跨 await 存活 ∩
//! `arc_class_place`）逐一 `rt_arc_dec`——与状态机 ctor inc / dtor dec
//! 严格配对。body 内 Assign / await 提取走 C11 alloca 覆写分支
//!（`in_state_machine=false`，inc-before-dec 自平衡），Drop 语句跳过
//! 帧持有的 class 局部。
//!
//! ## 分派（保守）
//!
//! [`FnEmitter::can_lower_as_coroutine`]：`is_async`、任意嵌套深度活块无
//! TryCatch/TryFinally 语句（EH funclet × 协程帧切分后置）、无捕获
//!（`captures` 与 `byref_captured_locals` 均空）。While/LinqForeach 与 CFG
//! backedge 已放开：循环内 await 由 pre-split 协程原生处理（CoroSplit 保留
//! loop CFG）。不满足者一律回退 `emit_async_sm` 状态机路径（零变化）。

use super::*;
use ast::TypeId;
use mir::{MirRvalue, MirStatement};

/// 语句（含嵌套 region body）是否含协程路径不支持的结构。
///
/// - TryCatch/TryFinally：EH funclet × 协程帧切分的交互 I1 不碰
///   （await-in-try 留状态机路径，其行为已验证），短中期后置。
/// - While/LinqForeach 已放开：循环内 await 由 pre-split 协程原生处理
///   （CoroSplit 保留 loop CFG，suspend 点内联于循环体中，resume 落入原块
///   continue 位置），`count_await_sites` 与发射端早已递归统计循环内 await。
fn stmt_has_coro_excluded_region(stmt: &MirStatement) -> bool {
    match stmt {
        MirStatement::TryCatch { .. } | MirStatement::TryFinally { .. } => true,
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(stmt_has_coro_excluded_region)
                || else_body.iter().any(stmt_has_coro_excluded_region)
        }
        _ => false,
    }
}

/// 统计语句树（含嵌套 region body）内的 await 位点总数。
///
/// 与发射端 `emit_coro_await` 的递增计数器构成双射：preamble 按此数为每个
/// await 预发帧槽 alloca（`%__coro_awaiter_N`，entry 块——非 entry 的
/// alloca 在 suspend 重入时行为未定义），发射时计数器递增取槽。
fn count_await_sites(stmt: &MirStatement) -> usize {
    match stmt {
        MirStatement::Await { .. } => 1,
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            try_body.iter().map(count_await_sites).sum::<usize>()
                + catch_body.iter().map(count_await_sites).sum::<usize>()
        }
        MirStatement::TryFinally { body, finally } => {
            body.iter().map(count_await_sites).sum::<usize>()
                + finally.iter().map(count_await_sites).sum::<usize>()
        }
        MirStatement::While { body, .. } | MirStatement::LinqForeach { body, .. } => {
            body.iter().map(count_await_sites).sum()
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().map(count_await_sites).sum::<usize>()
                + else_body.iter().map(count_await_sites).sum::<usize>()
        }
        _ => 0,
    }
}

impl<'a> FnEmitter<'a> {
    /// I1 分派：直线体 async 是否可走 pre-split 协程路径。
    ///
    /// 调用前提：`can_lower_as_state_machine` 已通过（is_async 且含 await）。
    pub(crate) fn can_lower_as_coroutine(&self) -> bool {
        if !self.cfg.is_async {
            return false;
        }
        // lambda async / 变量捕获（ByRef 堆槽 × 协程帧交互）→ 状态机路径。
        if !self.cfg.captures.is_empty() || !self.byref_captured_locals.is_empty() {
            return false;
        }
        // 循环已放开：CFG backedge 的 suspend 由 pre-split 协程原生处理
        //（CoroSplit 保留 loop CFG，resume 沿 backedge 回到循环头再入后续
        // 迭代，无需状态机回退）。仅 EH region（TryCatch/TryFinally）仍拒。
        self.cfg.blocks.values().all(|b| {
            super::emit_async_sm::is_dead_block(b)
                || b.statements
                    .iter()
                    .all(|s| !stmt_has_coro_excluded_region(s))
        })
    }

    /// 发射协程三件套（thunk + destroy + ramp）。main entry wrapper 由
    /// `emit_function` 统一生成（与状态机路径共用），此处不重复。
    pub(crate) fn emit_async_coroutine(&mut self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let sanitized = mangled.replace("::", "_");
        let internal = if is_entry_fn(name) {
            "__async_main".to_string()
        } else {
            mangled.clone()
        };
        let thunk_name = format!("__coro_thunk_{sanitized}");
        let destroy_name = format!("__coro_destroy_{sanitized}");

        // 保存上下文（FnEmitter 跨函数复用）。
        let prev_in_coro = std::mem::replace(&mut self.in_coroutine, true);
        let prev_task_slot = std::mem::take(&mut self.coro_task_slot);
        let prev_final = std::mem::take(&mut self.coro_final_label);
        let prev_cleanup = std::mem::take(&mut self.coro_cleanup_label);
        let prev_ret = std::mem::take(&mut self.coro_ret_label);
        let prev_await_ctr = std::mem::replace(&mut self.coro_await_counter, 0);
        let prev_await_live = std::mem::take(&mut self.await_live_locals);
        // 帧槽所有权判定与状态机同源：跨 await 存活局部（单一事实来源）。
        self.await_live_locals = mir::cross_await_live_locals(&self.cfg);
        self.coro_task_slot = "%__coro_task".to_string();
        self.coro_final_label = "coro_final".to_string();
        self.coro_cleanup_label = "coro_cleanup".to_string();
        self.coro_ret_label = "coro_ret".to_string();

        let thunk_fn = self.emit_coro_thunk(&thunk_name);
        let destroy_fn = self.emit_coro_destroy(&destroy_name);
        let ramp_fn = self.emit_coro_ramp(name, &internal, &thunk_name, &destroy_name);

        self.in_coroutine = prev_in_coro;
        self.coro_task_slot = prev_task_slot;
        self.coro_final_label = prev_final;
        self.coro_cleanup_label = prev_cleanup;
        self.coro_ret_label = prev_ret;
        self.coro_await_counter = prev_await_ctr;
        self.await_live_locals = prev_await_live;

        format!("{thunk_fn}\n{destroy_fn}\n{ramp_fn}\n")
    }

    /// ABI thunk：桥接 `rt_resume_fn`（`i32 (ptr env, ptr waker)` → 新 status）。
    ///
    /// done 预检是正确性前提：body 内所有 await 均 ready 时 ramp 一路跑到
    /// final suspend，返回的 Task 仍为 PENDING——首次 poll 若直接 resume
    /// final-suspended 帧即 UB（帧 resume 指针已置 null）。done → READY(0)；
    /// 否则 resume 后再查 done（中间 suspend → PENDING(1)，final → READY(0)）。
    fn emit_coro_thunk(&mut self, name: &str) -> String {
        self.output
            .push_str(&format!("define i32 @{name}(ptr %frame, ptr %waker) {{\n"));
        self.output.push_str("entry:\n");
        let done0 = self.fresh_temp();
        self.emit(&format!("{done0} = call i1 @llvm.coro.done(ptr %frame)"));
        self.emit(&format!(
            "br i1 {done0}, label %already_done, label %not_done"
        ));
        self.emit_label("already_done");
        self.emit("ret i32 0"); // RT_TASK_READY
        self.emit_label("not_done");
        self.emit("call void @llvm.coro.resume(ptr %frame)");
        let done1 = self.fresh_temp();
        self.emit(&format!("{done1} = call i1 @llvm.coro.done(ptr %frame)"));
        let status = self.fresh_temp();
        self.emit(&format!("{status} = select i1 {done1}, i32 0, i32 1"));
        self.emit(&format!("ret i32 {status}"));
        self.output.push_str("}\n");
        std::mem::take(&mut self.output)
    }

    /// destroy thunk：`rt_task_release` 经 dtor_fn 调用，把帧交还 CoroSplit
    /// 生成的 destroy 入口（各 suspend 点 `i8 1` 分支 → cleanup 路径）。
    fn emit_coro_destroy(&mut self, name: &str) -> String {
        self.output
            .push_str(&format!("define void @{name}(ptr %frame) {{\n"));
        self.output.push_str("entry:\n");
        self.emit("call void @llvm.coro.destroy(ptr %frame)");
        self.emit("ret void");
        self.output.push_str("}\n");
        std::mem::take(&mut self.output)
    }

    /// 发射协程 ramp（函数本体）。参数 ABI 与状态机 ctor 同构（Ref → ptr）；
    /// 属性恒 `presplitcoroutine uwtable`（+ Windows personality）——协程帧
    /// 上恒有 unwind 路径（await fault rethrow 的 `rt_throw` 为 plain call，
    /// 异常穿越 resume 帧需要 unwind 表），不采信 call-graph nounwind。
    fn emit_coro_ramp(
        &mut self,
        name: &str,
        internal: &str,
        thunk_name: &str,
        destroy_name: &str,
    ) -> String {
        let param_strs: Vec<String> = self
            .cfg
            .params
            .iter()
            .enumerate()
            .map(|(i, (_, ty))| {
                let param_ty = if matches!(ty, TypeId::Ref { .. } | TypeId::Void) {
                    "ptr".to_string()
                } else {
                    llvm_type_of(ty, self.layouts)
                };
                format!("{} %arg{i}", param_ty)
            })
            .collect();
        let mut eh_suffix = String::new();
        if self.is_windows {
            eh_suffix.push_str(" personality ptr @__CxxFrameHandler3");
        }
        self.output.push_str(&format!(
            "define {}ptr @{internal}({}) presplitcoroutine uwtable{}{}{} {{\n",
            self.linkage_prefix(),
            param_strs.join(", "),
            self.comdat_attr(),
            eh_suffix,
            self.dbg_attr()
        ));
        self.output.push_str("entry:\n");
        let _ = name; // 仅用于调试元数据（dbg_attr 已消费 subprogram）

        // Func/Action 形参运行时为 arc_closure*（与同步函数一致）。
        for (i, (_, ty)) in self.cfg.params.iter().enumerate() {
            if is_delegate_type(ty) {
                self.closure_locals.insert(mir::LocalId(i as u32));
            }
        }

        // 全部局部 alloca（entry 块 → CoroSplit/SROA 把跨 suspend 存活者提升
        // 为帧字段；ptr 槽零初始化防脏读——cleanup 的 dec 读到未赋值槽）。
        let local_allocas: Vec<(mir::LocalId, String, String)> = self
            .cfg
            .locals
            .iter()
            .filter(|(_, (_, ty))| !matches!(ty, TypeId::Void))
            .map(|(id, (_, ty))| {
                let slot_ty = if matches!(ty, TypeId::Ref { .. }) {
                    "ptr".to_string()
                } else {
                    llvm_type_of(ty, self.layouts)
                };
                (*id, self.local_ptr(*id), slot_ty)
            })
            .collect();
        for (_id, ptr, ty_str) in local_allocas {
            self.emit(&format!("{ptr} = alloca {ty_str}"));
            if ty_str == "ptr" {
                self.emit(&format!("store ptr null, ptr {ptr}"));
            }
        }

        // 参数 store 进帧槽；跨 await 存活的 class 参数授予帧独立 +1
        //（caller 仍持自己的引用），cleanup 路径 dec 配对——与状态机 ctor
        // 的 env-owned inc 同构。
        let params: Vec<(usize, TypeId)> = self
            .cfg
            .params
            .iter()
            .enumerate()
            .map(|(i, (_, ty))| (i, ty.clone()))
            .collect();
        for (i, ty) in params {
            if matches!(ty, TypeId::Void) {
                continue;
            }
            let store_ty = if matches!(ty, TypeId::Ref { .. }) {
                "ptr".to_string()
            } else {
                llvm_type_of(&ty, self.layouts)
            };
            let id = mir::LocalId(i as u32);
            let ptr = self.local_ptr(id);
            self.emit(&format!("store {store_ty} %arg{i}, ptr {ptr}"));
            if self.await_live_locals.contains(&id) && Self::arc_class_place(&ty, self.layouts) {
                self.emit(&format!("call void @rt_arc_inc(ptr %arg{i})"));
            }
        }

        // Task 槽与 awaiter 槽（跨 suspend 存活 → 帧字段）。
        self.emit("%__coro_task = alloca ptr");
        self.emit("store ptr null, ptr %__coro_task");
        let n_awaits: usize = self
            .cfg
            .blocks
            .values()
            .filter(|b| !super::emit_async_sm::is_dead_block(b))
            .map(|b| b.statements.iter().map(count_await_sites).sum::<usize>())
            .sum();
        for i in 0..n_awaits {
            self.emit(&format!("%__coro_awaiter_{i} = alloca ptr"));
            self.emit(&format!("store ptr null, ptr %__coro_awaiter_{i}"));
        }

        // 协程帧前奏（canonical）：coro.id → alloc/size/malloc → begin。
        let coro_id = self.fresh_temp();
        self.emit(&format!(
            "{coro_id} = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)"
        ));
        let need_alloc = self.fresh_temp();
        self.emit(&format!(
            "{need_alloc} = call i1 @llvm.coro.alloc(token {coro_id})"
        ));
        self.emit(&format!(
            "br i1 {need_alloc}, label %coro_frame_alloc, label %coro_frame_init"
        ));
        self.emit_label("coro_frame_alloc");
        let frame_size = self.fresh_temp();
        self.emit(&format!("{frame_size} = call i64 @llvm.coro.size.i64()"));
        let frame_mem = self.fresh_temp();
        self.emit(&format!("{frame_mem} = call ptr @malloc(i64 {frame_size})"));
        self.emit("br label %coro_frame_init");
        self.emit_label("coro_frame_init");
        let frame_phi = self.fresh_temp();
        self.emit(&format!(
            "{frame_phi} = phi ptr [ null, %entry ], [ {frame_mem}, %coro_frame_alloc ]"
        ));
        let coro_hdl = self.fresh_temp();
        self.emit(&format!(
            "{coro_hdl} = call ptr @llvm.coro.begin(token {coro_id}, ptr {frame_phi})"
        ));

        // Task 创建：单次协程 ABI 调用（I2 收敛）——runtime 直接持帧所有权，
        // resume=thunk（resume_data=帧）、dtor=destroy（CoroSplit cleanup
        // 路径）。Task 强持帧生命周期——release 即经 destroy 销毁帧。
        let task = self.fresh_temp();
        self.emit(&format!(
            "{task} = call ptr @rt_task_from_coroutine(ptr {coro_hdl}, ptr @{thunk_name}, ptr @{destroy_name})"
        ));
        self.emit(&format!("store ptr {task}, ptr %__coro_task"));

        // Body CFG（与同步函数同构；await / return 走 in_coroutine 分派）。
        let entry_prefix = std::mem::take(&mut self.output);
        let blocks: Vec<mir::MirBlock> = self.cfg.blocks.values().cloned().collect();
        for block in &blocks {
            self.emit_cfg_block(block);
        }
        let cfg_out = std::mem::take(&mut self.output);
        self.output = entry_prefix;
        self.flush_entry_allocas();
        // 起始挂起（index 0）：ramp 创建 Task 后立即 yield，body 不随 @F 调用
        // 运行。首轮 coro.resume（父 await 的 rt_task_poll 驱动）经本 suspend 的
        // 0 分支进入 bb{entry}——与状态机「创建即 PENDING、body 于首次 poll 驱动」
        // 语义对齐。这是「无 re-entrant suspend」的前提：body 内每个 await 的
        // coro.suspend 只在其具体子任务真正挂起时才首次到达（forward 路径），
        // 恢复路径（resume）直达提取，绝不自 resume 继续回环到同一 suspend 块。
        // 若此处仍直接 `br bb{entry}`，则 @F 调用即跑 body 到首个 await 挂起，
        // 父 await 的首次 rt_task_poll 即对该「已挂起的非 final suspend」再次
        // resume → 需 re-poll/re-suspend → 引入 LLVM CoroSplit 不支持的
        // re-entrant suspend（二次 resume 误判 final → coro.done=true → 误 READY，
        // 子成孤儿 → teardown AV）——此前 0xC0000005 铁证根因。
        self.emit_coro_suspend_switch(&format!("bb{}", self.cfg.entry.0));
        self.output.push_str(&cfg_out);

        // ---- 协程尾部 ----
        // final suspend：body 完成后挂起（done 置位），等待 destroy 释放帧。
        // 与 await 边界同构的 switch：0=resume（final 后 resume 未定义，
        // 防御性落 cleanup）/ 1=destroy → cleanup / default=yield 返回。
        self.emit_label("coro_final");
        let save_final = self.fresh_temp();
        self.emit(&format!(
            "{save_final} = call token @llvm.coro.save(ptr null)"
        ));
        let suspend_final = self.fresh_temp();
        self.emit(&format!(
            "{suspend_final} = call i8 @llvm.coro.suspend(token {save_final}, i1 true)"
        ));
        self.emit(&format!(
            "switch i8 {suspend_final}, label %coro_ret [\n    i8 0, label %coro_final_resumed\n    i8 1, label %coro_cleanup\n  ]"
        ));
        self.emit_label("coro_final_resumed");
        self.emit("br label %coro_cleanup");

        // cleanup（destroy 入口）：释放帧槽持有的 class 引用（与状态机 dtor
        // 的 dec 集合同源：跨 await 存活 ∩ arc_class_place，含参数），再释放
        // 帧内存。rt_arc_dec 对 null 安全（槽在 preamble 零初始化）。
        self.emit_label("coro_cleanup");
        let class_locals: Vec<mir::LocalId> = self
            .cfg
            .locals
            .iter()
            .filter(|(id, (_, ty))| {
                self.await_live_locals.contains(id)
                    && !matches!(ty, TypeId::Void)
                    && Self::arc_class_place(ty, self.layouts)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &class_locals {
            let ptr = self.local_ptr(*id);
            let val = self.fresh_temp();
            self.emit(&format!("{val} = load ptr, ptr {ptr}"));
            self.emit(&format!("call void @rt_arc_dec(ptr {val})"));
        }
        let free_mem = self.fresh_temp();
        self.emit(&format!(
            "{free_mem} = call ptr @llvm.coro.free(token {coro_id}, ptr {coro_hdl})"
        ));
        let has_free = self.fresh_temp();
        self.emit(&format!("{has_free} = icmp ne ptr {free_mem}, null"));
        self.emit(&format!(
            "br i1 {has_free}, label %coro_frame_free, label %coro_after_free"
        ));
        self.emit_label("coro_frame_free");
        self.emit(&format!("call void @free(ptr {free_mem})"));
        self.emit("br label %coro_after_free");
        self.emit_label("coro_after_free");
        self.emit("br label %coro_ret");

        // yield 返回：suspend 的 default 分支汇聚于此（ramp 首挂返回 /
        // resume 调用栈内返回，返回值对后者无意义但 IR 一致）。
        self.emit_label("coro_ret");
        self.emit("call void @llvm.coro.end(ptr null, i1 false, token none)");
        let ret_task = self.fresh_temp();
        self.emit(&format!("{ret_task} = load ptr, ptr %__coro_task"));
        self.emit(&format!("ret ptr {ret_task}"));
        self.output.push_str("}\n");
        std::mem::take(&mut self.output)
    }

    /// 发射 `coro.save` + `coro.suspend` + switch（clang 22 语义）。
    ///
    /// 返回值编码（与 LLVM 经典文档相反，以 clang 22 前端产出为准）：
    /// `0` = resume 续行、`1` = destroy → cleanup、default = suspend yield。
    fn emit_coro_suspend_switch(&mut self, resume_label: &str) {
        let save = self.fresh_temp();
        self.emit(&format!("{save} = call token @llvm.coro.save(ptr null)"));
        let sv = self.fresh_temp();
        self.emit(&format!(
            "{sv} = call i8 @llvm.coro.suspend(token {save}, i1 false)"
        ));
        self.emit(&format!(
            "switch i8 {sv}, label %{} [\n    i8 0, label %{resume_label}\n    i8 1, label %{}\n  ]",
            self.coro_ret_label, self.coro_cleanup_label
        ));
    }

    /// 在协程路径内发射单个 await（由 `emit_stmt` 在 `in_coroutine` 时调用）。
    ///
    /// 与状态机 `emit_sm_await` 逐段同构：重跑入口（抢占恢复）→ 抢占检查 →
    /// poll inner task → PENDING 则 register_waker + 挂起；完成则 fault
    /// rethrow / 提取结果。差异仅在挂起机制（env state ↔ coro.suspend）与
    /// 局部保存（save_locals ↔ 帧槽自动提升）。
    pub(super) fn emit_coro_await(&mut self, place: mir::LocalId, task: &MirRvalue) {
        let await_idx = self.coro_await_counter;
        self.coro_await_counter += 1;
        let place_ty = self.local_type(place);
        let task_expected = TypeId::Task {
            inner: Box::new(place_ty.clone()),
        };
        let awaiter_slot = format!("%__coro_awaiter_{await_idx}");

        // 入口（抢占恢复后重跑整个 await，含 task 表达式求值）。
        let reenter = format!("coro_await_{await_idx}_reenter");
        self.emit(&format!("br label %{reenter}"));
        self.emit_label(&reenter);
        let (_, task_val) = self.emit_rvalue_typed(task, &task_expected);

        // 抢占检查（与状态机 emit_sm_await 同构）：worker 协作调度边界。
        let no_preempt = self.fresh_label();
        let preempt = format!("coro_await_{await_idx}_preempt");
        let worker_ctx = self.fresh_temp();
        self.emit(&format!(
            "{worker_ctx} = call ptr @rt_threadpool_current_worker_ctx()"
        ));
        let preempt_flag = self.fresh_temp();
        self.emit(&format!(
            "{preempt_flag} = call i32 @rt_worker_preempt_check(ptr {worker_ctx})"
        ));
        let is_preempted = self.fresh_temp();
        self.emit(&format!("{is_preempted} = icmp ne i32 {preempt_flag}, 0"));
        self.emit(&format!(
            "br i1 {is_preempted}, label %{preempt}, label %{no_preempt}"
        ));

        // 抢占挂起：恢复（0 分支）后回 reenter 重跑本 await。
        self.emit_label(&preempt);
        self.emit(&format!(
            "call void @rt_worker_preempt_clear(ptr {worker_ctx})"
        ));
        self.emit_coro_suspend_switch(&reenter);

        // 无抢占：poll inner task（推进其状态机/协程）。
        self.emit_label(&no_preempt);
        self.emit(&format!("store ptr {task_val}, ptr {awaiter_slot}"));
        let status = self.fresh_temp();
        self.emit(&format!(
            "{status} = call i32 @rt_task_poll(ptr {task_val})"
        ));
        let pending = self.fresh_temp();
        self.emit(&format!("{pending} = icmp eq i32 {status}, 1"));
        let suspend_label = format!("coro_await_{await_idx}_suspend");
        let resume_label = format!("coro_await_{await_idx}_resume");
        self.emit(&format!(
            "br i1 {pending}, label %{suspend_label}, label %{resume_label}"
        ));

        // 挂起：登记 waker（inner 完成时唤醒本 Task → event loop 重新推进
        // 本协程帧），save + suspend。
        self.emit_label(&suspend_label);
        let outer_task = self.fresh_temp();
        self.emit(&format!(
            "{outer_task} = load ptr, ptr {}",
            self.coro_task_slot
        ));
        self.emit(&format!(
            "call void @rt_task_register_waker(ptr {task_val}, ptr {outer_task})"
        ));
        self.emit_coro_suspend_switch(&resume_label);

        // 恢复：re-poll 提取（非配对唤醒防御）。
        //
        // 历史教训：此处曾是「零 re-poll 直达提取」——前提「帧仅被 waker 触发
        // resume、此刻 awaiter 必已离开 PENDING」经 l2_net_batch accept-null
        // 取证证伪：await_waiting 守卫位可被非配对 coro_wake 清除（complete 与
        // register 的时序交错、slab 指针复用交叠），EventLoop 据此合法推进挂起
        // 帧 → 提取时 inner 仍 PENDING → ptr_result 空 → await 得 null
        //（FAIL:accept-null，net_tcp_echo_async 高频复现，实证序列：extract
        // status=PENDING 先于 accept COMPLETE）。
        //
        // 现语义：resume 后先 re-poll——READY 直下提取；PENDING 走**第二挂起
        // 点**（独立 coro.suspend，CoroSplit 按 suspend 点分配恢复索引；回边
        // 目标为 suspend2 自身，与「循环内 await」同款回环形态，合法）重等并
        // 重登记 waker（幽灵唤醒会消费 waker 槽，inner 真完成前必须重新挂上）。
        // 仍禁止回环到 suspend_label（首个挂起点）：其恢复索引已被首挂消费，
        // 二次进入即 re-entrant suspend（CoroSplit 误判 final → teardown AV）。
        self.emit_label(&resume_label);
        let inner_task = self.fresh_temp();
        self.emit(&format!("{inner_task} = load ptr, ptr {awaiter_slot}"));
        let ready_label = format!("coro_await_{await_idx}_ready");
        let suspend2_label = format!("coro_await_{await_idx}_suspend2");
        let resume2_label = format!("coro_await_{await_idx}_resume2");
        let st_repoll = self.fresh_temp();
        self.emit(&format!(
            "{st_repoll} = call i32 @rt_task_poll(ptr {inner_task})"
        ));
        let pending_repoll = self.fresh_temp();
        self.emit(&format!("{pending_repoll} = icmp eq i32 {st_repoll}, 1"));
        self.emit(&format!(
            "br i1 {pending_repoll}, label %{suspend2_label}, label %{ready_label}"
        ));

        // 第二挂起点：重登记 waker（内嵌槽覆盖幂等）→ coro.suspend。
        self.emit_label(&suspend2_label);
        let outer_task2 = self.fresh_temp();
        self.emit(&format!(
            "{outer_task2} = load ptr, ptr {}",
            self.coro_task_slot
        ));
        self.emit(&format!(
            "call void @rt_task_register_waker(ptr {inner_task}, ptr {outer_task2})"
        ));
        self.emit_coro_suspend_switch(&resume2_label);

        // 第二挂起点恢复：再 re-poll；PENDING 回环 suspend2 重等。
        self.emit_label(&resume2_label);
        let inner_task2 = self.fresh_temp();
        self.emit(&format!("{inner_task2} = load ptr, ptr {awaiter_slot}"));
        let st_repoll2 = self.fresh_temp();
        self.emit(&format!(
            "{st_repoll2} = call i32 @rt_task_poll(ptr {inner_task2})"
        ));
        let pending_repoll2 = self.fresh_temp();
        self.emit(&format!("{pending_repoll2} = icmp eq i32 {st_repoll2}, 1"));
        self.emit(&format!(
            "br i1 {pending_repoll2}, label %{suspend2_label}, label %{ready_label}"
        ));

        // awaiter 已完成：fault rethrow 或提取。
        self.emit_label(&ready_label);
        let inner_ready = self.fresh_temp();
        self.emit(&format!("{inner_ready} = load ptr, ptr {awaiter_slot}"));
        let fault_label = format!("coro_await_{await_idx}_fault");
        let extract_label = format!("coro_await_{await_idx}_extract");
        let faulted = self.fresh_temp();
        self.emit(&format!(
            "{faulted} = call i32 @rt_task_is_faulted(ptr {inner_ready})"
        ));
        let faulted_b = self.fresh_temp();
        self.emit(&format!("{faulted_b} = icmp ne i32 {faulted}, 0"));
        self.emit(&format!(
            "br i1 {faulted_b}, label %{fault_label}, label %{extract_label}"
        ));

        // faulted：提取异常 rethrow（与状态机同构：先 inc 授予在途副本，
        // 再 release 归还 Task 所有权）。异常穿越协程帧由 rt_task_poll 的
        // SEH 边界捕获 → rt_task_fault，destroy 经 dtor_fn 释放帧。
        self.emit_label(&fault_label);
        let exc = self.fresh_temp();
        self.emit(&format!(
            "{exc} = call ptr @rt_task_get_exception(ptr {inner_ready})"
        ));
        self.emit(&format!("call void @rt_arc_inc(ptr {exc})"));
        self.emit(&format!("call void @rt_task_release(ptr {inner_ready})"));
        self.emit_call_may_throw("void", "@rt_throw", &format!("ptr {exc}"), true, None);
        self.emit("unreachable");

        // 提取结果到帧槽（C11 覆写配对）+ 释放 inner Task——与状态机共用。
        self.emit_label(&extract_label);
        self.emit_await_extract(place, &inner_ready);
    }

    /// 协程 return（terminator 版，由 `emit_terminator` 在 `in_coroutine`
    /// 时调用）：结果写入 Task 句柄 → final suspend 等待 destroy。
    pub(super) fn emit_coro_return(&mut self, val: &Option<mir::MirOperand>) {
        let inner_ty = self.cfg.ret.task_inner().cloned().unwrap_or(TypeId::Void);
        if let Some(op) = val {
            if !matches!(inner_ty, TypeId::Void) {
                let is_class_ret = Self::arc_class_place(&inner_ty, self.layouts);
                let (ty, val_str) = self.emit_operand(op);
                let task_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{task_ptr} = load ptr, ptr {}",
                    self.coro_task_slot
                ));
                self.emit_coro_set_result(&task_ptr, is_class_ret, &ty, &val_str);
            }
        }
        self.emit(&format!("br label %{}", self.coro_final_label));
    }

    /// 协程 return（语句版，由 `emit_stmt::Return` 在 `in_coroutine` 时调用）。
    pub(super) fn emit_coro_return_stmt(&mut self, val: &Option<MirRvalue>) {
        let inner_ty = self.cfg.ret.task_inner().cloned().unwrap_or(TypeId::Void);
        if let Some(rv) = val {
            if !matches!(inner_ty, TypeId::Void) {
                let is_class_ret = Self::arc_class_place(&inner_ty, self.layouts);
                let (ty, val_str) = self.emit_rvalue_typed(rv, &inner_ty);
                let task_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{task_ptr} = load ptr, ptr {}",
                    self.coro_task_slot
                ));
                self.emit_coro_set_result(&task_ptr, is_class_ret, &ty, &val_str);
            }
        }
        self.emit(&format!("br label %{}", self.coro_final_label));
    }

    /// 结果写入 ABI（与状态机 emit_sm_return 同构）：class 结果无条件
    /// `rt_arc_inc`（授予 Task +1，与 `rt_task_release` 的 dec 严格配对）+
    /// `rt_task_set_result_class` 置 ptr_is_class；其余按实际表示走
    /// `emit_task_set_result_abi`（string/array 借用路径）。
    fn emit_coro_set_result(&mut self, task_ptr: &str, is_class: bool, ty: &str, val: &str) {
        if is_class {
            self.emit(&format!("call void @rt_arc_inc(ptr {val})"));
            self.emit(&format!(
                "call void @rt_task_set_result_class(ptr {task_ptr}, ptr {val})"
            ));
        } else {
            self.emit_task_set_result_abi(task_ptr, ty, val);
        }
    }

    /// 帧槽所有权谓词：跨 await 存活的 class 局部（含参数）。
    ///
    /// body 内 Drop 跳过（帧持所有权，cleanup 统一 dec），否则双重释放。
    pub(super) fn is_coro_owned_class_local(&self, id: mir::LocalId) -> bool {
        if !self.in_coroutine {
            return false;
        }
        if !self.await_live_locals.contains(&id) {
            return false;
        }
        let ty = self.local_type(id);
        !matches!(ty, TypeId::Void) && Self::arc_class_place(&ty, self.layouts)
    }
}
