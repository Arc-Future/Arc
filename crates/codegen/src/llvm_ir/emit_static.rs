//! RFC 006 M4：静态字段全局变量、`__sinit_<Class>` 静态初始化器、
//! `@__arc_module_init` 聚合调用。
//!
//! ## 设计要点
//!
//! - **全局变量命名**：`@__static_<Class>_<field>`，类型为字段的 LLVM 类型，
//!   初始化为 `zeroinitializer`（零值）。实际初值由 `__sinit_<Class>` 在
//!   `main` 入口前一次性写入。
//! - **`__sinit_<Class>` 函数**：每个声明了静态字段初始化器的类对应一个，
//!   函数体为对每个 init 表达式生成的 store 指令。当前仅支持字面量
//!   （int/float/string/bool/null）与 `new` 表达式（通过 `__ctor::` 调用）。
//! - **`@__arc_module_init`**：聚合调用所有 `__sinit_<Class>`，按类声明顺序。
//!   `arc` 入口生成器在 `main` 入口前调用此函数。
//! - **动态库**（RFC 017）：`rt_library_load` 后由 `__arc_library_init` 触发，
//!   本模块不直接处理动态库初始化时机，仅提供 `__sinit_<Class>` 符号。
//!
//! ## 线程模型
//!
//! `__sinit` 在 `main` 入口前单线程执行，无需锁。初始化后静态字段视为
//! 不可变（`readonly` 由 typeck 强制）；可变静态字段需用户使用
//! `ConcurrentDictionary` 等线程安全容器。

use super::static_init_deps::type_id_from_name;
use super::*;
use crate::llvm_ir::types::llvm_type_of;
use ast::{Expr, Ident, Type, TypeId, UnaryOp};
use indexmap::IndexMap;
use mir::MirCfgBody;

impl<'a> ModuleEmitter<'a> {
    /// 发射所有静态字段的全局变量声明。
    ///
    /// 输出形如：
    /// ```llvm
    /// @__static_Counter__count = global i32 0
    /// @__static_ModelCache__cache = global ptr null
    /// ```
    ///
    /// 全局变量初值为 `zeroinitializer`（零值），实际初值由 `__sinit_<Class>`
    /// 在 `main` 入口前写入。以 `weak` 链接发射：静态字段宿主类可能由**外部包**
    /// （core_arc 等）提供定义——子库 publish 时其 `.o` 引用但不独占这些类型，
    /// 强 `global` 会导致跨 `.o` 重复强符号（如 `__static_Guid_Empty`）链接冲突；
    /// `weak` 使多单元的同名定义可消解（有强定义则取强，否则任取一份），
    /// 且 `__sinit_<Class>`（linkonce_odr）写入的仍是选中实例。
    /// 注：不用 `extern_weak`——clang 的 IR 解析器拒绝 `extern_weak` 与不透明
    /// `ptr` 类型的组合（`extern_weak global ptr ...` 报 `expected top-level
    /// entity`），`weak` 语义等价且可解析。
    pub fn emit_static_field_globals(&self) -> String {
        if self.layouts.static_fields.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        out.push_str("; ---- RFC 006 M4: Static field globals ----\n");
        for sf in &self.layouts.static_fields {
            // RFC 006 V5：宿主类型属外部包（external_class_names）的静态字段**不**
            // 在本 TU 发射全局定义——其 `@__static_<Class>_<field>` 由所属包
            // （core_arc 等）强定义，消费方仅引用。若也发射，跨 `.o` 重复强符号
            // 触发 lld-link duplicate（如 `__static_Guid_Empty` 在 Lib.o 与
            // core_arc.o 各定义一次）。
            if self.external_class_names.contains(sf.class.as_str()) {
                continue;
            }
            let ty_str = self.static_field_llvm_ty(&sf.ty);
            let init = self.static_field_zero_init(&sf.ty);
            // RFC 005 Copy struct 静态字段（readonly 默认值，如 `ActorId.None`）：
            // 槽须指向真实 zeroinit 存储。struct 统一 ptr 表示（RFC 012 S6 A1）下
            // 读取侧按「load ptr → 解引用」消费，null 槽作 struct ptr 传入 callee
            // 即解引用崩溃。private constant 存储 + 槽以地址常量初始化：无 sinit
            // 写入、天然只读。
            if self.layouts.is_copy_struct(sf.ty.as_str())
                && sf.init.is_none()
                && !sf.is_lazy
                && ty_str == "ptr"
            {
                let storage = format!("@__static_{}_{}.storage", sf.class, sf.field);
                out.push_str(&format!(
                    "{storage} = private constant %struct.{} zeroinitializer\n",
                    sf.ty
                ));
                out.push_str(&format!(
                    "@__static_{}_{} = weak global ptr {storage}\n",
                    sf.class, sf.field
                ));
                continue;
            }
            out.push_str(&format!(
                "@__static_{}_{} = weak global {} {}\n",
                sf.class, sf.field, ty_str, init
            ));
        }

        // RFC 006 A3 S3：类级惰性标志。对含 ≥1 个 is_lazy 字段的类各发射一个
        // `@__lazy_<Class>`（i32 state：0=未初始化,1=初始化中,2=已初始化），
        // 供 `__lazy_init_<Class>` 与 StaticField 读取 guard 使用。
        // 类名转义规则与上面的 `@__static_<Class>_<field>` 完全一致（`sf.class` 原样）。
        // 同样以 `weak` 发射（与静态字段全局一致，跨 .o 可消解）。
        let mut lazy_classes: Vec<Ident> = Vec::new();
        for sf in &self.layouts.static_fields {
            if sf.is_lazy
                && !self.external_class_names.contains(sf.class.as_str())
                && !lazy_classes.contains(&sf.class)
            {
                lazy_classes.push(sf.class.clone());
            }
        }
        if !lazy_classes.is_empty() {
            out.push_str("; ---- RFC 006 A3 S3: class-level lazy guard flags ----\n");
            for class in &lazy_classes {
                out.push_str(&format!("@__lazy_{class} = weak global i32 0\n"));
            }
        }

        out.push('\n');
        out
    }

    /// RFC 017 阶段一：宿主 dbg 表加载期登记调用（`__arc_module_init` entry
    /// 块首段，先于一切静态构造）。
    ///
    /// 仅 MainObject 非 wasm 角色发射——插件 dll 的登记由 `rt_library_load` 持
    /// OS 句柄完成（rt_library.c），wasm 走 rt_wasm_min 内嵌路径无 registry ABI。
    /// 登记键取 `@__arc_dbg_table` 自身地址：非空（rt_debug.c 拒绝 null 句柄）、
    /// 全进程唯一、宿主常驻无卸载配对需求。
    fn render_host_dbg_registration(&self) -> String {
        if matches!(self.emit_role, crate::EmitRole::DynamicLibrary) || self.is_wasm {
            return String::new();
        }
        let mut out = String::new();
        out.push_str("  %dbg_reg_cnt = load i32, ptr @__arc_dbg_count\n");
        out.push_str(
            "  %dbg_reg = call i32 @rt_debug_module_register(ptr @__arc_dbg_table, ptr @__arc_dbg_table, i32 %dbg_reg_cnt)\n",
        );
        out
    }

    /// RFC 017 阶段一：渲染基元 typeinfo 槽回填 IR（`__arc_module_init`
    /// 体内、ret 前）。emit_typeinfos 将含基元槽的 RtFieldInfo/RtPropertyInfo
    /// 数组降级为可写 global（基元槽初值 null）并登记 GEP 地址；此处统一
    /// `call @rt_typeinfo_prim(id)` 取真实指针后 store。GEP 的基址为全局
    /// 符号、索引为常量，属常量表达式，可内联作 store 目标操作数。
    fn render_prim_fills(&self) -> String {
        let mut out = String::new();
        for (i, (gep, prim_id)) in self.pending_prim_fills.iter().enumerate() {
            out.push_str(&format!(
                "  %prim_fill_{i} = call ptr @rt_typeinfo_prim(i32 {prim_id})\n\
                 store ptr %prim_fill_{i}, ptr {gep}\n"
            ));
        }
        out
    }

    /// 发射 `@__arc_module_init` 占位定义（无静态字段时使用）。
    ///
    /// 当编译单元无静态字段时，`emit_sinit_and_module_init` 返回空字符串，
    /// 但 `emit_fn.rs` 仍会在 main 入口发射 `call void @__arc_module_init()`。
    /// 若不提供该符号定义，链接器会报 undefined symbol——故此处总是 emit
    /// 一个空函数体版本，保证符号始终定义。
    fn emit_empty_module_init(&self) -> String {
        // RFC 017 阶段一任务⑥：插件 dll 以 /EXPORT:__arc_module_init 导出该符号，
        // 使其成为链接器 GC root；linkonce_odr comdat 在插件 TU 无任何引用时会被
        // COFF 链接器整段丢弃（lld-link: undefined symbol 实测），故 DynamicLibrary
        // 角色发射 external 强定义；主程序保留 linkonce_odr（main 入口有引用 +
        // 跨 TU 去重）。
        let mut out = String::from(
            "; ---- RFC 006 M4: @__arc_module_init (no static fields, empty stub) ----\n",
        );
        if matches!(self.emit_role, crate::EmitRole::DynamicLibrary) {
            out.push_str("define void @__arc_module_init() {\n");
        } else {
            out.push_str("$__arc_module_init = comdat any\n");
            out.push_str("define linkonce_odr void @__arc_module_init() comdat {\n");
        }
        out.push_str("entry:\n");
        out.push_str(&self.render_host_dbg_registration());
        out.push_str(&self.render_prim_fills());
        out.push_str("  ret void\n}\n\n");
        out
    }

