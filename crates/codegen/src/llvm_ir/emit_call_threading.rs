//! Threading / sync / Parallel facade emission (extracted from emit_call).

use super::*;
use mir::MirOperand;

impl<'a> FnEmitter<'a> {
    pub(super) fn try_emit_thread_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let result: TyVal = match method {
            "Start" => {
                self.emit(&format!("call void @rt_thread_handle_start(ptr {recv})"));
                ("void".into(), String::new())
            }
            "Join" => {
                self.emit(&format!("call void @rt_thread_handle_join(ptr {recv})"));
                ("void".into(), String::new())
            }
            "get_IsAlive" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_thread_handle_is_alive(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// Mutex facade (RFC 009 M5.5).
    pub(super) fn try_emit_mutex_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let result: TyVal = match method {
            "Lock" => {
                self.emit(&format!("call void @rt_mutex_lock(ptr {recv})"));
                ("void".into(), String::new())
            }
            "Unlock" => {
                self.emit(&format!("call void @rt_mutex_unlock(ptr {recv})"));
                ("void".into(), String::new())
            }
            "TryLock" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_mutex_try_lock(ptr {recv})"));
                ("i32".into(), tmp)
            }
            "Dispose" => {
                self.emit(&format!("call void @rt_mutex_destroy(ptr {recv})"));
                ("void".into(), String::new())
            }
            _ => return None,
        };
        Some(result)
    }

