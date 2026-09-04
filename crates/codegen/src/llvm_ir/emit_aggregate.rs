//! Field, struct, array, null-conditional, and layout-helper emission.

use super::*;
use ast::TypeId;
use mir::MirOperand;
use typeck::VirtualSlot;

impl<'a> FnEmitter<'a> {
    // ---- Field access ----

    pub(super) fn emit_field_get(
        &mut self,
        object: &MirOperand,
        class: &str,
        field: &str,
    ) -> TyVal {
        // Task facade (RFC 009 M1): Task<T> 属性经 operand_from_expr 路径进入此处
        //（函数实参等 operand 上下文）。拦截并路由到 rt_task_* ABI，避免直接字段访问
        // 导致类型不匹配（ptr_result 字段被误 load 为 i32）。
        // 与 emit_call.rs try_emit_task_method 对齐，通过 local_type 获取 inner 类型。
        if class == "Task" {
            if let Some(inner) = self.task_inner_type(object) {
                return self.emit_task_field_get(object, field, &inner);
            }
        }
        // RFC 009 M4: SoA 数组的 `arr.Length` 路由到 rt_soa_length。
        // SoA 字段访问融合（arr[i].field → SoaFieldGet）已于 D3 实现（2026-07-26）。
        // RFC 005：Span / ReadOnlySpan → 胖指针 length / IsEmpty。
        if (class == "Span" || class == "ReadOnlySpan") && field == "Length" {
            return self.emit_span_length(object);
        }
        if (class == "Span" || class == "ReadOnlySpan") && field == "IsEmpty" {
            return self.emit_span_is_empty(object);
        }
        if field == "Length" {
            // Primitive `Length`: string → rt_str_length, array → rt_array_length.
            // SoA array → rt_soa_length（RFC 009 M4）。
            // Struct/class fields named `Length` (accessed as properties via
            // `get_Length`) never reach this path; they go through MethodCall.
            let (_, obj) = self.emit_operand(object);
            let tmp = self.fresh_temp();
            if class == "string" {
                self.emit(&format!("{tmp} = call i32 @rt_str_length(ptr {obj})"));
            } else if self.is_soa_array_operand(object) {
                self.emit(&format!("{tmp} = call i32 @rt_soa_length(ptr {obj})"));
            } else {
                self.emit(&format!("{tmp} = call i32 @rt_array_length(ptr {obj})"));
            }
            return ("i32".into(), tmp);
        }
        // CTS facade (RFC 009 M4): CancellationTokenSource 属性访问可能经
        // `operand_from_expr` Field 路径进入此处（尤其作为方法实参时）。
        // CT 与 CTS 共享同一 RtCts* 指针，Token 直接返回 receiver；
        // IsCancellationRequested 调用 rt_cts_is_canceled。
        if class == "CancellationTokenSource" {
            match field {
                "Token" => {
                    let (_, obj) = self.emit_operand(object);
                    return ("ptr".into(), obj);
                }
                "IsCancellationRequested" => {
                    let (_, obj) = self.emit_operand(object);
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = call i32 @rt_cts_is_canceled(ptr {obj})"));
                    return ("i32".into(), tmp);
                }
                _ => {}
            }
        }
        let (offset, field_ty) = if self.layouts.structs.contains_key(class) {
            self.struct_field_info(class, field)
        } else {
            self.field_info(class, field)
        };
        let (obj_ty, obj) = self.emit_operand(object);
        let field_ty_str = llvm_field_type(&field_ty, self.layouts);
        // 嵌套值类型字段访问（如 `t.Position.X`）：内层 `t.Position` 会把外层结构体
        // 按值 load 进寄存器（`%struct.Vector3`），此时直接 GEP 会因寄存器非指针而
        // 触发 LLVM IR 错误。将按值结构体 spill 到栈槽后再取地址。
        let obj_ptr = if obj_ty.starts_with("%struct.") {
            let spill = self.fresh_temp();
            self.emit(&format!("{spill} = alloca {obj_ty}"));
            self.emit(&format!("store {obj_ty} {obj}, ptr {spill}"));
            spill
        } else {
            obj
        };
        let addr = self.fresh_temp();
        self.emit(&format!(
            "{addr} = getelementptr inbounds i8, ptr {obj_ptr}, i32 {offset}"
        ));
        let tmp = self.fresh_temp();
        // 045 M5：用户 struct 字段读挂 struct-path TBAA（与写路径 `emit_field_store`
        // 对称，使 clang 能对用户类型做别名消歧）。仅 struct 值类型触发；类字段 /
        // 运行时句柄 / Task 等非此路径。
        let tbaa_suffix = if self.layouts.structs.contains_key(class) {
            if let Some(tag) = self.user_struct_field_tbaa(class, field, &field_ty, offset) {
                format!(", !tbaa !{tag}")
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        self.emit(&format!(
            "{tmp} = load {field_ty_str}, ptr {addr}{tbaa_suffix}"
        ));
        (field_ty_str, tmp)
    }

    // ---- SoA field access fusion (RFC 009 M4) ----

    /// RFC 009 M4：判断 operand 是否为 SoA 数组（local 变量类型为 `[SoA] struct[]`）。
    /// 用于 `arr.Length` 路由到 `rt_soa_length` 而非 `rt_array_length`。
    fn is_soa_array_operand(&self, operand: &MirOperand) -> bool {
        if let MirOperand::Local(id) = operand {
            if let TypeId::Array { elem } = self.local_type(*id) {
                if let TypeId::Named(name) = elem.as_ref() {
                    return self.layouts.structs.get(name).is_some_and(|s| s.soa);
                }
            }
        }
        false
    }

    /// RFC 009 M4：SoA struct 数组字面量分配。
    ///
    /// 当 `elem_type` 为 `[SoA]` struct 且所有元素为 `StructLit` rvalue 时，
    /// 使用 `rt_soa_array_create` 分配 SoA 布局，并逐字段 store 到对应字段数组。
    /// 返回 None 表示非 SoA 路径或元素非 StructLit，回退到通用数组分配。
    fn try_emit_soa_array_lit(
        &mut self,
        elem_type: &TypeId,
        elements: &[mir::ArrayLitElement],
    ) -> Option<TyVal> {
        let struct_name = match elem_type {
            TypeId::Named(name) => name.as_str(),
            // ArrayLit.elem_type 约定为完整 `T[]`；SoA 元素为 Named struct。
            TypeId::Array { elem } => match elem.as_ref() {
                TypeId::Named(name) => name.as_str(),
                _ => return None,
            },
            _ => return None,
        };
        let layout = self.layouts.structs.get(struct_name)?;
        if !layout.soa {
            return None;
        }
        // 仅处理无 spread 且全 StructLit 的简单情形（M4 MVP）
        if elements
            .iter()
            .any(|e| !matches!(e, mir::ArrayLitElement::Value(rv) if matches!(rv, mir::MirRvalue::StructLit { .. })))
        {
            return None;
        }
        let num_fields = layout.fields.len() as i32;
        let len = elements.len() as i32;

        // 在栈上分配 field_sizes 数组（i32 per field）
        let sizes_arr = self.fresh_temp();
        self.emit(&format!("{sizes_arr} = alloca [{num_fields} x i32]"));
        for (fidx, fl) in layout.fields.iter().enumerate() {
            let size = llvm_size_of_type_str(fl.ty.as_str()) as i32;
            let slot = self.fresh_temp();
            self.emit(&format!(
                "{slot} = getelementptr inbounds [{num_fields} x i32], ptr {sizes_arr}, i32 0, i32 {fidx}"
            ));
            self.emit(&format!("store i32 {size}, ptr {slot}"));
        }

        // 分配 SoA 数组
        let arr = self.fresh_temp();
        self.emit(&format!(
            "{arr} = call ptr @rt_soa_array_create(i32 {len}, i32 {num_fields}, ptr {sizes_arr})"
        ));

        // 逐元素、逐字段 store 到对应字段数组
        for (i, el) in elements.iter().enumerate() {
            let mir::ArrayLitElement::Value(rv) = el else {
                unreachable!()
            };
            let mir::MirRvalue::StructLit {
                fields: lit_fields, ..
            } = rv
            else {
                unreachable!()
            };
            for (fidx, fl) in layout.fields.iter().enumerate() {
                // 查找字面量中对应字段的值（按字段名匹配）
                let Some((_, fop)) = lit_fields.iter().find(|(fname, _)| fname == fl.name) else {
                    continue;
                };
                let (fty, fval) = self.emit_operand(fop);
                let field_arr = self.fresh_temp();
                self.emit(&format!(
                    "{field_arr} = call ptr @rt_soa_field_ptr(ptr {arr}, i32 {fidx})"
                ));
                let elem_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{elem_ptr} = getelementptr inbounds {fty}, ptr {field_arr}, i32 {i}"
                ));
                self.emit(&format!("store {fty} {fval}, ptr {elem_ptr}"));
            }
        }
        Some(("ptr".into(), arr))
    }