    /// 发射 `__sinit_<Class>` 函数与 `@__arc_module_init` 聚合调用。
    ///
    /// 输出形如：
    /// ```llvm
    /// define void @__sinit_Counter() {
    /// entry:
    ///   store i32 0, ptr @__static_Counter__count
    ///   ret void
    /// }
    ///
    /// define void @__arc_module_init() {
    /// entry:
    ///   call void @__sinit_Counter()
    ///   call void @__sinit_ModelCache()
    ///   ret void
    /// }
    /// ```
    ///
    /// 仅对**含初始化器**的静态字段发射 store 指令；无 init 的字段保持
    /// `zeroinitializer`（无需在 `__sinit` 内显式赋值）。
    ///
    /// `@__arc_module_init` 聚合所有 `__sinit_<Class>`（含无 init 的空函数），
    /// 保证未来扩展（如静态构造函数）有统一入口。
    ///
    /// `fns` 为全部 MIR 函数体：`@__arc_module_init` 的拓扑序基于
    /// [`super::static_init_deps::analyze_static_init_deps`]——穿透被调函数体收集
    /// `@__static_*` 引用（方案 B），并输出依赖环 / 跨包不可见诊断。
    ///
    /// 返回 `(IR 文本, 结构化诊断列表)`。诊断不在此处直接打印——由 arc CLI
    /// pipeline 统一渲染（对齐 `arc-cycle-001` warning 通道）。
    pub fn emit_sinit_and_module_init(
        &mut self,
        fns: &[(String, MirCfgBody)],
    ) -> (String, Vec<super::static_init_diag::StaticInitDiagnostic>) {
        if self.layouts.static_fields.is_empty() {
            // 无静态字段：仍发射 @__arc_module_init 空函数，保证 main 入口
            // 的 `call void @__arc_module_init()` 符号始终有定义。
            return (self.emit_empty_module_init(), Vec::new());
        }

        // 按类分组静态字段，每个类对应一个 `__sinit_<Class>` 函数。
        // 使用 IndexMap 保持声明顺序（首个出现的类优先），便于稳定 IR 输出。
        // RFC 006 V5：跳过外部宿主类的静态字段——其 `__sinit_<Class>` 由所属包
        // 定义（linkonce_odr 去重），本 TU 不重复发射。
        let mut by_class: IndexMap<Ident, Vec<&typeck::StaticFieldLayout>> = IndexMap::new();
        for sf in &self.layouts.static_fields {
            if self.external_class_names.contains(sf.class.as_str()) {
                continue;
            }
            by_class.entry(sf.class.clone()).or_default().push(sf);
        }

        // 第一遍：收集所有需要的 module 级常量
        let mut module_decls_by_class: IndexMap<Ident, String> = IndexMap::new();
        let mut fn_body_by_class: IndexMap<Ident, String> = IndexMap::new();
        // 表达式形态诊断（arc-sinit-003）：与拓扑序诊断（arc-sinit-001/002）在
        // 函数尾部合并入同一结构化通道返回。
        let mut expr_diags: Vec<super::static_init_diag::StaticInitDiagnostic> = Vec::new();

        for (class, fields) in &by_class {
            let mut fn_body = String::new();
            let mut mod_decls = String::new();
            let mut temp_counter = 0u32;
            fn_body.push_str("entry:\n");

            for sf in fields {
                // RFC 006 A3 S3：惰性字段不得在模块初始化时被急切构造。
                // 其初值由首次访问时的 `__lazy_init_<Class>` 写入。
                if sf.is_lazy {
                    continue;
                }
                let Some(init_expr) = &sf.init else {
                    continue;
                };
                let global = format!("@__static_{}_{}", sf.class, sf.field);
                let (val_ty, val) = self.emit_static_init_expr(
                    &mut fn_body,
                    &mut mod_decls,
                    &mut temp_counter,
                    init_expr,
                    &sf.ty,
                    &sf.class,
                    &sf.field,
                    &mut expr_diags,
                );
                fn_body.push_str(&format!("  store {} {}, ptr {}\n", val_ty, val, global));
            }
            fn_body.push_str("  ret void\n");

            fn_body_by_class.insert(class.clone(), fn_body);
            module_decls_by_class.insert(class.clone(), mod_decls);
        }

        let mut out = String::new();
        out.push_str("; ---- RFC 006 M4: __sinit_<Class> static initializers ----\n");

        for (class, _fields) in &by_class {
            let sinit_name = format!("__sinit_{}", class);
            let mod_decls = module_decls_by_class
                .get(class)
                .map(|s| s.as_str())
                .unwrap_or("");
            let fn_body = fn_body_by_class
                .get(class)
                .map(|s| s.as_str())
                .unwrap_or("entry:\n  ret void\n");

            out.push_str(mod_decls);
            out.push_str(&format!("${sinit_name} = comdat any\n"));
            out.push_str(&format!(
                "define linkonce_odr void @{sinit_name}() comdat {{\n"
            ));
            out.push_str(fn_body);
            out.push_str("}\n\n");
        }

        // @__arc_module_init：聚合调用所有 __sinit_<Class>。
        // main 入口（sync + async）在 entry 块开头 call void @__arc_module_init()。
        // RFC 006 V3：按静态字段依赖**拓扑序**调用——`Transform.Identity =
        // new Transform(Vector3.Zero, ...)` 引用 Vector3/Quaternion 的静态字段，
        // 须保证其 `__sinit_Vector3/__sinit_Quaternion` 先于 `__sinit_Transform`
        // 执行，否则被引用的全局仍是零值。
        out.push_str("; ---- RFC 006 M4: @__arc_module_init aggregator ----\n");
        // RFC 017 阶段一任务⑥：与 emit_empty_module_init 同因——插件 dll 的
        // /EXPORT: 使 __arc_module_init 成为链接器 GC root，linkonce_odr comdat
        // 无 TU 内引用会被 COFF 链接器丢弃，须发射 external 强定义。
        if matches!(self.emit_role, crate::EmitRole::DynamicLibrary) {
            out.push_str("define void @__arc_module_init() {\n");
        } else {
            out.push_str("$__arc_module_init = comdat any\n");
            out.push_str("define linkonce_odr void @__arc_module_init() comdat {\n");
        }
        out.push_str("entry:\n");
        out.push_str(&self.render_host_dbg_registration());
        let (init_order, mut diagnostics) = self.static_init_order(&by_class, fns);
        for class in init_order {
            out.push_str(&format!("  call void @__sinit_{}()\n", class));
        }
        out.push_str(&self.render_prim_fills());
        out.push_str("  ret void\n");
        out.push_str("}\n\n");

        // 合并表达式形态诊断（arc-sinit-003）到统一返回通道。
        diagnostics.append(&mut expr_diags);
        (out, diagnostics)
    }

    /// RFC 006 M4：`__sinit_<Class>` 的**拓扑执行序**。
    ///
    /// 委托 [`super::static_init_deps::analyze_static_init_deps`]（方案 B）：依赖分析
    /// **穿透被调函数体**——静态初始化器调用的函数（含间接调用）体内对
    /// `@__static_<Dep>_<field>` 的读写纳入依赖图，使 `DependencyPropertyRegistry` 等
    /// 宿主类的 `__sinit` 排在所有调用 `RegisterProperty` 等函数之前的正确位置。
    /// 返回 `(执行序, 结构化诊断列表)`（诊断由 arc CLI pipeline 统一渲染）。
    fn static_init_order(
        &self,
        by_class: &IndexMap<Ident, Vec<&typeck::StaticFieldLayout>>,
        fns: &[(String, MirCfgBody)],
    ) -> (
        Vec<Ident>,
        Vec<super::static_init_diag::StaticInitDiagnostic>,
    ) {
        let result = super::static_init_deps::analyze_static_init_deps(by_class, fns);
        let order = super::static_init_deps::topological_sort(&result.deps);
        (order, result.warnings)
    }

    /// RFC 006 A3 S3：为每个含惰性字段的类发射 `__lazy_init_<Class>` helper。
    ///
    /// 镜像 `__sinit_<Class>` 的 linkonce_odr + comdat 风格，但以类级惰性标志
    /// `@__lazy_<Class>` 加线程安全 guard：首触（`rt_lazy_init_begin` 赢得初始化权）
    /// 才执行惰性字段初始化器并 `rt_lazy_init_commit` 发布；二次访问直接走已完成
    /// 快速路径。结构：
    ///
    /// ```llvm
    /// define linkonce_odr void @__lazy_init_<Class>() comdat {
    /// entry:
    ///   %won = call i32 @rt_lazy_init_begin(ptr @__lazy_<Class>)
    ///   %c = icmp eq i32 %won, 1
    ///   br i1 %c, label %init, label %done
    /// init:
    ///   <对每个 is_lazy 字段：emit_static_init_expr 生成初始化器并 store>
    ///   call void @rt_lazy_init_commit(ptr @__lazy_<Class>)
    ///   br label %done
    /// done:
    ///   ret void
    /// }
    /// ```
    ///
    /// 每个惰性字段均有初始化器（typeck `is_lazy` 要求 init 为非编译期常量）。
    ///
    /// 返回 `(IR, 诊断列表)`——与 `emit_sinit_and_module_init` 共用
    /// `emit_static_init_expr`，其表达式形态诊断（arc-sinit-003）经本通道返回，
    /// 由 `emit_module` 合并入模块级诊断列表。
    pub fn emit_lazy_init_functions(
        &mut self,
    ) -> (String, Vec<super::static_init_diag::StaticInitDiagnostic>) {
        if self.layouts.static_fields.is_empty() {
            return (String::new(), Vec::new());
        }

        // 按类分组惰性字段，保持声明顺序（与 emit_static_field_globals 一致）。
        // RFC 006 V5：跳过外部宿主类（其 `__lazy_init_<Class>` 由所属包定义）。
        let mut by_class: IndexMap<Ident, Vec<&typeck::StaticFieldLayout>> = IndexMap::new();
        for sf in &self.layouts.static_fields {
            if sf.is_lazy && !self.external_class_names.contains(sf.class.as_str()) {
                by_class.entry(sf.class.clone()).or_default().push(sf);
            }
        }
        if by_class.is_empty() {
            return (String::new(), Vec::new());
        }

        let mut out = String::new();
        let mut diags: Vec<super::static_init_diag::StaticInitDiagnostic> = Vec::new();
        out.push_str("; ---- RFC 006 A3 S3: __lazy_init_<Class> lazy initializers ----\n");

        for (class, fields) in &by_class {
            let lazy_name = format!("__lazy_init_{class}");
            let mut mod_decls = String::new();
            let mut init_body = String::new();
            let mut temp_counter = 0u32;

            for sf in fields {
                let Some(init_expr) = &sf.init else {
                    continue;
                };
                let global = format!("@__static_{}_{}", sf.class, sf.field);
                let (val_ty, val) = self.emit_static_init_expr(
                    &mut init_body,
                    &mut mod_decls,
                    &mut temp_counter,
                    init_expr,
                    &sf.ty,
                    &sf.class,
                    &sf.field,
                    &mut diags,
                );
                init_body.push_str(&format!("  store {} {}, ptr {}\n", val_ty, val, global));
            }

            out.push_str(&mod_decls);
            out.push_str(&format!("${lazy_name} = comdat any\n"));
            out.push_str(&format!(
                "define linkonce_odr void @{lazy_name}() comdat {{\n"
            ));
            out.push_str("entry:\n");
            out.push_str(&format!(
                "  %won = call i32 @rt_lazy_init_begin(ptr @__lazy_{class})\n"
            ));
            out.push_str("  %c = icmp eq i32 %won, 1\n");
            out.push_str("  br i1 %c, label %init, label %done\n");
            out.push_str("init:\n");
            out.push_str(&init_body);
            out.push_str(&format!(
                "  call void @rt_lazy_init_commit(ptr @__lazy_{class})\n"
            ));
            out.push_str("  br label %done\n");
            out.push_str("done:\n");
            out.push_str("  ret void\n");
            out.push_str("}\n\n");
        }

        (out, diags)
    }

