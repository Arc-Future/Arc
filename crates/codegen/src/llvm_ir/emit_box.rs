//! FFI Marshal 装箱/拆箱的 LLVM IR 发射（RFC 016 v2 M2 / RFC 016 M3）。
//!
//! 与 `runtime/rt_box.c` 的 ArcBox 内存布局配合：
//!
//! ```text
//!   ┌────────────────┐  offset 0
//!   │ ArcHeader       │   _Atomic int32_t refcount  (4B)
//!   │                 │   const void* vtable        (8B, 4B padding 在前)
//!   ├────────────────┤  offset 16
//!   │ payload_size    │   int32_t (4B)
//!   ├────────────────┤  offset 20
//!   │ _padding        │   int32_t (4B)
//!   ├────────────────┤  offset 24
//!   │ payload[N]      │   实际负载数据（8B 对齐）
//!   └────────────────┘
//! ```
//!
//! - **Box**：调用 `@rt_box_create(size, align)` 分配 ArcBox，将源值写入 payload 区域。
//! - **Unbox**：调用 `@rt_box_unbox(box_ptr, expected_size, out_ptr, out_size)`，
//!   运行时进行 size 校验 + memcpy（不匹配触发 `rt_panic`）。
//!
//! **M2 范围**：基元值类型（int/long/short/byte/char/float/double/bool）完整支持。
//! 命名类型（struct）的 deep-copy 装箱留待后续迭代——当前 `llvm_size_of` 对
//! `TypeId::Named(_)` 返回默认 8，对 struct 内容仅存储 ptr（非 deep copy）。

use super::*;
use ast::TypeId;
use mir::MirOperand;

/// RFC 004 P0 Phase 1：可装箱基元 → 类型名（供 `primitive_typeinfo_id` 映射
/// `rt_box_vtable(id)` 查询 id；RFC 017 阶段一后基元 typeinfo 经函数符号查询）。
/// 仅含 8 个值类型基元；string 装箱走 rt_string_box 专用路径，struct/enum
/// 与 uint/ulong/ushort/sbyte（无 typeinfo）留待后续 Phase。
fn boxed_primitive_name(ty: &TypeId) -> Option<&'static str> {
    Some(match ty {
        TypeId::Int => "int",
        TypeId::Long => "long",
        TypeId::Short => "short",
        TypeId::Byte => "byte",
        TypeId::Char => "char",
        TypeId::Float => "float",
        TypeId::Double => "double",
        TypeId::Bool => "bool",
        _ => return None,
    })
}

