//! Rvalue and operand emission: MirRvalue/MirOperand -> LLVM IR instructions.
//!
//! Each `emit_rvalue` / `emit_operand` returns `(type_str, value_str)`:
//! - `type_str`: LLVM IR type (e.g. "i32", "ptr", "double")
//! - `value_str`: LLVM IR value or SSA name (e.g. "42", "%t0", "null")

use super::*;
use ast::TypeId;
use mir::{MirOperand, MirRvalue};

/// Result type for rvalue/operand emission: (LLVM type string, value string).
pub(crate) type TyVal = (String, String);

impl<'a> FnEmitter<'a> {
    /// Emit an rvalue, returning (type, value). For void rvalues, returns ("void", "").
    pub fn emit_rvalue(&mut self, rv: &MirRvalue) -> TyVal {
        match rv {
            MirRvalue::Use(op) => self.emit_operand(op),
            MirRvalue::Binary { op, left, right } => self.emit_binary(*op, left, right),
            MirRvalue::Call { func, args } => self.emit_call(func, args),
            MirRvalue::New {
                class,
                args,
                ctor_params,
            } => self.emit_new(class, args, ctor_params),
            MirRvalue::FieldGet {
                object,
                class,
                field,
            } => self.emit_field_get(object, class, field),
            MirRvalue::MethodCall {
                receiver,
                method,
                args,
                receiver_type,
                impl_class,
                target_fn,
                is_virtual,
                params,
            } => self.emit_method_call(
                receiver,
                method,
                args,
                receiver_type,
                impl_class.as_deref(),
                target_fn.as_deref(),
                *is_virtual,
                params,
            ),
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
            MirRvalue::StructLit {
                struct_name,
                fields,
            } => self.emit_struct_lit(struct_name, fields),
            MirRvalue::ArrayLit {
                elem_type,
                elements,
            } => self.emit_array_lit(elem_type, elements),
            MirRvalue::NewArray { elem_type, length } => self.emit_new_array(elem_type, length),
            MirRvalue::IndexGet {
                array,
                index,
                elem_type,
            } => {
                // RFC 005：Span 索引读（Local 或 Field class=Span/ReadOnlySpan）。
                if self.operand_is_span(array) {
                    return self.emit_span_index_get(array, index, elem_type);
                }
                self.emit_index_get(array, index, elem_type)
            }
            MirRvalue::SpanFromArray {
                array,
                start,
                length,
                mutable,
            } => self.emit_span_from_array(array, start, length, *mutable),
            MirRvalue::SpanFromStack {
                elements,
                elem_type,
                mutable,
            } => self.emit_span_from_stack(elements, elem_type, *mutable),
            MirRvalue::SpanSlice {
                span,
                start,
                length,
                mutable,
            } => self.emit_span_slice(span, start, length.as_ref(), *mutable),
            MirRvalue::SpanFill {
                span,
                value,
                elem_type,
            } => {
                self.emit_span_fill(span, value, elem_type);
                ("void".into(), String::new())
            }
            MirRvalue::SpanClear { span, elem_type } => {
                self.emit_span_clear(span, elem_type);
                ("void".into(), String::new())
            }
            MirRvalue::SpanCopyTo {
                src,
                dest,
                elem_type,
            } => {
                self.emit_span_copy_to(src, dest, elem_type);
                ("void".into(), String::new())
            }
            MirRvalue::SpanTryCopyTo {
                src,
                dest,
                elem_type,
            } => self.emit_span_try_copy_to(src, dest, elem_type),
            MirRvalue::SpanToArray { span, elem_type } => self.emit_span_to_array(span, elem_type),
            MirRvalue::SoaFieldGet {
                array,
                index,
                class,
                field,
            } => self.emit_soa_field_get(array, index, class, field),
            MirRvalue::Coalesce { left, right } => self.emit_coalesce(left, right),
            MirRvalue::Ternary {
                cond,
                then_val,
                else_val,
            } => self.emit_ternary(cond, then_val, else_val),
            MirRvalue::FnPtr { name } => {
                // RFC 045（di_decorate 崩溃根因）：FnPtr 的**值语义**（赋值存储/
                // 返回/捕获）统一为 arc_closure {fn, null}——委托值恒为 closure
                // 指针，闭包体内按 closure 结构解引用安全。裸函数指针仅存在于
                // 直接调用路径（emit_indirect_call 的 FnPtr 分支 / 内联 lambda
                // 实参经 emit_operand_as_closure）。旧实现存储裸 fn：被另一闭包
                // 捕获后按 closure 解引用 → 0xC0000005。
                let (_, cptr) = self.emit_fnptr_as_closure(name);
                ("ptr".into(), cptr)
            }
            MirRvalue::IndirectCall { func, args } => self.emit_indirect_call(func, args),
            MirRvalue::NullCondField {
                receiver,
                class,
                field,
                default,
            } => self.emit_null_cond_field(receiver, class, field, default),
            MirRvalue::NullCondMethod {
                receiver,
                method,
                args,
                receiver_type,
                impl_class,
                target_fn,
                is_virtual,
                params,
                default,
            } => self.emit_null_cond_method(
                receiver,
                method,
                args,
                receiver_type,
                impl_class.as_deref(),
                target_fn.as_deref(),
                *is_virtual,
                params,
                default,
            ),
            MirRvalue::ForceDerefField {
                receiver,
                class,
                field,
                span,
            } => self.emit_force_deref_field(receiver, class, field, *span),
            MirRvalue::ForceDerefMethod {
                receiver,
                method,
                args,
                receiver_type,
                impl_class,
                target_fn,
                is_virtual,
                params,
                span,
            } => self.emit_force_deref_method(
                receiver,
                method,
                args,
                receiver_type,
                impl_class.as_deref(),
                target_fn.as_deref(),
                *is_virtual,
                params,
                *span,
            ),
            MirRvalue::LinqChain(_) => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = alloca i8"));
                ("ptr".into(), tmp)
            }
            MirRvalue::ExpressionTreeConst { name, tree } => {
                // RFC 022 Sprint 2b: construct Expression object tree at runtime
                // instead of returning a pointer to a static rodata global.
                // The `name` field is retained for diagnostics but no longer
                // emitted as a global constant.
                let _ = name;
                self.emit_expression_tree(tree)
            }
            // FFI Marshal 装箱/拆箱（RFC 016 v2 M2 / RFC 016 M3）：
            // 调用 rt_box_create + store 或 rt_box_unbox + load。
            MirRvalue::Box { src, src_ty } => self.emit_box(src, src_ty),
            MirRvalue::Unbox { src, target_ty } => self.emit_unbox(src, target_ty),
            // RFC 004 M1：variant 构造 / tag 读取 / payload 提取
            MirRvalue::VariantConstruct {
                variant_name,
                case_name,
                payload,
            } => self.emit_variant_construct(variant_name, case_name, payload.as_ref()),
            MirRvalue::VariantTag {
                scrutinee,
                variant_name,
            } => self.emit_variant_tag(scrutinee, variant_name),
            MirRvalue::VariantExtract {
                scrutinee,
                variant_name,
                case_name,
                payload_ty,
            } => self.emit_variant_extract(scrutinee, variant_name, case_name, payload_ty),
        }
    }

    /// Emit an rvalue with an expected return type (for Call/MethodCall return type inference).
    pub fn emit_rvalue_typed(&mut self, rv: &MirRvalue, expected: &TypeId) -> TyVal {
        match rv {
            MirRvalue::Call { func, args } => self.emit_call_typed(func, args, expected),
            MirRvalue::MethodCall {
                receiver,
                method,
                args,
                receiver_type,
                impl_class,
                target_fn,
                is_virtual,
                params,
            } => self.emit_method_call_typed(
                receiver,
                method,
                args,
                receiver_type,
                impl_class.as_deref(),
                target_fn.as_deref(),
                *is_virtual,
                params,
                expected,
            ),
            _ => self.emit_rvalue(rv),
        }
    }

    /// Emit an operand, returning (type, value). Loads locals from their alloca slots.
    pub fn emit_operand(&mut self, op: &MirOperand) -> TyVal {
        match op {
            MirOperand::Local(id) => {
                let ty = self.local_type(*id);
                if matches!(ty, TypeId::Void) {
                    return ("void".into(), String::new());
                }
                // ByRef 捕获局部（变量捕获）：本地 alloca 存**外层变量槽地址**
                // （emit_fn 捕获初始化 load env field 得到），读取须经槽解引用。
                // 旧实现 env 存值快照，此处直接 load alloca 得值；改为槽地址后
                // 必须二次 load，否则拿到的是地址而非变量值。
                if let Some((_, _, c)) = self.cfg.captures.iter().find(|(cid, _, _)| *cid == *id) {
                    if matches!(c.mode, ast::CaptureMode::ByRef) {
                        let slot = self.fresh_temp();
                        self.emit(&format!("{slot} = load ptr, ptr {}", self.local_ptr(*id)));
                        let ty_str = llvm_type_of(&ty, self.layouts);
                        let tmp = self.fresh_temp();
                        self.emit(&format!("{tmp} = load {ty_str}, ptr {slot}"));
                        return (ty_str, tmp);
                    }
                }
                // Ref 槽读取按 RefKind 分派（TypeId::Ref 双语义契约，见
                // ast::RefKind 文档）。二者指令序列**不同构**：
                // - Var（ref/out/in 参数）：槽存指向调用方存储的指针 P，
                //   P 之下才是被引值 → 双 load（`P = load 槽; v = load *P`）。
                // - Value（struct 实例 this）：槽存实例地址 A，而 struct 的
                //   llvm_type_of 恒为 ptr（named_type：按引用 ABI），A 本身
                //   就是该槽类型的值 → 单 load（`v = load ptr, ptr 槽`）。
                //   旧实现对 Value 槽同样双 load，第二 load 把实例首字段
                //   （calloc 后 = 0）当作值读出：FieldSet/FieldGet 随即对其
                //   GEP 解引 → 写/读 *(0+offset) → 0xC0000005。
                if let TypeId::Ref { inner, kind, .. } = &ty {
                    let inner_ty = llvm_type_of(inner, self.layouts);
                    let slot_ptr = self.local_ptr(*id);
                    return match kind {
                        ast::RefKind::Var => {
                            let deref_ptr = self.fresh_temp();
                            self.emit(&format!("{deref_ptr} = load ptr, ptr {slot_ptr}"));
                            let tmp = self.fresh_temp();
                            self.emit(&format!("{tmp} = load {inner_ty}, ptr {deref_ptr}"));
                            (inner_ty, tmp)
                        }
                        ast::RefKind::Value => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!("{tmp} = load {inner_ty}, ptr {slot_ptr}"));
                            (inner_ty, tmp)
                        }
                    };
                }
                let ty_str = llvm_type_of(&ty, self.layouts);
                let ptr = self.local_ptr(*id);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = load {ty_str}, ptr {ptr}"));
                (ty_str, tmp)
            }
            MirOperand::ConstInt(n) => {
                // RFC 015 Phase 2: integer literals wider than i32 emit as i64
                // so that `long`-typed values (e.g. `9223372036854775807`) type-check
                // in LLVM IR. Narrower contexts (short/byte locals) truncate via
                // `coerce_value` at the assignment site.
                if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                    ("i64".into(), n.to_string())
                } else {
                    ("i32".into(), n.to_string())
                }
            }
            MirOperand::ConstFloat(f) => ("double".into(), format!("{f:?}")),
            MirOperand::ConstString(s) => {
                let global = self.intern_string(s);
                let len = s.len() + 1;
                (
                    "ptr".into(),
                    format!("getelementptr inbounds ([{len} x i8], ptr {global}, i32 0, i32 0)"),
                )
            }
            MirOperand::ConstBool(b) => ("i1".into(), if *b { "1".into() } else { "0".into() }),
            MirOperand::ConstNull => ("ptr".into(), "null".into()),
            // RFC 040：`default(T)` 类型化默认值操作数（单态化后 type_name 为具体
            // 类型名）。按具体类型发射默认值：基元 → 零/false；其余（string/object/
            // 类/接口/泛型实例）→ null 指针。与 MIR `default_operand_for_type`
            // 语义对齐；`default(bool)` 由此正确发射 `i1 0` 而非 `ptr null`。
            MirOperand::ConstDefault { type_name } => {
                // RFC 012 S6 A1（struct default）：registry struct → 栈 zeroinit
                // 存储（entry 提升，循环重入复用同一份），返回其地址——struct 值
                // 的 ptr 表示下这是唯一合法的「零值」。class/接口/泛型实例不在
                // structs → null（引用默认值，与旧行为一致）。
                if let Some(sl) = self.layouts.structs.get(type_name.as_str()) {
                    let agg = format!("%struct.{type_name}");
                    let _ = sl;
                    let slot = self.fresh_temp();
                    self.entry_allocas
                        .push_str(&format!("  {slot} = alloca {agg}\n"));
                    self.emit(&format!("store {agg} zeroinitializer, ptr {slot}"));
                    return ("ptr".into(), slot);
                }
                match type_name.as_str() {
                    "int" | "uint" | "char" => ("i32".into(), "0".into()),
                    "long" | "ulong" => ("i64".into(), "0".into()),
                    "short" | "ushort" => ("i16".into(), "0".into()),
                    "byte" | "sbyte" => ("i8".into(), "0".into()),
                    "float" => ("float".into(), "0.0".into()),
                    "double" => ("double".into(), "0.0".into()),
                    "bool" => ("i1".into(), "0".into()),
                    _ => ("ptr".into(), "null".into()),
                }
            }
            MirOperand::AddrOf(id) => {
                let ty = self.local_type(*id);
                if matches!(ty, TypeId::Ref { .. }) {
                    // Forwarding a ref param: load the stored pointer from the slot
                    let slot_ptr = self.local_ptr(*id);
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = load ptr, ptr {slot_ptr}"));
                    ("ptr".into(), tmp)
                } else {
                    let ptr = self.local_ptr(*id);
                    ("ptr".into(), ptr)
                }
            }
            MirOperand::Field {
                object,
                class,
                field,
            } => self.emit_field_get(object, class, field),
            MirOperand::Iface {
                object,
                class,
                iface,
            } => self.emit_make_iface(class, iface, object, true),
            MirOperand::UnboxIface { object, .. } => {
                // interface → 具体类转型：fat-pointer 盒 { ptr obj, ptr itable }
                // 取首槽底层对象指针。盒本身无 ArcHeader，绝不可对盒做 rt_arc_inc/dec
                // 或把盒地址当类对象（旧实现 → 0xC0000005 / 0xC0000409）。
                let (_, box_ptr) = self.emit_operand(object);
                let obj = self.fresh_temp();
                self.emit(&format!("{obj} = load ptr, ptr {box_ptr}"));
                ("ptr".into(), obj)
            }
            MirOperand::UnboxString { object } => {
                // RFC 045 P2：object→string 拆箱（is string 收窄 / 窄化 Cast 的
                // 叶子路径）。ArcStringBox（ArcHeader + char* @ offset 16）经
                // rt_string_unbox 提取（含 vtable 校验，null 入参返回 null）。
                // 与 MirRvalue::Unbox 的 string 分支同语义。
                let (_, box_ptr) = self.emit_operand(object);
                let str_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{str_ptr} = call ptr @rt_string_unbox(ptr {box_ptr})"
                ));
                ("ptr".into(), str_ptr)
            }
            MirOperand::UnboxGeneric { object, type_name } => {
                // 泛型 unbox cast：`(T)obj` 单态化后 type_name 为具体类型名。
                // string → rt_string_unbox（对齐 UnboxString）；
                // 基元值类型 → rt_box_unbox + load（对齐 MirRvalue::Unbox 值类型
                // 路径：size 校验 + memcpy，payload @ offset 24，size 与 emit_box
                // 装箱一致——llvm_size_of）；其余引用类型 → 类型断言直接透传。
                let (_, src_val) = self.emit_operand(object);
                match type_name.as_str() {
                    "string" => {
                        let result = self.fresh_temp();
                        self.emit(&format!(
                            "{result} = call ptr @rt_string_unbox(ptr {src_val})"
                        ));
                        ("ptr".into(), result)
                    }
                    ty @ ("int" | "uint" | "char" | "long" | "ulong" | "short" | "ushort"
                    | "byte" | "sbyte" | "float" | "double" | "bool") => {
                        let llvm_ty: &str = match ty {
                            "int" | "uint" | "char" => "i32",
                            "long" | "ulong" => "i64",
                            "short" | "ushort" => "i16",
                            "byte" | "sbyte" => "i8",
                            "float" => "float",
                            "double" => "double",
                            _ => "i1", // bool
                        };
                        let size = llvm_size_of_type_str(ty) as i32;
                        let slot = self.fresh_temp();
                        self.emit(&format!("{slot} = alloca {llvm_ty}"));
                        let status = self.fresh_temp();
                        self.emit(&format!(
                            "{status} = call i32 @rt_box_unbox(ptr {src_val}, i32 {size}, ptr {slot}, i32 {size})"
                        ));
                        let result = self.fresh_temp();
                        self.emit(&format!("{result} = load {llvm_ty}, ptr {slot}"));
                        (llvm_ty.to_string(), result)
                    }
                    _ => {
                        // 引用类型（类/接口/泛型实例）：类型断言直接透传对象指针。
                        ("ptr".into(), src_val)
                    }
                }
            }
            MirOperand::FnPtr { name } => ("ptr".into(), format!("@{}", mangle_fn_name(name))),
            MirOperand::Closure { fn_name, env } => self.emit_closure_value(fn_name, env),
            // RFC 018 M2 step 4 / M5：`typeof(T)` → RuntimeType 实例 ptr。
            //
            // typeck 推断 typeof(T) 表达式类型为 RuntimeType（具体子类），
            // codegen 发射 RuntimeType 实例（calloc + refcount=1 + vtable
            // + _typeInfoHandle = ptrtoint(@.typeinfo.{T} / @rt_typeinfo_<prim> to i64)）。
            //
            // 用户可：
            // - `RuntimeType? rt = typeof(T)` 直接获取
            // - `Type? t = typeof(T)` 多态赋值（RuntimeType : Type）
            // - `typeof(T).TypeId` 取 int 类型身份（codegen 拦截器读取 RtTypeInfo.type_id @ offset 0）
            // - `typeof(T).Name` / `.FullName` / `.Kind` / `.BaseType` 等元数据查询
            //
            // 引用类型（class/interface）引用 emit_typeinfos 发射的 @.typeinfo.{T}；
            // 基元类型经 rt_typeinfo_prim(id) 函数符号查询（RFC 017 阶段一，
            // 与 MirOperand::TypeInfoPtr 分支同一映射）。若无法发射 RuntimeType
            // （stdlib 未编译 / 类型无 typeinfo），返回 null ptr。
            // 语言层 Arc.TypeId struct 已于 M5 删除；MIR 仍用 MirOperand::TypeId 作
            // typeof 操作数名（历史命名，与语言类型无关）。
            //
            // DI 容器（emit_di.rs）仍从 MirOperand::TypeId 直接提取 type_name。
            MirOperand::TypeId { type_name } => {
                if let Some(tmp) = self.try_emit_typeof_as_runtime_type(type_name) {
                    return ("ptr".into(), tmp);
                }
                // M5：不再 fallback 到 %struct.TypeId；未命中则 null。
                ("ptr".into(), "null".into())
            }
            // RFC 018 M1 + RFC 018 §5.2.2：`@.typeinfo.{Class}` 全局常量指针。
            //
            // 用户类型（class/struct/interface/enum）直接引用 codegen
            // emit_typeinfos 发射的 `@.typeinfo.{Type}` 全局常量；M1 typeck
            // 已保证 `is T` 的 T 必有 vtable（emit_typeinfos 一定发射）。
            //
            // 基元类型（int/long/short/byte/char/float/double/bool/string/void/object）
            // 按 RFC 018 §5.2.2 由 runtime 静态初始化（rt_type.c 中的
            // `rt_typeinfo_<prim>` 全局变量），codegen 不重复发射，直接引用
            // runtime 符号 `@rt_typeinfo_<prim>`。
            MirOperand::TypeInfoPtr { type_name } => {
                // RFC 038 M2：统一守卫（外部类型登记 external 声明；无 typeinfo
                // 的类型发 null——rt_obj_isa 对 null target 返回 0，不崩溃）。
                let sym = self
                    .typeinfo_global(type_name.as_str())
                    .unwrap_or_else(|| "null".into());
                ("ptr".into(), sym)
            }
            // RFC 006 M4：静态字段读取——load 全局变量 `@__static_<class>_<field>`。
            // 字段 LLVM 类型由 layouts.static_fields 中的 ty 推导；若不在 static_fields
            // 中（理论上不应发生——typeck M3 已收集所有静态字段），回退到 ptr。
            MirOperand::StaticField { class, field } => {
                let global = format!("@__static_{class}_{field}");
                // 查找该静态字段的 typeck layout，推导字段 LLVM 类型；
                // 若不在 static_fields 中（理论上不应发生——typeck M3 已收集全部），
                // 回退到 ptr。
                let sf = self
                    .layouts
                    .static_fields
                    .iter()
                    .find(|s| s.class.as_str() == class && s.field.as_str() == field);
                let ty_str = sf
                    .map(|s| {
                        let type_id = match s.ty.as_str() {
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
                        };
                        llvm_type_of(&type_id, self.layouts)
                    })
                    .unwrap_or_else(|| "ptr".to_string());

                // RFC 006 A3 S3：惰性静态字段读取须先确保已完成惰性初始化。
                // 快速路径单原子 acquire 读类级标志；未初始化则 call `__lazy_init_<Class>`
                // 慢路径（线程安全：rt_lazy_init_begin/commit 保证初始化恰一次）。
                if sf.is_some_and(|s| s.is_lazy) {
                    let lazy_global = format!("@__lazy_{class}");
                    let lazy_fn = format!("@__lazy_init_{class}");
                    let st = self.fresh_temp();
                    self.emit(&format!(
                        "{st} = load atomic i32, ptr {lazy_global} acquire, align 4"
                    ));
                    let done = self.fresh_temp();
                    self.emit(&format!("{done} = icmp eq i32 {st}, 2"));
                    let check_bb = self.fresh_label();
                    let merge_bb = self.fresh_label();
                    self.emit(&format!(
                        "br i1 {done}, label %{merge_bb}, label %{check_bb}"
                    ));
                    self.emit(&format!("{check_bb}:"));
                    self.emit(&format!("call void {lazy_fn}()"));
                    self.emit(&format!("br label %{merge_bb}"));
                    self.emit(&format!("{merge_bb}:"));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = load {ty_str}, ptr {global}"));
                    return (ty_str, tmp);
                }

                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = load {ty_str}, ptr {global}"));
                (ty_str, tmp)
            }
        }
    }

    // ---- Helpers ----

    pub(super) fn is_string_operand(&self, op: &MirOperand) -> bool {
        match op {
            MirOperand::ConstString(_) => true,
            MirOperand::Local(id) => {
                // `string?`（Nullable{String}）与 `string` 同一运行时形态（句柄
                // ptr，可空性仅编译期注解）——相等/比较须走 rt_str_equals 值
                // 比较，而非通用 icmp ptr（literal 驻留下指针比较碰巧正确，
                // 运行时构造的堆串则必错）。
                match self.local_type(*id) {
                    TypeId::String => true,
                    TypeId::Nullable { inner } => matches!(*inner, TypeId::String),
                    _ => false,
                }
            }
            MirOperand::Field { class, field, .. } => {
                // Look up the field's Arc type name; "string" fields are
                // string operands eligible for rt_str_concat dispatch.
                let (class, field) = (class.as_str(), field.as_str());
                let field_ty = if self.layouts.structs.contains_key(class) {
                    self.struct_field_info(class, field).1
                } else {
                    self.field_info(class, field).1
                };
                field_ty == "string"
            }
            _ => false,
        }
    }

    /// Emit a closure value (RFC 008): allocate env struct + arc_closure on heap.
    /// Returns `(ptr, closure_ptr)` where `closure_ptr` points to a heap-allocated
    /// `%arc_closure = { ptr, ptr }` (fn_ptr, env_ptr).
    ///
    /// **堆分配（非 alloca）**：闭包结构体可能被存入字段/容器/作为委托实参跨
    /// 函数传递（如声明式 `Click="Method"` 的 `child.OnClick(_ => this.OnX())`、
    /// 用户直接 `_cb = (v) => this.OnX()`），宿主方法栈帧销毁后仍须有效。
    /// alloca 分配的闭包指针在宿主返回后悬垂 → 延迟调用读垃圾指针/崩溃。
    /// 与 env 结构体（已堆分配）一致，此处闭包结构体同样用 `@malloc(16)`。
    /// 闭包当前不参与 ARC（无 ArcHeader），生命周期为 leak-until-exit（与
    /// env 一致，H1 注释），改动仅引入每闭包 16B 堆分配，无 UAF/双放。
    ///
    /// When `env` is empty (no captures), env_ptr is `null` and no env struct is allocated.
    ///
    /// RFC 008 L2: each env field is typed according to its `CaptureMode`:
    /// `ByRef` → `ptr` (class/string/... pointer); `ByValue` → the value's LLVM
    /// type (e.g. `i32` for `int`, `double` for `double`). The source operand
    /// is loaded and stored into the field with the matching type.
    pub(super) fn emit_closure_value(
        &mut self,
        fn_name: &str,
        env: &[(ast::LambdaCapture, MirOperand)],
    ) -> (String, String) {
        // 定形捕获簇为主：`%arc_closure` 头（fn_ptr/env_ptr）+ 紧随其后的真实捕获
        // 结构体（`env_struct_type`）合并为**单块**堆分配。保持 RFC 005/006/008
        // H1 的 escape-safe 堆生命周期（闭包可入字段/容器、跨 suspend、跨函数
        // 传递、入 TLS 回调槽，宿主帧销毁后仍须有效），故不引入 alloca；本改动
        // 仅把原「闭包头 malloc(16) + env malloc(env_size)」两块收敛为一块
        // 「捕获簇 + 定形 capture struct」，行为等价（消费者只读 closure[0]=fn、
        // closure[1]=env、env 字段；且闭包与 env 均 leak-until-exit，无处 free）。
        // 更贴近 ir-mapping §3「捕获簇 … 定形 capture struct」，每闭包 malloc 由
        // 2 次降为 1 次。
        let captures: Vec<&ast::LambdaCapture> = env.iter().map(|(c, _)| c).collect();
        let env_size = if captures.is_empty() {
            0
        } else {
            self.env_struct_size(&captures)
        };
        let closure_ptr = self.fresh_temp();
        self.emit(&format!(
            "{closure_ptr} = call ptr @malloc(i64 {})",
            16 + env_size
        ));

        // Store fn_ptr (field 0)
        let fn_global = format!("@{}", mangle_fn_name(fn_name));
        let fn_field = self.fresh_temp();
        self.emit(&format!(
            "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
        ));
        self.emit(&format!("store ptr {fn_global}, ptr {fn_field}"));

        // Store env_ptr (field 1): null if no captures, else ptr to the trailing
        // capture struct（字节偏移 16 = sizeof(%arc_closure) = 2 × 8B ptr）。
        let env_field = self.fresh_temp();
        self.emit(&format!(
            "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
        ));
        if env.is_empty() {
            self.emit(&format!("store ptr null, ptr {env_field}"));
        } else {
            let env_ty = self.env_struct_type(&captures);
            let env_struct = self.fresh_temp();
            self.emit(&format!(
                "{env_struct} = getelementptr i8, ptr {closure_ptr}, i64 16"
            ));
            for (i, (capture, src)) in env.iter().enumerate() {
                let field_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{field_ptr} = getelementptr {env_ty}, ptr {env_struct}, i32 0, i32 {i}"
                ));
                // ByRef 捕获（class/string/接口引用）——变量捕获（RFC 008 by-ref）：
                // env 槽存**外层变量槽地址**，lambda 内经该地址读写外层变量。
                // 这样 `invokedName = c.Name` 等对捕获变量的重新绑定能传播到外层
                // （C# 闭包语义）。旧实现存值快照，lambda 内赋值不写回 → 外层
                // 读旧值（stream_events `fail:invoked` 实测）。
                // 捕获点在外层 async 状态机 resume 内时，外层变量的权威槽位是
                // async env 字段（3+local_id，跨 suspend 稳定）；同步函数是 alloca。
                let (src_ty, src_val) = if matches!(capture.mode, ast::CaptureMode::ByRef) {
                    match src {
                        MirOperand::Local(id) => {
                            // ByRef 再捕获（嵌套 lambda）：源变量本身已是本函数的
                            // ByRef 捕获变量时，其槽位存的是**外层权威槽地址**
                            // （捕获恢复写入，emit_operand 两次 load 解引用）。
                            // 此处传递槽地址本身（load 一层），使内层闭包直接
                            // 指向最外层变量槽——否则每嵌套一层多一重间接
                            // （流式 TTS e2e：块 lambda 读 ct 得槽地址而非对象，
                            // rt_cts_is_canceled 读垃圾 → 假 OCE → WhenAny 假超时）。
                            let src_is_byref_capture = self
                                .cfg
                                .captures
                                .iter()
                                .find(|(cid, _, _)| *cid == *id)
                                .map(|(_, _, c)| matches!(c.mode, ast::CaptureMode::ByRef))
                                .unwrap_or(false);
                            if src_is_byref_capture {
                                let slot_addr = self.fresh_temp();
                                self.emit(&format!(
                                    "{slot_addr} = load ptr, ptr {}",
                                    self.local_ptr(*id)
                                ));
                                ("ptr".into(), slot_addr)
                            } else if self.in_state_machine
                                && !self.sm_env_type.is_empty()
                                && self.sm_env_local_index.contains_key(id)
                            {
                                // RFC 016：env 字段索引改用 liveness 收敛的
                                // `local_env_field_index`（不再用固定 `3+local_id`）。
                                let slot = self.fresh_temp();
                                self.emit(&format!(
                                    "{slot} = getelementptr {}, ptr %env_ptr, i32 0, i32 {}",
                                    self.sm_env_type, self.sm_env_local_index[id]
                                ));
                                // M2 SM：捕获点（首个 await 的 poll 内联推进）可能在
                                // save_locals 之前执行——env 槽仍为 null，lambda 经槽读
                                // 得 null → GEP 崩溃。此处把捕获变量当前值同步写入槽
                                // （class 引用 ARC 覆写 inc 新/dec 旧；string 无
                                // ArcHeader 裸 store），使槽始终反映当前值。
                                // RFC 016：env 为唯一 owner——写穿 inc 新/dec 旧，
                                // 与 `emit_env_owned_class_store` 语义一致。
                                // 捕获变量类型感知（标量 ByRef 捕获修复）：int 等
                                // 值类型按 llvm_type_of 镜像——硬编码 `load ptr`
                                // 会把 4 字节 int 槽当 8 字节地址读，`store ptr`
                                // 写穿相邻 env 字段。非 ARC 类型（值类型/string）
                                // 无需 inc/dec 配对，old_slot 亦为死代码。
                                let cur_ty = llvm_type_of(&capture.ty, self.layouts);
                                let cur = self.fresh_temp();
                                let cur_src = self.local_ptr(*id);
                                self.emit(&format!("{cur} = load {cur_ty}, ptr {cur_src}"));
                                if Self::arc_class_place(&capture.ty, self.layouts) {
                                    self.emit(&format!("call void @rt_arc_inc(ptr {cur})"));
                                    let old_slot = self.fresh_temp();
                                    self.emit(&format!("{old_slot} = load ptr, ptr {slot}"));
                                    self.emit(&format!("store ptr {cur}, ptr {slot}"));
                                    self.emit(&format!("call void @rt_arc_dec(ptr {old_slot})"));
                                } else {
                                    self.emit(&format!("store {cur_ty} {cur}, ptr {slot}"));
                                }
                                ("ptr".into(), slot)
                            } else {
                                // 未提升局部（非跨 await 存活）或同步函数：env 无
                                // 该字段，用 alloca 地址。闭包仅在单 resume 段内被
                                // 创建并消费时安全（该段内 alloca 存活）。
                                ("ptr".into(), self.local_ptr(*id))
                            }
                        }
                        _ => self.emit_operand(src),
                    }
                } else {
                    // RFC 045（di_decorate 崩溃根因）：ByValue 捕获**委托/函数指针
                    // 类型**值（无捕获 lambda 优化为裸 FnPtr，存局部/委托变量）统一
                    // 包装为 arc_closure {fn, null}——闭包体内按 closure 结构 GEP
                    // 解引用 fn/env，裸 fn 被解引用 → 0xC0000005（`h = x => g(x)+10`
                    // 捕获 g 崩溃；DI 装饰工厂捕获 Func 变量同根）。值已是 closure
                    // （有捕获 lambda）时包装为 {closure_addr, null}——闭包体内解
                    // 引用 closure_addr 后其 fn/env 槽仍正确（双层解引用自洽）。
                    // 解包可空（`Func<...>?`——DI Factory 字段即可空委托）：
                    // Nullable{Func} 的 env 槽同样是 ptr。
                    let _fn_ty = match &capture.ty {
                        TypeId::Nullable { inner } => inner.as_ref(),
                        other => other,
                    };
                    // RFC 045：FnPtr 存储统一为 arc_closure 后，捕获值恒为
                    // closure 指针——直接存储。裸 FnPtr operand 捕获（内联
                    // lambda 值）包装兜底（闭包体内按 closure 解引用）。
                    let (v_ty, v_val) = if matches!(src, MirOperand::FnPtr { .. }) {
                        self.emit_operand_as_closure(src)
                    } else {
                        self.emit_operand(src)
                    };
                    // 循环局部（ByValue 快照）持有的引用对象：闭包创建点 rt_arc_inc
                    // 把「借用」提升为 env 的强引用，避免外层循环槽被下一次迭代覆写
                    // dec 后闭包读到已释放对象（web 连接线程交叉串连 UAF）。env 结构
                    // 生命周期为进程级（leak-until-exit），该强引用随之泄漏——与既有
                    // `this` ByValue 捕获的借用模型一致，只增不减，无双重释放。
                    if Self::arc_class_place(&capture.ty, self.layouts) {
                        self.emit(&format!("call void @rt_arc_inc(ptr {v_val})"));
                    }
                    (v_ty, v_val)
                };
                self.emit(&format!("store {src_ty} {src_val}, ptr {field_ptr}"));
            }
            self.emit(&format!("store ptr {env_struct}, ptr {env_field}"));
        }

        ("ptr".into(), closure_ptr)
    }

    /// Malloc-backed closure construction for async-escape contexts.
    /// `emit_closure_value` 现已统一堆分配闭包结构体（RFC 006 M5 G2），
    /// 两路径语义一致，此处委托以保持单一实现。
    ///
    /// H1: trampoline **does not** free the malloc'd env (leak-until-exit;
    /// early free interleaved with suite teardown damaged the heap).
    /// Work struct `w` is still freed after Task complete.
    #[allow(dead_code)]
    pub(super) fn emit_closure_value_heap(
        &mut self,
        fn_name: &str,
        env: &[(ast::LambdaCapture, MirOperand)],
    ) -> (String, String) {
        self.emit_closure_value(fn_name, env)
    }

    /// Return the byte size of an env struct for given captures.
    /// Rough estimate: sizeof(ptr) per ByRef, sizeof(T) per ByValue.
    ///
    /// RFC 045（di_decorate 崩溃根因）：ByValue 捕获引用类型（class/string/委托
    /// /接口等）时 env 槽是 `ptr`（8 字节，与 `env_struct_type` 的 `llvm_type_of`
    /// 一致）；旧实现默认 4 使 `malloc(4)` 分配不足 → store 8 字节越界写相邻堆块
    /// → 0xC0000005（装饰工厂闭包捕获 `Func` 委托变量实测崩溃）。
    fn env_struct_size(&self, captures: &[&ast::LambdaCapture]) -> i32 {
        let mut size: i32 = 0;
        for c in captures {
            match c.mode {
                ast::CaptureMode::ByRef => size += 8,
                ast::CaptureMode::ByValue => {
                    match &c.ty {
                        TypeId::Long | TypeId::Double | TypeId::ULong => size += 8,
                        TypeId::Float => size += 4,
                        TypeId::Int | TypeId::Bool | TypeId::UInt => size += 4,
                        TypeId::Short
                        | TypeId::Byte
                        | TypeId::Char
                        | TypeId::UShort
                        | TypeId::SByte => size += 2,
                        // 引用类型/委托/接口/其它：env 槽为 ptr（8 字节）。
                        _ => size += 8,
                    }
                }
            }
        }
        if size == 0 {
            size = 8;
        }
        size
    }

    /// Emit an operand that must be an `arc_closure*` regardless of whether the
    /// source lambda has captures.
    ///
    /// - `MirOperand::Closure { fn_name, env }` → uses `emit_closure_value`
    ///   (existing behavior, returns ptr to stack-allocated `{fn_ptr, env}`).
    /// - `MirOperand::FnPtr { name }` → synthesizes a temporary `arc_closure`
    ///   with `fn_ptr = @name` and `env = null`. Required for `ct.Register`
    ///   and `Task.ContinueWith` where the runtime trampoline dereferences
    ///   `closure->fn_ptr` / `closure->env` — passing a bare function pointer
    ///   would cause an access violation.
    /// - Other operands (e.g. `ConstNull`) → passed through as `ptr` (caller
    ///   is responsible for ensuring runtime handles NULL safely).
    ///
    /// **堆分配（非 alloca）**：合成的 arc_closure 必须在状态机 suspend 后仍有效
    /// （如 ContinueWith 在 antecedent 完成后才调用 closure，可能跨越多次 suspend），
    /// 且作为委托实参跨函数传递时可能被存入堆（如 Signal 的订阅列表），
    /// alloca 分配的栈帧在函数返回/suspend 后失效，会导致 use-after-free。
    /// 因此 FnPtr 路径用 `@malloc(16)` 堆分配；Closure 路径同样用 `@malloc`
    /// （`emit_closure_value_heap`），与 FnPtr 路径保持一致。
    pub(super) fn emit_operand_as_closure(&mut self, op: &MirOperand) -> (String, String) {
        match op {
            MirOperand::Closure { fn_name, env } => self.emit_closure_value_heap(fn_name, env),
            MirOperand::FnPtr { name } => self.emit_fnptr_as_closure(name),
            MirOperand::Local(id) if self.closure_locals.contains(id) => self.emit_operand(op),
            MirOperand::Local(_) => {
                // CD-23 后委托值统一为 arc_closure 表示（存储/返回/捕获，裸 fn 仅存
                // 直接调用路径）——委托局部（方法组委托 `Action<T,T> h = W.OnC;`、
                // 无捕获 lambda 赋值、捕获闭包）统一存 closure 指针，直接传值。
                // 旧注释「Func 局部可能仍存裸 FnPtr」在 CD-23 统一后不再成立；
                // 二次包装会把 closure 指针当 fn 字段（fn=closure → 调用代码指针 → AV，
                // X19 方法组 / X21 无捕获 lambda 传参实测 0xC0000005）。
                self.emit_operand(op)
            }
            _ => self.emit_operand(op),
        }
    }

    fn emit_fnptr_as_closure(&mut self, name: &str) -> (String, String) {
        let fn_global = format!("@{}", mangle_fn_name(name));
        self.emit_raw_fn_val_as_closure(&fn_global)
    }

    /// 堆分配 `arc_closure = { fn_ptr, env=null }`（同步跨函数传参 + async escape 安全）。
    fn emit_raw_fn_val_as_closure(&mut self, fn_val: &str) -> (String, String) {
        let closure_ptr = self.fresh_temp();
        self.emit(&format!("{closure_ptr} = call ptr @malloc(i64 16)"));
        let fn_field = self.fresh_temp();
        self.emit(&format!(
            "{fn_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 0"
        ));
        self.emit(&format!("store ptr {fn_val}, ptr {fn_field}"));
        let env_field = self.fresh_temp();
        self.emit(&format!(
            "{env_field} = getelementptr %arc_closure, ptr {closure_ptr}, i32 0, i32 1"
        ));
        self.emit(&format!("store ptr null, ptr {env_field}"));
        ("ptr".into(), closure_ptr)
    }

    /// LLVM type string for an env struct with `captures` fields.
    ///
    /// `ByRef` captures → `ptr` (class/string/... pointer).
    /// `ByValue` captures → the value's LLVM type (e.g. `i32`, `double`, ...).
    pub(super) fn env_struct_type(&self, captures: &[&ast::LambdaCapture]) -> String {
        let fields: Vec<String> = captures
            .iter()
            .map(|c| match c.mode {
                ast::CaptureMode::ByRef => "ptr".into(),
                ast::CaptureMode::ByValue => llvm_type_of(&c.ty, self.layouts),
            })
            .collect();
        format!("{{ {} }}", fields.join(", "))
    }

    /// RFC 018 M2 step 4: 尝试将 `typeof(T)` 发射为 RuntimeType 实例。
    ///
    /// 命中条件：
    /// - type_name 为基元类型（int/long/short/byte/char/float/double/bool/string/void/object）
    ///   ——RFC 017 阶段一经 `rt_typeinfo_prim(id)` 导出函数运行期查询（数据符号已 static 化）
    /// - 或 type_name 对应 class/interface 在 layouts.classes/interfaces 中
    ///   （emit_typeinfos 2026-07-31 起不限 has_vtable，`typeof(IFace)` / `typeof(纯数据类)`
    ///   均需类型身份）
    /// - RuntimeType 类自身在 layouts.classes 中（stdlib 已编译）
    /// - RuntimeType._typeInfoHandle 字段存在
    ///
    /// 命中时构造 RuntimeType 实例：
    /// - calloc 分配并零初始化
    /// - refcount=1（offset 0）
    /// - vtable=@.vtable.RuntimeType（offset 8）
    /// - _typeInfoHandle=ptrtoint(ptr @.typeinfo.{T} to i64)（命名类型）或
    ///   ptrtoint(rt_typeinfo_prim(id)) 查询值（基元，RFC 017 阶段一）
    ///
    /// 未命中返回 None（调用方发射 null ptr；语言层 TypeId struct 已于 M5 删除）。
    pub(super) fn try_emit_typeof_as_runtime_type(&mut self, type_name: &str) -> Option<String> {
        // typeinfo 为所有 class + interface 发射（emit_typeinfos 2026-07-31 起
        // 不限 has_vtable）；`typeof(IFace)` / `typeof(纯数据类)` 均需类型身份。
        //
        // 基元类型（int/long/short/byte/char/float/double/bool/string/void/object）
        // 不在 layouts.classes/interfaces 中——由 runtime 静态初始化（rt_type.c），
        // RFC 017 阶段一后经 `rt_typeinfo_prim(id)` 函数符号查询（指令语境可
        // call，与 MirOperand::TypeInfoPtr 分支同一映射）。此前 `typeof(基元)`
        // 未命中直接发射 null，消费方对 owner 做 getelementptr + load 即
        // 0xC0000005 空指针崩溃；现在与引用类型走同一 RuntimeType 构造路径。
        let handle_expr = match primitive_typeinfo_id(type_name) {
            Some(prim_id) => {
                // 基元：call 返回的 ptr 须 ptrtoint 转 i64 后方可作 handle_expr
                // （_typeInfoHandle 为 i64；命名类型臂的 ptrtoint 常量表达式
                // 天然合法，寄存器值必须显式转换，否则 store i64 类型不匹配）。
                let ti = self.fresh_temp();
                self.emit(&format!("{ti} = call ptr @rt_typeinfo_prim(i32 {prim_id})"));
                let handle = self.fresh_temp();
                self.emit(&format!("{handle} = ptrtoint ptr {ti} to i64"));
                handle
            }
            None => {
                if !self.layouts.classes.contains_key(type_name)
                    && !self.layouts.interfaces.contains_key(type_name)
                {
                    return None;
                }
                // RFC 038 M2：外部类型（external_class_names）经守卫登记 external
                // typeinfo 声明，由定义包 linkonce_odr 定义解析。
                let typeinfo_global = self.typeinfo_global(type_name)?;
                format!("ptrtoint (ptr {typeinfo_global} to i64)")
            }
        };

        const RUNTIME_TYPE: &str = "RuntimeType";
        let rt_layout = self.layouts.classes.get(RUNTIME_TYPE)?;
        let handle_offset = rt_layout
            .fields
            .iter()
            .find(|f| f.name.as_str() == "_typeInfoHandle")?
            .offset;

        // 构造 RuntimeType 实例，_typeInfoHandle = handle_expr（基元为
        // ptrtoint(rt_typeinfo_prim(id))，命名类型为 ptrtoint(@.typeinfo.{T})）。
        let tmp = self.emit_new_runtime_typeinfo_with_handle_expr(&handle_expr, handle_offset);
        Some(tmp)
    }
}

