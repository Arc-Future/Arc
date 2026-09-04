//! Function emission: MirCfgBody -> LLVM IR function text.
//!
//! Each function is emitted as:
//! ```llvm
//! define <ret_ty> @<mangled_name>(<params>) {
//! entry:
//!   ; alloca all locals
//!   ; store params into allocas
//!   br label %bb<entry_id>
//! bb0:
//!   ; statements
//!   ; terminator
//! ...
//! }
//! ```

use super::*;
use ast::TypeId;
use mir::MirBlock;

/// 平台 → zero-cost EH 配置（RFC 010 / 015 里程碑②）。返回 `(uwtable 属性后缀, personality 后缀)`；
/// may-throw 用户函数需 `uwtable`（unwind 表）使 unwinder 能穿透帧，附模块级 personality。
///
/// Windows 用 MSVC `__CxxFrameHandler3`（SEH，主平台，已落地）；POSIX Itanium
///（`__gxx_personality_v0`）为里程碑⑨（1.1+，非 1.0 门槛）尚未落地，故非 Windows 返回空。
/// `nounwind` 函数（无可证抛路径）保持裸片，避免误标阻止 unwind 穿透。
fn eh_platform_attrs(is_windows: bool, nounwind: bool) -> (&'static str, &'static str) {
    if is_windows && !nounwind {
        (" uwtable", " personality ptr @__CxxFrameHandler3")
    } else {
        ("", "")
    }
}

impl<'a> FnEmitter<'a> {
    /// Emit a complete function. Returns the LLVM IR text including the function definition.
    pub fn emit_function(&mut self, name: &str) -> String {
        self.is_main = is_entry_fn(name);
        // Builtin stubs (Dictionary/List methods)
        if let Some(stub) = self.try_emit_stub(name) {
            return stub;
        }

        if self.cfg.is_async {
            let mut out = self.emit_async_function(name);
            if is_entry_fn(name) {
                out.push_str(&self.emit_async_main_entry());
            }
            return out;
        }

        self.emit_sync_function(name)
    }

    // ---- Sync function ----