    /// RFC 017 §2.3：模块根元数据表（`--dynamic` 共享库 codegen 自动发射）。
    ///
    /// 发射 `@__arc_module_roots`（模块静态字段持有的 **class 引用槽位地址** 数组）
    /// 与 `@__arc_module_roots_count`。运行时在 `rt_library_load` 后自动发现该表，
    /// 将各槽位登记为模块根：`rt_library_root_scan` 沿槽位当前对象遍历可达闭包，
    /// 卸载前 `rt_lib_release_roots` 统一释放——宿主不再需要手动 `RegisterModuleRoot`。
    ///
    /// 仅 **class 类型** 静态字段参与（持 heap 对象的可遍历根）；string/object 等
    /// 可能持有 rodata 常量或非 class 对象，不列入根（对非 class 对象执行
    /// `rt_arc_walk_fields` / `rt_arc_dec` 不安全）。
    pub fn emit_module_roots_table(&self) -> String {
        if !matches!(self.emit_role, crate::EmitRole::DynamicLibrary) {
            return String::new();
        }
        let mut out = String::new();
        out.push_str("; ---- RFC 017 §2.3: Module root metadata table (codegen auto-emit) ----\n");
        let slots: Vec<&typeck::StaticFieldLayout> = self
            .layouts
            .static_fields
            .iter()
            .filter(|sf| self.layouts.classes.contains_key(sf.ty.as_str()))
            .collect();
        let count = slots.len();
        if count == 0 {
            out.push_str("@__arc_module_roots = constant [0 x ptr] []\n");
            out.push_str("@__arc_module_roots_count = constant i32 0\n\n");
            return out;
        }
        out.push_str(&format!(
            "@__arc_module_roots = constant [{count} x ptr] [\n"
        ));
        for (i, sf) in slots.iter().enumerate() {
            let comma = if i + 1 < count { "," } else { "" };
            out.push_str(&format!(
                "  ptr @__static_{}_{}{comma}\n",
                sf.class, sf.field
            ));
        }
        out.push_str("]\n");
        out.push_str(&format!(
            "@__arc_module_roots_count = constant i32 {count}\n\n"
        ));
        out
    }

    /// 解析静态字段的 LLVM 类型字符串。
    ///
    /// 复用 `llvm_type_of`——字段类型在 typeck 中以 `Ident`（类型名字符串）形式
    /// 存储，需要先转 `TypeId`。基元类型走快速路径，class/string 走 `ptr`。
    fn static_field_llvm_ty(&self, ty: &Ident) -> String {
        let type_id = type_id_from_name(ty.as_str());
        llvm_type_of(&type_id, self.layouts)
    }

    /// 静态字段 `zeroinitializer` 的字面量表示（用于全局变量初值）。
    ///
    /// `zeroinitializer` 是 LLVM 通用零值表示，对所有类型有效。
    /// 此处直接返回 `"zeroinitializer"` 字符串。
    fn static_field_zero_init(&self, _ty: &Ident) -> String {
        "zeroinitializer".to_string()
    }

    /// 发射静态字段初始化器表达式，返回 `(LLVM 类型, LLVM 值字符串)`。
    ///
    /// 检查 `method_name` 是否是 `class`（含基类链继承）的静态方法。
    /// 用于 `emit_static_init_expr` 中区分裸调用语义：
    /// 类静态方法须 `{class}_{method}` mangle，自由函数直接用 mangle_generic 结果。
    fn is_class_static_method(&self, class: &Ident, method_name: &Ident) -> bool {
        let mut current = Some(class.clone());
        while let Some(cn) = current {
            if let Some(cl) = self.layouts.classes.get(&cn) {
                if cl
                    .declared_methods
                    .iter()
                    .any(|m| m.name == *method_name && m.is_static)
                {
                    return true;
                }
                current = cl.parent.clone();
            } else {
                break;
            }
        }
        false
    }

