//! Call, method-call, new-object, and indirect-call emission.

use super::*;
use ast::TypeId;
use mir::MirOperand;

impl<'a> FnEmitter<'a> {
    // ---- Function calls ----

    pub(super) fn emit_call(&mut self, func: &str, args: &[MirOperand]) -> TyVal {
        self.emit_call_typed(func, args, &TypeId::Int)
    }

    pub(super) fn emit_call_typed(
        &mut self,
        func: &str,
        args: &[MirOperand],
        expected: &TypeId,
    ) -> TyVal {
        // [Builtin] dispatch registry — 统一入口，替换原有 if-else 字符串匹配链。
        if let Some(result) = self.try_emit_builtin_static(func, args, expected) {
            return result;
        }

        // RFC 037 M3 UI Element Tree ABI — WindowHost.* batch dispatch.
        if let Some(result) = self.try_emit_window_host_element(func, args) {
            return result;
        }

        // RFC 004 M1：基元类型 static abstract 调用（int.Add, double.Multiply 等）。
        if let Some(result) = self.try_emit_primitive_static(func, args) {
            return result;
        }

        // RFC 016：`Native.IsAvailable("<module>")` 内联发射——编译期常量模块名
        // 的可用性查询（触发一次懒解析并读取 per-module 状态，返回 i1）。
        // 非字面量参数按「未知模块不可用」（false）处理（语义下限，见 RFC 016
        // 差异记录）。std `Native::IsAvailable` 定义体不会被调用（本路径拦截）。
        // 静态方法调用在 MIR 中的 func 形如 `Native::IsAvailable`（`::` 分隔）；
        // `emit_method_call_typed` 侧另有 receiver_type 检查作对偶。
        if func == "Native.IsAvailable" || func == "Native::IsAvailable" {
            if let Some(MirOperand::ConstString(module)) = args.first() {
                let flag = self.emit_native_availability(module);
                return ("i1".into(), flag);
            }
            return ("i1".into(), "false".into());
        }

        // RFC 016 M1/M2/M3：Native contract call（libc::puts → @puts）。
        let native_key = func.replace("::", ".");
        if let Some(result) = self.try_emit_native_call(&native_key, args) {
            return result;
        }
        // Fallback: try unqualified name (e.g. "rt_os_now_ticks" → "rt_resources.rt_os_now_ticks").
        if native_key == func {
            // `func` has no module prefix; try matching against all `module.fn` keys.
            for key in self.native_symbols.keys() {
                if let Some(suffix) = key.split('.').next_back() {
                    if suffix == native_key {
                        if let Some(result) = self.try_emit_native_call(key, args) {
                            return result;
                        }
                        break;
                    }
                }
            }
        }

        // User function / runtime call ? fallthrough.
        // RFC 018: `rt_obj_isa` ABI returns i32 (0/1); Bool 需 icmp 规范化。
        // `call i1 @rt_obj_isa` 与 declare i32 不一致时由下方修正。
        if func == "rt_obj_isa" {
            let arg_strs: Vec<String> = args
                .iter()
                .map(|a| {
                    let (ty, val) = self.emit_operand(a);
                    format!("{ty} {val}")
                })
                .collect();
            let tmp = self.fresh_temp();
            self.emit(&format!(
                "{tmp} = call i32 @rt_obj_isa({})",
                arg_strs.join(", ")
            ));
            if matches!(expected, TypeId::Bool) {
                let flag = self.fresh_temp();
                self.emit(&format!("{flag} = icmp ne i32 {tmp}, 0"));
                return ("i1".into(), flag);
            }
            return ("i32".into(), tmp);
        }

        let mangled = mangle_fn_name(func);
        let ret_ty = if self.async_fns.contains(func) {
            "ptr".into()
        } else if let Some(ret_ty) = self.fn_returns.get(func) {
            llvm_type_of(ret_ty, self.layouts)
        } else {
            llvm_type_of(expected, self.layouts)
        };
        // RFC 007 M2c / RFC 008：`__lambda_*` 形参 ABI 为 ptr（见 emit_sync_function）。
        // RFC 008：委托实参（Func/Action / FnPtr / Closure）统一传 arc_closure*，
        // 与形参侧 closure_locals + emit_closure_indirect_call 对齐。
        let lambda_ptr_abi = func.starts_with("__lambda_");
        let arg_strs: Vec<String> = args
            .iter()
            .map(|a| {
                let (ty, val) = if self.operand_is_delegate_value(a) {
                    self.emit_operand_as_closure(a)
                } else {
                    self.emit_operand(a)
                };
                if lambda_ptr_abi && ty != "ptr" {
                    let slot = self.fresh_temp();
                    self.emit(&format!("{slot} = alloca {ty}"));
                    self.emit(&format!("store {ty} {val}, ptr {slot}"));
                    format!("ptr {slot}")
                } else {
                    format!("{ty} {val}")
                }
            })
            .collect();
        if ret_ty == "void" {
            let may_throw = self.callee_may_throw(func);
            self.emit_call_may_throw(
                "void",
                &format!("@{mangled}"),
                &arg_strs.join(", "),
                may_throw,
                None,
            );
            ("void".into(), String::new())
        } else {
            let tmp = self.fresh_temp();
            let may_throw = self.callee_may_throw(func);
            self.emit_call_may_throw(
                &ret_ty,
                &format!("@{mangled}"),
                &arg_strs.join(", "),
                may_throw,
                Some(&tmp),
            );
            // M6.2 协程暖启动（对标 .NET async 同步前缀）：async Arc 函数调用点
            // 立即首 poll 驱动 body 同步前缀至首个未完成 await，打破「create N →
            // await each」串行化——任务创建即开始执行，多任务可并行推进。仅对
            // Arc async 函数（协程/状态机入口）发射；运行时 Task-returning 面
            // （IO/Delay 等）不发射——其任务无 resume 帧，poll 恒经状态读，越界
            // resume 风险不存在，且置守卫位反而可能制造「注册即已完成」挂起。
            if self.async_fns.contains(func) {
                self.emit(&format!("call void @rt_task_autostart(ptr {tmp})"));
            }
            (ret_ty, tmp)
        }
    }

    // ---- Object creation ----

    /// ctor 符号（与 typeck 定义侧 `ctor_link_name` 决策一致）。
    ///
    /// 无参 ctor 恒为 `__ctor::Class`；有参 ctor 默认 `__ctor::Class_<arity>`；
    /// 当 `ctor_params` 非空（MIR lower 已判定同参数量碰撞）时按签名
    /// `__ctor::Class_<arity>_<p0>_<p1>...` 消歧——否则 `C(int)` / `C(string)`
    /// 两 ctor 均 mangle 为 `__ctor::C_1`，后者覆盖前者 → 调用方按错误签名
    /// 执行 → AV。与 check_class 的 mangle 决策共享同一规则，保证定义/调用一致。
    fn ctor_mangle(&self, class: &str, args_len: usize, ctor_params: &[String]) -> String {
        if ctor_params.is_empty() {
            if args_len == 0 {
                format!("__ctor::{class}")
            } else {
                format!("__ctor::{class}_{args_len}")
            }
        } else {
            format!(
                "__ctor::{class}_{}_{}",
                ctor_params.len(),
                ctor_params.join("_")
            )
        }
    }