    fn emit_sync_function(&mut self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let is_main = is_entry_fn(name);
        let is_lambda = name.starts_with("__lambda_");
        let has_captures = !self.cfg.captures.is_empty();
        let ret_ty = if is_main {
            "i32".to_string()
        } else {
            llvm_type_of(&self.cfg.ret, self.layouts)
        };

        // Function signature: ref/out params are passed as `ptr` (pointer to caller's slot).
        // Lambda params are always `ptr` (runtime ABI: `const void*` element pointer).
        // Captured lambdas (RFC 008) have `__env__` as first param (also `ptr`).
        let param_strs: Vec<String> = self
            .cfg
            .params
            .iter()
            .enumerate()
            .map(|(i, (_, ty))| {
                let param_ty = if is_lambda || matches!(ty, TypeId::Ref { .. } | TypeId::Void) {
                    "ptr".to_string()
                } else {
                    llvm_type_of(ty, self.layouts)
                };
                format!("{} %arg{i}", param_ty)
            })
            .collect();
        // Entry `Main` → LLVM `@main(i32 %argc, ptr %argv)` so CRT passes argv;
        // user-level `Main()` params (if any) are unchanged Mir locals, not CRT args.
        let llvm_params = if is_main {
            "i32 %argc, ptr %argv".to_string()
        } else {
            param_strs.join(", ")
        };
        let attrs = infer_user_fn_attrs(name, self.nounwind_map);
        let mut attr_str = attrs.render();
        // Zero-cost EH milestone ② (Windows SEH): may-throw user functions get
        // `uwtable` (unwind tables so the SEH walk can pass through the frame)
        // plus the module-level `__CxxFrameHandler3` personality. nounwind
        // functions (no local throw and no may-throw callees) stay bare.
        // LLVM grammar requires `personality` after `comdat`, so it is kept in
        // a separate suffix emitted after `comdat_attr()`.
        let (eh_attr, eh_suffix) = eh_platform_attrs(self.is_windows, attrs.nounwind);
        attr_str.push_str(eh_attr);
        self.output.push_str(&format!(
            "define {}{} @{mangled}({}){}{}{}{} {{\n",
            self.linkage_prefix(),
            ret_ty,
            llvm_params,
            attr_str,
            self.comdat_attr(),
            eh_suffix,
            self.dbg_attr()
        ));

        // Entry block: allocas + param stores
        self.output.push_str("entry:\n");
        // RFC 008：Func/Action 形参运行时为 arc_closure*（调用方经
        // emit_operand_as_closure 统一）。标记为 closure_locals，使
        // `f(args)` 走 extract fn_ptr/env，而非把 arc_closure* 当裸 FnPtr。
        for (i, (_, ty)) in self.cfg.params.iter().enumerate() {
            if is_delegate_type(ty) {
                self.closure_locals.insert(mir::LocalId(i as u32));
            }
        }
        // Seed Environment.ArgCount/GetArg, then RFC 006 M4 static ctors.
        if is_main {
            self.output
                .push_str("  call void @rt_env_init(i32 %argc, ptr %argv)\n");
            self.output.push_str("  call void @__arc_module_init()\n");
        }
        let local_allocas: Vec<(mir::LocalId, String, String)> =
            self.cfg
                .locals
                .iter()
                .filter(|(_, (_, ty))| !matches!(ty, TypeId::Void))
                .map(|(id, (_, ty))| {
                    // Ref param slots hold a pointer to the caller's variable
                    let slot_ty =
                        if matches!(ty, TypeId::Ref { .. }) {
                            "ptr".to_string()
                        } else if self.cfg.captures.iter().any(|(cid, _, c)| {
                            *cid == *id && matches!(c.mode, ast::CaptureMode::ByRef)
                        }) {
                            // ByRef 捕获恢复 prologue 存 ptr（外层权威槽地址），槽必须
                            // ptr 宽——标量 ByRef 捕获（capture_mode_for 变量捕获）按
                            // llvm_type_of 分配的 4 字节 alloca 会被 8 字节 store 写穿。
                            "ptr".to_string()
                        } else {
                            llvm_type_of(ty, self.layouts)
                        };
                    (*id, self.local_ptr(*id), slot_ty)
                })
                .collect();
        for (id, ptr, ty_str) in local_allocas {
            if self.byref_captured_locals.contains(&id) {
                // 闭包逃逸安全：ByRef 捕获局部提升为堆槽（C# display-class 语义）。
                // ByRef 捕获恒为引用类型（class/string/interface/Generic/Ref），
                // 槽为 8 字节 ptr；宿主帧返回后堆槽仍存活，闭包经 env 中的堆槽
                // 地址二次解引用读正确值（否则读死栈 alloca → 垃圾值/崩溃）。
                self.emit(&format!("{ptr} = call ptr @malloc(i64 8)"));
                self.emit(&format!("store ptr null, ptr {ptr}"));
            } else {
                self.emit(&format!("{ptr} = alloca {ty_str}"));
                // Zero-init ptr slots: conditionally-assigned class temps (`if (…) {
                // x = new T(); }`) must not leave garbage for epilogue `rt_arc_dec`
                // (PrintHelp / IConsole null-guard → 0xC0000005).
                if ty_str == "ptr" {
                    self.emit(&format!("store ptr null, ptr {ptr}"));
                }
            }
        }
        // Store params into their alloca slots.
        // Lambda ABI: runtime passes `const void*` (ptr to element); dereference to load
        // the element value before storing into the local alloca.
        // RFC 008: the `__env__` param (first when has_captures) is stored directly
        // (it's already a pointer to the env struct, not an element pointer).
        let param_info: Vec<(usize, TypeId, bool)> = self
            .cfg
            .params
            .iter()
            .enumerate()
            .map(|(i, (pname, ty))| (i, ty.clone(), pname.as_str() == "__env__"))
            .collect();
        for (i, ty, is_env_param) in param_info {
            if matches!(ty, TypeId::Void) {
                continue;
            }
            let store_ty = if matches!(ty, TypeId::Ref { .. }) {
                "ptr".to_string()
            } else {
                llvm_type_of(&ty, self.layouts)
            };
            let ptr = self.local_ptr(mir::LocalId(i as u32));
            if is_lambda && !is_env_param {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = load {store_ty}, ptr %arg{i}"));
                self.emit(&format!("store {store_ty} {tmp}, ptr {ptr}"));
            } else if let TypeId::Named(n) = &ty {
                // RFC 005 自动 Copy：struct 形参被调侧私有副本（C# 值语义——
                // 调用方实参不受被调方赋值影响）。此处天然在 entry 块文本序，
                // alloca 直接发射（entry_allocas 在形参 store 之后才 flush，
                // 走提升会出现使用先于定义）。
                if !self.try_emit_copy_struct_store(
                    n.as_str(),
                    &format!("%arg{i}"),
                    &ptr,
                    None,
                    false,
                ) {
                    self.emit(&format!("store {store_ty} %arg{i}, ptr {ptr}"));
                }
            } else {
                self.emit(&format!("store {store_ty} %arg{i}, ptr {ptr}"));
            }
        }
        // RFC 008: initialize captured-variable locals from the env struct.
        // Each capture (local_id, field_index, LambdaCapture) is loaded via
        // GEP+load from the `__env__` pointer (LocalId 0) and stored into the
        // local's alloca. The load type depends on `CaptureMode`:
        //   ByRef  → `ptr` (class/string/... pointer)
        //   ByValue → the value's LLVM type (e.g. `i32` for `int`)
        if has_captures {
            let env_alloca = self.local_ptr(mir::LocalId(0));
            let captures_ref: Vec<&ast::LambdaCapture> =
                self.cfg.captures.iter().map(|(_, _, c)| c).collect();
            let env_ty = self.env_struct_type(&captures_ref);
            // The `__env__` alloca holds the opaque `void*` passed as the
            // first parameter. Load it to get the actual env struct pointer
            // before GEP-ing into it (the alloca is `ptr` to `ptr`).
            let env_ptr = self.fresh_temp();
            self.emit(&format!("{env_ptr} = load ptr, ptr {env_alloca}"));
            // Clone to break the immutable borrow of self.cfg before self.emit().
            let captures: Vec<(mir::LocalId, usize, ast::CaptureMode, ast::TypeId)> = self
                .cfg
                .captures
                .iter()
                .map(|(id, idx, c)| (*id, *idx, c.mode.clone(), c.ty.clone()))
                .collect();
            for (local_id, field_idx, mode, ty) in captures {
                let field_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{field_ptr} = getelementptr {env_ty}, ptr {env_ptr}, i32 0, i32 {field_idx}"
                ));
                let field_ty = match mode {
                    ast::CaptureMode::ByRef => "ptr".to_string(),
                    ast::CaptureMode::ByValue => llvm_type_of(&ty, self.layouts),
                };
                let loaded = self.fresh_temp();
                self.emit(&format!("{loaded} = load {field_ty}, ptr {field_ptr}"));
                let dst = self.local_ptr(local_id);
                self.emit(&format!("store {field_ty} {loaded}, ptr {dst}"));
                // 捕获的委托局部（Func/Action 等）在运行时是 arc_closure*（跨边界
                // 传参/存储被 emit_operand_as_closure 统一为闭包）。默认捕获初始化
                // 直接 emit IR，不走 emit_cfg 的 closure_locals 跟踪，导致 wrapper
                // 内 `handler(args)` 被当作裸 FnPtr 直调（把闭包结构体地址当代码
                // 指针）→ 0xC0000005。这里显式标记为 closure_local，使
                // emit_indirect_call 走 emit_closure_indirect_call 提取 fn_ptr+env。
                if is_delegate_type(&ty) {
                    self.closure_locals.insert(local_id);
                }
            }
        }
        // Emit the CFG blocks into a scratch buffer: hoisted entry allocas
        // (`entry_allocas`) accumulate while loop bodies are emitted, so they
        // are spliced into the entry block only after the CFG is complete.
        let entry_prefix = std::mem::take(&mut self.output);
        // RFC 039 M2：在 CFG 发射前收集 lifetime 区间表——return 路径的
        // emit_stack_lifetime_ends 在 CFG 块发射期间执行，需依赖此表。
        self.collect_stack_lifetime();
        // RFC 005：CFG 发射前一次性地识别可提升的纯追加循环。
        self.sb_promotes = self.find_sb_promote_loops();
        // Emit each CFG block (clone to break borrow on self.cfg)
        let blocks: Vec<MirBlock> = self.cfg.blocks.values().cloned().collect();
        for block in &blocks {
            self.emit_cfg_block(block);
        }
        let cfg_out = std::mem::take(&mut self.output);
        self.output = entry_prefix;
        // Hoist expression-temp allocas into the entry block before branching.
        self.flush_entry_allocas();
        // RFC 039 M2：为尺寸精确可知的栈局部发射 `!llvm.lifetime.start`，
        // 并在 `stack_lifetime` 记录配套区间，供同步 return 路径发射 end。
        self.emit_stack_lifetime_starts();
        // Branch to CFG entry
        self.emit(&format!("br label %bb{}", self.cfg.entry.0));
        self.output.push_str(&cfg_out);
        self.output.push_str("}\n");
        std::mem::take(&mut self.output)
    }

    // ---- Async function ----
    //
    // RFC 009 M2: 含 await 的 async 走整图 CFG 状态机；无 await 的 async 回退 M1。
    // M1：同步执行体 + 已完成 Task（busy-wait+pump 仅当某处仍落入非 SM 的 Await）。

    fn emit_async_function(&mut self, name: &str) -> String {
        // RFC 009 I1/I1-ext：async（无捕获、CG 不含 EH region TryCatch/
        // TryFinally——含循环内 await，pre-split 协程原生处理 loop backedge）
        // 走协程路径（emit_async_coro）；其余含 await 的 async 走 M2
        // 状态机；无 await 的 async 回退 M1。
        if super::emit_async_sm::can_lower_as_state_machine(&self.cfg) {
            if self.can_lower_as_coroutine() {
                return self.emit_async_coroutine(name);
            }
            return self.emit_async_state_machine(name);
        }

        // M1 同步路径（fallback）
        let internal = if is_entry_fn(name) {
            "__async_main".to_string()
        } else {
            mangle_fn_name(name)
        };
        // Lambda ABI（与 emit_sync_function / emit_sm_ctor 对齐）：lambda 形参
        // 一律 `ptr` 接收（运行时 `const void*` 元素指针），函数体 load 取值。
        // 无 await 的 async lambda 落入 M1 时同样必须遵守，否则调用方
        // （emit_indirect_call 物化槽位传 ptr）与被调方（按值声明）错配，
        // 形参读到压栈指针低位——嵌套 async lambda 形参绑定损坏根因。
        let is_lambda = name.starts_with("__lambda_");

        let _ret_ty = if matches!(self.cfg.ret.task_inner(), Some(TypeId::Void)) {
            "void"
        } else {
            "i32"
        };

        // Internal async function returns ptr (task)
        let param_strs: Vec<String> = self
            .cfg
            .params
            .iter()
            .enumerate()
            .map(|(i, (_, ty))| {
                let param_ty = if is_lambda || matches!(ty, TypeId::Ref { .. } | TypeId::Void) {
                    "ptr".to_string()
                } else {
                    llvm_type_of(ty, self.layouts)
                };
                format!("{} %arg{i}", param_ty)
            })
            .collect();
        // Zero-cost EH milestone ② (Windows SEH): the M1 body compiles the full
        // user CFG synchronously, so a try/catch inside it emits a catchswitch
        // here — a may-throw M1 wrapper therefore needs `uwtable` + personality,
        // mirroring `emit_sync_function`. `personality` must follow `comdat`.
        let attrs = infer_user_fn_attrs(name, self.nounwind_map);
        let mut attr_str = String::new();
        let (eh_attr, eh_suffix) = eh_platform_attrs(self.is_windows, attrs.nounwind);
        attr_str.push_str(eh_attr);
        self.output.push_str(&format!(
            "define {}ptr @{internal}({}){}{}{}{} {{\n",
            self.linkage_prefix(),
            param_strs.join(", "),
            attr_str,
            self.comdat_attr(),
            eh_suffix,
            self.dbg_attr()
        ));

        // Entry block
        self.output.push_str("entry:\n");
        let local_allocas: Vec<(mir::LocalId, String, String)> =
            self.cfg
                .locals
                .iter()
                .filter(|(_, (_, ty))| !matches!(ty, TypeId::Void))
                .map(|(id, (_, ty))| {
                    let slot_ty =
                        if matches!(ty, TypeId::Ref { .. }) {
                            "ptr".to_string()
                        } else if self.cfg.captures.iter().any(|(cid, _, c)| {
                            *cid == *id && matches!(c.mode, ast::CaptureMode::ByRef)
                        }) {
                            // ByRef 捕获恢复 prologue 存 ptr——槽必须 ptr 宽（同 sync）。
                            "ptr".to_string()
                        } else {
                            llvm_type_of(ty, self.layouts)
                        };
                    (*id, self.local_ptr(*id), slot_ty)
                })
                .collect();
        for (id, ptr, ty_str) in local_allocas {
            if self.byref_captured_locals.contains(&id) {
                // 闭包逃逸安全：ByRef 捕获局部提升为堆槽（与 emit_sync_function 一致）。
                self.emit(&format!("{ptr} = call ptr @malloc(i64 8)"));
                self.emit(&format!("store ptr null, ptr {ptr}"));
            } else {
                self.emit(&format!("{ptr} = alloca {ty_str}"));
                if ty_str == "ptr" {
                    self.emit(&format!("store ptr null, ptr {ptr}"));
                }
            }
        }
        // Lambda ABI：`__env__` 形参直接 store（env 结构指针，非元素指针），
        // 其余 lambda 形参 load 元素值后 store——与 emit_sync_function 逐字对齐。
        let param_info: Vec<(usize, TypeId, bool)> = self
            .cfg
            .params
            .iter()
            .enumerate()
            .map(|(i, (pname, ty))| (i, ty.clone(), pname.as_str() == "__env__"))
            .collect();
        for (i, ty, is_env_param) in param_info {
            if matches!(ty, TypeId::Void) {
                continue;
            }
            let store_ty = if matches!(ty, TypeId::Ref { .. }) {
                "ptr".to_string()
            } else {
                llvm_type_of(&ty, self.layouts)
            };
            let ptr = self.local_ptr(mir::LocalId(i as u32));
            if is_lambda && !is_env_param {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = load {store_ty}, ptr %arg{i}"));
                self.emit(&format!("store {store_ty} {tmp}, ptr {ptr}"));
            } else {
                self.emit(&format!("store {store_ty} %arg{i}, ptr {ptr}"));
            }
        }
        // RFC 008：从 `__env__` 恢复捕获局部——与 emit_sync_function 逐字对齐。
        // 无 await 的 async lambda（M1）与 sync lambda 共享同一捕获 ABI；缺失
        // 此段时捕获槽保持 alloca 初值（int 恒 0、引用恒 null），嵌套 async
        // lambda 的内层捕获读到全零——流式编排「块值恒空」根因。
        let has_captures = !self.cfg.captures.is_empty();
        if has_captures {
            let env_alloca = self.local_ptr(mir::LocalId(0));
            let captures_ref: Vec<&ast::LambdaCapture> =
                self.cfg.captures.iter().map(|(_, _, c)| c).collect();
            let env_ty = self.env_struct_type(&captures_ref);
            let env_ptr = self.fresh_temp();
            self.emit(&format!("{env_ptr} = load ptr, ptr {env_alloca}"));
            let captures: Vec<(mir::LocalId, usize, ast::CaptureMode, ast::TypeId)> = self
                .cfg
                .captures
                .iter()
                .map(|(id, idx, c)| (*id, *idx, c.mode.clone(), c.ty.clone()))
                .collect();
            for (local_id, field_idx, mode, ty) in captures {
                let field_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{field_ptr} = getelementptr {env_ty}, ptr {env_ptr}, i32 0, i32 {field_idx}"
                ));
                let field_ty = match mode {
                    ast::CaptureMode::ByRef => "ptr".to_string(),
                    ast::CaptureMode::ByValue => llvm_type_of(&ty, self.layouts),
                };
                let loaded = self.fresh_temp();
                self.emit(&format!("{loaded} = load {field_ty}, ptr {field_ptr}"));
                let dst = self.local_ptr(local_id);
                self.emit(&format!("store {field_ty} {loaded}, ptr {dst}"));
                if is_delegate_type(&ty) {
                    self.closure_locals.insert(local_id);
                }
            }
        }

        // Buffer the CFG blocks; hoisted entry allocas accumulate during their
        // emission and are spliced into the entry block before the terminator.
        let entry_prefix = std::mem::take(&mut self.output);
        // Emit blocks (clone to break borrow on self.cfg)
        let blocks: Vec<MirBlock> = self.cfg.blocks.values().cloned().collect();
        for block in &blocks {
            self.emit_cfg_block(block);
        }
        let cfg_out = std::mem::take(&mut self.output);
        self.output = entry_prefix;
        self.flush_entry_allocas();
        self.emit(&format!("br label %bb{}", self.cfg.entry.0));
        self.output.push_str(&cfg_out);
        self.output.push_str("}\n");
        std::mem::take(&mut self.output)
    }

    /// Emit the main() wrapper for async main.
    ///
    /// RFC 009 M3：使用 EventLoop 驱动（取代 M1/M2 的 busy-wait poll）。
    /// 创建 EventLoop → set_current → 构造 root task → inc_pending → spawn → run → destroy。
    /// EventLoop 内部处理就绪队列 + 定时器 + waker 唤醒，直到无 pending task 退出。
    ///
    /// RFC 006 M4：在 entry 块开头调用 `@__arc_module_init()` 触发所有
    /// `__sinit_<Class>` 静态初始化器（在 EventLoop 创建之前，保证静态字段
    /// 在任何用户代码执行前初始化完毕）。
    ///
    /// RFC 009 M2（真异步缺漏纠正）：默认 async 程序自动创建并绑定 Reactor，
    /// 使 File.*Async 等数据面 I/O 直连 OS 非阻塞原语（IOCP/io_uring），不再
    /// 回退线程池包装。EventLoop tick 经 rt_io_completion_complete 分发完成事件。
    ///
    /// RFC 009 M6（多线程 Executor 默认启用）：默认 async 程序自动创建线程池
    /// （worker = hardware_concurrency）并绑定为续体执行器——EventLoop 线程
    /// 仅驱动 Reactor(IO) + 定时器 + 退出检测，async 续体由 N worker 并行执行。
    pub(super) fn emit_async_main_entry(&self) -> String {
        let mut out = String::new();
        out.push_str("define i32 @main(i32 %argc, ptr %argv) {\n");
        out.push_str("entry:\n");
        out.push_str("  call void @rt_env_init(i32 %argc, ptr %argv)\n");
        out.push_str("  call void @__arc_module_init()\n");
        out.push_str("  %loop = call ptr @rt_event_loop_create()\n");
        out.push_str("  call void @rt_event_loop_set_current(ptr %loop)\n");
        out.push_str("  %reactor = call ptr @rt_reactor_create()\n");
        out.push_str("  call void @rt_event_loop_set_reactor(ptr %loop, ptr %reactor)\n");
        out.push_str("  %pool = call ptr @rt_threadpool_create(i32 0, i32 0)\n");
        out.push_str("  call void @rt_event_loop_set_threadpool(ptr %loop, ptr %pool)\n");
        out.push_str("  %task = call ptr @__async_main()\n");
        out.push_str("  call void @rt_event_loop_set_root(ptr %loop, ptr %task)\n");
        out.push_str("  call void @rt_event_loop_inc_pending(ptr %loop)\n");
        out.push_str("  call void @rt_event_loop_spawn(ptr %loop, ptr %task)\n");
        out.push_str("  call void @rt_event_loop_run(ptr %loop)\n");
        out.push_str("  call void @rt_event_loop_destroy(ptr %loop)\n");
        out.push_str("  call void @rt_reactor_destroy(ptr %reactor)\n");
        out.push_str("  ret i32 0\n");
        out.push_str("}\n");
        out
    }
}