    /// 发射静态字段初始化器表达式，返回 `(LLVM 类型, LLVM 值字符串)`。
    ///
    /// 当前支持的字面量：
    /// - `int` / `long` / `short` / `byte` / `char` → i32/i64 字面量
    /// - `float` / `double` → 浮点字面量
    /// - `bool` → i1 字面量
    /// - `string` → 全局字符串常量 GEP（通过 `intern_string` 复用 string pool）
    /// - `null` → `ptr null`
    ///
    /// 以及：一元 Neg/Not/BitNot 常量折叠、裸调用/静态方法调用（`Call` /
    /// `MethodCall`）、`typeof`、`new`（class/struct 构造与对象初始化器）、
    /// 静态字段引用、**枚举成员访问**（`Enum.Member` 经 `layouts.enum_variants`
    /// 折叠为 i32 常量，与 MIR `enum_variant_operand` 同形：浅层 Ident 接收者）、
    /// **variant case 构造**（`Content.None` / `Content.Text("...")` 经
    /// `emit_static_variant_construct`，与 MIR `emit_variant_construct` 同形：
    /// Field=无 payload、MethodCall=有 payload、Call+Field=兼容路径，三者
    /// 对齐 MIR `variant_construct_rvalue_with_prep` 的形态识别全集）。
    ///
    /// **完整性纪律（零值兜底必须显影）**：任何未覆盖形态在回退零值的同时
    /// 必须向 `diags` 推送 `arc-sinit-003` 诊断——静默零值曾致枚举默认值
    /// 被 0 顶替（Stretch=3 → Left=0），排查代价远高于一条编译期警告。
    #[allow(clippy::too_many_arguments)]
    fn emit_static_init_expr(
        &mut self,
        out: &mut String,
        mod_decls: &mut String,
        temp_counter: &mut u32,
        init: &ast::Spanned<Expr>,
        field_ty: &Ident,
        class: &Ident,
        field: &Ident,
        diags: &mut Vec<super::static_init_diag::StaticInitDiagnostic>,
    ) -> (String, String) {
        match &init.node {
            Expr::IntLit(n) => {
                let int_ty = match field_ty.as_str() {
                    "long" | "ulong" => "i64",
                    "short" | "ushort" | "byte" | "sbyte" | "char" => "i32",
                    _ => {
                        if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                            "i64"
                        } else {
                            "i32"
                        }
                    }
                };
                (int_ty.to_string(), n.to_string())
            }
            Expr::FloatLit(ast::FloatLitValue::Double(f)) => {
                ("double".to_string(), format!("{f:?}"))
            }
            Expr::FloatLit(ast::FloatLitValue::Float(f)) => ("float".to_string(), format!("{f:?}")),
            Expr::BoolLit(b) => (
                "i1".to_string(),
                if *b { "1".to_string() } else { "0".to_string() },
            ),
            Expr::StringLit(s) => {
                // 发射 module 级字符串常量 + GEP 获取指针
                // 使用 class_field 加递增计数器确保全局唯一（同一字段的
                // 多个子表达式如 nameof + default value 各需独立常量）
                let idx = *temp_counter;
                let global_name = format!("@.sinit_str_{class}_{field}_{idx}");
                let bytes = s.as_bytes();
                let len = bytes.len() + 1;
                let mut escaped = String::new();
                for b in bytes {
                    if *b == b'\\' {
                        escaped.push_str("\\\\");
                    } else if b.is_ascii_graphic() || *b == b' ' {
                        escaped.push(*b as char);
                    } else {
                        escaped.push_str(&format!("\\{:02X}", b));
                    }
                }
                mod_decls.push_str(&format!(
                    "{global_name} = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
                    len, escaped
                ));
                let tmp = format!("%.sinit.{n}", n = temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "  {tmp} = getelementptr inbounds [{} x i8], ptr {global_name}, i64 0, i64 0\n",
                    len
                ));
                ("ptr".to_string(), tmp)
            }
            Expr::Null => ("ptr".to_string(), "null".to_string()),
            Expr::CharLit(c) => ("i32".to_string(), (*c as u32 as i64).to_string()),
            // RFC 006 M3：静态字段默认值中的负字面量（`RegisterProperty<int>(..., -1)`
            // 的 `-1` 解析为 `Unary(Neg, IntLit(1))`）。此前无此分支 → 落入 `_ =>`
            // 的 `zeroinitializer`，把 DP 默认值 `-1` 静默清零。此处常量折叠 Neg/Not。
            Expr::Unary { op, expr: inner } => {
                let (ty, val) = self.emit_static_init_expr(
                    out,
                    mod_decls,
                    temp_counter,
                    inner,
                    field_ty,
                    class,
                    field,
                    diags,
                );
                match op {
                    UnaryOp::Neg => match ty.as_str() {
                        "i32" => {
                            let n: i32 = val.parse().unwrap_or(0);
                            (ty, (-n).to_string())
                        }
                        "i64" => {
                            let n: i64 = val.parse().unwrap_or(0);
                            (ty, (-n).to_string())
                        }
                        "double" => {
                            let f: f64 = val.parse().unwrap_or(0.0);
                            (ty, (-f).to_string())
                        }
                        _ => (ty, val),
                    },
                    UnaryOp::Not => {
                        if ty == "i1" {
                            (
                                "i1".to_string(),
                                if val == "1" {
                                    "0".to_string()
                                } else {
                                    "1".to_string()
                                },
                            )
                        } else {
                            (ty, val)
                        }
                    }
                    UnaryOp::BitNot => match ty.as_str() {
                        "i32" => {
                            let n: i32 = val.parse().unwrap_or(0);
                            (ty, (!n).to_string())
                        }
                        "i64" => {
                            let n: i64 = val.parse().unwrap_or(0);
                            (ty, (!n).to_string())
                        }
                        _ => (ty, val),
                    },
                }
            }
            Expr::Call {
                func,
                args,
                type_args,
                params_span: _,
            } => {
                // variant case 构造调用（`Content.Text("...")` / `Content.None()`）——
                // callee 形态 `Field(Ident(Variant), case)`。对齐 MIR
                // `emit_variant_construct`（alloca + tag + payload + 按引用返回）。
                // 先于裸调用/静态方法检查：variant 名不是类，is_class_static_method
                // 不会命中，但 callee 非 Ident 的兜底诊断会误报"非常量折叠的调用目标"。
                if let Expr::Field {
                    receiver,
                    field: case_name,
                } = &func.node
                {
                    if let Expr::Ident(vname) = &receiver.node {
                        if let Some(vlayout) = self.layouts.variants.get(vname) {
                            if let Some(case) = vlayout.cases.iter().find(|c| c.name == *case_name)
                            {
                                return self.emit_static_variant_construct(
                                    out,
                                    mod_decls,
                                    temp_counter,
                                    vname,
                                    case,
                                    args,
                                    class,
                                    field,
                                    diags,
                                );
                            }
                        }
                    }
                }
                if let Expr::Ident(func_name) = &func.node {
                    let type_ids: Vec<TypeId> = type_args
                        .iter()
                        .map(|t| {
                            if let Type::Named { path, .. } = &t.node {
                                type_id_from_name(path.last().map(|i| i.as_str()).unwrap_or("void"))
                            } else {
                                TypeId::Void
                            }
                        })
                        .collect();
                    let base = typeck::mangle_generic(func_name, &type_ids);
                    // 区分裸调用的两种语义：
                    //   1. 类的静态方法（含基类链继承）→ `{class}_{method}` mangle
                    //   2. 模块级自由函数（如 RegisterProperty<T>）→ 直接用 mangle_generic 结果
                    // 无条件添加类前缀会导致自由函数符号与定义点不匹配
                    // （如 `RegisterProperty_bool` 被错误 mangle 为 `Button_RegisterProperty_bool`）。
                    let is_class_static = self.is_class_static_method(class, func_name);
                    let mangled = if is_class_static {
                        mangle_method(class, &base)
                    } else {
                        base
                    };

                    // 递归处理参数
                    let mut arg_strs = Vec::new();
                    for arg in args {
                        let (arg_ty, arg_val) = self.emit_static_init_expr(
                            out,
                            mod_decls,
                            temp_counter,
                            arg,
                            &"int".into(), // dummy field_ty for recursive args
                            class,
                            field,
                            diags,
                        );
                        arg_strs.push(format!("{arg_ty} {arg_val}"));
                    }

                    let result = format!("%.sinit.{n}", n = temp_counter);
                    *temp_counter += 1;
                    // 返回类型取字段的 LLVM 类型（静态方法返回类型与字段声明类型一致）。
                    let ret_ty = self.static_field_llvm_ty(field_ty);
                    out.push_str(&format!(
                        "  {result} = call {ret_ty} @{mangled}({})\n",
                        arg_strs.join(", ")
                    ));

                    (ret_ty, result)
                } else {
                    // 不支持 call 目标（非 Ident 的 func）
                    diags.push(
                        super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                            class: class.clone(),
                            field: field.clone(),
                            kind: "非常量折叠的调用目标（非标识符 callee）",
                        },
                    );
                    let ty_str = self.static_field_llvm_ty(field_ty);
                    (ty_str, "zeroinitializer".to_string())
                }
            }
            // RFC 006 A3 S6a：静态初始化器中的**静态方法调用**（`Class.Method(...)`）。
            // `Box.D = Box.Build()` / `CultureData._dict = CultureData._build()` 在
            // AST 中解析为 `MethodCall{ receiver: Ident(Class), method }`（非 `Call`），
            // 此前无分支 → 落入 `_ => zeroinitializer` → 惰性字段恒为 null → 运行期
            // 空引用崩溃。按 `{Class}_{method}`（泛型经 mangle_generic）mangle，对齐
            // emit_call 静态方法路径。返回类型取字段 LLVM 类型（ptr）。
            Expr::MethodCall {
                receiver,
                method,
                args,
                type_args,
                ..
            } => {
                // variant case 构造调用（`Content.Text("...")` / `Content.None()`）——
                // Parser 将 `Type.Case(payload)` 解析为 MethodCall（与 typeck
                // `coerce_to_variant` 重写产物、MIR `variant_construct_rvalue_with_prep`
                // 主路径同形）。对齐 MIR `emit_variant_construct`（alloca + tag +
                // payload + 按引用返回）。
                // **必须先于静态方法 mangle 检查**：否则 `Content.Text("x")` 会被
                // mangle 为 `@Content_Text` 错误调用——variant 名不是类，
                // `is_class_static_method` 查 classes 布局链永远不命中，
                // 直接掉进无条件 `{class}_{method}` mangle。
                if let Expr::Ident(vname) = &receiver.node {
                    if let Some(vlayout) = self.layouts.variants.get(vname) {
                        if let Some(case) = vlayout.cases.iter().find(|c| c.name == *method) {
                            return self.emit_static_variant_construct(
                                out,
                                mod_decls,
                                temp_counter,
                                vname,
                                case,
                                args,
                                class,
                                field,
                                diags,
                            );
                        }
                    }
                }
                if let Expr::Ident(class_name) = &receiver.node {
                    let base = if type_args.is_empty() {
                        method.to_string()
                    } else {
                        let type_ids: Vec<TypeId> = type_args
                            .iter()
                            .map(|t| {
                                if let Type::Named { path, .. } = &t.node {
                                    type_id_from_name(
                                        path.last().map(|i| i.as_str()).unwrap_or("void"),
                                    )
                                } else {
                                    TypeId::Void
                                }
                            })
                            .collect();
                        typeck::mangle_generic(method.as_str(), &type_ids)
                    };
                    let mangled = mangle_method(class_name, &base);

                    let mut arg_strs = Vec::new();
                    for arg in args {
                        let (arg_ty, arg_val) = self.emit_static_init_expr(
                            out,
                            mod_decls,
                            temp_counter,
                            arg,
                            &"int".into(),
                            class,
                            field,
                            diags,
                        );
                        arg_strs.push(format!("{arg_ty} {arg_val}"));
                    }

                    let result = format!("%.sinit.{n}", n = temp_counter);
                    *temp_counter += 1;
                    // 返回类型以被调方法的真实签名（fn_returns 权威表）为准，
                    // 而非外层静态字段类型：此前用 static_field_llvm_ty(field_ty)
                    // 且 ctor 实参递归把 field_ty 硬编码为 "int"，使嵌套静态方法调用
                    //（如 `new SolidColorBrush(Color.Transparent())`）返回 struct 指针
                    // 被 emit 成 `call i32` —— 64 位指针截断 → 对象字段垃圾 → 0xC0000005
                    // （Color 场景实测；对齐 emit_call 的 fn_returns 权威路径）。
                    let fn_key = format!("{class_name}::{base}");
                    let ret_ty = self
                        .fn_returns
                        .get(&fn_key)
                        .map(|t| llvm_type_of(t, self.layouts))
                        .unwrap_or_else(|| self.static_field_llvm_ty(field_ty));
                    out.push_str(&format!(
                        "  {result} = call {ret_ty} @{mangled}({})\n",
                        arg_strs.join(", ")
                    ));
                    (ret_ty, result)
                } else {
                    // 实例方法调用或非常量接收者——静态初始化器无法折叠。
                    diags.push(
                        super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                            class: class.clone(),
                            field: field.clone(),
                            kind: "实例方法调用或非常量接收者的方法调用",
                        },
                    );
                    let ty_str = self.static_field_llvm_ty(field_ty);
                    (ty_str, "zeroinitializer".to_string())
                }
            }
            Expr::TypeOf(ty) => {
                if let Type::Named { path, .. } = &ty.node {
                    let type_name = path.last().map(|i| i.as_str()).unwrap_or("");
                    // typeinfo 为所有 class + interface 发射（emit_typeinfos
                    // 2026-07-31 起不限 has_vtable）；静态 typeof 字段初始化同规则。
                    let has_typeinfo = self.layouts.classes.contains_key(type_name)
                        || self.layouts.interfaces.contains_key(type_name)
                        // RFC 017 阶段一：基元 typeinfo 经 rt_typeinfo_prim 函数
                        // 符号静态查询，与类/接口一样具备可包装的 RuntimeType。
                        || primitive_typeinfo_id(type_name).is_some();
                    if let Some(rt_layout) = self.layouts.classes.get("RuntimeType") {
                        if has_typeinfo {
                            let handle_offset = rt_layout
                                .fields
                                .iter()
                                .find(|f| f.name.as_str() == "_typeInfoHandle")
                                .map(|f| f.offset)
                                .unwrap_or(0);
                            let size = rt_layout.size_bytes() as u64;

                            let tmp = format!("%.sinit.{n}", n = temp_counter);
                            *temp_counter += 1;
                            out.push_str(&format!(
                                "  {tmp} = call ptr @calloc(i64 1, i64 {size})\n"
                            ));
                            // refcount = 1
                            out.push_str(&format!("  store i32 1, ptr {tmp}\n"));
                            // vtable
                            if rt_layout.has_vtable {
                                let vt_addr = format!("%.sinit.{n}", n = temp_counter);
                                *temp_counter += 1;
                                out.push_str(&format!(
                                    "  {vt_addr} = getelementptr inbounds i8, ptr {tmp}, i64 8\n"
                                ));
                                // RFC 038 M2：RuntimeType 为 stdlib 外部类
                                //（LibraryObject），vtable 经守卫登记 external 声明。
                                if let Some(rt_vt_sym) = self.vtable_global_reg("RuntimeType") {
                                    out.push_str(&format!(
                                        "  store ptr {rt_vt_sym}, ptr {vt_addr}\n"
                                    ));
                                }
                            }
                            // _typeInfoHandle
                            let handle_addr = format!("%.sinit.{n}", n = temp_counter);
                            *temp_counter += 1;
                            out.push_str(&format!(
                                "  {handle_addr} = getelementptr inbounds i8, ptr {tmp}, i64 {handle_offset}\n"
                            ));
                            // RFC 017 阶段一：基元经 rt_typeinfo_prim 函数符号查询
                            // （函数体内指令语境可 call），ptrtoint 后写入
                            // _typeInfoHandle；类/接口仍走常量表达式路径（外部类型
                            // typeinfo 经守卫登记 external 声明，RFC 038 M2）。
                            if let Some(prim_id) = primitive_typeinfo_id(type_name) {
                                let ti = format!("%.sinit.{n}", n = temp_counter);
                                *temp_counter += 1;
                                out.push_str(&format!(
                                    "  {ti} = call ptr @rt_typeinfo_prim(i32 {prim_id})\n"
                                ));
                                let handle_val = format!("%.sinit.{n}", n = temp_counter);
                                *temp_counter += 1;
                                out.push_str(&format!(
                                    "  {handle_val} = ptrtoint ptr {ti} to i64\n"
                                ));
                                out.push_str(&format!(
                                    "  store i64 {handle_val}, ptr {handle_addr}\n"
                                ));
                            } else {
                                let typeinfo_sym = self.typeinfo_global_for(type_name);
                                out.push_str(&format!(
                                    "  store i64 ptrtoint (ptr {typeinfo_sym} to i64), ptr {handle_addr}\n"
                                ));
                            }

                            return ("ptr".to_string(), tmp);
                        }
                    }
                }
                ("ptr".to_string(), "null".to_string())
            }
            // RFC 006 A3 S3b：`new T(args)`（class 构造）——静态初始化器中支持
            // 引用类型对象构造（对齐 emit_new 通用 calloc + 祖先 ctor + 目标 ctor
            // 序列），使 `static readonly X = new T(...)` 的最简洁惰性/急切形式成立。
            Expr::New { ty, args, obj_init } => self.emit_static_new_expr(
                out,
                mod_decls,
                temp_counter,
                ty,
                args,
                obj_init,
                field_ty,
                class,
                field,
                diags,
            ),
            // RFC 006 V3：静态字段引用（`Vector3.Zero` / `Quaternion.Identity`）——
            // 出现在其他 struct 静态字段初始化器（如 `Transform.Identity =
            // new Transform(Vector3.Zero, ...)`）。receiver 为类/struct 名 Ident，
            // 直接 load 对应 `@__static_<Class>_<field>` 全局，返回其 LLVM 类型与
            // 加载结果。依赖的 `__sinit_<Class>` 由 `@__arc_module_init` 拓扑序先执行。
            Expr::Field {
                receiver,
                field: member,
            } => {
                if let Expr::Ident(receiver_name) = &receiver.node {
                    if self
                        .layouts
                        .static_fields
                        .iter()
                        .any(|s| s.class == *receiver_name && s.field == *member)
                    {
                        let global = format!("@__static_{receiver_name}_{member}");
                        // 字段 LLVM 类型：查询 static_fields 布局。
                        let sf = self
                            .layouts
                            .static_fields
                            .iter()
                            .find(|s| s.class == *receiver_name && s.field == *member);
                        let ty_str = sf
                            .map(|s| self.static_field_llvm_ty(&s.ty))
                            .unwrap_or_else(|| "ptr".to_string());
                        let tmp = format!("%.sinit.{n}", n = temp_counter);
                        *temp_counter += 1;
                        out.push_str(&format!("  {tmp} = load {ty_str}, ptr {global}\n"));
                        return (ty_str, tmp);
                    }
                    // 枚举成员访问（`HorizontalAlignment.Stretch`）——sinit 直 emit
                    // 路径不经 MIR，无法复用 `enum_variant_operand` 折叠；须按
                    // `layouts.enum_variants` 判别值表折叠为 i32 常量（枚举 LLVM
                    // 类型恒为 i32，对齐 `llvm_type_of`）。此前无此分支 → 落入下方
                    // 零值兜底 → 枚举 0 值静默顶替真实判别值（Left=0 顶替 Stretch=3，
                    // ArmlDemo 默认对齐 DP 读回错误 → 布局宽度塌陷）。
                    if let Some(members) = self.layouts.enum_variants.get(receiver_name) {
                        if let Some((_, discriminant)) =
                            members.iter().find(|(name, _)| name == member)
                        {
                            return ("i32".to_string(), discriminant.to_string());
                        }
                    }
                    // variant 无 payload case 构造（`Content.None`）——对齐 MIR
                    // `emit_variant_construct` 无 payload 路径。payload case
                    // （`Content.Text("...")`）是 Call 形态，由 Call 分支的
                    // variant 构造路径处理。
                    if let Some(vlayout) = self.layouts.variants.get(receiver_name) {
                        if let Some(case) = vlayout.cases.iter().find(|c| c.name == *member) {
                            if case.payload.is_none() {
                                return self.emit_static_variant_construct(
                                    out,
                                    mod_decls,
                                    temp_counter,
                                    receiver_name,
                                    case,
                                    &[],
                                    class,
                                    field,
                                    diags,
                                );
                            }
                            // payload case 裸引用（`Content.Text` 无实参括号）——
                            // 不是值表达式，无法折叠；须以构造调用形式提供
                            // payload。显影诊断后零值兜底。
                            diags.push(super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                                class: class.clone(),
                                field: field.clone(),
                                kind: "variant payload case 裸引用（须以 Content.Text(...) 构造调用形式提供 payload）",
                            });
                            // 已诊断，直接返回零值（跳过下方重复的未命中诊断）。
                            let ty_str = self.static_field_llvm_ty(field_ty);
                            return (ty_str, "zeroinitializer".to_string());
                        }
                    }
                    // Ident 接收者但静态字段表与枚举表均未命中——可能是 const 字段
                    // 引用（const 不进 static_fields，typeck 在 MIR 路径折叠，本
                    // 路径无对应物）或未知符号。零值兜底 + 显影诊断。
                    diags.push(super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                        class: class.clone(),
                        field: field.clone(),
                        kind: "类型成员引用未命中静态字段表/枚举表/variant 表（const 引用或未知符号）",
                    });
                } else {
                    // 限定名路径（`Ns.Enum.Member` 等嵌套 Field 接收者）——与 MIR
                    // `enum_variant_operand` 的简易 Ident 限制同级，尚未支持。
                    diags.push(
                        super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                            class: class.clone(),
                            field: field.clone(),
                            kind: "限定名路径成员访问（嵌套接收者）",
                        },
                    );
                }
                // 非静态字段引用（实例字段/未知）→ 零值兜底。
                let ty_str = self.static_field_llvm_ty(field_ty);
                (ty_str, "zeroinitializer".to_string())
            }
            _ => {
                // 未覆盖的表达式形态：零值兜底 + 显影诊断（完整性纪律，见函数文档）。
                diags.push(
                    super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                        class: class.clone(),
                        field: field.clone(),
                        kind: static_init_expr_kind(&init.node),
                    },
                );
                let ty_str = self.static_field_llvm_ty(field_ty);
                (ty_str, "zeroinitializer".to_string())
            }
        }
    }
    /// 静态初始化器中的 variant case 构造（`Content.None` / `Content.Text("...")`）。
    ///
    /// 对齐 MIR `emit_variant_construct`：alloca `%variant.{Name}` → 零初始化 →
    /// store tag（GEP field 0）→ 若有 payload 则 store body（GEP field 2，class
    /// payload 额外 `rt_arc_inc`）。variant 按引用传递，返回 `(ptr, alloca_slot)`
    /// ——与函数签名中 variant 参数恒为 `ptr`（`llvm_type_of` RFC 004 M1）一致。
    ///
    /// **与 MIR 路径的同形性是正确性前提**：payload 基元类型映射（`int` →
    /// `TypeId::Int` 等）、`rt_arc_inc` 豁免规则（opaque runtime handle /
    /// string rodata 不 inc——inc 会写坏裸串首字节）须与 MIR 路径逐条对齐，
    /// 否则 sinit 构造的 variant 与运行期提取的 payload 类型错位。
    #[allow(clippy::too_many_arguments)]
    fn emit_static_variant_construct(
        &mut self,
        out: &mut String,
        mod_decls: &mut String,
        temp_counter: &mut u32,
        variant_name: &Ident,
        case: &typeck::EnumVariantInfo,
        args: &[ast::Spanned<Expr>],
        class: &Ident,
        field: &Ident,
        diags: &mut Vec<super::static_init_diag::StaticInitDiagnostic>,
    ) -> (String, String) {
        // arity 校验：payload case 恰 1 实参；无 payload case 0 实参。
        let arity_ok = match &case.payload {
            Some(_) => args.len() == 1,
            None => args.is_empty(),
        };
        if !arity_ok {
            diags.push(super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                class: class.clone(),
                field: field.clone(),
                kind: "variant case 构造实参数不匹配（payload case 恰 1 实参，无 payload case 0 实参）",
            });
            return ("ptr".to_string(), "null".to_string());
        }

        let variant_ty = format!("%variant.{variant_name}");
        let slot = format!("%.sinit.{n}", n = temp_counter);
        *temp_counter += 1;
        out.push_str(&format!("  {slot} = alloca {variant_ty}\n"));
        // 零初始化（padding + body 全零，避免未定义读取）。
        out.push_str(&format!(
            "  store {variant_ty} zeroinitializer, ptr {slot}\n"
        ));

        // tag：GEP field 0 → store case discriminant。
        let tag_ptr = format!("%.sinit.{n}", n = temp_counter);
        *temp_counter += 1;
        out.push_str(&format!(
            "  {tag_ptr} = getelementptr inbounds {variant_ty}, ptr {slot}, i32 0, i32 0\n"
        ));
        out.push_str(&format!(
            "  store i8 {}, ptr {tag_ptr}\n",
            case.discriminant as i32
        ));

        // payload：GEP field 2 → store payload 值。
        if let (Some(payload_ident), Some(arg)) = (&case.payload, args.first()) {
            // 基元类型 payload（double/int/bool 等）须映射为对应 TypeId 变体，
            // 不能统一包装为 TypeId::Named——否则 named_type 回退为 ptr，
            // 导致 payload 存储/提取类型不匹配（与 MIR 路径同源约束）。
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
            let (val_ty, val) = self.emit_static_init_expr(
                out,
                mod_decls,
                temp_counter,
                arg,
                payload_ident,
                class,
                field,
                diags,
            );

            let body_ptr = format!("%.sinit.{n}", n = temp_counter);
            *temp_counter += 1;
            out.push_str(&format!(
                "  {body_ptr} = getelementptr inbounds {variant_ty}, ptr {slot}, i32 0, i32 2\n"
            ));

            // RFC 004 M1 §12：仅 class payload 做 rt_arc_inc。string 常为
            // rodata / 裸 char*（无 ArcHeader）；inc 会写坏串首字节 → 堆损坏。
            let needs_arc = self.layouts.classes.contains_key(payload_ident)
                && !is_opaque_runtime_handle(payload_ident);
            if needs_arc {
                out.push_str(&format!("  call void @rt_arc_inc(ptr {val})\n"));
            }
            // val_ty 与 payload_ty_str 在基元场景应一致；命名类型均为 ptr。
            let store_ty = if val_ty == "ptr" || val_ty.is_empty() {
                payload_ty_str.clone()
            } else {
                val_ty.clone()
            };
            out.push_str(&format!("  store {store_ty} {val}, ptr {body_ptr}\n"));
        }

        ("ptr".to_string(), slot)
    }

    /// RFC 006 A3 S3b：静态初始化器中的 `new T(args)`（class 构造）。
    ///
    /// 复用通用构造序列（对齐 `emit_call::emit_new` 通用路径）：
    /// `calloc(size)` → refcount=1 → vtable（若 has_vtable）→ 祖先 ctor
    /// （基类优先，排除自身）→ 目标 ctor（无参 `__ctor::T` / 有参
    /// `__ctor::T_<arity>`）→ 返回分配指针。支持 `new T() { X = v }`
    /// 对象初始化器（按类布局字段偏移 GEP+store）。
    ///
    /// 仅支持 **class（引用类型）** 构造；struct/基元等值类型不支持（值类型
    /// 常量急切路径另议，S3b 边界）。返回 `(ptr, tmp)`。
    #[allow(clippy::too_many_arguments)]
    fn emit_static_new_expr(
        &mut self,
        out: &mut String,
        mod_decls: &mut String,
        temp_counter: &mut u32,
        ty: &ast::Spanned<Type>,
        args: &[ast::Spanned<Expr>],
        obj_init: &Option<Vec<(Ident, ast::Spanned<Expr>)>>,
        field_ty: &Ident,
        class: &Ident,
        field: &Ident,
        diags: &mut Vec<super::static_init_diag::StaticInitDiagnostic>,
    ) -> (String, String) {
        // 类名：`new T(...)` 的显式类型；target-typed 形式（Type::Infer）用字段类型。
        // 泛型 `new Dictionary<A,B>()`：`path.last()` 仅为 "Dictionary"，但布局/ctor/
        // vtable 键是单态化名（如 `Dictionary_string_CultureInfo`），须用
        // `mangle_generic` 还原，否则 `classes.contains_key("Dictionary")` 为假 →
        // 静默零值 → 运行期空引用崩溃（RFC 006 A3 S6a）。
        let type_name: Ident = match &ty.node {
            Type::Named { path, generics } => {
                let def = path
                    .last()
                    .map(|i| i.as_str().to_string())
                    .unwrap_or_else(|| field_ty.as_str().to_string());
                if generics.is_empty() {
                    def.into()
                } else {
                    let args: Vec<TypeId> = generics
                        .iter()
                        .map(|g| match &g.node {
                            Type::Named { path: gp, .. } => {
                                type_id_from_name(gp.last().map(|i| i.as_str()).unwrap_or("void"))
                            }
                            _ => TypeId::Void,
                        })
                        .collect();
                    typeck::mangle_generic(&def, &args).into()
                }
            }
            _ => field_ty.clone(),
        };
        // 运行时门面 new（RFC 008/009）：`new T()` 须拦截为 `rt_*_create()` ABI。
        // 这些类型"对象即裸句柄"（types::is_opaque_runtime_handle 豁免 ARC inc/dec）。
        // 成员判定与装配集中收敛到唯一事实来源 types::runtime_facade_new_spec，
        // 与普通路径 emit_call 走**同一逻辑**——禁止在此加内联活动清单（曾因漏
        // `Lock` 静态字段致全量回归崩溃）。
        let tname: &str = type_name.as_str();
        if crate::llvm_ir::types::is_runtime_facade_new(tname) {
            // Thread/Socket 族为过程式门面（闭包提取/绑定逻辑），静态初始化器里
            // 声明此类静态字段属不支持形态 → 显式诊断，禁止静默降级或 panic。
            let call = match crate::llvm_ir::types::runtime_facade_new_spec(tname) {
                Some((target, 0)) => target.to_string(),
                Some((target, arity)) => {
                    debug_assert_eq!(arity, 2, "facade args count drift");
                    // i32×2：Semaphore(init, max=1) / ThreadPoolScheduler(workers, numa)。
                    let a0 = match args.first() {
                        Some(expr) => {
                            let (a_ty, a_val) = self.emit_static_init_expr(
                                out,
                                mod_decls,
                                temp_counter,
                                expr,
                                &"int".into(),
                                class,
                                field,
                                diags,
                            );
                            format!("{a_ty} {a_val}")
                        }
                        None => "i32 0".to_string(),
                    };
                    let a1 = match args.get(1) {
                        Some(expr) => {
                            let (a_ty, a_val) = self.emit_static_init_expr(
                                out,
                                mod_decls,
                                temp_counter,
                                expr,
                                &"int".into(),
                                class,
                                field,
                                diags,
                            );
                            format!("{a_ty} {a_val}")
                        }
                        None => "i32 1".to_string(),
                    };
                    format!("{target}({a0}, {a1})")
                }
                None => {
                    diags.push(super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                        class: class.clone(),
                        field: field.clone(),
                        kind: "运行时门面 Thread/Socket 族在静态初始化器中不支持（需过程式闭包/绑定）",
                    });
                    let ty_str = self.static_field_llvm_ty(&type_name);
                    return (ty_str, "zeroinitializer".to_string());
                }
            };
            let tmp = format!("%.sinit.{n}", n = temp_counter);
            *temp_counter += 1;
            out.push_str(&format!("  {tmp} = call ptr {call}\n"));
            return ("ptr".to_string(), tmp);
        }

        // RFC 006 V3：struct（值类型）静态字段真实构造。struct 无 ARC 头/vtable/
        // 继承链，走 calloc + `__ctor::<Struct>`（有参 `__ctor::<Struct>_<arity>`）
        // 序列（对齐 emit_new 的 struct 分支），返回 `(ptr, tmp)`——struct 按引用
        // 存储（ptr to calloc'd struct）。此前返回 `zeroinitializer` 使静态字段恒为
        // 空指针 → 访问崩溃。
        if self.layouts.structs.contains_key(&type_name) {
            let size = self.layouts.size_of_ty(&type_name) as i64;
            let tmp = format!("%.sinit.{n}", n = temp_counter);
            *temp_counter += 1;
            out.push_str(&format!("  {tmp} = call ptr @calloc(i64 1, i64 {size})\n"));
            // ctor 重载 mangle：无参用 `__ctor::Struct`；有参用 `__ctor::Struct_<arity>`。
            let ctor_name = if args.is_empty() {
                mangle_fn_name(&format!("__ctor::{type_name}"))
            } else {
                mangle_fn_name(&format!("__ctor::{type_name}_{}", args.len()))
            };
            let mut arg_strs = Vec::new();
            for arg in args {
                let (arg_ty, arg_val) = self.emit_static_init_expr(
                    out,
                    mod_decls,
                    temp_counter,
                    arg,
                    &"int".into(),
                    class,
                    field,
                    diags,
                );
                arg_strs.push(format!("{arg_ty} {arg_val}"));
            }
            let call_args = if arg_strs.is_empty() {
                format!("ptr {tmp}")
            } else {
                format!("ptr {tmp}, {}", arg_strs.join(", "))
            };
            out.push_str(&format!("  call void @{ctor_name}({call_args})\n"));
            // 对象初始化器 `new T() { X = v }`：按 struct 布局字段偏移 GEP + store。
            if let Some(initializers) = obj_init {
                let sl = &self.layouts.structs[&type_name];
                for (name, expr) in initializers {
                    let offset = sl
                        .fields
                        .iter()
                        .find(|f| f.name == *name)
                        .map(|f| f.offset)
                        .unwrap_or(0);
                    let (val_ty, val) = self.emit_static_init_expr(
                        out,
                        mod_decls,
                        temp_counter,
                        expr,
                        &"int".into(),
                        class,
                        field,
                        diags,
                    );
                    let addr = format!("%.sinit.{n}", n = temp_counter);
                    *temp_counter += 1;
                    out.push_str(&format!(
                        "  {addr} = getelementptr inbounds i8, ptr {tmp}, i64 {offset}\n"
                    ));
                    out.push_str(&format!("  store {val_ty} {val}, ptr {addr}\n"));
                }
            }
            return ("ptr".to_string(), tmp);
        }
        // 仅支持 class 构造；非 class（struct 已在上方专属分支处理；基元/Vector
        // 等值类型或未知类型）→ 零值 + 显影诊断（完整性纪律：禁止静默降级）。
        if !self.layouts.classes.contains_key(&type_name) {
            diags.push(
                super::static_init_diag::StaticInitDiagnostic::UnsupportedInitExpr {
                    class: class.clone(),
                    field: field.clone(),
                    kind: "new 表达式的构造类型非 class/struct（基元、外部未知类型或泛型未单态化）",
                },
            );
            let ty_str = self.static_field_llvm_ty(&type_name);
            return (ty_str, "zeroinitializer".to_string());
        }

        let cls = &self.layouts.classes[&type_name];
        let size = cls.size_bytes() as i64;
        let tmp = format!("%.sinit.{n}", n = temp_counter);
        *temp_counter += 1;
        out.push_str(&format!("  {tmp} = call ptr @calloc(i64 1, i64 {size})\n"));
        out.push_str(&format!("  store i32 1, ptr {tmp}\n"));
        // 与其它静态初始化路径一致：外部类（external_class_names，任意角色）
        // vtable 经 `vtable_global_reg` 登记 external 声明（定义包 linkonce_odr
        // 定义解析），避免直接引用未定义符号导致 clang IR 编译失败。
        if let Some(vt) = self.vtable_global_reg(&type_name) {
            let vt_addr = format!("%.sinit.{n}", n = temp_counter);
            *temp_counter += 1;
            out.push_str(&format!(
                "  {vt_addr} = getelementptr inbounds i8, ptr {tmp}, i64 8\n"
            ));
            out.push_str(&format!("  store ptr {vt}, ptr {vt_addr}\n"));
        }
        // 祖先 ctor（基类优先，排除自身）——对齐 emit_new 通用路径。
        for ancestor in self
            .class_ancestors_base_first_ll(&type_name)
            .into_iter()
            .filter(|a| a != type_name)
        {
            let base_ctor = mangle_fn_name(&format!("__ctor::{ancestor}"));
            out.push_str(&format!("  call void @{base_ctor}(ptr {tmp})\n"));
        }
        // 目标 ctor（无参 `__ctor::T` / 有参 `__ctor::T_<arity>`）。
        let arity = args.len();
        let ctor_name = if arity == 0 {
            mangle_fn_name(&format!("__ctor::{type_name}"))
        } else {
            mangle_fn_name(&format!("__ctor::{type_name}_{arity}"))
        };
        // List stub 类（`List_<elem>`）的 ctor stub 由 **MIR 函数体** 中的
        // `new List<T>()` 触发发射（`emit_function` → `try_emit_stub`）。静态
        // 字段初始化器不产生 MIR 条目——若本模块没有函数体实例化同一
        // `List_<elem>`（如 `List<Action>` 仅用于 `_cbCallback` 静态字段），
        // `call __ctor::List_<elem>` 会引用未定义符号。此处内联与
        // `emit_stubs::list_stub` ctor 分支**等价**的序列：`rt_list_create` +
        // store handle@16（对象已 calloc + 写 refcount/vtable）。若 MIR 侧
        // 已有同名 stub，linkonce_odr 照常发射，两路互不冲突。
        if arity == 0 && obj_init.is_none() {
            if let Some(elem_suf) = crate::llvm_ir::types::parse_list_elem(&type_name) {
                let elem_size = crate::llvm_ir::types::list_elem_size(elem_suf, self.layouts);
                let eq_fn = crate::llvm_ir::types::list_eq_fn(elem_suf)
                    .map(|f| format!("ptr {f}"))
                    .unwrap_or_else(|| "ptr null".to_string());
                let arc_inc = crate::llvm_ir::types::list_arc_inc_fn(elem_suf, self.layouts)
                    .map(|f| format!("ptr {f}"))
                    .unwrap_or_else(|| "ptr null".to_string());
                let arc_dec = crate::llvm_ir::types::list_arc_dec_fn(elem_suf, self.layouts)
                    .map(|f| format!("ptr {f}"))
                    .unwrap_or_else(|| "ptr null".to_string());
                let h = format!("%.sinit.{n}", n = temp_counter);
                *temp_counter += 1;
                let hp = format!("%.sinit.{n}", n = temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "  {h} = call ptr @rt_list_create(i32 {elem_size}, {eq_fn}, {arc_inc}, {arc_dec})\n"
                ));
                out.push_str(&format!(
                    "  {hp} = getelementptr inbounds i8, ptr {tmp}, i64 16\n"
                ));
                out.push_str(&format!("  store ptr {h}, ptr {hp}\n"));
                return ("ptr".to_string(), tmp);
            }
        }
        let mut arg_strs = Vec::new();
        for arg in args {
            let (arg_ty, arg_val) = self.emit_static_init_expr(
                out,
                mod_decls,
                temp_counter,
                arg,
                &"int".into(),
                class,
                field,
                diags,
            );
            arg_strs.push(format!("{arg_ty} {arg_val}"));
        }
        let call_args = if arg_strs.is_empty() {
            format!("ptr {tmp}")
        } else {
            format!("ptr {tmp}, {}", arg_strs.join(", "))
        };
        out.push_str(&format!("  call void @{ctor_name}({call_args})\n"));
        // 对象初始化器 `new T() { X = v }`：按类布局字段偏移 GEP + store。
        if let Some(initializers) = obj_init {
            for (name, expr) in initializers {
                let offset = cls
                    .fields
                    .iter()
                    .find(|f| f.name == *name)
                    .map(|f| f.offset)
                    .unwrap_or(0);
                let (val_ty, val) = self.emit_static_init_expr(
                    out,
                    mod_decls,
                    temp_counter,
                    expr,
                    &"int".into(),
                    class,
                    field,
                    diags,
                );
                let addr = format!("%.sinit.{n}", n = temp_counter);
                *temp_counter += 1;
                out.push_str(&format!(
                    "  {addr} = getelementptr inbounds i8, ptr {tmp}, i64 {offset}\n"
                ));
                out.push_str(&format!("  store {val_ty} {val}, ptr {addr}\n"));
            }
        }
        ("ptr".to_string(), tmp)
    }

    /// 类继承链（根 → 直接基类，**不含自身**）。供静态初始化器 `new` 的祖先
    /// ctor 调用序列使用（对齐 FnEmitter::class_ancestors_base_first）。
    fn class_ancestors_base_first_ll(&self, class: &str) -> Vec<String> {
        let mut chain: Vec<String> = Vec::new();
        let mut cur = self
            .layouts
            .classes
            .get(class)
            .and_then(|c| c.parent.clone());
        while let Some(p) = cur {
            chain.push(p.as_str().to_string());
            cur = self.layouts.classes.get(&p).and_then(|c| c.parent.clone());
        }
        chain.reverse(); // 根 → 叶
        chain
    }
}