/// FNV-1a 32-bit hash of a type name → deterministic `Type.TypeId` (int).
///
/// Used when emitting `@.typeinfo.{T}` (`RtTypeInfo.type_id`) and by DI
/// lookup keys (`serviceType.TypeId`). Determinism is critical: the same
/// type name must yield the same ID across all functions and compilation
/// units.
///
/// Collision probability is negligible for typical type name lengths (<100 chars):
/// FNV-1a has good distribution for short strings. The 32-bit space accommodates
/// ~65K types before birthday-paradox collisions become likely.
pub(crate) fn type_name_to_id(name: &str) -> i32 {
    let mut hash: u32 = 2166136261;
    for byte in name.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16777619);
    }
    // Ensure non-zero (0 reserved for "unknown/unassigned").
    if hash == 0 {
        1
    } else {
        hash as i32
    }
}

/// RFC 017：Entry 导出符号的**布局指纹**（FNV-1a-64）。
///
/// 输入为类型的完整数据布局传递闭包：沿类继承链递归展开自定义复合类型
/// 字段（基类字段在前），直到基元 / 枚举 / variant / 布局不可见类型等叶子；
/// 每层计入类型名、字段偏移与字段类型。指纹只编码内存布局真值，不含字段
/// 名（重命名不改布局）与方法/属性。
///
/// 用途：Entry 导出符号追加指纹段（`__arc_entry__{TR_id}_{TR_sig}` /
/// `__arc_entry_{TP_id}_{TR_id}_{TP_sig}_{TR_sig}`），使宿主与插件对同名
/// 类型的布局漂移（热重载换代后字段增删/改型，含嵌套字段类型的深层变化）
/// 从 ABI 静默错配变为加载期显式 `EntryPointNotFoundException`。与
/// [`type_name_to_id`] 相互独立：后者只哈希类型名（RFC 026 typeinfo 三端
/// 共识，勿动），本指纹仅注入 Entry 符号层。指纹算法单点居 codegen，
/// 宿主与插件经同一编译器编译，双端天然同源。
///
/// 基元等叶子类型的指纹即类型名哈希（两端同名恒一致）；0 保留为空名退化
/// 哨兵（同 [`type_name_to_id`] 的 0 保留约定）。std 等被引用类型版本不
/// 一致时指纹不同而拒载——版本混载本就不应静默通过。
pub(crate) fn entry_layout_signature(layouts: &ProgramLayouts, type_name: &str) -> u64 {
    let init: u64 = 0xcbf2_9ce4_8422_2325;
    let mut hash = init;
    let mut seen: Vec<String> = Vec::new();
    hash_layout_type(layouts, type_name, &mut hash, &mut seen, 0);
    if hash == init { 0 } else { hash }
}

