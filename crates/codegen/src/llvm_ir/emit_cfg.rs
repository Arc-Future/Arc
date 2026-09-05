//! CFG block and statement emission for FnEmitter.
//!
//! Contains methods for emitting CFG blocks, statements, terminators,
//! and nested control-flow (if/while/try-catch) bodies.

use super::*;
use crate::llvm_ir::types::is_generic_template_name;
use ast::TypeId;
use mir::{MirBlock, MirOperand, MirRvalue, MirStatement, MirTerminator};

impl<'a> FnEmitter<'a> {
    /// When a variant or struct type returns `null` (from `default(T)` or
    /// `ConstNull`), materialize a zero-initialized alloca and return its
    /// pointer instead of the literal `null`.  This prevents null-pointer
    /// dereference crashes when the caller tries to read the struct's fields.
    fn materialize_null_return(
        &mut self,
        place_ty: &TypeId,
        ty: &str,
        val: String,
    ) -> (String, String) {
        if val != "null" || ty != "ptr" {
            return (ty.to_string(), val);
        }
        let name = match place_ty {
            TypeId::Named(n) => n.as_str(),
            _ => return (ty.to_string(), val),
        };
        if self.layouts.variants.contains_key(&ast::Ident::from(name)) {
            let variant_ty = format!("%variant.{name}");
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = alloca {variant_ty}"));
            self.emit(&format!("store {variant_ty} zeroinitializer, ptr {tmp}"));
            ("ptr".into(), tmp)
        } else if self.layouts.structs.contains_key(&ast::Ident::from(name)) {
            // 与 emit_new(struct) 一致：返回值必须堆分配，避免 ret 栈 alloca 悬空。
            let size = self.layouts.size_of_ty(name) as i64;
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = call ptr @calloc(i64 1, i64 {size})"));
            ("ptr".into(), tmp)
        } else {
            (ty.to_string(), val)
        }
    }

    /// 接口类型返回值堆化：把 `{obj, itable}` 胖指针（可能指向本帧栈 alloca）
    /// 复制到 calloc 堆块；`null` 时保持 `ptr null`（null 接口值）。
    ///
    /// `ret` 后本帧弹出，栈上胖指针会被后续调用覆盖，调用方按胖指针解引用
    /// 会读到垃圾 → ACCESS_VIOLATION。与 struct 返回堆化先例一致
    /// （见 `materialize_null_return` 注释）。
    fn emit_iface_ret_heap_copy(&mut self, src: &str) -> (String, String) {
        let fat = self.fresh_temp();
        self.emit(&format!("{fat} = call ptr @calloc(i64 1, i64 16)"));
        let isnull = self.fresh_temp();
        self.emit(&format!("{isnull} = icmp eq ptr {src}, null"));
        let copy_bb = self.fresh_label();
        let null_bb = self.fresh_label();
        let join = self.fresh_label();
        self.emit(&format!(
            "br i1 {isnull}, label %{null_bb}, label %{copy_bb}"
        ));
        self.emit(&format!("{copy_bb}:"));
        let oa = self.fresh_temp();
        self.emit(&format!(
            "{oa} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 0"
        ));
        let soa = self.fresh_temp();
        self.emit(&format!(
            "{soa} = getelementptr inbounds {{ ptr, ptr }}, ptr {src}, i32 0, i32 0"
        ));
        let obj = self.fresh_temp();
        self.emit(&format!("{obj} = load ptr, ptr {soa}"));
        self.emit(&format!("store ptr {obj}, ptr {oa}"));
        let va = self.fresh_temp();
        self.emit(&format!(
            "{va} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 1"
        ));
        let sva = self.fresh_temp();
        self.emit(&format!(
            "{sva} = getelementptr inbounds {{ ptr, ptr }}, ptr {src}, i32 0, i32 1"
        ));
        let it = self.fresh_temp();
        self.emit(&format!("{it} = load ptr, ptr {sva}"));
        self.emit(&format!("store ptr {it}, ptr {va}"));
        self.emit(&format!("br label %{join}"));
        self.emit(&format!("{null_bb}:"));
        self.emit(&format!("br label %{join}"));
        self.emit(&format!("{join}:"));
        let result = self.fresh_temp();
        self.emit(&format!(
            "{result} = phi ptr [ {fat}, %{copy_bb} ], [ null, %{null_bb} ]"
        ));
        ("ptr".into(), result)
    }

    /// Variant 类型返回值堆化（RFC 004 D3 清偿）：variant 构造于**当前函数栈帧**
    /// （`emit_variant_construct` alloca），`ret` 后本帧弹出，返回的 variant 指针
    /// 悬垂——调用方后续回读 tag/payload 读到被复用栈 → 坏数据
    /// （`SetterValueHelper.ResourceToSetter` 返回 `SetterValue.String(...)` 后
    /// StyleEvaluator 读空串，`ui_style_apply_e2e` fail:static 根因）。
    ///
    /// 与 FieldSet / 列表元素存 variant 的 `emit_variant_deep_copy` 同构：返回前
    /// 把 variant 字节复制到独立堆块；null 源（null variant 返回值）保持 null。
    /// 复用先例：接口胖指针返回堆化 `emit_iface_ret_heap_copy`、struct 返回堆化
    /// `materialize_null_return`。
    fn heap_copy_variant_return(
        &mut self,
        ret_ty: &TypeId,
        ty: &str,
        val: String,
    ) -> (String, String) {
        if ty == "ptr" {
            if let TypeId::Named(vname) = ret_ty {
                if self.layouts.variants.contains_key(vname.as_str()) {
                    let heap = self.emit_variant_deep_copy(vname.as_str(), &val);
                    return ("ptr".into(), heap);
                }
            }
        }
        (ty.to_string(), val)
    }
    // ---- CFG block emission ----

    pub(super) fn emit_cfg_block(&mut self, block: &MirBlock) {
        // Block label
        self.output.push_str(&format!("bb{}:\n", block.id.0));
        // RFC 039 M3：记录当前块 ID，供 `emit_terminator` 判定是否为循环 backedge。
        self.current_block_id = block.id;

        // RFC 005：若本块是纯追加循环的出口，先把 shadow 一次性 flush 回堆头
        // （在块语句前，循环后的代码读到最新的头字段）。shadow 从 `sb_shadow_map`
        // 按体块取用，与发射顺序解耦。
        if let Some(plan) = self.sb_promotes.iter().find(|p| p.exit == block.id) {
            if let Some(sh) = self.sb_shadow_map.get(&plan.body).cloned() {
                self.emit_sb_shadow_flush(&sh);
            }
            self.sb_shadow = None;
        }
        // RFC 005：若本块是纯追加体，激活 shadow（`emit_sb_append_char_inline`
        // 据此改读写 shadow alloca）。`to_cfg` 分配块 id 时出口常在体前，跨块
        // 瞬态 `sb_shadow` 会被提前取走，故此处按体块重新建立。
        if let Some(sh) = self.sb_shadow_map.get(&block.id).cloned() {
            self.sb_shadow = Some(sh);
        }

        // Statements（记录 stmt 下标，供 M2 整图 CFG 的 await 位点索引）
        for (stmt_idx, stmt) in block.statements.iter().enumerate() {
            self.current_stmt_index = stmt_idx;
            self.stmt_path = vec![stmt_idx];
            self.emit_stmt(stmt);
        }
        self.stmt_path.clear();

        // RFC 005：体块发射完毕即撤销 shadow，避免状态泄漏到随后发射的
        // 循环外代码（其 Append 不得误用 stale shadow）。
        if self.sb_shadow_map.contains_key(&block.id) {
            self.sb_shadow = None;
        }
        // RFC 005：若本块是可提升纯追加循环的前置头，在此建 shadow alloca 并
        // 初始化（在块语句后、`Goto(header)` 前），确保捕获进入循环前最新的
        // 堆头状态，并按体块登记供其取用。
        if let Some(plan) = self.sb_promotes.iter().find(|p| p.preheader == block.id) {
            let (receiver, body) = (plan.receiver, plan.body);
            if let Some(sh) = self.emit_sb_shadow_preheader(receiver) {
                self.sb_shadow_map.insert(body, sh);
            }
        }

        // Terminator
        self.emit_terminator(&block.terminator);
    }

    pub fn emit_label(&mut self, label: &str) {
        self.output.push_str(label);
        self.output.push_str(":\n");
    }

    /// RFC 004 可空视图：把 rvalue 物化为内联 `{ i1, T }`（`int → int?`）。
    ///
    /// 仅当 `place_ty` 为值类型 `Nullable<T>`（T 基元值类型）且 rvalue 尚未是内联
    /// 聚合时生效；`null` 字面量（`Use(ConstNull)`）→ `{ false, undef }`，其余值
    /// coerce 到内层 LLVM 类型后 `{ true, value }`。引用类型 `T?`（string? 等）与
    /// 非 Nullable 左值原样透传。
    fn materialize_nullable_assign(
        &mut self,
        rvalue: &MirRvalue,
        place_ty: &TypeId,
        from_ty: &str,
        from_val: &str,
    ) -> (String, String) {
        let TypeId::Nullable { inner } = place_ty else {
            return (from_ty.to_string(), from_val.to_string());
        };
        let Some(agg_ty) = nullable_value_llvm_type(inner, self.layouts) else {
            return (from_ty.to_string(), from_val.to_string());
        };
        // 已是聚合（如 `int? b = a` / `int? len = s?.Length`）→ 原样透传。
        if from_ty == agg_ty.as_str() {
            return (from_ty.to_string(), from_val.to_string());
        }
        let inner_llvm = primitive_value_storage_llvm_type(inner).to_string();
        let is_null = matches!(rvalue, MirRvalue::Use(MirOperand::ConstNull));
        let (val_ty, val) = if is_null {
            (inner_llvm, "undef".to_string())
        } else {
            self.coerce_value(from_ty, from_val.to_string(), &inner_llvm)
        };
        let has = if is_null { "false" } else { "true" };
        let t0 = self.fresh_temp();
        self.emit(&format!("{t0} = insertvalue {agg_ty} undef, i1 {has}, 0"));
        let t1 = self.fresh_temp();
        self.emit(&format!(
            "{t1} = insertvalue {agg_ty} {t0}, {val_ty} {val}, 1"
        ));
        (agg_ty, t1)
    }

    // ---- Statement emission ----