impl<'a> FnEmitter<'a> {
    /// 发射 FFI Marshal 装箱 IR（`MirRvalue::Box`）。
    ///
    /// 流程：
    /// 1. 发射源操作数 → `(src_ty_str, src_val)`
    /// 2. 计算 `size`/`align`（基元类型硬编码，命名类型默认 8/8）
    /// 3. 调用 `@rt_box_create(size, align)` → `box_ptr`（运行时初始化 ArcHeader + payload_size）
    /// 4. 计算 payload 地址（offset 24）
    /// 5. 将源值 `store` 到 payload 地址
    ///
    /// 返回 `("ptr", box_ptr)`——表达式整体类型为 `object`，与 typeck 写入的
    /// `TypedExpr.ty` 一致；`src_ty` 仅用于推导 size/align。
    pub(super) fn emit_box(&mut self, src: &MirOperand, src_ty: &TypeId) -> TyVal {
        // RFC 006 M3: string→object 装箱走专用 runtime ABI（rt_string_box）。
        // string 是裸 char*（无 ArcHeader），不能用通用 rt_box_create 的 size 校验
        // 拆箱路径；box 带 vtable→rt_typeinfo_string，使 `o is string` 可识别。
        //
        // null 保留：`string s = null; object o = s;` 须保持 `o == null`（null
        // 引用装箱仍是 null），与用户 class → object 的指针直传 null 语义对齐。
        // rt_string_box(NULL) 会分配一个 str=NULL 的空盒（非 null 指针），故此处
        // 先判 null：null 源直接产出 `ptr null`，非 null 才走 rt_string_box。
        if *src_ty == TypeId::String {
            let (_, src_val) = self.emit_operand(src);
            let isnull = self.fresh_temp();
            self.emit(&format!("{isnull} = icmp eq ptr {src_val}, null"));
            let null_bb = self.fresh_label();
            let box_bb = self.fresh_label();
            let join = self.fresh_label();
            self.emit(&format!(
                "br i1 {isnull}, label %{null_bb}, label %{box_bb}"
            ));
            self.emit(&format!("{box_bb}:"));
            let box_ptr = self.fresh_temp();
            self.emit(&format!(
                "{box_ptr} = call ptr @rt_string_box(ptr {src_val})"
            ));
            self.emit(&format!("br label %{join}"));
            self.emit(&format!("{null_bb}:"));
            self.emit(&format!("br label %{join}"));
            self.emit(&format!("{join}:"));
            let result = self.fresh_temp();
            self.emit(&format!(
                "{result} = phi ptr [ {box_ptr}, %{box_bb} ], [ null, %{null_bb} ]"
            ));
            return ("ptr".into(), result);
        }

        // RFC 004 P0 Phase 2：struct → object 装箱走逐字段深拷贝（memcpy 结构体
        // 字节 + 内嵌 class 句柄 rt_arc_inc），非裸 ptr 存储。
        if let TypeId::Named(struct_name) = src_ty {
            if self.layouts.structs.contains_key(struct_name) {
                return self.emit_struct_box(src, struct_name);
            }
        }

        let (src_ty_str, src_val) = self.emit_operand(src);
        let size = llvm_size_of(src_ty) as i32;
        let align = llvm_align_of(src_ty) as i32;

        // 调用 rt_box_create 分配 ArcBox（运行时初始化 ArcHeader + payload_size）。
        let box_ptr = self.fresh_temp();
        self.emit(&format!(
            "{box_ptr} = call ptr @rt_box_create(i32 {size}, i32 {align})"
        ));

        // RFC 004 P0 Phase 1：基元装箱写入 vtable 指针（slot0 = 基元
        // typeinfo），使 `o is int` 可判别、`rt_arc_dec` 沿 vtable slot1/2
        // 安全读 null finalizer/walk。struct/enum 装箱留待 Phase 2/4（无
        // vtable，`is` 暂不支持）。
        //
        // RFC 017 阶段一：`.vtable.{prim}_Box` 静态常量已随 typeinfo 数据
        // 符号 static 化一并删除，改经导出函数 `rt_box_vtable(id)` 运行期
        // 查询——数据符号跨共享库映像引用会别名导入 thunk（指向 GOT 槽的
        // 指针）而非数据本身。
        if let Some(prim) = boxed_primitive_name(src_ty) {
            let prim_id =
                primitive_typeinfo_id(prim).expect("boxed primitive must have a typeinfo id");
            let vtable_addr = self.fresh_temp();
            self.emit(&format!(
                "{vtable_addr} = getelementptr inbounds i8, ptr {box_ptr}, i32 8"
            ));
            let vt = self.fresh_temp();
            self.emit(&format!("{vt} = call ptr @rt_box_vtable(i32 {prim_id})"));
            self.emit(&format!("store ptr {vt}, ptr {vtable_addr}"));
        }

        // 计算 payload 地址：ArcBoxHeader = 16B ArcHeader + 4B payload_size + 4B padding = 24B。
        let payload_addr = self.fresh_temp();
        self.emit(&format!(
            "{payload_addr} = getelementptr inbounds i8, ptr {box_ptr}, i32 24"
        ));

        // 将源值写入 payload 区域。
        // 基元类型：直接 store 即可（i32/i64/float/double 等 LLVM IR 标量类型）。
        // 命名类型（struct）：当前仅存储 ptr（非 deep copy），后续迭代补齐。
        self.emit(&format!("store {src_ty_str} {src_val}, ptr {payload_addr}"));

        ("ptr".into(), box_ptr)
    }

    /// RFC 038 M2 链接模型：struct 装箱 vtable（`@.vtable.{T}_Box`）引用守卫
    /// （FnEmitter 侧，函数体路径）。外部 struct 的 `_Box` vtable 由
    /// `emit_boxed_struct_vtables` 跳过（任意角色，定义包才发射），登记 external
    /// 声明、由定义包 linkonce_odr 定义解析；struct 不在本 TU 布局返回 `None`。
    fn boxed_struct_vtable_global(&mut self, struct_name: &str) -> Option<String> {
        if !self.layouts.structs.contains_key(struct_name) {
            return None;
        }
        let sym = format!("@.vtable.{struct_name}_Box");
        if self.external_class_names.contains(struct_name) {
            self.external_aggregate_refs
                .entry(sym[1..].to_string())
                .or_insert_with(|| "[3 x ptr]".to_string());
        }
        Some(sym)
    }