/// [`entry_layout_signature`] 的递归展开：类型名入哈希后按 classes →
/// structs 顺序查布局展开字段（枚举布局恒为 i32 判别值、variant 与未知
/// 类型作叶子，仅计入类型名）。`seen` 防循环引用，`depth` 防病态深链。
fn hash_layout_type(
    layouts: &ProgramLayouts,
    type_name: &str,
    hash: &mut u64,
    seen: &mut Vec<String>,
    depth: u32,
) {
    const MAX_DEPTH: u32 = 16;
    if depth > MAX_DEPTH {
        return;
    }
    hash_bytes(hash, type_name.as_bytes());
    if seen.iter().any(|s| s == type_name) {
        return;
    }
    seen.push(type_name.to_string());
    if let Some(class) = layouts.classes.get(type_name) {
        // 基类字段在对象内存布局中位于派生字段之前，哈希序与之对齐。
        if let Some(parent) = &class.parent {
            hash_layout_type(layouts, parent.as_str(), hash, seen, depth + 1);
        }
        for field in &class.fields {
            hash_bytes(hash, &field.offset.to_le_bytes());
            hash_layout_type(layouts, field.ty.as_str(), hash, seen, depth + 1);
        }
    } else if let Some(strct) = layouts.structs.get(type_name) {
        for field in &strct.fields {
            hash_bytes(hash, &field.offset.to_le_bytes());
            hash_layout_type(layouts, field.ty.as_str(), hash, seen, depth + 1);
        }
    }
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= *byte as u64;
        *hash = hash.wrapping_mul(0x100_0000_01b3);
    }
}