    pub(crate) fn emit_stmt(&mut self, stmt: &MirStatement) {
        match stmt {
            MirStatement::Assign { place, rvalue } => {
                // RFC 008: track locals that hold `arc_closure*` (vs bare FnPtr).
                // Cross-boundary Func values (params / List elements / fields /
                // call returns) are unified as `arc_closure*` by
                // `emit_operand_as_closure`; local no-capture assigns stay bare
                // FnPtr for zero-overhead direct calls. Missing this mark made
                // `Signal.TrySet` invoke a list-loaded handler as a raw fn ptr
                // → 0xC0000005 (GEP/call on heap closure object).
                let place_ty = self.local_type(*place);
                if is_delegate_type(&place_ty) {
                    match rvalue {
                        MirRvalue::Use(MirOperand::Closure { .. }) => {
                            self.closure_locals.insert(*place);
                        }
                        // Method-group / no-capture lambda：裸 FnPtr 存储，本地直调。
                        MirRvalue::FnPtr { .. } | MirRvalue::Use(MirOperand::FnPtr { .. }) => {
                            self.closure_locals.remove(place);
                        }
                        MirRvalue::Use(MirOperand::Local(src)) => {
                            if self.closure_locals.contains(src) {
                                self.closure_locals.insert(*place);
                            } else {
                                self.closure_locals.remove(place);
                            }
                        }
                        _ => {
                            // List/字段/形参/调用返回等跨边界 Func → arc_closure*
                            self.closure_locals.insert(*place);
                        }
                    }
                } else if matches!(rvalue, MirRvalue::Use(MirOperand::Closure { .. })) {
                    self.closure_locals.insert(*place);
                }
                if matches!(place_ty, TypeId::Void) {
                    // Void rvalue (e.g. Console.WriteLine) — emit as void call
                    self.emit_rvalue_typed(rvalue, &place_ty);
                    return;
                }
                // For ref params, unwrap to inner type for rvalue evaluation
                let effective_ty = match &place_ty {
                    TypeId::Ref { inner, .. } => inner.as_ref().clone(),
                    _ => place_ty.clone(),
                };
                let (ty, val) = self.emit_rvalue_typed(rvalue, &effective_ty);
                if ty != "void" {
                    // RFC 004 可空视图：`int → int?` 值物化——左值类型为值类型
                    // `Nullable<T>`（T 基元值类型）且 rvalue 尚未是内联聚合时，包一层
                    // `{ HasValue, Value }`；`null` 字面量 → `{ false, undef }`。
                    // 引用类型 `T?` 保持 ptr，不在此物化（见 materialize_nullable_assign）。
                    let (ty, val) =
                        self.materialize_nullable_assign(rvalue, &effective_ty, &ty, &val);
                    let place_llvm_ty = llvm_type_of(&effective_ty, self.layouts);
                    let (store_ty, store_val) = self.coerce_value(&ty, val, &place_llvm_ty);
                    if let TypeId::Ref { inner, .. } = &place_ty {
                        // Ref param: store through the pointer stored in the slot
                        let slot_ptr = self.local_ptr(*place);
                        let deref_ptr = self.fresh_temp();
                        self.emit(&format!("{deref_ptr} = load ptr, ptr {slot_ptr}"));
                        // ARC：经 ref/out 参数写入 class 引用——目标槽须持独立 +1
                        // （调用方本地会在其 epilogue dec），与 `items = list` 中
                        // 局部 list 在函数退出 dec 配对。缺 retain 时 list（rc=1）
                        // 被退出 dec 释放，目标槽悬垂 → 后续 inc UAF → 0xC0000374
                        // （AIToolArgsReader.ClassifyArray 实测）。槽位零初始化，
                        // 首次写入 dec(null) 为 no-op；out 参数语义不变。
                        if Self::arc_class_place(inner, self.layouts) {
                            self.emit(&format!("call void @rt_arc_inc(ptr {store_val})"));
                            let old = self.fresh_temp();
                            self.emit(&format!("{old} = load ptr, ptr {deref_ptr}"));
                            self.emit(&format!("store {store_ty} {store_val}, ptr {deref_ptr}"));
                            self.emit(&format!("call void @rt_arc_dec(ptr {old})"));
                        } else {
                            // RFC 005 自动 Copy：ref 写入目标槽须落独立副本
                            //（指针替换会让被引用变量别名源 storage）。
                            let copied = match &**inner {
                                TypeId::Named(n) => self.try_emit_copy_struct_store(
                                    n.as_str(),
                                    &store_val,
                                    &deref_ptr,
                                    None,
                                    true,
                                ),
                                _ => false,
                            };
                            if !copied {
                                self.emit(&format!(
                                    "store {store_ty} {store_val}, ptr {deref_ptr}"
                                ));
                            }
                        }
                    } else {
                        let ptr = self.local_ptr(*place);
                        // ByRef 捕获局部赋值（变量捕获写回）：本地 alloca 存外层变量
                        // 槽地址，赋值经槽 store——`invokedName = c.Name` 才能传播到
                        // 外层（C# 闭包语义）。ARC 覆写（inc 新/dec 旧）与普通局部
                        // 对称；string 无 ArcHeader 跳过。
                        let is_byref_capture = self.cfg.captures.iter().any(|(cid, _, c)| {
                            *cid == *place && matches!(c.mode, ast::CaptureMode::ByRef)
                        });
                        if is_byref_capture {
                            let slot = self.fresh_temp();
                            self.emit(&format!("{slot} = load ptr, ptr {ptr}"));
                            if Self::arc_class_place(&effective_ty, self.layouts) {
                                if Self::assign_needs_arc_retain(
                                    rvalue,
                                    &effective_ty,
                                    self.layouts,
                                ) {
                                    self.emit(&format!("call void @rt_arc_inc(ptr {store_val})"));
                                }
                                let old = self.fresh_temp();
                                self.emit(&format!("{old} = load ptr, ptr {slot}"));
                                self.emit(&format!("store {store_ty} {store_val}, ptr {slot}"));
                                self.emit(&format!("call void @rt_arc_dec(ptr {old})"));
                            } else {
                                // RFC 005 自动 Copy：ByRef 写回同样落独立副本。
                                let copied = match &effective_ty {
                                    TypeId::Named(n) => self.try_emit_copy_struct_store(
                                        n.as_str(),
                                        &store_val,
                                        &slot,
                                        None,
                                        true,
                                    ),
                                    _ => false,
                                };
                                if !copied {
                                    self.emit(&format!("store {store_ty} {store_val}, ptr {slot}"));
                                }
                            }
                        } else {
                            // RFC 016：env 唯一 owner 的 class 局部（跨 await 存活、在 env、
                            // 非捕获、arc_class_place）——赋值**写穿到 env 字段**（唯一 owner），
                            // alloca 仅作镜像，由 dtor + EH cleanup pad 释放恰一次。否则任何
                            // unwind（EH cleanup pad 触发的 dtor）会读到 stale 的 env 旧值 →
                            // double-dec / UAF。
                            if self.is_env_owned_class_local(*place) {
                                self.emit_env_owned_class_assign(
                                    *place,
                                    &store_ty,
                                    &store_val,
                                    rvalue,
                                    &effective_ty,
                                );
                            } else {
                                // ARC：局部 ← 另一局部/字段的 class 拷贝须 retain。
                                // `new`/`typeof`/Call 等移交所有权路径不 inc（生产者已 rc=1）。
                                // 缺 retain 时 `Type a = b` 双 Drop → 堆损坏（RuntimeType/
                                // MemberInfo 字段 walk 放大为 0xC0000374）。
                                // string 不含：常量串常为 rodata，无 ArcHeader。
                                if Self::arc_class_place(&effective_ty, self.layouts) {
                                    // 刀 2.2 跨块 ARC：dead-copy 局部（从不读取、仅拷贝赋值）
                                    // 整对消除——跳过 inc(新) 与 dec(旧)；epilogue dec 一并跳过
                                    //（emit_sync_epilogue_drops）。被引用对象仍由源持有，引用
                                    // 计数净变化为零。保留 store（槽仅作镜像，LLVM 消去死 store）。
                                    if self.dead_arc_locals.contains(place) {
                                        self.emit(&format!(
                                            "store {store_ty} {store_val}, ptr {ptr}"
                                        ));
                                    } else {
                                        // inc 新值（仅拷贝语义 rvalue；`new`/Call 移交所有权不 inc）。
                                        // 必须 inc-before-dec：`x = x` 自赋值时先 dec 会使 rc 归零释放。
                                        if Self::assign_needs_arc_retain(
                                            rvalue,
                                            &effective_ty,
                                            self.layouts,
                                        ) {
                                            self.emit(&format!(
                                                "call void @rt_arc_inc(ptr {store_val})"
                                            ));
                                        }
                                        // load 旧值 → store 新值 → dec 旧值（ARC 覆写语义）。
                                        // 旧槽位 entry 块零初始化为 null，首次赋值 dec(null) 为 no-op。
                                        // 与 FieldSet/IndexSet 的 inc/load/store/dec 对称，平衡函数
                                        // 末尾 epilogue drop。不含 opaque handle（Thread/Lock/…）：
                                        // 无 ArcHeader，rt_arc_dec 会误读首字段为 refcount。
                                        let old = self.fresh_temp();
                                        self.emit(&format!("{old} = load ptr, ptr {ptr}"));
                                        self.emit(&format!(
                                            "store {store_ty} {store_val}, ptr {ptr}"
                                        ));
                                        self.emit(&format!("call void @rt_arc_dec(ptr {old})"));
                                    }
                                } else if let TypeId::Named(n) = &effective_ty {
                                    // RFC 005 自动 Copy：Copy 型 struct 赋值 = 私有副本聚合
                                    // 拷贝 + 槽存新 ptr（指针替换会让源/目标别名共享 storage）。
                                    if !self.try_emit_copy_struct_store(
                                        n.as_str(),
                                        &store_val,
                                        &ptr,
                                        None,
                                        true,
                                    ) {
                                        self.emit(&format!(
                                            "store {store_ty} {store_val}, ptr {ptr}"
                                        ));
                                    }
                                } else {
                                    self.emit(&format!("store {store_ty} {store_val}, ptr {ptr}"));
                                }
                            } // end env-owned-class-local else (普通局部路径)
                        } // end is_byref_capture else (普通局部路径)
                    }
                }
            }
            MirStatement::Drop(id) => {
                // Sync functions: skip class-local drops — `emit_sync_epilogue_drops`
                // handles them on every return path. `mir::lower` appends these
                // Drops after the body's Return; `to_cfg` puts them in a post-Return
                // block that the codegen still emits (all blocks are iterated).
                // Without this skip, the drops execute twice (once here, once in the
                // epilogue) → double-dec → heap corruption (0xC0000374).
                if !self.cfg.is_async {
                    let ty = self.local_type(*id);
                    if Self::arc_class_place(&ty, self.layouts)
                        || matches!(ty, TypeId::Named(n) if self.layouts.classes.contains_key(n.as_str()))
                    {
                        return;
                    }
                }
                // RFC 016（SM / M2 状态机）：仅跳过 **env 唯一 owner** 的 class 局部
                // 的 Drop——其所有权由 env 字段持有，统一由 dtor + EH cleanup pad 释放。
                // **未存活 / 未提升**的 class 局部不在 env（零 save/load/配对），其
                // alloca 仍持所有权，Drop 必须正常执行（emit_drop 释放），否则泄漏。
                // M1 async（is_async 且非 SM）无 save_locals/dtor 路径，保持 emit_drop。
                if self.cfg.is_async && self.in_state_machine && self.is_env_owned_class_local(*id)
                {
                    return;
                }
                // RFC 009 I1：协程路径——跨 await 存活的 class 局部所有权由
                // 帧槽持有（C11 覆写配对），cleanup 路径统一 dec；body 内
                // Drop 跳过，否则双重释放。
                if self.cfg.is_async && self.in_coroutine && self.is_coro_owned_class_local(*id) {
                    return;
                }
                // Recursive drop sequence lives in `arc_drop.rs`.
                self.emit_drop(*id);
            }
            MirStatement::FieldSet {
                object,
                class,
                field,
                value,
            } => {
                let (offset, field_ty) = if self.layouts.structs.contains_key(class.as_str()) {
                    self.struct_field_info(class, field)
                } else {
                    self.field_info(class, field)
                };
                let (_, obj) = self.emit_operand(object);
                // C6：委托字段跨边界统一为 `arc_closure*`（与调用点/参数/列表一致）。
                // 无捕获 lambda 以裸 FnPtr 存储（`lower_lambda_to_fnptr`），若直接存
                // 裸函数指针，后续按 `%arc_closure` GEP 调用会解引用函数代码 → AV。
                // 此处把 `Use(FnPtr)` 包装为 heap `arc_closure{fn,null}`，与
                // `emit_operand_as_closure` 的字段跨界语义对齐。
                let (vty, vval) = if field_ty == "Action"
                    || field_ty == "Func"
                    || field_ty.starts_with("Action_")
                    || field_ty.starts_with("Func_")
                {
                    match value {
                        MirRvalue::Use(MirOperand::FnPtr { name }) => {
                            self.emit_operand_as_closure(&MirOperand::FnPtr { name: name.clone() })
                        }
                        _ => self.emit_rvalue(value),
                    }
                } else {
                    self.emit_rvalue(value)
                };
                let addr = self.fresh_temp();
                self.emit(&format!(
                    "{addr} = getelementptr inbounds i8, ptr {obj}, i32 {offset}"
                ));
                // RFC 016 M3 §3.3 FFI：rvalue 类型与字段声明类型不匹配时（如
                // MIR 丢弃 Cast 后 i64 long → NativePtr ptr 字段），调用
                // coerce_value 发射 inttoptr/ptrtoint 转换指令。
                let field_llvm_ty = llvm_field_type(&field_ty, self.layouts);
                let (store_ty, store_val) = if vty == field_llvm_ty {
                    (vty, vval)
                } else {
                    self.coerce_value(&vty, vval, &field_llvm_ty)
                };
                // RFC 037 M-D0：`[Observable]` auto-property 通知合成——
                // 相等性短路 + backing field 写入 + 隐藏通道（Signal<T>）
                // 惰性创建并通知。仅 class 实例字段插桩；struct 不合成
                //（值类型复制语义；RFC §5.3 仅 auto-property 插桩）。
                if !self.layouts.structs.contains_key(class.as_str())
                    && self.layouts.has_observable_property(class, field)
                {
                    self.emit_observable_property_set(
                        &obj, &field_ty, &store_ty, &store_val, &addr, class, field,
                    );
                } else {
                    // 045 M5：用户 struct 字段写挂 struct-path TBAA（与读路径
                    // `emit_field_get` 对称）。仅 struct 值类型；class 字段（ARC
                    // 维护路径）不挂。
                    let tbaa = if self.layouts.structs.contains_key(class.as_str()) {
                        self.user_struct_field_tbaa(class, field, &field_ty, offset)
                    } else {
                        None
                    };
                    self.emit_field_store(&field_ty, &store_ty, &store_val, &addr, tbaa);
                }
            }
            // RFC 006 M4：静态字段写入——store 到 `@__static_<class>_<field>` 全局变量。
            // 与 FieldSet 对偶：后者通过 `this` 指针 GEP store 到实例字段，
            // 本变体直接 store 到模块级全局变量，无需 GEP。
            // ARC 维护：类类型字段（如 `static Dictionary<...> _cache`）需 inc/dec，
            // 但当前 M4 仅支持基元类型 + null 的初始化器（emit_static_init_expr），
            // 运行时 `StaticFieldSet` 主要用于基元类型累加（如 `_count = _count + 1`），
            // 类类型静态字段写入延后 M5+ 处理（需 ARC 维护逻辑）。
            MirStatement::StaticFieldSet {
                class,
                field,
                value,
            } => {
                let global = format!("@__static_{class}_{field}");
                let (vty, vval) = self.emit_rvalue(value);
                // 字段类型从 layouts.static_fields 查询，与 emit_operand 中
                // `MirOperand::StaticField` load 路径一致。
                let field_ty_str = self
                    .layouts
                    .static_fields
                    .iter()
                    .find(|s| {
                        s.class.as_str() == class.as_str() && s.field.as_str() == field.as_str()
                    })
                    .map(|sf| {
                        let field_ty = self.static_field_type_id(&sf.ty);
                        llvm_type_of(&field_ty, self.layouts)
                    })
                    .unwrap_or_else(|| vty.clone());
                // 类型不匹配时强制 coerce（与 FieldSet 一致）
                let (store_ty, store_val) = if vty == field_ty_str {
                    (vty, vval)
                } else {
                    self.coerce_value(&vty, vval, &field_ty_str)
                };
                // ARC：类类型静态字段写入须 inc 新值 / dec 旧值（与 FieldSet 对齐）。
                // 此前类静态字段裸 store——写入方局部在出口 dec 会把 rc=1 对象提前
                // 释放，静态槽悬垂 → 后续读/用 UAF（同借引用根因）。基元/string/
                // opaque 句柄跳过。
                let static_ty = self
                    .layouts
                    .static_fields
                    .iter()
                    .find(|s| {
                        s.class.as_str() == class.as_str() && s.field.as_str() == field.as_str()
                    })
                    .map(|sf| self.static_field_type_id(&sf.ty))
                    .unwrap_or(TypeId::Void);
                if Self::arc_class_place(&static_ty, self.layouts) {
                    self.emit(&format!("call void @rt_arc_inc(ptr {store_val})"));
                    let old = self.fresh_temp();
                    self.emit(&format!("{old} = load ptr, ptr {global}"));
                    self.emit(&format!("store {store_ty} {store_val}, ptr {global}"));
                    self.emit(&format!("call void @rt_arc_dec(ptr {old})"));
                } else {
                    self.emit(&format!("store {store_ty} {store_val}, ptr {global}"));
                }
            }
            MirStatement::IndexSet {
                array,
                index,
                elem_type,
                value,
            } => {
                // RFC 005：Span 索引写（仅可变 Span；Local 或 Field class=Span）。
                if self.operand_is_mutable_span(array) {
                    self.emit_span_index_set(array, index, elem_type, value);
                    return;
                }
                let (_, arr) = self.emit_operand(array);
                let (_, idx) = self.emit_operand(index);
                // M6.1：以 elem_type 作为 expected 发射 RHS——数组元素赋值的 RHS 类型
                // 必须走 typed 分派。此前误用未 typed 的 emit_rvalue（expected 默认 Int）：
                // `tasks[i] = Task.Run<int>(...)` 在 try_emit_task_static 中因 expected 非
                // Task 落入「语句级 Action」回退 → rt_task_run（Action，结果恒 0）→
                // Task.Run<T> 结果丢失（Executor_ThreadPool_RunFanout_NoLostWakeup 根因）。
                let (vty, vval) = self.emit_rvalue_typed(value, elem_type);
                let elem_ty = match elem_type {
                    TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
                    other => llvm_type_of(other, self.layouts),
                };
                let addr = self.fresh_temp();
                self.emit(&format!(
                    "{addr} = getelementptr inbounds {elem_ty}, ptr {arr}, i32 {idx}"
                ));
                let (store_ty, store_val) = if vty == elem_ty {
                    (vty, vval)
                } else {
                    self.coerce_value(&vty, vval, &elem_ty)
                };
                // RFC 004 生命周期（D3）：variant 元素值深拷贝到堆（同 FieldSet）。
                let store_val = if let TypeId::Named(name) = elem_type {
                    if self.layouts.variants.contains_key(name.as_str()) && store_ty == "ptr" {
                        self.emit_variant_deep_copy(name.as_str(), &store_val)
                    } else {
                        store_val
                    }
                } else {
                    store_val
                };
                // 类类型元素：inc 新值、load 旧值、store、dec 旧值（与 FieldSet 对齐）。
                if let TypeId::Named(name) = elem_type {
                    if self.layouts.classes.contains_key(name.as_str())
                        && !is_opaque_runtime_handle(name.as_str())
                    {
                        self.emit(&format!("call void @rt_arc_inc(ptr {store_val})"));
                        let old = self.fresh_temp();
                        self.emit(&format!("{old} = load ptr, ptr {addr}"));
                        self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
                        self.emit(&format!("call void @rt_arc_dec(ptr {old})"));
                    } else {
                        self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
                    }
                } else {
                    self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
                }
            }
            MirStatement::Throw { value } => {
                let (_, val) = self.emit_rvalue(value);
                // A1 / P1-B2：try body（将被同层 catch 接住）内不先跑 finally；
                // catch 内 rethrow / 无 catch 的 try-finally 仍走 finally_chain。
                // Zero-cost EH M3 (Windows SEH)：try-finally 的 finally 由 cleanup
                // funclet 在 unwind 时执行——若这里再 inline 执行会双跑。Windows
                // 上所有 finally 均为 funclet，因此一律跳过 inline 链。
                if !self.emitting_caught_try_body && !self.is_windows {
                    self.emit_finally_chain();
                }
                // L2：首次 throw 时写入 Exception.StackTrace（仅当槽位仍为 null）。
                self.emit_attach_exception_stacktrace(&val);
                // Zero-cost EH milestone ②：rt_throw 恒 may-throw。Windows 上若在
                // try region 内发 invoke（异常落入本区域 catchswitch）；否则 plain
                // call（异常沿 .pdata 向调用方传播）。POSIX Itanium 属里程碑⑨。
                self.emit_call_may_throw("void", "@rt_throw", &format!("ptr {val}"), true, None);
                self.emit("unreachable");
                self.flow_terminated = true;
            }
            MirStatement::Await { place, task } => {
                // M2 整图 CFG：resume 内就地 suspend/resume（多块链 / 循环内 / 单臂均覆盖）。
                if self.in_state_machine {
                    // RFC 004 里程碑⑦：await 可位于 try/catch/if/while/linq 嵌套 body 内，
                    // key 采用块下标 + 嵌套路径（`stmt_path`），与 collect_await_sites 的
                    // 递归遍历一致；try-finally 的 finally body 不索引（funclet 双发射不支持）。
                    let key = (self.current_block_id.0, self.stmt_path.clone());
                    let await_idx = match self.sm_await_index.get(&key) {
                        Some(&i) => i,
                        None => {
                            let mut keys: Vec<&(u32, Vec<usize>)> =
                                self.sm_await_index.keys().collect();
                            keys.sort_by_key(|k| k.0);
                            panic!(
                                "await site not indexed for M2 state machine: key block={} path={:?}; \
                                 indexed keys: {:?}",
                                key.0, key.1, keys
                            );
                        }
                    };
                    self.emit_sm_await_site(*place, task, await_idx);
                    return;
                }
                // RFC 009 I1：协程路径——coro.save/suspend 边界（帧槽由
                // CoroSplit 自动提升，无 env save/load）。
                if self.in_coroutine {
                    self.emit_coro_await(*place, task);
                    return;
                }
                // M1 同步路径：poll until ready（无真实 suspend）。
                // 稳定性修复：根据 place_ty 分派 rt_task_result_int/ptr/value，
                // 避免 string/value 类型结果读取崩溃。
                // M4 mitigation：无 await 的 async 或未走 M2 时保留；每次 poll 间隙
                // 调用 rt_event_loop_pump，避免定时器永不触发。
                let place_ty = self.local_type(*place);
                // task 的 expected 类型是 Task<place_ty>，使 Task.FromResult 等
                // facade 静态方法能被 try_emit_task_static 正确拦截（按 inner 分派 ABI）。
                let task_expected = TypeId::Task {
                    inner: Box::new(place_ty.clone()),
                };
                let (_, task_val) = self.emit_rvalue_typed(task, &task_expected);
                let poll_label = self.fresh_label();
                let done_label = self.fresh_label();
                self.emit(&format!("br label %{poll_label}"));
                self.emit_label(&poll_label);
                // 驱动 EventLoop：处理就绪队列 + 到期定时器，让 Task.Delay / CTS.CancelAfter 得以推进。
                let loop_ptr = self.fresh_temp();
                self.emit(&format!("{loop_ptr} = call ptr @rt_event_loop_current()"));
                self.emit(&format!("call void @rt_event_loop_pump(ptr {loop_ptr})"));
                let status = self.fresh_temp();
                self.emit(&format!(
                    "{status} = call i32 @rt_task_poll(ptr {task_val})"
                ));
                let pending = self.fresh_temp();
                self.emit(&format!("{pending} = icmp eq i32 {status}, 1"));
                self.emit(&format!(
                    "br i1 {pending}, label %{poll_label}, label %{done_label}"
                ));
                self.emit_label(&done_label);
                // Zero-cost EH milestone ⑦ (async 协作 · §2.5): faulted Task 的
                // 异常在 await 提取点 rethrow（try 区域内发 invoke 落入本区域
                // catchswitch/cleanup funclet）。
                let fault_label = self.fresh_label();
                let extract_label = self.fresh_label();
                let faulted = self.fresh_temp();
                self.emit(&format!(
                    "{faulted} = call i32 @rt_task_is_faulted(ptr {task_val})"
                ));
                let faulted_b = self.fresh_temp();
                self.emit(&format!("{faulted_b} = icmp ne i32 {faulted}, 0"));
                self.emit(&format!(
                    "br i1 {faulted_b}, label %{fault_label}, label %{extract_label}"
                ));
                self.emit_label(&fault_label);
                let exc = self.fresh_temp();
                self.emit(&format!(
                    "{exc} = call ptr @rt_task_get_exception(ptr {task_val})"
                ));
                // RFC 016 子项 M2：异常所有权统一转移。Task 持唯一引用（rt_task_fault
                // 转移 throw 在途 +1），rt_task_release 对 FAULTED dec ptr_result。
                // 必须先 inc（授予 await/catch 独立副本）再 release（归还 Task 所有权）：
                // 顺序颠倒会在 release dec→0 后对已释放异常 UAF。
                self.emit(&format!("call void @rt_arc_inc(ptr {exc})"));
                self.emit(&format!("call void @rt_task_release(ptr {task_val})"));
                self.emit_call_may_throw("void", "@rt_throw", &format!("ptr {exc}"), true, None);
                self.emit("unreachable");
                self.emit_label(&extract_label);
                if !matches!(place_ty, TypeId::Void) {
                    let ptr = self.local_ptr(*place);
                    let slot_ty = llvm_type_of(&place_ty, self.layouts);
                    match &place_ty {
                        TypeId::Int
                        | TypeId::Short
                        | TypeId::Byte
                        | TypeId::Char
                        | TypeId::Bool
                        | TypeId::UInt
                        | TypeId::UShort
                        | TypeId::SByte => {
                            let result = self.fresh_temp();
                            self.emit(&format!(
                                "{result} = call i32 @rt_task_result_int(ptr {task_val})"
                            ));
                            // bool 槽位是 i1，rt_task_result_int 返回 i32：store 前 trunc。
                            if slot_ty != "i32" {
                                let narrowed = self.fresh_temp();
                                self.emit(&format!("{narrowed} = trunc i32 {result} to {slot_ty}"));
                                self.emit(&format!("store {slot_ty} {narrowed}, ptr {ptr}"));
                            } else {
                                self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                            }
                        }
                        TypeId::String
                        | TypeId::Named(_)
                        | TypeId::Array { .. }
                        | TypeId::Task { .. } => {
                            let result = self.fresh_temp();
                            self.emit(&format!(
                                "{result} = call ptr @rt_task_result_ptr(ptr {task_val})"
                            ));
                            // 与 M2 SM await 提取一致：task 的 ptr_result 是借引用
                            // （rt_task_release 不 dec），class 结果须 retain 使局部
                            // 持独立 +1，与 epilogue dec 配对。缺 retain 时 rc=1 结果
                            // 被局部出口 dec 提前释放 → UAF。string/array 无 ArcHeader
                            // 不 inc。
                            if Self::arc_class_place(&place_ty, self.layouts) {
                                self.emit(&format!("call void @rt_arc_inc(ptr {result})"));
                            }
                            self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                        }
                        TypeId::Long | TypeId::Float | TypeId::Double | TypeId::ULong => {
                            let size = match place_ty {
                                TypeId::Long | TypeId::ULong => 8,
                                TypeId::Float => 4,
                                TypeId::Double => 8,
                                _ => unreachable!(),
                            };
                            let tmp = self.fresh_temp();
                            self.emit(&format!("{tmp} = alloca {slot_ty}"));
                            self.emit(&format!(
                                "call void @rt_task_result_value(ptr {task_val}, ptr {tmp}, i32 {size})"
                            ));
                            let result = self.fresh_temp();
                            self.emit(&format!("{result} = load {slot_ty}, ptr {tmp}"));
                            self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                        }
                        _ => {
                            // 兜底：按 int 处理（保持向前兼容）
                            let result = self.fresh_temp();
                            self.emit(&format!(
                                "{result} = call i32 @rt_task_result_int(ptr {task_val})"
                            ));
                            self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                        }
                    }
                }
                // M5.2：inner Task 结果已提取，释放回 slab（与 M2 路径对齐）。
                self.emit(&format!("call void @rt_task_release(ptr {task_val})"));
            }
            MirStatement::TryCatch {
                try_body,
                catch_var,
                catch_ty,
                catch_body,
            } => {
                self.emit_try_catch(try_body, *catch_var, catch_ty, catch_body);
            }
            MirStatement::TryFinally { body, finally } => {
                self.emit_try_finally(body, finally);
            }
            MirStatement::LinqForeach { var, chain, body } => {
                self.emit_linq_foreach(var, chain, body);
            }
            // These should only appear in nested bodies (TryCatch/LinqForeach),
            // but handle them for safety.
            MirStatement::Return(val) => {
                // Zero-cost EH M6: legacy try-stack registry removed (milestone ⑥);
                // return inside a try body needs no balancing pops.

                // Phase 0 async SM: delegate return to state-machine-aware path
                // (sets state=-1, writes result to Task, ret i32 0).
                if self.in_state_machine {
                    self.emit_finally_chain();
                    let env_type = self.sm_env_type.clone();
                    self.emit_sm_return_stmt(val, &env_type);
                    self.flow_terminated = true;
                    return;
                }
                // RFC 009 I1：协程路径 return——结果写入 Task → final suspend。
                if self.in_coroutine {
                    self.emit_finally_chain();
                    self.emit_coro_return_stmt(val);
                    self.flow_terminated = true;
                    return;
                }
                // C# 对齐：`try { return <expr>; } finally { … }` 先求值并保存
                // 返回值，再执行 finally，最后 return 保存值。否则 finally 对
                // 局部变量的修改会污染返回值；且 void 入口函数（LLVM `i32 @main`）
                // 的 `return;` 会误发 `ret void` 与函数签名不匹配。故把「求值」
                // 与「发射 ret」拆开，中间插入 finally 内联执行（栈为空时是 no-op）。
                if let Some(rv) = val {
                    let place_ty = if self.cfg.is_async {
                        self.cfg.ret.task_inner().cloned().unwrap_or(TypeId::Void)
                    } else {
                        self.cfg.ret.clone()
                    };
                    if matches!(place_ty, TypeId::Void) {
                        self.emit_rvalue(rv);
                        self.emit_finally_chain();
                        if self.cfg.is_async {
                            let task = self.fresh_temp();
                            self.emit(&format!("{task} = call ptr @rt_task_void()"));
                            self.emit(&format!("ret ptr {task}"));
                        } else {
                            self.emit_ret_void();
                        }
                    } else if self.cfg.is_async {
                        // 通道按值实际发射的 LLVM 表示选择（唯一权威）：
                        // ptr → from_ptr（class/string/interface/array/task 句柄），
                        // i64/double/float → from_value（8/4B 值槽），其余 → from_int。
                        // 不得按 inner_ty 分派——lambda 块体 return 局部在宿主
                        // ctx 不可解析时推断 fallback Bool→Int（infer_lambda_block_ret），
                        // inner_ty 残缺误报 Int 族，from_int(ptr) 会把引用截断进
                        // int_result（ptr_result 恒 null），外层 await 按引用提取
                        // 得 null——嵌套 async lambda 引用返回丢失根因（流式 TTS
                        // chunk=null / probe4）。
                        let (ty, val) = self.emit_rvalue_typed(rv, &place_ty);
                        self.emit_finally_chain();
                        // RFC 009 §结果所有权（强持有，2026-08-22 收敛）：class
                        // 结果无条件 inc（与 release 的 dec 严格配对）+
                        // from_class 置位。接口胖指针盒/Task 句柄无 ArcHeader
                        // （借用），string/array immortal，均走 from_ptr。
                        let task = if Self::arc_class_place(&place_ty, self.layouts) {
                            self.emit(&format!("call void @rt_arc_inc(ptr {val})"));
                            let task = self.fresh_temp();
                            self.emit(&format!("{task} = call ptr @rt_task_from_class(ptr {val})"));
                            task
                        } else {
                            self.emit_task_from_abi(&ty, &val)
                        };
                        self.emit(&format!("ret ptr {task}"));
                    } else {
                        // 接口类型返回：胖指针必须堆分配——栈 alloca 随本帧
                        // `ret` 弹出，调用方后续（隔次调用后）读取会解引用已
                        // 覆盖的栈区 → ACCESS_VIOLATION（struct 返回同款先例，
                        // 见 `materialize_null_return` 注释）。
                        let (ty, val) = match &place_ty {
                            TypeId::Named(n) if is_iface_name(n) => match rv {
                                MirRvalue::MakeIface {
                                    class,
                                    iface,
                                    object,
                                } => self.emit_make_iface(class, iface, object, true),
                                MirRvalue::MakeIfaceDyn { iface, object } => {
                                    self.emit_make_iface_dyn(iface, object, true)
                                }
                                MirRvalue::AdaptIface {
                                    from_iface,
                                    to_iface,
                                    object,
                                } => self.emit_adapt_iface(from_iface, to_iface, object, true),
                                // 其它 rvalue 按返回类型 typed 发射——保证 builtin facade
                                // （如 `Task.FromResult(...)`）能以正确的 `expected` 命中
                                // `try_emit_task_static` 分派，而非落成裸 `@Task.FromResult`
                                // 未定义符号（sync 返回路径与 async/place-store 路径对齐）。
                                _ => self.emit_rvalue_typed(rv, &place_ty),
                            },
                            _ => self.emit_rvalue_typed(rv, &place_ty),
                        };
                        let ret_ty_str = llvm_type_of(&place_ty, self.layouts);
                        let (coerced_ty, coerced_val) = self.coerce_value(&ty, val, &ret_ty_str);
                        let (coerced_ty, coerced_val) =
                            self.materialize_null_return(&place_ty, &coerced_ty, coerced_val);
                        // Variant 返回深拷贝到堆（构造 alloca 随本帧 `ret` 弹出而悬垂）。
                        let (coerced_ty, coerced_val) =
                            self.heap_copy_variant_return(&place_ty, &coerced_ty, coerced_val);
                        // ARC 所有权：`try { return <class 局部/字段>; } finally { … }`
                        // 中，返回值须在 finally 前 inc 取得独立所有权——否则 finally
                        // 对源局部/字段重赋值会 dec 到 rc=0 释放，`ret` 悬垂。仅对
                        // 「位置读取」（local/field/static/FieldGet）inc：Call/New 结果
                        // 已是新鲜 rc=1 无需 inc（否则 +1 泄漏）。finally 后再
                        // epilogue-drop 释放源局部的原引用，与 inc 配对避免泄漏。
                        let place_class_ret = !self.finally_stack.is_empty()
                            && Self::arc_class_place(&place_ty, self.layouts)
                            && matches!(
                                rv,
                                MirRvalue::Use(MirOperand::Local(_))
                                    | MirRvalue::Use(MirOperand::Field { .. })
                                    | MirRvalue::Use(MirOperand::StaticField { .. })
                                    | MirRvalue::FieldGet { .. }
                            );
                        if place_class_ret {
                            self.emit(&format!("call void @rt_arc_inc(ptr {coerced_val})"));
                        }
                        self.emit_finally_chain();
                        if place_class_ret {
                            self.emit_sync_epilogue_drops(None);
                        }
                        self.emit(&format!("ret {coerced_ty} {coerced_val}"));
                    }
                } else if self.cfg.is_async {
                    self.emit_finally_chain();
                    let task = self.fresh_temp();
                    self.emit(&format!("{task} = call ptr @rt_task_void()"));
                    self.emit(&format!("ret ptr {task}"));
                } else {
                    self.emit_finally_chain();
                    self.emit_ret_void();
                }
                self.flow_terminated = true;
            }
            MirStatement::If {
                cond,
                then_body,
                else_body,
            } => {
                self.emit_nested_if(cond, then_body, else_body);
            }
            MirStatement::While { cond, body, .. } => {
                self.emit_nested_while(cond, body);
            }
            MirStatement::Break => {
                let exit = self
                    .nested_loop_stack
                    .last()
                    .map(|(exit, _)| exit.clone())
                    .unwrap_or_else(|| {
                        panic!("codegen: break outside loop (expected to_cfg flatten)")
                    });
                self.emit_finally_chain();
                self.emit(&format!("br label %{exit}"));
                self.flow_terminated = true;
            }
            MirStatement::Continue => {
                let cont = self
                    .nested_loop_stack
                    .last()
                    .map(|(_, cont)| cont.clone())
                    .unwrap_or_else(|| {
                        panic!("codegen: continue outside loop (expected to_cfg flatten)")
                    });
                self.emit_finally_chain();
                self.emit(&format!("br label %{cont}"));
                self.flow_terminated = true;
            }
        }
    }