    // /// RFC 009 D3：SoA struct 数组字段访问融合（已于 2026-07-26 实现）。
    // ///
    // /// 当 `class` 为 `[SoA]` struct 时，`arr[i].field` 在 MIR lower 阶段
    // /// 被直接降为 `MirRvalue::SoaFieldGet`（见 `lower_expr.rs`），codegen
    // /// 在 `emit_soa_field_get` 中发射：
    // ///   1. `rt_soa_field_ptr(arr, field_idx)` → 字段数组首指针
    // ///   2. `getelementptr field_ty, ptr field_arr, i` → 元素指针
    // ///   3. `load field_ty, ptr elem_ptr` → 字段值
    // ///
    // /// **实现状态**：D3 融合已落地，消除 AoS 回退路径中的 struct 物化开销，
    // /// 字段访问连续利于 SIMD auto-vectorization。

    // ---- Task facade helpers (RFC 009 M1) ----

    /// 从 MirOperand 提取 Task<T> 的 inner 类型 T。
    /// 仅支持 MirOperand::Local（变量），其他形式返回 None（回退到通用字段访问）。
    fn task_inner_type(&self, object: &MirOperand) -> Option<TypeId> {
        if let MirOperand::Local(id) = object {
            match self.local_type(*id) {
                TypeId::Task { inner } => return Some((*inner).clone()),
                // 非泛型 `Task` 在 MIR 局部可能被标为 `Named("Task")`（未归一化为
                // Task<Void>），尤其在 QIF 宿主等生成代码中 `Task _t = ...`。
                // 与 typeck check_type.rs 的非泛型 Task→Task<Void> 别名对齐，识别
                // 为 void 任务，使 `_t.IsCompleted`/`_t.Status` 走 rt_task_* ABI。
                TypeId::Named(n) if n.as_str() == "Task" => return Some(TypeId::Void),
                _ => {}
            }
        }
        None
    }