/// `arc-sinit-003` 诊断的形态描述：`emit_static_init_expr` 未覆盖的表达式类别。
///
/// 仅在 `_ =>` 兜底分支触发（字面量/一元/调用/typeof/new/字段引用均已有专属分支），
/// 用于向用户指明初始化器中哪类表达式被回退为零值。
fn static_init_expr_kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::Binary { .. } => "二元运算",
        Expr::Index { .. } => "索引访问",
        Expr::Lambda(_) => "lambda 表达式",
        Expr::InterpolatedString { .. } => "插值字符串",
        Expr::Cast { .. } => "类型转换",
        Expr::Await(_) => "await 表达式",
        Expr::If { .. } => "if 表达式",
        Expr::Switch(_) | Expr::SwitchForm(_) => "switch 表达式",
        Expr::CollectionExpr { .. } => "集合表达式",
        Expr::Block(_) => "块表达式",
        Expr::Path(_) => "路径表达式",
        Expr::Ident(_) => "裸标识符引用（同类静态字段简写或 const——本路径须写全限定形式）",
        _ => "其他未覆盖形态",
    }
}

#[cfg(test)]
mod v3_struct_static_tests {
    use super::*;
    use crate::EmitRole;
    use crate::GenerateToTable;
    use typeck::{ClassLayout, FieldLayout, ProgramLayouts, StaticFieldLayout, StructLayout};