    // ---- Terminator emission ----

    /// 按值实际发射的 LLVM 表示选择 Task 完成包装 ABI（from_int/from_ptr/from_value）。
    ///
    /// 通道选择的唯一权威：`emit_rvalue_typed`/`emit_operand` 返回的 `ty` 是值的
    /// 真实运行时表示。`cfg.ret` 的 inner_ty 是推断副本——lambda 块体 `return`
    /// 局部在宿主 ctx 不可解析时 fallback Bool→Int（`infer_lambda_block_ret`），
    /// 按 inner_ty 分派会 `from_int(ptr)` 把引用截断进 int_result（ptr_result
    /// 恒 null → 外层 await 按引用提取得 null，嵌套 async lambda 引用返回丢失）。
    pub(crate) fn emit_task_from_abi(&mut self, ty: &str, val: &str) -> String {
        let task = self.fresh_temp();
        match ty {
            "ptr" => {
                self.emit(&format!("{task} = call ptr @rt_task_from_ptr(ptr {val})"));
            }
            "i64" | "double" | "float" => {
                let size: i32 = if ty == "float" { 4 } else { 8 };
                let slot = self.fresh_temp();
                self.emit(&format!("{slot} = alloca {ty}"));
                self.emit(&format!("store {ty} {val}, ptr {slot}"));
                self.emit(&format!(
                    "{task} = call ptr @rt_task_from_value(ptr {slot}, i32 {size})"
                ));
            }
            _ => {
                self.emit(&format!("{task} = call ptr @rt_task_from_int({ty} {val})"));
            }
        }
        task
    }

    /// 按值实际发射的 LLVM 表示选择 SM 完成结果写入 ABI（set_result_int/ptr/value）。
    /// 与 `emit_task_from_abi` 同因同则：不信任 inner_ty，按实际表示分派。
    pub(crate) fn emit_task_set_result_abi(&mut self, task_ptr: &str, ty: &str, val: &str) {
        match ty {
            "ptr" => {
                self.emit(&format!(
                    "call void @rt_task_set_result_ptr(ptr {task_ptr}, ptr {val})"
                ));
            }
            "i64" | "double" | "float" => {
                let size: i32 = if ty == "float" { 4 } else { 8 };
                let slot = self.fresh_temp();
                self.emit(&format!("{slot} = alloca {ty}"));
                self.emit(&format!("store {ty} {val}, ptr {slot}"));
                self.emit(&format!(
                    "call void @rt_task_set_result_value(ptr {task_ptr}, ptr {slot}, i32 {size})"
                ));
            }
            _ => {
                self.emit(&format!(
                    "call void @rt_task_set_result_int(ptr {task_ptr}, {ty} {val})"
                ));
            }
        }
    }