/// RFC 023 冲刺批次一：type_id 碰撞载荷（编译期诊断数据）。
///
/// [`type_name_to_id`] 为 FNV-1a 32 位哈希，全程序类型数逼近 ~65K 时生日
/// 悖论碰撞概率显著。type_id 相同的不同类型共享运行时类型身份——`is`
/// 判别 / itable 分派 / DI 查找会错配到错误类型（静默错误）。守卫在模块
/// 发射期收集本 TU 全部会获得 type_id 的类型名，首次冲突产出本载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeIdCollision {
    /// 先登记、已占用该 type_id 的类型名。
    pub(crate) existing_name: String,
    /// 后登记、与前者哈希冲突的类型名。
    pub(crate) colliding_name: String,
    /// 二者共享的 type_id（FNV-1a 32 位）。
    pub(crate) type_id: i32,
}

impl TypeIdCollision {
    /// 渲染为编译期诊断文案：两个类型名 + 冲突 type_id + 重命名指引。
    pub(crate) fn render(&self) -> String {
        format!(
            "type_id collision: `{}` and `{}` both hash to type_id {} \
             (FNV-1a 32-bit of the type name); identical type_ids make \
             vtable/itable dispatch resolve to the wrong type — rename \
             one of them",
            self.existing_name, self.colliding_name, self.type_id
        )
    }
}