    /// `Spanned` 便捷构造（DUMMY span）。
    fn sp<T>(node: T) -> ast::Spanned<T> {
        ast::Spanned::new(node, ast::Span::DUMMY)
    }

    /// 构造含 `Vector3.Zero = new Vector3(1.0, 2.0, 3.0)` 静态字段的布局。
    fn struct_static_layouts() -> ProgramLayouts {
        let vec3 = StructLayout {
            name: "Vector3".into(),
            fields: vec![
                FieldLayout {
                    name: "X".into(),
                    ty: "double".into(),
                    offset: 0,
                },
                FieldLayout {
                    name: "Y".into(),
                    ty: "double".into(),
                    offset: 8,
                },
                FieldLayout {
                    name: "Z".into(),
                    ty: "double".into(),
                    offset: 16,
                },
            ],
            is_readonly: false,
            soa: false,
            ..Default::default()
        };
        // `new Vector3(1.0, 2.0, 3.0)` 的 AST。
        let new_expr = sp(Expr::New {
            ty: sp(ast::Type::Named {
                path: vec!["Vector3".into()],
                generics: vec![],
            }),
            args: vec![
                sp(Expr::FloatLit(ast::FloatLitValue::Double(1.0))),
                sp(Expr::FloatLit(ast::FloatLitValue::Double(2.0))),
                sp(Expr::FloatLit(ast::FloatLitValue::Double(3.0))),
            ],
            obj_init: None,
        });
        ProgramLayouts {
            classes: IndexMap::new(),
            structs: IndexMap::from([("Vector3".into(), vec3)]),
            enums: Default::default(),
            enum_variants: Default::default(),
            interfaces: Default::default(),
            variants: Default::default(),
            static_fields: vec![StaticFieldLayout {
                class: "Vector3".into(),
                field: "Zero".into(),
                ty: "Vector3".into(),
                init: Some(new_expr),
                is_lazy: false,
            }],
            observable_properties: Default::default(),
            type_full_names: Default::default(),
        }
    }