    /// RFC 004 P0 Phase 2：struct → object 深拷贝装箱。
    ///
    /// struct 值在 MIR 中为指向栈 alloca `%struct.{T}` 的 ptr，装箱须：
    /// 1. `rt_box_create(size, align)` 分配 ArcBox（size = 实际结构体字节数，非默认 8）
    /// 2. store `@.vtable.{T}_Box` 到 ArcHeader.vtable（slot0 = `@.typeinfo.{T}`，
    ///    使 `o is T` 可判别）
    /// 3. `@llvm.memcpy` 把结构体字节深拷贝到 payload（offset 24）
    /// 4. 内嵌 class 句柄字段 `rt_arc_inc`（与 FieldSet 的 ARC 维护同源）
    fn emit_struct_box(&mut self, src: &MirOperand, struct_name: &str) -> TyVal {
        let (_, src_val) = self.emit_operand(src);
        let size = self.layouts.size_of_ty(struct_name) as i32;
        let align = 8;

        let box_ptr = self.fresh_temp();
        self.emit(&format!(
            "{box_ptr} = call ptr @rt_box_create(i32 {size}, i32 {align})"
        ));

        // RFC 038 M2：外部 struct（LibraryObject）的 `_Box` vtable 由守卫登记
        // external 声明（emit_boxed_struct_vtables 跳过本 TU 定义），防链接期
        // undefined symbol；MainObject 角色外部类则直引 linkonce_odr 定义。
        if let Some(vtable_sym) = self.boxed_struct_vtable_global(struct_name) {
            let vtable_addr = self.fresh_temp();
            self.emit(&format!(
                "{vtable_addr} = getelementptr inbounds i8, ptr {box_ptr}, i32 8"
            ));
            self.emit(&format!("store ptr {vtable_sym}, ptr {vtable_addr}"));
        }

        let payload_addr = self.fresh_temp();
        self.emit(&format!(
            "{payload_addr} = getelementptr inbounds i8, ptr {box_ptr}, i32 24"
        ));
        self.emit(&format!(
            "call void @llvm.memcpy.p0.p0.i64(ptr {payload_addr}, ptr {src_val}, i64 {size}, i1 false)"
        ));

        // 内嵌 class 句柄随拷贝 rt_arc_inc（浅拷贝复制了指针，须为盒内新引用计数）。
        if let Some(slayout) = self.layouts.structs.get(struct_name) {
            for field in &slayout.fields {
                let field_ty = field.ty.as_str();
                if self.layouts.classes.contains_key(field_ty)
                    && !is_opaque_runtime_handle(field_ty)
                {
                    let faddr = self.fresh_temp();
                    self.emit(&format!(
                        "{faddr} = getelementptr inbounds i8, ptr {payload_addr}, i32 {}",
                        field.offset
                    ));
                    let fval = self.fresh_temp();
                    self.emit(&format!("{fval} = load ptr, ptr {faddr}"));
                    self.emit(&format!("call void @rt_arc_inc(ptr {fval})"));
                }
            }
        }

        ("ptr".into(), box_ptr)
    }

    /// 发射 FFI Marshal 拆箱 IR（`MirRvalue::Unbox`）。
    ///
    /// 流程：
    /// 1. 发射源操作数 → `(src_ty_str, src_val)`——应为 `ptr`（`object` 引用）
    /// 2. 计算 `expected_size`（目标值类型的 size）
    /// 3. `alloca` 目标类型 slot（接收 unbox 后的字节）
    /// 4. 调用 `@rt_box_unbox(src_val, expected_size, slot_ptr, out_size)`
    ///    运行时执行 size 校验 + memcpy（不匹配则 `rt_panic`）
    /// 5. 从 slot 加载目标值
    ///
    /// 返回 `(target_ty_str, loaded_val)`——表达式整体类型为目标值类型。
    pub(super) fn emit_unbox(&mut self, src: &MirOperand, target_ty: &TypeId) -> TyVal {
        // RFC 006 M3: `(string)obj` 从 object 槽提取 string——走 rt_string_unbox，
        // 非 string box 返回 NULL（调用方按解引用/字符串语义处理）。
        if *target_ty == TypeId::String {
            let (_, src_val) = self.emit_operand(src);
            let result = self.fresh_temp();
            self.emit(&format!(
                "{result} = call ptr @rt_string_unbox(ptr {src_val})"
            ));
            return ("ptr".into(), result);
        }
        let (_, src_val) = self.emit_operand(src);
        // RFC 004 P0 Phase 2：struct 拆箱——slot 用 `%struct.{T}`（实际字节数），
        // 返回值以 ptr 引用（struct 值按引用存储，与 `emit_struct_lit` 一致）。
        if let TypeId::Named(struct_name) = target_ty {
            if self.layouts.structs.contains_key(struct_name) {
                let target_size = self.layouts.size_of_ty(struct_name) as i32;
                let slot = self.fresh_temp();
                self.emit(&format!("{slot} = alloca %struct.{struct_name}"));
                let status = self.fresh_temp();
                self.emit(&format!(
                    "{status} = call i32 @rt_box_unbox(ptr {src_val}, i32 {target_size}, ptr {slot}, i32 {target_size})"
                ));
                return ("ptr".into(), slot);
            }
        }
        let target_size = llvm_size_of(target_ty) as i32;
        let target_ty_str = llvm_type_of(target_ty, self.layouts);

        // 分配目标 slot（接收 unbox 后的字节）。
        let slot = self.fresh_temp();
        self.emit(&format!("{slot} = alloca {target_ty_str}"));

        // 调用 rt_box_unbox：运行时执行 size 校验 + memcpy。
        // 返回值：0 = 成功；-1 = null 指针；-2 = size mismatch（已 rt_panic，不会返回）。
        // 因 size mismatch 在运行时已 panic，codegen 不再检查 status。
        let status = self.fresh_temp();
        self.emit(&format!(
            "{status} = call i32 @rt_box_unbox(ptr {src_val}, i32 {target_size}, ptr {slot}, i32 {target_size})"
        ));

        // 加载 unboxed 值。
        let result = self.fresh_temp();
        self.emit(&format!("{result} = load {target_ty_str}, ptr {slot}"));

        (target_ty_str, result)
    }
}