    /// Task 属性访问 → rt_task_* ABI 路由。与 emit_call.rs try_emit_task_method 对齐。
    fn emit_task_field_get(&mut self, object: &MirOperand, field: &str, inner: &TypeId) -> TyVal {
        let (_, recv) = self.emit_operand(object);
        let tmp = self.fresh_temp();
        match field {
            "Status" => {
                self.emit(&format!("{tmp} = call i32 @rt_task_status(ptr {recv})"));
                ("i32".into(), tmp)
            }
            "IsCompleted" => {
                self.emit(&format!("{tmp} = call i32 @rt_task_status(ptr {recv})"));
                let cmp = self.fresh_temp();
                self.emit(&format!("{cmp} = icmp eq i32 {tmp}, 0")); // RT_TASK_READY == 0
                ("i1".into(), cmp)
            }
            "IsCanceled" => {
                self.emit(&format!(
                    "{tmp} = call i32 @rt_task_is_canceled(ptr {recv})"
                ));
                ("i32".into(), tmp)
            }
            "IsFaulted" => {
                self.emit(&format!("{tmp} = call i32 @rt_task_is_faulted(ptr {recv})"));
                ("i32".into(), tmp)
            }
            "Exception" => {
                self.emit(&format!(
                    "{tmp} = call ptr @rt_task_get_exception(ptr {recv})"
                ));
                ("ptr".into(), tmp)
            }
            _ => {
                // 按 inner 类型选择 ABI（与 try_emit_task_method get_Result 分支一致）
                match inner {
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
                    TypeId::Float | TypeId::Double | TypeId::Long | TypeId::ULong => {
                        let (llvm_ty, size) = if matches!(inner, TypeId::Long | TypeId::ULong) {
                            ("i64".to_string(), 8)
                        } else if matches!(inner, TypeId::Float) {
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
                    // 引用族（String/Named/Array/Task/Func）+ 兜底 → rt_task_result_ptr
                    _ => {
                        self.emit(&format!("{tmp} = call ptr @rt_task_result_ptr(ptr {recv})"));
                        // CD-29：与 emit_call.rs try_emit_task_method get_Result
                        // 一致——class 结果 retain（借引用 → 独立 +1，与调用方
                        // 局部 epilogue dec 配对），防同步提取 UAF。
                        if Self::arc_class_place(inner, self.layouts) {
                            self.emit(&format!("call void @rt_arc_inc(ptr {tmp})"));
                        }
                        ("ptr".into(), tmp)
                    }
                }
            }
        }
    }

    // ---- Interface fat pointer construction ----

    /// 分配并填充接口胖指针 `{ ptr obj, ptr itable }`。
    ///
    /// `heap = true` 用于**接口类型函数返回**：栈 alloca 在 `ret` 后随被调帧
    /// 弹出，调用方后续读取可能已被其它调用覆盖 → ACCESS_VIOLATION；堆分配
    /// 与 `materialize_null_return` 的 struct 返回先例一致（注释：返回值必须
    /// 堆分配，避免 ret 栈 alloca 悬空）。
    pub(super) fn emit_make_iface(
        &mut self,
        class: &str,
        iface: &str,
        object: &MirOperand,
        heap: bool,
    ) -> TyVal {
        // RFC 004 P0 Phase 2：struct → interface 装箱分派。`class` 为 "{Struct}_Box"
        // 时先深拷贝装箱（`emit_box` struct 分支），fat[0] 存 box ptr（非裸 struct
        // ptr），接口分派 thunk 据此把 `this` 重定位到 box+24。struct 无 vtable，
        // 不走 `MakeIfaceDyn`（读 obj+8 会 AV）。
        let (_, obj) = match class.strip_suffix("_Box") {
            Some(struct_name) if self.layouts.structs.contains_key(struct_name) => {
                self.emit_box(object, &TypeId::Named(struct_name.into()))
            }
            _ => self.emit_operand(object),
        };
        let vtable_name = format!("@.itable.{class}_{iface}");
        let tmp = self.fresh_temp();
        if heap {
            self.emit(&format!("{tmp} = call ptr @calloc(i64 1, i64 16)"));
        } else {
            self.emit(&format!("{tmp} = alloca {{ ptr, ptr }}"));
        }
        // 堆盒 = 持引用（与 Task<接口> 装箱对偶）：盒可跨越创建帧存活
        // （存入字段 / 传参后被 callee 保存），缺 retain 时创建方局部出口
        // dec 会把 rc=1 对象提前释放 → 盒悬垂 → 接口分派解引用 UAF。
        // `rt_arc_inc` 对 null 安全（`(I…)null` 转型）。boxed struct 已由
        // `emit_box` 产出新鲜 rc=1 盒，无需再 inc（否则 +1 泄漏）。
        if heap && !class.ends_with("_Box") {
            self.emit(&format!("call void @rt_arc_inc(ptr {obj})"));
        }
        let obj_addr = self.fresh_temp();
        self.emit(&format!(
            "{obj_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {tmp}, i32 0, i32 0"
        ));
        self.emit(&format!("store ptr {obj}, ptr {obj_addr}"));
        let vtbl_addr = self.fresh_temp();
        self.emit(&format!(
            "{vtbl_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {tmp}, i32 0, i32 1"
        ));
        self.emit(&format!("store ptr {vtable_name}, ptr {vtbl_addr}"));
        ("ptr".into(), tmp)
    }

    /// object / 基类静态类型 → 接口：经 `rt_obj_to_iface` 读对象 runtime typeinfo
    /// 的 `interface_itables`，统一覆盖 class 与 boxed struct（单一分派路径）。
    /// downcast 失败（对象未实现接口 → null itable）→ `rt_panic` 清晰异常。
    pub(super) fn emit_make_iface_dyn(
        &mut self,
        iface: &str,
        object: &MirOperand,
        heap: bool,
    ) -> TyVal {
        let (_, obj) = self.emit_operand(object);
        let fat = self.fresh_temp();
        if heap {
            self.emit(&format!("{fat} = call ptr @calloc(i64 1, i64 16)"));
        } else {
            self.emit(&format!("{fat} = alloca {{ ptr, ptr }}"));
        }
        // 堆盒 = 持引用（与 `emit_make_iface` 一致；`rt_arc_inc` null 安全）。
        if heap {
            self.emit(&format!("call void @rt_arc_inc(ptr {obj})"));
        }
        let obj_addr = self.fresh_temp();
        self.emit(&format!(
            "{obj_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 0"
        ));
        self.emit(&format!("store ptr {obj}, ptr {obj_addr}"));
        let vtbl_slot = self.fresh_temp();
        self.emit(&format!(
            "{vtbl_slot} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 1"
        ));
        self.emit(&format!("store ptr null, ptr {vtbl_slot}"));

        // `(I)null` → null 接口引用（合法，不抛）；否则经 rt_obj_to_iface 动态
        // 查找 itable，失败（返回 null）→ InvalidCastException（非崩溃）。
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {obj}, null"));
        let join = self.fresh_label();
        let lookup = self.fresh_label();
        self.emit(&format!("br i1 {is_null}, label %{join}, label %{lookup}"));
        self.emit(&format!("{lookup}:"));
        let it = self.fresh_temp();
        // RFC 038 M2：外部接口（external_class_names）typeinfo 经守卫登记 external 声明。
        let iface_ti = self
            .typeinfo_global(iface)
            .unwrap_or_else(|| format!("@.typeinfo.{iface}"));
        self.emit(&format!(
            "{it} = call ptr @rt_obj_to_iface(ptr {obj}, ptr {iface_ti})"
        ));
        self.emit(&format!("store ptr {it}, ptr {vtbl_slot}"));
        let it_null = self.fresh_temp();
        self.emit(&format!("{it_null} = icmp eq ptr {it}, null"));
        let ok = self.fresh_label();
        let bad = self.fresh_label();
        self.emit(&format!("br i1 {it_null}, label %{bad}, label %{ok}"));
        self.emit(&format!("{bad}:"));
        self.emit("call void @rt_panic(ptr @__arc_invalid_cast)");
        self.emit("unreachable");
        self.emit(&format!("{ok}:"));
        self.emit(&format!("br label %{join}"));
        self.emit(&format!("{join}:"));

        ("ptr".into(), fat)
    }

    /// Interface → variance-compatible interface：比较源 itable 指针并重绑定。
    ///
    /// 不依赖 class vtable / type_id（实现类可能无虚方法），候选为同时具备
    /// `@.itable.{C}_{from}` 与 `@.itable.{C}_{to}` 的类。
    pub(super) fn emit_adapt_iface(
        &mut self,
        from_iface: &str,
        to_iface: &str,
        object: &MirOperand,
        heap: bool,
    ) -> TyVal {
        let (_, src_fat) = self.emit_operand(object);
        let fat = self.fresh_temp();
        if heap {
            self.emit(&format!("{fat} = call ptr @calloc(i64 1, i64 16)"));
        } else {
            self.emit(&format!("{fat} = alloca {{ ptr, ptr }}"));
        }

        let src_obj_a = self.fresh_temp();
        self.emit(&format!(
            "{src_obj_a} = getelementptr inbounds {{ ptr, ptr }}, ptr {src_fat}, i32 0, i32 0"
        ));
        let obj = self.fresh_temp();
        self.emit(&format!("{obj} = load ptr, ptr {src_obj_a}"));
        // 堆盒 = 持引用：adapt 生成新盒，与 `emit_make_iface` 的堆盒语义一致
        // （`rt_arc_inc` null 安全）。
        if heap {
            self.emit(&format!("call void @rt_arc_inc(ptr {obj})"));
        }
        let src_it_a = self.fresh_temp();
        self.emit(&format!(
            "{src_it_a} = getelementptr inbounds {{ ptr, ptr }}, ptr {src_fat}, i32 0, i32 1"
        ));
        let src_it = self.fresh_temp();
        self.emit(&format!("{src_it} = load ptr, ptr {src_it_a}"));

        let dst_obj_a = self.fresh_temp();
        self.emit(&format!(
            "{dst_obj_a} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 0"
        ));
        self.emit(&format!("store ptr {obj}, ptr {dst_obj_a}"));
        let dst_vt = self.fresh_temp();
        self.emit(&format!(
            "{dst_vt} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 1"
        ));
        // Default: keep source itable if no candidate matches (should not happen).
        self.emit(&format!("store ptr {src_it}, ptr {dst_vt}"));

        let pairs: Vec<(String, String)> = self
            .layouts
            .classes
            .values()
            .filter(|c| {
                c.interfaces.iter().any(|i| i.as_str() == from_iface)
                    && c.interfaces.iter().any(|i| i.as_str() == to_iface)
            })
            .map(|c| {
                (
                    format!("@.itable.{}_{}", c.name, from_iface),
                    format!("@.itable.{}_{}", c.name, to_iface),
                )
            })
            .collect();

        let join = self.fresh_label();
        if pairs.is_empty() {
            self.emit(&format!("br label %{join}"));
            self.emit(&format!("{join}:"));
            return ("ptr".into(), fat);
        }

        for (i, (from_it, to_it)) in pairs.iter().enumerate() {
            let matched = self.fresh_label();
            let next = if i + 1 < pairs.len() {
                self.fresh_label()
            } else {
                join.clone()
            };
            let cmp = self.fresh_temp();
            self.emit(&format!("{cmp} = icmp eq ptr {src_it}, {from_it}"));
            self.emit(&format!("br i1 {cmp}, label %{matched}, label %{next}"));
            self.emit(&format!("{matched}:"));
            self.emit(&format!("store ptr {to_it}, ptr {dst_vt}"));
            self.emit(&format!("br label %{join}"));
            if i + 1 < pairs.len() {
                self.emit(&format!("{next}:"));
            }
        }
        self.emit(&format!("{join}:"));
        ("ptr".into(), fat)
    }

    // ---- Struct literal ----

    pub(super) fn emit_struct_lit(
        &mut self,
        struct_name: &str,
        fields: &[(String, MirOperand)],
    ) -> TyVal {
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = alloca %struct.{struct_name}"));
        for (fname, fop) in fields {
            let (offset, field_ty) = self.struct_field_info(struct_name, fname);
            let (fty, fval) = self.emit_operand(fop);
            let addr = self.fresh_temp();
            self.emit(&format!(
                "{addr} = getelementptr inbounds i8, ptr {tmp}, i32 {offset}"
            ));
            // RFC 004 生命周期（D3）：variant 字段值深拷贝到堆——boxed struct
            // 随其创建帧消亡时，裸存指针会悬垂（与 FieldSet 同因）。
            let store_val = if self.layouts.variants.contains_key(field_ty.as_str()) && fty == "ptr"
            {
                self.emit_variant_deep_copy(&field_ty, &fval)
            } else {
                fval
            };
            self.emit(&format!("store {fty} {store_val}, ptr {addr}"));
        }
        ("ptr".into(), tmp)
    }

    // ---- Array literal ----

    pub(super) fn emit_array_lit(
        &mut self,
        elem_type: &TypeId,
        elements: &[mir::ArrayLitElement],
    ) -> TyVal {
        // RFC 009 M4: SoA struct 数组分配路径。
        // 当 elem_type 为 [SoA] struct 且无 spread 时，使用 rt_soa_array_create
        // 分配 SoA 布局（每字段独立连续数组），并逐字段 store。
        if let Some(ty_val) = self.try_emit_soa_array_lit(elem_type, elements) {
            return ty_val;
        }
        // 元素存储类型：对数组嵌套（T[]）解包为元素类型；struct 以 ptr 存储。
        // 对 Task<T> 等 facade 类型，llvm_type_of 返回 "ptr"（句柄），与
        // try_emit_task_static 返回的 ("ptr", tmp) 一致，store 类型匹配。
        let (elem_ty, inner_elem_ty) = match elem_type {
            TypeId::Array { elem } => {
                // Structs are stored by reference (ptr), not by value, since
                // `emit_struct_lit` returns a ptr to an alloca'd struct.
                if let TypeId::Named(name) = elem.as_ref() {
                    if self.layouts.structs.contains_key(name) {
                        ("ptr".into(), elem.as_ref().clone())
                    } else {
                        (llvm_type_of(elem, self.layouts), elem.as_ref().clone())
                    }
                } else {
                    (llvm_type_of(elem, self.layouts), elem.as_ref().clone())
                }
            }
            other => (llvm_type_of(other, self.layouts), other.clone()),
        };
        let expected_elem = match elem_type {
            TypeId::Array { elem } => elem.as_ref().clone(),
            other => other.clone(),
        };
        let elem_size = llvm_size_of(&inner_elem_ty) as i32;
        let has_spread = elements
            .iter()
            .any(|e| matches!(e, mir::ArrayLitElement::Spread(_)));

        if !has_spread {
            // RFC 015 Phase B / RFC 004 M2：使用 rt_array_create 分配带 RtArrayHeader
            // 的堆数组，使 `array.Length` 能经 rt_array_length 读取 header 中的长度。
            let len = elements.len();
            let tmp = self.fresh_temp();
            self.emit(&format!(
                "{tmp} = call ptr @rt_array_create(i32 {len}, i32 {elem_size})"
            ));
            for (i, el) in elements.iter().enumerate() {
                let mir::ArrayLitElement::Value(rv) = el else {
                    unreachable!("no-spread path");
                };
                let (ety, eval) = self.emit_rvalue_typed(rv, &expected_elem);
                // 元素值须按元素存储类型收窄/加宽后 store：整数字面量按 i32 发射，
                // `byte[]`（elem_ty=i8）直接 `store i32` 会在每个元素槽写 4 字节，
                // 末元素越过 rt_array_create 分配的 payload 边界 → 堆越界写
                // （0xC0000374 于进程退出时被 CRT 堆校验检出）。
                let (store_ty, store_val) = if ety == elem_ty {
                    (ety, eval)
                } else {
                    self.coerce_value(&ety, eval, &elem_ty)
                };
                let addr = self.fresh_temp();
                self.emit(&format!(
                    "{addr} = getelementptr inbounds {elem_ty}, ptr {tmp}, i32 {i}"
                ));
                self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
            }
            return ("ptr".into(), tmp);
        }

        // RFC 017 #8：含 spread — 运行时求和长度后 memcpy/store。
        let total = self.fresh_temp();
        self.emit(&format!("{total} = alloca i32"));
        self.emit(&format!("store i32 0, ptr {total}"));
        let mut spread_ptrs: Vec<(String, String)> = Vec::new(); // (arr_tmp, len_tmp)
        for el in elements {
            match el {
                mir::ArrayLitElement::Value(_) => {
                    let cur = self.fresh_temp();
                    self.emit(&format!("{cur} = load i32, ptr {total}"));
                    let next = self.fresh_temp();
                    self.emit(&format!("{next} = add i32 {cur}, 1"));
                    self.emit(&format!("store i32 {next}, ptr {total}"));
                }
                mir::ArrayLitElement::Spread(op) => {
                    let (_, arr) = self.emit_operand(op);
                    let len = self.fresh_temp();
                    self.emit(&format!("{len} = call i32 @rt_array_length(ptr {arr})"));
                    let cur = self.fresh_temp();
                    self.emit(&format!("{cur} = load i32, ptr {total}"));
                    let next = self.fresh_temp();
                    self.emit(&format!("{next} = add i32 {cur}, {len}"));
                    self.emit(&format!("store i32 {next}, ptr {total}"));
                    spread_ptrs.push((arr, len));
                }
            }
        }
        let total_val = self.fresh_temp();
        self.emit(&format!("{total_val} = load i32, ptr {total}"));
        let tmp = self.fresh_temp();
        self.emit(&format!(
            "{tmp} = call ptr @rt_array_create(i32 {total_val}, i32 {elem_size})"
        ));

        let idx = self.fresh_temp();
        self.emit(&format!("{idx} = alloca i32"));
        self.emit(&format!("store i32 0, ptr {idx}"));
        let mut spread_i = 0usize;
        for el in elements {
            match el {
                mir::ArrayLitElement::Value(rv) => {
                    let (ety, eval) = self.emit_rvalue_typed(rv, &expected_elem);
                    let (store_ty, store_val) = if ety == elem_ty {
                        (ety, eval)
                    } else {
                        self.coerce_value(&ety, eval, &elem_ty)
                    };
                    let i = self.fresh_temp();
                    self.emit(&format!("{i} = load i32, ptr {idx}"));
                    let addr = self.fresh_temp();
                    self.emit(&format!(
                        "{addr} = getelementptr inbounds {elem_ty}, ptr {tmp}, i32 {i}"
                    ));
                    self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
                    let next = self.fresh_temp();
                    self.emit(&format!("{next} = add i32 {i}, 1"));
                    self.emit(&format!("store i32 {next}, ptr {idx}"));
                }
                mir::ArrayLitElement::Spread(_) => {
                    let (arr, len) = &spread_ptrs[spread_i];
                    spread_i += 1;
                    let i = self.fresh_temp();
                    self.emit(&format!("{i} = load i32, ptr {idx}"));
                    let dest = self.fresh_temp();
                    self.emit(&format!(
                        "{dest} = getelementptr inbounds {elem_ty}, ptr {tmp}, i32 {i}"
                    ));
                    let nbytes = self.fresh_temp();
                    self.emit(&format!("{nbytes} = mul i32 {len}, {elem_size}"));
                    let nbytes64 = self.fresh_temp();
                    self.emit(&format!("{nbytes64} = zext i32 {nbytes} to i64"));
                    self.emit(&format!(
                        "call void @llvm.memcpy.p0.p0.i64(ptr {dest}, ptr {arr}, i64 {nbytes64}, i1 false)"
                    ));
                    // 仅 class 实例指针带 ArcHeader；`rt_array_create` 数组与
                    // C 字符串字面量不是 ARC 对象——对其 rt_arc_inc 会改写 payload/
                    // 只读段（嵌套 `[..a]` / `string[]` spread 静默错或崩溃）。
                    let needs_arc_inc = match &inner_elem_ty {
                        TypeId::Named(name) => {
                            self.layouts.classes.contains_key(name)
                                && !is_opaque_runtime_handle(name)
                        }
                        _ => false,
                    };
                    if needs_arc_inc {
                        let j = self.fresh_temp();
                        self.emit(&format!("{j} = alloca i32"));
                        self.emit(&format!("store i32 0, ptr {j}"));
                        let loop_h = self.fresh_label();
                        let loop_b = self.fresh_label();
                        let loop_e = self.fresh_label();
                        self.emit(&format!("br label %{loop_h}"));
                        self.emit_label(&loop_h);
                        let jv = self.fresh_temp();
                        self.emit(&format!("{jv} = load i32, ptr {j}"));
                        let cmp = self.fresh_temp();
                        self.emit(&format!("{cmp} = icmp slt i32 {jv}, {len}"));
                        self.emit(&format!("br i1 {cmp}, label %{loop_b}, label %{loop_e}"));
                        self.emit_label(&loop_b);
                        let el_addr = self.fresh_temp();
                        self.emit(&format!(
                            "{el_addr} = getelementptr inbounds {elem_ty}, ptr {dest}, i32 {jv}"
                        ));
                        let el_val = self.fresh_temp();
                        self.emit(&format!("{el_val} = load ptr, ptr {el_addr}"));
                        self.emit(&format!("call void @rt_arc_inc(ptr {el_val})"));
                        let jn = self.fresh_temp();
                        self.emit(&format!("{jn} = add i32 {jv}, 1"));
                        self.emit(&format!("store i32 {jn}, ptr {j}"));
                        self.emit(&format!("br label %{loop_h}"));
                        self.emit_label(&loop_e);
                    }
                    let next = self.fresh_temp();
                    self.emit(&format!("{next} = add i32 {i}, {len}"));
                    self.emit(&format!("store i32 {next}, ptr {idx}"));
                }
            }
        }
        ("ptr".into(), tmp)
    }

    /// `new T[n]` — 运行时长度、零初始化的堆数组分配。
    /// 发射 `rt_array_create(length, elem_size)`（带 RtArrayHeader，`Length` 可读）。
    /// `elem_type` 为元素类型（不含数组后缀）；`length` 为运行时长度操作数（int）。
    pub(super) fn emit_new_array(&mut self, elem_type: &TypeId, length: &MirOperand) -> TyVal {
        let elem_size = llvm_size_of(elem_type) as i32;
        let (_, len) = self.emit_operand(length);
        let tmp = self.fresh_temp();
        self.emit(&format!(
            "{tmp} = call ptr @rt_array_create(i32 {len}, i32 {elem_size})"
        ));
        ("ptr".into(), tmp)
    }

    // ---- Index access ----

    pub(super) fn emit_index_get(
        &mut self,
        array: &MirOperand,
        index: &MirOperand,
        elem_type: &TypeId,
    ) -> TyVal {
        let (_, arr) = self.emit_operand(array);
        let (_, idx) = self.emit_operand(index);
        // RFC 009 D3：SoA 元素整体读取（`Particle p = soaArr[i]` / 值语义）——
        // 从各字段数组逐字段 gather 到 AoS 临时 struct（与 `emit_struct_lit` 同构），
        // 避免对 `rt_soa_array` 描述符按 AoS 布局 GEP 导致越界读写。
        // 返回 `ptr`（struct 按引用存储），与常规 `%struct.Particle*` 数组元素一致。
        if let TypeId::Named(name) = elem_type {
            if self.layouts.structs.get(name).is_some_and(|s| s.soa) {
                let struct_name = name.as_str();
                let layout = self.layouts.structs.get(struct_name).unwrap();
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = alloca %struct.{struct_name}"));
                for (fidx, fl) in layout.fields.iter().enumerate() {
                    let fty = llvm_field_type(fl.ty.as_ref(), self.layouts);
                    let field_arr = self.fresh_temp();
                    self.emit(&format!(
                        "{field_arr} = call ptr @rt_soa_field_ptr(ptr {arr}, i32 {fidx})"
                    ));
                    let elem_ptr = self.fresh_temp();
                    self.emit(&format!(
                        "{elem_ptr} = getelementptr inbounds {fty}, ptr {field_arr}, i32 {idx}"
                    ));
                    let val = self.fresh_temp();
                    self.emit(&format!("{val} = load {fty}, ptr {elem_ptr}"));
                    let addr = self.fresh_temp();
                    self.emit(&format!(
                        "{addr} = getelementptr inbounds i8, ptr {tmp}, i32 {}",
                        fl.offset
                    ));
                    self.emit(&format!("store {fty} {val}, ptr {addr}"));
                }
                return ("ptr".into(), tmp);
            }
        }
        // Structs are stored by reference (ptr) in arrays.
        let elem_ty = match elem_type {
            TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
            other => llvm_type_of(other, self.layouts),
        };
        let tmp = self.fresh_temp();
        self.emit(&format!(
            "{tmp} = getelementptr inbounds {elem_ty}, ptr {arr}, i32 {idx}"
        ));
        let result = self.fresh_temp();
        self.emit(&format!("{result} = load {elem_ty}, ptr {tmp}"));
        (elem_ty, result)
    }

    /// RFC 009 D3：SoA struct 数组字段访问融合。
    ///
    /// 将 `soaArr[i].field` 直接编译为：
    ///   1. `%field_arr = call ptr @rt_soa_field_ptr(ptr %arr, i32 %field_idx)`
    ///   2. `%elem_ptr = getelementptr inbounds %field_ty, ptr %field_arr, i32 %i`
    ///   3. `%val = load %field_ty, ptr %elem_ptr`
    ///
    /// 相比 AoS 回退路径（先 `arr[i]` 物化为临时 local，再 FieldGet），
    /// 此融合消除一次 struct 物化与一次 GEP，且字段访问连续利于 SIMD 向量化。
    pub(super) fn emit_soa_field_get(
        &mut self,
        array: &MirOperand,
        index: &MirOperand,
        class: &str,
        field: &str,
    ) -> TyVal {
        let (_, arr) = self.emit_operand(array);
        let (_, idx) = self.emit_operand(index);
        let (field_idx, field_ty_str) = self.soa_field_idx_ty(class, field);
        let field_ty = llvm_field_type(&field_ty_str, self.layouts);
        // 1. 取字段数组首指针
        let field_arr = self.fresh_temp();
        self.emit(&format!(
            "{field_arr} = call ptr @rt_soa_field_ptr(ptr {arr}, i32 {field_idx})"
        ));
        // 2. GEP 到第 idx 个元素
        let elem_ptr = self.fresh_temp();
        self.emit(&format!(
            "{elem_ptr} = getelementptr inbounds {field_ty}, ptr {field_arr}, i32 {idx}"
        ));
        // 045 M5 全覆盖：SoA 字段读挂 struct-path TBAA（复用同一 struct 的字段 tag）。
        // SoA 下每字段是独立连续数组，字段间天然无别名——tag 仅标注「该 struct 内
        // 此字段位置的访问」，与普通字段读同构，安全。
        let tbaa_suffix = self
            .user_struct_field_tbaa(class, field, &field_ty_str, 0)
            .map(|tag| format!(", !tbaa !{tag}"))
            .unwrap_or_default();
        // 3. load 字段值
        let result = self.fresh_temp();
        self.emit(&format!(
            "{result} = load {field_ty}, ptr {elem_ptr}{tbaa_suffix}"
        ));
        (field_ty, result)
    }

    /// Look up SoA field index + Arc field type string (`int`/`double`/…).
    ///
    /// Layout fields store type *names* (same as `struct_field_info`); must use
    /// `llvm_field_type`, not `llvm_type_of(Named(name))` (which maps `"int"` → `ptr`).
    fn soa_field_idx_ty(&self, class: &str, field: &str) -> (i32, String) {
        self.layouts
            .structs
            .get(class)
            .and_then(|layout| {
                layout
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, f)| f.name == field)
                    .map(|(i, f)| (i as i32, f.ty.to_string()))
            })
            .unwrap_or_else(|| {
                panic!(
                    "codegen: SoaFieldGet on `{class}.{field}` but SoA struct layout/field missing \
                     (typeck should have rejected)"
                )
            })
    }

