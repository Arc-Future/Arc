//! Variant 类型构造/读取的 LLVM IR 发射（RFC 004 M1）。
//!
//! Variant 内存布局（与 `mod.rs::emit_type_definitions` 配合）：
//!
//! ```llvm
//! %variant.{Name}.body = type { <largest_payload_ty> }   ; 或 [0 x i8] 若无 payload
//! %variant.{Name}      = type { i8, [3 x i8], %variant.{Name}.body }
//! ```
//!
//! - 字段 0：`i8` tag（case discriminant）
//! - 字段 1：`[3 x i8]` 填充
//! - 字段 2：`%variant.{Name}.body` payload（union 语义，所有 case 共享同一容器）
//!
//! **M1 范围**：仅支持单字段 payload（基元/ptr）。多字段 struct payload
//! 需先声明 struct 再作为 payload（与 `VariantCase.payload: Option<Ident>`
//! 单一类型约束一致）。

use super::*;
use ast::TypeId;
use mir::MirOperand;
use typeck::VariantLayout;

impl<'a> FnEmitter<'a> {
    /// 发射 variant 构造 IR（`MirRvalue::VariantConstruct`）。
    ///
    /// 流程：
    /// 1. 查 `layouts.variants[variant_name]` 获取 `VariantLayout`
    /// 2. 找到 `case_name` 对应的 case，取 `discriminant`
    /// 3. `alloca %variant.{Name}` 并零初始化
    /// 4. `store i8 discriminant` 到 tag 字段（GEP 0, 0）
    /// 5. 若 `payload` 为 `Some`：发射 payload 操作数，`store` 到 body 字段（GEP 0, 2）
    ///    - class/string payload（ptr）：额外发射 `rt_arc_inc` 维护引用计数
    /// 6. 返回 `("%variant.{Name}", alloca_ptr)`
    pub(super) fn emit_variant_construct(
        &mut self,
        variant_name: &str,
        case_name: &str,
        payload: Option<&MirOperand>,
    ) -> TyVal {
        let vlayout = match self.layouts.variants.get(variant_name) {
            Some(v) => v,
            None => {
                // 不应在 typeck 通过后到达此处；返回 undef ptr 兜底。
                return ("ptr".into(), "null".into());
            }
        };
        let case = match vlayout.cases.iter().find(|c| c.name.as_str() == case_name) {
            Some(c) => c,
            None => return ("ptr".into(), "null".into()),
        };
        let discriminant = case.discriminant as i32;

        let variant_ty = format!("%variant.{variant_name}");
        let slot = self.fresh_temp();
        self.emit(&format!("{slot} = alloca {variant_ty}"));
        // 零初始化（确保 padding + body 全零，避免未定义读取）。
        self.emit(&format!("store {variant_ty} zeroinitializer, ptr {slot}"));

        // 1. 写 tag：GEP field 0 → i8*，store discriminant
        let tag_ptr = self.fresh_temp();
        self.emit(&format!(
            "{tag_ptr} = getelementptr inbounds {variant_ty}, ptr {slot}, i32 0, i32 0"
        ));
        self.emit(&format!("store i8 {discriminant}, ptr {tag_ptr}"));

        // 2. 若有 payload：写 body 字段（GEP field 2）
        if let (Some(payload_op), Some(payload_ident)) = (payload, &case.payload) {
            // 基元类型 payload（double/int/bool 等）须映射为对应 TypeId 变体，
            // 不能统一包装为 TypeId::Named——否则 named_type 回退为 ptr，
            // 导致 payload 存储/提取类型不匹配。
            let payload_ty_id = match payload_ident.as_str() {
                "int" => TypeId::Int,
                "long" => TypeId::Long,
                "short" => TypeId::Short,
                "byte" => TypeId::Byte,
                "char" => TypeId::Char,
                "bool" => TypeId::Bool,
                "string" => TypeId::String,
                "void" => TypeId::Void,
                "float" => TypeId::Float,
                "double" => TypeId::Double,
                "uint" => TypeId::UInt,
                "ulong" => TypeId::ULong,
                "ushort" => TypeId::UShort,
                "sbyte" => TypeId::SByte,
                other => TypeId::Named(other.into()),
            };
            let payload_ty_str = llvm_type_of(&payload_ty_id, self.layouts);
            let (val_ty, val) = self.emit_operand(payload_op);

            let body_ptr = self.fresh_temp();
            self.emit(&format!(
                "{body_ptr} = getelementptr inbounds {variant_ty}, ptr {slot}, i32 0, i32 2"
            ));

            // RFC 004 M1 §12：仅 class payload 做 rt_arc_inc。
            // string 常为 rodata / 裸 char*（无 ArcHeader）；inc 会写坏串首字节 → 堆损坏。
            let needs_arc = self.layouts.classes.contains_key(payload_ident)
                && !is_opaque_runtime_handle(payload_ident);
            if needs_arc {
                self.emit(&format!("call void @rt_arc_inc(ptr {val})"));
            }
            // val_ty 与 payload_ty_str 在基元场景应一致；命名类型均为 ptr。
            let store_ty = if val_ty == "ptr" || val_ty.is_empty() {
                payload_ty_str.clone()
            } else {
                val_ty.clone()
            };
            self.emit(&format!("store {store_ty} {val}, ptr {body_ptr}"));
        }

        // variant 是栈上值类型，按引用传递（与 struct 一致）：
        // local slot 存储 ptr（指向 alloca'd variant 结构体）
        ("ptr".into(), slot)
    }

