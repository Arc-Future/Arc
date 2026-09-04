//! DI Container codegen (RFC 023 M1) — 编译期工厂生成。
//!
//! v0.7 重构：DI 容器运行时逻辑已从 C runtime + codegen 拦截迁移到纯 Arc 实现
//! （见 std/DI/ServiceCollection.as / ServiceProvider.as / ServiceScope.as），
//! codegen 仅保留编译期工厂函数生成（因 Arc 零反射，必须编译期查询 ctor 签名）。
//!
//! ## 唯一 codegen 介入点：ServiceDescriptor 构造器变换（emit_new 末尾）
//!
//! `new ServiceDescriptor(typeof(TService), typeof(TImpl), lifetime)`
//! 在 emit_new 正常构造 ServiceDescriptor 对象后，codegen 检测到 方式1 模式，
//! 生成 `__di_factory_TImpl(sp)` 工厂函数与模块级闭包全局常量
//! `@.di_closure.TImpl`，内联写入 `desc.Factory` 字段（不调用 SetFactory
//! 方法——std 方法体仅在用户直接调用时才发射，codegen 注入调用会得到
//! undefined symbol）。
//!
//! ## 工厂函数生成（方式 1：实现类型构造）
//!
//! ```llvm
//! define ptr @__di_factory_ConsoleLogger(ptr %sp) {
//! entry:
//!   %inst = call ptr @calloc(i64 1, i64 <size>)
//!   store i32 1, ptr %inst                ; refcount = 1
//!   %vptr_slot = getelementptr ...        ; 若 has_vtable
//!   store ptr @.vtable.ConsoleLogger, ptr %vptr_slot
//!   ; RFC 018 M5：构造 RuntimeType（等同 typeof(Dep)），经 itable 调 GetService(Type)
//!   %dep_ty0 = ... RuntimeType { _typeInfoHandle = ptrtoint(@.typeinfo.Dep) }
//!   %dep0 = call ptr %GetService(ptr %sp_obj, ptr %dep_ty0)
//!   call void @__ctor_ConsoleLogger(ptr %inst, ptr %dep0, ...)
//!   ret ptr %inst
//! }
//! ```
//!
//! 工厂函数在 module 级别发射（emit_module 末尾），按 TImpl 去重。
//! 构造器选择对齐 .NET CallSiteFactory（见 [`FnEmitter::select_ctor_params`]）：
//! 多 ctor 取参数最多者，并列最多时取唯一超集签名者，无法唯一判定报编译期
//! 诊断；类无显式 ctor（默认 ctor）保持无参路径。不支持 struct 值类型服务
//!（仅 class）。
//!
//! ## 工厂闭包全局常量与注入 IR（emit_new 末尾）
//!
//! ```llvm
//! ; 模块级（与工厂函数同步去重发射）——env 恒 null、fn 编译期已知，
//! ; 无需每注册 malloc(16) 复制同一份常量：
//! @.di_closure.TImpl = internal global { ptr, ptr } { ptr @__di_factory_TImpl, ptr null }
//!
//! ; desc 已由 calloc + __ctor 正常构造后：
//! %factory_slot = getelementptr i8, ptr %desc, i32 <Factory offset>
//! store ptr @.di_closure.TImpl, ptr %factory_slot  ; 内联直写，零堆分配
//! ```

use super::*;
use mir::MirOperand;
use std::collections::HashSet;

/// 从 `new ServiceDescriptor(...)` args 中提取 方式1 的实现类型名。
///
/// 方式 1 特征：args[0] = TypeId operand (TService), args[1] = TypeId (TImpl), args[2] = ConstInt (lifetime)
/// 方式 2 特征：args[1] = Closure/FnPtr (factory delegate)
///
/// `MirOperand::TypeId` 是 MIR typeof 操作数名（历史命名），值为 RuntimeType，非语言 TypeId struct。
///
/// 返回 `Some(impl_type_name)` 仅当 方式1；其余返回 None。
pub(crate) fn extract_impl_type_for_factory(args: &[MirOperand]) -> Option<String> {
    if args.len() < 3 {
        return None;
    }
    // args[1] 为 typeof(TImpl) → MirOperand::TypeId
    match &args[1] {
        MirOperand::TypeId { type_name } => Some(type_name.clone()),
        _ => None,
    }
}