    pub(super) fn emit_terminator(&mut self, term: &MirTerminator) {
        match term {
            MirTerminator::Goto(bb) => {
                // RFC 009 M3：`[Parallelize]` 标记的函数中，若该 Goto 是 while
                // 循环 backedge（记录于 `cfg.loop_backedges`），附加
                // `!llvm.loop !N` metadata 强制 LLVM loop-vectorize pass 向量化。
                //
                // **平台差异化**：metadata 平台无关，LLVM 据目标 CPU 特征选择
                // 指令集——x86-64 启用 SSE2/AVX2/AVX-512，AArch64 启用 NEON，
                // 其他平台退化标量（无错误，仅无性能收益）。
                if self.cfg.parallelize && self.cfg.loop_backedges.contains(&self.current_block_id)
                {
                    let loop_md_id = self.dbg.alloc_loop_md();
                    self.emit(&format!("br label %bb{}, !llvm.loop !{loop_md_id}", bb.0));
                } else {
                    self.emit(&format!("br label %bb{}", bb.0));
                }
            }
            MirTerminator::CondBr {
                cond,
                then_bb,
                else_bb,
            } => {
                let (ty, val) = self.emit_operand(cond);
                let bool_val = if ty == "i1" {
                    val
                } else {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = icmp ne {ty} {val}, 0"));
                    t
                };
                self.emit(&format!(
                    "br i1 {bool_val}, label %bb{}, label %bb{}",
                    then_bb.0, else_bb.0
                ));
            }
            MirTerminator::Return(val) => {
                // RFC 009 M2: 状态机 resume 函数内的 return 走状态机路径
                //（设置 state=-1 + 写 result 到 Task 句柄 + ret i32 0）。
                // Phase 0: emit finally chain before SM return (TryFinally support).
                if self.in_state_machine {
                    self.emit_finally_chain();
                    let env_type = self.sm_env_type.clone();
                    self.emit_sm_return(val, &env_type);
                    return;
                }
                // RFC 009 I1：协程路径——结果写入 Task → final suspend 挂起
                //（done 置位），帧由 destroy 释放。
                if self.in_coroutine {
                    self.emit_finally_chain();
                    self.emit_coro_return(val);
                    return;
                }
                // ARC 返回契约：借引用返回源（字段/静态字段/参数局部）须在
                // epilogue drop 前求值并 inc——把「借用」转成调用方拥有的
                // owned ref（与调用方 epilogue 的 dec 配对），并避免 drop
                // 本地对象后才读其悬垂字段（`return e.Descriptor` 此前先
                // drop e 再读 → 悬垂读）。非参数局部已持所有权（new/Call
                // 移交 / 拷贝 retain 均建立），直接转移不 inc——工厂对象
                // （rc=1 新对象）由此不泄漏。
                let mut pre_ret: Option<(String, String)> = None;
                if !self.cfg.is_async {
                    if let Some(op) = val.as_ref() {
                        let op_owned = op.clone();
                        let ret_ty = self.cfg.ret.clone();
                        pre_ret = self.maybe_emit_return_inc(&op_owned, &ret_ty);
                    }
                }
                // ARC epilogue：sync ret 前释放 class 局部（async/SM 在
                // emit_sync_epilogue_drops 内部 no-op）。排除 `return <local>;`
                // 的返回局部（强引用移交调用方）。
                //
                // 返回操作数若是接口装箱/拆箱包装（`return (IFace)<local>` /
                // `return (IFace)new T(...)`），其底层源局部与装箱结果是**同一强
                // 引用**——return 即把所有权移交调用方，该局部同样不得被 epilogue
                // drop。否则 rc 1→0 现场释放，`ret` 交出悬垂指针：实测 DI 装饰
                // 工厂闭包（DecorationExtensions.Decorate 的
                // `(sp) => (object)new TDecorator(...)`）返回的装饰对象被多扣一次，
                // 满载堆churn下 vtable/类型元数据被复用 → 间歇 InvalidCastException
                // （di_decorate_two_layers_onion_order / di_onion_stress 根因）。
                let returned = returned_owner_local(val.as_ref());
                self.emit_sync_epilogue_drops(returned);
                if self.cfg.is_async {
                    // Async return: wrap value in task
                    if let Some(op) = val {
                        let inner_ty = self.cfg.ret.task_inner().cloned().unwrap_or(TypeId::Void);
                        if matches!(inner_ty, TypeId::Void) {
                            let (_, val_str) = self.emit_operand(op);
                            let _ = val_str;
                            let task = self.fresh_temp();
                            self.emit(&format!("{task} = call ptr @rt_task_void()"));
                            self.emit(&format!("ret ptr {task}"));
                        } else {
                            // 通道按值实际发射的 LLVM 表示选择（唯一权威），
                            // 不得按 inner_ty 分派——推断残缺（Bool→Int fallback）
                            // 会 from_int(ptr) 截断引用（详见上方同因注释）。
                            let (ty, val_str) = self.emit_operand(op);
                            // RFC 009 §结果所有权（强持有，2026-08-22 收敛）：
                            // class 结果无条件 inc + from_class（与 release 的
                            // dec 严格配对）；string/array（immortal）/接口胖指
                            // 针盒（无 ArcHeader）走 from_abi 借用。
                            let task = if Self::arc_class_place(&inner_ty, self.layouts) {
                                self.emit(&format!("call void @rt_arc_inc(ptr {val_str})"));
                                let task = self.fresh_temp();
                                self.emit(&format!(
                                    "{task} = call ptr @rt_task_from_class(ptr {val_str})"
                                ));
                                task
                            } else {
                                self.emit_task_from_abi(&ty, &val_str)
                            };
                            self.emit(&format!("ret ptr {task}"));
                        }
                    } else {
                        let task = self.fresh_temp();
                        self.emit(&format!("{task} = call ptr @rt_task_void()"));
                        self.emit(&format!("ret ptr {task}"));
                    }
                } else if let Some(op) = val {
                    let (ty, val_str) = match pre_ret {
                        Some((t, v)) => (t, v),
                        // 委托类型返回值统一为 `arc_closure*`（调用方 GEP 解引用
                        // fn_ptr/env）。裸 `FnPtr`（无捕获 lambda / 方法组）与存裸
                        // FnPtr 的局部经 `emit_operand_as_closure` 包装为
                        // `arc_closure{fn, null}`，否则调用方把函数代码当闭包对象
                        // 解引用 → 0xC0000005 / 静默失败。
                        None if is_delegate_type(&self.cfg.ret) => self.emit_operand_as_closure(op),
                        None => self.emit_operand(op),
                    };
                    // RFC 039 M2：返回值已读入寄存器，此处发射配套
                    // `!llvm.lifetime.end`（排除返回局部槽）。必须在 ret 前、
                    // 值加载完成后，避免把仍被引用/读取的槽判为已死。
                    self.emit_stack_lifetime_ends(returned);
                    if ty == "void" {
                        if self.is_main {
                            self.emit("ret i32 0");
                        } else {
                            self.emit("ret void");
                        }
                    } else {
                        // RFC 037 M3：返回值类型与函数签名不匹配时强制 coerce
                        // （如 `long foo() { return 0; }` 中 i32 字面量需 sext 到 i64）。
                        // 此前 MirTerminator::Return 路径直接 emit `ret {ty} {val}`
                        // 不做 coercion，导致 `ret i32 0` 与 `define i64 @foo()` 签名
                        // 不匹配触发 LLVM verifier 报错。
                        let ret_ty_str = llvm_type_of(&self.cfg.ret, self.layouts);
                        if ret_ty_str == "void" {
                            // 函数返回 void 但表达式产生非 void 值（三元/coalesce/async task
                            // 等 MIR lower 产生的中间值）。该值不能返回——写入临时变量后
                            // ret void，阻止 LLVM verifier "value doesn't match function
                            // result type 'void'" 错误。
                            let tmp = self.fresh_temp();
                            self.emit(&format!("{tmp} = add {ty} {val_str}, 0"));
                            self.emit("ret void");
                        } else {
                            let (coerced_ty, coerced_val) =
                                self.coerce_value(&ty, val_str, &ret_ty_str);
                            let ret_type = self.cfg.ret.clone();
                            // 接口类型返回：把（可能指向本帧栈 alloca 的）胖指针
                            // 复制到 calloc 堆块。`ret` 后本帧弹出，栈上 `{ptr,ptr}`
                            // 会被后续调用覆盖 → 调用方按胖指针解引用 ACCESS_VIOLATION
                            // （与 struct 返回堆化先例一致，见 materialize_null_return）。
                            if matches!(&ret_type, TypeId::Named(n) if is_iface_name(n))
                                && coerced_ty == "ptr"
                            {
                                let (cty, cval) = self.emit_iface_ret_heap_copy(&coerced_val);
                                self.emit(&format!("ret {cty} {cval}"));
                            } else {
                                let (coerced_ty, coerced_val) = self.materialize_null_return(
                                    &ret_type,
                                    &coerced_ty,
                                    coerced_val,
                                );
                                // Variant 返回深拷贝到堆（构造 alloca 随本帧 `ret`
                                // 弹出而悬垂，调用方回读 tag/payload 读到被复用栈）。
                                let (coerced_ty, coerced_val) = self.heap_copy_variant_return(
                                    &ret_type,
                                    &coerced_ty,
                                    coerced_val,
                                );
                                self.emit(&format!("ret {coerced_ty} {coerced_val}"));
                            }
                        }
                    }
                } else {
                    // Return(None) in non-async function: void return or unreachable.
                    // RFC 039 M2：无返回值，直接发射配套 `!llvm.lifetime.end`。
                    self.emit_stack_lifetime_ends(returned);
                    if self.is_main {
                        self.emit("ret i32 0");
                    } else if matches!(self.cfg.ret, TypeId::Void) {
                        self.emit("ret void");
                    } else {
                        // Non-void function with Return(None) — this is dead code
                        // (e.g. after a Return statement). Emit unreachable to avoid
                        // type mismatch (ret void in i32 function).
                        self.emit("unreachable");
                    }
                }
            }
            MirTerminator::Throw(val) => {
                let (_, val_str) = self.emit_operand(val);
                // Zero-cost EH M5：rt_throw 恒 may-throw。try/finally 区域内发
                // invoke（异常落入本区域 catchswitch/cleanup funclet）；区域外
                // plain call（异常沿 .pdata 向调用方传播）。
                self.emit_call_may_throw(
                    "void",
                    "@rt_throw",
                    &format!("ptr {val_str}"),
                    true,
                    None,
                );
                self.emit("unreachable");
            }
            MirTerminator::Unreachable => {
                if self.is_main && !self.cfg.is_async {
                    self.emit("ret i32 0");
                } else {
                    self.emit("unreachable");
                }
            }
        }
    }

    // ---- Nested statement emission (for TryCatch/LinqForeach bodies) ----

    fn emit_nested_if(
        &mut self,
        cond: &MirOperand,
        then_body: &[MirStatement],
        else_body: &[MirStatement],
    ) {
        let (cond_ty, cond_val) = self.emit_operand(cond);
        let bool_val = if cond_ty == "i1" {
            cond_val
        } else {
            let t = self.fresh_temp();
            self.emit(&format!("{t} = icmp ne {cond_ty} {cond_val}, 0"));
            t
        };
        let then_label = self.fresh_label();
        let else_label = self.fresh_label();
        let merge_label = self.fresh_label();
        self.emit(&format!(
            "br i1 {bool_val}, label %{then_label}, label %{else_label}"
        ));
        self.emit_label(&then_label);
        let saved = self.flow_terminated;
        self.flow_terminated = false;
        for (i, s) in then_body.iter().enumerate() {
            if self.flow_terminated {
                break;
            }
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
        }
        let then_term = self.flow_terminated;
        if !then_term {
            self.emit(&format!("br label %{merge_label}"));
        }
        self.emit_label(&else_label);
        self.flow_terminated = false;
        for (i, s) in else_body.iter().enumerate() {
            if self.flow_terminated {
                break;
            }
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
        }
        let else_term = self.flow_terminated;
        if !else_term {
            self.emit(&format!("br label %{merge_label}"));
        }
        self.flow_terminated = saved || (then_term && else_term);
        if !(then_term && else_term) {
            self.emit_label(&merge_label);
        }
    }

    /// 结构化 `while` 发射。`to_cfg` 已把函数体展平为 flag 式 CFG（块内不含
    /// `While`），此处仅作兜底；游标提升统一走平面 CFG 的 `sb_promotes` 计划。
    fn emit_nested_while(&mut self, cond: &MirRvalue, body: &[MirStatement]) {
        let header = self.fresh_label();
        let body_label = self.fresh_label();
        let exit = self.fresh_label();
        self.emit(&format!("br label %{header}"));
        self.emit_label(&header);
        let (cond_ty, cond_val) = self.emit_rvalue(cond);
        let bool_val = if cond_ty == "i1" {
            cond_val
        } else {
            let t = self.fresh_temp();
            self.emit(&format!("{t} = icmp ne {cond_ty} {cond_val}, 0"));
            t
        };
        self.emit(&format!(
            "br i1 {bool_val}, label %{body_label}, label %{exit}"
        ));
        self.emit_label(&body_label);
        self.nested_loop_stack.push((exit.clone(), header.clone()));
        let saved = self.flow_terminated;
        self.flow_terminated = false;
        for (i, s) in body.iter().enumerate() {
            if self.flow_terminated {
                break;
            }
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
        }
        let body_term = self.flow_terminated;
        self.nested_loop_stack.pop();
        self.flow_terminated = saved;
        if !body_term {
            // RFC 009 M3：`[Parallelize]` 标记的函数，在 while 循环 backedge 上
            // 附加 `!llvm.loop !N` metadata，强制 LLVM loop-vectorize pass 向量化。
            if self.cfg.parallelize {
                let loop_md_id = self.dbg.alloc_loop_md();
                self.emit(&format!("br label %{header}, !llvm.loop !{loop_md_id}"));
            } else {
                self.emit(&format!("br label %{header}"));
            }
        }
        self.emit_label(&exit);
    }