    /// 将空容器泄漏为 `'static`（仅测试用），返回 emitter。
    fn make_emitter<'a>(layouts: &'a ProgramLayouts) -> ModuleEmitter<'a> {
        let empty_syms: &'static super::super::native::NativeSymbolTable =
            Box::leak(Box::new(std::collections::HashMap::new()));
        let empty_cbs: &'static super::super::emit_native_callback::NativeCallbackTable =
            Box::leak(Box::new(std::collections::HashMap::new()));
        let empty_rt: &'static super::super::native::RuntimeModuleInfos =
            Box::leak(Box::new(std::collections::HashMap::new()));
        let empty_spans: &'static std::collections::HashMap<String, ast::Span> =
            Box::leak(Box::new(std::collections::HashMap::new()));
        let empty_gen: &'static GenerateToTable = Box::leak(Box::new(GenerateToTable::default()));
        ModuleEmitter::new(
            layouts,
            false,
            false,
            "test.as",
            "",
            false,
            empty_spans,
            empty_syms,
            empty_cbs,
            String::new(),
            empty_gen,
            &[],
            EmitRole::MainObject,
            None,
            empty_rt,
        )
    }

    // RFC 006 V3：struct 静态字段发射**真实构造**（calloc + `__ctor::Vector3_3`），
    // 而非此前的 `zeroinitializer` 零值。全局为 `ptr` 类型（struct 按引用存储）。
    #[test]
    fn struct_static_field_emits_real_construction() {
        let layouts = struct_static_layouts();
        let mut em = make_emitter(&layouts);
        let globals = em.emit_static_field_globals();
        let (sinit, _diags) = em.emit_sinit_and_module_init(&[]);
        let ir = format!("{globals}\n{sinit}");

        // 全局声明：struct 静态字段全局（ptr 类型）——`Vector3` 按引用存储。
        assert!(
            ir.contains("@__static_Vector3_Zero = weak global ptr zeroinitializer"),
            "struct static global must be ptr-typed weak, got:\n{ir}"
        );
        // __sinit_Vector3 内含真实构造：calloc + __ctor::Vector3_3（非零值）。
        assert!(
            ir.contains("define linkonce_odr void @__sinit_Vector3()"),
            "eager __sinit_Vector3 must be emitted, got:\n{ir}"
        );
        assert!(
            ir.contains("call ptr @calloc(i64 1, i64 24)"),
            "struct static must calloc size 24 (3 doubles), got:\n{ir}"
        );
        assert!(
            ir.contains("call void @__ctor_Vector3_3"),
            "struct static must invoke arity-3 ctor, got:\n{ir}"
        );
        assert!(
            ir.contains("store ptr %"),
            "struct static construction result must be stored to global, got:\n{ir}"
        );
        // 急切：无惰性 guard（无 @__lazy_Vector3）。
        assert!(
            !ir.contains("@__lazy_Vector3"),
            "value-type eager static must have NO lazy guard, got:\n{ir}"
        );
        // 聚合器调用 __sinit_Vector3。
        assert!(
            ir.contains("call void @__sinit_Vector3()"),
            "@__arc_module_init must call __sinit_Vector3, got:\n{ir}"
        );
    }

    // RFC 006 V3：静态初始化**拓扑序**——`Transform.Identity = new Transform(Vector3.Zero, ...)`
    // 引用 Vector3 静态字段，`@__arc_module_init` 须先调 `__sinit_Vector3` 再调 `__sinit_Transform`。
    #[test]
    fn struct_static_init_dependency_topo_order() {
        // Transform 依赖 Vector3（通过 new Transform(Vector3.Zero) 引用其静态字段）。
        let new_v3 = sp(Expr::New {
            ty: sp(ast::Type::Named {
                path: vec!["Vector3".into()],
                generics: vec![],
            }),
            args: vec![sp(Expr::FloatLit(ast::FloatLitValue::Double(1.0)))],
            obj_init: None,
        });
        // Transform.Identity = new Transform(Vector3.Zero, ...)
        let field_ref = sp(Expr::Field {
            receiver: Box::new(sp(Expr::Ident("Vector3".into()))),
            field: "Zero".into(),
        });
        let transform_new = sp(Expr::New {
            ty: sp(ast::Type::Named {
                path: vec!["Transform".into()],
                generics: vec![],
            }),
            args: vec![field_ref],
            obj_init: None,
        });

        let vec3 = StructLayout {
            name: "Vector3".into(),
            fields: vec![FieldLayout {
                name: "X".into(),
                ty: "double".into(),
                offset: 0,
            }],
            is_readonly: false,
            soa: false,
            ..Default::default()
        };
        let transform = StructLayout {
            name: "Transform".into(),
            fields: vec![FieldLayout {
                name: "Pos".into(),
                ty: "Vector3".into(),
                offset: 0,
            }],
            is_readonly: false,
            soa: false,
            ..Default::default()
        };
        let layouts = ProgramLayouts {
            classes: IndexMap::new(),
            structs: IndexMap::from([("Vector3".into(), vec3), ("Transform".into(), transform)]),
            enums: Default::default(),
            enum_variants: Default::default(),
            interfaces: Default::default(),
            variants: Default::default(),
            static_fields: vec![
                StaticFieldLayout {
                    class: "Vector3".into(),
                    field: "Zero".into(),
                    ty: "Vector3".into(),
                    init: Some(new_v3),
                    is_lazy: false,
                },
                // Transform 声明在 Vector3 之后（模拟依赖方后声明）。
                StaticFieldLayout {
                    class: "Transform".into(),
                    field: "Identity".into(),
                    ty: "Transform".into(),
                    init: Some(transform_new),
                    is_lazy: false,
                },
            ],
            observable_properties: Default::default(),
            type_full_names: Default::default(),
        };

        let mut em = make_emitter(&layouts);
        let (sinit, _diags) = em.emit_sinit_and_module_init(&[]);
        // @__arc_module_init 内 __sinit_Vector3 必须出现在 __sinit_Transform 之前。
        let idx_v3 = sinit
            .find("call void @__sinit_Vector3()")
            .expect("sinit_Vector3 present");
        let idx_tf = sinit
            .find("call void @__sinit_Transform()")
            .expect("sinit_Transform present");
        assert!(
            idx_v3 < idx_tf,
            "topo order violated: Vector3 must init before Transform, got:\n{sinit}"
        );
    }

    /// 裸 `Action` 泛型实参必须归一化为 `Func_void`（对齐 typeck `lower_type`），
    /// 否则 `new List<Action>()` 静态初始化 mangle 为 `List_Action`，与布局
    /// 单态化键 `List_Func_void` 失配 → 静默零值 → 运行期空引用崩溃。
    #[test]
    fn action_generic_arg_normalizes_to_func_void() {
        assert_eq!(
            type_id_from_name("Action"),
            TypeId::Func {
                params: Vec::new(),
                ret: Box::new(TypeId::Void),
            }
        );
        assert_eq!(
            typeck::mangle_generic("List", &[type_id_from_name("Action")]),
            "List_Func_void"
        );
        // 对照：基元类型不受影响。
        assert_eq!(
            typeck::mangle_generic("List", &[type_id_from_name("long")]),
            "List_long"
        );
    }

    /// `new List<Action>()` 静态字段初始化必须内联 List ctor stub 序列
    /// （`rt_list_create` + store handle@16），而非 `call __ctor::List_<elem>`。
    /// 静态初始化器不产生 MIR 条目，引用外部 stub 会 undefined symbol；
    /// 且须真实构造，否则 `_cbCallback.Clear()` 对 null 对象 GEP+16 load 崩溃。
    #[test]
    fn static_list_of_delegate_inlines_ctor_stub() {
        let list_cls = ClassLayout {
            name: "List_Func_void".into(),
            fields: vec![FieldLayout {
                name: "_handle".into(),
                ty: "int".into(),
                offset: 16,
            }],
            parent: None,
            interfaces: vec![],
            method_impl: IndexMap::new(),
            virtual_slots: vec![],
            has_vtable: false,
            constructors: vec![vec![]],
            declared_methods: vec![],
            declared_properties: vec![],
        };
        let new_list = sp(Expr::New {
            ty: sp(ast::Type::Named {
                path: vec!["List".into()],
                generics: vec![sp(ast::Type::Named {
                    path: vec!["Action".into()],
                    generics: vec![],
                })],
            }),
            args: vec![],
            obj_init: None,
        });
        let layouts = ProgramLayouts {
            classes: IndexMap::from([("List_Func_void".into(), list_cls)]),
            structs: IndexMap::new(),
            enums: Default::default(),
            enum_variants: Default::default(),
            interfaces: Default::default(),
            variants: Default::default(),
            static_fields: vec![StaticFieldLayout {
                class: "MotionEngine".into(),
                field: "_cbCallback".into(),
                ty: "List_Func_void".into(),
                init: Some(new_list),
                is_lazy: false,
            }],
            observable_properties: Default::default(),
            type_full_names: Default::default(),
        };
        let mut em = make_emitter(&layouts);
        let (sinit, _diags) = em.emit_sinit_and_module_init(&[]);
        assert!(
            sinit.contains("call ptr @rt_list_create(i32 8, ptr null, ptr null, ptr null)"),
            "List<Action> static init must inline rt_list_create(elem_size=8), got:\n{sinit}"
        );
        assert!(
            !sinit.contains("call void @__ctor_List_Func_void"),
            "static init must NOT reference external __ctor::List_Func_void stub, got:\n{sinit}"
        );
        assert!(
            sinit.contains("getelementptr inbounds i8, ptr %.sinit."),
            "List static init must store handle into object, got:\n{sinit}"
        );
        // 聚合器必须调用 __sinit_MotionEngine。
        assert!(
            sinit.contains("call void @__sinit_MotionEngine()"),
            "@__arc_module_init must call __sinit_MotionEngine, got:\n{sinit}"
        );
    }
}