/// RuntimeType `_typeInfoHandle` 字段偏移（布局数据驱动；类或字段缺失返回
/// `None`，调用方回退 calloc 路径，避免硬编码偏移与真实布局漂移）。
fn runtime_type_handle_offset(layouts: &ProgramLayouts) -> Option<u64> {
    layouts.classes.get("RuntimeType").and_then(|c| {
        c.fields
            .iter()
            .find(|f| f.name.as_str() == "_typeInfoHandle")
            .map(|f| f.offset as u64)
    })
}

impl<'a> FnEmitter<'a> {
    /// 尝试注入 DI 工厂闭包到已构造的 ServiceDescriptor。
    ///
    /// 由 emit_new 在正常 calloc + __ctor 之后调用。
    /// 仅 方式1（args[1] 为 typeof(TImpl)）触发工厂生成与闭包注入；
    /// 方式2（args[1] 为工厂委托）无操作——用户已提供 Factory 委托。
    ///
    /// 注入内容：
    ///   1. 确保工厂函数与闭包全局 `@.di_closure.{TImpl}` 已生成（按 TImpl 去重）
    ///   2. GEP + store 把全局闭包地址内联写入 `Factory` 字段（offset 取自
    ///      class layout）
    ///
    /// 闭包结构 `{ fn, env=null }` 的两个字段编译期均已知，与每注册
    /// malloc(16) 复制同一份常量相比，模块级 internal global 零堆分配。
    /// 调用侧（`emit_closure_indirect_call`，env=null 无捕获分支）只经
    /// GEP+load 读槽位后 `call fn(&arg_slot)`——对全局常量的只读访问与
    /// 堆闭包完全同构，且 DI 路径无人写闭包槽 / 无人对其做 ARC 计数
    /// （闭包无对象头，原 malloc 路径同样不被 ARC 触碰），全局只读安全。
    ///
    /// `desc_ptr` 参数是 emit_new 中 calloc 返回的对象裸指针（%tmp 变量名）。
    pub(crate) fn try_inject_di_factory(
        &mut self,
        class: &str,
        args: &[MirOperand],
        desc_ptr: &str,
    ) {
        if class != "ServiceDescriptor" {
            return;
        }
        let impl_type = match extract_impl_type_for_factory(args) {
            Some(t) => t,
            None => return,
        };

        self.ensure_factory_generated(&impl_type);
        let closure_symbol = format!(".di_closure.{impl_type}");

        // 内联写入 Factory 字段（offset 取自 class layout，随布局自动漂移）。
        // 不能调用 SetFactory 方法：std 方法体只有在某 MIR body 直接调用时才发射，
        // e2e 用户代码不调用 → `use of undefined value`。而 SetFactory 本体就是
        // `Factory = factory;` 单字段 store，内联直写语义完全等价，且零符号依赖。
        let (factory_offset, _) = self.field_info("ServiceDescriptor", "Factory");
        let factory_slot = self.fresh_temp();
        self.emit(&format!(
            "{factory_slot} = getelementptr inbounds i8, ptr {desc_ptr}, i32 {factory_offset}"
        ));
        self.emit(&format!("store ptr @{closure_symbol}, ptr {factory_slot}"));
    }

    // ── 工厂函数生成（保留自 v0.6）──

    /// 确保 `__di_factory_<impl_type>` 工厂函数与闭包全局常量已生成。
    ///
    /// 按 impl_type 去重：同一 impl_type 多次 Add 只生成一个工厂与一个闭包。
    /// 工厂 IR 推入 `di_factories.irs`、闭包全局推入 `di_factories.closure_irs`，
    /// 由 ModuleEmitter 在模块级发射。
    fn ensure_factory_generated(&mut self, impl_type: &str) {
        if self.di_factories.names.contains(impl_type) {
            return;
        }
        self.di_factories.names.insert(impl_type.to_string());

        let factory_fn_name = format!("__di_factory_{impl_type}");
        let mangled = mangle_fn_name(&factory_fn_name);

        let factory_ir = self.generate_factory_ir(impl_type, &mangled);
        self.di_factories.irs.push(factory_ir);

        // 工厂闭包全局常量：`{ fn, env=null }` 两字段编译期已知（与 immortal
        // RuntimeType 全局同思路），注册路径直接存全局地址，零 malloc。
        // 符号前缀 `.di_closure.` 与既有 `.runtime_type.` / `.vtable.` /
        // `.typeinfo.` 命名空间不冲突。
        self.di_factories.closure_irs.push(format!(
            "@.di_closure.{impl_type} = internal global {{ ptr, ptr }} \
             {{ ptr @{mangled}, ptr null }}\n\n"
        ));
    }