    /// 纯追加循环 preheader：解析 StringBuilder 接收者 → 头句柄，并为
    /// data/len/cap 建 shadow alloca（hoist 到 entry 块，SROA → 寄存器）。
    /// shadow 从堆头一次性初始化，循环内不再触碰堆头。
    fn emit_sb_shadow_preheader(&mut self, rid: mir::LocalId) -> Option<SbShadow> {
        // 接收者 local 的 alloca 持有 StringBuilder 对象指针。
        let recv_obj = self.fresh_temp();
        self.emit(&format!(
            "{recv_obj} = load ptr, ptr {}",
            self.local_ptr(rid)
        ));
        // rt_sb_t 句柄位于 StringBuilder 对象偏移 16（RFC 005 布局契约）。
        let hp = self.fresh_temp();
        self.emit(&format!(
            "{hp} = getelementptr inbounds i8, ptr {recv_obj}, i32 16"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load ptr, ptr {hp}"));
        // shadow alloca 提升到 entry 块，使 mem2reg/SROA 可转寄存器。
        let data = self.fresh_temp();
        self.entry_allocas
            .push_str(&format!("  {data} = alloca ptr\n"));
        let len = self.fresh_temp();
        self.entry_allocas
            .push_str(&format!("  {len} = alloca i64\n"));
        let cap = self.fresh_temp();
        self.entry_allocas
            .push_str(&format!("  {cap} = alloca i64\n"));
        // 从堆头一次性初始化 shadow。
        let tbaa = self.dbg.sb_tbaa();
        let data_addr = self.fresh_temp();
        self.emit(&format!(
            "{data_addr} = getelementptr inbounds i8, ptr {handle}, i32 0"
        ));
        let d = self.fresh_temp();
        self.emit(&format!(
            "{d} = load ptr, ptr {data_addr}, !tbaa !{}",
            tbaa.data
        ));
        self.emit(&format!("store ptr {d}, ptr {data}"));
        let len_addr = self.fresh_temp();
        self.emit(&format!(
            "{len_addr} = getelementptr inbounds i8, ptr {handle}, i32 8"
        ));
        let l = self.fresh_temp();
        self.emit(&format!(
            "{l} = load i64, ptr {len_addr}, !tbaa !{}",
            tbaa.len
        ));
        self.emit(&format!("store i64 {l}, ptr {len}"));
        let cap_addr = self.fresh_temp();
        self.emit(&format!(
            "{cap_addr} = getelementptr inbounds i8, ptr {handle}, i32 16"
        ));
        let c = self.fresh_temp();
        self.emit(&format!(
            "{c} = load i64, ptr {cap_addr}, !tbaa !{}",
            tbaa.cap
        ));
        self.emit(&format!("store i64 {c}, ptr {cap}"));
        Some(SbShadow {
            handle,
            data,
            len,
            cap,
        })
    }

    /// 出口 flush：把 shadow 一次性写回 rt_sb_t 头字段。
    fn emit_sb_shadow_flush(&mut self, sh: &SbShadow) {
        let tbaa = self.dbg.sb_tbaa();
        let data_addr = self.fresh_temp();
        self.emit(&format!(
            "{data_addr} = getelementptr inbounds i8, ptr {}, i32 0",
            sh.handle
        ));
        let d = self.fresh_temp();
        self.emit(&format!("{d} = load ptr, ptr {}", sh.data));
        self.emit(&format!(
            "store ptr {d}, ptr {data_addr}, !tbaa !{}",
            tbaa.data
        ));
        let len_addr = self.fresh_temp();
        self.emit(&format!(
            "{len_addr} = getelementptr inbounds i8, ptr {}, i32 8",
            sh.handle
        ));
        let l = self.fresh_temp();
        self.emit(&format!("{l} = load i64, ptr {}", sh.len));
        self.emit(&format!(
            "store i64 {l}, ptr {len_addr}, !tbaa !{}",
            tbaa.len
        ));
        let cap_addr = self.fresh_temp();
        self.emit(&format!(
            "{cap_addr} = getelementptr inbounds i8, ptr {}, i32 16",
            sh.handle
        ));
        let c = self.fresh_temp();
        self.emit(&format!("{c} = load i64, ptr {}", sh.cap));
        self.emit(&format!(
            "store i64 {c}, ptr {cap_addr}, !tbaa !{}",
            tbaa.cap
        ));
    }

    /// RFC 005：扫描 flat CFG，识别可提升的 flag 式纯追加循环
    /// `while(flag){ if(cond){ sb.Append(char); … } else { flag=false } }`。
    ///
    /// 结构（由 `to_cfg` 展平产生）：
    /// - `backedge` 块 `Goto(header)`；
    /// - header `CondBr{ then: B, else: E }`（B=嵌套 If 头，E=循环出口）；
    /// - B `CondBr{ then: B_then, else: B_else }`（B_then=纯追加体，B_else=flag=false）；
    /// - B_then / B_else `Goto(b_join)`，b_join `Goto(header)`（backedge）。
    ///
    /// 仅当：B_then 为纯 `sb.Append(char)` 体、循环内各块不引用接收者、且
    /// header 唯一前驱（preheader）时才提升。返回 (preheader, exit, receiver)。
    pub(super) fn find_sb_promote_loops(&self) -> Vec<SbPromoteLoop> {
        use mir::MirTerminator;
        let blocks = &self.cfg.blocks;
        let mut out = Vec::new();
        for bj in &self.cfg.loop_backedges {
            let bj_block = match blocks.get(bj) {
                Some(b) => b,
                None => continue,
            };
            let MirTerminator::Goto(header) = &bj_block.terminator else {
                continue;
            };
            let hblock = match blocks.get(header) {
                Some(h) => h,
                None => continue,
            };
            let MirTerminator::CondBr {
                then_bb: if_hdr,
                else_bb: exit,
                ..
            } = &hblock.terminator
            else {
                continue;
            };
            // 嵌套 If 头（header 的 then 目标）：其 CondBr 的 then = 纯追加体，
            // else = flag=false。
            let bblock = match blocks.get(if_hdr) {
                Some(b) => b,
                None => continue,
            };
            let MirTerminator::CondBr {
                then_bb: body_then,
                else_bb: body_else,
                ..
            } = &bblock.terminator
            else {
                continue;
            };
            let (Some(bt), Some(be)) = (blocks.get(body_then), blocks.get(body_else)) else {
                continue;
            };
            let receiver = match sb_promote::sb_append_cfg_receiver(
                &bt.statements,
                &[hblock, bblock, be, bj_block],
            ) {
                Some(r) => r,
                None => continue,
            };
            // header 必须恰有两个前驱：一个 backedge（bj）+ 一个 preheader。
            // 若存在 CondBr 直入 header 等多前驱形态 → 结构不纯，跳过。
            let mut preheader: Option<mir::BlockId> = None;
            let mut non_goto_pred = false;
            let mut preds = 0usize;
            for (pid, pb) in blocks {
                let targets_header = match &pb.terminator {
                    MirTerminator::Goto(h) => h == header,
                    MirTerminator::CondBr {
                        then_bb, else_bb, ..
                    } => then_bb == header || else_bb == header,
                    _ => false,
                };
                if !targets_header {
                    continue;
                }
                preds += 1;
                if *pid == *bj {
                    continue; // backedge 不算 preheader
                }
                if !matches!(&pb.terminator, MirTerminator::Goto(_)) {
                    non_goto_pred = true;
                }
                if preheader.is_some() {
                    non_goto_pred = true;
                }
                preheader = Some(*pid);
            }
            if non_goto_pred || preds != 2 || preheader.is_none() {
                continue;
            }
            let preheader = preheader.unwrap();
            // 嵌套保护：单 `sb_shadow` 状态无法同时承载内外两层提升（内层 exit
            // flush 会 `take()` 掉外层 shadow，后续外层热路径回归堆头 → 错）。
            // `to_cfg` 按 DFS 序分配块 id，嵌套循环的块 id 严格落在外层
            // preheader 与 exit 之间——据此在同函数内互斥丢弃嵌套候选。
            if crate::llvm_ir::sb_shadow_nested(&out, preheader, *exit) {
                continue;
            }
            out.push(SbPromoteLoop {
                preheader,
                body: *body_then,
                exit: *exit,
                receiver,
            });
        }
        out
    }

    fn emit_try_catch(
        &mut self,
        try_body: &[MirStatement],
        catch_var: mir::LocalId,
        catch_ty: &TypeId,
        catch_body: &[MirStatement],
    ) {
        // Zero-cost EH milestone ② (Windows SEH): invoke + catchswitch/catchpad.
        if self.is_windows {
            self.emit_try_catch_seh(try_body, catch_var, catch_ty, catch_body);
            return;
        }
        // Milestone ⑥ removed the legacy try-stack pattern; POSIX
        // zero-cost EH (Itanium personality + landingpad) is milestone ⑨
        // (1.1+, 非 1.0 门槛). 可达函数集在 `ModuleEmitter::emit_module`
        // 入口已被 `reject_try_catch_outside_windows`（arc-eh-001）结构化
        // 拦截——此处 panic 仅为防御性 ICE（防未来新发射路径绕过该门）。
        // 里程碑⑨ 需同时落地 codegen（landingpad/resume + __gxx_personality_v0/
        // uwtable）与 POSIX 运行时（rt_throw 改 _Unwind_RaiseException，见
        // crates/runtime/rt_exc.c），且须在 Linux/macOS 环境验收（见
        // docs/rfc/010-exceptions-resources.md、docs/plan.md 缺漏 #6）。
        panic!(
            "codegen: try/catch is not yet supported on non-Windows targets \
             (zero-cost EH milestone ⑨ / POSIX Itanium, RFC 010)"
        );
    }

    /// Zero-cost EH milestone ② (Windows SEH): `invoke` + funclet `catchswitch`
    /// implementation of `try/catch` with a catch-all selector (`catch ptr null`,
    /// RFC 010 §2.1 minimal personality — no typeinfo; type filtering is
    /// milestone ④).
    ///
    /// IR shape (validated by probes against clang/LLVM Windows x64):
    ///
    /// ```text
    ///   br label %try0
    /// try0:                          ; try region — may-throw calls become
    ///   invoke void @helper() to label %c1 unwind label %cs0
    /// c1:
    ///   …
    ///   br label %after0
    /// cs0:
    ///   %cs = catchswitch within none [label %pad0] unwind to caller
    /// pad0:
    ///   %cp = catchpad within %cs [ptr null, i32 64, ptr null]
    ///   catchret from %cp to label %catch0
    /// catch0:                        ; normal block — SEH forbids calls inside
    ///   %exc = call ptr @rt_get_exception()   ; a catchpad funclet directly,
    ///   … catch body …                        ; so we exit via early catchret
    ///   br label %after0
    /// after0:
    /// ```
    ///
    /// `rt_get_exception` reads the TLS slot set by `rt_throw`; the legacy
    /// try-stack registry was removed entirely in milestone ⑥.
    fn emit_try_catch_seh(
        &mut self,
        try_body: &[MirStatement],
        catch_var: mir::LocalId,
        catch_ty: &TypeId,
        catch_body: &[MirStatement],
    ) {
        let try_label = self.fresh_label();
        let cs_label = self.fresh_label();
        let pad_label = self.fresh_label();
        let catch_label = self.fresh_label();
        let after_label = self.fresh_label();

        self.emit(&format!("br label %{try_label}"));
        self.emit_label(&try_label);

        let saved_ft = self.flow_terminated;
        self.flow_terminated = false;
        let prev_caught = self.emitting_caught_try_body;
        self.emitting_caught_try_body = true;
        self.eh_region_stack.push(cs_label.clone());
        // M3 (Windows SEH)：catchswitch 同时加入 cleanup 链。try body 内嵌套的
        // try/finally 其 cleanupret 若未匹配，必须 `unwind label %cs` 让本层
        // catchswitch 有机会接住；否则异常直接 `unwind to caller` 跳过本 catch。
        self.eh_cleanup_stack.push(cs_label.clone());
        for (i, s) in try_body.iter().enumerate() {
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
            if self.flow_terminated {
                break;
            }
        }
        self.eh_region_stack.pop();
        self.eh_cleanup_stack.pop();
        self.emitting_caught_try_body = prev_caught;
        let try_term = self.flow_terminated;
        if !try_term {
            self.emit(&format!("br label %{after_label}"));
        }

        // catchswitch block: the unwinder routes matched exceptions here.
        // If inside an enclosing try/finally (Windows SEH), unmatched unwind
        // chains to that finally's cleanup funclet; otherwise to caller.
        let cs_val = self.fresh_temp();
        self.emit_label(&cs_label);
        let unwind = match self.eh_cleanup_stack.last() {
            Some(outer) => format!("unwind label %{outer}"),
            None => "unwind to caller".to_string(),
        };
        self.emit(&format!(
            "{cs_val} = catchswitch within none [label %{pad_label}] {unwind}"
        ));
        // catchpad funclet — must contain only the catchpad + a transfer; emit
        // an immediate catchret to a normal block where the catch body lives.
        let cp_val = self.fresh_temp();
        self.emit_label(&pad_label);
        self.emit(&format!(
            "{cp_val} = catchpad within {cs_val} [ptr null, i32 64, ptr null]"
        ));
        self.emit(&format!("catchret from {cp_val} to label %{catch_label}"));

        // Normal catch body block — unrestricted calls allowed.
        self.emit_label(&catch_label);
        let exc = self.fresh_temp();
        self.emit(&format!("{exc} = call ptr @rt_get_exception()"));

        // Zero-cost EH milestone ④ (Windows SEH): catch 类型过滤 (C# 对齐)。
        // catch-all（`Exception`——合成/显式一致）跳过；具体类型在 catch 入口做
        // vtable `is` 检查（rt_obj_isa 沿 parent 链比对 type_id），不匹配 →
        // 以同一异常对象 rethrow 继续 unwind 到外层 handler（SEH 下等价于
        // Itanium landingpad 的 `resume`）。
        let catch_name = match catch_ty {
            TypeId::Named(n) => n.as_str(),
            _ => "Exception",
        };
        if catch_name != "Exception" {
            let is_ok = self.fresh_temp();
            let is_bool = self.fresh_temp();
            let match_label = self.fresh_label();
            let mismatch_label = self.fresh_label();
            // RFC 038 M2：外部异常类型（external_class_names）typeinfo 经守卫登记
            // external 声明，由定义包 linkonce_odr 定义解析。
            let catch_ti = self
                .typeinfo_global(catch_name)
                .unwrap_or_else(|| format!("@.typeinfo.{catch_name}"));
            self.emit(&format!(
                "{is_ok} = call i32 @rt_obj_isa(ptr {exc}, ptr {catch_ti})"
            ));
            self.emit(&format!("{is_bool} = icmp ne i32 {is_ok}, 0"));
            self.emit(&format!(
                "br i1 {is_bool}, label %{match_label}, label %{mismatch_label}"
            ));
            self.emit_label(&mismatch_label);
            // 类型不匹配：rethrow 同一异常对象，继续 unwind。若 catch body 位于
            // 外层 try/finally 区域内，invoke 目标为外层 cleanup（M3 链）。
            self.emit_call_may_throw("void", "@rt_throw", &format!("ptr {exc}"), true, None);
            self.emit("unreachable");
            self.emit_label(&match_label);
        }

        let catch_ptr = self.local_ptr(catch_var);
        self.emit(&format!("store ptr {exc}, ptr {catch_ptr}"));
        // catch 内 rethrow（when 失败）须跑 finally；暂时退出 try-body 模式。
        let prev_caught = self.emitting_caught_try_body;
        self.emitting_caught_try_body = false;
        self.flow_terminated = false;
        for (i, s) in catch_body.iter().enumerate() {
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
            if self.flow_terminated {
                break;
            }
        }
        self.emitting_caught_try_body = prev_caught;
        let catch_term = self.flow_terminated;
        if !catch_term {
            self.emit(&format!("br label %{after_label}"));
        }

        self.emit_label(&after_label);
        self.flow_terminated = saved_ft || (try_term && catch_term);
    }

    /// Zero-cost EH milestone ②: the innermost active Windows catchswitch block
    /// label, if the current emission point sits inside a try region. `None`
    /// outside any try region (POSIX Itanium EH is milestone ⑨).
    fn eh_unwind_label(&self) -> Option<&str> {
        if !self.is_windows {
            return None;
        }
        self.eh_region_stack.last().map(|s| s.as_str())
    }

    /// Whether a callee may unwind (throw an exception).
    ///
    /// - Defined user functions: resolved via the module call-graph `nounwind`
    ///   analysis (`nounwind_map`, RFC 015 Phase B.7).
    /// - Known nounwind externals (`rt_*` whitelist / libc): no.
    /// - Unknown externals / opaque (virtual, iface, indirect, ctor): yes —
    ///   conservative, since we must not mark a frame `nounwind` through which
    ///   an exception may pass.
    pub(super) fn callee_may_throw(&self, callee: &str) -> bool {
        if let Some(&nounwind) = self.nounwind_map.get(callee) {
            return !nounwind;
        }
        !is_known_nounwind_external(callee)
    }

    /// Emit a call instruction. Inside a Windows try region where `may_throw`,
    /// emit `invoke … to label %cont unwind label %cs` so the exception lands in
    /// the region's catchswitch; otherwise a plain `call`. `result_slot`, when
    /// given, is the SSA temp the return value is written into.
    pub(super) fn emit_call_may_throw(
        &mut self,
        ret_ty: &str,
        callee: &str,
        args: &str,
        may_throw: bool,
        result_slot: Option<&str>,
    ) {
        if may_throw {
            if let Some(cs) = self.eh_unwind_label().map(|s| s.to_string()) {
                let cont = self.fresh_label();
                match result_slot {
                    Some(t) => self.emit(&format!(
                        "{t} = invoke {ret_ty} {callee}({args}) to label %{cont} unwind label %{cs}"
                    )),
                    None => self.emit(&format!(
                        "invoke {ret_ty} {callee}({args}) to label %{cont} unwind label %{cs}"
                    )),
                }
                self.emit_label(&cont);
                return;
            }
        }
        match result_slot {
            Some(t) => self.emit(&format!("{t} = call {ret_ty} {callee}({args})")),
            None => self.emit(&format!("call {ret_ty} {callee}({args})")),
        }
    }

    /// L2：在 `rt_throw` 前若 `Exception.StackTrace` 仍为 null，写入 `rt_format_stacktrace()`。
    /// 偏移取自 `Exception` 布局（派生异常同槽位）；无 Exception 布局时跳过（保持 null）。
    pub(super) fn emit_attach_exception_stacktrace(&mut self, ex_val: &str) {
        let offset = match self
            .layouts
            .classes
            .get("Exception")
            .and_then(|c| c.fields.iter().find(|f| f.name == "StackTrace"))
            .map(|f| f.offset)
        {
            Some(o) => o,
            None => return,
        };
        let addr = self.fresh_temp();
        let old = self.fresh_temp();
        let is_null = self.fresh_temp();
        let capture = self.fresh_label();
        let cont = self.fresh_label();
        self.emit(&format!(
            "{addr} = getelementptr inbounds i8, ptr {ex_val}, i32 {offset}"
        ));
        self.emit(&format!("{old} = load ptr, ptr {addr}"));
        self.emit(&format!("{is_null} = icmp eq ptr {old}, null"));
        self.emit(&format!("br i1 {is_null}, label %{capture}, label %{cont}"));
        self.emit_label(&capture);
        let st = self.fresh_temp();
        self.emit(&format!("{st} = call ptr @rt_format_stacktrace()"));
        self.emit(&format!("store ptr {st}, ptr {addr}"));
        self.emit(&format!("br label %{cont}"));
        self.emit_label(&cont);
    }

    /// 发射 void 返回，与 `MirTerminator::Return(None)` 的处理保持一致：
    /// 入口 `main`（LLVM 签名 `i32`）返回 `ret i32 0`；普通 void 函数
    /// `ret void`；非 void 函数的无值 return 是死代码（如 return 语句之后）
    /// → `unreachable` 避免「value doesn't match function result type」。
    fn emit_ret_void(&mut self) {
        if self.is_main {
            self.emit("ret i32 0");
        } else if matches!(self.cfg.ret, TypeId::Void) {
            self.emit("ret void");
        } else {
            self.emit("unreachable");
        }
    }

    /// A1: 在 return/throw 前 inline 执行 `finally_stack` 中所有 finally 块。
    ///
    /// 设计要点：
    /// - clone 栈快照后依次执行，不修改原栈（保持 `emit_try_finally` 的 push/pop 平衡）。
    /// - 外层 finally 先 push、后 pop，故栈底→栈顶 = 外层→内层；return 应按 内层→外层
    ///   顺序释放，此处逆序迭代栈快照。
    /// - finally 块内的 return/throw 会递归调用本方法，但此时栈已 pop（或为空），不会
    ///   重复执行自身，避免无限递归。
    /// - 正常路径下 `emit_try_finally` 在 body 完成后仍会执行一次 finally 块；若 body
    ///   以 return/throw 退出，那段代码为 dead code（ret/unreachable 之后），LLVM 会
    ///   忽略。
    fn emit_finally_chain(&mut self) {
        if self.finally_stack.is_empty() {
            return;
        }
        // 逆序（内层→外层）clone 并执行，避免 borrow checker 抱怨 self 同时被借。
        let snapshots: Vec<Vec<MirStatement>> = self.finally_stack.iter().rev().cloned().collect();
        for block in &snapshots {
            for s in block {
                self.emit_stmt(s);
            }
        }
    }

    /// `try { body } finally { cleanup }` — body 块正常执行后，无条件执行 finally 块。
    /// body 中的 return/throw 会先 inline 执行所有 finally 块再退出（见 emit_stmt 的
    /// Return/Throw 分支）。若 body 以 terminator（ret/unreachable）退出，此处不再
    /// 重复发射 finally（否则在 terminator 后产生无效 LLVM IR）。
    fn emit_try_finally(&mut self, body: &[MirStatement], finally: &[MirStatement]) {
        // Zero-cost EH milestone ③ (Windows SEH): cleanup funclet for deep
        // unwind. POSIX keeps the inline compile-time path until milestone ⑦.
        if self.is_windows {
            self.emit_try_finally_seh(body, finally);
            return;
        }
        // push finally 块到栈（供 body 中的 return/throw inline 执行）
        self.finally_stack.push(finally.to_vec());

        // 记录 body 发射前的输出长度，用于判断 body 是否以 terminator 结束。
        let output_before = self.output.len();

        // body 块：正常执行路径
        for (i, s) in body.iter().enumerate() {
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
        }

        // pop finally 块（若 body 中 return/throw 已 clone 执行过，此处仍 pop 保持栈平衡）
        self.finally_stack.pop();

        // 若 body 以 terminator（ret / unreachable）结束，跳过 finally 块发射。
        // 此时 body 内的 Return/Throw 已通过 emit_finally_chain 执行了 finally
        // 语义，再次发射会在 terminator 后产生无效 LLVM IR。
        let body_emitted = &self.output[output_before..];
        // emit() 追加 `\n`，所以直接用 ends_with 匹配完整行。
        if body_emitted.ends_with("unreachable\n") {
            return;
        }
        // 检查是否以 `ret ...` 结尾（body 中 Return 的 terminator）。
        if body_emitted.contains("ret ")
            && body_emitted.trim_end().ends_with('\n')
            && body_emitted
                .trim_end()
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .starts_with("ret ")
        {
            return;
        }

        // finally 块：body 正常完成后执行
        for (i, s) in finally.iter().enumerate() {
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
        }
    }

    /// Zero-cost EH milestone ③ (Windows SEH): deep-unwind cleanup funclet.
    ///
    /// Shape (matches clang for `try { body } finally { cleanup }` on Windows):
    ///
    /// ```llvm
    /// br label %try
    /// try:                              ; region: may-throw calls become invokes
    ///   invoke ... to label %cont unwind label %cleanup_dispatch
    ///   ...
    ///   ; normal path — inline finally, then continue
    ///   <cleanup statements>
    ///   br label %after
    /// cleanup_dispatch:                 ; deep-unwind path
    ///   %cp = cleanuppad within none []
    ///   <cleanup statements>
    ///   cleanupret from %cp unwind to caller | unwind label %outer_cleanup
    /// after:
    /// ```
    ///
    /// The finally body is emitted twice: inline on the normal (zero-cost) path
    /// and inside the funclet for the unwind path. Nested finally regions chain
    /// via `cleanupret unwind label %outer` (tracked by `eh_cleanup_stack`), and
    /// an enclosing try/catch's catchswitch unwinds to the nearest finally
    /// cleanup as well. M3 scope: a finally body that itself performs a
    /// may-throw call or contains `return`/`throw` would need a nested funclet
    /// and is not yet lowered (documented limitation).
    fn emit_try_finally_seh(&mut self, body: &[MirStatement], finally: &[MirStatement]) {
        let try_label = self.fresh_label();
        let cleanup_label = self.fresh_label();
        let after_label = self.fresh_label();

        self.emit(&format!("br label %{try_label}"));
        self.emit_label(&try_label);

        // finally 块仍推入 finally_stack：body 中 `return` 走 inline 语义
        //（正常完成，不经过 funclet）；`throw` 在 SEH 下由 funclet 处理，
        // 见 emit_stmt 的 Throw 分支（Windows 跳过 emit_finally_chain）。
        self.finally_stack.push(finally.to_vec());
        self.eh_region_stack.push(cleanup_label.clone());
        self.eh_cleanup_stack.push(cleanup_label.clone());

        let saved_ft = self.flow_terminated;
        self.flow_terminated = false;
        for (i, s) in body.iter().enumerate() {
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
            if self.flow_terminated {
                break;
            }
        }
        self.eh_region_stack.pop();
        self.eh_cleanup_stack.pop();
        self.finally_stack.pop();
        let body_term = self.flow_terminated;
        self.flow_terminated = saved_ft;

        if !body_term {
            // 正常路径：inline 执行 finally，然后继续。
            for (i, s) in finally.iter().enumerate() {
                self.stmt_path.push(i);
                self.emit_stmt(s);
                self.stmt_path.pop();
                if self.flow_terminated {
                    break;
                }
            }
            if !self.flow_terminated {
                self.emit(&format!("br label %{after_label}"));
            }
        }

        // 深层 unwind 路径：cleanup funclet（正常路径不可达）。
        self.emit_label(&cleanup_label);
        let cp = self.fresh_temp();
        self.emit(&format!("{cp} = cleanuppad within none []"));
        self.flow_terminated = false;
        let funclet_body_start = self.output.len();
        for (i, s) in finally.iter().enumerate() {
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
            if self.flow_terminated {
                break;
            }
        }
        let funclet_body_end = self.output.len();
        self.annotate_funclet_calls(funclet_body_start, funclet_body_end, &cp);
        self.flow_terminated = false;
        let unwind = match self.eh_cleanup_stack.last() {
            Some(outer) => format!("unwind label %{outer}"),
            None => "unwind to caller".to_string(),
        };
        self.emit(&format!("cleanupret from {cp} {unwind}"));

        self.emit_label(&after_label);
        // after 仅当 body 正常完成时可达；body 以 terminator 结束时不发射。
    }

    /// Whether a place holds an ARC-managed class reference (strong ref that
    /// must be inc'd on copy / dec'd on overwrite). Excludes opaque runtime
    /// handles (Thread/Lock/…): no ArcHeader, so `rt_arc_inc`/`dec` would
    /// corrupt the leading fields. Includes `Nullable<T>` where `T` is a class
    /// (represented as a `ptr` slot, mirroring the existing retain path).
    pub(super) fn arc_class_place(ty: &TypeId, layouts: &typeck::ProgramLayouts) -> bool {
        match ty {
            // Object / Nullable{Object} 槽位按 class 计 ARC（设计裁决：选项 A，
            // 2026-09-05 人裁决采纳）。前提已满足：raw/λ 路径 string→object 实参
            // 装箱补齐（maybe_box_string_to_object，6d44d87e）后 object 槽内均为
            // 带 ArcHeader 的 box/class 实例——typed-inject 注册表借引用 over-dec
            // 根治（选项 B 为保留零计数借用语义的备选，见 CHANGELOG 9/5 登记）。
            // 若再有 raw 串入 object 槽，inc/dec 会把字符串内容当 refcount 原子写
            // → 0xC0000005（VEH 取证），回归时以 chord corpus + workspace 门禁兜底。
            TypeId::Object => true,
            TypeId::Named(n) => {
                layouts.classes.contains_key(n.as_str())
                    && !is_opaque_runtime_handle(n.as_str())
                    && !is_generic_template_name(n.as_str())
            }
            TypeId::Nullable { inner } => match inner.as_ref() {
                TypeId::Object => true,
                TypeId::Named(n) => {
                    layouts.classes.contains_key(n.as_str())
                        && !is_opaque_runtime_handle(n.as_str())
                        && !is_generic_template_name(n.as_str())
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// 静态字段类型字符串 → TypeId（`StaticFieldSet` 与 `MirOperand::StaticField`
    /// load 的 llvm 类型映射共用；与 layouts.static_fields 的 `ty` 格式对齐）。
    fn static_field_type_id(&self, ty_str: &str) -> TypeId {
        match ty_str {
            "int" => TypeId::Int,
            "long" => TypeId::Long,
            "short" => TypeId::Short,
            "byte" => TypeId::Byte,
            "uint" => TypeId::UInt,
            "ushort" => TypeId::UShort,
            "sbyte" => TypeId::SByte,
            "char" => TypeId::Char,
            "bool" => TypeId::Bool,
            "float" => TypeId::Float,
            "double" => TypeId::Double,
            "string" => TypeId::String,
            "void" => TypeId::Void,
            "object" => TypeId::Object,
            other => TypeId::Named(other.into()),
        }
    }

    /// Assign 是否应对 ARC 管理的 class 局部拷贝做 retain（非所有权移交）。
    ///
    /// **不含 string**：字面量/常量串常为 rodata 指针，无 ArcHeader；对其
    /// `rt_arc_inc` 会 ACCESS_VIOLATION。string 拷贝 ARC 另轨。
    ///
    /// 产生 **借引用 class 值** 的 rvalue 一律 retain：
    /// - 局部/字段/静态字段/FieldGet 直接拷贝；
    /// - Ternary / Coalesce：被选中分支可能是局部/字段（借引用）——retain
    ///   使目标局部持独立 +1（new 分支的物化临时局部在 epilogue dec 与
    ///   本 inc 配对，不泄漏）。缺 retain 时 `m = meta != null ? meta : new X()`
    ///   借引用 meta 被调用方退出 dec 提前释放 → 0xC0000374。
    /// - IndexGet：数组元素为借引用（数组持有所有权）。
    ///
    /// `new`/`typeof`/Call/MethodCall 移交所有权（生产者返回 owned ref）不 retain。
    pub(crate) fn assign_needs_arc_retain(
        rvalue: &MirRvalue,
        place_ty: &TypeId,
        layouts: &typeck::ProgramLayouts,
    ) -> bool {
        if !Self::arc_class_place(place_ty, layouts) {
            return false;
        }
        matches!(
            rvalue,
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

    /// Return 的借引用源（字段/静态字段/参数局部）在 epilogue drop 前求值并
    /// `rt_arc_inc`，返回 `(ty, val)` 供 ret 复用。
    ///
    /// 所有权契约：函数返回的值对调用方必须是 **owned ref**（调用方 epilogue
    /// 会 dec 持有的 class 局部）。字段/静态字段/参数均为借用（引用计数由
    /// 真正持有者计），直接返回会把「未拥有的引用」交给调用方 → 调用方 dec
    /// 时提前释放（如 `FindDescriptor` 返回 entry.Descriptor、`TryGateTool`
    /// 存 local 后退出 → rc=1 对象被 free → 后续 inc 悬垂 → 0xC0000374）。
    /// 非参数局部已建立所有权（`new`/Call 移交、拷贝 retain），转移不 inc，
    /// 故工厂对象（rc=1 新对象）不泄漏。
    ///
    /// 求值必须先于 `emit_sync_epilogue_drops`：`return e.Descriptor` 的
    /// `e` 是本地对象，若先 drop `e` 再读字段 → 悬垂读（靠内存未复用侥幸存活）。
    fn maybe_emit_return_inc(
        &mut self,
        op: &MirOperand,
        ret_ty: &TypeId,
    ) -> Option<(String, String)> {
        if !Self::arc_class_place(ret_ty, self.layouts) {
            return None;
        }
        let borrowed = match op {
            MirOperand::Field { .. } | MirOperand::StaticField { .. } => true,
            MirOperand::Local(id) => (id.0 as usize) < self.cfg.param_count,
            _ => false,
        };
        if !borrowed {
            return None;
        }
        let (ty, val) = self.emit_operand(op);
        self.emit(&format!("call void @rt_arc_inc(ptr {val})"));
        Some((ty, val))
    }

    // `emit_linq_foreach` lives in `linq_foreach.rs` (separate module).
}

impl<'a> FnEmitter<'a> {
    // ---- RFC 037 M-D0：`[Observable]` auto-property 通知合成 ----

    /// 发射 `[Observable]` auto-property 的合成 setter 逻辑（FieldSet 内联）。
    ///
    /// 可编译期值比较的类型（基础标量）走**相等性短路**：新值 == 旧值 →
    /// 跳过 store 与通知（RFC 037 §5.3「相等性短路」）；不可比较类型
    /// （string/class/泛型/struct 等）回退**无条件通知**（正确性优先——
    /// 通知可能多余但绝不少发）。`field` 选定该属性的通道槽——**每属性
    /// 一槽**（`observable_channel_offset(class, field)`），多属性类不再
    /// 共享同一槽（2026-08-04 修复）。
    fn emit_observable_property_set(
        &mut self,
        obj: &str,
        field_ty: &str,
        store_ty: &str,
        store_val: &str,
        addr: &str,
        class: &str,
        field: &str,
    ) {
        match observable_eq_compare(field_ty) {
            Some(op) => {
                let old = self.fresh_temp();
                let eq = self.fresh_temp();
                self.emit(&format!("{old} = load {store_ty}, ptr {addr}"));
                self.emit(&format!("{eq} = {op} {store_ty} {old}, {store_val}"));
                let skip = self.fresh_label();
                let upd = self.fresh_label();
                self.emit(&format!("br i1 {eq}, label %{skip}, label %{upd}"));
                self.emit_label(&upd);
                self.emit_field_store(field_ty, store_ty, store_val, addr, None);
                self.emit_observable_notify(obj, field_ty, store_ty, store_val, class, field);
                self.emit(&format!("br label %{skip}"));
                self.emit_label(&skip);
            }
            None => {
                // 回退：无条件通知（正确性优先，见函数文档）。
                self.emit_field_store(field_ty, store_ty, store_val, addr, None);
                self.emit_observable_notify(obj, field_ty, store_ty, store_val, class, field);
            }
        }
    }

    /// RFC 005 自动 Copy：Copy 型 struct 的赋值发射——私有副本 alloca + 聚合
    /// load/store 整体拷贝 + 目标槽改存新副本指针。返回 true 表示已拦截。
    ///
    /// 全语言用户 struct 统一 ptr 表示（RFC 012 S6 A1）：普通 `store ptr` 是
    /// 指针替换，会让源/目标槽别名共享同一 storage，违背 RFC 005 Copy 语义
    /// （逐字段复制、源仍可用）。拷贝落为「私有 alloca → 聚合 load+store →
    /// 槽存新副本 ptr」。`hoist_to_entry`：CFG 块内的赋值须把 alloca 提升至
    /// entry 块（`entry_allocas`，循环内复用同一副本，每次执行整体覆盖 + 槽
    /// 同步改指，无跨赋值点别名）；形参 store 天然在 entry 块文本序，走提升
    /// 会出现使用先于定义（flush 在形参区之后），须直接发射。
    pub(crate) fn try_emit_copy_struct_store(
        &mut self,
        struct_name: &str,
        src_ptr: &str,
        dest_slot: &str,
        tbaa: Option<u32>,
        hoist_to_entry: bool,
    ) -> bool {
        if !self.layouts.is_copy_struct(struct_name) {
            return false;
        }
        let agg = format!("%struct.{struct_name}");
        let copy = self.fresh_temp();
        if hoist_to_entry {
            self.entry_allocas
                .push_str(&format!("  {copy} = alloca {agg}\n"));
        } else {
            self.emit(&format!("{copy} = alloca {agg}"));
        }
        let v = self.fresh_temp();
        self.emit(&format!("{v} = load {agg}, ptr {src_ptr}"));
        self.emit(&format!("store {agg} {v}, ptr {copy}"));
        let tbaa_suffix = tbaa.map(|t| format!(", !tbaa !{t}")).unwrap_or_default();
        self.emit(&format!("store ptr {copy}, ptr {dest_slot}{tbaa_suffix}"));
        true
    }

    /// 发射字段 store（含类类型字段的 ARC 覆写维护）。
    ///
    /// 从 FieldSet 内联逻辑抽取，供普通字段与 `[Observable]` 合成路径复用，
    /// 行为与原实现完全一致。`tbaa` 为 045 M5 用户 struct 字段访问 tag（None
    /// 表示不挂，覆盖 class-typed 字段的 ARC 读写路径——它们不挂 TBAA）。
    fn emit_field_store(
        &mut self,
        field_ty: &str,
        store_ty: &str,
        store_val: &str,
        addr: &str,
        tbaa: Option<u32>,
    ) {
        // ARC maintenance for class-typed fields: inc new, load old,
        // store new, dec old. Balances the drop sequence's field-dec
        // (arc_drop.rs). Safe because emit_new uses calloc (fields
        // start as null → dec-old is no-op) and rt_arc_inc/dec are
        // null-safe. Non-class fields (int, bool, etc.) skip ARC.
        // Opaque runtime handles (Lock/Mutex/…) have no ArcHeader —
        // FieldSet must not inc/dec them (corrupts CRITICAL_SECTION).
        // RFC 004 生命周期（D3）：variant 字段值深拷贝到堆——构造 alloca 随
        // 创建帧消亡，裸存指针会悬垂（SetterValue 回读坏数据根因）。
        let store_val = if self.layouts.variants.contains_key(field_ty) && store_ty == "ptr" {
            self.emit_variant_deep_copy(field_ty, store_val)
        } else {
            store_val.to_string()
        };
        let tbaa_suffix = tbaa.map(|t| format!(", !tbaa !{t}")).unwrap_or_default();
        if self.layouts.classes.contains_key(field_ty) && !is_opaque_runtime_handle(field_ty) {
            self.emit(&format!("call void @rt_arc_inc(ptr {store_val})"));
            let old = self.fresh_temp();
            self.emit(&format!("{old} = load ptr, ptr {addr}"));
            self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
            self.emit(&format!("call void @rt_arc_dec(ptr {old})"));
        } else if self.layouts.is_copy_struct(field_ty) && store_ty == "ptr" {
            // RFC 004 生命周期（D3 struct 字段版）：字段槽属于宿主堆对象，寿命
            // 长于本帧——RFC 005 Copy 的栈副本 alloca 随本帧消亡，裸存栈地址
            // 即悬垂（ctor `this.Id = id` 返回后回读 `a.Id` 解引用栈尸 → AV）。
            // 副本落堆：calloc(sizeof) + 聚合
            // 拷贝；槽内旧副本为本字段私有（Copy 语义每次写入新建），free 安全
            // （Copy struct 字段闭包无 class 句柄，无内部引用需 dec）；
            // 零初始化槽首写 free(null) 为 no-op。
            let old = self.fresh_temp();
            self.emit(&format!("{old} = load ptr, ptr {addr}"));
            let size = self.fresh_temp();
            self.emit(&format!(
                "{size} = ptrtoint ptr getelementptr (%struct.{field_ty}, ptr null, i32 1) to i64"
            ));
            let heap = self.fresh_temp();
            self.emit(&format!("{heap} = call ptr @calloc(i64 1, i64 {size})"));
            let v = self.fresh_temp();
            self.emit(&format!("{v} = load %struct.{field_ty}, ptr {store_val}"));
            self.emit(&format!("store %struct.{field_ty} {v}, ptr {heap}"));
            self.emit(&format!("store ptr {heap}, ptr {addr}{tbaa_suffix}"));
            self.emit(&format!("call void @free(ptr {old})"));
        } else if !self.try_emit_copy_struct_store(field_ty, &store_val, addr, tbaa, true) {
            // RFC 005 自动 Copy：struct 字段赋值同样落独立副本（字段槽存新
            // 副本 ptr，与 emit_struct_lit 的字段槽 ptr 语义一致）。
            self.emit(&format!(
                "store {store_ty} {store_val}, ptr {addr}{tbaa_suffix}"
            ));
        }
    }

    /// 发射隐藏通知通道的惰性创建与通知发送（RFC 016 §4.2「编译期符号」）。
    ///
    /// 通道为**每实例、每属性**的 `Signal<T>`，槽字段（`ptr`）位于类布局末
    /// （`observable_channel_offset(class, field)`，每 `[Observable]` 属性一
    /// 槽）。首次 set 时惰性 `new Signal<T>()`（`calloc` + `__ctor::Signal_<T>`）；
    /// 值变化后调 `Signal_<T>_Set` 发送通知。字段按**编译期符号静态定址**
    /// （GEP 常量偏移 + 静态符号 `@Signal_<T>_Set`），绝无运行期字符串查找。
    ///
    /// 观察者入口 `ObserveProperty` 的**编译期识别与隐藏通道直访已落地**
    /// （`try_emit_observable_observe`，2026-08-04，IR 断言）；运行期订阅注册
    /// （`.Subscribe(handler)` 闭包链路）与 `data_driven_property_e2e`
    /// 运行期验收属 M-D0 后续切片（依赖 lambda 链接修复）。
    fn emit_observable_notify(
        &mut self,
        obj: &str,
        field_ty: &str,
        store_ty: &str,
        store_val: &str,
        class: &str,
        field: &str,
    ) {
        let signal_class = format!("Signal_{field_ty}");
        let ch_addr = self.fresh_temp();
        let offset = self.layouts.observable_channel_offset(class, field);
        self.emit(&format!(
            "{ch_addr} = getelementptr inbounds i8, ptr {obj}, i64 {offset}"
        ));
        let ch = self.fresh_temp();
        self.emit(&format!("{ch} = load ptr, ptr {ch_addr}"));
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {ch}, null"));
        let laz = self.fresh_label();
        let ready = self.fresh_label();
        self.emit(&format!("br i1 {is_null}, label %{laz}, label %{ready}"));
        self.emit_label(&laz);
        // 惰性创建：`new Signal<T>()`。`Signal_<T>` 由 typeck 泛型单态化
        // 提供（`__ctor::Signal_<T>` 与 `Set`），此处仅按符号发射调用。
        let (_, sig) = self.emit_new(&signal_class, &[], &[]);
        self.emit(&format!("store ptr {sig}, ptr {ch_addr}"));
        self.emit(&format!("br label %{ready}"));
        self.emit_label(&ready);
        let ch_ready = self.fresh_temp();
        self.emit(&format!("{ch_ready} = load ptr, ptr {ch_addr}"));
        // 通知发送：`Signal<T>.Set(value)` → TrySet → NotifyChanged。
        // 可拒绝语义（changing handler 否决时回滚 backing field）留待后续切片。
        let callee = mangle_method(&signal_class, "Set");
        let may_throw = self.callee_may_throw(&format!("{signal_class}::Set"));
        self.emit_call_may_throw(
            "void",
            &format!("@{callee}"),
            &format!("ptr {ch_ready}, {store_ty} {store_val}"),
            may_throw,
            None,
        );
    }

    /// 发射隐藏通道字段的**静态定址直访 + 惰性创建**，返回就绪通道 `ptr`。
    ///
    /// 与 [`FnEmitter::emit_observable_notify`] 的惰性创建同构（同一通道槽：
    /// `observable_channel_offset(class, field)`）：GEP 常量偏移 → 判空 →
    /// 首次访问时 `new Signal<T>()`（calloc + `__ctor::Signal_<T>`）回填通道槽。
    /// 观察者入口 `ObserveProperty` 复用它取得可订阅句柄。
    fn emit_observable_channel_lazy(
        &mut self,
        obj: &str,
        class: &str,
        field: &str,
        signal_class: &str,
    ) -> String {
        let ch_addr = self.fresh_temp();
        let offset = self.layouts.observable_channel_offset(class, field);
        self.emit(&format!(
            "{ch_addr} = getelementptr inbounds i8, ptr {obj}, i64 {offset}"
        ));
        let ch = self.fresh_temp();
        self.emit(&format!("{ch} = load ptr, ptr {ch_addr}"));
        let is_null = self.fresh_temp();
        self.emit(&format!("{is_null} = icmp eq ptr {ch}, null"));
        let laz = self.fresh_label();
        let ready = self.fresh_label();
        self.emit(&format!("br i1 {is_null}, label %{laz}, label %{ready}"));
        self.emit_label(&laz);
        let (_, sig) = self.emit_new(signal_class, &[], &[]);
        self.emit(&format!("store ptr {sig}, ptr {ch_addr}"));
        self.emit(&format!("br label %{ready}"));
        self.emit_label(&ready);
        let ch_ready = self.fresh_temp();
        self.emit(&format!("{ch_ready} = load ptr, ptr {ch_addr}"));
        ch_ready
    }

    /// RFC 037 M-D0：观察者入口 `ObserveProperty("Name")` 的 codegen 变换。
    ///
    /// 编译器在含 `[Observable]` auto-property 的类上合成实例方法
    /// `Signal<T> ObserveProperty(string symbol)`；typeck 已保证接收者类
    /// 含隐藏通知通道、实参为命名某 `[Observable]` auto-property 的编译期
    /// 字符串字面量。此处将调用展开为隐藏通道字段的静态定址直访：GEP 常量
    /// 偏移 + 惰性 `new Signal<T>()`，返回该 `Signal<T>` 句柄（`ptr`）——
    /// 绝无运行期字符串查找/解析（RFC 016 §16 非目标 1）。
    ///
    /// 非本入口形态（类无合成通道 / 实参非常量字符串 / 命名属性非
    /// `[Observable]` auto-property / 属性缺失）返回 `None` 交回常规发射
    /// 路径——typeck 应已拒绝，此处仅为防御性降级。
    pub(super) fn try_emit_observable_observe(
        &mut self,
        receiver: &MirOperand,
        args: &[MirOperand],
        receiver_type: &str,
    ) -> Option<TyVal> {
        if !self.layouts.class_has_observable_channel(receiver_type) {
            return None;
        }
        let MirOperand::ConstString(prop) = args.first()? else {
            return None;
        };
        // 仅 `[Observable]` auto-property 有合成隐藏通道槽；命名普通属性时
        // 不变换（交回常规路径，typeck 应已拒绝）。
        if !self
            .layouts
            .has_observable_property(receiver_type, prop.as_str())
        {
            return None;
        }
        let class = self.layouts.classes.get(receiver_type)?;
        let prop_ty = class
            .declared_properties
            .iter()
            .find(|p| p.name.as_str() == prop.as_str())
            .map(|p| p.property_type.as_str().to_string())?;
        let signal_class = format!("Signal_{prop_ty}");
        let (_, recv) = self.emit_operand(receiver);
        let ch =
            self.emit_observable_channel_lazy(&recv, receiver_type, prop.as_str(), &signal_class);
        Some(("ptr".into(), ch))
    }

    /// RFC 037 M-D0：通知侧入口 `NotifyPropertyChanged("Name")` 的 codegen 变换。
    ///
    /// 编译器在含 `[Observable]` 属性（auto 或 custom-accessor）的类上合成
    /// 实例方法 `void NotifyPropertyChanged(string symbol)`（§5.3 场景 6）；
    /// typeck 已保证接收者类含隐藏通知通道、实参为命名某 `[Observable]`
    /// 可读属性的编译期字符串字面量。此处将调用展开为**显式 raise**：
    /// 定位隐藏通道槽（`observable_channel_offset`）→ 惰性读取通道
    /// `Signal<T>` 实例（复用 `emit_observable_channel_lazy`）→ 读取当前
    /// 属性值（auto-property 直接 field load；custom-accessor 调属性 getter）
    /// → 调 `Signal_<T>.Set(当前值)`。
    ///
    /// **相等性短路语义**：**无短路**——显式 raise 无视值相等、无条件通知
    ///（开发者手动控制；对齐 `Signal.Set` 自身语义，而非 auto-property setter
    /// 合成的相等性短路）。依据 RFC 037 §5.3 场景 6：`NotifyPropertyChanged`
    /// 无「新值」实参，其语义是「重新发送当前值」；custom-accessor 开发者
    /// 期望每次显式调用都触发（WPF INotifyPropertyChanged 惯例），如需短路
    /// 可自行 `if (field != value)` 包裹。
    ///
    /// 非本入口形态（类无合成通道 / 实参非常量字符串 / 命名属性非
    /// `[Observable]`）返回 `None` 交回常规发射路径——typeck 应已拒绝，
    /// 此处仅为防御性降级。
    pub(super) fn try_emit_observable_notify(
        &mut self,
        receiver: &MirOperand,
        args: &[MirOperand],
        receiver_type: &str,
    ) -> Option<TyVal> {
        if !self.layouts.class_has_observable_channel(receiver_type) {
            return None;
        }
        let MirOperand::ConstString(prop) = args.first()? else {
            return None;
        };
        // 仅 `[Observable]` 属性有合成隐藏通道槽；命名普通属性时不变换
        //（交回常规路径，typeck 应已拒绝）。
        if !self
            .layouts
            .has_observable_property(receiver_type, prop.as_str())
        {
            return None;
        }
        let class = self.layouts.classes.get(receiver_type)?;
        let prop_ty = class
            .declared_properties
            .iter()
            .find(|p| p.name.as_str() == prop.as_str())
            .map(|p| p.property_type.as_str().to_string())?;
        let signal_class = format!("Signal_{prop_ty}");
        let (_, recv) = self.emit_operand(receiver);
        let ch =
            self.emit_observable_channel_lazy(&recv, receiver_type, prop.as_str(), &signal_class);
        // 当前值读取：
        // - auto-property（类布局含同名 backing field）→ 直接 field load
        //   （与 FieldSet/`emit_field_get` 同一 GEP 常量偏移 + 存储类型约定）；
        // - custom-accessor（无同名 field，有逻辑 getter）→ 调属性 getter
        //   `@<Class>_get_<Name>(ptr this)`（返回类型与 emit_fn 的返回类型
        //   计算一致：`llvm_type_of(demangle_simple_type_part(prop_ty))`）。
        let (value_ty, value) = if class.fields.iter().any(|f| f.name == prop.as_str()) {
            let (offset, field_ty) = self.field_info(receiver_type, prop.as_str());
            let addr = self.fresh_temp();
            self.emit(&format!(
                "{addr} = getelementptr inbounds i8, ptr {recv}, i32 {offset}"
            ));
            let llvm_ty = llvm_field_type(&field_ty, self.layouts);
            let val = self.fresh_temp();
            self.emit(&format!("{val} = load {llvm_ty}, ptr {addr}"));
            (llvm_ty, val)
        } else {
            let llvm_ty = llvm_type_of(
                &super::types::demangle_simple_type_part(&prop_ty),
                self.layouts,
            );
            let getter = mangle_method(receiver_type, &format!("get_{prop}"));
            let may_throw = self.callee_may_throw(&format!("{receiver_type}::get_{prop}"));
            let val = self.fresh_temp();
            self.emit_call_may_throw(
                &llvm_ty,
                &format!("@{getter}"),
                &format!("ptr {recv}"),
                may_throw,
                Some(&val),
            );
            (llvm_ty, val)
        };
        // 显式 raise：`Signal<T>.Set(当前值)` → TrySet → NotifyChanged。
        // 与 setter 合成 `emit_observable_notify` 的发射一致（`@Signal_<T>_Set`）。
        let callee = mangle_method(&signal_class, "Set");
        let may_throw = self.callee_may_throw(&format!("{signal_class}::Set"));
        self.emit_call_may_throw(
            "void",
            &format!("@{callee}"),
            &format!("ptr {ch}, {value_ty} {value}"),
            may_throw,
            None,
        );
        Some(("void".into(), String::new()))
    }
}

/// RFC 037 M-D0：属性类型能否做编译期相等性短路。
///
/// 支持全部基础标量（整型/布尔/字符经 `icmp eq`，浮点经 `fcmp oeq`）。
/// string/class/泛型/struct 等无法编译期值比较 → 返回 `None`，
/// 调用方回退无条件通知（见 [`FnEmitter::emit_observable_property_set`]）。
fn observable_eq_compare(field_ty: &str) -> Option<&'static str> {
    match field_ty {
        "int" | "long" | "short" | "byte" | "char" | "bool" => Some("icmp eq"),
        "float" | "double" => Some("fcmp oeq"),
        _ => None,
    }
}

/// POSIX try/catch 编译门（`arc-eh-001`）。
///
/// Windows SEH 是 1.0 唯一实现的 zero-cost EH 面；非 Windows 目标上
/// MIR 中出现的任何 `try/catch`——无论嵌套位置——都应在此给出结构化
/// 编译错误，而不是落到 [`FnEmitter::emit_try_catch`] 深处 ICE。本门由
/// `ModuleEmitter::emit_module` 在发射任何函数体前调用：作用域与触发面
/// 完全一致（发射即可达），Windows 目标行为不变（零路径开销）。
pub(super) fn reject_try_catch_outside_windows(
    fns: &[(String, MirCfgBody)],
    is_windows: bool,
    file_path: &str,
) -> Result<(), crate::CodegenError> {
    if is_windows {
        return Ok(());
    }
    if let Some((name, body)) = fns.iter().find(|(_, b)| body_contains_try_catch(b)) {
        let display = match &body.owner {
            Some(owner) => format!("{owner}::{name}"),
            None => name.clone(),
        };
        return Err(crate::CodegenError::UnsupportedTryCatch(format!(
            "arc-eh-001: 非 Windows 目标不支持 try/catch——Windows SEH 是 1.0 唯一 \
             zero-cost EH 实现面，POSIX Itanium 属里程碑⑨（1.1+，RFC 010）；函数 \
             `{display}`（{file_path}）含 try/catch，请改在 Windows 目标构建或移除该构造"
        )));
    }
    Ok(())
}

/// 递归扫描语句树是否含 `TryCatch`（穿透 `If`/`While`/`LinqForeach`/
/// `TryFinally` 的嵌套体；`TryCatch` 本身恒为命中）。
fn statements_contain_try_catch(stmts: &[MirStatement]) -> bool {
    stmts.iter().any(|s| match s {
        MirStatement::TryCatch { .. } => true,
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => statements_contain_try_catch(then_body) || statements_contain_try_catch(else_body),
        MirStatement::While { body, .. } | MirStatement::LinqForeach { body, .. } => {
            statements_contain_try_catch(body)
        }
        MirStatement::TryFinally { body, finally } => {
            statements_contain_try_catch(body) || statements_contain_try_catch(finally)
        }
        _ => false,
    })
}

/// 函数体（全 CFG 块）是否含 `try/catch`。
fn body_contains_try_catch(body: &MirCfgBody) -> bool {
    body.blocks
        .values()
        .any(|b| statements_contain_try_catch(&b.statements))
}

#[cfg(test)]
mod observable_synth_tests {
    use super::*;
    use crate::EmitRole;
    use crate::GenerateToTable;
    use indexmap::{IndexMap, IndexSet};
    use mir::{
        BlockId, LocalId, MirBlock, MirCfgBody, MirOperand, MirRvalue, MirStatement, MirTerminator,
    };
    use typeck::{ClassLayout, FieldLayout, ProgramLayouts, PropertyLayout};

    fn vm_class(field_ty: &str) -> ClassLayout {
        ClassLayout {
            name: "VM".into(),
            fields: vec![FieldLayout {
                name: "Name".into(),
                ty: field_ty.into(),
                offset: 16,
            }],
            parent: None,
            interfaces: vec![],
            method_impl: Default::default(),
            virtual_slots: vec![],
            has_vtable: false,
            constructors: vec![],
            declared_methods: vec![],
            declared_properties: vec![PropertyLayout {
                name: "Name".into(),
                property_type: field_ty.into(),
                can_read: true,
                can_write: true,
            }],
        }
    }

    /// 双 `[Observable]` 属性类：int Count + string Name。M-D0 多属性共享单
    /// 通道槽 P0 缺陷的复现场景（修复后每属性一槽）。
    ///
    /// 布局：16 头 + Count（int，偏移 16）+ Name（ptr，偏移 24）→ 末字段末尾
    /// 32；通道槽区 align8(32)=32：Count 槽 32、Name 槽 40，calloc = 48。
    fn vm_multi_class() -> ClassLayout {
        ClassLayout {
            name: "VM".into(),
            fields: vec![
                FieldLayout {
                    name: "Count".into(),
                    ty: "int".into(),
                    offset: 16,
                },
                FieldLayout {
                    name: "Name".into(),
                    ty: "string".into(),
                    offset: 24,
                },
            ],
            parent: None,
            interfaces: vec![],
            method_impl: Default::default(),
            virtual_slots: vec![],
            has_vtable: false,
            constructors: vec![],
            declared_methods: vec![],
            declared_properties: vec![
                PropertyLayout {
                    name: "Count".into(),
                    property_type: "int".into(),
                    can_read: true,
                    can_write: true,
                },
                PropertyLayout {
                    name: "Name".into(),
                    property_type: "string".into(),
                    can_read: true,
                    can_write: true,
                },
            ],
        }
    }

    fn layouts(observable: bool, field_ty: &str) -> ProgramLayouts {
        let observable_properties = if observable {
            IndexSet::from([("VM".into(), "Name".into())])
        } else {
            IndexSet::new()
        };
        ProgramLayouts {
            classes: IndexMap::from([("VM".into(), vm_class(field_ty))]),
            structs: Default::default(),
            enums: Default::default(),
            enum_variants: Default::default(),
            interfaces: Default::default(),
            variants: Default::default(),
            static_fields: Default::default(),
            observable_properties,
            type_full_names: Default::default(),
        }
    }

    /// 双属性类布局：Count（int）+ Name（string）均 `[Observable]`。
    /// 规范序按属性名升序：Count → k=0（槽 32）、Name → k=1（槽 40）。
    fn multi_layouts() -> ProgramLayouts {
        ProgramLayouts {
            classes: IndexMap::from([("VM".into(), vm_multi_class())]),
            structs: Default::default(),
            enums: Default::default(),
            enum_variants: Default::default(),
            interfaces: Default::default(),
            variants: Default::default(),
            static_fields: Default::default(),
            observable_properties: IndexSet::from([
                ("VM".into(), "Count".into()),
                ("VM".into(), "Name".into()),
            ]),
            type_full_names: Default::default(),
        }
    }

    fn field_set_body(value: MirRvalue) -> MirCfgBody {
        let mut blocks = IndexMap::new();
        blocks.insert(
            BlockId(0),
            MirBlock {
                id: BlockId(0),
                statements: vec![
                    // `new VM()`：验证 observable 类的 calloc 尺寸含通道字段。
                    MirStatement::Assign {
                        place: LocalId(1),
                        rvalue: MirRvalue::New {
                            class: "VM".into(),
                            args: vec![],
                            ctor_params: vec![],
                        },
                    },
                    // `vm.Name = <value>`：合成 setter 的发射点。
                    MirStatement::FieldSet {
                        object: MirOperand::Local(LocalId(1)),
                        class: "VM".into(),
                        field: "Name".into(),
                        value,
                    },
                ],
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: ast::TypeId::Void,
            param_count: 0,
            locals: IndexMap::from([
                (LocalId(0), ("v".into(), ast::TypeId::Named("VM".into()))),
                (LocalId(1), ("vm".into(), ast::TypeId::Named("VM".into()))),
            ]),
            entry: BlockId(0),
            blocks,
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: mir::Linkage::External,
            parallelize: false,
            loop_backedges: std::collections::HashSet::new(),
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        }
    }

    /// try/catch 门测试体：`blocks[0]` 内含深嵌套 try/catch
    /// （While → TryFinally → If → TryCatch），覆盖递归扫描路径。
    fn try_catch_body() -> MirCfgBody {
        let mut blocks = IndexMap::new();
        blocks.insert(
            BlockId(0),
            MirBlock {
                id: BlockId(0),
                statements: vec![MirStatement::While {
                    cond: MirRvalue::Use(MirOperand::ConstInt(0)),
                    body: vec![MirStatement::TryFinally {
                        body: vec![MirStatement::If {
                            cond: MirOperand::ConstInt(0),
                            then_body: vec![MirStatement::TryCatch {
                                try_body: vec![],
                                catch_var: LocalId(0),
                                catch_ty: ast::TypeId::Void,
                                catch_body: vec![],
                            }],
                            else_body: vec![],
                        }],
                        finally: vec![],
                    }],
                    foreach_source: None,
                }],
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: ast::TypeId::Void,
            param_count: 0,
            locals: IndexMap::new(),
            entry: BlockId(0),
            blocks,
            is_async: false,
            owner: Some("C".into()),
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: mir::Linkage::External,
            parallelize: false,
            loop_backedges: std::collections::HashSet::new(),
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        }
    }

    /// 无 try/catch 的普通函数体（门放行基线）。
    fn plain_body() -> MirCfgBody {
        let mut blocks = IndexMap::new();
        blocks.insert(
            BlockId(0),
            MirBlock {
                id: BlockId(0),
                statements: vec![],
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: ast::TypeId::Void,
            param_count: 0,
            locals: IndexMap::new(),
            entry: BlockId(0),
            blocks,
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: mir::Linkage::External,
            parallelize: false,
            loop_backedges: std::collections::HashSet::new(),
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        }
    }

    #[test]
    fn try_catch_gate_allows_windows_target() {
        let fns = vec![("Main".to_string(), try_catch_body())];
        assert!(reject_try_catch_outside_windows(&fns, true, "t.as").is_ok());
    }

    #[test]
    fn try_catch_gate_allows_posix_without_try() {
        let fns = vec![("Main".to_string(), plain_body())];
        assert!(reject_try_catch_outside_windows(&fns, false, "t.as").is_ok());
    }

    #[test]
    fn try_catch_gate_rejects_nested_try_on_posix() {
        let fns = vec![("Main".to_string(), try_catch_body())];
        let msg = reject_try_catch_outside_windows(&fns, false, "src/prog.as")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("arc-eh-001"), "unexpected msg: {msg}");
        assert!(msg.contains("C::Main"), "function name missing: {msg}");
        assert!(msg.contains("src/prog.as"), "source file missing: {msg}");
    }

    #[test]
    fn try_catch_scan_reaches_finally_and_ignores_plain() {
        let in_finally = vec![MirStatement::TryFinally {
            body: vec![],
            finally: vec![MirStatement::TryCatch {
                try_body: vec![],
                catch_var: LocalId(0),
                catch_ty: ast::TypeId::Void,
                catch_body: vec![],
            }],
        }];
        assert!(statements_contain_try_catch(&in_finally));
        assert!(!statements_contain_try_catch(&[MirStatement::Return(None)]));
    }

    /// 双属性 FieldSet 测试体：`new VM()` 后分别 `vm.Count = 42`（int 短路路径）
    /// 与 `vm.Name = "x"`（string 无条件通知路径）。
    fn multi_field_set_body() -> MirCfgBody {
        let mut blocks = IndexMap::new();
        blocks.insert(
            BlockId(0),
            MirBlock {
                id: BlockId(0),
                statements: vec![
                    // `new VM()`：验证多属性类的 calloc 尺寸含两个通道槽。
                    MirStatement::Assign {
                        place: LocalId(1),
                        rvalue: MirRvalue::New {
                            class: "VM".into(),
                            args: vec![],
                            ctor_params: vec![],
                        },
                    },
                    // `vm.Count = 42`：int 相等性短路路径。
                    MirStatement::FieldSet {
                        object: MirOperand::Local(LocalId(1)),
                        class: "VM".into(),
                        field: "Count".into(),
                        value: MirRvalue::Use(MirOperand::ConstInt(42)),
                    },
                    // `vm.Name = "x"`：string 无条件通知路径。
                    MirStatement::FieldSet {
                        object: MirOperand::Local(LocalId(1)),
                        class: "VM".into(),
                        field: "Name".into(),
                        value: MirRvalue::Use(MirOperand::ConstString("x".into())),
                    },
                ],
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: ast::TypeId::Void,
            param_count: 0,
            locals: IndexMap::from([
                (LocalId(0), ("v".into(), ast::TypeId::Named("VM".into()))),
                (LocalId(1), ("vm".into(), ast::TypeId::Named("VM".into()))),
            ]),
            entry: BlockId(0),
            blocks,
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: mir::Linkage::External,
            parallelize: false,
            loop_backedges: std::collections::HashSet::new(),
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        }
    }

    fn emit_ir(layouts: &ProgramLayouts, body: MirCfgBody) -> String {
        let fns = vec![("Main".to_string(), body)];
        let empty_syms: super::super::native::NativeSymbolTable = std::collections::HashMap::new();
        let empty_cbs: super::super::emit_native_callback::NativeCallbackTable =
            std::collections::HashMap::new();
        let empty_rt: super::super::native::RuntimeModuleInfos = std::collections::HashMap::new();
        let empty_spans: std::collections::HashMap<String, ast::Span> =
            std::collections::HashMap::new();
        let empty_gen = GenerateToTable::default();
        let emitter = ModuleEmitter::new(
            layouts,
            false,
            false,
            "test.as",
            "",
            false,
            &empty_spans,
            &empty_syms,
            &empty_cbs,
            String::new(),
            &empty_gen,
            &[],
            EmitRole::MainObject,
            None,
            &empty_rt,
        );
        emitter.emit_module(&fns).unwrap().0
    }

    #[test]
    fn observable_int_property_synthesizes_shortcircuit_channel_notify() {
        let layouts = layouts(true, "int");
        let ir = emit_ir(
            &layouts,
            field_set_body(MirRvalue::Use(MirOperand::ConstInt(42))),
        );
        // 隐藏通道字段：类 struct 末尾追加 `ptr`（backing field 复用原字段）。
        assert!(
            ir.contains("%struct.VM = type { %struct.ArcHeader, i32, ptr }"),
            "observable 类须含合成通道字段"
        );
        // calloc 尺寸放大：16 头 + 4 字段 + 4 对齐 + 8 通道 = 32。
        assert!(
            ir.contains("calloc(i64 1, i64 32)"),
            "calloc 须含通道 8 字节"
        );
        // 相等性短路：新值 == 旧值 → 跳过 store/通知。
        assert!(ir.contains("icmp eq i32"), "须发射相等性短路分支");
        // 隐藏通道惰性创建 + 通知发送。
        assert!(
            ir.contains("__ctor_Signal_int"),
            "须惰性 `new Signal<int>()`（calloc + ctor）"
        );
        assert!(
            ir.contains("@Signal_int_Set"),
            "须对隐藏通道发 `Signal<int>.Set` 通知"
        );
    }

    #[test]
    fn unobservable_property_is_not_instrumented() {
        let layouts = layouts(false, "int");
        let ir = emit_ir(
            &layouts,
            field_set_body(MirRvalue::Use(MirOperand::ConstInt(42))),
        );
        // 未标记属性：无通道字段、无通知调用、calloc 尺寸不放大。
        assert!(
            ir.contains("%struct.VM = type { %struct.ArcHeader, i32 }"),
            "未标记类不得追加通道字段"
        );
        assert!(ir.contains("calloc(i64 1, i64 20)"), "calloc 保持原尺寸");
        assert!(!ir.contains("@Signal_int_Set"), "不得发通知");
        assert!(!ir.contains("__ctor_Signal_int"), "不得惰性创建通道");
    }

    #[test]
    fn string_observable_falls_back_to_unconditional_notify() {
        let layouts = layouts(true, "string");
        let ir = emit_ir(
            &layouts,
            field_set_body(MirRvalue::Use(MirOperand::ConstString("x".into()))),
        );
        // string 无法编译期值比较 → 无条件通知（无 icmp eq i32 短路）。
        assert!(!ir.contains("icmp eq i32"), "string 属性不得发射相等性短路");
        assert!(
            ir.contains("@Signal_string_Set"),
            "string 属性须无条件通知 Signal<string>"
        );
    }

    /// M-D0 P0 缺陷回归：同一类含 int + string 两个 `[Observable]` 属性时，
    /// **每属性一隐藏通道槽**——struct 末尾两个 `ptr`、calloc 尺寸含两槽
    /// （align8(32) + 2*8 = 48）、两个槽 GEP 偏移不同（32 / 40）且各自通知
    /// 正确的 `Signal_int` / `Signal_string`（修复前共享单槽 → 必然崩溃）。
    #[test]
    fn multi_observable_properties_get_per_property_channel_slots() {
        let layouts = multi_layouts();
        let ir = emit_ir(&layouts, multi_field_set_body());
        // 通道槽区：struct 末尾按规范序追加两个 `ptr`（Count 槽 + Name 槽）。
        assert!(
            ir.contains("%struct.VM = type { %struct.ArcHeader, i32, ptr, ptr, ptr }"),
            "多属性类须含两个通道槽"
        );
        // calloc 尺寸 = align8(32) + 2*8 = 48。
        assert!(
            ir.contains("calloc(i64 1, i64 48)"),
            "calloc 须含两个通道槽（48 字节）"
        );
        // 两个槽偏移不同：Count 槽 32、Name 槽 40（规范序 k*8）。按行匹配
        // 避免误命中 calloc 的 `i64 48` 等常量。
        let has_channel_gep = |off: &str| {
            ir.lines().any(|l| {
                l.contains("getelementptr inbounds i8, ptr") && l.contains(&format!("i64 {off}"))
            })
        };
        assert!(has_channel_gep("32"), "Count 通道槽须在偏移 32");
        assert!(has_channel_gep("40"), "Name 通道槽须在偏移 40");
        // 两槽各自惰性创建对应类型的 Signal 并各自通知。
        assert!(
            ir.contains("__ctor_Signal_int"),
            "Count 槽须惰性创建 Signal_int"
        );
        assert!(
            ir.contains("__ctor_Signal_string"),
            "Name 槽须惰性创建 Signal_string"
        );
        assert!(
            ir.contains("@Signal_int_Set"),
            "Count setter 须通知 Signal_int"
        );
        assert!(
            ir.contains("@Signal_string_Set"),
            "Name setter 须通知 Signal_string"
        );
    }

    /// 观察者入口测试体：`new VM()` 后调用 `vm.ObserveProperty("Name")`。
    fn observe_body() -> MirCfgBody {
        let mut blocks = IndexMap::new();
        blocks.insert(
            BlockId(0),
            MirBlock {
                id: BlockId(0),
                statements: vec![
                    // `new VM()`。
                    MirStatement::Assign {
                        place: LocalId(1),
                        rvalue: MirRvalue::New {
                            class: "VM".into(),
                            args: vec![],
                            ctor_params: vec![],
                        },
                    },
                    // `vm.ObserveProperty("Name")` → 隐藏通道字段静态定址直访。
                    MirStatement::Assign {
                        place: LocalId(2),
                        rvalue: MirRvalue::MethodCall {
                            receiver: MirOperand::Local(LocalId(1)),
                            method: "ObserveProperty".into(),
                            args: vec![MirOperand::ConstString("Name".into())],
                            receiver_type: "VM".into(),
                            impl_class: None,
                            target_fn: None,
                            is_virtual: false,
                            params: vec![],
                        },
                    },
                ],
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: ast::TypeId::Void,
            param_count: 0,
            locals: IndexMap::from([
                (LocalId(0), ("v".into(), ast::TypeId::Named("VM".into()))),
                (LocalId(1), ("vm".into(), ast::TypeId::Named("VM".into()))),
                (
                    LocalId(2),
                    ("sig".into(), ast::TypeId::Named("Signal_string".into())),
                ),
            ]),
            entry: BlockId(0),
            blocks,
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: mir::Linkage::External,
            parallelize: false,
            loop_backedges: std::collections::HashSet::new(),
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        }
    }

    /// 双属性观察者入口测试体：`new VM()` 后分别 `vm.ObserveProperty("Count")`
    /// 与 `vm.ObserveProperty("Name")`。
    fn multi_observe_body() -> MirCfgBody {
        let mut blocks = IndexMap::new();
        blocks.insert(
            BlockId(0),
            MirBlock {
                id: BlockId(0),
                statements: vec![
                    // `new VM()`。
                    MirStatement::Assign {
                        place: LocalId(1),
                        rvalue: MirRvalue::New {
                            class: "VM".into(),
                            args: vec![],
                            ctor_params: vec![],
                        },
                    },
                    // `vm.ObserveProperty("Count")` → Count 通道槽静态定址直访。
                    MirStatement::Assign {
                        place: LocalId(2),
                        rvalue: MirRvalue::MethodCall {
                            receiver: MirOperand::Local(LocalId(1)),
                            method: "ObserveProperty".into(),
                            args: vec![MirOperand::ConstString("Count".into())],
                            receiver_type: "VM".into(),
                            impl_class: None,
                            target_fn: None,
                            is_virtual: false,
                            params: vec![],
                        },
                    },
                    // `vm.ObserveProperty("Name")` → Name 通道槽静态定址直访。
                    MirStatement::Assign {
                        place: LocalId(3),
                        rvalue: MirRvalue::MethodCall {
                            receiver: MirOperand::Local(LocalId(1)),
                            method: "ObserveProperty".into(),
                            args: vec![MirOperand::ConstString("Name".into())],
                            receiver_type: "VM".into(),
                            impl_class: None,
                            target_fn: None,
                            is_virtual: false,
                            params: vec![],
                        },
                    },
                ],
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: ast::TypeId::Void,
            param_count: 0,
            locals: IndexMap::from([
                (LocalId(0), ("v".into(), ast::TypeId::Named("VM".into()))),
                (LocalId(1), ("vm".into(), ast::TypeId::Named("VM".into()))),
                (
                    LocalId(2),
                    ("countSig".into(), ast::TypeId::Named("Signal_int".into())),
                ),
                (
                    LocalId(3),
                    ("nameSig".into(), ast::TypeId::Named("Signal_string".into())),
                ),
            ]),
            entry: BlockId(0),
            blocks,
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: mir::Linkage::External,
            parallelize: false,
            loop_backedges: std::collections::HashSet::new(),
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        }
    }

    #[test]
    fn observe_property_resolves_to_hidden_channel_gep_direct_access() {
        let layouts = layouts(true, "string");
        let ir = emit_ir(&layouts, observe_body());
        // 隐藏通道字段：VM struct 末尾追加 `ptr`（string 属性：16 头 + 8 字段
        // + 8 通道 = 32，通道偏移 24，8 字节对齐）。
        assert!(
            ir.contains("%struct.VM = type { %struct.ArcHeader, ptr, ptr }"),
            "observable 类须含合成通道字段"
        );
        assert!(
            ir.contains("calloc(i64 1, i64 32)"),
            "calloc 须含通道 8 字节"
        );
        // ObserveProperty("Name") → 通道字段 GEP 常量偏移直访（编译期符号定址）。
        assert!(
            ir.contains("getelementptr inbounds i8, ptr"),
            "须对隐藏通道字段 GEP 常量偏移直访"
        );
        // 惰性 `new Signal<string>()` 回填通道槽。
        assert!(
            ir.contains("__ctor_Signal_string"),
            "须惰性创建 Signal<string>"
        );
        // 无运行期字符串解析（RFC 027 §16 非目标 1）：合成方法无实体符号，
        // 不得落常规 `@VM_ObserveProperty` 方法调用；不得引入字符串比较。
        assert!(
            !ir.contains("@VM_ObserveProperty"),
            "ObserveProperty 须被变换为通道直访，不得落常规方法调用"
        );
        assert!(!ir.contains("@strcmp"), "不得引入运行期字符串解析");
    }

    /// M-D0 P0 缺陷回归（观察者入口侧）：同一类两 `[Observable]` 属性，
    /// `ObserveProperty` 各返回**各自槽**——两处 GEP 偏移不同（32 / 40），
    /// 各自惰性创建对应类型的 `Signal_int` / `Signal_string`（修复前共享
    /// 单槽：`ObserveProperty("Name")` 把 `Signal_int` 当 `Signal_string`
    /// 返回 → 崩溃）。
    #[test]
    fn multi_observable_observe_resolves_per_property_channel_slots() {
        let layouts = multi_layouts();
        let ir = emit_ir(&layouts, multi_observe_body());
        assert!(
            ir.contains("%struct.VM = type { %struct.ArcHeader, i32, ptr, ptr, ptr }"),
            "多属性类须含两个通道槽"
        );
        assert!(
            ir.contains("calloc(i64 1, i64 48)"),
            "calloc 须含两个通道槽（48 字节）"
        );
        let has_channel_gep = |off: &str| {
            ir.lines().any(|l| {
                l.contains("getelementptr inbounds i8, ptr") && l.contains(&format!("i64 {off}"))
            })
        };
        assert!(has_channel_gep("32"), "Count 通道槽须在偏移 32");
        assert!(has_channel_gep("40"), "Name 通道槽须在偏移 40");
        // 两槽各自惰性创建对应类型的 Signal。
        assert!(
            ir.contains("__ctor_Signal_int"),
            "Count 槽须惰性创建 Signal_int"
        );
        assert!(
            ir.contains("__ctor_Signal_string"),
            "Name 槽须惰性创建 Signal_string"
        );
        // 两处 ObserveProperty 均被变换为通道直访，无运行期字符串解析。
        assert!(
            !ir.contains("@VM_ObserveProperty"),
            "ObserveProperty 须被变换为通道直访，不得落常规方法调用"
        );
        assert!(!ir.contains("@strcmp"), "不得引入运行期字符串解析");
    }

    #[test]
    fn observe_property_on_non_observable_class_falls_through() {
        // 类无合成通道（未标记 [Observable]）：ObserveProperty 不被变换，
        // 降级为常规方法调用（typeck 应已拒绝该形态，此处为防御性降级）。
        let layouts = layouts(false, "string");
        let ir = emit_ir(&layouts, observe_body());
        assert!(
            !ir.contains("getelementptr inbounds i8, ptr"),
            "非 observable 类不得产生通道直访"
        );
        assert!(
            ir.contains("@VM_ObserveProperty"),
            "非 observable 类须降级为常规方法调用"
        );
    }
}