    /// RFC 004 生命周期修正（D3 清偿）：variant 值跨帧转储（类/结构体字段、
    /// 数组/列表元素、boxed struct）时深拷贝到堆。
    ///
    /// 原实现 `emit_variant_construct` 在**当前函数栈帧** alloca variant，
    /// 返回其指针；该指针存进堆对象（如 `Setter.Value`）后，随创建帧消亡
    /// 而悬垂——后续回读 tag/payload 读到被复用栈 → DP 回读坏数据
    /// （`style_selector_cascade_e2e` cas_all 根因）。本函数把 variant 字节
    /// 拷贝到独立堆块，使指针生命周期绑定堆而非创建帧；class payload 额外
    /// `rt_arc_inc`（与构造对称，源 variant 所有权不变）。
    ///
    /// 堆块由「结构体字段不 walk drop」模型统一视为泄漏（与 boxed struct
    /// 一致，见 arc_drop.rs 模块注释）。
    pub(super) fn emit_variant_deep_copy(&mut self, variant_name: &str, src_ptr: &str) -> String {
        let variant_ty = format!("%variant.{variant_name}");
        let Some(vlayout) = self.layouts.variants.get(variant_name) else {
            return src_ptr.to_string();
        };
        // 与 mod.rs::emit_type_definitions 布局一致：`{ i8, [3 x i8], body }`，
        // 但 LLVM DataLayout 会按 body 对齐补齐尾部（body=ptr/double/i64 →
        // 总 16 字节；body=i32 → 8 字节）。仅 memcpy 4+max_payload 会截断
        // 8 字节对齐的 ptr payload 高 4 字节 → 悬垂字符串 → strlen AV。
        let size = self.variant_byte_size(vlayout);

        // 源可能为 null（空 variant / zeroinitializer 默认值，如 `Content.Empty`）：
        // 此时保持 null，不复制——否则 memcpy(NULL) 触发 AV。
        let null_cmp = self.fresh_temp();
        self.emit(&format!("{null_cmp} = icmp eq ptr {src_ptr}, null"));
        let null_label = self.fresh_label();
        let copy_label = self.fresh_label();
        self.emit(&format!(
            "br i1 {null_cmp}, label %{null_label}, label %{copy_label}"
        ));

        let join_label = self.fresh_label();
        let heap = self.fresh_temp();
        let mut copy_join = copy_label.clone();

        self.emit_label(&copy_label);
        self.emit(&format!("{heap} = call ptr @calloc(i64 1, i64 {size})"));
        // 聚合 load/store 复制整个 variant（含 padding 与对齐，避免手工算 size）。
        let loaded = self.fresh_temp();
        self.emit(&format!("{loaded} = load {variant_ty}, ptr {src_ptr}"));
        self.emit(&format!("store {variant_ty} {loaded}, ptr {heap}"));
        // class payload case：堆副本须持 +1（源 variant 所有权不变）。
        let class_cases: Vec<u32> = vlayout
            .cases
            .iter()
            .filter_map(|c| {
                let p = c.payload.as_ref()?;
                if self.layouts.classes.contains_key(p) && !is_opaque_runtime_handle(p) {
                    Some(c.discriminant)
                } else {
                    None
                }
            })
            .collect();
        if !class_cases.is_empty() {
            let tag_ptr = self.fresh_temp();
            self.emit(&format!(
                "{tag_ptr} = getelementptr inbounds {variant_ty}, ptr {heap}, i32 0, i32 0"
            ));
            let tag_u8 = self.fresh_temp();
            self.emit(&format!("{tag_u8} = load i8, ptr {tag_ptr}"));
            let tag = self.fresh_temp();
            self.emit(&format!("{tag} = zext i8 {tag_u8} to i32"));
            let mut next_label = self.fresh_label();
            self.emit(&format!("br label %{next_label}"));
            for disc in &class_cases {
                let cur_label = next_label;
                let inc_label = self.fresh_label();
                next_label = self.fresh_label();
                self.emit_label(&cur_label);
                let cmp = self.fresh_temp();
                self.emit(&format!("{cmp} = icmp eq i32 {tag}, {disc}"));
                self.emit(&format!(
                    "br i1 {cmp}, label %{inc_label}, label %{next_label}"
                ));
                self.emit_label(&inc_label);
                let body_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{body_ptr} = getelementptr inbounds {variant_ty}, ptr {heap}, i32 0, i32 2"
                ));
                let payload_val = self.fresh_temp();
                self.emit(&format!("{payload_val} = load ptr, ptr {body_ptr}"));
                self.emit(&format!("call void @rt_arc_inc(ptr {payload_val})"));
                self.emit(&format!("br label %{next_label}"));
            }
            self.emit_label(&next_label);
            copy_join = next_label;
        }
        self.emit(&format!("br label %{join_label}"));

        self.emit_label(&null_label);
        self.emit(&format!("br label %{join_label}"));

        self.emit_label(&join_label);
        let result = self.fresh_temp();
        self.emit(&format!(
            "{result} = phi ptr [ {heap}, %{copy_join} ], [ null, %{null_label} ]"
        ));
        result
    }

    /// 计算 `%variant.{Name} = { i8, [3 x i8], body }` 的 LLVM DataLayout 字节大小。
    ///
    /// tag(1) + pad(3) = 4 字节起；body 按最大 payload 类型取
    /// （ptr/double/i64 → 8 对齐 → 总 16；i32 → 4 对齐 → 总 8；空 body → 4）。
    /// 与 `emit_type_definitions` 的 `pick_largest_payload` 语义一致。
    fn variant_byte_size(&self, vlayout: &VariantLayout) -> u64 {
        let mut max_size: u64 = 0;
        let mut max_align: u64 = 1;
        for case in &vlayout.cases {
            let Some(p) = &case.payload else { continue };
            let (sz, al) = match p.as_str() {
                "int" | "bool" | "float" | "char" | "uint" => (4u64, 4u64),
                "double" | "long" | "ulong" => (8, 8),
                "short" | "ushort" => (2, 2),
                "byte" | "sbyte" => (1, 1),
                // ptr 类（string/class/struct 句柄/委托/数组）与复合 payload 兜底
                _ => (8, 8),
            };
            max_size = max_size.max(sz);
            max_align = max_align.max(al);
        }
        let total = 4 + max_size;
        // 按最大对齐上取整（LLVM DataLayout 尾部补齐）
        total.div_ceil(max_align) * max_align
    }

    /// 发射 variant tag 读取 IR（`MirRvalue::VariantTag`）。
    ///
    /// 用于 `switch`/`match` 分派：读取 scrutinee 的 tag 字段（i8），
    /// 零扩展为 i32 供后续 `icmp eq` 比较。
    ///
    /// 返回 `("i32", zext_result)`。
    pub(super) fn emit_variant_tag(&mut self, scrutinee: &MirOperand, variant_name: &str) -> TyVal {
        let variant_ty = format!("%variant.{variant_name}");
        let (_, scrut_ptr) = self.emit_operand(scrutinee);

        let tag_ptr = self.fresh_temp();
        self.emit(&format!(
            "{tag_ptr} = getelementptr inbounds {variant_ty}, ptr {scrut_ptr}, i32 0, i32 0"
        ));
        let tag_u8 = self.fresh_temp();
        self.emit(&format!("{tag_u8} = load i8, ptr {tag_ptr}"));
        let tag_i32 = self.fresh_temp();
        self.emit(&format!("{tag_i32} = zext i8 {tag_u8} to i32"));
        ("i32".into(), tag_i32)
    }

    /// 发射 variant payload 提取 IR（`MirRvalue::VariantExtract`）。
    ///
    /// 在 `match` case 块内调用：从 scrutinee 提取 `case_name` 对应的
    /// payload 值，绑定到 local。
    ///
    /// **所有权语义**：class payload 发射 `rt_arc_inc`——绑定 local 被
    /// `emit_sync_epilogue_drops`（及 SM dtor）当作普通 ARC class local
    /// 在出口 dec；若提取不 inc，每次 case 命中都会对 payload 净 -1，
    /// 容器（如 ResourceDictionary）持有的对象被提前释放 → 后续访问
    /// UAF/AV（ArmlDemo 悬停帧 `Application.ResolveColor` 实测根因）。
    /// 直接 `return <binding>;` 由 returned_local 排除 dec，所有权正确
    /// 移交调用方。string/基元 payload 不 inc（无 ArcHeader）。
    ///
    /// 返回 `(payload_ty_str, loaded_val)`。
    pub(super) fn emit_variant_extract(
        &mut self,
        scrutinee: &MirOperand,
        variant_name: &str,
        _case_name: &str,
        payload_ty: &TypeId,
    ) -> TyVal {
        let variant_ty = format!("%variant.{variant_name}");
        let (_, scrut_ptr) = self.emit_operand(scrutinee);
        let payload_ty_str = llvm_type_of(payload_ty, self.layouts);

        // GEP field 2（body）→ load payload
        let body_ptr = self.fresh_temp();
        self.emit(&format!(
            "{body_ptr} = getelementptr inbounds {variant_ty}, ptr {scrut_ptr}, i32 0, i32 2"
        ));
        let loaded = self.fresh_temp();
        self.emit(&format!("{loaded} = load {payload_ty_str}, ptr {body_ptr}"));
        // class payload：绑定 local 持独立 +1（与出口 dec 配对；rt_arc_inc null 安全）。
        if let TypeId::Named(name) = payload_ty {
            if self.layouts.classes.contains_key(name.as_str())
                && !is_opaque_runtime_handle(name.as_str())
            {
                self.emit(&format!("call void @rt_arc_inc(ptr {loaded})"));
            }
        }
        (payload_ty_str, loaded)
    }

    /// 发射 variant 局部变量的析构 IR（`arc_drop::emit_drop` 路由进入）。
    ///
    /// RFC 004 M1 §12 ARC 透明策略：variant 是栈分配（alloca），本身不需要
    /// `rt_arc_dec`。但若 case payload 为 class/string（ptr 类型，ARC 管理），
    /// 需要按 tag 分派 dec payload 引用。
    ///
    /// 策略：对每个有 class payload 的 case 生成 `icmp eq tag, disc` 条件分支，
    /// 命中则 `load ptr` payload + `call void @rt_arc_dec`。
    /// 无 class payload 的 case（None 或基元类型）不需要 drop。
    pub(super) fn emit_variant_drop(&mut self, id: mir::LocalId, variant_name: &str) {
        // variant local 的 LLVM 类型是 ptr（指向 alloca'd variant 结构体）
        // emit_operand(Local(id)) 会 load ptr 得到 variant 地址
        let (_, variant_ptr) = self.emit_operand(&MirOperand::Local(id));
        let variant_ty = format!("%variant.{variant_name}");

        // 读取 tag（i8 → zext i32，用于 icmp eq 比较）
        let tag_ptr = self.fresh_temp();
        self.emit(&format!(
            "{tag_ptr} = getelementptr inbounds {variant_ty}, ptr {variant_ptr}, i32 0, i32 0"
        ));
        let tag_u8 = self.fresh_temp();
        self.emit(&format!("{tag_u8} = load i8, ptr {tag_ptr}"));
        let tag = self.fresh_temp();
        self.emit(&format!("{tag} = zext i8 {tag_u8} to i32"));

        // 收集需要 dec 的 case（payload 为 class 类型）
        let vlayout = match self.layouts.variants.get(variant_name) {
            Some(l) => l,
            None => return,
        };
        let class_cases: Vec<u32> = vlayout
            .cases
            .iter()
            .filter_map(|c| {
                let p = c.payload.as_ref()?;
                // 仅 class 类型需要 ARC dec；string/基元/struct 不需要
                if self.layouts.classes.contains_key(p) {
                    Some(c.discriminant)
                } else {
                    None
                }
            })
            .collect();

        if class_cases.is_empty() {
            // 无 class payload case——无需 drop
            return;
        }

        // 为每个 class payload case 生成条件 drop 分支（if-else 链）
        let mut next_label = self.fresh_label();
        self.emit(&format!("br label %{next_label}"));
        for disc in &class_cases {
            let cur_label = next_label;
            let drop_label = self.fresh_label();
            next_label = self.fresh_label();
            self.emit_label(&cur_label);
            let cmp = self.fresh_temp();
            self.emit(&format!("{cmp} = icmp eq i32 {tag}, {disc}"));
            self.emit(&format!(
                "br i1 {cmp}, label %{drop_label}, label %{next_label}"
            ));
            self.emit_label(&drop_label);
            // 提取 payload（borrow-only，不 inc）然后 dec
            let body_ptr = self.fresh_temp();
            self.emit(&format!(
                "{body_ptr} = getelementptr inbounds {variant_ty}, ptr {variant_ptr}, i32 0, i32 2"
            ));
            let payload_val = self.fresh_temp();
            self.emit(&format!("{payload_val} = load ptr, ptr {body_ptr}"));
            self.emit(&format!("call void @rt_arc_dec(ptr {payload_val})"));
            self.emit(&format!("br label %{next_label}"));
        }
        self.emit_label(&next_label);
        // variant 结构体本身是栈上 alloca，无需释放
    }
}