    /// 按需登记 immortal RuntimeType 全局常量 `@.runtime_type.{T}`，返回符号名。
    ///
    /// 依赖解析原先每次工厂调用 `calloc` 一个 RuntimeType（refcount=1）再传给
    /// `GetService`（被调方返回路径 `rt_arc_dec` 归零释放）——每次解析一次
    /// 堆分配/释放。RuntimeType 在 DI 解析路径只承载 `_typeInfoHandle` 一个
    /// 编译期常量（GetService 仅读 TypeId），发射为模块级 immortal 全局
    /// （refcount 置哨兵 `INT32_MAX - 1`，`rt_arc_dec` 的 fetch_sub 结果永不
    /// 等于 1，对齐 Swift immortal object 手法）后依赖解析零分配。哨兵取
    /// `2147483646` 而非 `INT32_MAX`：实测本工具链 LLVM 汇编对十进制
    /// `i32 2147483647` 词法误报 expected type，`-1` 虽合法但引入负 refcount
    /// 语义。
    ///
    /// 布局数据驱动：从 `layouts` 读取真实 size 与 `_typeInfoHandle` 偏移，
    /// 全局常量按四段发射——标准对象头 16B（refcount@0 + padding@4 +
    /// vtable@8）+ 中段 zeroinitializer + `i64 _typeInfoHandle` + 尾段
    /// zeroinitializer。中/尾段字段（Type 基类成员）在 DI 路径不被触碰，
    /// 零填充安全。布局异常（字段缺失 / handle 溢出 size）回退 calloc 路径，
    /// 正确性优先。
    fn intern_runtime_type_global(&mut self, ty: &str) -> Option<String> {
        let rt_size = self.class_size("RuntimeType");
        let has_rt_vtable = self.class_has_vtable("RuntimeType");
        let handle_offset = runtime_type_handle_offset(self.layouts)?;
        if !has_rt_vtable || handle_offset < 16 || rt_size < handle_offset + 8 {
            return None;
        }
        let mid = handle_offset - 16;
        let tail = rt_size - handle_offset - 8;

        let symbol = format!(".runtime_type.{ty}");
        if !self.di_factories.runtime_type_names.contains(ty) {
            self.di_factories.runtime_type_names.insert(ty.to_string());
            // RFC 038 M2：外部依赖类型（LibraryObject）typeinfo / RuntimeType
            // vtable 经守卫登记 external 声明（runtime_type 全局常量内引用
            // @.typeinfo.{ty} / @.vtable.RuntimeType）。
            self.typeinfo_global(ty);
            self.vtable_global("RuntimeType");
            // 数组字段用带类型前缀的 `[N x i8] zeroinitializer`——实测本工具链
            // LLVM 汇编对 packed initializer 中大整数字面量后跟裸 `zeroinitializer`
            // 词法误报 expected type，typed 形式稳定（mini e2e 实证）。
            let (mid_ty, mid_init) = if mid > 0 {
                (
                    format!(", [{mid} x i8]"),
                    format!(", [{mid} x i8] zeroinitializer"),
                )
            } else {
                (String::new(), String::new())
            };
            let (tail_ty, tail_init) = if tail > 0 {
                (
                    format!(", [{tail} x i8]"),
                    format!(", [{tail} x i8] zeroinitializer"),
                )
            } else {
                (String::new(), String::new())
            };
            self.di_factories.runtime_type_irs.push(format!(
                "; Immortal RuntimeType: refcount 恒为哨兵值，rt_arc_dec 永不归零（零分配依赖解析）\n\
                 @.runtime_type.{ty} = internal global <{{ i32, [4 x i8], ptr{mid_ty}, i64{tail_ty} }}> \
                 <{{ i32 2147483646, [4 x i8] zeroinitializer, ptr @.vtable.RuntimeType{mid_init}, \
                 i64 ptrtoint (ptr @.typeinfo.{ty} to i64){tail_init} }}>\n\n"
            ));
        }
        Some(symbol)
    }