    // ---- Null coalescing ----

    pub(super) fn emit_coalesce(&mut self, left: &MirOperand, right: &MirOperand) -> TyVal {
        let (lty, lval) = self.emit_operand(left);
        let (rty, rval) = self.emit_operand(right);

        // RFC 004 §值类型视图 ABI：值类型 `T? ?? T_default` —— 左为内联 `{ i1, T }`，
        // 右为 T 值。`select HasValue, Value, default`——无指针解引用（消除既有
        // 「指针装箱」下非空值的悬垂 load）。
        if let Some(inner) = nullable_aggregate_inner(&lty) {
            let has = self.fresh_temp();
            self.emit(&format!("{has} = extractvalue {lty} {lval}, 0"));
            let value = self.fresh_temp();
            self.emit(&format!("{value} = extractvalue {lty} {lval}, 1"));
            // 对齐右值到 inner（如 ConstFloat 默认 double，inner 可能是 float）
            let (_, r_coerced) = self.coerce_value(&rty, rval, inner);
            let tmp = self.fresh_temp();
            self.emit(&format!(
                "{tmp} = select i1 {has}, {inner} {value}, {inner} {r_coerced}"
            ));
            return (inner.to_string(), tmp);
        }

        // 引用类型 `T? ?? T`（lty == rty == "ptr"）走原有 select ptr 路径。
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {lval}, null"));
        let tmp = self.fresh_temp();
        self.emit(&format!(
            "{tmp} = select i1 {is_null}, ptr {rval}, ptr {lval}"
        ));
        ("ptr".into(), tmp)
    }