    /// Semaphore facade (RFC 009 M5.5).
    pub(super) fn try_emit_semaphore_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let result: TyVal = match method {
            "Wait" => {
                if args.is_empty() {
                    self.emit(&format!("call void @rt_semaphore_wait(ptr {recv})"));
                } else {
                    let (ms_ty, ms) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let ms64 = if ms_ty == "i64" {
                        ms
                    } else {
                        let widened = self.fresh_temp();
                        self.emit(&format!("{widened} = sext {ms_ty} {ms} to i64"));
                        widened
                    };
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_semaphore_wait_timeout(ptr {recv}, i64 {ms64})"
                    ));
                    return Some(("i32".into(), tmp));
                }
                ("void".into(), String::new())
            }
            "Release" => {
                if args.is_empty() {
                    self.emit(&format!("call void @rt_semaphore_release(ptr {recv})"));
                } else {
                    // std P3: Release(int count) 批量归还（§7.3 登记，Yamux
                    // 字节级流控前置）。int 发射面为 i32，防御性 trunc 与
                    // Wait 分支的 sext 对偶。
                    let (n_ty, n) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let n32 = if n_ty == "i32" {
                        n
                    } else {
                        let narrowed = self.fresh_temp();
                        self.emit(&format!("{narrowed} = trunc {n_ty} {n} to i32"));
                        narrowed
                    };
                    self.emit(&format!(
                        "call void @rt_semaphore_release_n(ptr {recv}, i32 {n32})"
                    ));
                }
                ("void".into(), String::new())
            }
            "Dispose" => {
                self.emit(&format!("call void @rt_semaphore_destroy(ptr {recv})"));
                ("void".into(), String::new())
            }
            _ => return None,
        };
        Some(result)
    }

    /// Monitor facade (RFC 009 M5.5).
    pub(super) fn try_emit_monitor_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let result: TyVal = match method {
            "Enter" => {
                let (_, obj) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!("call void @rt_monitor_enter(ptr {obj})"));
                ("void".into(), String::new())
            }
            "Exit" => {
                let (_, obj) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!("call void @rt_monitor_exit(ptr {obj})"));
                ("void".into(), String::new())
            }
            "TryEnter" => {
                let (_, obj) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                if args.len() >= 2 {
                    let (_, _ms) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_monitor_try_enter(ptr {obj})"
                    ));
                    ("i32".into(), tmp)
                } else {
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_monitor_try_enter(ptr {obj})"
                    ));
                    ("i32".into(), tmp)
                }
            }
            "Wait" => {
                let (_, obj) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!("call void @rt_monitor_wait(ptr {obj})"));
                ("void".into(), String::new())
            }
            "Pulse" => {
                let (_, obj) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!("call void @rt_monitor_pulse(ptr {obj})"));
                ("void".into(), String::new())
            }
            "PulseAll" => {
                let (_, obj) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!("call void @rt_monitor_pulse_all(ptr {obj})"));
                ("void".into(), String::new())
            }
            _ => return None,
        };
        Some(result)
    }

    /// Interlocked facade (RFC 009 §7.5) — LLVM `atomicrmw` / `cmpxchg`（seq_cst）。
    ///
    /// C# 语义：`Increment` 返回**新**值；`Exchange` / `CompareExchange` 返回**旧**值。
    /// `ref int` 实参经 MIR `AddrOf`；`emit_operand` 得 `ptr`。
    pub(super) fn try_emit_interlocked_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let loc = {
            let a = args.first()?;
            let (ty, val) = self.emit_operand(a);
            if ty != "ptr" {
                return None;
            }
            val
        };
        match method {
            "Increment" => {
                let old = self.fresh_temp();
                self.emit(&format!("{old} = atomicrmw add ptr {loc}, i32 1 seq_cst"));
                let neu = self.fresh_temp();
                self.emit(&format!("{neu} = add i32 {old}, 1"));
                Some(("i32".into(), neu))
            }
            "Decrement" => {
                let old = self.fresh_temp();
                self.emit(&format!("{old} = atomicrmw sub ptr {loc}, i32 1 seq_cst"));
                let neu = self.fresh_temp();
                self.emit(&format!("{neu} = sub i32 {old}, 1"));
                Some(("i32".into(), neu))
            }
            "Exchange" => {
                let (_, val) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let old = self.fresh_temp();
                self.emit(&format!(
                    "{old} = atomicrmw xchg ptr {loc}, i32 {val} seq_cst"
                ));
                Some(("i32".into(), old))
            }
            "CompareExchange" => {
                let (_, value) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, comparand) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let pair = self.fresh_temp();
                self.emit(&format!(
                    "{pair} = cmpxchg ptr {loc}, i32 {comparand}, i32 {value} seq_cst seq_cst"
                ));
                let old = self.fresh_temp();
                self.emit(&format!("{old} = extractvalue {{ i32, i1 }} {pair}, 0"));
                Some(("i32".into(), old))
            }
            _ => None,
        }
    }

    /// ThreadPoolScheduler facade (RFC 009 M5.7).
    pub(super) fn try_emit_threadpool_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let result: TyVal = match method {
            "Run" => {
                // 与 Task.Run 相同：FnPtr 不得对函数符号做 {ptr,ptr} GEP。
                let action = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                let (fn_val, data_val) = self.emit_task_run_fn_env(&action);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_task_run_on_pool(ptr {recv}, ptr {fn_val}, ptr {data_val})"
                ));
                ("ptr".into(), tmp)
            }
            "Shutdown" => {
                self.emit(&format!("call void @rt_threadpool_shutdown(ptr {recv})"));
                ("void".into(), String::new())
            }
            "ShutdownDefaultPool" => {
                // Static; receiver operand unused (null / dummy).
                self.emit("call void @rt_default_pool_shutdown()");
                ("void".into(), String::new())
            }
            "Destroy" => {
                // Safe destroy：rt_threadpool_destroy（wait_idle + join 跳过已 Shutdown + free）。
                // 池句柄非 ArcBox；codegen Drop 已跳过 ThreadPoolScheduler（见 arc_drop.rs）。
                self.emit(&format!("call void @rt_threadpool_destroy(ptr {recv})"));
                ("void".into(), String::new())
            }
            "get_PendingTaskCount" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_threadpool_pending_count(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            "get_ActiveWorkerCount" => {
                // 配置的 worker 数（非瞬时 busy 计数）；ABI = rt_threadpool_worker_count。
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_threadpool_worker_count(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// Parallel.For emission (RFC 009 / RFC 009 M5.7).
    pub(super) fn emit_parallel_for(&mut self, args: &[MirOperand]) -> TyVal {
        let (_, from) =
            self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
        let (_, to) = self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));

        let (body_operand, pool, cts, max_degree) = match args.len() {
            3 => (
                args[2].clone(),
                "null".to_string(),
                "null".to_string(),
                "0".to_string(),
            ),
            4 => {
                let (_, options_ptr) = self.emit_operand(&args[2]);
                let (sched_off, _) = self.field_info("ParallelOptions", "Scheduler");
                let (mdp_off, _) = self.field_info("ParallelOptions", "MaxDegreeOfParallelism");
                let (cts_off, _) = self.field_info("ParallelOptions", "CancellationToken");
                let sched_addr = self.fresh_temp();
                let mdp_addr = self.fresh_temp();
                let cts_addr = self.fresh_temp();
                self.emit(&format!(
                    "{sched_addr} = getelementptr inbounds i8, ptr {options_ptr}, i32 {sched_off}"
                ));
                self.emit(&format!(
                    "{mdp_addr} = getelementptr inbounds i8, ptr {options_ptr}, i32 {mdp_off}"
                ));
                self.emit(&format!(
                    "{cts_addr} = getelementptr inbounds i8, ptr {options_ptr}, i32 {cts_off}"
                ));
                let sched_val = self.fresh_temp();
                let mdp_val = self.fresh_temp();
                let cts_val = self.fresh_temp();
                self.emit(&format!("{sched_val} = load ptr, ptr {sched_addr}"));
                self.emit(&format!("{mdp_val} = load i32, ptr {mdp_addr}"));
                self.emit(&format!("{cts_val} = load ptr, ptr {cts_addr}"));
                (args[3].clone(), sched_val, cts_val, mdp_val)
            }
            _ => (
                args.get(2).cloned().unwrap_or(MirOperand::ConstNull),
                "null".to_string(),
                "null".to_string(),
                "0".to_string(),
            ),
        };

        let (_, body_ptr) = self.emit_operand(&body_operand);
        let fn_tmp = self.fresh_temp();
        let data_tmp = self.fresh_temp();
        self.emit(&format!(
            "{fn_tmp} = getelementptr inbounds {{ptr, ptr}}, ptr {body_ptr}, i32 0, i32 0"
        ));
        self.emit(&format!(
            "{data_tmp} = getelementptr inbounds {{ptr, ptr}}, ptr {body_ptr}, i32 0, i32 1"
        ));
        let fn_val = self.fresh_temp();
        let data_val = self.fresh_temp();
        self.emit(&format!("{fn_val} = load ptr, ptr {fn_tmp}"));
        self.emit(&format!("{data_val} = load ptr, ptr {data_tmp}"));

        // runtime `rt_parallel_for` 以 `body(int32_t i, void* env)` 调用回调——
        // i 按值、env 在末位。而 Arc 闭包 ABI 为 `fn(env, idx_ptr)`（env 首位、
        // 参数按指针）。直接传闭包 fn 会因参数顺序与传参方式错配导致 AV（严重）。
        // 故为此调用点生成 trampoline：接收 `(i, env)`，将 `i` 存入栈槽后以
        // 指针传给 `fn(env, idx)`，vanilla 对齐 `fn(env, idx_ptr)` 约定。
        let tramp_name = format!("__parallel_for_tramp_{}", self.parallel_for_tramp_counter);
        self.parallel_for_tramp_counter += 1;

        // 调用点 env 结构：{ ptr user_fn, ptr user_env }，供 trampoline 载荷。
        let env_ptr = self.fresh_temp();
        self.emit(&format!("{env_ptr} = alloca {{ptr, ptr}}"));
        let env_fn_addr = self.fresh_temp();
        self.emit(&format!(
            "{env_fn_addr} = getelementptr inbounds {{ptr, ptr}}, ptr {env_ptr}, i32 0, i32 0"
        ));
        self.emit(&format!("store ptr {fn_val}, ptr {env_fn_addr}"));
        let env_user_env_addr = self.fresh_temp();
        self.emit(&format!(
            "{env_user_env_addr} = getelementptr inbounds {{ptr, ptr}}, ptr {env_ptr}, i32 0, i32 1"
        ));
        self.emit(&format!("store ptr {data_val}, ptr {env_user_env_addr}"));

        // trampoline：`(i32 %i, ptr %env)` → `fn(env, &i)`（Arc 闭包 ABI）。
        // 每次调用点生成独立函数，经 `try_push` 模块级去重（TLS 无状态）。
        let tramp_ir = format!(
            "define void @{tramp_name}(i32 %i, ptr %env) {{\n\
             entry:\n\
             %fn_slot = getelementptr inbounds {{ptr, ptr}}, ptr %env, i32 0, i32 0\n\
             %fn = load ptr, ptr %fn_slot\n\
             %ue_slot = getelementptr inbounds {{ptr, ptr}}, ptr %env, i32 0, i32 1\n\
             %ue = load ptr, ptr %ue_slot\n\
             %idx_slot = alloca i32\n\
             store i32 %i, ptr %idx_slot\n\
             call void %fn(ptr %ue, ptr %idx_slot)\n\
             ret void\n\
             }}\n\n"
        );
        self.native_trampolines.try_push(&tramp_name, tramp_ir);

        let completed = self.fresh_temp();
        self.emit(&format!(
            "{completed} = call i32 @rt_parallel_for(i32 {from}, i32 {to}, ptr @{tramp_name}, ptr {env_ptr}, ptr {pool}, ptr {cts}, i32 {max_degree})"
        ));

        let result_ptr = self.fresh_temp();
        self.emit(&format!("{result_ptr} = alloca %struct.ParallelResult"));
        self.emit(&format!("store i32 {completed}, ptr {result_ptr}"));

        ("ptr".into(), result_ptr)
    }

    /// Parallel.ForEach emission (RFC 009 M6).
    ///
    /// 将 `Parallel.ForEach<T>(source, body)` / `Parallel.ForEach<T>(source, options, body)`
    /// 发射为 `@rt_parallel_foreach` ABI 调用。
    ///
    /// **实现策略**：
    /// - source 数组通过 `@rt_array_length` 获取长度
    /// - body 闭包 `{fn_ptr, env}` 提取为独立参数
    /// - 生成 trampoline 函数 `__foreach_tramp_N`，接收 `(i, elem_ptr, env)`，
    ///   调用 body 闭包 `user_fn(elem_ptr, user_env)`——传递元素指针
    /// - trampoline 通过 `native_trampolines` 累积器在模块级发射
    ///
    /// **多平台说明**：ABI 平台无关；底层线程池/信号量由 rt_threadpool 提供。
    ///
    /// **当前限制（M6 MVP）**：trampoline 传递元素指针（`ptr`）。
    /// 对引用类型（class/string/array）完全正确。
    /// 对值类型（int/float/struct），body 闭包期望按值接收 T——当前 MVP 传指针。
    /// 值类型 ForEach 建议使用 `Parallel.For` + 手动索引。
    pub(super) fn emit_parallel_foreach(&mut self, args: &[MirOperand]) -> TyVal {
        // source 数组指针（第一个参数）
        let (_, array_ptr) =
            self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));

        // 获取数组长度
        let len_tmp = self.fresh_temp();
        self.emit(&format!(
            "{len_tmp} = call i32 @rt_array_length(ptr {array_ptr})"
        ));

        // 提取 body 闭包 + options
        let (body_operand, pool, cts, max_degree) = match args.len() {
            // ForEach(source, body) → 2 args
            2 => (
                args[1].clone(),
                "null".to_string(),
                "null".to_string(),
                "0".to_string(),
            ),
            // ForEach(source, options, body) → 3 args
            3 => {
                let (_, options_ptr) = self.emit_operand(&args[1]);
                let (sched_off, _) = self.field_info("ParallelOptions", "Scheduler");
                let (mdp_off, _) = self.field_info("ParallelOptions", "MaxDegreeOfParallelism");
                let (cts_off, _) = self.field_info("ParallelOptions", "CancellationToken");
                let sched_addr = self.fresh_temp();
                let mdp_addr = self.fresh_temp();
                let cts_addr = self.fresh_temp();
                self.emit(&format!(
                    "{sched_addr} = getelementptr inbounds i8, ptr {options_ptr}, i32 {sched_off}"
                ));
                self.emit(&format!(
                    "{mdp_addr} = getelementptr inbounds i8, ptr {options_ptr}, i32 {mdp_off}"
                ));
                self.emit(&format!(
                    "{cts_addr} = getelementptr inbounds i8, ptr {options_ptr}, i32 {cts_off}"
                ));
                let sched_val = self.fresh_temp();
                let mdp_val = self.fresh_temp();
                let cts_val = self.fresh_temp();
                self.emit(&format!("{sched_val} = load ptr, ptr {sched_addr}"));
                self.emit(&format!("{mdp_val} = load i32, ptr {mdp_addr}"));
                self.emit(&format!("{cts_val} = load ptr, ptr {cts_addr}"));
                (args[2].clone(), sched_val, cts_val, mdp_val)
            }
            _ => (
                args.get(1).cloned().unwrap_or(MirOperand::ConstNull),
                "null".to_string(),
                "null".to_string(),
                "0".to_string(),
            ),
        };

        // 提取 body 闭包 {fn_ptr, env_ptr}
        let (_, body_ptr) = self.emit_operand(&body_operand);
        let fn_tmp = self.fresh_temp();
        let data_tmp = self.fresh_temp();
        self.emit(&format!(
            "{fn_tmp} = getelementptr inbounds {{ptr, ptr}}, ptr {body_ptr}, i32 0, i32 0"
        ));
        self.emit(&format!(
            "{data_tmp} = getelementptr inbounds {{ptr, ptr}}, ptr {body_ptr}, i32 0, i32 1"
        ));
        let user_fn_val = self.fresh_temp();
        let user_env_val = self.fresh_temp();
        self.emit(&format!("{user_fn_val} = load ptr, ptr {fn_tmp}"));
        self.emit(&format!("{user_env_val} = load ptr, ptr {data_tmp}"));

        // 生成 trampoline：接收 (i, elem_ptr, env)，调用 user_fn(elem_ptr, user_env)
        // env 结构 = {ptr user_fn, ptr user_env}
        // trampoline 名按 user_fn_val 去重（同一 body 闭包类型仅生成一次）
        let tramp_name = format!("__foreach_tramp_{}", self.foreach_tramp_counter);
        self.foreach_tramp_counter += 1;

        let tramp_ir = format!(
            "define void @{tramp_name}(i32 %i, ptr %elem_ptr, ptr %env) {{\n\
             entry:\n\
             %user_fn_ptr = getelementptr inbounds {{ptr, ptr}}, ptr %env, i32 0, i32 0\n\
             %user_fn = load ptr, ptr %user_fn_ptr\n\
             %user_env_ptr = getelementptr inbounds {{ptr, ptr}}, ptr %env, i32 0, i32 1\n\
             %user_env = load ptr, ptr %user_env_ptr\n\
             call void %user_fn(ptr %elem_ptr, ptr %user_env)\n\
             ret void\n\
             }}\n\n"
        );
        self.native_trampolines.try_push(&tramp_name, tramp_ir);

        // 分配 env 结构 {ptr user_fn, ptr user_env} 在栈上
        let env_ptr = self.fresh_temp();
        self.emit(&format!("{env_ptr} = alloca {{ptr, ptr}}"));
        let env_fn_addr = self.fresh_temp();
        let env_user_env_addr = self.fresh_temp();
        self.emit(&format!(
            "{env_fn_addr} = getelementptr inbounds {{ptr, ptr}}, ptr {env_ptr}, i32 0, i32 0"
        ));
        self.emit(&format!(
            "{env_user_env_addr} = getelementptr inbounds {{ptr, ptr}}, ptr {env_ptr}, i32 0, i32 1"
        ));
        self.emit(&format!("store ptr {user_fn_val}, ptr {env_fn_addr}"));
        self.emit(&format!(
            "store ptr {user_env_val}, ptr {env_user_env_addr}"
        ));

        // 调用 @rt_parallel_foreach(array_ptr, len, tramp_fn, env_ptr, pool, cts, max_degree)
        let completed = self.fresh_temp();
        self.emit(&format!(
            "{completed} = call i32 @rt_parallel_foreach(ptr {array_ptr}, i32 {len_tmp}, ptr @{tramp_name}, ptr {env_ptr}, ptr {pool}, ptr {cts}, i32 {max_degree})"
        ));

        let result_ptr = self.fresh_temp();
        self.emit(&format!("{result_ptr} = alloca %struct.ParallelResult"));
        self.emit(&format!("store i32 {completed}, ptr {result_ptr}"));

        ("ptr".into(), result_ptr)
    }
}