    /// DI 工厂构造器最优选择（RFC 023 冲刺批次二，对齐 .NET CallSiteFactory）。
    ///
    /// 规则：
    ///   1. 至多一个 ctor（含无显式 ctor 的空列表）→ 原样返回（默认 ctor
    ///      保持现状，走无参 `__ctor::Class` 路径）；
    ///   2. 多 ctor → 选参数最多者；
    ///   3. 并列最多 → 要求其中恰有一个的参数类型**集**是其余所有并列者
    ///      的超集，选它（同基数下超集者携带更完整的依赖集）；无法唯一
    ///      判定则 panic 报编译期诊断（对齐 native trampoline 超限的编译期
    ///      拒绝惯例），列出全部 ctor 签名后阻断编译，不做运行时回退。
    ///
    /// 仅读 `layouts.classes[].constructors`（多 ctor 信息 typeck 已记录），
    /// 不动 typeck；类型集按 HashSet 语义去重比较（重复参数类型不参与计数）。
    fn select_ctor_params(&self, impl_type: &str) -> Vec<String> {
        let Some(class) = self.layouts.classes.get(impl_type) else {
            return Vec::new();
        };
        let ctors: Vec<Vec<String>> = class
            .constructors
            .iter()
            .map(|params| params.iter().map(|p| p.to_string()).collect())
            .collect();
        if ctors.len() <= 1 {
            return ctors.into_iter().next().unwrap_or_default();
        }

        let max_len = ctors.iter().map(Vec::len).max().unwrap_or(0);
        let tied: Vec<&Vec<String>> = ctors.iter().filter(|c| c.len() == max_len).collect();
        if tied.len() == 1 {
            return tied[0].clone();
        }

        let tied_sets: Vec<(&Vec<String>, HashSet<&str>)> = tied
            .iter()
            .map(|c| (*c, c.iter().map(|s| s.as_str()).collect()))
            .collect();
        let supersets: Vec<&Vec<String>> = tied_sets
            .iter()
            .filter(|(_, set)| tied_sets.iter().all(|(_, other)| other.is_subset(set)))
            .map(|(c, _)| *c)
            .collect();
        if supersets.len() == 1 {
            return supersets[0].clone();
        }

        let signatures = ctors
            .iter()
            .map(|c| format!("({})", c.join(", ")))
            .collect::<Vec<_>>()
            .join("; ");
        panic!(
            "DI 多构造器歧义: '{impl_type}' 有 {} 个并列最多参数({max_len})的构造器，\
             且无唯一超集签名; 全部构造器: {signatures}; \
             请改用工厂委托注册(方式 2)或消除构造器重载歧义",
            tied.len()
        );
    }