    // ---- Ternary conditional ----

    /// 分支操作数是否可能因解引用而触发内存错误（CD-5）。
    ///
    /// `Field`（GEP+load）/`UnboxIface`（从 fat 盒 load）经 null 指针解引用会
    /// 触发 0xC0000005；`Iface`（calloc 分配 fat 盒）即便不崩溃也有分配副作用。
    /// LLVM `select` 会急切求值两个操作数——此类分支必须发 branch+phi，
    /// 仅求值被选中的分支（C#/Arc 三元语义：`x != null ? x.F : default` 在
    /// `x == null` 时不得读 `x.F`）。纯值操作数（Local/Const/StaticField 等）
    /// 无此风险，维持 `select` 快路径。
    fn ternary_operand_may_fault(op: &MirOperand) -> bool {
        matches!(
            op,
            MirOperand::Field { .. } | MirOperand::Iface { .. } | MirOperand::UnboxIface { .. }
        )
    }

    pub(super) fn emit_ternary(
        &mut self,
        cond: &MirOperand,
        then_val: &MirOperand,
        else_val: &MirOperand,
    ) -> TyVal {
        let (cty, cval) = self.emit_operand(cond);
        // select 指令要求条件为 i1；若 MIR lower 产生非 i1 条件（如 i32 比较结果），trunc 之
        let cval_i1 = if cty != "i1" {
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = trunc {cty} {cval} to i1"));
            tmp
        } else {
            cval
        };
        // CD-5：分支含内存读取（Field/Iface/UnboxIface）时急切 select 会解引用
        // null 指针（`desc != null ? desc.Capability : default`，info1=32）。
        if Self::ternary_operand_may_fault(then_val) || Self::ternary_operand_may_fault(else_val) {
            return self.emit_ternary_branched(&cval_i1, then_val, else_val);
        }
        let (tty, tval) = self.emit_operand(then_val);
        let (ety, eval) = self.emit_operand(else_val);
        // CD-31：select 要求两分支同一 LLVM 类型——else 为 int 字面量（i32）而
        // then 为 i64（`cond ? GetPixel(...) : -1`）时须先提升，否则 clang 报
        // `'%t98' defined with type 'i32' but expected 'i64'`。与 branched 路径
        // 的 coerce_value 对齐（CD-5 同构；CD-31 是类型标注错配，非求值语义）。
        let eval_aligned = if ety != tty {
            self.coerce_value(&ety, eval, &tty).1
        } else {
            eval
        };
        let tmp = self.fresh_temp();
        self.emit(&format!(
            "{tmp} = select i1 {cval_i1}, {tty} {tval}, {tty} {eval_aligned}"
        ));
        (tty, tmp)
    }