/// RFC 023 冲刺批次一：type_id 唯一性收集器（编译期碰撞守卫）。
///
/// [`register`](Self::register) 按 [`type_name_to_id`] 同一哈希登记类型名；
/// 同名重复登记合法（同一类型的多处发射共享 id），异名同哈希返回
/// [`TypeIdCollision`]。纯编译期检查，未碰撞程序零开销（不发射任何 IR）。
#[derive(Debug, Default)]
pub(crate) struct TypeIdUniquenessGuard {
    seen: std::collections::HashMap<u32, String>,
}

impl TypeIdUniquenessGuard {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 登记一个将获得 type_id 的类型名；与已登记的异名类型哈希冲突时返回碰撞载荷。
    pub(crate) fn register(&mut self, name: &str) -> Result<(), TypeIdCollision> {
        let type_id = type_name_to_id(name);
        match self.seen.get(&(type_id as u32)) {
            Some(existing) if existing != name => Err(TypeIdCollision {
                existing_name: existing.clone(),
                colliding_name: name.to_string(),
                type_id,
            }),
            Some(_) => Ok(()),
            None => {
                self.seen.insert(type_id as u32, name.to_string());
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod type_id_guard_tests {
    use super::*;

    /// 未碰撞：互异类型名逐一登记全部成功（守卫对正常程序零影响）。
    #[test]
    fn distinct_names_register_cleanly() {
        let mut guard = TypeIdUniquenessGuard::new();
        for name in ["Foo", "Bar", "IShape", "Point", "Color", "List_int"] {
            assert!(guard.register(name).is_ok(), "`{name}` 不应冲突");
        }
    }

    /// 同名重复登记合法：同一类型在多个发射点共享 type_id，不是碰撞。
    #[test]
    fn same_name_re_registration_is_legal() {
        let mut guard = TypeIdUniquenessGuard::new();
        assert!(guard.register("Foo").is_ok());
        assert!(guard.register("Foo").is_ok());
    }

    /// 真实 FNV-1a 32 位碰撞对（暴力搜索验证）：`T323329` 与 `T1134096`
    /// 同哈希 0xc9e043f1。前置断言碰撞对确实同 id——若 `type_name_to_id`
    /// 未来换哈希算法，本测试会在此处失败提示更新碰撞对，而非误报守卫缺陷。
    #[test]
    fn colliding_names_are_rejected() {
        assert_eq!(
            type_name_to_id("T323329"),
            type_name_to_id("T1134096"),
            "碰撞对失效：type_name_to_id 哈希算法已变更，需重新构造碰撞样例"
        );
        let mut guard = TypeIdUniquenessGuard::new();
        assert!(guard.register("T323329").is_ok());
        let collision = guard
            .register("T1134096")
            .expect_err("异名同哈希必须报碰撞");
        assert_eq!(collision.existing_name, "T323329");
        assert_eq!(collision.colliding_name, "T1134096");
        assert_eq!(collision.type_id, type_name_to_id("T323329"));
    }

    /// 诊断文案含两个类型名、冲突 type_id 值与重命名指引。
    #[test]
    fn collision_render_contains_names_id_and_guidance() {
        let collision = TypeIdCollision {
            existing_name: "T323329".to_string(),
            colliding_name: "T1134096".to_string(),
            type_id: type_name_to_id("T323329"),
        };
        let rendered = collision.render();
        assert!(rendered.contains("T323329"));
        assert!(rendered.contains("T1134096"));
        assert!(rendered.contains(&collision.type_id.to_string()));
        assert!(rendered.contains("rename"));
    }
}