    /// 生成 `__di_factory_<impl_type>` 函数的完整 LLVM IR 文本。
    ///
    /// 函数体：calloc 分配实例 → 初始化 refcount/vtable → 递归解析 ctor 依赖 → 调用 __ctor → 返回实例。
    ///
    /// 依赖解析（RFC 018 M5）：构造 RuntimeType（等同 `typeof(Dep)`），经 IServiceProvider
    /// itable slot 0 调用 `GetService(Type)`。%sp 为 fat pointer（obj + itable），因可能是
    /// ServiceProvider 或 ServiceScope。
    ///
    /// 构造器由 [`FnEmitter::select_ctor_params`] 按 .NET CallSiteFactory 语义选定。
    fn generate_factory_ir(&mut self, impl_type: &str, mangled: &str) -> String {
        let size = self.class_size(impl_type);

        let ctor_params = self.select_ctor_params(impl_type);

        // RuntimeType 布局（calloc 回退路径共用；immortal 常量化守卫见
        // `intern_runtime_type_global`）。字段缺失时偏移兜底 16（旧行为，
        // 实际 RuntimeType 属 std 稳定面、字段恒存在）。
        let rt_size = self.class_size("RuntimeType");
        let has_rt_vtable = self.class_has_vtable("RuntimeType");
        let handle_offset = runtime_type_handle_offset(self.layouts).unwrap_or(16);

        let mut out = String::new();
        // 工厂可能因缺失依赖抛 InvalidOperationException（rt_throw）；Windows 上须
        // 携带 uwtable + personality，否则 SEH 展开无法穿过本帧（异常变崩溃）。
        let eh_suffix = if self.is_windows {
            " uwtable personality ptr @__CxxFrameHandler3"
        } else {
            ""
        };
        out.push_str(&format!(
            "define ptr @{mangled}(ptr %sp){eh_suffix} {{\nentry:\n"
        ));
        // 闭包调用约定（GetService bb32）：env=null 时 `call fn(&this_slot)`，
        // `%sp` 是**指向栈槽的指针**，槽内才是 ServiceProvider/ServiceScope 对象。
        // 工厂若需递归解析依赖（GetService），必须先 `load` 解引用拿到真实 SP。
        out.push_str("  %sp_obj = load ptr, ptr %sp\n");
        // RFC 040 M-C：工厂可能被 ServiceScope 调用（作用域内解析），静态调用
        // `ServiceProvider::GetService` 会把 Scope 字段按 Provider 偏移读取 →
        // 崩溃。此处按对象 runtime type_id 动态选 IServiceProvider itable 并经
        // slot 0 分派 GetService（ServiceProvider/ServiceScope 共用）。
        // itable 候选存栈槽（alloca 于 entry，单帧内消费、不逃逸）——仅当 ctor
        // 形参含 IServiceProvider 时才构造堆 fat 盒（ctor 可能将其存入字段）。
        out.push_str("  %sp_it_slot = alloca ptr\n");
        out.push_str("  store ptr null, ptr %sp_it_slot\n");
        out.push_str("  %sp_vta = getelementptr inbounds i8, ptr %sp_obj, i64 8\n");
        out.push_str("  %sp_vt = load ptr, ptr %sp_vta\n");
        out.push_str("  %sp_tia = getelementptr inbounds ptr, ptr %sp_vt, i32 0\n");
        out.push_str("  %sp_tip = load ptr, ptr %sp_tia\n");
        out.push_str("  %sp_tid = load i32, ptr %sp_tip\n");
        let sp_cands: Vec<(String, i32)> = self
            .layouts
            .classes
            .values()
            .filter(|c| {
                c.has_vtable
                    && c.interfaces
                        .iter()
                        .any(|i| i.as_str() == "IServiceProvider")
            })
            .map(|c| {
                let id = crate::llvm_ir::emit_rvalue::type_name_to_id(c.name.as_str());
                (c.name.to_string(), id)
            })
            .collect();
        if !sp_cands.is_empty() {
            for (ci, (cname, tid)) in sp_cands.iter().enumerate() {
                let next = if ci + 1 < sp_cands.len() {
                    format!("sp_n{ci}")
                } else {
                    "sp_join".to_string()
                };
                out.push_str(&format!("  %sp_c{ci} = icmp eq i32 %sp_tid, {tid}\n"));
                out.push_str(&format!(
                    "  br i1 %sp_c{ci}, label %sp_m{ci}, label %{next}\n"
                ));
                out.push_str(&format!("sp_m{ci}:\n"));
                out.push_str(&format!(
                    "  store ptr @.itable.{cname}_IServiceProvider, ptr %sp_it_slot\n"
                ));
                out.push_str("  br label %sp_join\n");
                if ci + 1 < sp_cands.len() {
                    out.push_str(&format!("{next}:\n"));
                }
            }
            out.push_str("sp_join:\n");
            out.push_str("  %sp_it = load ptr, ptr %sp_it_slot\n");
            out.push_str("  %sp_gsa = getelementptr inbounds ptr, ptr %sp_it, i32 0\n");
            out.push_str("  %sp_gs = load ptr, ptr %sp_gsa\n");
        } else {
            // 无 IServiceProvider 实现者时退回静态调用（与旧行为一致）。
            let gs_mangled = mangle_fn_name("ServiceProvider::GetService");
            out.push_str(&format!("  %sp_gs = bitcast ptr @{gs_mangled} to ptr\n"));
        }
        out.push_str(&format!("  %inst = call ptr @calloc(i64 1, i64 {size})\n"));
        out.push_str("  store i32 1, ptr %inst\n");
        // RFC 038 M2：外部实现类（LibraryObject）vtable 经守卫登记 external 声明。
        if let Some(vt_sym) = self.vtable_global(impl_type) {
            out.push_str("  %vptr_slot = getelementptr inbounds i8, ptr %inst, i64 8\n");
            out.push_str(&format!("  store ptr {vt_sym}, ptr %vptr_slot\n"));
        }

        let mut dep_args = vec!["ptr %inst".to_string()];
        for (i, param_ty) in ctor_params.iter().enumerate() {
            // RFC 040 M-C：工厂的 `%sp` 自身即 IServiceProvider（ServiceProvider 或
            // ServiceScope）。ctor 依赖为 IServiceProvider 时直接传 `%sp_obj`
            //（已从 fat 盒解出的真实对象指针），不再经 GetService 解析，也不再
            // 传 fat 盒指针本身——类字段按「interface 裸对象指针」布局（如
            // Mediator._provider），getter 经 rt_obj_to_iface 按对象 runtime
            // type_id 动态重选 itable 再回传；若误传 fat 盒地址会把该地址当
            // 对象指针解引用 → 0xC0000005（Mediator::get_Provider 实测）。
            if param_ty.as_str() == "IServiceProvider" {
                dep_args.push("ptr %sp_obj".to_string());
                continue;
            }
            let is_interface = self.layouts.interfaces.contains_key(param_ty.as_str());
            let has_typeinfo = self
                .layouts
                .classes
                .get(param_ty.as_str())
                .map(|c| c.has_vtable)
                .unwrap_or(false)
                || is_interface;

            if has_typeinfo && self.layouts.classes.contains_key("RuntimeType") {
                // 依赖类型常量优先：immortal 全局 `@.runtime_type.{T}`（零分配）；
                // RuntimeType 布局异常时回退逐次 calloc（旧路径，语义等价）。
                let dep_ty_operand = match self.intern_runtime_type_global(param_ty) {
                    Some(sym) => format!("ptr @{sym}"),
                    None => {
                        out.push_str(&format!(
                            "  %dep_ty{i} = call ptr @calloc(i64 1, i64 {rt_size})\n"
                        ));
                        out.push_str(&format!("  store i32 1, ptr %dep_ty{i}\n"));
                        if has_rt_vtable {
                            // RFC 038 M2：RuntimeType（stdlib 外部类）vtable 经守卫登记 external 声明。
                            if let Some(rt_vt_sym) = self.vtable_global("RuntimeType") {
                                out.push_str(&format!(
                                    "  %rt_vt{i} = getelementptr inbounds i8, ptr %dep_ty{i}, i64 8\n"
                                ));
                                out.push_str(&format!("  store ptr {rt_vt_sym}, ptr %rt_vt{i}\n"));
                            }
                        }
                        // RFC 038 M2：外部依赖类型（external_class_names）typeinfo 经守卫登记 external 声明。
                        // RFC 017 阶段一：基元不达此处（has_typeinfo 守卫已排除）；
                        // 防御性处理走 rt_typeinfo_prim(id) call + ptrtoint 指令
                        // （常量表达式语法不接受寄存器操作数，须两步）。
                        if let Some(prim_id) = primitive_typeinfo_id(param_ty.as_str()) {
                            let ti = self.fresh_temp();
                            let h = self.fresh_temp();
                            out.push_str(&format!(
                                "  {ti} = call ptr @rt_typeinfo_prim(i32 {prim_id})\n"
                            ));
                            out.push_str(&format!("  {h} = ptrtoint ptr {ti} to i64\n"));
                            out.push_str(&format!(
                                "  store i64 {h}, ptr getelementptr inbounds i8, ptr %dep_ty{i}, i64 {handle_offset}\n"
                            ));
                        } else {
                            let param_ti = self
                                .typeinfo_global(param_ty.as_str())
                                .unwrap_or_else(|| format!("@.typeinfo.{param_ty}"));
                            out.push_str(&format!(
                                "  %rt_h{i} = getelementptr inbounds i8, ptr %dep_ty{i}, i64 {handle_offset}\n"
                            ));
                            out.push_str(&format!(
                                "  store i64 ptrtoint (ptr {param_ti} to i64), ptr %rt_h{i}\n"
                            ));
                        }
                        format!("ptr %dep_ty{i}")
                    }
                };
                out.push_str(&format!(
                    "  %dep{i} = call ptr %sp_gs(ptr %sp_obj, {dep_ty_operand})\n"
                ));
                // 缺失依赖检测（对齐 .NET）：GetService 返回 null（依赖未注册）→
                // 抛 InvalidOperationException（含依赖类型名），不再静默注入 null 至 ctor。
                let exc_msg = format!(
                    "Unable to resolve service for type '{param_ty}' while attempting to activate '{impl_type}'."
                );
                let exc_msg_global = self.intern_string(&exc_msg);
                let exc_msg_len = exc_msg.len() + 1;
                let exc_size = self.class_size("InvalidOperationException");
                let has_exc_vtable = self.class_has_vtable("InvalidOperationException");
                let exc_ctor = mangle_fn_name("__ctor::InvalidOperationException_1");
                out.push_str(&format!("  %dep{i}_null = icmp eq ptr %dep{i}, null\n"));
                out.push_str(&format!(
                    "  br i1 %dep{i}_null, label %dep{i}_throw, label %dep{i}_ok\n"
                ));
                out.push_str(&format!("dep{i}_throw:\n"));
                out.push_str(&format!(
                    "  %dep{i}_exc = call ptr @calloc(i64 1, i64 {exc_size})\n"
                ));
                out.push_str(&format!("  store i32 1, ptr %dep{i}_exc\n"));
                if has_exc_vtable {
                    // RFC 038 M2：InvalidOperationException（stdlib 外部类）vtable 经守卫登记 external 声明。
                    if let Some(exc_vt_sym) = self.vtable_global("InvalidOperationException") {
                        out.push_str(&format!(
                            "  %dep{i}_exc_vt = getelementptr inbounds i8, ptr %dep{i}_exc, i64 8\n"
                        ));
                        out.push_str(&format!("  store ptr {exc_vt_sym}, ptr %dep{i}_exc_vt\n"));
                    }
                }
                out.push_str(&format!(
                    "  call void @{exc_ctor}(ptr %dep{i}_exc, ptr getelementptr inbounds ([{exc_msg_len} x i8], ptr {exc_msg_global}, i32 0, i32 0))\n"
                ));
                out.push_str(&format!("  call void @rt_throw(ptr %dep{i}_exc)\n"));
                out.push_str("  unreachable\n");
                out.push_str(&format!("dep{i}_ok:\n"));
                if is_interface {
                    // RFC 040 M-C：接口 ctor 依赖。GetService 返回裸对象（`object?`），
                    // 而 ctor 形参是接口胖指针 `{obj, itable}`——直接传裸指针会把对象
                    // 头部 refcount/vtable 误当 fat[0]/fat[1]，itable 分派 `call vtable[0]`
                    // 执行 `.typeinfo` 数据区 → 0xC0000005（web_core_auth_concurrency 实测）。
                    // 此处构造堆盒胖指针，并按对象 runtime type_id 动态选 itable
                    //（与 `emit_make_iface_dyn` 同构）。
                    out.push_str(&format!(
                        "  %dep{i}_fat = call ptr @calloc(i64 1, i64 16)\n"
                    ));
                    out.push_str(&format!("  call void @rt_arc_inc(ptr %dep{i})\n"));
                    out.push_str(&format!(
                        "  %dep{i}_oa = getelementptr inbounds {{ ptr, ptr }}, ptr %dep{i}_fat, i32 0, i32 0\n"
                    ));
                    out.push_str(&format!("  store ptr %dep{i}, ptr %dep{i}_oa\n"));
                    out.push_str(&format!(
                        "  %dep{i}_vs = getelementptr inbounds {{ ptr, ptr }}, ptr %dep{i}_fat, i32 0, i32 1\n"
                    ));
                    out.push_str(&format!("  store ptr null, ptr %dep{i}_vs\n"));
                    out.push_str(&format!(
                        "  %dep{i}_vta = getelementptr inbounds i8, ptr %dep{i}, i64 8\n"
                    ));
                    out.push_str(&format!("  %dep{i}_vt = load ptr, ptr %dep{i}_vta\n"));
                    out.push_str(&format!(
                        "  %dep{i}_tia = getelementptr inbounds ptr, ptr %dep{i}_vt, i32 0\n"
                    ));
                    out.push_str(&format!("  %dep{i}_tip = load ptr, ptr %dep{i}_tia\n"));
                    out.push_str(&format!("  %dep{i}_tid = load i32, ptr %dep{i}_tip\n"));
                    let candidates: Vec<(String, i32)> = self
                        .layouts
                        .classes
                        .values()
                        .filter(|c| {
                            c.has_vtable
                                && c.interfaces
                                    .iter()
                                    .any(|ifc| ifc.as_str() == param_ty.as_str())
                        })
                        .map(|c| {
                            let id = crate::llvm_ir::emit_rvalue::type_name_to_id(c.name.as_str());
                            (c.name.to_string(), id)
                        })
                        .collect();
                    if !candidates.is_empty() {
                        let join = format!("dep{i}_join");
                        for (ci, (cname, type_id)) in candidates.iter().enumerate() {
                            let next = if ci + 1 < candidates.len() {
                                format!("dep{i}_n{ci}")
                            } else {
                                join.clone()
                            };
                            out.push_str(&format!(
                                "  %dep{i}_c{ci} = icmp eq i32 %dep{i}_tid, {type_id}\n"
                            ));
                            out.push_str(&format!(
                                "  br i1 %dep{i}_c{ci}, label %dep{i}_m{ci}, label %{next}\n"
                            ));
                            out.push_str(&format!("dep{i}_m{ci}:\n"));
                            out.push_str(&format!(
                                "  store ptr @.itable.{cname}_{param_ty}, ptr %dep{i}_vs\n"
                            ));
                            out.push_str(&format!("  br label %{join}\n"));
                            if ci + 1 < candidates.len() {
                                out.push_str(&format!("{next}:\n"));
                            }
                        }
                        out.push_str(&format!("{join}:\n"));
                    }
                    dep_args.push(format!("ptr %dep{i}_fat"));
                } else {
                    dep_args.push(format!("ptr %dep{i}"));
                }
            } else {
                // 无 typeinfo 时无法构造 Type；依赖解析失败 → null
                dep_args.push("ptr null".into());
            }
        }

        // ctor 碰撞（同参数量重载）判定与 typeck/emit_new 同源：存在多个
        // 与已选 ctor 同参数个数的构造器时按签名 mangle 消歧。
        let ctor_collision = !ctor_params.is_empty()
            && self.layouts.classes.get(impl_type).is_some_and(|c| {
                c.constructors
                    .iter()
                    .filter(|p| p.len() == ctor_params.len())
                    .count()
                    > 1
            });
        let ctor_mangled = mangle_fn_name(&di_ctor_symbol(impl_type, &ctor_params, ctor_collision));
        out.push_str(&format!(
            "  call void @{ctor_mangled}({})\n",
            dep_args.join(", ")
        ));
        out.push_str("  ret ptr %inst\n");
        out.push_str("}\n\n");
        out
    }
}