    /// CD-5：分支可能解引用 null 时的三元发射——branch + phi 只求值选中分支。
    ///
    /// 与 `emit_null_cond_field` 同构：条件求值 → 双分支各自求值 → merge phi。
    /// 分支类型不一致（如 ptr 与 i32 装箱）时经 `coerce_value` 对齐到 then 类型，
    /// 与 select 快路径的 `{tty} {eval}` 假设一致。
    fn emit_ternary_branched(
        &mut self,
        cval_i1: &str,
        then_val: &MirOperand,
        else_val: &MirOperand,
    ) -> TyVal {
        let then_bb = self.fresh_label();
        let else_bb = self.fresh_label();
        let merge_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {cval_i1}, label %{then_bb}, label %{else_bb}"
        ));
        self.emit_label(&then_bb);
        let (tty, tval) = self.emit_operand(then_val);
        self.emit(&format!("br label %{merge_bb}"));
        self.emit_label(&else_bb);
        let (ety, eval) = self.emit_operand(else_val);
        let coerced_eval = if ety != tty {
            self.coerce_value(&ety, eval, &tty).1
        } else {
            eval
        };
        self.emit(&format!("br label %{merge_bb}"));
        self.emit_label(&merge_bb);
        let tmp = self.fresh_temp();
        self.emit(&format!(
            "{tmp} = phi {tty} [{tval}, %{then_bb}], [{coerced_eval}, %{else_bb}]"
        ));
        (tty, tmp)
    }

    // ---- Null-conditional access ----

    pub(super) fn emit_null_cond_field(
        &mut self,
        receiver: &MirOperand,
        class: &str,
        field: &str,
        default: &MirOperand,
    ) -> TyVal {
        // MIR lower 已将 `default` 归一为 `ConstNull`；内联布局下 null 分支直接
        // 构 `{ false, undef }`，无需解引用 default。
        let _ = default;
        let (_, recv) = self.emit_operand(receiver);
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {recv}, null"));
        let null_bb = self.fresh_label();
        let load_bb = self.fresh_label();
        let merge_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {is_null}, label %{null_bb}, label %{load_bb}"
        ));

        // `string?.Length` 等内置 facade 字段不能走 struct gep+load（string 是 `char*`
        // 原语，Length 由 `rt_str_length(s)` 计算，offset 16 会越界读）。
        let is_str_len = class == "string" && field == "Length";
        let (field_ty_str, offset) = if is_str_len {
            ("i32".to_string(), 0u32)
        } else {
            let (offset, field_ty) = self.field_info(class, field);
            (llvm_field_type(&field_ty, self.layouts), offset)
        };
        // `?.` 结果类型为 `T?`：值类型字段 → 内联 `{ i1, T }`；引用类型字段 → `ptr`。
        let agg = if field_ty_str == "ptr" {
            None
        } else {
            Some(format!("{{ i1, {field_ty_str} }}"))
        };

        self.emit_label(&null_bb);
        let null_val = match &agg {
            Some(a) => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = insertvalue {a} undef, i1 false, 0"));
                tmp
            }
            None => "null".to_string(),
        };
        self.emit(&format!("br label %{merge_bb}"));

        self.emit_label(&load_bb);
        let loaded = if is_str_len {
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = call i32 @rt_str_length(ptr {recv})"));
            tmp
        } else {
            let addr = self.fresh_temp();
            self.emit(&format!(
                "{addr} = getelementptr inbounds i8, ptr {recv}, i32 {offset}"
            ));
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = load {field_ty_str}, ptr {addr}"));
            tmp
        };
        let load_val = match &agg {
            Some(a) => {
                let t0 = self.fresh_temp();
                self.emit(&format!("{t0} = insertvalue {a} undef, i1 true, 0"));
                let t1 = self.fresh_temp();
                self.emit(&format!(
                    "{t1} = insertvalue {a} {t0}, {field_ty_str} {loaded}, 1"
                ));
                t1
            }
            None => loaded,
        };
        self.emit(&format!("br label %{merge_bb}"));

        self.emit_label(&merge_bb);
        let tmp = self.fresh_temp();
        match agg {
            Some(a) => {
                self.emit(&format!(
                    "{tmp} = phi {a} [{null_val}, %{null_bb}], [{load_val}, %{load_bb}]"
                ));
                (a, tmp)
            }
            None => {
                self.emit(&format!(
                    "{tmp} = phi ptr [{null_val}, %{null_bb}], [{load_val}, %{load_bb}]"
                ));
                ("ptr".into(), tmp)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_null_cond_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
        impl_class: Option<&str>,
        target_fn: Option<&str>,
        is_virtual: bool,
        params: &[String],
        default: &MirOperand,
    ) -> TyVal {
        // MIR lower 已将 `default` 归一为 `ConstNull`；内联布局下 null 分支直接
        // 构 `{ false, undef }`（方法返回类型在 call_bb 才可知，见 merge 段）。
        let _ = default;
        let (_, recv) = self.emit_operand(receiver);
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {recv}, null"));
        let null_bb = self.fresh_label();
        let call_bb = self.fresh_label();
        let merge_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {is_null}, label %{null_bb}, label %{call_bb}"
        ));
        self.emit_label(&null_bb);
        self.emit(&format!("br label %{merge_bb}"));
        self.emit_label(&call_bb);
        let (ty, call_val) = self.emit_method_call(
            receiver,
            method,
            args,
            receiver_type,
            impl_class,
            target_fn,
            is_virtual,
            params,
        );
        self.emit(&format!("br label %{merge_bb}"));
        self.emit_label(&merge_bb);

        // `?.` 结果类型为 `T?`：
        // - 引用类型方法（ty == "ptr"）：call_val 已为 ptr，phi null / call_val。
        // - 值类型方法（ty != "ptr"）：在 merge 段用 phi 分选 HasValue（i1）与
        //   Value（T），再 insertvalue 构 `{ i1, T }`——方法返回类型在 call_bb 才
        //   可知，null 分支不可预先构造聚合（Value 字段用 undef，HasValue=false
        //   时永不读取）。
        let tmp = self.fresh_temp();
        if ty == "ptr" {
            self.emit(&format!(
                "{tmp} = phi ptr [null, %{null_bb}], [{call_val}, %{call_bb}]"
            ));
            ("ptr".into(), tmp)
        } else {
            let agg = format!("{{ i1, {ty} }}");
            let has = self.fresh_temp();
            self.emit(&format!(
                "{has} = phi i1 [false, %{null_bb}], [true, %{call_bb}]"
            ));
            let val = self.fresh_temp();
            self.emit(&format!(
                "{val} = phi {ty} [undef, %{null_bb}], [{call_val}, %{call_bb}]"
            ));
            let agg0 = self.fresh_temp();
            self.emit(&format!("{agg0} = insertvalue {agg} undef, i1 {has}, 0"));
            let agg1 = self.fresh_temp();
            self.emit(&format!("{agg1} = insertvalue {agg} {agg0}, {ty} {val}, 1"));
            (agg, agg1)
        }
    }

    // ---- Force deref (!.) ----

    pub(super) fn emit_force_deref_field(
        &mut self,
        receiver: &MirOperand,
        class: &str,
        field: &str,
        span: Span,
    ) -> TyVal {
        let (_, recv) = self.emit_operand(receiver);
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {recv}, null"));
        let panic_bb = self.fresh_label();
        let ok_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {is_null}, label %{panic_bb}, label %{ok_bb}"
        ));
        self.emit_label(&panic_bb);
        let (line, col) = self.span_to_line_col(span);
        // RFC 031 §2: attach DILocation to the panic call for lldb source mapping.
        let loc_id = self.emit_dilocation(span);
        self.emit_dbg(
            &format!("call void @rt_panic_at(ptr null, ptr @__arc_file, i32 {line}, i32 {col})"),
            loc_id,
        );
        self.emit("unreachable");
        self.emit_label(&ok_bb);
        self.emit_field_get(receiver, class, field)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_force_deref_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
        impl_class: Option<&str>,
        target_fn: Option<&str>,
        is_virtual: bool,
        params: &[String],
        span: Span,
    ) -> TyVal {
        let (_, recv) = self.emit_operand(receiver);
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {recv}, null"));
        let panic_bb = self.fresh_label();
        let ok_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {is_null}, label %{panic_bb}, label %{ok_bb}"
        ));
        self.emit_label(&panic_bb);
        let (line, col) = self.span_to_line_col(span);
        // RFC 031 §2: attach DILocation to the panic call for lldb source mapping.
        let loc_id = self.emit_dilocation(span);
        self.emit_dbg(
            &format!("call void @rt_panic_at(ptr null, ptr @__arc_file, i32 {line}, i32 {col})"),
            loc_id,
        );
        self.emit("unreachable");
        self.emit_label(&ok_bb);
        self.emit_method_call(
            receiver,
            method,
            args,
            receiver_type,
            impl_class,
            target_fn,
            is_virtual,
            params,
        )
    }

    // ---- Helpers ----

    /// Get field offset and type string from class layout.
    pub fn field_info(&self, class: &str, field: &str) -> (u32, String) {
        self.layouts
            .classes
            .get(class)
            .and_then(|c| c.fields.iter().find(|f| f.name == field))
            .map(|f| (f.offset, f.ty.to_string()))
            .unwrap_or((16, "int".into()))
    }

    /// Get struct field offset and type.
    pub fn struct_field_info(&self, struct_name: &str, field: &str) -> (u32, String) {
        self.layouts
            .structs
            .get(struct_name)
            .and_then(|s| s.fields.iter().find(|f| f.name == field))
            .map(|f| (f.offset, f.ty.to_string()))
            .unwrap_or((0, "int".into()))
    }

    /// 045 M5：将 Arc 字段类型名映射为 TBAA 标量名（粗粒度——仅用于让 clang 区分
    /// 不同字段；同 struct 不同 offset 已保证互不 alias，标量名不参与 NoAlias 判定）。
    fn tbaa_scalar_name(&self, field_ty: &str) -> String {
        if self.layouts.structs.contains_key(field_ty)
            || self.layouts.classes.contains_key(field_ty)
        {
            return "ptr".to_string();
        }
        match field_ty {
            "int" | "long" | "short" | "byte" | "uint" | "ulong" | "ushort" | "sbyte" => {
                "i64".into()
            }
            "bool" => "i1".into(),
            "float" | "double" => "f64".into(),
            "char" | "string" => "ptr".into(),
            _ => "i64".into(),
        }
    }

    /// 045 M5：获取用户 struct 某字段的 TBAA 访问 tag（惰性构建整个 struct 的 TBAA
    /// 层次后查表）。返回 None 表示该 struct 无需/无法挂 TBAA。
    pub(super) fn user_struct_field_tbaa(
        &mut self,
        struct_name: &str,
        field: &str,
        _field_ty: &str,
        _offset: u32,
    ) -> Option<u32> {
        let layout = self.layouts.structs.get(struct_name)?;
        let fields: Vec<(String, String, i64)> = layout
            .fields
            .iter()
            .map(|f| {
                (
                    f.name.to_string(),
                    self.tbaa_scalar_name(f.ty.as_str()),
                    f.offset as i64,
                )
            })
            .collect();
        let ids = self.dbg.struct_tbaa(struct_name, &fields);
        ids.field_tag.get(field).copied()
    }

    /// Compute class size in bytes (header + fields).
    ///
    /// Field sizes come from `ProgramLayouts::size_of_ty` (layout SSoT).
    ///
    /// RFC 037 M-D0：带 `[Observable]` auto-property 的类额外含合成通知通道
    /// 槽区（**每属性一 `ptr`**，紧随布局末字段、按 8 字节对齐），calloc 尺寸
    /// 相应放大——与 LLVM struct 类型（`emit_struct_types` 按规范序追加 N 个
    /// `ptr`）及通道 GEP 偏移（`observable_channel_offset`）三处共用同一
    /// 规范序（`class_observable_properties`）。
    pub fn class_size(&self, class: &str) -> u64 {
        let base = self
            .layouts
            .classes
            .get(class)
            .map(|c| c.size_bytes() as u64)
            .unwrap_or(16);
        let obs_count = self.layouts.class_observable_properties(class).len() as u64;
        if obs_count > 0 {
            ((base + 7) & !7) + obs_count * 8
        } else {
            base
        }
    }

    /// Whether class has a vtable.
    pub fn class_has_vtable(&self, class: &str) -> bool {
        self.layouts
            .classes
            .get(class)
            .is_some_and(|c| c.has_vtable)
    }

    /// 返回 class 的 vtable 全局符号名。外部类（`external_class_names` 成员，
    /// 即依赖包导出面，vtable 由定义包以 linkonce_odr + COMDAT 发射）在本 TU
    /// **不发射**定义（`emit_vtables` 无条件跳过），但函数体引用点（`new T()` 的
    /// vptr 槽写入）必须拿到**真实符号**而非 `null`——否则对象 vptr 为 null，
    /// 后续虚调用立即空指针崩溃。故经 RFC 038 M2 统一守卫登记
    /// `@.vtable.{Ext} = external global` 声明（共享 ModuleEmitter.external_aggregate_refs，
    /// emit_module 末尾发射），再返回符号名供 `store ptr` 引用。
    ///
    /// 仅当类**无 vtable** 时返回 `None`（调用方跳过 vptr 槽写入）。注入的
    /// Arc.Collections 泛型模板体（如 `ObservableCollection` ctor）引用的
    /// `List_int` / `List_T` 等未单态化类不在 `layouts` 中——`class_has_vtable`
    /// 判 false 返回 `None`，发射 `null` vtable 无害（此类模板体实为死代码）。
    pub fn vtable_global(&mut self, class: &str) -> Option<String> {
        if !self.class_has_vtable(class) {
            return None;
        }
        let sym = format!("@.vtable.{class}");
        if self.external_class_names.contains(class) {
            let slots = self
                .layouts
                .classes
                .get(class)
                .map(|c| c.virtual_slots.len() + 3)
                .unwrap_or(3);
            self.external_aggregate_refs
                .entry(sym[1..].to_string())
                .or_insert_with(|| format!("[{slots} x ptr]"));
        }
        Some(sym)
    }

    /// RFC 038 M2 链接模型：typeinfo 全局符号引用守卫（FnEmitter 侧，函数体
    /// 路径）。与 `vtable_global` 同源——外部类型（`external_class_names` 成员，
    /// 即依赖包导出面，typeinfo 由 `emit_typeinfos` 跳过本 TU 发射）
    /// 登记 `@.typeinfo.{T} = external global` 声明（消费者 linkonce_odr
    /// COMDAT 定义解析），再返回符号名供引用。无 typeinfo 的类型（数组 mangle /
    /// 不在 layouts）返回 `None`，调用方发 `null`。
    pub fn typeinfo_global(&mut self, type_name: &str) -> Option<String> {
        let sym = typeinfo_symbol_core(self.layouts, type_name)?;
        if self.external_class_names.contains(type_name) {
            self.external_aggregate_refs
                .entry(sym.trim_start_matches('@').to_string())
                .or_insert_with(|| RT_TYPEINFO_LLVM_TY.to_string());
        }
        Some(sym)
    }

    /// Get vtable slot index for a virtual method (CD-10/D1：签名键)。
    /// 精确 (method, params) 优先；同名槽唯一时按名兜底。
    pub fn virtual_slot_index(&self, class: &str, method: &str, params: &[String]) -> usize {
        self.layouts
            .classes
            .get(class)
            .map(|c| {
                if let Some(idx) = c.virtual_slots.iter().position(|s| {
                    s.name.as_str() == method
                        && s.params.len() == params.len()
                        && s.params
                            .iter()
                            .zip(params.iter())
                            .all(|(a, b)| a.as_str() == b.as_str())
                }) {
                    return idx;
                }
                // CD-29 防御：签名不匹配（重载）时不得按名兜底——与 MIR
                // `is_virtual_member` 对称（虚分派判定已收紧，此处仅防御重载
                // 错位：`WriteString(string,string)` 曾经槽 3 误调单参实现）。
                if !params.is_empty() {
                    return 0;
                }
                let mut only: Option<usize> = None;
                for (i, s) in c.virtual_slots.iter().enumerate() {
                    if s.name.as_str() == method {
                        if only.is_some() {
                            return 0;
                        }
                        only = Some(i);
                    }
                }
                only.unwrap_or(0)
            })
            .unwrap_or(0)
    }

    /// Get the declared return type name of a virtual method from the vtable layout.
    /// Returns `None` if the class or method is not found.
    pub fn virtual_method_ret_name(
        &self,
        class: &str,
        method: &str,
        params: &[String],
    ) -> Option<&str> {
        let c = self.layouts.classes.get(class)?;
        if let Some(s) = c.virtual_slots.iter().find(|s| {
            s.name.as_str() == method
                && s.params.len() == params.len()
                && s.params
                    .iter()
                    .zip(params.iter())
                    .all(|(a, b)| a.as_str() == b.as_str())
        }) {
            return Some(s.ret.as_str());
        }
        let matches: Vec<&VirtualSlot> = c
            .virtual_slots
            .iter()
            .filter(|s| s.name.as_str() == method)
            .collect();
        if matches.len() == 1 {
            return Some(matches[0].ret.as_str());
        }
        None
    }

    /// Get interface method slot index (CD-12/D3：接口继承扁平化后按签名定位)。
    /// Searches methods first (exact name+params; unique-name fallback), then
    /// property getters (offset by methods.len()), then generic instantiation
    /// slots (RFC 006). Property `Name` → getter method `get_Name`.
    ///
    /// MIR 调用目标的 target_fn 对**重载方法**已按实现符号 mangle
    /// （`SpawnActor_Transform`），而 `InterfaceLayout.methods` 存裸名
    /// （`SpawnActor` + params）——裸名匹配失败且重载使「同名唯一」兜底
    /// 失效时旧实现 return 0（槽 0），itable 槽序错位调用（重载方法被
    /// 分派到错误槽 → 垃圾返回值 → AV）。此处增补
    /// mangle 等价匹配：正向构造 `mname[_param…]` 与调用点名比对，无反解
    /// 歧义。
    pub fn iface_method_index(&self, iface: &str, method: &str, params: &[String]) -> usize {
        let il = match self.layouts.interfaces.get(iface) {
            Some(il) => il,
            None => return 0,
        };
        let mangles_to = |mname: &str, mparams: &[Ident]| -> bool {
            if mparams.is_empty() {
                return mname == method;
            }
            let mut mangled = String::from(mname);
            for p in mparams {
                mangled.push('_');
                mangled.push_str(p.as_str());
            }
            mangled == method
        };
        if let Some(idx) = il.methods.iter().position(|(m, _, p)| {
            (m == method
                && p.len() == params.len()
                && p.iter()
                    .zip(params.iter())
                    .all(|(a, b)| a.as_str() == b.as_str()))
                || mangles_to(m, p)
        }) {
            return idx;
        }
        // 同名唯一时按名兜底（调用点 params 缺失/简化的兼容路径）
        let name_matches: Vec<usize> = il
            .methods
            .iter()
            .enumerate()
            .filter(|(_, (m, _, _))| m == method)
            .map(|(i, _)| i)
            .collect();
        if name_matches.len() == 1 {
            return name_matches[0];
        }
        let prop_name = method.strip_prefix("get_").unwrap_or(method);
        let method_count = il.methods.len();
        if let Some(idx) = il.properties.iter().position(|(p, _)| p == prop_name) {
            return method_count + idx;
        }
        // RFC 006：泛型方法实例化槽位（如 "Get__Seed"）
        let prop_count = il.properties.len();
        if let Some(idx) = il.generic_instances.iter().position(|m| m == method) {
            return method_count + prop_count + idx;
        }
        0
    }
}