/// 静态初始化器 variant case 构造测试（`Content.None` / `Content.Text("...")`）。
///
/// 覆盖 MIR `variant_construct_rvalue_with_prep` 的形态识别全集：
/// - Field 形态（无 payload case：`Content.None`）
/// - MethodCall 形态（有 payload case：`Content.Text("x")`——Parser 产物）
/// - 非法形态显影诊断（payload case 裸引用 / arity 不匹配 → arc-sinit-003）
#[cfg(test)]
mod sinit_variant_construct_tests {
    use super::*;
    use crate::EmitRole;
    use crate::GenerateToTable;
    use typeck::{EnumVariantInfo, ProgramLayouts, StaticFieldLayout, VariantLayout};

    fn sp<T>(node: T) -> ast::Spanned<T> {
        ast::Spanned::new(node, ast::Span::DUMMY)
    }

    /// Content variant 布局（对齐 std/UI/Core/Markup/Content.as 的 None + Text of string 子集）。
    fn content_variant() -> VariantLayout {
        VariantLayout {
            name: "Content".into(),
            cases: vec![
                EnumVariantInfo {
                    name: "None".into(),
                    fields: vec![],
                    discriminant: 0,
                    payload: None,
                },
                EnumVariantInfo {
                    name: "Text".into(),
                    fields: vec![],
                    discriminant: 1,
                    payload: Some("string".into()),
                },
            ],
        }
    }

    /// 单静态字段布局：`ContentControl.ContentProperty = <init>`（类型 Content）。
    fn variant_static_layouts(init: ast::Spanned<Expr>) -> ProgramLayouts {
        ProgramLayouts {
            classes: Default::default(),
            structs: Default::default(),
            enums: Default::default(),
            enum_variants: Default::default(),
            interfaces: Default::default(),
            variants: indexmap::IndexMap::from([("Content".into(), content_variant())]),
            static_fields: vec![StaticFieldLayout {
                class: "ContentControl".into(),
                field: "ContentProperty".into(),
                ty: "Content".into(),
                init: Some(init),
                is_lazy: false,
            }],
            observable_properties: Default::default(),
            type_full_names: Default::default(),
        }
    }

    fn make_emitter<'a>(layouts: &'a ProgramLayouts) -> ModuleEmitter<'a> {
        let empty_syms: &'static super::super::native::NativeSymbolTable =
            Box::leak(Box::new(std::collections::HashMap::new()));
        let empty_cbs: &'static super::super::emit_native_callback::NativeCallbackTable =
            Box::leak(Box::new(std::collections::HashMap::new()));
        let empty_rt: &'static super::super::native::RuntimeModuleInfos =
            Box::leak(Box::new(std::collections::HashMap::new()));
        let empty_spans: &'static std::collections::HashMap<String, ast::Span> =
            Box::leak(Box::new(std::collections::HashMap::new()));
        let empty_gen: &'static GenerateToTable = Box::leak(Box::new(GenerateToTable::default()));
        ModuleEmitter::new(
            layouts,
            false,
            false,
            "test.as",
            "",
            false,
            empty_spans,
            empty_syms,
            empty_cbs,
            String::new(),
            empty_gen,
            &[],
            EmitRole::MainObject,
            None,
            empty_rt,
        )
    }

    /// `Content.None`（Field 形态，无 payload case）——alloca + 零初始化 +
    /// store tag 0，按引用（ptr）传递。无 arc-sinit-003 诊断。
    #[test]
    fn variant_static_init_none_case() {
        let init = sp(Expr::Field {
            receiver: Box::new(sp(Expr::Ident("Content".into()))),
            field: "None".into(),
        });
        let layouts = variant_static_layouts(init);
        let mut em = make_emitter(&layouts);
        let (sinit, diags) = em.emit_sinit_and_module_init(&[]);

        assert!(
            sinit.contains("alloca %variant.Content"),
            "None case must alloca %variant.Content, got:\n{sinit}"
        );
        assert!(
            sinit.contains("store %variant.Content zeroinitializer"),
            "None case must zero-init the variant (padding + body), got:\n{sinit}"
        );
        // tag 字段（GEP field 0）store 判别值 0（None）。
        let tag_gep_idx = sinit.find("i32 0, i32 0").expect("tag GEP present");
        let after_gep = &sinit[tag_gep_idx..];
        assert!(
            after_gep.contains("store i8 0, ptr %"),
            "None case tag must store discriminant 0, got:\n{sinit}"
        );
        assert!(
            !diags.iter().any(|d| d.code() == "arc-sinit-003"),
            "None case construction must not emit arc-sinit-003, got: {diags:?}"
        );
    }

    /// `Content.Text("hello")`（MethodCall 形态，有 payload case）——tag=1 +
    /// payload 存 body（GEP field 2）。string 为 rodata，不做 rt_arc_inc
    /// （MIR 路径同规则：非 class payload 豁免 ARC）。
    #[test]
    fn variant_static_init_text_case_payload() {
        let init = sp(Expr::MethodCall {
            receiver: Box::new(sp(Expr::Ident("Content".into()))),
            method: "Text".into(),
            args: vec![sp(Expr::StringLit("hello".to_string()))],
            type_args: vec![],
            params_span: None,
        });
        let layouts = variant_static_layouts(init);
        let mut em = make_emitter(&layouts);
        let (sinit, diags) = em.emit_sinit_and_module_init(&[]);

        assert!(
            sinit.contains("alloca %variant.Content"),
            "Text case must alloca %variant.Content, got:\n{sinit}"
        );
        // tag 字段 store 判别值 1（Text）。
        assert!(
            sinit.contains("store i8 1, ptr %"),
            "Text case tag must store discriminant 1, got:\n{sinit}"
        );
        // body 字段（GEP field 2）store payload。
        assert!(
            sinit.contains("i32 0, i32 2"),
            "Text case payload must store to body (GEP field 2), got:\n{sinit}"
        );
        // string literal payload 经 intern 全局常量 GEP。
        assert!(
            sinit.contains("@.sinit_str_ContentControl_ContentProperty_"),
            "Text case string payload must use interned string constant, got:\n{sinit}"
        );
        // string 非 class payload：不 rt_arc_inc（rodata 无 ArcHeader）。
        assert!(
            !sinit.contains("rt_arc_inc"),
            "string payload must NOT rt_arc_inc (rodata), got:\n{sinit}"
        );
        assert!(
            !diags.iter().any(|d| d.code() == "arc-sinit-003"),
            "Text case construction must not emit arc-sinit-003, got: {diags:?}"
        );
    }

    /// `Content.Text`（Field 形态 payload case 裸引用）——不是值表达式，
    /// 无法构造；arc-sinit-003 显影 + 零值兜底（完整性纪律）。
    #[test]
    fn variant_payload_case_bare_ref_diagnosed() {
        let init = sp(Expr::Field {
            receiver: Box::new(sp(Expr::Ident("Content".into()))),
            field: "Text".into(),
        });
        let layouts = variant_static_layouts(init);
        let mut em = make_emitter(&layouts);
        let (sinit, diags) = em.emit_sinit_and_module_init(&[]);

        assert!(
            diags
                .iter()
                .any(|d| d.code() == "arc-sinit-003"
                    && format!("{d:?}").contains("裸引用")),
            "bare payload case reference must emit arc-sinit-003 with bare-ref kind, got: {diags:?}"
        );
        // 零值兜底：不发射 variant 构造序列。
        assert!(
            !sinit.contains("alloca %variant.Content"),
            "bare payload case ref must NOT construct variant, got:\n{sinit}"
        );
    }

    /// `Content.Text()`（MethodCall 0 实参，payload case）——arity 不匹配；
    /// arc-sinit-003 显影。
    #[test]
    fn variant_case_arity_mismatch_diagnosed() {
        let init = sp(Expr::MethodCall {
            receiver: Box::new(sp(Expr::Ident("Content".into()))),
            method: "Text".into(),
            args: vec![],
            type_args: vec![],
            params_span: None,
        });
        let layouts = variant_static_layouts(init);
        let mut em = make_emitter(&layouts);
        let (_sinit, diags) = em.emit_sinit_and_module_init(&[]);

        assert!(
            diags
                .iter()
                .any(|d| d.code() == "arc-sinit-003" && format!("{d:?}").contains("实参数不匹配")),
            "arity mismatch must emit arc-sinit-003 with arity kind, got: {diags:?}"
        );
    }
}