/// 计算 impl 类型的 ctor 符号名（与 check_class.rs 的 ctor 重载 mangle 一致）：
/// 无参 ctor `__ctor::Class`，有参 ctor `__ctor::Class_<arity>`（arity = 形参个数，
/// 排除 this）；当 impl 类型存在同参数量碰撞（`collision`）时按签名
/// `__ctor::Class_<arity>_<p0>...` 消歧。`ctor_params` 为工厂已解析的构造器
/// 形参类型（不含 this）。
fn di_ctor_symbol(impl_type: &str, ctor_params: &[String], collision: bool) -> String {
    if ctor_params.is_empty() {
        format!("__ctor::{impl_type}")
    } else if collision {
        format!(
            "__ctor::{impl_type}_{}_{}",
            ctor_params.len(),
            ctor_params.join("_")
        )
    } else {
        format!("__ctor::{impl_type}_{}", ctor_params.len())
    }
}

/// 模块级 DI 工厂去重集合与累积器。
#[derive(Default)]
pub(crate) struct DiFactoryAccumulator {
    pub names: HashSet<String>,
    pub irs: Vec<String>,
    /// 工厂闭包全局常量（`@.di_closure.{T}`）定义文本，与工厂函数同步去重，
    /// 由 ModuleEmitter 在模块全局区发射（注册路径零 malloc）。
    pub closure_irs: Vec<String>,
    /// immortal RuntimeType 全局（`@.runtime_type.{T}`）去重集合与定义文本，
    /// 由 ModuleEmitter 在工厂函数之前发射（依赖解析零分配路径）。
    pub runtime_type_names: HashSet<String>,
    pub runtime_type_irs: Vec<String>,
}

impl DiFactoryAccumulator {
    pub fn new() -> Self {
        Self::default()
    }
}