    pub(super) fn emit_new(
        &mut self,
        class: &str,
        args: &[MirOperand],
        ctor_params: &[String],
    ) -> TyVal {
        // Vector<T, N> is a value type (RFC 021 Phase 2): stack-allocated, no ARC,
        // no runtime ABI. Intercept the mangled name "Vector_{elem}_{n}" and emit
        // a zero-initialized vector constant instead of malloc + ctor.
        if let Some((elem_ty, n)) = parse_vector_class(class) {
            let vec_ty = format!("<{n} x {elem_ty}>");
            return (vec_ty, "zeroinitializer".into());
        }

        // StringBuilder facade (RFC 021 §4.3 M4): intercept `new StringBuilder()` /
        // `new StringBuilder(string)` / `new StringBuilder(int capacity)` to
        // allocate the Arc object and call the corresponding rt_text_sb_new_* ABI.
        // The generic calloc+__ctor path is bypassed (the ctor stub only handles
        // the no-arg case); multi-arg ctors are handled here.
        if class == "StringBuilder" {
            let handle = self.fresh_temp();
            if args.is_empty() {
                self.emit(&format!("{handle} = call ptr @rt_text_sb_new()"));
            } else {
                let (arg_ty, arg_val) = self.emit_operand(&args[0]);
                match arg_ty.as_str() {
                    "ptr" => {
                        self.emit(&format!(
                            "{handle} = call ptr @rt_text_sb_new_with_str(ptr {arg_val})"
                        ));
                    }
                    "i32" => {
                        self.emit(&format!(
                            "{handle} = call ptr @rt_text_sb_new_with_capacity(i32 {arg_val})"
                        ));
                    }
                    _ => {
                        self.emit(&format!("{handle} = call ptr @rt_text_sb_new()"));
                    }
                }
            }
            let size = self.class_size(class);
            let obj = self.fresh_temp();
            self.emit(&format!("{obj} = call ptr @calloc(i64 1, i64 {size})"));
            self.emit(&format!("store i32 1, ptr {obj}")); // refcount
            if let Some(vt) = self.vtable_global(class) {
                let vtbl_addr = self.fresh_temp();
                self.emit(&format!(
                    "{vtbl_addr} = getelementptr inbounds i8, ptr {obj}, i64 8"
                ));
                self.emit(&format!("store ptr {vt}, ptr {vtbl_addr}"));
            }
            let hp = self.fresh_temp();
            self.emit(&format!(
                "{hp} = getelementptr inbounds i8, ptr {obj}, i32 16"
            ));
            self.emit(&format!("store ptr {handle}, ptr {hp}"));
            return ("ptr".into(), obj);
        }

        // 运行时门面 new（Lock/Mutex/Semaphore/CancellationToken(Source)/
        // ThreadPoolScheduler/TaskCompletionSource*）——统一收敛到唯一事实来源
        // types::runtime_facade_new_spec：对象即裸句柄，禁止 calloc+vtable+ctor。
        if let Some((target, arity)) = crate::llvm_ir::types::runtime_facade_new_spec(class) {
            let tmp = self.fresh_temp();
            if arity == 0 {
                self.emit(&format!("{tmp} = call ptr {target}"));
            } else {
                // i32×2：Semaphore(init, max=1) / ThreadPoolScheduler(workers, numa)。
                let (_, a0) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, a1) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(1)));
                self.emit(&format!("{tmp} = call ptr {target}(i32 {a0}, i32 {a1})"));
            }
            return ("ptr".into(), tmp);
        }

        if class == "Thread" {
            // new Thread(action) → @rt_thread_handle_create(fn, data)
            // Action 须为 arc_closure*（裸 FnPtr 包装）；再提取 fn / env。
            let action = args.first().cloned().unwrap_or(MirOperand::ConstNull);
            let (_, action_ptr) = if self.operand_is_delegate_value(&action) {
                self.emit_operand_as_closure(&action)
            } else {
                self.emit_operand(&action)
            };
            let fn_tmp = self.fresh_temp();
            let data_tmp = self.fresh_temp();
            self.emit(&format!(
                "{fn_tmp} = getelementptr inbounds {{ptr, ptr}}, ptr {action_ptr}, i32 0, i32 0"
            ));
            self.emit(&format!(
                "{data_tmp} = getelementptr inbounds {{ptr, ptr}}, ptr {action_ptr}, i32 0, i32 1"
            ));
            let fn_val = self.fresh_temp();
            let data_val = self.fresh_temp();
            self.emit(&format!("{fn_val} = load ptr, ptr {fn_tmp}"));
            self.emit(&format!("{data_val} = load ptr, ptr {data_tmp}"));
            let tmp = self.fresh_temp();
            self.emit(&format!(
                "{tmp} = call ptr @rt_thread_handle_create(ptr {fn_val}, ptr {data_val})"
            ));
            return ("ptr".into(), tmp);
        }

        // Arc.Net network facade (RFC 025 M4)：`new Tcp*/Socket/Udp*` → `@rt_socket_create`。
        // 实例方法（`try_emit_socket_method`）把 receiver 当作 `RtSocket*` 直接传 ABI——
        // 与 Mutex/Thread 同形；禁止走 calloc+空 stub ctor（否则 Start/Connect 恒失败）。
        if matches!(class, "TcpClient" | "TcpListener" | "Socket" | "UdpClient") {
            let tmp = self.fresh_temp();
            match class {
                "Socket" => {
                    let (_, af) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let (_, st) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let (_, proto) =
                        self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_socket_create(i32 {af}, i32 {st}, i32 {proto})"
                    ));
                }
                "UdpClient" => {
                    // UdpClient() → Dgram+Udp；UdpClient(int port) → create 后 bind。
                    // arity-1 与 AddressFamily 重载冲突：本刀按 port 语义 bind（不扩 UDP e2e）。
                    let af = "0";
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_socket_create(i32 {af}, i32 1, i32 1)"
                    ));
                    if args.len() == 1 {
                        let (_, port) = self.emit_operand(&args[0]);
                        let _bind = self.fresh_temp();
                        self.emit(&format!(
                            "{_bind} = call i32 @rt_socket_bind(ptr {tmp}, i32 {port})"
                        ));
                    }
                }
                _ => {
                    // TcpClient / TcpListener：Stream + Tcp；可选 AddressFamily。
                    let (_, af) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_socket_create(i32 {af}, i32 0, i32 0)"
                    ));
                }
            }
            return ("ptr".into(), tmp);
        }

        // RFC 048: `new NamedPipeServerStream(name[, maxInstances])` →
        // `@rt_pipe_server_create`；`new NamedPipeClientStream(name)` →
        // `@rt_pipe_client_create`（未连接壳，Connect(timeout) 走
        // rt_pipe_client_connect）。与 socket 同形：禁止 calloc+空 stub ctor。
        if matches!(class, "NamedPipeServerStream" | "NamedPipeClientStream") {
            let tmp = self.fresh_temp();
            if class == "NamedPipeServerStream" {
                let (_, name) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, max_inst) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(1)));
                self.emit(&format!(
                    "{tmp} = call ptr @rt_pipe_server_create(ptr {name}, i32 {max_inst})"
                ));
            } else {
                let (_, name) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                self.emit(&format!("{tmp} = call ptr @rt_pipe_client_create(ptr {name})"));
            }
            return ("ptr".into(), tmp);
        }

        // RFC 024 M7：`new BlockingCollection<T>(IConcurrentCollection, int)`。
        // ctor 仅按 arity mangle（`_2`），与 arity-1 `create(capacity)` stub 签名交叉；
        // 在此按实参静态类型分派 create_with(kind)，绕过通用 __ctor 路径。
        if class.starts_with("BlockingCollection_") && args.len() == 2 {
            return self.emit_blocking_collection_pcc_new(class, args);
        }

        // List<T> facade (P5-H): intercept new List<T>() and new List<T>(capacity).
        // Generic constructor path would call the no-arg stub with wrong signature
        // for capacity overload, so handle both cases here like StringBuilder.
        if let Some(elem_suf) = parse_list_elem(class) {
            let elem_size = list_elem_size(elem_suf, self.layouts);
            let eq_fn = match list_eq_fn(elem_suf) {
                Some(f) => format!("ptr {f}"),
                None => "ptr null".to_string(),
            };
            let arc_inc = match list_arc_inc_fn(elem_suf, self.layouts) {
                Some(f) => format!("ptr {f}"),
                None => "ptr null".to_string(),
            };
            let arc_dec = match list_arc_dec_fn(elem_suf, self.layouts) {
                Some(f) => format!("ptr {f}"),
                None => "ptr null".to_string(),
            };
            let handle = self.fresh_temp();
            if args.is_empty() {
                self.emit(&format!(
                    "{handle} = call ptr @rt_list_create(i32 {elem_size}, {eq_fn}, {arc_inc}, {arc_dec})"
                ));
            } else {
                let (_, cap) = self.emit_operand(&args[0]);
                self.emit(&format!(
                    "{handle} = call ptr @rt_list_create_with_capacity(i32 {elem_size}, i32 {cap}, {eq_fn}, {arc_inc}, {arc_dec})"
                ));
            }
            let size = self.class_size(class);
            let obj = self.fresh_temp();
            self.emit(&format!("{obj} = call ptr @calloc(i64 1, i64 {size})"));
            self.emit(&format!("store i32 1, ptr {obj}"));
            if let Some(vt) = self.vtable_global(class) {
                let vtbl_addr = self.fresh_temp();
                self.emit(&format!(
                    "{vtbl_addr} = getelementptr inbounds i8, ptr {obj}, i64 8"
                ));
                self.emit(&format!("store ptr {vt}, ptr {vtbl_addr}"));
            }
            let hp = self.fresh_temp();
            self.emit(&format!(
                "{hp} = getelementptr inbounds i8, ptr {obj}, i32 16"
            ));
            self.emit(&format!("store ptr {handle}, ptr {hp}"));
            return ("ptr".into(), obj);
        }

        // Struct value types: heap-allocate via calloc (no ARC header, no vtable).
        // 先前 alloca 时工厂方法 `ret ptr` 指向销毁中的栈帧 → 悬空指针。
        // 当前 ABI 以 ptr 传递 struct；返回值须指向逃逸后仍存活的存储。
        if self.layouts.structs.contains_key(class) {
            let size = self.layouts.size_of_ty(class) as i64;
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = call ptr @calloc(i64 1, i64 {size})"));
            // ctor 重载 mangle：与 class 路径一致，无参用 `__ctor::Class`；
            // 有参用 `__ctor::Class_<arity>`；MIR 判定同参数量碰撞时
            // （`ctor_params` 非空）按签名 `__ctor::Class_<arity>_<p0>...` 消歧。
            let ctor_key = self.ctor_mangle(class, args.len(), ctor_params);
            let ctor_name = mangle_fn_name(&ctor_key);
            let arg_strs: Vec<String> = args
                .iter()
                .map(|a| {
                    let (ty, val) = if self.operand_is_delegate_value(a) {
                        self.emit_operand_as_closure(a)
                    } else {
                        self.emit_operand(a)
                    };
                    format!("{ty} {val}")
                })
                .collect();
            let call_args = if arg_strs.is_empty() {
                format!("ptr {tmp}")
            } else {
                format!("ptr {tmp}, {}", arg_strs.join(", "))
            };
            let may_throw = self.callee_may_throw(&ctor_key);
            self.emit_call_may_throw(
                "void",
                &format!("@{ctor_name}"),
                &call_args,
                may_throw,
                None,
            );
            return ("ptr".into(), tmp);
        }

        let size = self.class_size(class);
        let tmp = self.fresh_temp();
        // calloc zero-initializes — ensures class-typed fields start as null so
        // FieldSet's dec-old and the drop sequence's field-dec are safe (no-op
        // on null). Without this, uninitialized garbage pointers would crash
        // rt_arc_dec when the drop sequence runs.
        self.emit(&format!("{tmp} = call ptr @calloc(i64 1, i64 {size})"));

        // Init refcount = 1 at offset 0
        self.emit(&format!("store i32 1, ptr {tmp}"));

        // Init vtable at offset 8。外部类 vtable 在本 TU 不发射（`vtable_global`
        // 守卫返回 None，见 emit_aggregate::vtable_global 契约）——发射 `null`
        // 而非悬空的 `@.vtable.{class}`（泛型模板体如 `Enum.GetOptions<T>()`
        // 以 linkonce_odr 弱符号发射时引用外部类 `EnumOptions_T` 的 vtable，
        // 若直接内联 `@.vtable.{class}` 会因未定义导致 clang IR 编译失败）。
        if let Some(vt) = self.vtable_global(class) {
            let vtbl_addr = self.fresh_temp();
            self.emit(&format!(
                "{vtbl_addr} = getelementptr inbounds i8, ptr {tmp}, i64 8"
            ));
            self.emit(&format!("store ptr {vt}, ptr {vtbl_addr}"));
        }

        // object（根类）无用户字段，其 ctor 为空——calloc + refcount + vtable 已在
        // 上方完成，直接 new object() 无须调用 `@__ctor_object`（该符号无 MIR body 亦
        // 非 stub 类，语义级裁剪后无定义 → LLVM undefined / `arc-prune-001`）。提前
        // 返回，语义与 .NET `object` 的空构造函数一致。
        if class == "object" {
            return ("ptr".into(), tmp);
        }

        // RFC 037 M3 UI / RFC 009 L1：无显式 `: base()` 的合成 ctor 为空 stub；
        // 须按继承链自根至直接基类依次调用无参 __ctor::Ancestor（如 Element 初始化
        // Children），再调用目标 ctor（含 new 实参）。
        for ancestor in self
            .class_ancestors_base_first(class)
            .into_iter()
            .filter(|a| a != class)
        {
            let base_ctor_key = format!("__ctor::{ancestor}");
            let base_ctor = mangle_fn_name(&base_ctor_key);
            let may_throw = self.callee_may_throw(&base_ctor_key);
            self.emit_call_may_throw(
                "void",
                &format!("@{base_ctor}"),
                &format!("ptr {tmp}"),
                may_throw,
                None,
            );
        }

        // Call constructor
        // ctor 重载 mangle：无参用 `__ctor::Class`；有参用 `__ctor::Class_<arity>`；
        // MIR 判定同参数量碰撞时（`ctor_params` 非空）按签名
        // `__ctor::Class_<arity>_<p0>...` 消歧。
        // 与 typeck check_class 的 push_typed_fn 命名逻辑保持一致。
        let ctor_key = self.ctor_mangle(class, args.len(), ctor_params);
        let ctor_name = mangle_fn_name(&ctor_key);
        let arg_strs: Vec<String> = args
            .iter()
            .map(|a| {
                // RFC 008：ctor 形参若为 Func/Action，裸 FnPtr 须包装为
                // arc_closure*（否则 Lazy(factory) 字段存储后 GEP 代码地址 → AV）。
                let (ty, val) = if self.operand_is_delegate_value(a) {
                    self.emit_operand_as_closure(a)
                } else {
                    self.emit_operand(a)
                };
                format!("{ty} {val}")
            })
            .collect();
        let call_args = if arg_strs.is_empty() {
            format!("ptr {tmp}")
        } else {
            format!("ptr {tmp}, {}", arg_strs.join(", "))
        };
        let may_throw = self.callee_may_throw(&ctor_key);
        self.emit_call_may_throw(
            "void",
            &format!("@{ctor_name}"),
            &call_args,
            may_throw,
            None,
        );

        // RFC 023 M1: 方式1 ServiceDescriptor 构造 → 注入编译期工厂闭包。
        // ServiceDescriptor 类通过正常 calloc + __ctor 路径构造后，
        // codegen 检测到 typeof(TImpl) 入参时生成 __di_factory_TImpl + 闭包，
        // 内联写入 desc.Factory 字段以备 ServiceProvider 运行时调用。
        self.try_inject_di_factory(class, args, &tmp);

        ("ptr".into(), tmp)
    }

    /// RFC 024 M7：`BlockingCollection(IConcurrentCollection, int)`。
    ///
    /// 从 PCC 对象 offset 16 取 runtime handle，按实参静态类型选 kind，
    /// 调用 `rt_blocking_collection_create_with`。自定义 PCC / 接口擦除变量未宣称。
    fn emit_blocking_collection_pcc_new(&mut self, class: &str, args: &[MirOperand]) -> TyVal {
        let (kind, coll_obj) = self.emit_pcc_collection_operand(&args[0]);
        let inner_addr = self.fresh_temp();
        self.emit(&format!(
            "{inner_addr} = getelementptr inbounds i8, ptr {coll_obj}, i32 16"
        ));
        let inner = self.fresh_temp();
        self.emit(&format!("{inner} = load ptr, ptr {inner_addr}"));
        let (_, cap) = self.emit_operand(&args[1]);
        let handle = self.fresh_temp();
        self.emit(&format!(
            "{handle} = call ptr @rt_blocking_collection_create_with(ptr {inner}, i32 {kind}, i32 {cap}, i32 0)"
        ));

        let size = self.class_size(class);
        let obj = self.fresh_temp();
        self.emit(&format!("{obj} = call ptr @calloc(i64 1, i64 {size})"));
        self.emit(&format!("store i32 1, ptr {obj}"));
        // 与 emit_new 同规则：外部类 vtable 本 TU 不发射时填 null。
        if let Some(vt) = self.vtable_global(class) {
            let vtbl_addr = self.fresh_temp();
            self.emit(&format!(
                "{vtbl_addr} = getelementptr inbounds i8, ptr {obj}, i64 8"
            ));
            self.emit(&format!("store ptr {vt}, ptr {vtbl_addr}"));
        }
        let hp = self.fresh_temp();
        self.emit(&format!(
            "{hp} = getelementptr inbounds i8, ptr {obj}, i32 16"
        ));
        self.emit(&format!("store ptr {handle}, ptr {hp}"));
        ("ptr".into(), obj)
    }

    /// 解析 PCC 实参 → `(kind, object_ptr)`。
    /// 支持：具体 ConcurrentQueue/Bag/Stack；`MirOperand::Iface`（保留 class）。
    fn emit_pcc_collection_operand(&mut self, op: &MirOperand) -> (i32, String) {
        match op {
            MirOperand::Iface { object, class, .. } => {
                let kind = pcc_kind_from_type_name(class).unwrap_or_else(|| {
                    panic!(
                        "BlockingCollection(IConcurrentCollection, int): \
                         unsupported PCC class `{class}` (only ConcurrentQueue/Bag/Stack)"
                    )
                });
                let (_, obj) = self.emit_operand(object);
                (kind, obj)
            }
            MirOperand::Local(id) => {
                let ty = self.local_type(*id);
                let TypeId::Named(n) = &ty else {
                    panic!(
                        "BlockingCollection(IConcurrentCollection, int): \
                         expected named PCC type, got {ty:?}"
                    );
                };
                let name = n.as_str();
                if let Some(kind) = pcc_kind_from_type_name(name) {
                    let (_, obj) = self.emit_operand(op);
                    return (kind, obj);
                }
                if name.starts_with("IConcurrentCollection_") {
                    // 接口擦除局部：fat ptr → obj；kind 无法静态得知。
                    panic!(
                        "BlockingCollection(IConcurrentCollection, int): \
                         interface-typed local `{name}` not supported in this slice \
                         (pass ConcurrentQueue/Bag/Stack; custom PCC 未宣称)"
                    );
                }
                panic!(
                    "BlockingCollection(IConcurrentCollection, int): \
                     unsupported type `{name}` (only ConcurrentQueue/Bag/Stack)"
                );
            }
            _ => {
                // 内联 `new ConcurrentQueue<T>()` 等已物化为 Local；其它形态拒绝。
                panic!(
                    "BlockingCollection(IConcurrentCollection, int): \
                     unsupported operand shape (pass ConcurrentQueue/Bag/Stack local)"
                );
            }
        }
    }

    // ---- Method calls ----

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_method_call(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
        impl_class: Option<&str>,
        target_fn: Option<&str>,
        is_virtual: bool,
        params: &[String],
    ) -> TyVal {
        self.emit_method_call_typed(
            receiver,
            method,
            args,
            receiver_type,
            impl_class,
            target_fn,
            is_virtual,
            params,
            &TypeId::Int,
        )
    }

    /// RFC 017 M2：`Assembly.Entry<T>` 强类型间接调用（gap ①）。
    ///
    /// 按方法身份识别调用点（接收者静态类型 `Assembly` + 方法名 `Entry` +
    /// 参数个数 ≤ 1），降为「符号名内联类型身份」的裸函数指针间接调用。泛型
    /// 实参经 `target_fn`（重载 mangling 的三下划线形
    /// `Assembly::Entry___{TR}` / `Assembly::Entry___{TP}__{TR}`，兼容无重载
    /// 场景的双下划线规范形）提取，计算目标符号名（`type_name_to_id`
    /// FNV-1a-32 类型名哈希 + `entry_layout_signature` FNV-1a-64 布局指纹段，
    /// 与库侧 `emit_entry_wrappers` 同源同值），经 `rt_library_sym(handle,
    /// symbol)` 解析函数指针；`NULL` → `EntryPointNotFoundException`（签名
    /// 安全：符号名即类型身份 + 布局指纹，同名异构在加载期显式失败）；否则
    /// 以统一 C ABI `void* → void*` 发射 `call ptr %fn(...)` 间接调用。
    /// 仅 0 参与单参两种形态；多参编译期拒绝（返回 None）。
    fn try_emit_assembly_entry(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
        target_fn: Option<&str>,
    ) -> Option<TyVal> {
        if receiver_type != "Assembly" || method != "Entry" {
            return None;
        }
        // 边界：仅 0 参与单参 Entry；多参（>1）的 C ABI 编组语义未定义。
        if args.len() > 1 {
            return None;
        }

        // 泛型实参来自 target_fn。0 实参泛型方法被重载 mangling 后模板基底
        // 尾随 `_`（`method_link_name` 对空实参后缀的产出，`Assembly::Entry_`），
        // 与 `__` 分隔符叠成三下划线——与 MIR 侧 `split_mono_name` 同源约定。
        // 先剥三下划线形态再退回双下划线规范形；TR/TP 受 PascalCase 契约约束
        // 不以 `_` 开头，两种解析无歧义。
        let tfn = target_fn?;
        let suffix = tfn
            .strip_prefix("Assembly::Entry___")
            .or_else(|| tfn.strip_prefix("Assembly::Entry__"))?;
        let parts: Vec<&str> = suffix.split("__").collect();
        let (tp_name, tr_name): (Option<String>, String) = match parts.as_slice() {
            [tr] => (None, (*tr).to_string()),
            [tp, tr] => (Some((*tp).to_string()), (*tr).to_string()),
            _ => return None,
        };

        // 符号名内联类型身份（签名安全的基础）：同名类型恒定同 id；布局
        // 指纹段使同名异构（热重载换代布局漂移）在加载期显式
        // EntryPointNotFound，替代 ABI 静默错配——与插件侧
        // emit_entry_wrappers / 导出登记同构（entry_layout_signature 文档）。
        let symbol = match &tp_name {
            Some(tp) => format!(
                "__arc_entry_{}_{}_{}_{}",
                type_name_to_id(tp),
                type_name_to_id(&tr_name),
                entry_layout_signature(self.layouts, tp),
                entry_layout_signature(self.layouts, &tr_name)
            ),
            None => format!(
                "__arc_entry__{}_{}",
                type_name_to_id(&tr_name),
                entry_layout_signature(self.layouts, &tr_name)
            ),
        };

        // 1. 读取接收者 `_handle`（NativePtr，对象头后 offset 16 的首字段）。
        let (_, recv) = self.emit_operand(receiver);
        let handle_addr = self.fresh_temp();
        self.emit(&format!(
            "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

        // 2. 符号名全局常量 → rt_library_sym(handle, symbol)。
        let sym_global = self.string_consts.intern(&symbol);
        let sym_len = symbol.len() + 1;
        let sym_ptr =
            format!("getelementptr inbounds ([{sym_len} x i8], ptr {sym_global}, i32 0, i32 0)");
        let fn_ptr = self.fresh_temp();
        self.emit(&format!(
            "{fn_ptr} = call ptr @rt_library_sym(ptr {handle}, ptr {sym_ptr})"
        ));

        // 3. NULL → EntryPointNotFoundException（签名安全 / 符号缺失 / 库已卸载）。
        let throw_label = self.fresh_label();
        let call_label = self.fresh_label();
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {fn_ptr}, null"));
        self.emit(&format!(
            "br i1 {is_null}, label %{throw_label}, label %{call_label}"
        ));

        self.emit_label(&throw_label);
        let msg = format!("Assembly.Entry: entry point not found: {symbol}");
        let (_, exc_ptr) = self.emit_new(
            "EntryPointNotFoundException",
            &[MirOperand::ConstString(msg)],
            &[],
        );
        self.emit_attach_exception_stacktrace(&exc_ptr);
        self.emit_call_may_throw("void", "@rt_throw", &format!("ptr {exc_ptr}"), true, None);
        self.emit("unreachable");

        self.emit_label(&call_label);

        // 4. 类型化间接调用（统一 C ABI `void* → void*`）。引用类型 ArcHeader* 直传；
        //    值类型指向调用方已布局值的指针（null → NULL，wrapper 内 zero-init 编组）。
        let arg_str = if tp_name.is_some() {
            let (arg_ty, arg_val) = self.emit_operand(&args[0]);
            if arg_ty == "ptr" {
                format!("ptr {arg_val}")
            } else {
                let slot = self.fresh_temp();
                self.emit(&format!("{slot} = alloca {arg_ty}"));
                self.emit(&format!("store {arg_ty} {arg_val}, ptr {slot}"));
                format!("ptr {slot}")
            }
        } else {
            "ptr null".to_string()
        };
        let ret_ptr = self.fresh_temp();
        self.emit(&format!("{ret_ptr} = call ptr {fn_ptr}({arg_str})"));

        // 5. 返回编组（零装箱，不经过 rt_box_*）。
        if self.layouts.classes.contains_key(tr_name.as_str())
            || tr_name == "string"
            || tr_name == "object"
        {
            // 引用类型：ArcHeader* 直传；返回对象进入引用计数域，调用方照常 inc/dec。
            Some(("ptr".into(), ret_ptr))
        } else if self.layouts.structs.contains_key(tr_name.as_str()) {
            // 值类型：指向堆分配拷贝的指针（wrapper 内 malloc + memcpy）→ 读取后释放。
            let val_llvm = format!("%struct.{tr_name}");
            let val = self.fresh_temp();
            self.emit(&format!("{val} = load {val_llvm}, ptr {ret_ptr}"));
            self.emit(&format!("call void @free(ptr {ret_ptr})"));
            Some((val_llvm, val))
        } else {
            // 基元等：库侧不发射对应 wrapper（emit_entry_wrappers 仅处理 Named 类型），
            // 恒在 throw 分支以 EntryPointNotFoundException 终结；此处防御性直传。
            Some(("ptr".into(), ret_ptr))
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_method_call_typed(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
        impl_class: Option<&str>,
        target_fn: Option<&str>,
        is_virtual: bool,
        params: &[String],
        expected: &TypeId,
    ) -> TyVal {
        // 定位公理：未解析的 receiver 禁止静默生成 @unknown_*（链接期才失败）。
        if receiver_type == "unknown" {
            panic!(
                "codegen: method call `{method}` on unresolved receiver type \"unknown\" \
                 (would emit @unknown_{method}); fix MIR type lowering"
            );
        }

        // RFC 017 M2：`Assembly.Entry<T>` 强类型间接调用（gap ①）。
        // 按方法身份识别（接收者静态类型 Assembly + 方法名 Entry + 参数个数），
        // 降级为「符号名内联类型身份」的裸函数指针间接调用。拦截优先级最高——
        // 不允许落到普通方法调用（Entry 方法体为 dead-code facade）。
        if receiver_type == "Assembly" && method == "Entry" {
            if let Some(result) =
                self.try_emit_assembly_entry(receiver, method, args, receiver_type, target_fn)
            {
                return result;
            }
        }

        // [Builtin] dispatch registry — 统一入口。
        if let Some(result) =
            self.try_emit_builtin_method(receiver, method, args, receiver_type, expected)
        {
            return result;
        }

        // Builtin collections/string/StringBuilder — inline IR emission.
        if let Some(result) =
            self.emit_builtin_method_call(receiver, method, args, receiver_type, target_fn)
        {
            return result;
        }

        // Weak<T>.GetWeakSlot stub 内联：模板实例（如 `AssemblyLoadContext
        // RegisterWeakReference<T>`）直接以模板名调用 `Weak_T_GetWeakSlot`，
        // 而模板 stub 无独立 MIR/fns 条目 → 普通 call `undefined value`
        // （`--dynamic` 库 contribution_carrier_dynamic_e2e 实测）。此处内联
        // `weak_stub` GetWeakSlot 等价 IR：读取 offset 16 的 `_target` 槽位
        // （RtWeak* 不透明指针，供 ALC 边界登记）。
        if method == "GetWeakSlot" && receiver_type.starts_with("Weak_") {
            let (_, self_val) = self.emit_operand(receiver);
            let hp = self.fresh_temp();
            self.emit(&format!(
                "{hp} = getelementptr inbounds i8, ptr {self_val}, i32 16"
            ));
            let slot = self.fresh_temp();
            self.emit(&format!("{slot} = load ptr, ptr {hp}"));
            return ("ptr".into(), slot);
        }

        // Interface dispatch: fat pointer { ptr obj, ptr vtable }.
        // Interface-typed locals / params already hold a pointer to `{ptr,ptr}`
        // (from MakeIface / MirOperand::Iface). Never rebuild the fat pointer
        // from `impl_class` here — that double-wraps the alloca address as the
        // object pointer and crashes (0xc00000ff / STATUS_BAD_STACK).
        if is_iface_name(receiver_type) {
            let recv_ptr = match receiver {
                MirOperand::Local(id) => {
                    let lptr = self.local_ptr(*id);
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = load ptr, ptr {lptr}"));
                    tmp
                }
                _ => {
                    let (_, val) = self.emit_operand(receiver);
                    val
                }
            };
            // RFC 006：接口泛型方法经实例化名查 itable 槽位。
            // target_fn 形如 "IGetter::Get__Seed" → slot name "Get__Seed"。
            // 非泛型方法 target_fn 形如 "IGetter::GetValue" → slot name "GetValue"。
            let slot_name = target_fn
                .and_then(|tfn| tfn.rsplit("::").next())
                .unwrap_or(method);
            return self.emit_iface_method_call(
                &recv_ptr,
                slot_name,
                args,
                receiver_type,
                expected,
                params,
            );
        }

        // Native contract call (RFC 016 M1/M2).
        let native_key = format!("{receiver_type}.{method}");
        if let Some(result) = self.try_emit_native_call(&native_key, args) {
            return result;
        }

        // RFC 016：`Native.IsAvailable("<module>")` 内联发射（静态方法分派路径）。
        // 与 `emit_call_typed` 的 `func == "Native.IsAvailable"` 拦截对偶——
        // 静态类方法经 `MirRvalue::MethodCall` 到达此处，非 `emit_call_typed`。
        if receiver_type == "Native" && method == "IsAvailable" {
            if let Some(MirOperand::ConstString(module)) = args.first() {
                let flag = self.emit_native_availability(module);
                return ("i1".into(), flag);
            }
            return ("i1".into(), "false".into());
        }

        // RFC 016 子项 M1：`RefCount.GetRefCount(obj)` 静态调用分发已下沉至
        // `try_emit_builtin_static`（func="RefCount.GetRefCount"），见 builtin_dispatch.rs。
        // 静态方法调用经 `emit_call_typed` 进入，不经过本 instance 路径。

        // RFC 037 M-D0：观察者入口 `ObserveProperty("Name")`——编译器在含
        // `[Observable]` auto-property 的类上合成的实例方法（§5.3）。
        // 调用被展开为隐藏通知通道字段的**静态定址直访**（GEP 常量偏移 +
        // 惰性 `new Signal<T>()`），返回 `Signal<T>` 句柄；与 setter 合成
        // 的 `emit_observable_notify` 共用同一通道槽，绝无运行期字符串查找。
        if method == "ObserveProperty" {
            if let Some(result) = self.try_emit_observable_observe(receiver, args, receiver_type) {
                return result;
            }
        }

        // RFC 037 M-D0：通知侧入口 `NotifyPropertyChanged("Name")`——编译器在
        // 含 `[Observable]` 属性的类上合成的实例方法（§5.3 场景 6，与
        // `ObserveProperty` 对偶：订阅侧 vs 通知侧）。调用被展开为隐藏通道的
        // **显式 raise**：读取通道 `Signal<T>` 实例（惰性 new，复用
        // `emit_observable_channel_lazy`）→ 调 `Signal<T>.Set(当前属性值)`
        //（当前值读取：auto-property 读 backing field、custom-accessor 调
        // 属性 getter）——与 setter 合成 / `ObserveProperty` 共用同一通道槽，
        // 绝无运行期字符串查找。
        if method == "NotifyPropertyChanged" {
            if let Some(result) = self.try_emit_observable_notify(receiver, args, receiver_type) {
                return result;
            }
        }

        let (_, recv) = self.emit_operand(receiver);

        // Virtual dispatch via class vtable
        if is_virtual {
            return self.emit_virtual_call(&recv, method, args, receiver_type, expected, params);
        }

        // Static method call
        let class = impl_class.unwrap_or(receiver_type);
        let (fn_name, fn_key) = if let Some(tfn) = target_fn {
            (mangle_fn_name(tfn), tfn.to_string())
        } else {
            (mangle_method(class, method), format!("{class}::{method}"))
        };
        let ret_ty = if let Some(ret_ty) = self.fn_returns.get(&fn_key) {
            llvm_type_of(ret_ty, self.layouts)
        } else {
            llvm_type_of(expected, self.layouts)
        };

        let mut call_args = vec![format!("ptr {recv}")];
        // 形参 LLVM 类型——用于数值加宽转换（int→double 等，RFC 015）。
        // typeck 判定 int→double 兼容但不插入转换节点，MIR 亦不处理数值 Cast，
        // 故此处按形参类型对实参做 sitofp/fpext，避免 x64 ABI 整数/浮点寄存器错位
        //（否则 setter 收到 0）。method_param_llvm_tys 沿父类链查找声明类。
        let param_llvm_tys = self.method_param_llvm_tys(class, method, args);
        for (i, a) in args.iter().enumerate() {
            let is_ref_or_out = matches!(a, MirOperand::AddrOf(_));
            let (ty, val) = if self.operand_is_delegate_value(a) {
                self.emit_operand_as_closure(a)
            } else {
                self.emit_operand(a)
            };
            let arg_str = if is_ref_or_out {
                // ref/out 参数：形参 ABI 为 ptr（实参地址），`emit_operand(AddrOf)`
                // 已返回 `("ptr", 地址)`。绝不能按形参 pointee 类型 coerce_value——
                // 否则 `ptrtoint ptr %v to i32` 把地址截断成 i32，被调方按 ptr 解引用
                // 命中非法地址 → 0xC0000005（WebTransport.ParseUrl ref int 实测）。
                format!("ptr {val}")
            } else if let Some(pt) = param_llvm_tys.get(i) {
                if ty == *pt {
                    format!("{ty} {val}")
                } else {
                    let (new_ty, coerced) = self.coerce_value(&ty, val, pt);
                    if new_ty == ty {
                        // coerce 未实际转换（如 struct 参数 ptr 直接传递），保持原类型前缀。
                        format!("{ty} {coerced}")
                    } else {
                        format!("{new_ty} {coerced}")
                    }
                }
            } else {
                format!("{ty} {val}")
            };
            call_args.push(arg_str);
        }
        if ret_ty == "void" {
            let may_throw = self.callee_may_throw(&fn_key);
            self.emit_call_may_throw(
                "void",
                &format!("@{fn_name}"),
                &call_args.join(", "),
                may_throw,
                None,
            );
            ("void".into(), String::new())
        } else {
            let tmp = self.fresh_temp();
            let may_throw = self.callee_may_throw(&fn_key);
            self.emit_call_may_throw(
                &ret_ty,
                &format!("@{fn_name}"),
                &call_args.join(", "),
                may_throw,
                Some(&tmp),
            );
            // M6.2 协程暖启动（对标 .NET async 同步前缀）：async **方法**调用点
            // 统一发射——`emit_call_typed` 已覆盖自由函数路径，此处补齐实例方法
            // 路径（如 ExecutorStressTests.FanoutWorker，经 MirRvalue::MethodCall）。
            // 立即首 poll 驱动 body 同步前缀至首个未完成 await，打破「create N →
            // await each」串行化。仅对 Arc async 方法发射（async_fns 判定）；
            // 运行时 Task-returning 面（Task.Run/Delay 等）不在此集合，零误伤。
            if self.async_fns.contains(&fn_key) {
                self.emit(&format!("call void @rt_task_autostart(ptr {tmp})"));
            }
            (ret_ty, tmp)
        }
    }

    /// 方法形参的 LLVM 类型列表（沿父类链查找声明类）。
    ///
    /// 供普通方法调用参数发射做数值加宽转换（int→double 等）。Rust 方法可见性
    /// 原因：`ModuleEmitter::class_method_param_types` 与 `FnEmitter` 非同一类型，
    /// 此处以 `FnEmitter.layouts` 内联等价逻辑。
    fn method_param_llvm_tys(&self, class: &str, method: &str, args: &[MirOperand]) -> Vec<String> {
        // 收集类链上所有同名重载的形参类型（含继承）。重载消歧：
        // 先按 arity 匹配实参个数；仍歧义（同参数量不同签名，如
        // `Parse(int)` vs `Parse(List<string>)`）时按实参类型名匹配，
        // 避免 name-only 首匹配选中错误重载，把 List 指针截断成 i32 造成 0xC0000005。
        let mut candidates: Vec<&Vec<Ident>> = Vec::new();
        let mut cur = class;
        while let Some(cl) = self.layouts.classes.get(cur) {
            for m in &cl.declared_methods {
                if m.name.as_str() == method {
                    candidates.push(&m.param_types);
                }
            }
            match &cl.parent {
                Some(p) => cur = p.as_str(),
                None => break,
            }
        }
        if candidates.is_empty() {
            return Vec::new();
        }
        let map = |p: &Vec<Ident>| {
            p.iter()
                .map(|t| llvm_field_type(t.as_str(), self.layouts))
                .collect()
        };
        let arity = args.len();
        let by_arity: Vec<&Vec<Ident>> = candidates
            .iter()
            .copied()
            .filter(|p| p.len() == arity)
            .collect();
        if by_arity.len() == 1 {
            return map(by_arity[0]);
        }
        if by_arity.len() > 1 {
            let arg_names: Vec<String> = args.iter().map(|a| self.operand_type_name(a)).collect();
            if let Some(m) = by_arity.iter().find(|p| {
                p.iter()
                    .zip(&arg_names)
                    .all(|(pt, an)| pt.as_str() == an.as_str())
            }) {
                return map(m);
            }
        }
        let chosen: Option<&Vec<Ident>> = by_arity
            .first()
            .copied()
            .or_else(|| candidates.first().copied());
        match chosen {
            Some(p) => map(p),
            None => Vec::new(),
        }
    }

    /// 推导实参在 `MethodLayout.param_types` 命名空间下的类型名（如 `int` / `string` /
    /// `List_string`）。用于同 arity 重载消歧；无法确定的实参返回空串（不参与匹配）。
    fn operand_type_name(&self, op: &MirOperand) -> String {
        match op {
            MirOperand::Local(id) | MirOperand::AddrOf(id) => self.local_type(*id).display(),
            MirOperand::ConstInt(_) => "int".into(),
            MirOperand::ConstFloat(_) => "float".into(),
            MirOperand::ConstString(_) => "string".into(),
            MirOperand::ConstBool(_) => "bool".into(),
            _ => String::new(),
        }
    }

    /// RFC 016 M1/M2/M3：Native contract call 发射（共享 helper）。
    ///
    /// 同时服务 `emit_call_typed`（func 形式 `<module>::<fn>` 或 `<module>.<fn>`，
    /// 调用方需先把 `::` 替换为 `.` 再传入）和 `emit_method_call_typed`
    /// （`native_key = format!("{receiver_type}.{method}")`）两条路径，
    /// 确保 `declare`（emit_native_decls 使用 `info.symbol`）与 `call`
    /// （本 helper 同样使用 `info.symbol`）符号一致，避免 `libc::puts` 被
    /// mangle 为 `libc_puts` 与 `declare @puts` 不匹配导致链接错误。
    ///
    /// native_key 格式为 `<module>.<fn>`，对应 `build_native_symbol_table`
    /// 中 `format!("{module.name}.{fn_decl.name}")` 的键。命中即返回 `Some(TyVal)`
    /// 表示已 emit 调用；`None` 表示无匹配，调用方应继续 fallback。
    ///
    /// RFC 016 M2：`out`/`ref` 参数传地址而非值。若实参为 `MirOperand::Local(id)`，
    /// 直接复用局部变量地址（C 函数写入即更新 Arc 变量）；否则 alloca 临时槽位
    /// （防御性兜底，typeck 应保证 `out`/`ref` 仅接受左值）。
    ///
    /// RFC 016 M3 §3.3：契约 struct 按值传递（`load %struct.<Name>, ptr %val`）；
    /// `List<T>` 零拷贝展开为 `ptr buffer, i32 size` 两个 LLVM 参数；返回值若为
    /// `%struct.<Name>` 则 alloca + store 为 ptr-to-struct（与 Arc 侧存储约定一致）。
    fn try_emit_native_call(&mut self, native_key: &str, args: &[MirOperand]) -> Option<TyVal> {
        // 提取所需字段并 clone，NLL 会在最后一次访问 info 后自动释放
        // 对 self.native_symbols 的不可变借用，从而允许后续调用 self.emit /
        // self.fresh_temp / self.emit_operand 等 &mut self 方法。每次调用仅
        // clone 几个 String/Vec，远小于 FFI 调用本身的 ABI 开销，可忽略。
        let info = self.native_symbols.get(native_key)?;
        let symbol = info.symbol.clone();
        let ret_llvm = info.ret_llvm.clone();
        let param_llvms: Vec<String> = info.param_llvms.clone();
        let param_directions: Vec<ast::ParamDirection> = info.param_directions.clone();
        let calling_conv = info.calling_conv;
        let param_marshal: Vec<super::native::ParamMarshal> = info.param_marshal.clone();
        // RFC 016 M1：每个原始参数的 callback 类型名（若为 callback 类型）。
        // 长度 = 原始参数数量。`None` 表示非 callback 参数。
        let param_callback_types: Vec<Option<String>> = info.param_callback_types.clone();

        // RFC 016：判断模块生效策略是否为 runtime（懒解析 + 间接调用）。
        // 命中时返回 (模块名, 函数表槽位索引, 函数表大小)，走 `emit_runtime_native_call`。
        let runtime_slot: Option<(String, usize, usize)> = {
            let (module, fn_name) = native_key
                .rsplit_once('.')
                .unwrap_or((native_key, native_key));
            self.runtime_native.get(module).and_then(|m| {
                m.fn_index
                    .get(fn_name)
                    .map(|i| (m.name.clone(), *i, m.symbols.len()))
            })
        };
        if let Some((module, slot_index, ftable_n)) = &runtime_slot {
            return self.emit_runtime_native_call(
                module,
                *slot_index,
                *ftable_n,
                &ret_llvm,
                &param_llvms,
                &param_directions,
                &param_marshal,
                &param_callback_types,
                args,
            );
        }

        let cc_prefix = match calling_conv {
            ast::CallingConv::C => "",
            ast::CallingConv::Stdcall => "stdcallcc ",
        };
        // RFC 016 M3 §3.3 List<T> marshal：用 `param_marshal` 驱动迭代，
        // 每个 `ParamMarshal::List` 展开为 `ptr buffer, i32 size` 两个 LLVM 参数。
        // `param_llvms` 是展开后的列表，需要单独的 `llvm_idx` 游标跟踪。
        let arg_strs = self.emit_native_call_args(
            args,
            &param_marshal,
            &param_directions,
            &param_llvms,
            &param_callback_types,
        );
        if ret_llvm == "void" {
            // Zero-cost EH M5：FFI 调用属未知外部（B.7 opaque，may-throw）；
            // 在 try/finally 区域内发 invoke 使异常落入本区域 landingpad。
            self.emit_call_may_throw(
                "void",
                &format!("{cc_prefix}@{symbol}"),
                &arg_strs.join(", "),
                true,
                None,
            );
            self.emit_clear_ffi_slots();
            return Some(("void".into(), String::new()));
        }
        // RFC 016 M3 §3.3：契约 struct 返回值按值接收。
        // native call 返回 %struct.<Name> SSA value；alloca + store 为 ptr-to-struct，
        // 与 Arc 侧 struct 存储约定一致（按引用存储），后续传递时再 load 出 struct 值。
        if ret_llvm.starts_with("%struct.") {
            let tmp = self.fresh_temp();
            self.emit_call_may_throw(
                &ret_llvm,
                &format!("{cc_prefix}@{symbol}"),
                &arg_strs.join(", "),
                true,
                Some(&tmp),
            );
            let slot = self.fresh_temp();
            self.emit(&format!("{slot} = alloca {}", ret_llvm));
            self.emit(&format!("store {} {tmp}, ptr {slot}", ret_llvm));
            self.emit_clear_ffi_slots();
            return Some(("ptr".into(), slot));
        }
        let tmp = self.fresh_temp();
        self.emit_call_may_throw(
            &ret_llvm,
            &format!("{cc_prefix}@{symbol}"),
            &arg_strs.join(", "),
            true,
            Some(&tmp),
        );
        self.emit_clear_ffi_slots();
        Some((ret_llvm.clone(), tmp))
    }

    /// RFC 016：运行时加载模块的 native 调用发射（懒解析 + 间接调用）。
    ///
    /// 发射序列：
    /// 1. `call @__arc_ani_ensure_<mod>()`——懒解析器（一次性，幂等）。
    /// 2. 可用性门闩：读 `@__arc_ani_<mod>_avail`，失败分支抛
    ///    `NativeLibraryNotFoundException`（经 std `Native.ThrowIfUnavailable`），
    ///    成功分支经 `@__arc_ani_<mod>_ftable[slot_index]` 间接调用。
    ///
    /// `slot_index` 为函数在 per-module 函数表中的槽位（声明顺序）。
    fn emit_runtime_native_call(
        &mut self,
        module: &str,
        slot_index: usize,
        ftable_n: usize,
        ret_llvm: &str,
        param_llvms: &[String],
        param_directions: &[ast::ParamDirection],
        param_marshal: &[super::native::ParamMarshal],
        param_callback_types: &[Option<String>],
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let p = format!("__arc_ani_{module}");
        // 1. ensure 懒解析（幂等——解析器内部有 _loaded 一次性门闩）。
        self.emit(&format!("call void @{p}_ensure()"));
        // 2. 可用性门闩。
        let av = self.fresh_temp();
        self.emit(&format!("{av} = load i8, ptr @{p}_avail"));
        let avb = self.fresh_temp();
        self.emit(&format!("{avb} = icmp ne i8 {av}, 0"));
        let ok_label = self.fresh_label();
        let throw_label = self.fresh_label();
        self.emit(&format!(
            "br i1 {avb}, label %{ok_label}, label %{throw_label}"
        ));
        // 3. 失败 → Native.ThrowIfUnavailable("<module>") 抛异常。
        self.emit(&format!("{throw_label}:"));
        self.emit_call_may_throw(
            "void",
            "@Native_ThrowIfUnavailable",
            &format!("ptr @{p}_mname"),
            true,
            None,
        );
        self.emit("unreachable");
        // 4. 成功 → 经函数表间接调用（实参 marshal 与静态路径完全一致）。
        self.emit(&format!("{ok_label}:"));
        let arg_strs = self.emit_native_call_args(
            args,
            param_marshal,
            param_directions,
            param_llvms,
            param_callback_types,
        );
        let slot_addr = self.fresh_temp();
        self.emit(&format!(
            "{slot_addr} = getelementptr inbounds [{ftable_n} x ptr], ptr @{p}_ftable, i32 0, i32 {slot_index}"
        ));
        let fptr = self.fresh_temp();
        self.emit(&format!("{fptr} = load ptr, ptr {slot_addr}"));
        if ret_llvm == "void" {
            // Zero-cost EH M5：FFI 调用属未知外部（B.7 opaque，may-throw）；
            // 在 try/finally 区域内发 invoke 使异常落入本区域 landingpad。
            self.emit_call_may_throw("void", &fptr, &arg_strs.join(", "), true, None);
            self.emit_clear_ffi_slots();
            return Some(("void".into(), String::new()));
        }
        if ret_llvm.starts_with("%struct.") {
            // RFC 016 M3 §3.3：契约 struct 返回值按值接收。
            let tmp = self.fresh_temp();
            self.emit_call_may_throw(ret_llvm, &fptr, &arg_strs.join(", "), true, Some(&tmp));
            let slot = self.fresh_temp();
            self.emit(&format!("{slot} = alloca {ret_llvm}"));
            self.emit(&format!("store {ret_llvm} {tmp}, ptr {slot}"));
            self.emit_clear_ffi_slots();
            return Some(("ptr".into(), slot));
        }
        let tmp = self.fresh_temp();
        self.emit_call_may_throw(ret_llvm, &fptr, &arg_strs.join(", "), true, Some(&tmp));
        self.emit_clear_ffi_slots();
        Some((ret_llvm.to_string(), tmp))
    }

    /// RFC 016：发射 `Native.IsAvailable("<module>")` 可用性查询。
    ///
    /// - 生效策略为 runtime 的模块：`call ensure` + 读 `_avail` → i1。
    /// - 编译期静态链接的 native 模块：恒 `true`。
    /// - 未知名称（非 native 模块）：恒 `false`。
    fn emit_native_availability(&mut self, module: &str) -> String {
        if let Some(info) = self.runtime_native.get(module) {
            let mname = info.name.clone();
            self.emit(&format!("call void @__arc_ani_{}_ensure()", mname));
            let av = self.fresh_temp();
            self.emit(&format!("{av} = load i8, ptr @__arc_ani_{}_avail", mname));
            let flag = self.fresh_temp();
            self.emit(&format!("{flag} = icmp ne i8 {av}, 0"));
            return flag;
        }
        // 静态 native 模块（编译期已链接）恒可用；未知模块恒不可用。
        let is_native_module = self
            .native_symbols
            .keys()
            .any(|k| k.starts_with(&format!("{module}.")));
        if is_native_module {
            "true".into()
        } else {
            "false".into()
        }
    }

    /// native 调用实参 marshal（RFC 016 M1/M2/M3 共享 helper）。
    ///
    /// 生成 LLVM 调用参数串列表，返回后可直接 `join(", ")` 拼入 `call`/`invoke`。
    /// 含 `out`/`ref` 传址、callback trampoline、契约 struct 按值、List<T> 零拷贝
    /// 展开等全部 marshal 规则（与 `try_emit_native_call` 静态路径共用）。
    fn emit_native_call_args(
        &mut self,
        args: &[MirOperand],
        param_marshal: &[super::native::ParamMarshal],
        param_directions: &[ast::ParamDirection],
        param_llvms: &[String],
        param_callback_types: &[Option<String>],
    ) -> Vec<String> {
        let mut arg_strs: Vec<String> = Vec::with_capacity(param_llvms.len());
        let mut llvm_idx = 0;
        for (i, a) in args.iter().enumerate() {
            let marshal = param_marshal
                .get(i)
                .copied()
                .unwrap_or(super::native::ParamMarshal::Normal);
            match marshal {
                super::native::ParamMarshal::Normal => {
                    let direction = param_directions
                        .get(i)
                        .copied()
                        .unwrap_or(ast::ParamDirection::In);
                    let param_ty = param_llvms
                        .get(llvm_idx)
                        .map(|s| s.as_str())
                        .unwrap_or("ptr");
                    let arg_str = match direction {
                        ast::ParamDirection::Out | ast::ParamDirection::InOut => {
                            self.emit_native_byref_arg(a, param_ty)
                        }
                        ast::ParamDirection::In => {
                            // RFC 016 M1：若该参数为 callback 类型且实参为 FnPtr（无捕获 lambda），
                            // 生成 trampoline 函数适配 C ABI，传 trampoline 函数指针给 C 端。
                            // trampoline 在模块级累积，emit_module 末尾统一发射。
                            let cb_type = param_callback_types
                                .get(i)
                                .and_then(|t| t.as_ref())
                                .map(|s| s.as_str());
                            if let (Some(cb_name), MirOperand::FnPtr { name }) = (cb_type, a) {
                                let lambda_mangled = mangle_fn_name(name);
                                let tramp_name = format!("__tramp_{cb_name}_{lambda_mangled}");
                                // 仅在首次遇到该 (cb_name, lambda) 对时发射 trampoline IR。
                                if let Some(cb_info) = self.native_callback_table.get(cb_name) {
                                    let ir = super::emit_native_callback::emit_trampoline(
                                        &lambda_mangled,
                                        cb_info,
                                        &tramp_name,
                                    );
                                    self.native_trampolines.try_push(&tramp_name, ir);
                                    format!("ptr @{tramp_name}")
                                } else {
                                    // callback 类型未在表内：fallback 到直接传 lambda 指针。
                                    let (_, val) = self.emit_operand(a);
                                    format!("ptr {val}")
                                }
                            } else if let (Some(cb_name), MirOperand::Closure { .. }) = (cb_type, a)
                            {
                                // RFC 016 M2：有捕获 lambda → C 回调。调用前把
                                // arc_closure 指针存入 TLS slot，传 TLS trampoline
                                // 指针给 C 端；C 回调经 trampoline 从 slot 取
                                // closure 间接调用。调用返回后清理 slot。
                                // （与 emit_trampoline 的 FnPtr 分支并列。）
                                if let Some(cb_info) = self.native_callback_table.get(cb_name) {
                                    let slot = self.native_trampolines.alloc_slot();
                                    let (_, closure_ptr) = self.emit_operand(a);
                                    let tramp_name = format!("__tramp_tls_{slot}_{cb_name}_cb");
                                    // 设置 TLS slot。
                                    self.emit(&format!(
                                        "call void @rt_ffi_set_callback(i32 {slot}, ptr {closure_ptr})"
                                    ));
                                    // 发射 TLS trampoline（模块级累积，去重）。
                                    let ir = super::emit_native_callback::emit_tls_trampoline(
                                        slot,
                                        "",
                                        cb_info,
                                        &tramp_name,
                                    );
                                    self.native_trampolines.try_push(&tramp_name, ir);
                                    // 登记待清理 slot（调用返回后清理）。
                                    self.pending_ffi_slots.push(slot);
                                    format!("ptr @{tramp_name}")
                                } else {
                                    let (_, val) = self.emit_operand(a);
                                    format!("ptr {val}")
                                }
                            } else {
                                let (op_ty, val) = self.emit_operand(a);
                                // RFC 016 M3 §3.3：契约 struct 按值传递。
                                // Arc 侧 struct local 是 ptr（指向 alloca'd struct），
                                // 需 `load %struct.<Name>, ptr %val` 加载 struct 值后传递。
                                // LLVM 后端优化：小 struct（≤16B）寄存器传递，大 struct 栈传递，零额外开销。
                                if param_ty.starts_with("%struct.") {
                                    let loaded = self.fresh_temp();
                                    self.emit(&format!("{loaded} = load {param_ty}, ptr {val}"));
                                    format!("{param_ty} {loaded}")
                                } else if op_ty == param_ty {
                                    // 类型匹配——直接发射，零开销。
                                    format!("{param_ty} {val}")
                                } else {
                                    // RFC 016 M3 §3.3 FFI：operand 类型与 native 参数类型不匹配
                                    //（如 i64 long → ptr NativePtr，或 i32 字面量 → ptr）。
                                    // 调用 coerce_value 发射 inttoptr/ptrtoint 转换指令。
                                    let (_, coerced) = self.coerce_value(&op_ty, val, param_ty);
                                    format!("{param_ty} {coerced}")
                                }
                            }
                        }
                    };
                    arg_strs.push(arg_str);
                    llvm_idx += 1;
                }
                super::native::ParamMarshal::ByteArray => {
                    // RFC 025 S4：`byte[]`（RtArrayHeader 载体）→ 直接传 payload 指针。
                    // Arc byte[] 变量/字面量经 emit_operand 即得 payload 指针（header 在
                    // payload-8）；`In`/`Out`/`InOut` 均传该指针——C shim 经
                    // `arr_len(data)` 读 header 得知容量并写入 payload，Arc 变量持同一
                    // 指针，写入即对 Arc 可见。零拷贝、单 LLVM 参数（ptr）。
                    let (_, val) = self.emit_operand(a);
                    arg_strs.push(format!("ptr {val}"));
                    llvm_idx += 1;
                }
                super::native::ParamMarshal::List => {
                    // RFC 016 M3 §3.3 List<T> marshal：零拷贝展开。
                    // 实参是 List 对象 ptr（emit_operand 返回），需 GEP offset 16
                    // 加载 _handle (RtList*)，然后调用 `rt_list_buffer_and_size`
                    // 获取内部 buffer 指针和元素数量，传 `ptr buffer, i32 size` 两个参数。
                    //
                    // 性能考量（RFC 009 M5 零分配热路径 + RFC 009 IO 吞吐 ≥20×）：
                    // - 零拷贝：直接暴露内部 buffer，不复制元素
                    // - O(1) 复杂度：rt_list_buffer_and_size 仅读 RtList 字段
                    // - 2 条 alloca + 1 条 call + 2 条 load = 5 条 IR 指令开销
                    // - 与 RFC 009 IO 多路复用协同：ORM 查询结果 List 直接传递给 FFI
                    let (_, list_ptr) = self.emit_operand(a);
                    // 从 List 对象 offset 16 加载 _handle (ptr)
                    let handle_addr = self.fresh_temp();
                    self.emit(&format!(
                        "{handle_addr} = getelementptr inbounds i8, ptr {list_ptr}, i32 16"
                    ));
                    let handle = self.fresh_temp();
                    self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));
                    // alloca out 参数槽位：ptr* buf, i32* size
                    let buf_slot = self.fresh_temp();
                    self.emit(&format!("{buf_slot} = alloca ptr"));
                    let size_slot = self.fresh_temp();
                    self.emit(&format!("{size_slot} = alloca i32"));
                    // 调用 rt_list_buffer_and_size(handle, &buf, &size)
                    self.emit(&format!(
                        "call void @rt_list_buffer_and_size(ptr {handle}, ptr {buf_slot}, ptr {size_slot})"
                    ));
                    // load buf 和 size
                    let buf = self.fresh_temp();
                    self.emit(&format!("{buf} = load ptr, ptr {buf_slot}"));
                    let size = self.fresh_temp();
                    self.emit(&format!("{size} = load i32, ptr {size_slot}"));
                    arg_strs.push(format!("ptr {buf}"));
                    arg_strs.push(format!("i32 {size}"));
                    llvm_idx += 2;
                }
            }
        }
        arg_strs
    }

    /// RFC 016 M2：native call 返回后清理 TLS callback slots。
    /// 有捕获 lambda 调用前 `rt_ffi_set_callback` 存 slot；返回后逐个
    /// `rt_ffi_clear_callback`，避免跨调用残留。
    fn emit_clear_ffi_slots(&mut self) {
        if self.pending_ffi_slots.is_empty() {
            return;
        }
        let slots: Vec<i32> = std::mem::take(&mut self.pending_ffi_slots);
        for slot in slots {
            self.emit(&format!("call void @rt_ffi_clear_callback(i32 {slot})"));
        }
    }

    /// 为 native 契约的 `out`/`ref` 参数生成 byref 传递（RFC 016 M2）。
    ///
    /// - 若实参为 `MirOperand::Local(id)` 或 `MirOperand::AddrOf(id)`：
    ///   直接复用局部变量栈地址，C 函数写入即更新 Arc 变量
    /// - 其他情况：alloca 临时槽位 + 存入实参值 + 传地址（防御性兜底，typeck 应保证 `out`/`ref` 仅接受左值）
    ///
    /// `pointee_llvm` 是参数指向的元素类型（如 `i32`、`ptr`），用于 alloca 与 store。
    /// 返回 `ptr <addr>` 形式，可直接拼接到 `call` 指令的参数列表。
    ///
    /// 注意：MIR lower 将 `out ipart` 解析为 `MirOperand::AddrOf(id)`，所以此处必须
    /// 同时匹配 `Local` 与 `AddrOf`，否则会走兜底路径生成错误的二级指针传递。
    pub(super) fn emit_native_byref_arg(&mut self, arg: &MirOperand, pointee_llvm: &str) -> String {
        if let MirOperand::Local(id) | MirOperand::AddrOf(id) = arg {
            let ptr = self.byref_arg_ptr(*id);
            return format!("ptr {ptr}");
        }
        // 兜底：alloca + store + 传地址。
        let (val_ty, val) = self.emit_operand(arg);
        let slot = self.fresh_temp();
        self.emit(&format!("{slot} = alloca {pointee_llvm}"));
        self.emit(&format!("store {val_ty} {val}, ptr {slot}"));
        format!("ptr {slot}")
    }

    /// Virtual dispatch: load vtable pointer, get method slot, indirect call.
    ///
    /// The return type is resolved from the vtable layout's declared method
    /// signature rather than the caller's `expected` TypeId. This is critical
    /// because MIR type inference (`infer_type_from_expr`) may fall back to
    /// `TypeId::Int` when overload resolution fails (e.g. namespace-qualified
    /// receiver types), which would produce invalid IR like `call i32` for a
    /// method that actually returns `string` (ptr).
    fn emit_virtual_call(
        &mut self,
        recv: &str,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
        expected: &TypeId,
        params: &[String],
    ) -> TyVal {
        let slot = self.virtual_slot_index(receiver_type, method, params);
        // Prefer the vtable-declared return type; fall back to `expected` only
        // if the layout lookup fails (e.g. external/builtin receiver).
        let ret_ty = self
            .virtual_method_ret_name(receiver_type, method, params)
            .map(|ret_name| {
                let ty_id = type_name_str_to_type_id(ret_name);
                llvm_type_of(&ty_id, self.layouts)
            })
            .unwrap_or_else(|| llvm_type_of(expected, self.layouts));

        // Load vtable pointer from offset 8
        let vptr_addr = self.fresh_temp();
        self.emit(&format!(
            "{vptr_addr} = getelementptr inbounds i8, ptr {recv}, i64 8"
        ));
        let vptr = self.fresh_temp();
        self.emit(&format!("{vptr} = load ptr, ptr {vptr_addr}"));

        // Get method slot from vtable (RFC 004 D5 修订)：
        // slot 0 = typeinfo*，slot 1 = finalizer，slot 2 = walk，slot 3+ = virtual methods
        let slot_idx = slot + 3;
        let fn_addr = self.fresh_temp();
        self.emit(&format!(
            "{fn_addr} = getelementptr inbounds ptr, ptr {vptr}, i32 {slot_idx}"
        ));
        let fn_ptr = self.fresh_temp();
        self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_addr}"));

        // Build call args: ptr self, then method args
        let mut call_args = vec![format!("ptr {recv}")];
        for a in args {
            // 委托实参（Func/Action / FnPtr / Closure）统一走 arc_closure* +
            // 堆分配（与 emit_call / emit_method_call 对齐）。虚方法（如
            // Signal.Subscribe）把闭包存入堆订阅列表，若用 emit_operand 的
            // alloca 栈分配，函数返回后闭包结构体悬垂 → use-after-free。
            let (ty, val) = if self.operand_is_delegate_value(a) {
                self.emit_operand_as_closure(a)
            } else {
                self.emit_operand(a)
            };
            call_args.push(format!("{ty} {val}"));
        }

        if ret_ty == "void" {
            self.emit_call_may_throw("void", &fn_ptr.clone(), &call_args.join(", "), true, None);
            ("void".into(), String::new())
        } else {
            let tmp = self.fresh_temp();
            self.emit_call_may_throw(
                &ret_ty,
                &fn_ptr.clone(),
                &call_args.join(", "),
                true,
                Some(&tmp),
            );
            (ret_ty, tmp)
        }
    }

    /// Interface dispatch via fat pointer.
    fn emit_iface_method_call(
        &mut self,
        recv: &str,
        method: &str,
        args: &[MirOperand],
        iface: &str,
        expected: &TypeId,
        params: &[String],
    ) -> TyVal {
        let ret_ty = llvm_type_of(expected, self.layouts);

        // Fat pointer: { ptr obj, ptr vtable }
        // recv is a stack alloca holding { ptr, ptr }
        let obj_addr = self.fresh_temp();
        self.emit(&format!(
            "{obj_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {recv}, i32 0, i32 0"
        ));
        let obj = self.fresh_temp();
        self.emit(&format!("{obj} = load ptr, ptr {obj_addr}"));

        let vtbl_addr = self.fresh_temp();
        self.emit(&format!(
            "{vtbl_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {recv}, i32 0, i32 1"
        ));
        let vtbl = self.fresh_temp();
        self.emit(&format!("{vtbl} = load ptr, ptr {vtbl_addr}"));

        // Find method slot index in interface layout
        let slot_idx = self.iface_method_index(iface, method, params);
        let fn_addr = self.fresh_temp();
        self.emit(&format!(
            "{fn_addr} = getelementptr inbounds ptr, ptr {vtbl}, i32 {slot_idx}"
        ));
        let fn_ptr = self.fresh_temp();
        self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_addr}"));

        let mut call_args = vec![format!("ptr {obj}")];
        for a in args {
            let (ty, val) = self.emit_operand(a);
            call_args.push(format!("{ty} {val}"));
        }

        if ret_ty == "void" {
            self.emit_call_may_throw("void", &fn_ptr.clone(), &call_args.join(", "), true, None);
            return ("void".into(), String::new());
        }
        let tmp = self.fresh_temp();
        self.emit_call_may_throw(
            &ret_ty,
            &fn_ptr.clone(),
            &call_args.join(", "),
            true,
            Some(&tmp),
        );
        // Return ABI matches the itable view: variance adapter thunks (emitted
        // into `@.itable.{Class}_{CovariantIface}`) wrap class→iface / rebind
        // nested iface returns. No call-site MakeIface fallback.
        (ret_ty, tmp)
    }

    // ---- Indirect call (function pointers / closures) ----

    pub(super) fn emit_indirect_call(&mut self, func: &MirOperand, args: &[MirOperand]) -> TyVal {
        // RFC 023: when the callee is an inline `Closure` operand, build the
        // arc_closure on the fly and invoke via fn_ptr(env, args...).
        if let MirOperand::Closure { fn_name, env } = func {
            let (_, closure_ptr) = self.emit_closure_value(fn_name, env);
            let ret_ty = self.indirect_call_ret_ty(func, args.len());
            return self.emit_closure_indirect_call(&closure_ptr, args, &ret_ty);
        }
        // RFC 008 / C6：字段加载的委托（`this.Callback(args)` / `obj.DelegateField(...)`）
        // 字段存的是 arc_closure*（{fn_ptr, env_ptr} 结构体指针），不是裸函数指针。
        // 必须走 emit_closure_indirect_call 提取 fn_ptr/env_ptr；否则把闭包对象地址
        // 当裸函数指针调用 → 静默无效（public 委托字段 codegen 缺陷 C6 根因）。
        if let MirOperand::Field { class, field, .. } = func {
            if self.field_is_delegate(class, field) {
                let (_, closure_ptr) = self.emit_operand(func);
                let ret_ty = self.indirect_call_ret_ty(func, args.len());
                return self.emit_closure_indirect_call(&closure_ptr, args, &ret_ty);
            }
        }
        // RFC 008: when the callee is a local that holds an arc_closure value
        // (capturing lambda, Func param, or List/field-loaded delegate), load
        // the closure pointer and extract fn_ptr/env_ptr before calling.
        // No-capture locals assigned from bare `FnPtr` stay unmarked and keep
        // the direct-call path (zero overhead).
        let is_closure_local = if let MirOperand::Local(id) = func {
            self.closure_locals.contains(id)
        } else {
            false
        };
        // RFC 045（di_decorate 崩溃根因）：FnPtr 存储统一为 arc_closure 后，
        // **一切 Func/Action 类型局部**（含无捕获 lambda 赋值的局部）都持
        // closure 指针——调用必须经 emit_closure_indirect_call 提取 fn/env，
        // 否则把 closure 地址当裸函数调用 → 0xC0000005（di_dec2 装饰工厂
        // captured(sp) 经闭包捕获后调用崩溃）。裸 FnPtr operand（内联立即
        // 调用）保持直调（零开销，不存储）。
        let is_func_local = matches!(
            func,
            MirOperand::Local(id)
                if self.cfg.locals.get(id).is_some_and(|(_, ty)| {
                    // RFC 045（di_decorate 崩溃根因）：可空委托局部
                    // （`Func<...>?`——ServiceProvider.GetService 的 `fac`）
                    // 与 Func 同形（closure 指针），须解包 Nullable 判别，
                    // 否则落直调路径 call closure 地址 → 0xC0000005/409。
                    let ty = match ty {
                        TypeId::Nullable { inner } => inner.as_ref(),
                        other => other,
                    };
                    matches!(ty, TypeId::Func { .. })
                        || matches!(
                            ty,
                            TypeId::Named(n)
                                if n.starts_with("Func_") || n.starts_with("Action_")
                        )
                })
        );
        if is_closure_local || is_func_local {
            let (_, closure_ptr) = self.emit_operand(func);
            let ret_ty = self.indirect_call_ret_ty(func, args.len());
            return self.emit_closure_indirect_call(&closure_ptr, args, &ret_ty);
        }
        let (_, fn_val) = self.emit_operand(func);
        // RFC 008：无捕获 lambda 以裸函数指针存储（`Func<T,R> f = x => ...`），
        // 但 lambda 函数的 ABI 期望所有参数为 `ptr`（运行时 `const void*` 元素指针，
        // 见 `emit_sync_function` 的 `is_lambda` 分支）。调用点必须把值实参
        // 物化为临时 alloca + store + 传指针，与 lambda 函数体 `load i32, ptr %arg`
        // 对齐。否则 `call i32 %f(i32 5)` 传入 i32 但函数按 ptr 解引用 → 0xc0000005。
        //
        // RFC 037：从 `List<Func<...>>` 取出的元素类型常为 mangled
        // `Named("Func_int_int_bool")`（非 `TypeId::Func`）——与 MIR lower 一致，
        // 必须同样走 ptr ABI，否则 Signal.OnChanging → TrySet 调用 handler 崩溃。
        //
        // RFC 007 M2c：立即调用 `(... => ...)(args)` 的 callee 是 `MirOperand::FnPtr`
        //（非 Local），同样必须走 ptr ABI；并以 `@__lambda_` 符号名兜底（与
        // `emit_call` / `emit_sync_function` 的 is_lambda 判定对齐）。
        let is_lambda_fn_ptr_abi = matches!(
            func,
            MirOperand::Local(id) if self.cfg.locals.get(id).is_some_and(|(_, ty)| {
                matches!(ty, TypeId::Func { .. })
                    || matches!(
                        ty,
                        TypeId::Named(n)
                            if n.starts_with("Func_") || n.starts_with("Action_")
                    )
            })
        ) || matches!(func, MirOperand::FnPtr { .. })
            || fn_val.contains("__lambda_");
        let arg_strs: Vec<String> = if is_lambda_fn_ptr_abi {
            args.iter()
                .map(|a| {
                    let (ty, val) = self.emit_operand(a);
                    // 与 emit_closure_indirect_call 一致：lifted lambda 形参
                    // 全部是槽位地址，ptr（class/string）参数也必须走槽位。
                    let slot = self.fresh_temp();
                    self.emit(&format!("{slot} = alloca {ty}"));
                    self.emit(&format!("store {ty} {val}, ptr {slot}"));
                    format!("ptr {slot}")
                })
                .collect()
        } else {
            args.iter()
                .map(|a| {
                    let (ty, val) = self.emit_operand(a);
                    format!("{ty} {val}")
                })
                .collect()
        };
        let ret_ty = self.indirect_call_ret_ty(func, args.len());
        if ret_ty == "void" {
            self.emit_call_may_throw("void", &fn_val.clone(), &arg_strs.join(", "), true, None);
            return ("void".into(), String::new());
        }
        let tmp = self.fresh_temp();
        self.emit_call_may_throw(
            &ret_ty,
            &fn_val.clone(),
            &arg_strs.join(", "),
            true,
            Some(&tmp),
        );
        (ret_ty, tmp)
    }

    /// 推断 IndirectCall 的返回 LLVM 类型。
    ///
    /// 优先用委托 `Func`/`Action` 的返回类型（含 mangled `Func_string`）；
    /// Closure/FnPtr 查 `fn_returns`（lambda cfg.ret）。禁止对引用返回硬编码
    /// `i32`（`Lazy<string>` / `Func<string>` → 0xC0000005）。
    fn indirect_call_ret_ty(&self, func: &MirOperand, arg_count: usize) -> String {
        match func {
            MirOperand::Local(id) => {
                if let Some((_, ty)) = self.cfg.locals.get(id) {
                    if let Some(ret) = delegate_ret_type(ty, self.layouts, arg_count) {
                        return llvm_type_of(&ret, self.layouts);
                    }
                }
                "i32".into()
            }
            MirOperand::Closure { fn_name, .. } | MirOperand::FnPtr { name: fn_name } => {
                if self.is_async_lambda(fn_name) {
                    return "ptr".into();
                }
                if let Some(ret) = self.fn_returns.get(fn_name) {
                    return llvm_type_of(ret, self.layouts);
                }
                "i32".into()
            }
            // C6：字段加载的委托按字段类型推断返回 LLVM 类型（Action → void，Func_x → x）。
            MirOperand::Field { class, field, .. } => {
                if let Some(ret) = self.field_delegate_ret_llvm_ty(class, field) {
                    return ret;
                }
                "i32".into()
            }
            _ => "i32".into(),
        }
    }

    /// 检查给定函数名是否对应一个 async lambda（基于 async_fns 集合）。
    fn is_async_lambda(&self, name: &str) -> bool {
        self.async_fns.contains(name)
    }

    /// Extract fn_ptr (field 0) and env_ptr (field 1) from an arc_closure and
    /// invoke the underlying lambda.
    ///
    /// RFC 008 双路径（与 open Q4 渐进统一对齐）：
    /// - `env != null`（有捕获）：`fn_ptr(env, args...)`（lambda 形参含 `__env__`）
    /// - `env == null`（无捕获 / 合成包装）：`fn_ptr(args...)`（P1-I 裸 lambda ABI，
    ///   无 `__env__`）。跨函数传 `arc_closure{fn,null}` 时必须走此分支，否则会把
    ///   null env 误当作第一个用户实参。
    pub(super) fn emit_closure_indirect_call(
        &mut self,
        closure_ptr: &str,
        args: &[MirOperand],
        ret_ty: &str,
    ) -> TyVal {
        let fn_field = self.fresh_temp();
        self.emit(&format!(
            "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
        ));
        let fn_ptr = self.fresh_temp();
        self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));

        let env_field = self.fresh_temp();
        self.emit(&format!(
            "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
        ));
        let env_ptr = self.fresh_temp();
        self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));

        let user_args: Vec<String> = args
            .iter()
            .map(|a| {
                let (ty, val) = self.emit_operand(a);
                // Lambda functions expect ALL non-env parameters as `ptr`
                // (alloca'd slots)——lifted lambda body 一律 `load {ty}, ptr %arg`
                // 取值。ptr（class/string）参数也必须走槽位：直接传值会被
                // lambda 当作槽地址解引用，读到对象头部 refcount 等垃圾 →
                // `Action<AIToolCall>` 事件 lambda AV（stream_events 实测
                // 0x1a = 头部值 + 字段偏移）。
                let slot = self.fresh_temp();
                self.emit(&format!("{slot} = alloca {ty}"));
                self.emit(&format!("store {ty} {val}, ptr {slot}"));
                format!("ptr {slot}")
            })
            .collect();

        let env_is_null = self.fresh_temp();
        self.emit(&format!("{env_is_null} = icmp eq ptr {env_ptr}, null"));
        let lbl_cap = self.fresh_label();
        let lbl_bare = self.fresh_label();
        let lbl_join = self.fresh_label();

        let result_slot = if ret_ty != "void" {
            let slot = self.fresh_temp();
            self.emit(&format!("{slot} = alloca {ret_ty}"));
            Some(slot)
        } else {
            None
        };

        self.emit(&format!(
            "br i1 {env_is_null}, label %{lbl_bare}, label %{lbl_cap}"
        ));

        // Capturing: fn(env, user_args...)
        self.emit_label(&lbl_cap);
        {
            let mut cap_args = vec![format!("ptr {env_ptr}")];
            cap_args.extend(user_args.iter().cloned());
            if let Some(ref slot) = result_slot {
                let tmp = self.fresh_temp();
                self.emit_call_may_throw(
                    ret_ty,
                    &fn_ptr.clone(),
                    &cap_args.join(", "),
                    true,
                    Some(&tmp),
                );
                self.emit(&format!("store {ret_ty} {tmp}, ptr {slot}"));
            } else {
                self.emit_call_may_throw("void", &fn_ptr.clone(), &cap_args.join(", "), true, None);
            }
            self.emit(&format!("br label %{lbl_join}"));
        }
        // No-capture: fn(user_args...) — P1-I ABI without __env__
        self.emit_label(&lbl_bare);
        {
            if let Some(ref slot) = result_slot {
                let tmp = self.fresh_temp();
                self.emit_call_may_throw(
                    ret_ty,
                    &fn_ptr.clone(),
                    &user_args.join(", "),
                    true,
                    Some(&tmp),
                );
                self.emit(&format!("store {ret_ty} {tmp}, ptr {slot}"));
            } else {
                self.emit_call_may_throw(
                    "void",
                    &fn_ptr.clone(),
                    &user_args.join(", "),
                    true,
                    None,
                );
            }
            self.emit(&format!("br label %{lbl_join}"));
        }
        self.emit_label(&lbl_join);
        // NOTE: Do not free env_ptr here — the closure env is reference-counted
        // by ARC and freed when the arc_closure is dropped.
        if let Some(slot) = result_slot {
            let loaded = self.fresh_temp();
            self.emit(&format!("{loaded} = load {ret_ty}, ptr {slot}"));
            (ret_ty.to_string(), loaded)
        } else {
            ("void".into(), String::new())
        }
    }

    /// Environment.* 静态方法 codegen（Phase 1 + Phase 2）。
    ///
    /// Phase 1 (2026-07-20)：ArgCount / GetArg（命令行参数）
    /// Phase 2 (2026-07-21)：环境变量、进程控制、系统信息、当前目录、机器/用户名
    /// 所有方法直接发射 rt_env_* ABI，bool 返回值发射为 i32（由 coerce_value 转 i1）。
    pub(super) fn try_emit_environment_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        match method {
            // ── Phase 1：命令行参数 ──
            "ArgCount" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_argc()"));
                Some(("i32".into(), tmp))
            }
            "GetArg" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, idx) = self.emit_operand(&args[0]);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_env_argv(i32 {idx})"));
                Some(("ptr".into(), tmp))
            }

            // ── Phase 2：环境变量 ──
            "GetEnvironmentVariable" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, name) = self.emit_operand(&args[0]);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_env_get_var(ptr {name})"));
                Some(("ptr".into(), tmp))
            }
            "SetEnvironmentVariable" => {
                if args.len() != 2 {
                    return None;
                }
                let (_, name) = self.emit_operand(&args[0]);
                let (_, value) = self.emit_operand(&args[1]);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_env_set_var(ptr {name}, ptr {value})"
                ));
                Some(("i32".into(), tmp))
            }

            // ── Phase 2：进程控制 ──
            "SelfProcessPath" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_env_self_exe()"));
                Some(("ptr".into(), tmp))
            }
            "Exit" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, code) = self.emit_operand(&args[0]);
                self.emit(&format!("call void @rt_env_exit(i32 {code})"));
                Some(("void".into(), String::new()))
            }
            "GetExitCode" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_get_exit_code()"));
                Some(("i32".into(), tmp))
            }
            "SetExitCode" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, code) = self.emit_operand(&args[0]);
                self.emit(&format!("call void @rt_env_set_exit_code(i32 {code})"));
                Some(("void".into(), String::new()))
            }
            "FailFast" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, msg) = self.emit_operand(&args[0]);
                self.emit(&format!("call void @rt_env_fail_fast(ptr {msg})"));
                Some(("void".into(), String::new()))
            }

            // ── Phase 2：系统信息 ──
            "NewLine" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_env_newline()"));
                Some(("ptr".into(), tmp))
            }
            "ProcessorCount" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_processor_count()"));
                Some(("i32".into(), tmp))
            }
            "Is64BitProcess" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_is_64bit_process()"));
                Some(("i32".into(), tmp))
            }

            // ── Phase 2：当前目录 ──
            "GetCurrentDirectory" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_env_get_cwd()"));
                Some(("ptr".into(), tmp))
            }
            "SetCurrentDirectory" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, path) = self.emit_operand(&args[0]);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_set_cwd(ptr {path})"));
                Some(("i32".into(), tmp))
            }

            // ── Phase 2：机器名 / 用户名 ──
            "MachineName" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_env_machine_name()"));
                Some(("ptr".into(), tmp))
            }
            "UserName" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_env_user_name()"));
                Some(("ptr".into(), tmp))
            }

            // ── Phase 2：平台标识 ──
            "Platform" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_env_platform()"));
                Some(("ptr".into(), tmp))
            }
            "IsWindows" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_is_windows()"));
                Some(("i32".into(), tmp))
            }
            "IsLinux" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_is_linux()"));
                Some(("i32".into(), tmp))
            }
            "IsMacOS" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_is_macos()"));
                Some(("i32".into(), tmp))
            }
            "IsAndroid" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_is_android()"));
                Some(("i32".into(), tmp))
            }
            "IsIOS" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_is_ios()"));
                Some(("i32".into(), tmp))
            }
            "IsOHOS" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_env_is_ohos()"));
                Some(("i32".into(), tmp))
            }

            _ => None,
        }
    }

    /// Console.* 静态方法 codegen（Phase 1+2+3）。
    ///
    /// 统一处理 `emit_call_typed`（func = "Console.Method"）与
    /// `emit_method_call_typed`（receiver_type = "Console"）两条路径，
    /// 避免重复实现。返回 `None` 表示未识别的 Console 方法，由调用方继续 fallback。
    ///
    /// Phase 1：Write/WriteLine/WriteLine()/ReadLine/Read
    /// Phase 2：SetForegroundColor/SetBackgroundColor/GetForegroundColor/
    ///          GetBackgroundColor/ResetColor
    pub(super) fn try_emit_console_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let result: TyVal = match method {
            "WriteLine" => {
                // WriteLine() 空行 / WriteLine(string) 带换行输出
                match args.len() {
                    0 => {
                        self.emit("call void @rt_println(ptr null)");
                        ("void".into(), String::new())
                    }
                    1 => {
                        let (_, arg) = self.emit_operand(&args[0]);
                        self.emit(&format!("call void @rt_println(ptr {arg})"));
                        ("void".into(), String::new())
                    }
                    _ => return None,
                }
            }
            "Write" => {
                // Write(string) 无换行输出
                if args.len() != 1 {
                    return None;
                }
                let (_, arg) = self.emit_operand(&args[0]);
                self.emit(&format!("call void @rt_print(ptr {arg})"));
                ("void".into(), String::new())
            }
            // ── Phase 3 (2026-07-20): Console.Error 标准错误输出 ──
            "ErrorWriteLine" => match args.len() {
                0 => {
                    self.emit("call void @rt_println_error(ptr null)");
                    ("void".into(), String::new())
                }
                1 => {
                    let (_, arg) = self.emit_operand(&args[0]);
                    self.emit(&format!("call void @rt_println_error(ptr {arg})"));
                    ("void".into(), String::new())
                }
                _ => return None,
            },
            "ErrorWrite" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, arg) = self.emit_operand(&args[0]);
                self.emit(&format!("call void @rt_print_error(ptr {arg})"));
                ("void".into(), String::new())
            }
            "ReadLine" => {
                // ReadLine() → string（EOF 返回 NULL，Arc 侧表现为空串）
                if !args.is_empty() {
                    return None;
                }
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_read_line()"));
                ("ptr".into(), tmp)
            }
            "Read" => {
                // Read() → int（EOF 返回 -1）
                if !args.is_empty() {
                    return None;
                }
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_read_char()"));
                ("i32".into(), tmp)
            }
            "SetForegroundColor" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, color) = self.emit_operand(&args[0]);
                self.emit(&format!("call void @rt_console_set_fg(i32 {color})"));
                ("void".into(), String::new())
            }
            "SetBackgroundColor" => {
                if args.len() != 1 {
                    return None;
                }
                let (_, color) = self.emit_operand(&args[0]);
                self.emit(&format!("call void @rt_console_set_bg(i32 {color})"));
                ("void".into(), String::new())
            }
            "GetForegroundColor" => {
                if !args.is_empty() {
                    return None;
                }
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_console_get_fg()"));
                ("i32".into(), tmp)
            }
            "GetBackgroundColor" => {
                if !args.is_empty() {
                    return None;
                }
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_console_get_bg()"));
                ("i32".into(), tmp)
            }
            "ResetColor" => {
                if !args.is_empty() {
                    return None;
                }
                self.emit("call void @rt_console_reset_color()");
                ("void".into(), String::new())
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 037 M3 UI Element Tree ABI 拦截——`WindowHost.Element*` 系列静态方法。
    ///
    /// 句柄在 Arc 侧为 `long`（i64），C ABI 侧为 `RtUiElement*`（ptr）。调用
    /// rt_ui_* 前对 i64 句柄 emit `inttoptr`，调用后对返回的 ptr emit `ptrtoint`。
    ///
    /// 支持方法：
    ///   - `WindowHost.ElementCreate(string) -> long`
    ///   - `WindowHost.ElementSetString(long, string, string)`
    ///   - `WindowHost.ElementSetNumber(long, string, double)`
    ///   - `WindowHost.ElementSetBool(long, string, int)`
    ///   - `WindowHost.ElementAddChild(long, long)`
    ///   - `WindowHost.RunWithRoot(string, int, int, long)`
    ///
    /// `RunWithRoot` 转发到 4 参 C ABI `__arc_window_run_with_root(title, w, h, root)`，
    /// 元素树渲染路径——内容由 root 指向的 RtUiElement 树承载（Window.Text 字段已废弃）。
    pub(super) fn try_emit_window_host_element(
        &mut self,
        func: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        // Normalize `::` → `.`：`user_type_static_method_func` 将
        // `WindowHost.Method(...)` 静态调用降级为
        // `MirRvalue::Call { func: "WindowHost::Method" }`（与用户类型
        // 静态方法一致），此处统一为 match 使用的 `.` 分隔符，避免
        // 拦截器漏掉、fallthrough 到 stub 用户函数（stub 返回 0
        // 导致 CreateWindow/NativeHandle/RunEventLoop 等全部失效）。
        let func = func.replace("::", ".");
        match func.as_str() {
            "WindowHost.ElementCreate" => {
                let (_, type_name) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString("Element".into())),
                );
                let ptr_tmp = self.fresh_temp();
                self.emit(&format!(
                    "{ptr_tmp} = call ptr @rt_ui_element_create(ptr {type_name})"
                ));
                // ptr → i64（Arc 侧 long 句柄）
                let i64_tmp = self.fresh_temp();
                self.emit(&format!("{i64_tmp} = ptrtoint ptr {ptr_tmp} to i64"));
                Some(("i64".into(), i64_tmp))
            }
            "WindowHost.ElementSetString" => {
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, name) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, value) = self.emit_operand(
                    &args
                        .get(2)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                self.emit(&format!(
                    "call void @rt_ui_element_set_string(ptr {h_val}, ptr {name}, ptr {value})"
                ));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.ElementSetNumber" => {
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, name) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (val_ty, val) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstFloat(0.0)));
                // 数值统一转 double：i32 字面量需 sitofp，double 字面量直接用。
                // LLVM IR 要求 `double <literal>` 必须是浮点字面量（含小数点
                // 或 hex 形式），整数 `8` 会被当作 i32 报 "integer constant
                // must have integer type" 错误。
                let double_val = if val_ty == "double" {
                    val
                } else if val_ty == "i32" || val_ty == "i64" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = sitofp {val_ty} {val} to double"));
                    t
                } else if val_ty == "float" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = fpext float {val} to double"));
                    t
                } else {
                    // 其他类型——尝试直接 emit，让 LLVM 报错以暴露问题
                    val
                };
                self.emit(&format!(
                    "call void @rt_ui_element_set_number(ptr {h_val}, ptr {name}, double {double_val})"
                ));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.ElementSetBool" => {
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, name) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (val_ty, val) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                // i1 布尔字面量需 zext 到 i32 才能匹配 ABI 签名。
                let i32_val = if val_ty == "i32" {
                    val
                } else if val_ty == "i1" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = zext i1 {val} to i32"));
                    t
                } else if val_ty == "i64" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = trunc i64 {val} to i32"));
                    t
                } else {
                    val
                };
                self.emit(&format!(
                    "call void @rt_ui_element_set_bool(ptr {h_val}, ptr {name}, i32 {i32_val})"
                ));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.ElementAddChild" => {
                let (_, parent_ty, parent_ptr) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, child_ty, child_ptr) = self
                    .emit_handle_as_ptr(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_ui_element_add_child(ptr {parent_ptr}, ptr {child_ptr})"
                ));
                let _ = (parent_ty, child_ty);
                Some(("void".into(), String::new()))
            }
            "WindowHost.ElementSetArcPtr" => {
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (arc_ty, arc_val) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let arc_i64 = if arc_ty == "i64" {
                    arc_val
                } else if arc_ty == "ptr" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = ptrtoint ptr {arc_val} to i64"));
                    t
                } else {
                    arc_val
                };
                self.emit(&format!(
                    "call void @rt_ui_element_set_arc_ptr(ptr {h_val}, i64 {arc_i64})"
                ));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetButtonClickHandler" => {
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_button_click_handler(ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetButtonVisualStateHandler" => {
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_button_visual_state_handler(ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            // RFC 037 D10.6 指针路由泛化：按控件类型名注册 click/visual/drag 回调。
            // 与 Button 专用注册并存；type_name 为 args[0]（string），handler 为 args[1]（closure）。
            "WindowHost.SetControlClickHandler" => {
                let (_, type_name) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_control_click_handler(ptr {type_name}, ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetControlVisualStateHandler" => {
                let (_, type_name) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_control_visual_state_handler(ptr {type_name}, ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetControlDragHandler" => {
                let (_, type_name) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_control_drag_handler(ptr {type_name}, ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetInputFocusHandler" => {
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_input_focus_handler(ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetInputClickHandler" => {
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_input_click_handler(ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetKeyboardHandler" => {
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_keyboard_handler(ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.EventPoll" => {
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_event_poll(ptr {h_val})"));
                let _ = h_ty;
                Some(("i32".into(), tmp))
            }
            "WindowHost.WaitEvents" => {
                // A-1② 配套：空闲阻塞等待。2 args: windowHandle (i64) → ptr,
                // timeoutMs (int) → i32（负值 = 无限等待）。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, timeout) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(-1)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_event_wait(ptr {h_val}, i32 {timeout})"
                ));
                let _ = h_ty;
                Some(("i32".into(), tmp))
            }
            "WindowHost.WakeUIThread" => {
                // 跨线程唤醒：无参，投递空消息使阻塞中的 WaitEvents 立即返回。
                self.emit("call void @rt_ui_wake_ui_thread()");
                Some(("void".into(), String::new()))
            }
            "WindowHost.ShouldClose" => {
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_window_should_close(ptr {h_val})"
                ));
                let _ = h_ty;
                Some(("i32".into(), tmp))
            }
            "WindowHost.SetRootElement" => {
                let (_, win_ty, win_ptr) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, root_ty, root_ptr) = self
                    .emit_handle_as_ptr(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_window_set_root_element(ptr {win_ptr}, ptr {root_ptr})"
                ));
                let _ = (win_ty, root_ty);
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetWgpuActive" => {
                // RFC 037: 设置 wgpu 接管渲染标志（WM_PAINT 跳过软件光栅）。
                // 2 args: windowHandle (i64) → ptr, active (int) → i32。
                let (_, win_ty, win_ptr) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, active) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_window_set_wgpu_active(ptr {win_ptr}, i32 {active})"
                ));
                let _ = win_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.ImeInstallHandler" => {
                self.emit("call void @rt_ui_ime_install_arc_handler()");
                Some(("void".into(), String::new()))
            }
            "WindowHost.ClearControlHandlers" => {
                self.emit("call void @rt_ui_clear_control_handlers()");
                Some(("void".into(), String::new()))
            }
            "WindowHost.ImeSetFocus" => {
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!("call void @rt_ui_ime_set_focus(ptr {h_val})"));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.ImeSetCandidateRect" => {
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, x) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, y) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, w) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, h) =
                    self.emit_operand(&args.get(4).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_ui_ime_set_candidate_rect(ptr {h_val}, i32 {x}, i32 {y}, i32 {w}, i32 {h})"
                ));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.NativeCStringFromPtr" => {
                let (_, ptr_ty, ptr_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, empty) = self.emit_operand(&MirOperand::ConstString(String::new()));
                let is_null = self.fresh_temp();
                self.emit(&format!("{is_null} = icmp eq ptr {ptr_val}, null"));
                let null_label = self.fresh_label();
                let copy_label = self.fresh_label();
                let merge_label = self.fresh_label();
                let owned = self.fresh_temp();
                self.emit(&format!(
                    "br i1 {is_null}, label %{null_label}, label %{copy_label}"
                ));
                self.emit(&format!("{null_label}:"));
                self.emit(&format!("br label %{merge_label}"));
                self.emit(&format!("{copy_label}:"));
                let len = self.fresh_temp();
                self.emit(&format!("{len} = call i32 @rt_str_length(ptr {ptr_val})"));
                self.emit(&format!(
                    "{owned} = call ptr @rt_str_substring(ptr {ptr_val}, i32 0, i32 {len})"
                ));
                self.emit(&format!("br label %{merge_label}"));
                self.emit(&format!("{merge_label}:"));
                let result = self.fresh_temp();
                self.emit(&format!(
                    "{result} = phi ptr [ {empty}, %{null_label} ], [ {owned}, %{copy_label} ]"
                ));
                let _ = ptr_ty;
                Some(("ptr".into(), result))
            }
            "WindowHost.ImeCompositionText" => {
                let (_, comp_ty, comp_ptr) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, empty) = self.emit_operand(&MirOperand::ConstString(String::new()));
                let is_null = self.fresh_temp();
                self.emit(&format!("{is_null} = icmp eq ptr {comp_ptr}, null"));
                let null_label = self.fresh_label();
                let read_label = self.fresh_label();
                let merge_label = self.fresh_label();
                let text_ptr = self.fresh_temp();
                let owned = self.fresh_temp();
                self.emit(&format!(
                    "br i1 {is_null}, label %{null_label}, label %{read_label}"
                ));
                self.emit(&format!("{null_label}:"));
                self.emit(&format!("br label %{merge_label}"));
                self.emit(&format!("{read_label}:"));
                self.emit(&format!("{text_ptr} = load ptr, ptr {comp_ptr}"));
                let text_null = self.fresh_temp();
                self.emit(&format!("{text_null} = icmp eq ptr {text_ptr}, null"));
                let text_empty_label = self.fresh_label();
                let text_copy_label = self.fresh_label();
                self.emit(&format!(
                    "br i1 {text_null}, label %{text_empty_label}, label %{text_copy_label}"
                ));
                self.emit(&format!("{text_empty_label}:"));
                self.emit(&format!("br label %{merge_label}"));
                self.emit(&format!("{text_copy_label}:"));
                let len = self.fresh_temp();
                self.emit(&format!("{len} = call i32 @rt_str_length(ptr {text_ptr})"));
                self.emit(&format!(
                    "{owned} = call ptr @rt_str_substring(ptr {text_ptr}, i32 0, i32 {len})"
                ));
                self.emit(&format!("br label %{merge_label}"));
                self.emit(&format!("{merge_label}:"));
                let result = self.fresh_temp();
                self.emit(&format!(
                    "{result} = phi ptr [ {empty}, %{null_label} ], [ {empty}, %{text_empty_label} ], [ {owned}, %{text_copy_label} ]"
                ));
                let _ = comp_ty;
                Some(("ptr".into(), result))
            }

            "WindowHost.HitTest" => {
                let (_, root_ty, root_ptr) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, w) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, h) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, x) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, y) =
                    self.emit_operand(&args.get(4).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let hit_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{hit_ptr} = call ptr @rt_ui_hit_test(ptr {root_ptr}, i32 {w}, i32 {h}, i32 {x}, i32 {y})"
                ));
                let hit_i64 = self.fresh_temp();
                self.emit(&format!("{hit_i64} = ptrtoint ptr {hit_ptr} to i64"));
                let _ = root_ty;
                Some(("i64".into(), hit_i64))
            }
            // ===== RFC 037 M3.5 元素树只读访问器——供 WgpuRender.RenderElementTree 遍历 =====
            "WindowHost.ElementGetTypeName" => {
                // 1 arg: handle (i64) → 转 ptr 调用 rt_ui_element_get_type_name → 返回 ptr (string)。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_ui_element_get_type_name(ptr {h_val})"
                ));
                let _ = h_ty;
                Some(("ptr".into(), tmp))
            }
            "WindowHost.ElementGetString" => {
                // 3 args: handle (i64), name (string), def (string) → 返回 ptr (string)。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, name) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, def_val) = self.emit_operand(
                    &args
                        .get(2)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_ui_element_get_string(ptr {h_val}, ptr {name}, ptr {def_val})"
                ));
                let _ = h_ty;
                Some(("ptr".into(), tmp))
            }
            "WindowHost.ElementGetNumber" => {
                // 3 args: handle (i64), name (string), def (double) → 返回 double。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, name) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (def_ty, def_val) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstFloat(0.0)));
                let def_double = if def_ty == "double" {
                    def_val
                } else if def_ty == "i32" || def_ty == "i64" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = sitofp {def_ty} {def_val} to double"));
                    t
                } else if def_ty == "float" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = fpext float {def_val} to double"));
                    t
                } else {
                    def_val
                };
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call double @rt_ui_element_get_number(ptr {h_val}, ptr {name}, double {def_double})"
                ));
                let _ = h_ty;
                Some(("double".into(), tmp))
            }
            "WindowHost.ElementGetBool" => {
                // 3 args: handle (i64), name (string), def (int) → 返回 i32。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, name) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (def_ty, def_val) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let def_i32 = if def_ty == "i32" {
                    def_val
                } else if def_ty == "i64" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = trunc i64 {def_val} to i32"));
                    t
                } else if def_ty == "i1" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = zext i1 {def_val} to i32"));
                    t
                } else {
                    def_val
                };
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_ui_element_get_bool(ptr {h_val}, ptr {name}, i32 {def_i32})"
                ));
                let _ = h_ty;
                Some(("i32".into(), tmp))
            }
            "WindowHost.ElementGetChildCount" => {
                // 1 arg: handle (i64) → 转 ptr 调用 rt_ui_element_get_child_count → 返回 i32。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_ui_element_get_child_count(ptr {h_val})"
                ));
                let _ = h_ty;
                Some(("i32".into(), tmp))
            }
            "WindowHost.ElementGetChild" => {
                // 2 args: handle (i64), index (int) → 返回 i64 (ptrtoint)。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (idx_ty, idx_val) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let idx_i32 = if idx_ty == "i32" {
                    idx_val
                } else if idx_ty == "i64" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = trunc i64 {idx_val} to i32"));
                    t
                } else {
                    idx_val
                };
                let ptr_tmp = self.fresh_temp();
                self.emit(&format!(
                    "{ptr_tmp} = call ptr @rt_ui_element_get_child(ptr {h_val}, i32 {idx_i32})"
                ));
                let i64_tmp = self.fresh_temp();
                self.emit(&format!("{i64_tmp} = ptrtoint ptr {ptr_tmp} to i64"));
                let _ = h_ty;
                Some(("i64".into(), i64_tmp))
            }
            "WindowHost.RunWithRoot" => {
                // 4 args: title, width, height, root_handle
                // M3 元素树渲染路径——内容由 root_handle 指向的元素树承载，
                // 不再需要 text 参数（Window.Text 字段已废弃）。
                let (_, title) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString("Arc".into())),
                );
                let (_, width) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(640)));
                let (_, height) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(480)));
                // args[3] = root_handle（i64）
                let (root_ty, root_val) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                // 确保句柄为 i64——int 字面量 emit 为 i32 时需 zext
                let root_i64 = if root_ty == "i64" {
                    root_val
                } else {
                    let z = self.fresh_temp();
                    self.emit(&format!("{z} = zext {root_ty} {root_val} to i64"));
                    z
                };
                self.emit(&format!(
                    "call void @__arc_window_run_with_root(ptr {title}, i32 {width}, i32 {height}, i64 {root_i64})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.NativeHandle" => {
                // RFC 037 §D7.2: 提取平台原生窗口 handle（HWND/Window/NSView）。
                // 1 arg: windowHandle (i64) → 转 ptr 调用 rt_window_native_handle → 返回 i64。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i64 @rt_window_native_handle(ptr {h_val})"
                ));
                let _ = h_ty;
                Some(("i64".into(), tmp))
            }
            "WindowHost.CreateWindow" => {
                // RFC 037 §D7.2: 创建平台窗口（不进入消息循环）。
                // 3 args: title, width, height → 返回 ptr（window handle，存为 i64）。
                let (_, title) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString("Arc".into())),
                );
                let (_, width) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(640)));
                let (_, height) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(480)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_window_create(ptr {title}, i32 {width}, i32 {height})"
                ));
                // ptrtoint 到 i64（Arc 侧 long 存储）
                let i64_tmp = self.fresh_temp();
                self.emit(&format!("{i64_tmp} = ptrtoint ptr {tmp} to i64"));
                Some(("i64".into(), i64_tmp))
            }
            "WindowHost.RunEventLoop" => {
                // RFC 037 §D7.2: 进入平台消息循环（阻塞直到窗口关闭）。
                // 1 arg: windowHandle (i64) → 转 ptr 调用 rt_event_poll 循环。
                //
                // 注意：此实现简化为「忙等 rt_event_poll 直到 should_close」——
                // 与 __arc_window_run_with_root 一致。完整实现应在 WM_PAINT/Expose
                // 事件触发时回调 IRenderBackend.BeginFrame/Render/EndFrame。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let loop_label = self.fresh_label();
                let end_label = self.fresh_label();
                self.emit(&format!("br label %{loop_label}"));
                self.emit(&format!("{loop_label}:"));
                self.emit(&format!("call i32 @rt_event_poll(ptr {h_val})"));
                let should_close = self.fresh_temp();
                self.emit(&format!(
                    "{should_close} = call i32 @rt_window_should_close(ptr {h_val})"
                ));
                let cond = self.fresh_temp();
                self.emit(&format!("{cond} = icmp ne i32 {should_close}, 0"));
                self.emit(&format!(
                    "br i1 {cond}, label %{end_label}, label %{loop_label}"
                ));
                self.emit(&format!("{end_label}:"));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }

            "WindowHost.SetScrollWheelHandler" => {
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_scroll_wheel_handler(ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.SetScrollBarHandler" => {
                let (_, closure_ptr) = self.emit_operand_as_closure(
                    &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                );
                let fn_field = self.fresh_temp();
                self.emit(&format!(
                    "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                let fn_ptr = self.fresh_temp();
                self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                let env_field = self.fresh_temp();
                self.emit(&format!(
                    "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let env_ptr = self.fresh_temp();
                self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                self.emit(&format!(
                    "call void @rt_ui_set_scroll_bar_handler(ptr {fn_ptr}, ptr {env_ptr})"
                ));
                Some(("void".into(), String::new()))
            }
            "WindowHost.InvalidateActiveWindow" => {
                self.emit("call void @rt_ui_invalidate_active_window()");
                Some(("void".into(), String::new()))
            }
            "WindowHost.DestroyWindow" => {
                // RFC 037 §D7.2: 销毁平台窗口。
                // 1 arg: windowHandle (i64) → 转 ptr 调用 rt_window_destroy。
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!("call void @rt_window_destroy(ptr {h_val})"));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.GetClientSize" => {
                // 3 args: windowHandle, out int width, out int height
                let (_, h_ty, h_val) = self
                    .emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let w_ptr = self.emit_native_byref_arg(
                    &args.get(1).cloned().unwrap_or(MirOperand::ConstNull),
                    "i32",
                );
                let h_ptr = self.emit_native_byref_arg(
                    &args.get(2).cloned().unwrap_or(MirOperand::ConstNull),
                    "i32",
                );
                self.emit(&format!(
                    "call void @rt_window_get_client_size(ptr {h_val}, {w_ptr}, {h_ptr})"
                ));
                let _ = h_ty;
                Some(("void".into(), String::new()))
            }
            "WindowHost.SystemDpiScale" => {
                // 0 args：返回 double @rt_window_dpi_scale()（DPI / 96.0）。
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call double @rt_window_dpi_scale()"));
                Some(("double".into(), tmp))
            }
            _ => None,
        }
    }

    /// 将 i64 句柄 MirOperand 转换为 ptr LLVM 值。
    ///
    /// 用于 rt_ui_element_* ABI 调用前——Arc 侧句柄为 `long`（i64），
    /// C ABI 侧为 `RtUiElement*`（ptr）。emit `inttoptr i64 %h to ptr`。
    ///
    /// 返回 (operand_ty, ptr_value_name)。operand_ty 通常是 "i64"，
    /// 但若是 i32 字面量（ConstInt）则先 zext 到 i64 再 inttoptr。
    fn emit_handle_as_ptr(&mut self, op: &MirOperand) -> (String, String, String) {
        let (op_ty, op_val) = self.emit_operand(op);
        // 先确保为 i64
        let i64_val = if op_ty == "i64" {
            op_val
        } else if op_ty == "i32" {
            let z = self.fresh_temp();
            self.emit(&format!("{z} = zext i32 {op_val} to i64"));
            z
        } else {
            // ptr 或其他类型——先 ptrtoint 到 i64
            let p = self.fresh_temp();
            self.emit(&format!("{p} = ptrtoint {op_ty} {op_val} to i64"));
            p
        };
        // inttoptr i64 → ptr
        let ptr_tmp = self.fresh_temp();
        self.emit(&format!("{ptr_tmp} = inttoptr i64 {i64_val} to ptr"));
        (op_ty, i64_val, ptr_tmp)
    }

    /// File/Directory/Path 静态方法调用拦截（M1 + M3：基础文件操作 + 目录与路径）。
    ///
    /// 与 Console/Math/Security 一致采用 facade 模式：.as 方法体为空 stub，
    /// codegen 在调用点拦截并直接发射 @rt_file_*/@rt_dir_*/@rt_path_* ABI。
    /// 返回 None 表示未识别的方法，由调用方继续 fallback。
    ///
    /// 设计：
    ///   - bool 返回类型用 i32 表示（0/1）
    ///   - string 返回类型用 ptr 表示（malloc'd NUL-terminated）
    ///   - 错误统一返回 0/NULL，不引入异常机制
    pub(super) fn try_emit_io_static(
        &mut self,
        receiver_type: &str,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        // 注意：emit_method_call_typed 路径下，args 可能包含 receiver 占位（NULL）作为
        // 第一个元素（与 File.ReadAllText 现有拦截一致，用 args.first() 而非 args[0]）。
        // 这里统一用 args.first()/args.get(1) 取参数，不强制 args.len() 检查。
        let result: TyVal = match (receiver_type, method) {
            // ── File 类（M1：基础文件操作） ──
            ("File", "Exists") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_file_exists(ptr {path})"));
                ("i32".into(), tmp)
            }
            ("File", "Delete") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_file_delete(ptr {path})"));
                ("i32".into(), tmp)
            }
            ("File", "AppendAllText") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, content) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_file_append(ptr {path}, ptr {content})"
                ));
                ("i32".into(), tmp)
            }
            ("File", "Copy") => {
                let (_, src) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, dst) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_file_copy(ptr {src}, ptr {dst})"
                ));
                ("i32".into(), tmp)
            }
            ("File", "Move") => {
                let (_, src) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, dst) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_file_move(ptr {src}, ptr {dst})"
                ));
                ("i32".into(), tmp)
            }
            // ── File Async（M5.7：返回 Task*） ──
            ("File", "ReadAllTextAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_read_all_text_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "WriteAllTextAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, content) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_write_all_text_async(ptr {path}, ptr {content})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "AppendAllTextAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, content) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_append_all_text_async(ptr {path}, ptr {content})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "CopyAsync") => {
                let (_, src) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, dst) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_copy_async(ptr {src}, ptr {dst})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "MoveAsync") => {
                let (_, src) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, dst) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_move_async(ptr {src}, ptr {dst})"
                ));
                ("ptr".into(), tmp)
            }
            // ── IO Async 补全（RFC 009 异步优先）：File 其余异步面 ──
            ("File", "ReadAllLinesAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_read_all_lines_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "ReadAllBytesAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_read_all_bytes_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "WriteAllBytesAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, bytes) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_write_all_bytes_async(ptr {path}, ptr {bytes})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "DeleteAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_delete_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "ExistsAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_exists_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            // FileStream 工厂（标准库就绪 P0）：委托 emit_new(FileStream, path, mode)
            ("File", "OpenRead") | ("File", "OpenText") => {
                let path = args
                    .first()
                    .cloned()
                    .unwrap_or(MirOperand::ConstString(String::new()));
                self.emit_new("FileStream", &[path, MirOperand::ConstInt(0)], &[])
            }
            ("File", "OpenWrite") => {
                let path = args
                    .first()
                    .cloned()
                    .unwrap_or(MirOperand::ConstString(String::new()));
                self.emit_new("FileStream", &[path, MirOperand::ConstInt(1)], &[])
            }
            ("File", "Create") => {
                let path = args
                    .first()
                    .cloned()
                    .unwrap_or(MirOperand::ConstString(String::new()));
                self.emit_new("FileStream", &[path, MirOperand::ConstInt(2)], &[])
            }
            // ── Directory 类（M3：目录操作） ──
            ("Directory", "CreateDirectory") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_dir_create(ptr {path})"));
                ("i32".into(), tmp)
            }
            ("Directory", "Exists") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_dir_exists(ptr {path})"));
                ("i32".into(), tmp)
            }
            ("Directory", "Delete") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_dir_delete(ptr {path})"));
                ("i32".into(), tmp)
            }
            ("Directory", "GetFiles") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                // 模式匹配在 runtime（rt_dir_list_files_pattern）；此处仅按 arity 分派。
                if args.len() >= 2 {
                    let (_, pat) = self.emit_operand(
                        &args
                            .get(1)
                            .cloned()
                            .unwrap_or(MirOperand::ConstString(String::new())),
                    );
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_dir_list_files_pattern(ptr {path}, ptr {pat})"
                    ));
                } else {
                    self.emit(&format!("{tmp} = call ptr @rt_dir_list_files(ptr {path})"));
                }
                ("ptr".into(), tmp)
            }
            ("Directory", "GetDirectories") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_dir_list_dirs(ptr {path})"));
                ("ptr".into(), tmp)
            }
            // ── Directory 异步面（RFC 009 异步优先）──
            ("Directory", "CreateDirectoryAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_dir_create_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("Directory", "ExistsAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_dir_exists_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("Directory", "DeleteAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_dir_delete_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("Directory", "GetFilesAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                // 与同步 GetFiles 同策：按 arity 分派（pattern 版本）。
                if args.len() >= 2 {
                    let (_, pat) = self.emit_operand(
                        &args
                            .get(1)
                            .cloned()
                            .unwrap_or(MirOperand::ConstString(String::new())),
                    );
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_dir_list_files_pattern_async(ptr {path}, ptr {pat})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_dir_list_files_async(ptr {path})"
                    ));
                }
                ("ptr".into(), tmp)
            }
            ("Directory", "GetDirectoriesAsync") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_dir_list_dirs_async(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            // ── Path 类（M3：路径操作，纯字符串计算） ──
            ("Path", "Combine") => {
                let (_, a) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, b) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_path_combine(ptr {a}, ptr {b})"
                ));
                ("ptr".into(), tmp)
            }
            ("Path", "GetDirectoryName") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_path_get_dir_name(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("Path", "GetFileName") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_path_get_file_name(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("Path", "GetFileNameWithoutExtension") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_path_get_file_name_without_ext(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("Path", "GetExtension") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_path_get_extension(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("Path", "ChangeExtension") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, ext) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_path_change_extension(ptr {path}, ptr {ext})"
                ));
                ("ptr".into(), tmp)
            }
            ("Path", "HasExtension") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_path_has_extension(ptr {path})"
                ));
                ("i32".into(), tmp)
            }
            ("Path", "GetTempPath") => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_path_get_temp_path()"));
                ("ptr".into(), tmp)
            }
            ("File", "ReadAllBytes") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_read_all_bytes(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            ("File", "WriteAllBytes") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, bytes) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_file_write_all_bytes(ptr {path}, ptr {bytes})"
                ));
                ("i32".into(), tmp)
            }
            ("File", "ReadAllLines") => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_read_all_lines(ptr {path})"
                ));
                ("ptr".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// FileStream 实例方法拦截（标准库就绪 P0）。
    /// `_handle` 位于对象头后 offset 16；与 ConcurrentQueue 布局契约一致。
    pub(super) fn try_emit_file_stream_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let handle_addr = self.fresh_temp();
        self.emit(&format!(
            "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

        let method = method.strip_prefix("get_").unwrap_or(method);
        Some(match method {
            "CanRead" | "can_read" => {
                let raw = self.fresh_temp();
                self.emit(&format!(
                    "{raw} = call i32 @rt_file_stream_can_read(ptr {handle})"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                ("i1".into(), tmp)
            }
            "CanWrite" | "can_write" => {
                let raw = self.fresh_temp();
                self.emit(&format!(
                    "{raw} = call i32 @rt_file_stream_can_write(ptr {handle})"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                ("i1".into(), tmp)
            }
            "CanSeek" | "can_seek" => {
                let raw = self.fresh_temp();
                self.emit(&format!(
                    "{raw} = call i32 @rt_file_stream_can_seek(ptr {handle})"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                ("i1".into(), tmp)
            }
            "Length" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i64 @rt_file_stream_get_length(ptr {handle})"
                ));
                ("i64".into(), tmp)
            }
            "_getPosition" | "get_Position" | "Position" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i64 @rt_file_stream_get_position(ptr {handle})"
                ));
                ("i64".into(), tmp)
            }
            "_setPosition" | "set_Position" => {
                let (ty, val) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let as_i64 = if ty == "i64" {
                    val
                } else {
                    let w = self.fresh_temp();
                    self.emit(&format!("{w} = sext {ty} {val} to i64"));
                    w
                };
                self.emit(&format!(
                    "call void @rt_file_stream_set_position(ptr {handle}, i64 {as_i64})"
                ));
                ("void".into(), String::new())
            }
            "Read" => {
                let (_, buf) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, off) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, cnt) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_file_stream_read(ptr {handle}, ptr {buf}, i32 {off}, i32 {cnt})"
                ));
                ("i32".into(), tmp)
            }
            "Write" => {
                let (_, buf) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, off) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, cnt) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_file_stream_write(ptr {handle}, ptr {buf}, i32 {off}, i32 {cnt})"
                ));
                ("void".into(), String::new())
            }
            "Seek" => {
                let (off_ty, off) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, origin) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let off64 = if off_ty == "i64" {
                    off
                } else {
                    let w = self.fresh_temp();
                    self.emit(&format!("{w} = sext {off_ty} {off} to i64"));
                    w
                };
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i64 @rt_file_stream_seek(ptr {handle}, i64 {off64}, i32 {origin})"
                ));
                ("i64".into(), tmp)
            }
            "SetLength" => {
                let (ty, val) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let val64 = if ty == "i64" {
                    val
                } else {
                    let w = self.fresh_temp();
                    self.emit(&format!("{w} = sext {ty} {val} to i64"));
                    w
                };
                self.emit(&format!(
                    "call void @rt_file_stream_set_length(ptr {handle}, i64 {val64})"
                ));
                ("void".into(), String::new())
            }
            "Flush" => {
                self.emit(&format!("call void @rt_file_stream_flush(ptr {handle})"));
                ("void".into(), String::new())
            }
            // 真异步（文件 I/O 专用池卸载 + 完成投递；rt_file_stream_async.c）：
            // 返回 Pending Task*（ptr），await 侧统一走 Task 挂起/恢复链路。
            // CT 参数省略时（= default 经 typeck 补全或直接省参）传 null（不可取消）。
            "ReadAsync" => {
                let (_, buf) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, off) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, cnt) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let ct = match args.get(3) {
                    Some(op) => self.emit_operand(op).1,
                    None => "null".to_string(),
                };
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_stream_read_async(ptr {handle}, ptr {buf}, i32 {off}, i32 {cnt}, ptr {ct})"
                ));
                ("ptr".into(), tmp)
            }
            "WriteAsync" => {
                let (_, buf) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, off) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, cnt) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let ct = match args.get(3) {
                    Some(op) => self.emit_operand(op).1,
                    None => "null".to_string(),
                };
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_stream_write_async(ptr {handle}, ptr {buf}, i32 {off}, i32 {cnt}, ptr {ct})"
                ));
                ("ptr".into(), tmp)
            }
            "FlushAsync" => {
                let ct = match args.first() {
                    Some(op) => self.emit_operand(op).1,
                    None => "null".to_string(),
                };
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_stream_flush_async(ptr {handle}, ptr {ct})"
                ));
                ("ptr".into(), tmp)
            }
            "Dispose" | "_closeHandle" | "Close" => {
                self.emit(&format!("call void @rt_file_stream_close(ptr {handle})"));
                self.emit(&format!("store ptr null, ptr {handle_addr}"));
                ("void".into(), String::new())
            }
            _ => return None,
        })
    }

    /// Task facade (RFC 009 M1): Task静态方法调用拦截。
    ///
    /// 与 Console/Math/IO 一致采用 facade 模式：std/Arc/Tasks/Task.as 中 Task 类为
    /// stub，方法体不执行；codegen 在调用点拦截并直接发射 rt_task_* ABI。
    /// 返回 None 表示未识别的方法，由调用方继续 fallback。
    ///
    /// `expected` 是 typeck 推断的返回类型：多数工厂/组合子为 `Task<T>`；
    /// `WaitAll` → void、`WaitAny` → int（不依赖 Task expected，须先于守卫处理）。
    pub(super) fn try_emit_task_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
        expected: &TypeId,
    ) -> Option<TyVal> {
        // WaitAll / WaitAny：返回 void / int，须在 Task expected 守卫之前处理。
        match method {
            "WaitAll" => {
                let arg = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                if !matches!(&arg, MirOperand::ConstNull | MirOperand::ConstInt(_)) {
                    let (_, span_ptr) = self.emit_operand(&arg);
                    let (data, len) = self.emit_unpack_span(&span_ptr);
                    self.emit(&format!(
                        "call void @rt_task_wait_all(ptr {data}, i32 {len})"
                    ));
                }
                return Some(("void".into(), String::new()));
            }
            "WaitAny" => {
                let arg = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                let tmp = self.fresh_temp();
                if matches!(&arg, MirOperand::ConstNull | MirOperand::ConstInt(_)) {
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_task_wait_any(ptr null, i32 0)"
                    ));
                } else {
                    let (_, span_ptr) = self.emit_operand(&arg);
                    let (data, len) = self.emit_unpack_span(&span_ptr);
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_task_wait_any(ptr {data}, i32 {len})"
                    ));
                }
                return Some(("i32".into(), tmp));
            }
            _ => {}
        }

        // Delay 结果恒为 Task<Void>，与 expected 无关。异步状态机路径可能以非 typed
        // `emit_rvalue`（expected = TypeId::Int）发射该语句，故必须在 expected Task
        // 门控之前处理，否则 `Task.Delay` 会穿透为 `@Task.Delay` 未定义符号。
        if method == "Delay" {
            return match args.len() {
                1 => {
                    let arg = args.first().cloned().unwrap_or(MirOperand::ConstInt(0));
                    let (_, val) = self.emit_operand(&arg);
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = call ptr @rt_task_delay(i32 {val})"));
                    Some(("ptr".into(), tmp))
                }
                2 => {
                    let arg0 = args.first().cloned().unwrap_or(MirOperand::ConstInt(0));
                    let arg1 = args.get(1).cloned().unwrap_or(MirOperand::ConstNull);
                    let (_, ms) = self.emit_operand(&arg0);
                    let (_, ct) = self.emit_operand(&arg1);
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_task_delay_ct(i32 {ms}, ptr {ct})"
                    ));
                    Some(("ptr".into(), tmp))
                }
                _ => None,
            };
        }

        // M5.7：语句级 `Task.Run(...)`（返回值丢弃，expected 非 Task 类型）与
        // Delay 同策在 expected Task 门控前处理——否则穿透为 `@Task.Run` 未定义
        // 符号（task_run_stmt 探针实证：`Task.Run(() => ...);` 语句形态必现；
        // 赋值/await 形态 expected=Task<T> 走下方门控后分支，泛型 Func<T> 判定
        // 不受影响）。非 Task expected 恒为 Action 路径（`Task.Run(Action)`）。
        if method == "Run" && !matches!(expected, TypeId::Task { .. }) {
            let func = args.first().cloned().unwrap_or(MirOperand::ConstNull);
            let (fn_val, data_val) = self.emit_task_run_fn_env(&func);
            let tmp = self.fresh_temp();
            if args.len() >= 2 {
                let (_, extra) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!(
                    "{tmp} = call ptr @rt_task_run_on_pool(ptr {extra}, ptr {fn_val}, ptr {data_val})"
                ));
            } else {
                self.emit(&format!(
                    "{tmp} = call ptr @rt_task_run(ptr {fn_val}, ptr {data_val})"
                ));
            }
            return Some(("ptr".into(), tmp));
        }

        let inner = match expected {
            TypeId::Task { inner } => inner.as_ref(),
            _ => return None,
        };
        let result: TyVal = match method {
            "FromResult" => {
                let arg = args.first().cloned().unwrap_or(MirOperand::ConstInt(0));
                let (arg_ty, arg_val) = self.emit_operand(&arg);
                let tmp = self.fresh_temp();
                match inner {
                    // 整数族：截断/零扩展到 i32 后调用 rt_task_from_int。
                    // bool/short/byte 等窄整型（i1/i8/i16）不能直接作 i32 实参——
                    // 先拓宽（bool/byte/ushort/uint 无符号 zext；short/sbyte 有符号 sext）。
                    TypeId::Int
                    | TypeId::Short
                    | TypeId::Byte
                    | TypeId::Char
                    | TypeId::Bool
                    | TypeId::UInt
                    | TypeId::UShort
                    | TypeId::SByte => {
                        let int_arg = if arg_ty == "i32" {
                            arg_val.clone()
                        } else {
                            let w = self.fresh_temp();
                            let ext = if matches!(inner, TypeId::Short | TypeId::SByte) {
                                "sext"
                            } else {
                                "zext"
                            };
                            self.emit(&format!("{w} = {ext} {arg_ty} {arg_val} to i32"));
                            w
                        };
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_task_from_int(i32 {int_arg})"
                        ));
                    }
                    // 引用族：直接传递 ptr
                    TypeId::String
                    | TypeId::Named(_)
                    | TypeId::Array { .. }
                    | TypeId::Task { .. }
                    | TypeId::Func { .. } => {
                        // `Task<接口>` 结果必须存**堆盒**（`{ obj, itable }`，与接口
                        // 函数返回值同构）：旧实现经 `rt_task_from_ptr` 仅存裸 obj
                        // （丢 vtable 的编译器缺口）→ 调用方 `(TcpConnection)await
                        // ...` 把裸 obj 当胖指针盒解引用 → 0xC0000005（p2p_dial_e2e
                        // 实测）。装箱同时 retain obj（与 class 路径 task 持 +1 对偶；
                        // 缺 retain 时调用方局部出口 dec 会提前释放 obj → 盒悬垂）。
                        // 实现类经运行时 type_id 分派（`(IConnection)x` 转型在 MIR
                        // 被折叠为裸 obj，静态类名不可得）；仅对已物化局部（Local）
                        // 触发，ConstNull 等常量保持旧透传。
                        let iface_box = match inner {
                            TypeId::Named(n) if is_iface_name(n.as_str()) => {
                                matches!(arg, MirOperand::Local(_)).then(|| n.as_str())
                            }
                            _ => None,
                        };
                        match iface_box {
                            Some(iface) => {
                                // 装箱同时 retain obj（`emit_make_iface_dyn` 堆盒路径
                                // 已含 `rt_arc_inc`，与 class 路径 task 持 +1 对偶；
                                // 缺 retain 时调用方局部出口 dec 会提前释放 obj →
                                // 盒悬垂）。实现类经运行时 type_id 分派（`(IConnection)x`
                                // 转型在 MIR 被折叠为裸 obj，静态类名不可得）。
                                let (_, box_val) = self.emit_make_iface_dyn(iface, &arg, true);
                                self.emit(&format!(
                                    "{tmp} = call ptr @rt_task_from_ptr(ptr {box_val})"
                                ));
                            }
                            None => {
                                // class 值：task 强持有独立 +1（RFC 009 §结果
                                // 所有权收敛：rt_task_from_class 置位，release
                                // 统一 dec）。inc 授予 task +1——缺 retain 时
                                // 调用方局部出口 dec 会把 rc=1 值（如
                                // `Task.FromResult(AIToolResult.Ok(...))`）提前
                                // 释放 → task 悬垂 → await 提取 UAF（stream_events
                                // 实测）。string/array/Func 委托无 ArcHeader
                                // （immortal 借用），不 inc、走 from_ptr。
                                if Self::arc_class_place(inner, self.layouts) {
                                    self.emit(&format!("call void @rt_arc_inc(ptr {arg_val})"));
                                    self.emit(&format!(
                                        "{tmp} = call ptr @rt_task_from_class(ptr {arg_val})"
                                    ));
                                } else {
                                    self.emit(&format!(
                                        "{tmp} = call ptr @rt_task_from_ptr(ptr {arg_val})"
                                    ));
                                }
                            }
                        }
                    }
                    // 值类型族：alloca + store + rt_task_from_value
                    TypeId::Float | TypeId::Double | TypeId::Long | TypeId::ULong => {
                        let slot = self.fresh_temp();
                        let size = if matches!(inner, TypeId::Long | TypeId::ULong) {
                            8
                        } else {
                            4
                        };
                        let llvm_ty = if matches!(inner, TypeId::Long | TypeId::ULong) {
                            "i64".to_string()
                        } else if matches!(inner, TypeId::Float) {
                            "float".to_string()
                        } else {
                            "double".to_string()
                        };
                        self.emit(&format!("{slot} = alloca {llvm_ty}"));
                        self.emit(&format!("store {arg_ty} {arg_val}, ptr {slot}"));
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_task_from_value(ptr {slot}, i32 {size})"
                        ));
                    }
                    // 非泛型 `Task.FromResult(x)`：目标为 `Task`（inner Void），
                    // 值被丢弃，返回已完成 void task。此前落入 `_ =>` 兜底发射
                    // `rt_task_from_ptr(ptr {i32 值})` → clang `integer constant must
                    // have integer type`（`MemoryStreamTransport.WriteBytesAsync`
                    // 返回 `Task.FromResult(0)`；body 经 vtable force-keep 暴露）。
                    TypeId::Void => {
                        self.emit(&format!("{tmp} = call ptr @rt_task_void()"));
                    }
                    TypeId::Vector { elem, n } => {
                        // Vector<T, N> 是值类型，按字节大小拷贝
                        let elem_size = if matches!(**elem, TypeId::Float) {
                            4
                        } else {
                            8
                        };
                        let size = elem_size * (*n as i32);
                        let llvm_ty = format!(
                            "<{n} x {}>",
                            if matches!(**elem, TypeId::Float) {
                                "float"
                            } else {
                                "double"
                            }
                        );
                        let slot = self.fresh_temp();
                        self.emit(&format!("{slot} = alloca {llvm_ty}"));
                        self.emit(&format!("store {arg_ty} {arg_val}, ptr {slot}"));
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_task_from_value(ptr {slot}, i32 {size})"
                        ));
                    }
                    // 其他类型（struct 等）暂用 ptr 兜底
                    _ => {
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_task_from_ptr(ptr {arg_val})"
                        ));
                    }
                }
                ("ptr".into(), tmp)
            }
            "WhenAll" | "WhenAny" => {
                // RFC 005 dogfood：`params ReadOnlySpan<Task>` → 解包 Span 胖指针后调 ABI。
                // 两种实参形态：
                //   - 同步路径：MIR 已把 params 物化为单一 `{ptr, i32}` span 实参 → 直接解包。
                //   - 异步状态机路径：MIR 未物化 span，把各 Task 作为独立实参传入
                //     （如 `WhenAny(t1, t2)` → args=[t1, t2]）→ 须合成 [N x ptr] 数组 + span。
                // 统一：args.len() >= 2 时合成数组；否则按单 span 实参处理。
                let abi = if method == "WhenAll" {
                    "@rt_task_when_all"
                } else {
                    "@rt_task_when_any"
                };
                let tmp = self.fresh_temp();
                if args.len() >= 2 {
                    let n = args.len();
                    let arr = self.fresh_temp();
                    self.emit(&format!("{arr} = alloca [{n} x ptr]"));
                    for (i, a) in args.iter().enumerate() {
                        let (_, v) = self.emit_operand(a);
                        let slot = self.fresh_temp();
                        self.emit(&format!(
                            "{slot} = getelementptr inbounds [{n} x ptr], ptr {arr}, i32 0, i32 {i}"
                        ));
                        self.emit(&format!("store ptr {v}, ptr {slot}"));
                    }
                    self.emit(&format!("{tmp} = call ptr {abi}(ptr {arr}, i32 {n})"));
                } else {
                    let arg = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                    if matches!(&arg, MirOperand::ConstNull | MirOperand::ConstInt(_)) {
                        self.emit(&format!("{tmp} = call ptr {abi}(ptr null, i32 0)"));
                    } else {
                        let (_, span_ptr) = self.emit_operand(&arg);
                        let (data, len) = self.emit_unpack_span(&span_ptr);
                        self.emit(&format!("{tmp} = call ptr {abi}(ptr {data}, i32 {len})"));
                    }
                }
                ("ptr".into(), tmp)
            }
            "CompletedTask" => {
                // 已完成的 void Task（M1 同步路径，rt_task_void 返回 READY Task）
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_task_void()"));
                ("ptr".into(), tmp)
            }
            "Run" => {
                // M5.7: Task.Run(Action) / Task.Run<T>(Func<T>)
                // M5.7: Task.Run(Action, ThreadPoolScheduler) / Task.Run<T>(Func<T>, CancellationToken)
                // 通过 expected 区分：Task<Void> → Action 路径，Task<T> → Func<T> 路径
                // FnPtr（无捕获 lambda）不得对函数符号做 {ptr,ptr} GEP——须合成 arc_closure
                // 或直接传 @fn + null env（与 ct.Register / ContinueWith 同契约）。
                let is_action = matches!(inner, TypeId::Void);
                let func = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                let (fn_val, data_val) = self.emit_task_run_fn_env(&func);
                let tmp = self.fresh_temp();
                if args.len() >= 2 {
                    let (_, extra) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                    if is_action {
                        self.emit(&format!("{tmp} = call ptr @rt_task_run_on_pool(ptr {extra}, ptr {fn_val}, ptr {data_val})"));
                    } else {
                        self.emit(&format!("{tmp} = call ptr @rt_task_run_func_ct(ptr {fn_val}, ptr {data_val}, ptr {extra})"));
                    }
                } else if is_action {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_task_run(ptr {fn_val}, ptr {data_val})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_task_run_func(ptr {fn_val}, ptr {data_val})"
                    ));
                }
                ("ptr".into(), tmp)
            }
            // M5.7: Task.FromCanceled(CancellationToken) → @rt_task_from_canceled()
            "FromCanceled" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_task_from_canceled()"));
                ("ptr".into(), tmp)
            }
            // Task.FromException(Exception) → @rt_task_from_exception(ex)
            // RFC 016 子项 M2：异常所有权转移给 Task——发射前 rt_arc_inc 使 Task 持独立
            // +1（与 rt_task_release 对 FAULTED 的 dec 配对）。调用方局部仍持其 +1、
            // 正常自 dec（符合 .NET 语义：传入 FromException 不使调用方引用失效），
            // 与「调用方不再自持 dec」净效果等价，且无需改局部析构语义。
            "FromException" => {
                let arg = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                let (_, ex) = self.emit_operand(&arg);
                self.emit(&format!("call void @rt_arc_inc(ptr {ex})"));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_task_from_exception(ptr {ex})"
                ));
                ("ptr".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 008：实参是否为委托值（须以 `arc_closure*` 跨函数传递）。
    fn operand_is_delegate_value(&self, op: &MirOperand) -> bool {
        match op {
            MirOperand::Closure { .. } | MirOperand::FnPtr { .. } => true,
            MirOperand::Local(id) => {
                self.closure_locals.contains(id)
                    || self
                        .cfg
                        .locals
                        .get(id)
                        .is_some_and(|(_, ty)| is_delegate_type(ty))
            }
            MirOperand::Field { class, field, .. } => self.field_is_delegate(class, field),
            _ => false,
        }
    }

    /// 字段是否为委托类型（Action_*/Func_* / 裸 Action/Func）。字段类型名来自
    /// layouts 的 field_info/struct_field_info（mangled 名，如 `Action_string`）。
    fn field_is_delegate(&self, class: &str, field: &str) -> bool {
        let field_ty = self.field_type_name(class, field);
        field_ty == "Action"
            || field_ty == "Func"
            || field_ty.starts_with("Action_")
            || field_ty.starts_with("Func_")
    }

    /// 字段委托的返回 LLVM 类型（Action → void；Func_x → x 的 LLVM 类型）。
    /// 非委托字段返回 None。
    fn field_delegate_ret_llvm_ty(&self, class: &str, field: &str) -> Option<String> {
        let field_ty = self.field_type_name(class, field);
        if field_ty.starts_with("Action") {
            return Some("void".into());
        }
        if field_ty.starts_with("Func") {
            let rest = field_ty.strip_prefix("Func_")?;
            // 嵌套 Func/Action mangling 不支持（与 delegate_ret_type 一致）。
            if rest.contains("Func_") || rest.contains("Action_") {
                return None;
            }
            // 单参 Func_X 整段即返回类型（Func_Task_int → Task_int，ptr）；
            // 字段委托调用点无实参计数可辨多参，按单参整段处理。
            let ret_part = rest;
            return Some(llvm_type_of(
                &super::types::demangle_simple_type_part(ret_part),
                self.layouts,
            ));
        }
        None
    }

    /// 解析类/结构体字段的 Arc 类型名（mangled）。
    fn field_type_name(&self, class: &str, field: &str) -> String {
        if self.layouts.structs.contains_key(class) {
            self.struct_field_info(class, field).1
        } else {
            self.field_info(class, field).1
        }
    }

    /// Extract `(fn_ptr, env_ptr)` for `Task.Run` / `ThreadPoolScheduler.Run`.
    ///
    /// No-capture lambdas lower to `MirOperand::FnPtr` (bare function symbol).
    /// Treating that symbol as `%arc_closure` and GEP-loading fields reads the
    /// machine code as pointers — body never runs. Pass `@fn` + `null` env
    /// directly; capture lambdas go through `emit_operand_as_closure`.
    pub(super) fn emit_task_run_fn_env(&mut self, func: &MirOperand) -> (String, String) {
        match func {
            MirOperand::FnPtr { name } => {
                let fn_global = format!("@{}", mangle_fn_name(name));
                (fn_global, "null".into())
            }
            _ => {
                let (_, closure_ptr) = self.emit_operand_as_closure(func);
                let fn_tmp = self.fresh_temp();
                let data_tmp = self.fresh_temp();
                self.emit(&format!(
                    "{fn_tmp} = getelementptr inbounds %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
                ));
                self.emit(&format!(
                    "{data_tmp} = getelementptr inbounds %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
                ));
                let fn_val = self.fresh_temp();
                let data_val = self.fresh_temp();
                self.emit(&format!("{fn_val} = load ptr, ptr {fn_tmp}"));
                self.emit(&format!("{data_val} = load ptr, ptr {data_tmp}"));
                (fn_val, data_val)
            }
        }
    }

    /// Task facade (RFC 009 M1): Task<T>/Task 实例方法与 property getter 拦截。
    ///
    /// 覆盖两类调用：
    ///   - property getter（由 MIR lower 从 `t.Prop` 转换而来）：`get_Result`/
    ///     `get_Status`/`get_IsCanceled`/`get_IsCompleted`
    ///   - 实例方法：`Wait`/`Cancel`/`GetResult`（方法形式访问结果）
    ///
    /// `expected` 是 typeck 推断的返回类型，用于 `get_Result` 选择 ABI 分支：
    ///   - Int 族 → `rt_task_result_int`（返回 i32）
    ///   - 引用族 → `rt_task_result_ptr`（返回 ptr）
    ///   - 值类型族 → `rt_task_result_value`（alloca + load）
    pub(super) fn try_emit_task_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        expected: &TypeId,
    ) -> Option<TyVal> {
        // property getter 形式统一去掉 "get_" 前缀，便于匹配
        let prop = method.strip_prefix("get_").unwrap_or("");
        let result: TyVal = match method {
            "Wait" => {
                // M5.7: task.Wait() / task.Wait(int timeoutMs) / task.Wait(CancellationToken)
                let (_, recv) = self.emit_operand(receiver);
                if args.is_empty() {
                    // 无参 Wait：poll 一次（M1 同步路径）
                    self.emit(&format!("call i32 @rt_task_poll(ptr {recv})"));
                    ("void".into(), String::new())
                } else {
                    let arg0 = &args[0];
                    let (arg_ty, arg_val) = self.emit_operand(arg0);
                    let tmp = self.fresh_temp();
                    if arg_ty == "ptr" {
                        // Wait(CancellationToken): 通过 ptr 类型区分 CT vs int
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_task_wait_ct(ptr {recv}, ptr {arg_val})"
                        ));
                    } else {
                        // Wait(int timeoutMs)
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_task_wait_timeout(ptr {recv}, i32 {arg_val})"
                        ));
                    }
                    // 返回 bool（1=完成/true, 0=超时/false），但 Arc 期望 bool 为 i1
                    let bool_tmp = self.fresh_temp();
                    self.emit(&format!("{bool_tmp} = trunc i32 {tmp} to i1"));
                    ("i1".into(), bool_tmp)
                }
            }
            "Cancel" => {
                let (_, recv) = self.emit_operand(receiver);
                self.emit(&format!("call void @rt_task_cancel(ptr {recv})"));
                ("void".into(), String::new())
            }
            "GetResult" | "get_Result" => {
                let (_, recv) = self.emit_operand(receiver);
                let tmp = self.fresh_temp();
                match expected {
                    // 整数族：rt_task_result_int 返回 i32
                    TypeId::Int
                    | TypeId::Short
                    | TypeId::Byte
                    | TypeId::Char
                    | TypeId::Bool
                    | TypeId::UInt
                    | TypeId::UShort
                    | TypeId::SByte => {
                        self.emit(&format!("{tmp} = call i32 @rt_task_result_int(ptr {recv})"));
                        ("i32".into(), tmp)
                    }
                    // 引用族：rt_task_result_ptr 返回 ptr
                    TypeId::String
                    | TypeId::Named(_)
                    | TypeId::Array { .. }
                    | TypeId::Task { .. }
                    | TypeId::Func { .. } => {
                        self.emit(&format!("{tmp} = call ptr @rt_task_result_ptr(ptr {recv})"));
                        // CD-29 根因修复（与 await 提取 C12 语义对齐）：
                        // `task.Result`/`GetResult()` 同步提取返回 ptr_result 的
                        // 「借引用」（rt_task_release 不 dec ptr_result）。class
                        // 结果须 retain 授予调用方独立 +1，与命名局部 epilogue dec
                        // 配对——缺 retain 时 rc=1 结果被调用方局部出口 dec 提前
                        // 释放 → 悬垂 → UAF（web_core_auth_concurrency 的
                        // EndpointDispatcher.Dispatch `resp = task.Result` 实测：
                        // 每请求 InvalidCast / free DUP）。string/array/Task/Func
                        // 无 ArcHeader 不 inc。
                        if Self::arc_class_place(expected, self.layouts) {
                            self.emit(&format!("call void @rt_arc_inc(ptr {tmp})"));
                        }
                        ("ptr".into(), tmp)
                    }
                    // 值类型族：rt_task_result_value 需 alloca + load
                    TypeId::Float | TypeId::Double | TypeId::Long | TypeId::ULong => {
                        let (llvm_ty, size) = if matches!(expected, TypeId::Long | TypeId::ULong) {
                            ("i64".to_string(), 8)
                        } else if matches!(expected, TypeId::Float) {
                            ("float".to_string(), 4)
                        } else {
                            ("double".to_string(), 8)
                        };
                        let slot = self.fresh_temp();
                        self.emit(&format!("{slot} = alloca {llvm_ty}"));
                        self.emit(&format!(
                            "call void @rt_task_result_value(ptr {recv}, ptr {slot}, i32 {size})"
                        ));
                        self.emit(&format!("{tmp} = load {llvm_ty}, ptr {slot}"));
                        (llvm_ty, tmp)
                    }
                    TypeId::Vector { elem, n } => {
                        let elem_size = if matches!(**elem, TypeId::Float) {
                            4
                        } else {
                            8
                        };
                        let size = elem_size * (*n as i32);
                        let llvm_ty = format!(
                            "<{n} x {}>",
                            if matches!(**elem, TypeId::Float) {
                                "float"
                            } else {
                                "double"
                            }
                        );
                        let slot = self.fresh_temp();
                        self.emit(&format!("{slot} = alloca {llvm_ty}"));
                        self.emit(&format!(
                            "call void @rt_task_result_value(ptr {recv}, ptr {slot}, i32 {size})"
                        ));
                        self.emit(&format!("{tmp} = load {llvm_ty}, ptr {slot}"));
                        (llvm_ty, tmp)
                    }
                    // 其他类型默认 ptr 兜底
                    _ => {
                        self.emit(&format!("{tmp} = call ptr @rt_task_result_ptr(ptr {recv})"));
                        ("ptr".into(), tmp)
                    }
                }
            }
            "get_Status" => {
                let (_, recv) = self.emit_operand(receiver);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_task_status(ptr {recv})"));
                ("i32".into(), tmp)
            }
            "get_IsCanceled" => {
                let (_, recv) = self.emit_operand(receiver);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_task_is_canceled(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            "get_IsFaulted" => {
                let (_, recv) = self.emit_operand(receiver);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_task_is_faulted(ptr {recv})"));
                ("i32".into(), tmp)
            }
            "get_Exception" => {
                let (_, recv) = self.emit_operand(receiver);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_task_get_exception(ptr {recv})"
                ));
                ("ptr".into(), tmp)
            }
            "get_IsCompleted" => {
                // RT_TASK_READY == 0；status == 0 即已完成
                let (_, recv) = self.emit_operand(receiver);
                let raw = self.fresh_temp();
                self.emit(&format!("{raw} = call i32 @rt_task_status(ptr {recv})"));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = icmp eq i32 {raw}, 0"));
                ("i1".into(), tmp)
            }
            "ConfigureAwait" => {
                // 恒等映射返回 this（Arc 无 SynchronizationContext，语义正确）
                self.emit_operand(receiver)
            }
            _ => return None,
        };
        // 静默未使用变量警告（prop 在 match 后未使用，仅用于调试时排查）
        let _ = prop;
        Some(result)
    }

    /// CancellationTokenSource facade (RFC 009 M4): CTS 实例方法拦截。
    ///
    /// 覆盖：Cancel/CancelAfter/Token/IsCancellationRequested/Dispose。
    /// CT 与 CTS 共享同一 RtCts* 指针（D2 决策），get_Token 直接返回 receiver。
    pub(super) fn try_emit_cts_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let result: TyVal = match method {
            "Cancel" => {
                let (_, recv) = self.emit_operand(receiver);
                self.emit(&format!("call void @rt_cts_cancel(ptr {recv})"));
                ("void".into(), String::new())
            }
            "CancelAfter" => {
                let (_, recv) = self.emit_operand(receiver);
                let arg = args.first().cloned().unwrap_or(MirOperand::ConstInt(0));
                let (_, val) = self.emit_operand(&arg);
                self.emit(&format!(
                    "call void @rt_cts_cancel_after(ptr {recv}, i32 {val})"
                ));
                ("void".into(), String::new())
            }
            "get_Token" => {
                // CT 与 CTS 共享指针：直接返回 receiver（ptr）
                let (_, recv) = self.emit_operand(receiver);
                ("ptr".into(), recv)
            }
            "get_IsCancellationRequested" => {
                let (_, recv) = self.emit_operand(receiver);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_cts_is_canceled(ptr {recv})"));
                ("i32".into(), tmp)
            }
            "Dispose" => {
                let (_, recv) = self.emit_operand(receiver);
                self.emit(&format!("call void @rt_cts_destroy(ptr {recv})"));
                ("void".into(), String::new())
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 008 AsyncStream：TaskCompletionSource<T> 实例成员拦截。
    /// 对象即 PENDING 态 RtTask*：`get_Task` 共享返回 receiver（零拷贝，同
    /// CTS get_Token 模式）；`SetResult` 按 T 实际 LLVM 表示分派
    /// set_result_* 后 complete（Arc class 结果先 inc——task 持独立引用，
    /// 调用方出口 dec 不悬垂；string/array 无 ArcHeader 借引用不 inc，
    /// 对齐 FromResult 拦截）；`SetException` 先 inc 再 fault（发射侧计数
    /// 契约与 FromException 一致）；`SetCanceled` → cancel。
    pub(super) fn try_emit_tcs_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let result: TyVal = match method {
            "get_Task" => {
                // RFC 008：每次 get_Task 发射独立 follower task——leader（TCS
                // 根句柄）完成时级联传播结果。await 单消费语义不动（消费
                // follower 不影响 leader 与其他 follower），多次 get_Task /
                // 多次 await 各自独立（对标 .NET TCS 的 continuation 扇出）。
                let (_, recv) = self.emit_operand(receiver);
                let t = self.fresh_temp();
                self.emit(&format!("{t} = call ptr @rt_task_create_pending()"));
                self.emit(&format!(
                    "call void @rt_task_add_follower(ptr {recv}, ptr {t})"
                ));
                ("ptr".into(), t)
            }
            "SetResult" => {
                let (_, recv) = self.emit_operand(receiver);
                let arg = args.first().cloned().unwrap_or(MirOperand::ConstInt(0));
                let (arg_ty, arg_val) = self.emit_operand(&arg);
                // 窄整型（i1/i8/i16）不能直接作 i32 实参：按实参 TypeId 精确
                // 拓宽（有符号 Short/SByte sext，其余 zext；与 FromResult 拦截
                // 同则）。非 Local（常量）窄值按 zext 兜底。
                let (eff_ty, eff_val) = match arg_ty.as_str() {
                    "i1" | "i8" | "i16" => {
                        let sext = match &arg {
                            MirOperand::Local(id) => {
                                self.cfg.locals.get(id).is_some_and(|(_, ty)| {
                                    matches!(ty, TypeId::Short | TypeId::SByte)
                                })
                            }
                            _ => false,
                        };
                        let w = self.fresh_temp();
                        let op = if sext { "sext" } else { "zext" };
                        self.emit(&format!("{w} = {op} {arg_ty} {arg_val} to i32"));
                        ("i32".to_string(), w)
                    }
                    _ => (arg_ty.clone(), arg_val.clone()),
                };
                // Arc class 结果（Local 可解析 TypeId 时）先 inc。
                if eff_ty == "ptr" {
                    if let MirOperand::Local(id) = &arg {
                        let is_arc = self
                            .cfg
                            .locals
                            .get(id)
                            .is_some_and(|(_, ty)| Self::arc_class_place(ty, self.layouts));
                        if is_arc {
                            self.emit(&format!("call void @rt_arc_inc(ptr {eff_val})"));
                        }
                    }
                }
                self.emit_task_set_result_abi(&recv, &eff_ty, &eff_val);
                self.emit(&format!("call void @rt_task_complete(ptr {recv})"));
                ("void".into(), String::new())
            }
            "SetException" => {
                let (_, recv) = self.emit_operand(receiver);
                let arg = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                let (_, ex) = self.emit_operand(&arg);
                self.emit(&format!("call void @rt_arc_inc(ptr {ex})"));
                self.emit(&format!("call void @rt_task_fault(ptr {recv}, ptr {ex})"));
                ("void".into(), String::new())
            }
            "SetCanceled" => {
                let (_, recv) = self.emit_operand(receiver);
                // cancel 置 CANCELED 后补 complete：触发 waker + 级联扇出
                //（对齐 Task.Delay 取消路径 cancel+complete 成对模式，
                // 挂起中的 await 得以唤醒查询 IsCanceled）。
                self.emit(&format!("call void @rt_task_cancel(ptr {recv})"));
                self.emit(&format!("call void @rt_task_complete(ptr {recv})"));
                ("void".into(), String::new())
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 018 M2 step 3: 构造 RuntimeType 实例，_typeInfoHandle 填入指定 i64 表达式。
    ///
    /// 公共 helper，供 `try_emit_typeof_as_runtime_type`（handle = ptrtoint(@.typeinfo.{T})）
    /// 与 `get_BaseType`（handle = ptrtoint(parent RtTypeInfo*)）复用。
    ///
    /// `handle_expr` 是 i64 类型的 LLVM 常量表达式或寄存器值（如
    /// `ptrtoint (ptr @.typeinfo.Shape to i64)` 或 `%parent_as_i64`）。
    /// `handle_offset` 是 RuntimeType._typeInfoHandle 字段在对象内的字节偏移。
    pub(super) fn emit_new_runtime_typeinfo_with_handle_expr(
        &mut self,
        handle_expr: &str,
        handle_offset: u32,
    ) -> String {
        const RUNTIME_TYPE: &str = "RuntimeType";

        // 1. calloc 分配并零初始化（与 emit_new 通用路径一致）
        let size = self.class_size(RUNTIME_TYPE);
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = call ptr @calloc(i64 1, i64 {size})"));

        // 2. refcount = 1（offset 0）
        self.emit(&format!("store i32 1, ptr {tmp}"));

        // 3. vtable（offset 8，RuntimeType 继承 Type 有虚方法）
        if self.class_has_vtable(RUNTIME_TYPE) {
            let vtbl_addr = self.fresh_temp();
            self.emit(&format!(
                "{vtbl_addr} = getelementptr inbounds i8, ptr {tmp}, i64 8"
            ));
            // RFC 038 M2：RuntimeType 为 stdlib 外部类（LibraryObject），
            // vtable 经守卫登记 external 声明。
            if let Some(vt_sym) = self.vtable_global(RUNTIME_TYPE) {
                self.emit(&format!("store ptr {vt_sym}, ptr {vtbl_addr}"));
            }
        }

        // 4. _typeInfoHandle = handle_expr
        let handle_addr = self.fresh_temp();
        self.emit(&format!(
            "{handle_addr} = getelementptr inbounds i8, ptr {tmp}, i64 {handle_offset}"
        ));
        self.emit(&format!("store i64 {handle_expr}, ptr {handle_addr}"));

        tmp
    }

    /// RFC 018 M2 step 2/3: RuntimeType getter 拦截——从 _typeInfoHandle 字段
    /// （i64 句柄）还原 RtTypeInfo* 指针，直接 load 对应字段。
    ///
    /// 已实现的 getter：
    /// - `get_TypeId`：load i32 from offset 0（RtTypeInfo.type_id）
    /// - `get_Name`：load ptr from offset 16（RtTypeInfo.name，C string 直接作为 Arc string）
    /// - `get_FullName`：load ptr from offset 24（RtTypeInfo.full_name）
    /// - `get_Kind`：load i32 from offset 40（RtTypeInfo.kind，TypeKind 枚举底层 i32）
    /// - `get_BaseType`：load ptr from offset 8（RtTypeInfo.parent），null 返回 null，
    ///   否则构造新 RuntimeType 实例（_typeInfoHandle = ptrtoint(parent)）
    ///
    /// **Arc string 与 C string 同构**：Arc `string` 类型在 codegen 中即 `ptr`（指向
    /// null-terminated C string），与 RtTypeInfo.name/full_name/ns 字段（`const char*`）
    /// 二进制兼容，无需转换。
    ///
    /// **RtTypeInfo 字段布局**（rt_abi.h，offset 由 emit_typeinfos 发射）：
    /// ```c
    /// typedef struct RtTypeInfo {
    ///     int32_t type_id;          // offset 0
    ///     void* parent;             // offset 8
    ///     char* name;               // offset 16
    ///     char* full_name;          // offset 24
    ///     char* namespace;          // offset 32
    ///     int32_t kind;             // offset 40
    ///     int32_t flags;            // offset 44
    ///     ...
    /// } RtTypeInfo;
    /// ```
    pub(super) fn try_emit_runtime_typeinfo_getter(
        &mut self,
        receiver: &MirOperand,
        method: &str,
    ) -> Option<TyVal> {
        const RUNTIME_TYPE: &str = "RuntimeType";
        let rt_layout = self.layouts.classes.get(RUNTIME_TYPE)?;
        let handle_offset = rt_layout
            .fields
            .iter()
            .find(|f| f.name.as_str() == "_typeInfoHandle")?
            .offset;

        // 公共：从 receiver 还原 RtTypeInfo* 指针
        // receiver 是 RuntimeType 实例（ptr），_typeInfoHandle 在 offset {handle_offset}
        let (_, recv) = self.emit_operand(receiver);
        let handle_addr = self.fresh_temp();
        self.emit(&format!(
            "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i64 {handle_offset}"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load i64, ptr {handle_addr}"));
        let typeinfo_ptr = self.fresh_temp();
        self.emit(&format!("{typeinfo_ptr} = inttoptr i64 {handle} to ptr"));

        match method {
            "get_TypeId" => {
                // RtTypeInfo.type_id @ offset 0（i32）
                let type_id = self.fresh_temp();
                self.emit(&format!("{type_id} = load i32, ptr {typeinfo_ptr}"));
                Some(("i32".into(), type_id))
            }
            "get_Name" => {
                // RtTypeInfo.name @ offset 16（ptr，C string 直接作为 Arc string）
                let name_addr = self.fresh_temp();
                self.emit(&format!(
                    "{name_addr} = getelementptr inbounds i8, ptr {typeinfo_ptr}, i64 16"
                ));
                let name = self.fresh_temp();
                self.emit(&format!("{name} = load ptr, ptr {name_addr}"));
                Some(("ptr".into(), name))
            }
            "get_FullName" => {
                // RtTypeInfo.full_name @ offset 24（ptr）
                let fn_addr = self.fresh_temp();
                self.emit(&format!(
                    "{fn_addr} = getelementptr inbounds i8, ptr {typeinfo_ptr}, i64 24"
                ));
                let full_name = self.fresh_temp();
                self.emit(&format!("{full_name} = load ptr, ptr {fn_addr}"));
                Some(("ptr".into(), full_name))
            }
            "get_Kind" => {
                // RtTypeInfo.kind @ offset 40（i32，TypeKind 枚举底层）
                let kind_addr = self.fresh_temp();
                self.emit(&format!(
                    "{kind_addr} = getelementptr inbounds i8, ptr {typeinfo_ptr}, i64 40"
                ));
                let kind = self.fresh_temp();
                self.emit(&format!("{kind} = load i32, ptr {kind_addr}"));
                Some(("i32".into(), kind))
            }
            "get_BaseType" => {
                // RtTypeInfo.parent @ offset 8（ptr，可能为 null）
                let parent_addr = self.fresh_temp();
                self.emit(&format!(
                    "{parent_addr} = getelementptr inbounds i8, ptr {typeinfo_ptr}, i64 8"
                ));
                let parent = self.fresh_temp();
                self.emit(&format!("{parent} = load ptr, ptr {parent_addr}"));

                // null 检查：parent == null → 返回 null（ptr null）
                // 否则构造新 RuntimeType，_typeInfoHandle = ptrtoint(parent to i64)
                let result = self.fresh_temp();
                self.emit(&format!("{result} = alloca ptr"));
                self.emit(&format!("store ptr null, ptr {result}"));

                let is_null = self.fresh_temp();
                self.emit(&format!("{is_null} = icmp eq ptr {parent}, null"));

                let non_null_bb = self.fresh_label();
                let end_bb = self.fresh_label();
                self.emit(&format!(
                    "br i1 {is_null}, label %{end_bb}, label %{non_null_bb}"
                ));

                // non_null BB: 构造新 RuntimeType，handle = ptrtoint(parent to i64)
                self.emit(&format!("{non_null_bb}:"));
                let parent_as_i64 = self.fresh_temp();
                self.emit(&format!("{parent_as_i64} = ptrtoint ptr {parent} to i64"));
                let new_rt =
                    self.emit_new_runtime_typeinfo_with_handle_expr(&parent_as_i64, handle_offset);
                self.emit(&format!("store ptr {new_rt}, ptr {result}"));
                self.emit(&format!("br label %{end_bb}"));

                // end BB: load result
                self.emit(&format!("{end_bb}:"));
                let final_val = self.fresh_temp();
                self.emit(&format!("{final_val} = load ptr, ptr {result}"));
                Some(("ptr".into(), final_val))
            }
            // RFC 007 M3+：枚举 declared_methods / declared_fields / declared_properties → List<*Info>
            // RFC 007 J-C：`GetMethods` 含继承（走 parent 链），`get_DeclaredMethods` 仅本类型。
            "GetMethods" => {
                Some(self.emit_runtime_type_member_list(
                    &typeinfo_ptr,
                    /* ptr_off */ 48,
                    /* count_off */ 56,
                    /* stride */ 56,
                    "RuntimeMethodInfo",
                    "_methodInfoHandle",
                    "List_MethodInfo",
                    /* include_inherited */ true,
                ))
            }
            "get_DeclaredMethods" => {
                Some(self.emit_runtime_type_member_list(
                    &typeinfo_ptr,
                    /* ptr_off */ 48,
                    /* count_off */ 56,
                    /* stride */ 56,
                    "RuntimeMethodInfo",
                    "_methodInfoHandle",
                    "List_MethodInfo",
                    /* include_inherited */ false,
                ))
            }
            "GetFields" => {
                Some(self.emit_runtime_type_member_list(
                    &typeinfo_ptr,
                    /* ptr_off */ 64,
                    /* count_off */ 72,
                    /* stride */ 48,
                    "RuntimeFieldInfo",
                    "_fieldInfoHandle",
                    "List_FieldInfo",
                    /* include_inherited */ true,
                ))
            }
            "get_DeclaredFields" => {
                Some(self.emit_runtime_type_member_list(
                    &typeinfo_ptr,
                    /* ptr_off */ 64,
                    /* count_off */ 72,
                    /* stride */ 48,
                    "RuntimeFieldInfo",
                    "_fieldInfoHandle",
                    "List_FieldInfo",
                    /* include_inherited */ false,
                ))
            }
            "GetProperties" => {
                Some(self.emit_runtime_type_member_list(
                    &typeinfo_ptr,
                    /* ptr_off */ 80,
                    /* count_off */ 88,
                    /* stride */ 72,
                    "RuntimePropertyInfo",
                    "_propertyInfoHandle",
                    "List_PropertyInfo",
                    /* include_inherited */ true,
                ))
            }
            "get_DeclaredProperties" => {
                Some(self.emit_runtime_type_member_list(
                    &typeinfo_ptr,
                    /* ptr_off */ 80,
                    /* count_off */ 88,
                    /* stride */ 72,
                    "RuntimePropertyInfo",
                    "_propertyInfoHandle",
                    "List_PropertyInfo",
                    /* include_inherited */ false,
                ))
            }
            _ => None,
        }
    }

    /// RFC 007 M3+：从 RtTypeInfo.declared_* 数组构造 `List<MethodInfo|FieldInfo|PropertyInfo>`。
    ///
    /// `ptr_off`/`count_off` 为 RtTypeInfo 内字段字节偏移；`stride` 为 RtMethodInfo
    /// （56）、RtFieldInfo（48）或 RtPropertyInfo（72）的 LLVM 结构体大小。每个元素包装为
    /// RuntimeMethodInfo / RuntimeFieldInfo / RuntimePropertyInfo（handle = ptrtoint(entry)）。
    ///
    /// RFC 007 J-C：`include_inherited` 为 true 时沿 parent 链（RtTypeInfo.parent @ offset 8）
    /// 逐级收集 declared_* 合并（GetXxx 语义）；false 时仅本类型（DeclaredXxx 语义）。
    fn emit_runtime_type_member_list(
        &mut self,
        typeinfo_ptr: &str,
        ptr_off: u32,
        count_off: u32,
        stride: u64,
        info_class: &str,
        handle_field: &str,
        list_class: &str,
        include_inherited: bool,
    ) -> TyVal {
        // new List_T()
        let eq_fn = "ptr null";
        let arc_inc = "ptr @rt_list_arc_inc_ref";
        let arc_dec = "ptr @rt_list_arc_dec_ref";
        let handle = self.fresh_temp();
        self.emit(&format!(
            "{handle} = call ptr @rt_list_create(i32 8, {eq_fn}, {arc_inc}, {arc_dec})"
        ));
        let list_size = self.class_size(list_class);
        let list_obj = self.fresh_temp();
        self.emit(&format!(
            "{list_obj} = call ptr @calloc(i64 1, i64 {list_size})"
        ));
        self.emit(&format!("store i32 1, ptr {list_obj}"));
        if let Some(vt) = self.vtable_global(list_class) {
            let vtbl_addr = self.fresh_temp();
            self.emit(&format!(
                "{vtbl_addr} = getelementptr inbounds i8, ptr {list_obj}, i64 8"
            ));
            self.emit(&format!("store ptr {vt}, ptr {vtbl_addr}"));
        }
        let hp = self.fresh_temp();
        self.emit(&format!(
            "{hp} = getelementptr inbounds i8, ptr {list_obj}, i32 16"
        ));
        self.emit(&format!("store ptr {handle}, ptr {hp}"));

        let info_layout = match self.layouts.classes.get(info_class) {
            Some(l) => l,
            None => return ("ptr".into(), list_obj),
        };
        let info_handle_off = match info_layout
            .fields
            .iter()
            .find(|f| f.name.as_str() == handle_field)
        {
            Some(f) => f.offset,
            None => return ("ptr".into(), list_obj),
        };
        let info_size = self.class_size(info_class);

        // 共享 slot alloca（entry 块提升，避免循环内泄漏栈槽）+ 当前 typeinfo 指针槽。
        let slot = self.fresh_temp();
        self.entry_allocas
            .push_str(&format!("  {slot} = alloca ptr\n"));
        let cur_slot = self.fresh_temp();
        self.entry_allocas
            .push_str(&format!("  {cur_slot} = alloca ptr\n"));
        self.emit(&format!("store ptr {typeinfo_ptr}, ptr {cur_slot}"));

        // 外层循环：沿 parent 链（include_inherited）或单次（仅本类型）。
        let outer_loop_bb = self.fresh_label();
        let outer_body_bb = self.fresh_label();
        let outer_end_bb = self.fresh_label();
        self.emit(&format!("br label %{outer_loop_bb}"));

        self.emit(&format!("{outer_loop_bb}:"));
        let cur = self.fresh_temp();
        self.emit(&format!("{cur} = load ptr, ptr {cur_slot}"));
        let cur_null = self.fresh_temp();
        self.emit(&format!("{cur_null} = icmp eq ptr {cur}, null"));
        self.emit(&format!(
            "br i1 {cur_null}, label %{outer_end_bb}, label %{outer_body_bb}"
        ));

        self.emit(&format!("{outer_body_bb}:"));
        let arr_addr = self.fresh_temp();
        self.emit(&format!(
            "{arr_addr} = getelementptr inbounds i8, ptr {cur}, i64 {ptr_off}"
        ));
        let arr_ptr = self.fresh_temp();
        self.emit(&format!("{arr_ptr} = load ptr, ptr {arr_addr}"));
        let count_addr = self.fresh_temp();
        self.emit(&format!(
            "{count_addr} = getelementptr inbounds i8, ptr {cur}, i64 {count_off}"
        ));
        let count = self.fresh_temp();
        self.emit(&format!("{count} = load i32, ptr {count_addr}"));

        // 内层循环：for i in 0..count { push Runtime*Info(ptrtoint(&arr[i])) }
        let idx_slot = self.fresh_temp();
        self.entry_allocas
            .push_str(&format!("  {idx_slot} = alloca i32\n"));
        self.emit(&format!("store i32 0, ptr {idx_slot}"));
        let loop_bb = self.fresh_label();
        let body_bb = self.fresh_label();
        let end_bb = self.fresh_label();
        self.emit(&format!("br label %{loop_bb}"));

        self.emit(&format!("{loop_bb}:"));
        let idx = self.fresh_temp();
        self.emit(&format!("{idx} = load i32, ptr {idx_slot}"));
        let cmp = self.fresh_temp();
        self.emit(&format!("{cmp} = icmp slt i32 {idx}, {count}"));
        self.emit(&format!("br i1 {cmp}, label %{body_bb}, label %{end_bb}"));

        self.emit(&format!("{body_bb}:"));
        let idx64 = self.fresh_temp();
        self.emit(&format!("{idx64} = zext i32 {idx} to i64"));
        let byte_off = self.fresh_temp();
        self.emit(&format!("{byte_off} = mul i64 {idx64}, {stride}"));
        let entry_ptr = self.fresh_temp();
        self.emit(&format!(
            "{entry_ptr} = getelementptr inbounds i8, ptr {arr_ptr}, i64 {byte_off}"
        ));
        let entry_as_i64 = self.fresh_temp();
        self.emit(&format!("{entry_as_i64} = ptrtoint ptr {entry_ptr} to i64"));

        // calloc Runtime*Info
        let info_obj = self.fresh_temp();
        self.emit(&format!(
            "{info_obj} = call ptr @calloc(i64 1, i64 {info_size})"
        ));
        self.emit(&format!("store i32 1, ptr {info_obj}"));
        // RFC 038 M2：反射 Info 类（stdlib 外部）vtable 经守卫登记 external 声明。
        if let Some(vt_sym) = self.vtable_global(info_class) {
            let vtbl_addr = self.fresh_temp();
            self.emit(&format!(
                "{vtbl_addr} = getelementptr inbounds i8, ptr {info_obj}, i64 8"
            ));
            self.emit(&format!("store ptr {vt_sym}, ptr {vtbl_addr}"));
        }
        let ih = self.fresh_temp();
        self.emit(&format!(
            "{ih} = getelementptr inbounds i8, ptr {info_obj}, i64 {info_handle_off}"
        ));
        self.emit(&format!("store i64 {entry_as_i64}, ptr {ih}"));

        // rt_list_push(handle, &info_obj_slot) — slot alloca 已提升到 entry 块。
        self.emit(&format!("store ptr {info_obj}, ptr {slot}"));
        self.emit(&format!(
            "call void @rt_list_push(ptr {handle}, ptr {slot})"
        ));

        let idx_next = self.fresh_temp();
        self.emit(&format!("{idx_next} = add i32 {idx}, 1"));
        self.emit(&format!("store i32 {idx_next}, ptr {idx_slot}"));
        self.emit(&format!("br label %{loop_bb}"));

        self.emit(&format!("{end_bb}:"));

        // 推进到 parent（include_inherited）或终止（仅本类型）。
        if include_inherited {
            let parent_addr = self.fresh_temp();
            self.emit(&format!(
                "{parent_addr} = getelementptr inbounds i8, ptr {cur}, i64 8"
            ));
            let parent = self.fresh_temp();
            self.emit(&format!("{parent} = load ptr, ptr {parent_addr}"));
            self.emit(&format!("store ptr {parent}, ptr {cur_slot}"));
        } else {
            self.emit(&format!("store ptr null, ptr {cur_slot}"));
        }
        self.emit(&format!("br label %{outer_loop_bb}"));

        self.emit(&format!("{outer_end_bb}:"));
        ("ptr".into(), list_obj)
    }

    /// RFC 018 M3+：RuntimeMethodInfo.Name ← RtMethodInfo.name @ offset 0。
    /// RFC 018 J-C：RuntimeMethodInfo.ReturnType ← RtMethodInfo.return_type @ offset 16。
    pub(super) fn try_emit_runtime_methodinfo_getter(
        &mut self,
        receiver: &MirOperand,
        method: &str,
    ) -> Option<TyVal> {
        const CLASS: &str = "RuntimeMethodInfo";
        let layout = self.layouts.classes.get(CLASS)?;
        let handle_offset = layout
            .fields
            .iter()
            .find(|f| f.name.as_str() == "_methodInfoHandle")?
            .offset;
        let (_, recv) = self.emit_operand(receiver);
        let handle_addr = self.fresh_temp();
        self.emit(&format!(
            "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i64 {handle_offset}"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load i64, ptr {handle_addr}"));
        let info_ptr = self.fresh_temp();
        self.emit(&format!("{info_ptr} = inttoptr i64 {handle} to ptr"));
        match method {
            // RtMethodInfo.name @ offset 0
            "get_Name" => {
                let name = self.fresh_temp();
                self.emit(&format!("{name} = load ptr, ptr {info_ptr}"));
                Some(("ptr".into(), name))
            }
            // RtMethodInfo.return_type @ offset 16
            "get_ReturnType" => Some(self.emit_member_info_type(&info_ptr, 16)),
            _ => None,
        }
    }

    /// RFC 018 M3+：RuntimeFieldInfo.Name ← RtFieldInfo.name @ offset 0。
    /// RFC 018 J-C：RuntimeFieldInfo.FieldType ← RtFieldInfo.field_type @ offset 16。
    pub(super) fn try_emit_runtime_fieldinfo_getter(
        &mut self,
        receiver: &MirOperand,
        method: &str,
    ) -> Option<TyVal> {
        const CLASS: &str = "RuntimeFieldInfo";
        let layout = self.layouts.classes.get(CLASS)?;
        let handle_offset = layout
            .fields
            .iter()
            .find(|f| f.name.as_str() == "_fieldInfoHandle")?
            .offset;
        let (_, recv) = self.emit_operand(receiver);
        let handle_addr = self.fresh_temp();
        self.emit(&format!(
            "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i64 {handle_offset}"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load i64, ptr {handle_addr}"));
        let info_ptr = self.fresh_temp();
        self.emit(&format!("{info_ptr} = inttoptr i64 {handle} to ptr"));
        match method {
            "get_Name" => {
                let name = self.fresh_temp();
                self.emit(&format!("{name} = load ptr, ptr {info_ptr}"));
                Some(("ptr".into(), name))
            }
            // RtFieldInfo.field_type @ offset 16
            "get_FieldType" => Some(self.emit_member_info_type(&info_ptr, 16)),
            _ => None,
        }
    }

    /// RFC 018 M3+：RuntimePropertyInfo.Name ← RtPropertyInfo.name @ offset 0。
    /// RFC 018 J-C：RuntimePropertyInfo.PropertyType ← RtPropertyInfo.property_type @ offset 16。
    pub(super) fn try_emit_runtime_propertyinfo_getter(
        &mut self,
        receiver: &MirOperand,
        method: &str,
    ) -> Option<TyVal> {
        const CLASS: &str = "RuntimePropertyInfo";
        let layout = self.layouts.classes.get(CLASS)?;
        let handle_offset = layout
            .fields
            .iter()
            .find(|f| f.name.as_str() == "_propertyInfoHandle")?
            .offset;
        let (_, recv) = self.emit_operand(receiver);
        let handle_addr = self.fresh_temp();
        self.emit(&format!(
            "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i64 {handle_offset}"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load i64, ptr {handle_addr}"));
        let info_ptr = self.fresh_temp();
        self.emit(&format!("{info_ptr} = inttoptr i64 {handle} to ptr"));
        match method {
            "get_Name" => {
                let name = self.fresh_temp();
                self.emit(&format!("{name} = load ptr, ptr {info_ptr}"));
                Some(("ptr".into(), name))
            }
            // RtPropertyInfo.property_type @ offset 16
            "get_PropertyType" => Some(self.emit_member_info_type(&info_ptr, 16)),
            _ => None,
        }
    }

    /// RFC 018 J-C：从 Rt*Info 结构读类型指针（field_type/return_type/property_type
    /// 均 @ offset 16），构造 RuntimeType（handle = ptrtoint(type ptr)）；null → null。
    fn emit_member_info_type(&mut self, info_ptr: &str, type_off: u32) -> TyVal {
        const RUNTIME_TYPE: &str = "RuntimeType";
        let rt_layout = self.layouts.classes.get(RUNTIME_TYPE).unwrap();
        let handle_offset = rt_layout
            .fields
            .iter()
            .find(|f| f.name.as_str() == "_typeInfoHandle")
            .unwrap()
            .offset;
        let type_addr = self.fresh_temp();
        self.emit(&format!(
            "{type_addr} = getelementptr inbounds i8, ptr {info_ptr}, i64 {type_off}"
        ));
        let type_ptr = self.fresh_temp();
        self.emit(&format!("{type_ptr} = load ptr, ptr {type_addr}"));
        let result = self.fresh_temp();
        self.emit(&format!("{result} = alloca ptr"));
        self.emit(&format!("store ptr null, ptr {result}"));
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {type_ptr}, null"));
        let non_null_bb = self.fresh_label();
        let end_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {is_null}, label %{end_bb}, label %{non_null_bb}"
        ));
        self.emit(&format!("{non_null_bb}:"));
        let type_as_i64 = self.fresh_temp();
        self.emit(&format!("{type_as_i64} = ptrtoint ptr {type_ptr} to i64"));
        let new_rt = self.emit_new_runtime_typeinfo_with_handle_expr(&type_as_i64, handle_offset);
        self.emit(&format!("store ptr {new_rt}, ptr {result}"));
        self.emit(&format!("br label %{end_bb}"));
        self.emit(&format!("{end_bb}:"));
        let final_val = self.fresh_temp();
        self.emit(&format!("{final_val} = load ptr, ptr {result}"));
        ("ptr".into(), final_val)
    }

    /// CancellationToken facade (RFC 009 M4): CT 实例方法拦截。
    ///
    /// 覆盖：ThrowIfCancellationRequested/Register/IsCancellationRequested。
    /// ThrowIfCancellationRequested 反糖为 C# 等价语义
    /// `if (IsCancellationRequested) throw new OperationCanceledException()`——
    /// 异常经 rt_throw 进入统一异常通道（async 状态机 resume 续体 / SEH 边界
    /// 捕获 → rt_task_fault → await 方 catch），取代原 rt_panic 直崩（旁路异常
    /// 通道，取消异常不可捕获）。
    pub(super) fn try_emit_ct_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let result: TyVal = match method {
            "ThrowIfCancellationRequested" => {
                let (_, recv) = self.emit_operand(receiver);
                let canceled = self.fresh_temp();
                self.emit(&format!(
                    "{canceled} = call i32 @rt_cts_is_canceled(ptr {recv})"
                ));
                let is_canceled = self.fresh_temp();
                self.emit(&format!("{is_canceled} = icmp ne i32 {canceled}, 0"));
                let throw_label = self.fresh_label();
                let cont_label = self.fresh_label();
                self.emit(&format!(
                    "br i1 {is_canceled}, label %{throw_label}, label %{cont_label}"
                ));
                self.emit_label(&throw_label);
                let (_, exc) = self.emit_new("OperationCanceledException", &[], &[]);
                self.emit_attach_exception_stacktrace(&exc);
                self.emit_call_may_throw("void", "@rt_throw", &format!("ptr {exc}"), true, None);
                self.emit("unreachable");
                self.emit_label(&cont_label);
                ("void".into(), String::new())
            }
            "Register" => {
                // args[0] 是 Action（arc_closure 或 FnPtr）。M4 简化：直接传 closure 指针，
                // rt_cts_register 存储 fn=rt_cts_callback_trampoline, data=closure_ptr。
                // ct 取消时 trampoline(closure) → closure->fn_ptr(closure->env)。
                // FnPtr（无捕获 lambda）需合成临时 arc_closure（env=null），否则
                // trampoline 把函数指针当作 arc_closure* 解引用会触发访问违例。
                let (_, recv) = self.emit_operand(receiver);
                let arg = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                let (_, arg_val) = self.emit_operand_as_closure(&arg);
                self.emit(&format!(
                    "call void @rt_cts_register(ptr {recv}, ptr @rt_cts_callback_trampoline, ptr {arg_val})"
                ));
                ("void".into(), String::new())
            }
            "get_IsCancellationRequested" => {
                let (_, recv) = self.emit_operand(receiver);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_cts_is_canceled(ptr {recv})"));
                ("i32".into(), tmp)
            }
            "get_CanBeCanceled" => {
                // .NET 语义：null（None 令牌）恒不可取消；真实 CTS 返回可取消性。
                let (_, recv) = self.emit_operand(receiver);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_cts_can_be_canceled(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 048: 命名管道门面（本机 IPC · rt_pipe_* 同步面）。
    /// WaitForConnection/Connect/Read/Write/Disconnect/get_IsConnected/Dispose
    /// → @rt_pipe_* ABI。receiver 是 RtPipe* 指针（emit_new 返回）。
    /// byte[] 实参即 RtArray 载荷指针（同 socket SendBytes 形态），offset 经
    /// getelementptr 显式进位，count 直传——内部 0x00 完整往返，无 NUL 截断。
    /// Write/Disconnect 发射后丢弃 i32 结果（facade 面为 void）。
    pub(super) fn try_emit_pipe_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let result: TyVal = match method {
            "WaitForConnection" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_pipe_server_wait_connect(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            "Connect" => {
                let (_, timeout) = self
                    .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(-1)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_pipe_client_connect(ptr {recv}, i32 {timeout})"
                ));
                ("i32".into(), tmp)
            }
            "Read" => {
                let (_, buffer) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, offset) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, count) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let base = self.fresh_temp();
                self.emit(&format!(
                    "{base} = getelementptr inbounds i8, ptr {buffer}, i32 {offset}"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_pipe_read(ptr {recv}, ptr {base}, i32 {count})"
                ));
                ("i32".into(), tmp)
            }
            "Write" => {
                let (_, buffer) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, offset) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, count) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let base = self.fresh_temp();
                self.emit(&format!(
                    "{base} = getelementptr inbounds i8, ptr {buffer}, i32 {offset}"
                ));
                self.emit(&format!(
                    "call i32 @rt_pipe_write(ptr {recv}, ptr {base}, i32 {count})"
                ));
                ("void".into(), String::new())
            }
            "Disconnect" => {
                self.emit(&format!("call i32 @rt_pipe_server_disconnect(ptr {recv})"));
                ("void".into(), String::new())
            }
            "Terminate" | "Dispose" => {
                self.emit(&format!("call void @rt_pipe_close(ptr {recv})"));
                ("void".into(), String::new())
            }
            "get_IsConnected" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_pipe_is_connected(ptr {recv})"));
                ("i32".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// Thread facade (RFC 009 M5.5): Thread 实例方法拦截。
    /// Start/Join/IsAlive → @rt_thread_handle_* ABI。
    /// Socket/TcpClient/TcpListener/UdpClient facade (RFC 025 M4): 实例方法拦截。
    /// Connect/Bind/Listen/Accept/Send/Receive/Close/Available/SetTimeout →
    /// @rt_socket_* ABI。receiver 是 RtSocket* 指针（emit_new 返回）。
    /// receiver_type 用于区分 UdpClient 数据报级 byte[] 面（RFC 025 §1.2.g ·
    /// 2026-08-05）与 TCP 流 string 面。
    pub(super) fn try_emit_socket_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let result: TyVal = match method {
            "Connect" => {
                let (_, host) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, port) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_socket_connect(ptr {recv}, ptr {host}, i32 {port})"
                ));
                ("i32".into(), tmp)
            }
            "Bind" | "Start" | "JoinMulticastGroup" => {
                let (_, port) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_socket_bind(ptr {recv}, i32 {port})"
                ));
                // Start() also needs to call listen after bind.
                // Use backlog from second arg if present; default to 5.
                if method == "Start" {
                    let (_, backlog) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(5)));
                    let _listen = self.fresh_temp();
                    self.emit(&format!(
                        "{_listen} = call i32 @rt_socket_listen(ptr {recv}, i32 {backlog})"
                    ));
                }
                ("i32".into(), tmp)
            }
            "Pending" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_socket_poll(ptr {recv}, i32 0, i32 0)"
                ));
                ("i32".into(), tmp)
            }
            "Listen" => {
                let (_, backlog) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(5)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_socket_listen(ptr {recv}, i32 {backlog})"
                ));
                ("i32".into(), tmp)
            }
            "Accept" | "AcceptTcpClient" | "AcceptSocket" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_socket_accept(ptr {recv})"));
                ("ptr".into(), tmp)
            }
            "Send" => {
                // UdpClient（RFC 025 §1.2.g 数据报级升级）：byte[] 数据报 sendto——
                // 显式长度（offset/count），内部 0x00 完整往返，不 connect。
                if receiver_type == "UdpClient" {
                    let (_, data) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let (_, offset) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let (_, count) =
                        self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let (_, host) = self.emit_operand(
                        &args
                            .get(3)
                            .cloned()
                            .unwrap_or(MirOperand::ConstString(String::new())),
                    );
                    let (_, port) =
                        self.emit_operand(&args.get(4).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let base = self.fresh_temp();
                    self.emit(&format!(
                        "{base} = getelementptr inbounds i8, ptr {data}, i32 {offset}"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_socket_sendto_bytes(ptr {recv}, ptr {base}, i32 {count}, ptr {host}, i32 {port})"
                    ));
                    ("i32".into(), tmp)
                } else {
                    // TCP-style Send(data) or UDP-style Send(data, host, port).
                    // For TCP: rt_str_length(data) + rt_socket_send(recv, data, len).
                    // For UDP: connect to target first, then send.
                    let (_, data) = self.emit_operand(
                        &args
                            .first()
                            .cloned()
                            .unwrap_or(MirOperand::ConstString(String::new())),
                    );
                    let len_tmp = self.fresh_temp();
                    self.emit(&format!("{len_tmp} = call i32 @rt_str_length(ptr {data})"));
                    if args.len() >= 3 {
                        let (_, host) = self.emit_operand(
                            &args
                                .get(1)
                                .cloned()
                                .unwrap_or(MirOperand::ConstString(String::new())),
                        );
                        let (_, port) = self
                            .emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                        let _conn = self.fresh_temp();
                        self.emit(&format!("{_conn} = call i32 @rt_socket_connect(ptr {recv}, ptr {host}, i32 {port})"));
                    }
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_socket_send(ptr {recv}, ptr {data}, i32 {len_tmp})"
                    ));
                    ("i32".into(), tmp)
                }
            }
            "Receive" => {
                // UdpClient（RFC 025 §1.2.g 数据报级升级）：byte[] 数据报 recvfrom——
                // 写入调用方 buffer（offset/count），显式长度，内部 0x00 完整往返。
                if receiver_type == "UdpClient" {
                    let (_, buffer) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let (_, offset) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let (_, count) =
                        self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let base = self.fresh_temp();
                    self.emit(&format!(
                        "{base} = getelementptr inbounds i8, ptr {buffer}, i32 {offset}"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_socket_recvfrom_bytes(ptr {recv}, ptr {base}, i32 {count})"
                    ));
                    ("i32".into(), tmp)
                } else {
                    let (_, buf_size) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(4096)));
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_socket_receive(ptr {recv}, i32 {buf_size})"
                    ));
                    ("ptr".into(), tmp)
                }
            }
            // S2 (RFC 025 §2.4): 原始字节面——显式长度，无 NUL 截断（HTTP/2 帧/SETTINGS
            // 载荷必含 0x00，string 面不可用）。`byte[]` 实参即 RtArray 载荷指针，
            // rt_socket_send 按显式 length 发送，内部 0x00 不会被 strlen 截断。
            "SendBytes" => {
                let (_, data) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, offset) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, count) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let base = self.fresh_temp();
                self.emit(&format!(
                    "{base} = getelementptr inbounds i8, ptr {data}, i32 {offset}"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_socket_send(ptr {recv}, ptr {base}, i32 {count})"
                ));
                ("i32".into(), tmp)
            }
            // S2 (RFC 032 §2.4): 原始字节接收面——写入调用方 buffer，返回实际读入字节数
            //（EOF/超时 0）。底层 rt_net_recv 按 fd 直收，无 NUL 终止。RtSocket 首字段即
            // 平台 fd：Win64 SOCKET 为 i64（截断 i32 后经 rt_fd_to_socket 还原），POSIX 为 i32。
            "ReceiveBytes" => {
                let (_, buffer) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, offset) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, count) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let fd_raw = self.fresh_temp();
                if self.is_windows {
                    let fd64 = self.fresh_temp();
                    self.emit(&format!("{fd64} = load i64, ptr {recv}"));
                    self.emit(&format!("{fd_raw} = trunc i64 {fd64} to i32"));
                } else {
                    self.emit(&format!("{fd_raw} = load i32, ptr {recv}"));
                }
                let base = self.fresh_temp();
                self.emit(&format!(
                    "{base} = getelementptr inbounds i8, ptr {buffer}, i32 {offset}"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_net_recv(i32 {fd_raw}, ptr {base}, i32 {count})"
                ));
                ("i32".into(), tmp)
            }
            "Close" | "Dispose" | "Stop" => {
                self.emit(&format!("call void @rt_socket_close(ptr {recv})"));
                ("void".into(), String::new())
            }
            "get_Available" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_socket_available(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            "get_Connected" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_socket_connected(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            "Shutdown" => {
                let (_, how) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_socket_shutdown(ptr {recv}, i32 {how})"
                ));
                ("void".into(), String::new())
            }
            "Poll" => {
                let (_, us) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(-1)));
                let (_, mode) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_socket_poll(ptr {recv}, i32 {us}, i32 {mode})"
                ));
                ("i32".into(), tmp)
            }
            "SetNoDelay" => {
                let (_, val) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_socket_set_no_delay(ptr {recv}, i32 {val})"
                ));
                ("void".into(), String::new())
            }
            "SetSendBufferSize" => {
                let (_, size) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(8192)));
                self.emit(&format!(
                    "call void @rt_socket_set_send_buf_size(ptr {recv}, i32 {size})"
                ));
                ("void".into(), String::new())
            }
            "SetReceiveBufferSize" => {
                let (_, size) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(8192)));
                self.emit(&format!(
                    "call void @rt_socket_set_recv_buf_size(ptr {recv}, i32 {size})"
                ));
                ("void".into(), String::new())
            }
            "SetReceiveTimeout" => {
                let (_, ms) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_socket_set_recv_timeout(ptr {recv}, i32 {ms})"
                ));
                ("void".into(), String::new())
            }
            "SetSendTimeout" => {
                let (_, ms) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_socket_set_send_timeout(ptr {recv}, i32 {ms})"
                ));
                ("void".into(), String::new())
            }
            // ── RFC 009 M2: 异步网络 IO ──
            // 调用 rt_socket_*_async 创建 Pending Task + 提交到 Reactor。
            // 返回 ptr（Task*），await 挂起；Reactor 完成后 EventLoop tick
            // 通过 rt_io_completion_complete 把结果写回 Task 并触发 waker。
            "ConnectAsync" => {
                let (_, host) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, port) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_socket_connect_async(ptr {recv}, ptr {host}, i32 {port})"
                ));
                ("ptr".into(), tmp)
            }
            "AcceptAsync" | "AcceptTcpClientAsync" | "AcceptSocketAsync" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_socket_accept_async(ptr {recv})"
                ));
                ("ptr".into(), tmp)
            }
            "SendAsync" => {
                let (_, data) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                // rt_str_length 获取字节数（不含 NUL）
                let len_tmp = self.fresh_temp();
                self.emit(&format!("{len_tmp} = call i32 @rt_str_length(ptr {data})"));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_socket_send_async(ptr {recv}, ptr {data}, i32 {len_tmp})"
                ));
                ("ptr".into(), tmp)
            }
            "ReceiveAsync" => {
                let (_, buf_size) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(4096)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_socket_receive_async(ptr {recv}, i32 {buf_size})"
                ));
                ("ptr".into(), tmp)
            }
            // S2 字节面异步（RFC 009 异步为主 · WebSocket wss TLS 密文含 0x00）：
            // byte[] 实参即 RtArray 载荷指针（+ offset），rt_socket_send_async 本身
            // 字节无关（void*+length），按显式 count 发送不 NUL 截断；接收侧写入
            // 调用方 buffer 并返回字节数（int_result）。
            "SendBytesAsync" => {
                let (_, data) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, offset) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, count) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let base = self.fresh_temp();
                self.emit(&format!(
                    "{base} = getelementptr inbounds i8, ptr {data}, i32 {offset}"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_socket_send_async(ptr {recv}, ptr {base}, i32 {count})"
                ));
                ("ptr".into(), tmp)
            }
            "ReceiveBytesAsync" => {
                let (_, buffer) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, offset) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, count) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let base = self.fresh_temp();
                self.emit(&format!(
                    "{base} = getelementptr inbounds i8, ptr {buffer}, i32 {offset}"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_socket_receive_bytes_async(ptr {recv}, ptr {base}, i32 {count})"));
                ("ptr".into(), tmp)
            }
            "ReadAsync" | "WriteAsync" => {
                // NetworkStream 的 ReadAsync/WriteAsync 委托到 TcpClient 的
                // ReceiveAsync/SendAsync，由 builtin_method 上层处理 receiver 切换。
                // 此处不应到达——若到达说明 receiver 未正确切换，回退 None。
                return None;
            }
            _ => return None,
        };
        Some(result)
    }

    /// L3 Orm SQLite MVP：`SqliteDb.*` → `rt_sqlite_*`（1-based int 句柄）。
    pub(super) fn try_emit_sqlite_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let arg0 = args.first().cloned().unwrap_or(MirOperand::ConstInt(0));
        let arg1 = args.get(1).cloned();
        let result: TyVal = match method {
            "Open" => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_sqlite_open(ptr {path})"));
                ("i32".into(), tmp)
            }
            "Close" => {
                let (_, db) = self.emit_operand(&arg0);
                self.emit(&format!("call void @rt_sqlite_close(i32 {db})"));
                ("void".into(), String::new())
            }
            "Exec" => {
                let (_, db) = self.emit_operand(&arg0);
                let (_, sql) =
                    self.emit_operand(&arg1.unwrap_or(MirOperand::ConstString(String::new())));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_sqlite_exec(i32 {db}, ptr {sql})"
                ));
                ("i32".into(), tmp)
            }
            "Prepare" => {
                let (_, db) = self.emit_operand(&arg0);
                let (_, sql) =
                    self.emit_operand(&arg1.unwrap_or(MirOperand::ConstString(String::new())));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_sqlite_prepare(i32 {db}, ptr {sql})"
                ));
                ("i32".into(), tmp)
            }
            "Step" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_sqlite_step(i32 {stmt})"));
                ("i32".into(), tmp)
            }
            "ColumnCount" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_sqlite_column_count(i32 {stmt})"
                ));
                ("i32".into(), tmp)
            }
            "ColumnType" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let (_, col) = self.emit_operand(&arg1.unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_sqlite_column_type(i32 {stmt}, i32 {col})"
                ));
                ("i32".into(), tmp)
            }
            "ColumnInt" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let (_, col) = self.emit_operand(&arg1.unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_sqlite_column_int(i32 {stmt}, i32 {col})"
                ));
                ("i32".into(), tmp)
            }
            "ColumnDouble" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let (_, col) = self.emit_operand(&arg1.unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call double @rt_sqlite_column_double(i32 {stmt}, i32 {col})"
                ));
                ("double".into(), tmp)
            }
            "ColumnText" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let (_, col) = self.emit_operand(&arg1.unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_sqlite_column_text(i32 {stmt}, i32 {col})"
                ));
                ("ptr".into(), tmp)
            }
            "ColumnName" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let (_, col) = self.emit_operand(&arg1.unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_sqlite_column_name(i32 {stmt}, i32 {col})"
                ));
                ("ptr".into(), tmp)
            }
            "Finalize" => {
                let (_, stmt) = self.emit_operand(&arg0);
                self.emit(&format!("call void @rt_sqlite_finalize(i32 {stmt})"));
                ("void".into(), String::new())
            }
            "Errmsg" => {
                let (_, db) = self.emit_operand(&arg0);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_sqlite_errmsg(i32 {db})"));
                ("ptr".into(), tmp)
            }
            "BindText" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let (_, idx) = self.emit_operand(&arg1.unwrap_or(MirOperand::ConstInt(0)));
                let (_, text) = self.emit_operand(
                    &args
                        .get(2)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_sqlite_bind_text(i32 {stmt}, i32 {idx}, ptr {text})"
                ));
                ("i32".into(), tmp)
            }
            "BindInt" => {
                let (_, stmt) = self.emit_operand(&arg0);
                let (_, idx) = self.emit_operand(&arg1.unwrap_or(MirOperand::ConstInt(0)));
                let (_, val) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_sqlite_bind_int(i32 {stmt}, i32 {idx}, i32 {val})"
                ));
                ("i32".into(), tmp)
            }
            "Changes" => {
                let (_, db) = self.emit_operand(&arg0);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_sqlite_changes(i32 {db})"));
                ("i32".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 029 M1：`ImageNative.*` → `rt_image_*`（std/Drawing 内部 C ABI 门面）。
    ///
    /// 像素缓冲形态 = **NativePtr 句柄**：Arc 侧 Bitmap 持 `long` 句柄（RGBA8 缓冲
    /// 指针的整数形态）；decode/encode 输出为 stbi/malloc 原生缓冲，释放经
    /// `rt_image_free`。句柄 `long` 参数 `inttoptr` 成 `ptr` 传 C ABI；`out`
    /// 参数（`long` 缓冲句柄 / `int` 宽高 / `long` 长度）经 byref 槽位
    /// （`emit_native_byref_arg` 同构，MIR 将 `out id` lower 为 Local/AddrOf）。
    pub(super) fn try_emit_image_native_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        // 取第 i 个 int 实参（缺省 dflt）。
        let int_arg = |em: &mut Self, i: usize, dflt: i32| -> String {
            let (ty, val) = em.emit_operand(
                &args
                    .get(i)
                    .cloned()
                    .unwrap_or(MirOperand::ConstInt(dflt as i64)),
            );
            debug_assert_eq!(ty, "i32", "ImageNative int arg {i} typed {ty}");
            val
        };
        // 取第 i 个 long 实参 → inttoptr 成 ptr。
        let long_arg_as_ptr = |em: &mut Self, i: usize, dflt: i64| -> String {
            let (ty, val) =
                em.emit_operand(&args.get(i).cloned().unwrap_or(MirOperand::ConstInt(dflt)));
            debug_assert_eq!(ty, "i64", "ImageNative long arg {i} typed {ty}");
            let p = em.fresh_temp();
            em.emit(&format!("{p} = inttoptr i64 {val} to ptr"));
            p
        };
        // 第 i 个 out 参数槽位（Local/AddrOf → 局部栈地址）。
        let out_slot = |em: &mut Self, i: usize, pointee: &str| -> String {
            let arg = args.get(i).cloned().unwrap_or(MirOperand::ConstNull);
            em.emit_native_byref_arg(&arg, pointee)
        };

        let result: TyVal = match method {
            "Alloc" => {
                let w = int_arg(self, 0, 0);
                let h = int_arg(self, 1, 0);
                let p = self.fresh_temp();
                self.emit(&format!("{p} = call ptr @rt_image_alloc(i32 {w}, i32 {h})"));
                let h64 = self.fresh_temp();
                self.emit(&format!("{h64} = ptrtoint ptr {p} to i64"));
                ("i64".into(), h64)
            }
            "Decode" => {
                let (_, data) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let len32 = self.fresh_temp();
                self.emit(&format!("{len32} = call i32 @rt_array_length(ptr {data})"));
                let len = self.fresh_temp();
                self.emit(&format!("{len} = zext i32 {len32} to i64"));
                let rgba = out_slot(self, 1, "i64");
                let w = out_slot(self, 2, "i32");
                let h = out_slot(self, 3, "i32");
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_decode(ptr {data}, i64 {len}, {rgba}, {w}, {h})"
                ));
                ("i32".into(), tmp)
            }
            "DecodeFile" => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let rgba = out_slot(self, 1, "i64");
                let w = out_slot(self, 2, "i32");
                let h = out_slot(self, 3, "i32");
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_decode_file(ptr {path}, {rgba}, {w}, {h})"
                ));
                ("i32".into(), tmp)
            }
            "EncodePng" => {
                let rgba = long_arg_as_ptr(self, 0, 0);
                let w = int_arg(self, 1, 0);
                let h = int_arg(self, 2, 0);
                let buf = out_slot(self, 3, "i64");
                let len = out_slot(self, 4, "i64");
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_encode_png(ptr {rgba}, i32 {w}, i32 {h}, {buf}, {len})"
                ));
                ("i32".into(), tmp)
            }
            "EncodeJpg" => {
                let rgba = long_arg_as_ptr(self, 0, 0);
                let w = int_arg(self, 1, 0);
                let h = int_arg(self, 2, 0);
                let q = int_arg(self, 3, 90);
                let buf = out_slot(self, 4, "i64");
                let len = out_slot(self, 5, "i64");
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_encode_jpg(ptr {rgba}, i32 {w}, i32 {h}, i32 {q}, {buf}, {len})"
                ));
                ("i32".into(), tmp)
            }
            "GetPixel" => {
                let rgba = long_arg_as_ptr(self, 0, 0);
                let w = int_arg(self, 1, 0);
                let h = int_arg(self, 2, 0);
                let x = int_arg(self, 3, 0);
                let y = int_arg(self, 4, 0);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i64 @rt_image_get_pixel(ptr {rgba}, i32 {w}, i32 {h}, i32 {x}, i32 {y})"
                ));
                ("i64".into(), tmp)
            }
            "SetPixel" => {
                let rgba = long_arg_as_ptr(self, 0, 0);
                let w = int_arg(self, 1, 0);
                let h = int_arg(self, 2, 0);
                let x = int_arg(self, 3, 0);
                let y = int_arg(self, 4, 0);
                let (_, argb) =
                    self.emit_operand(&args.get(5).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_set_pixel(ptr {rgba}, i32 {w}, i32 {h}, i32 {x}, i32 {y}, i64 {argb})"
                ));
                ("i32".into(), tmp)
            }
            "FillRect" => {
                let rgba = long_arg_as_ptr(self, 0, 0);
                let w = int_arg(self, 1, 0);
                let h = int_arg(self, 2, 0);
                let x = int_arg(self, 3, 0);
                let y = int_arg(self, 4, 0);
                let rw = int_arg(self, 5, 0);
                let rh = int_arg(self, 6, 0);
                let (_, argb) =
                    self.emit_operand(&args.get(7).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_fill_rect(ptr {rgba}, i32 {w}, i32 {h}, i32 {x}, i32 {y}, i32 {rw}, i32 {rh}, i64 {argb})"
                ));
                ("i32".into(), tmp)
            }
            "WriteBuffer" => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let buf = long_arg_as_ptr(self, 1, 0);
                let (_, len) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_write_buffer(ptr {path}, ptr {buf}, i64 {len})"
                ));
                ("i32".into(), tmp)
            }
            "Free" => {
                let p = long_arg_as_ptr(self, 0, 0);
                self.emit(&format!("call void @rt_image_free(ptr {p})"));
                ("void".into(), String::new())
            }
            // RFC 029 M2：GIF 多帧解码——全部帧连续缓冲 + 每帧延时（毫秒）数组。
            "DecodeGif" => {
                let (_, data) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let len32 = self.fresh_temp();
                self.emit(&format!("{len32} = call i32 @rt_array_length(ptr {data})"));
                let len = self.fresh_temp();
                self.emit(&format!("{len} = zext i32 {len32} to i64"));
                let rgba = out_slot(self, 1, "i64");
                let w = out_slot(self, 2, "i32");
                let h = out_slot(self, 3, "i32");
                let fc = out_slot(self, 4, "i32");
                let delays = out_slot(self, 5, "i64");
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_decode_gif(ptr {data}, i64 {len}, {rgba}, {w}, {h}, {fc}, {delays})"
                ));
                ("i32".into(), tmp)
            }
            // RFC 029 M2：定位 GIF 帧 i 的起始指针（无拷贝；越界返回 NULL → 0）。
            "GifFrame" => {
                let rgba = long_arg_as_ptr(self, 0, 0);
                let w = int_arg(self, 1, 0);
                let h = int_arg(self, 2, 0);
                let fi = int_arg(self, 3, 0);
                let p = self.fresh_temp();
                self.emit(&format!(
                    "{p} = call ptr @rt_image_gif_frame(ptr {rgba}, i32 {w}, i32 {h}, i32 {fi})"
                ));
                let h64 = self.fresh_temp();
                self.emit(&format!("{h64} = ptrtoint ptr {p} to i64"));
                ("i64".into(), h64)
            }
            // RFC 029 M2：读取 GIF 帧 i 的延时（毫秒；越界 -1）。
            "GifDelay" => {
                let delays = long_arg_as_ptr(self, 0, 0);
                let fi = int_arg(self, 1, 0);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_gif_delay(ptr {delays}, i32 {fi})"
                ));
                ("i32".into(), tmp)
            }
            // RFC 029 M2：SVG 光栅化 → 直通 RGBA8（scale 为 float 缩放系数）。
            "DecodeSvg" => {
                let (_, data) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let len32 = self.fresh_temp();
                self.emit(&format!("{len32} = call i32 @rt_array_length(ptr {data})"));
                let len = self.fresh_temp();
                self.emit(&format!("{len} = zext i32 {len32} to i64"));
                let (_, scale) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstFloat(1.0)));
                let rgba = out_slot(self, 2, "i64");
                let w = out_slot(self, 3, "i32");
                let h = out_slot(self, 4, "i32");
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_decode_svg(ptr {data}, i64 {len}, {rgba}, {w}, {h}, float {scale})"
                ));
                ("i32".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 029 M2：`QrCodeNative.Encode(text, ecc, mask, modules, out size)`
    /// → `rt_qrcode_encode`（文本 → qrcodegen bit-packed 模块矩阵）。
    /// modules 由调用方预分配（≥ 3918 字节）；成功时 modules[0] = 边长。
    pub(super) fn try_emit_qrcode_native_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let result: TyVal = match method {
            "Encode" => {
                let (_, text) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, ecc) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(1)));
                let (_, mask) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(-1)));
                let (_, modules) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstNull));
                let size = self.emit_native_byref_arg(
                    &args.get(4).cloned().unwrap_or(MirOperand::ConstNull),
                    "i32",
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_qrcode_encode(ptr {text}, i32 {ecc}, i32 {mask}, ptr {modules}, {size})"
                ));
                ("i32".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 009 M4：`BarcodeNative.QuircDecode(rgba, w, h, textOut, textCap)`
    /// → `rt_barcode_quirc_decode`（RGBA8 → quirc 灰度 → 首个 QR → NUL 终止文本）。
    /// textCap 为 i32 表面参数 → size_t ABI 需 zext i64。
    pub(super) fn try_emit_barcode_native_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let result: TyVal = match method {
            "QuircDecode" => {
                let (_, rgba) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, w) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, h) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, text) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, cap) =
                    self.emit_operand(&args.get(4).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let cap64 = self.fresh_temp();
                self.emit(&format!("{cap64} = zext i32 {cap} to i64"));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_barcode_quirc_decode(ptr {rgba}, i32 {w}, i32 {h}, ptr {text}, i64 {cap64})"
                ));
                ("i32".into(), tmp)
            }
            "OneDDecode" => {
                let (_, rgba) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, w) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, h) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, text) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, cap) =
                    self.emit_operand(&args.get(4).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let cap64 = self.fresh_temp();
                self.emit(&format!("{cap64} = zext i32 {cap} to i64"));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_barcode_1d_decode(ptr {rgba}, i32 {w}, i32 {h}, ptr {text}, i64 {cap64})"
                ));
                ("i32".into(), tmp)
            }
            _ => return None,
        };
        Some(result)
    }

    /// RFC 029 M6：`Font::_*` 私有静态 [Builtin] → `rt_image_font_*`（stb_truetype）。
    /// 句柄在 Arc 侧为 long（i64），C ABI 侧为 opaque ptr（inttoptr）。
    pub(super) fn try_emit_font_native_static(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let handle_ptr = |em: &mut Self| {
            let (_, _, hp) =
                em.emit_handle_as_ptr(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
            hp
        };
        let result: TyVal = match method {
            "Load" => {
                let (_, ttf) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let len32 = self.fresh_temp();
                self.emit(&format!("{len32} = call i32 @rt_array_length(ptr {ttf})"));
                let len = self.fresh_temp();
                self.emit(&format!("{len} = zext i32 {len32} to i64"));
                let (_, size) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstFloat(0.0)));
                let p = self.fresh_temp();
                self.emit(&format!(
                    "{p} = call ptr @rt_image_font_load(ptr {ttf}, i64 {len}, float {size})"
                ));
                let h64 = self.fresh_temp();
                self.emit(&format!("{h64} = ptrtoint ptr {p} to i64"));
                ("i64".into(), h64)
            }
            "Metrics" => {
                let hp = handle_ptr(self);
                let a = self.emit_native_byref_arg(
                    &args.get(1).cloned().unwrap_or(MirOperand::ConstNull),
                    "float",
                );
                let d = self.emit_native_byref_arg(
                    &args.get(2).cloned().unwrap_or(MirOperand::ConstNull),
                    "float",
                );
                let g = self.emit_native_byref_arg(
                    &args.get(3).cloned().unwrap_or(MirOperand::ConstNull),
                    "float",
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_font_metrics(ptr {hp}, {a}, {d}, {g})"
                ));
                ("i32".into(), tmp)
            }
            "Measure" => {
                let hp = handle_ptr(self);
                let (_, text) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call float @rt_image_font_measure(ptr {hp}, ptr {text})"
                ));
                ("float".into(), tmp)
            }
            "Glyph" => {
                let hp = handle_ptr(self);
                let (_, cp) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, bmp) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstNull));
                let w = self.emit_native_byref_arg(
                    &args.get(3).cloned().unwrap_or(MirOperand::ConstNull),
                    "i32",
                );
                let h = self.emit_native_byref_arg(
                    &args.get(4).cloned().unwrap_or(MirOperand::ConstNull),
                    "i32",
                );
                let xoff = self.emit_native_byref_arg(
                    &args.get(5).cloned().unwrap_or(MirOperand::ConstNull),
                    "float",
                );
                let yoff = self.emit_native_byref_arg(
                    &args.get(6).cloned().unwrap_or(MirOperand::ConstNull),
                    "float",
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_image_font_glyph(ptr {hp}, i32 {cp}, ptr {bmp}, {w}, {h}, {xoff}, {yoff})"
                ));
                ("i32".into(), tmp)
            }
            "Free" => {
                let hp = handle_ptr(self);
                self.emit(&format!("call void @rt_image_font_free(ptr {hp})"));
                ("void".into(), String::new())
            }
            _ => return None,
        };
        Some(result)
    }
}

/// Convert a type name string (as stored in vtable layout signatures) to a `TypeId`.
/// Handles primitive names and falls back to `TypeId::Named` for class/struct types.
fn type_name_str_to_type_id(name: &str) -> TypeId {
    match name {
        "int" => TypeId::Int,
        "long" => TypeId::Long,
        "short" => TypeId::Short,
        "byte" => TypeId::Byte,
        "char" => TypeId::Char,
        "float" => TypeId::Float,
        "double" => TypeId::Double,
        "bool" => TypeId::Bool,
        "string" => TypeId::String,
        "void" => TypeId::Void,
        "uint" => TypeId::UInt,
        "ulong" => TypeId::ULong,
        "ushort" => TypeId::UShort,
        "sbyte" => TypeId::SByte,
        other => TypeId::Named(other.into()),
    }
}

impl<'a> FnEmitter<'a> {
    /// 从 `ClassLayout.parent` 返回 `[root, …, class]`（基类在前）。
    fn class_ancestors_base_first(&self, class: &str) -> Vec<String> {
        let mut chain = vec![class.to_string()];
        let mut current = class;
        while let Some(cl) = self.layouts.classes.get(current) {
            if let Some(parent) = &cl.parent {
                chain.push(parent.to_string());
                current = parent.as_str();
            } else {
                break;
            }
        }
        chain.reverse();
        chain
    }
}
