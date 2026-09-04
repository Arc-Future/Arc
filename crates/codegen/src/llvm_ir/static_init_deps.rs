//! 静态初始化依赖分析（RFC 006 M4 架构级方案 B）。
//!
//! ## 动机
//!
//! `__arc_module_init` 按拓扑序调用各 `__sinit_<Class>`。旧实现（`collect_static_init_deps`）
//! 只收集初始化器**表达式树内直接可见**的 `Expr::Field` 引用（如 `Vector3.Zero`），对
//! `Expr::Call` / `Expr::MethodCall` 只扫 func/实参标识符，**不穿透被调函数体**。当静态字段
//! 初始化器调用一个函数、而该函数体读写其它类的静态字段时（如 UI 各类型静态初始化调用
//! `RegisterProperty<T>` → `DependencyPropertyRegistry._byOwner`），依赖边缺失 →
//! `__sinit_DependencyPropertyRegistry` 被排到调用之后 → 运行期在 `@__arc_module_init`
//! 阶段对零值静态字段解引用崩溃（0xC0000005，CD-5）。
//!
//! ## 架构：基于 MIR 的统一依赖分析
//!
//! 依赖分析**直接消费 codegen 发射所依赖的同一份 MIR**（`fns: &[(String, MirCfgBody)]`），
//! 而非从 AST 重建被调函数语义。MIR 中静态全局访问已被降级为结构化形式：
//!
//! - 读：`MirOperand::StaticField { class, field }`（codegen 发射 `load @__static_<C>_<f>`）
//! - 写：`MirStatement::StaticFieldSet { class, field, .. }`（store 到同一全局）
//!
//! 算法：
//!
//! 1. **符号倒排索引**：对每个 `fns` 键，记 `mangle_fn_name(key) → key`（含归一化变体：
//!    MIR 泛型方法键 `Class::M__T` 与静态初始化器 mangle 约定 `Class_M_T` 经
//!    `__`→`_` 归一化桥接）。初始化器表达式算出的 LLVM 符号经索引反查函数体。
//! 2. **函数级摘要**：扫描每个函数体，得 `direct_static`（直接读写的静态字段宿主类）与
//!    `direct_callees`（直接调用的函数键，含静态方法 `Class::M`、构造函数 `__ctor::C`）。
//! 3. **传递闭包（fixpoint）**：`closure[f] = direct_static[f] ∪ ⋃ closure[callee]`。fixpoint
//!    对**函数调用环**天然正确（环成员的直接静态字段并入并集），且无递归爆栈风险。
//! 4. **类级依赖**：对每个类 `C`，`deps(C)` = 初始化器 AST 直接引用 ∪ 初始化器内每个调用
//!    目标对应函数体的 `closure`。仅分析**急切**字段（`is_lazy == false`）——惰性字段在
//!    首次访问时初始化（模块 `@__arc_module_init` 之后），不参与急切排序。调用目标符号
//!    不可解析（跨包/外部/stub）时按「保守排序 + 诊断」处理：不加合成边，输出
//!    `arc-sinit-002` 诊断（`New` 构造器例外——stub ctor 由 codegen 内联，静默跳过）。
//! 5. **初始化序环**：Kahn 拓扑排序完成后剩余类即环成员，输出 `arc-sinit-001` 诊断，
//!    环内按声明序回退（不静默）。
//!
//! 诊断以结构化载荷产出（见 [`super::static_init_diag::StaticInitDiagnostic`]），由 arc CLI
//! pipeline 统一渲染为 `warning[<code>]: <message>` 打印到 stderr，不阻断编译（exit 0）。
//!
//! ## 已知边界（保守降级，文档记录）
//!
//! - **虚调用 / 间接调用**：目标运行时动态解析，静态不可得 → 不合成调用边。函数指针值
//!   （`MirOperand::FnPtr` / `Closure`）被**保守纳入**被调边（过近似无害）。
//! - **lambda 体调用**：`LinqChain` 的 lambda 被提升为独立 `__lambda_rt_N` 函数并纳入整体
//!   扫描；但提升函数经间接调用触发，父函数→lambda 的调用边不追踪（与 `attr.rs` 对 LINQ
//!   的处理一致）。lambda 体直接读静态字段在 `fns` 对应提升函数扫描中捕获。
//! - **裸调用歧义**：同一裸调用既可能是自由函数也可能是类静态方法，候选符号一并携带
//!   （`Name` 与 `Class_Name`），反查命中任一即解析。`declared_methods` 对私有静态方法的
//!   `is_static` 标记实测不可靠，故不依赖该判定。
//! - **static/instance 同名重载**：MIR 键可能带 arity 后缀（`Class::M_int`），多候选符号
//!   未覆盖该形态；静态初始化器中调用此类方法极为罕见，缺失时依赖分析不报错、仅可能
//!   少排一条边。

use super::static_init_diag::StaticInitDiagnostic;
use super::*;
use ast::{Expr, Ident, Spanned, Type, TypeId};
use indexmap::IndexMap;
use mir::{MirCfgBody, MirOperand, MirRvalue, MirStatement, MirTerminator};
use std::collections::{HashMap, HashSet};
use typeck::StaticFieldLayout;

/// 静态初始化器表达式内的一个调用目标。
///
/// 与 `emit_static_init_expr` 的调用分支对偶：`Free` 对应裸 `Call`（自由函数或当前类
/// 静态方法），`StaticMethod` 对应 `Class.Method`（receiver 为类名），`New` 对应
/// `new Class(args)`（构造函数体同样可能读写静态字段）。
#[derive(Debug, Clone)]
enum InitCallee {
    /// 裸调用。`enclosing_class` 为所在类：同一调用既可能是自由函数，也可能是本类
    /// 静态方法（`Class::M` 符号），候选列表一并携带、反查命中任一即可。
    Free {
        name: String,
        type_ids: Vec<TypeId>,
        enclosing_class: Option<Ident>,
    },
    StaticMethod {
        class: String,
        method: String,
        type_ids: Vec<TypeId>,
    },
    New {
        class: String,
        arity: usize,
    },
}

/// 分析结果：类级依赖 + 编译期诊断（结构化，随 `__sinit` 发射由 pipeline 渲染）。
pub(crate) struct StaticInitDeps {
    /// `class → 必须先于它执行 `__sinit` 的类集合`（已去重、剔除自身、过滤外部类）。
    pub deps: IndexMap<Ident, Vec<Ident>>,
    /// `arc-sinit-XXX` 结构化诊断（去重）。
    pub warnings: Vec<StaticInitDiagnostic>,
}

/// 执行静态初始化依赖分析（方案 B：穿透被调函数体）。
pub(crate) fn analyze_static_init_deps(
    by_class: &IndexMap<Ident, Vec<&StaticFieldLayout>>,
    fns: &[(String, MirCfgBody)],
) -> StaticInitDeps {
    let mut warnings: Vec<StaticInitDiagnostic> = Vec::new();

    // 1. 函数体键索引 + 符号倒排索引（主符号 + `__`→`_` 归一化变体）。
    let mut body_by_key: HashMap<String, &MirCfgBody> = HashMap::new();
    let mut symbol_to_keys: HashMap<String, Vec<String>> = HashMap::new();
    for (key, body) in fns {
        body_by_key.insert(key.clone(), body);
        let sym = mangle_fn_name(key);
        symbol_to_keys
            .entry(sym.clone())
            .or_default()
            .push(key.clone());
        let normalized = normalize_symbol(&sym);
        if normalized != sym {
            symbol_to_keys
                .entry(normalized)
                .or_default()
                .push(key.clone());
        }
    }

    // 2. 函数级摘要（直接静态字段宿主 + 直接被调函数）。
    let mut direct_static: HashMap<String, HashSet<Ident>> = HashMap::new();
    let mut direct_callees: HashMap<String, HashSet<String>> = HashMap::new();
    for (key, body) in fns {
        let mut classes = HashSet::new();
        let mut callees = HashSet::new();
        scan_body(body, &mut classes, &mut callees);
        direct_static.insert(key.clone(), classes);
        direct_callees.insert(key.clone(), callees);
    }

    // 3. 传递闭包（fixpoint；函数调用环正确并入并集）。
    // 避免 `closure.iter_mut()` 与 `closure.get()` 的别名冲突：先快照传播源，再统一写入。
    let mut closure: HashMap<String, HashSet<Ident>> = direct_static.clone();
    let mut changed = true;
    while changed {
        changed = false;
        // 本轮传播的目标键 → 待并入的新类集合。
        let mut additions: Vec<(String, Vec<Ident>)> = Vec::new();
        for key in closure.keys() {
            let deps = &closure[key];
            let mut extra: Vec<Ident> = Vec::new();
            if let Some(callee_set) = direct_callees.get(key) {
                for c in callee_set {
                    if let Some(cd) = closure.get(c) {
                        for d in cd {
                            if !deps.contains(d) && !extra.contains(d) {
                                extra.push(d.clone());
                            }
                        }
                    }
                }
            }
            if !extra.is_empty() {
                additions.push((key.clone(), extra));
            }
        }
        if !additions.is_empty() {
            changed = true;
            for (key, extra) in additions {
                closure.entry(key).or_default().extend(extra);
            }
        }
    }

    // 4. 类级依赖集合。
    // 仅分析**急切**字段（`is_lazy == false`）：惰性字段在首次访问时初始化（模块
    // `@__arc_module_init` 之后），其初始化器调用不参与急切排序，跳过可消除
    // `_makeZhCN`/`ColorValue` 等惰性模板工厂的解析噪音。
    let class_names: Vec<Ident> = by_class.keys().cloned().collect();
    let mut deps: IndexMap<Ident, Vec<Ident>> = IndexMap::new();
    let mut warned_symbols: HashSet<String> = HashSet::new();
    for (class, fields) in by_class {
        let mut set: Vec<Ident> = Vec::new();
        for sf in fields {
            if sf.is_lazy {
                continue;
            }
            let Some(init) = &sf.init else {
                continue;
            };
            collect_static_init_deps(&init.node, &class_names, &mut set);
            let mut callees = Vec::new();
            collect_init_callees(&init.node, class, &mut callees);
            for callee in callees {
                let symbols = callee_symbols(&callee);
                let mut resolved = false;
                for sym in &symbols {
                    if let Some(keys) = symbol_to_keys.get(sym) {
                        for key in keys {
                            resolved = true;
                            if let Some(fn_deps) = closure.get(key) {
                                for d in fn_deps {
                                    // 仅纳入本 TU 有 `__sinit` 的类（外部类自归所属包 init）。
                                    if class_names.contains(d) && !set.contains(d) {
                                        set.push(d.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                // `New` 构造器不可解析 = stub（List/Weak/template ctor 由 codegen 内联，
                // 不引用静态全局）；真实 ctor 被调用则必在 `fns`。静默跳过，避免噪音。
                if !resolved && !matches!(callee, InitCallee::New { .. }) {
                    let sym0 = symbols.first().cloned().unwrap_or_default();
                    if warned_symbols.insert(sym0.clone()) {
                        warnings.push(StaticInitDiagnostic::UnresolvedCallee { symbol: sym0 });
                    }
                }
            }
        }
        // 去重 + 剔除自身。
        let mut cleaned: Vec<Ident> = Vec::new();
        for d in set {
            if d != *class && !cleaned.contains(&d) {
                cleaned.push(d);
            }
        }
        deps.insert(class.clone(), cleaned);
    }

    // 5. 初始化序环诊断（拓扑排序后剩余成员）。
    let remaining = kahn_remaining(&deps, &class_names);
    if !remaining.is_empty() {
        warnings.push(StaticInitDiagnostic::InitCycle { members: remaining });
    }

    StaticInitDeps { deps, warnings }
}

/// Kahn 拓扑排序后仍未满足依赖的类（即环成员或环的传递依赖者）。
fn kahn_remaining(deps: &IndexMap<Ident, Vec<Ident>>, class_names: &[Ident]) -> Vec<Ident> {
    let mut done: Vec<Ident> = Vec::new();
    let mut remaining: Vec<Ident> = class_names.to_vec();
    let mut changed = true;
    while changed {
        changed = false;
        let mut next_round: Vec<Ident> = Vec::new();
        for class in &remaining {
            let unmet = deps
                .get(class)
                .map(|d| d.iter().any(|x| !done.contains(x)))
                .unwrap_or(false);
            if unmet {
                next_round.push(class.clone());
            } else {
                done.push(class.clone());
                changed = true;
            }
        }
        remaining = next_round;
    }
    remaining
}

/// 按依赖图对类做拓扑排序，返回执行序（声明序稳定；环成员按声明序回退追加）。
pub(crate) fn topological_sort(deps: &IndexMap<Ident, Vec<Ident>>) -> Vec<Ident> {
    let class_names: Vec<Ident> = deps.keys().cloned().collect();
    let mut order: Vec<Ident> = Vec::new();
    let mut done: Vec<Ident> = Vec::new();
    let mut remaining: Vec<Ident> = class_names;
    let mut changed = true;
    while changed {
        changed = false;
        let mut next_round: Vec<Ident> = Vec::new();
        for class in &remaining {
            let unmet = deps
                .get(class)
                .map(|d| d.iter().any(|x| !done.contains(x)))
                .unwrap_or(false);
            if unmet {
                next_round.push(class.clone());
            } else {
                order.push(class.clone());
                done.push(class.clone());
                changed = true;
            }
        }
        remaining = next_round;
    }
    for class in remaining {
        if !done.contains(&class) {
            order.push(class);
        }
    }
    order
}

/// MIR 泛型方法键 `Class::M__T`（`mangle_fn_name` 后 `Class_M__T`）与静态初始化器
/// mangle 约定 `Class_M_T` 之间的符号归一化：`__` → `_`。
fn normalize_symbol(sym: &str) -> String {
    sym.replace("__", "_")
}

/// 初始化器调用目标 → 候选 LLVM 符号列表（倒排索引反查用）。
///
/// 多候选覆盖 mangle 约定差异：泛型实例（`mangle_generic`）、非泛型回退（裸名）、
/// MIR `Class::M__T` 约定等。命中任一即算解析成功。
fn callee_symbols(callee: &InitCallee) -> Vec<String> {
    match callee {
        InitCallee::Free {
            name,
            type_ids,
            enclosing_class,
        } => {
            let mut syms: Vec<String> = if type_ids.is_empty() {
                vec![name.clone()]
            } else {
                vec![typeck::mangle_generic(name, type_ids), name.clone()]
            };
            // 裸调用也可能是当前类的静态方法（`Class::M` → `Class_M`）。恒附带该候选：
            // 反查 miss 即忽略，命中则解析为类静态方法体。不依赖 `declared_methods` 的
            // `is_static` 标记（对私有静态方法实测不可靠）。
            if let Some(class) = enclosing_class {
                let base = if type_ids.is_empty() {
                    name.clone()
                } else {
                    typeck::mangle_generic(name, type_ids)
                };
                syms.push(mangle_method(class, &base));
                syms.push(mangle_method(class, name));
            }
            syms
        }
        InitCallee::StaticMethod {
            class,
            method,
            type_ids,
        } => {
            let base = if type_ids.is_empty() {
                method.clone()
            } else {
                typeck::mangle_generic(method, type_ids)
            };
            vec![mangle_method(class, &base), mangle_method(class, method)]
        }
        InitCallee::New { class, arity } => {
            if *arity == 0 {
                vec![mangle_fn_name(&format!("__ctor::{class}"))]
            } else {
                vec![
                    mangle_fn_name(&format!("__ctor::{class}_{arity}")),
                    mangle_fn_name(&format!("__ctor::{class}")),
                ]
            }
        }
    }
}

/// 递归收集静态字段初始化器表达式中的**调用目标**。
///
/// 与 `collect_static_init_deps` 对偶：该函数收集直接引用的静态字段宿主类，
/// 本函数收集调用点，供函数体穿透依赖分析使用。
fn collect_init_callees(expr: &Expr, class: &Ident, out: &mut Vec<InitCallee>) {
    match expr {
        Expr::Call {
            func,
            args,
            type_args,
            ..
        } => {
            if let Expr::Ident(func_name) = &func.node {
                out.push(InitCallee::Free {
                    name: func_name.to_string(),
                    type_ids: type_args_to_ids(type_args),
                    enclosing_class: Some(class.clone()),
                });
            }
            collect_init_callees(&func.node, class, out);
            for arg in args {
                collect_init_callees(&arg.node, class, out);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            type_args,
            ..
        } => {
            if let Expr::Ident(class_name) = &receiver.node {
                out.push(InitCallee::StaticMethod {
                    class: class_name.to_string(),
                    method: method.to_string(),
                    type_ids: type_args_to_ids(type_args),
                });
            }
            collect_init_callees(&receiver.node, class, out);
            for arg in args {
                collect_init_callees(&arg.node, class, out);
            }
        }
        Expr::New {
            ty, args, obj_init, ..
        } => {
            let type_name = new_type_name(ty);
            out.push(InitCallee::New {
                class: type_name,
                arity: args.len(),
            });
            for arg in args {
                collect_init_callees(&arg.node, class, out);
            }
            if let Some(inits) = obj_init {
                for (_, e) in inits {
                    collect_init_callees(&e.node, class, out);
                }
            }
        }
        Expr::Field { receiver, .. } => {
            collect_init_callees(&receiver.node, class, out);
        }
        Expr::Unary { expr, .. } => collect_init_callees(&expr.node, class, out),
        Expr::Binary { left, right, .. } => {
            collect_init_callees(&left.node, class, out);
            collect_init_callees(&right.node, class, out);
        }
        Expr::Index {
            receiver, index, ..
        } => {
            collect_init_callees(&receiver.node, class, out);
            collect_init_callees(&index.node, class, out);
        }
        Expr::Cast { expr, .. } => collect_init_callees(&expr.node, class, out),
        Expr::Coalesce { left, right, .. } => {
            collect_init_callees(&left.node, class, out);
            collect_init_callees(&right.node, class, out);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            collect_init_callees(&cond.node, class, out);
            collect_init_callees(&then_branch.node, class, out);
            collect_init_callees(&else_branch.node, class, out);
        }
        Expr::NullCond { access, .. } | Expr::ForceDeref { access, .. } => {
            collect_init_callees(&access.node, class, out);
        }
        _ => {}
    }
}

/// `new T(...)` 的类型名（泛型经 mangle 归一化，对齐 `emit_static_new_expr`）。
fn new_type_name(ty: &Spanned<Type>) -> String {
    match &ty.node {
        Type::Named { path, generics } => {
            let def = path
                .last()
                .map(|i| i.as_str().to_string())
                .unwrap_or_default();
            if generics.is_empty() {
                def
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
                typeck::mangle_generic(&def, &args)
            }
        }
        _ => String::new(),
    }
}

/// `type_args` → `TypeId` 列表（对齐 `emit_static_init_expr` 的提取逻辑）。
fn type_args_to_ids(type_args: &[Spanned<Type>]) -> Vec<TypeId> {
    type_args
        .iter()
        .map(|t| match &t.node {
            Type::Named { path, .. } => {
                type_id_from_name(path.last().map(|i| i.as_str()).unwrap_or("void"))
            }
            _ => TypeId::Void,
        })
        .collect()
}

// ---- MIR 函数体扫描 ----

/// 扫描函数体：收集直接读写的静态字段宿主类 + 直接被调函数键。
fn scan_body(body: &MirCfgBody, classes: &mut HashSet<Ident>, callees: &mut HashSet<String>) {
    for (_id, block) in &body.blocks {
        for stmt in &block.statements {
            scan_stmt(stmt, classes, callees);
        }
        match &block.terminator {
            MirTerminator::CondBr { cond, .. } => scan_operand(cond, classes, callees),
            MirTerminator::Return(Some(op)) => scan_operand(op, classes, callees),
            MirTerminator::Throw(op) => scan_operand(op, classes, callees),
            MirTerminator::Goto(_) | MirTerminator::Unreachable | MirTerminator::Return(None) => {}
        }
    }
}

fn scan_stmt(stmt: &MirStatement, classes: &mut HashSet<Ident>, callees: &mut HashSet<String>) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => scan_rvalue(rvalue, classes, callees),
        MirStatement::Return(Some(rv)) => scan_rvalue(rv, classes, callees),
        MirStatement::Return(None) => {}
        MirStatement::If {
            cond,
            then_body,
            else_body,
        } => {
            scan_operand(cond, classes, callees);
            for s in then_body {
                scan_stmt(s, classes, callees);
            }
            for s in else_body {
                scan_stmt(s, classes, callees);
            }
        }
        MirStatement::While { cond, body, .. } => {
            scan_rvalue(cond, classes, callees);
            for s in body {
                scan_stmt(s, classes, callees);
            }
        }
        MirStatement::FieldSet { object, value, .. } => {
            scan_operand(object, classes, callees);
            scan_rvalue(value, classes, callees);
        }
        MirStatement::StaticFieldSet { class, value, .. } => {
            classes.insert(class.as_str().into());
            scan_rvalue(value, classes, callees);
        }
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            scan_operand(array, classes, callees);
            scan_operand(index, classes, callees);
            scan_rvalue(value, classes, callees);
        }
        MirStatement::LinqForeach { chain, body, .. } => {
            scan_linq_chain(chain, classes, callees);
            for s in body {
                scan_stmt(s, classes, callees);
            }
        }
        MirStatement::Await { task, .. } => scan_rvalue(task, classes, callees),
        MirStatement::Throw { value } => scan_rvalue(value, classes, callees),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                scan_stmt(s, classes, callees);
            }
            for s in catch_body {
                scan_stmt(s, classes, callees);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                scan_stmt(s, classes, callees);
            }
            for s in finally {
                scan_stmt(s, classes, callees);
            }
        }
        MirStatement::Drop(_) | MirStatement::Break | MirStatement::Continue => {}
    }
}

/// LINQ 链：扫描 source 操作数。lambda 体被提升为独立 `__lambda_rt_N` 函数，其直接
/// 静态字段引用在 `fns` 对应提升函数扫描中捕获；父函数→lambda 的调用边不追踪
/// （与 `attr.rs` 对 LINQ 的处理一致，见模块文档「已知边界」）。
fn scan_linq_chain(
    chain: &mir::LinqChain,
    classes: &mut HashSet<Ident>,
    callees: &mut HashSet<String>,
) {
    scan_operand(&chain.source, classes, callees);
}

fn scan_rvalue(rv: &MirRvalue, classes: &mut HashSet<Ident>, callees: &mut HashSet<String>) {
    match rv {
        MirRvalue::Use(op) => scan_operand(op, classes, callees),
        MirRvalue::Binary { left, right, .. } => {
            scan_operand(left, classes, callees);
            scan_operand(right, classes, callees);
        }
        MirRvalue::Call { func, args } => {
            callees.insert(func.clone());
            for a in args {
                scan_operand(a, classes, callees);
            }
        }
        MirRvalue::New { class, args, .. } => {
            callees.insert(format!("__ctor::{class}_{}", args.len()));
            callees.insert(format!("__ctor::{class}"));
            for a in args {
                scan_operand(a, classes, callees);
            }
        }
        MirRvalue::FieldGet { object, .. } => scan_operand(object, classes, callees),
        MirRvalue::MethodCall {
            receiver,
            method,
            args,
            receiver_type,
            impl_class,
            target_fn,
            is_virtual,
            ..
        } => scan_method_call_targets(
            method,
            receiver_type,
            impl_class.as_deref(),
            target_fn.as_deref(),
            *is_virtual,
            callees,
            receiver,
            args,
            classes,
        ),
        MirRvalue::MakeIface { object, .. } => scan_operand(object, classes, callees),
        MirRvalue::MakeIfaceDyn { object, .. } => scan_operand(object, classes, callees),
        MirRvalue::AdaptIface { object, .. } => scan_operand(object, classes, callees),
        MirRvalue::StructLit { fields, .. } => {
            for (_, op) in fields {
                scan_operand(op, classes, callees);
            }
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for el in elements {
                match el {
                    mir::ArrayLitElement::Value(v) => scan_rvalue(v, classes, callees),
                    mir::ArrayLitElement::Spread(op) => scan_operand(op, classes, callees),
                }
            }
        }
        MirRvalue::NewArray { length, .. } => scan_operand(length, classes, callees),
        MirRvalue::IndexGet { array, index, .. } => {
            scan_operand(array, classes, callees);
            scan_operand(index, classes, callees);
        }
        MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            scan_operand(array, classes, callees);
            if let Some(s) = start {
                scan_operand(s, classes, callees);
            }
            if let Some(l) = length {
                scan_operand(l, classes, callees);
            }
        }
        MirRvalue::SpanFromStack { elements, .. } => {
            for e in elements {
                scan_operand(e, classes, callees);
            }
        }
        MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            scan_operand(span, classes, callees);
            scan_operand(start, classes, callees);
            if let Some(l) = length {
                scan_operand(l, classes, callees);
            }
        }
        MirRvalue::SpanFill { span, value, .. } => {
            scan_operand(span, classes, callees);
            scan_operand(value, classes, callees);
        }
        MirRvalue::SpanClear { span, .. } => scan_operand(span, classes, callees),
        MirRvalue::SpanCopyTo { src, dest, .. } => {
            scan_operand(src, classes, callees);
            scan_operand(dest, classes, callees);
        }
        MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            scan_operand(src, classes, callees);
            scan_operand(dest, classes, callees);
        }
        MirRvalue::SpanToArray { span, .. } => scan_operand(span, classes, callees),
        MirRvalue::SoaFieldGet { array, index, .. } => {
            scan_operand(array, classes, callees);
            scan_operand(index, classes, callees);
        }
        MirRvalue::LinqChain(chain) => scan_linq_chain(chain, classes, callees),
        MirRvalue::ExpressionTreeConst { .. } => {}
        MirRvalue::FnPtr { name } => {
            // 函数指针值：可能被间接调用 → 保守纳入被调边（过近似无害）。
            callees.insert(name.clone());
        }
        MirRvalue::IndirectCall { func, args } => {
            // 间接调用目标动态；被引用的函数指针值已在 `FnPtr`/`Closure` 处建边。
            scan_operand(func, classes, callees);
            for a in args {
                scan_operand(a, classes, callees);
            }
        }
        MirRvalue::Coalesce { left, right } => {
            scan_operand(left, classes, callees);
            scan_operand(right, classes, callees);
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            scan_operand(cond, classes, callees);
            scan_operand(then_val, classes, callees);
            scan_operand(else_val, classes, callees);
        }
        MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            scan_operand(receiver, classes, callees);
            scan_operand(default, classes, callees);
        }
        MirRvalue::NullCondMethod {
            receiver,
            method,
            args,
            receiver_type,
            impl_class,
            target_fn,
            is_virtual,
            default,
            ..
        } => {
            scan_method_call_targets(
                method,
                receiver_type,
                impl_class.as_deref(),
                target_fn.as_deref(),
                *is_virtual,
                callees,
                receiver,
                args,
                classes,
            );
            scan_operand(default, classes, callees);
        }
        MirRvalue::ForceDerefField { receiver, .. } => {
            scan_operand(receiver, classes, callees);
        }
        MirRvalue::ForceDerefMethod {
            receiver,
            method,
            args,
            receiver_type,
            impl_class,
            target_fn,
            is_virtual,
            ..
        } => scan_method_call_targets(
            method,
            receiver_type,
            impl_class.as_deref(),
            target_fn.as_deref(),
            *is_virtual,
            callees,
            receiver,
            args,
            classes,
        ),
        MirRvalue::Box { src, .. } => scan_operand(src, classes, callees),
        MirRvalue::Unbox { src, .. } => scan_operand(src, classes, callees),
        MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(p) = payload {
                scan_operand(p, classes, callees);
            }
        }
        MirRvalue::VariantTag { scrutinee, .. } => scan_operand(scrutinee, classes, callees),
        MirRvalue::VariantExtract { scrutinee, .. } => scan_operand(scrutinee, classes, callees),
    }
}

/// 方法调用（含 NullCond/ForceDeref 形式）的被调函数键解析：对齐 `attr.rs` 约定——
/// 虚调用 opaque（不合成边）；`target_fn` 命中用 link name；否则 `Class::Method`。
#[allow(clippy::too_many_arguments)]
fn scan_method_call_targets(
    method: &str,
    receiver_type: &str,
    impl_class: Option<&str>,
    target_fn: Option<&str>,
    is_virtual: bool,
    callees: &mut HashSet<String>,
    receiver: &MirOperand,
    args: &[MirOperand],
    classes: &mut HashSet<Ident>,
) {
    if !is_virtual {
        let key = target_fn
            .map(|t| t.to_string())
            .unwrap_or_else(|| format!("{}::{method}", impl_class.unwrap_or(receiver_type)));
        callees.insert(key);
    }
    scan_operand(receiver, classes, callees);
    for a in args {
        scan_operand(a, classes, callees);
    }
}

fn scan_operand(op: &MirOperand, classes: &mut HashSet<Ident>, callees: &mut HashSet<String>) {
    match op {
        MirOperand::StaticField { class, .. } => {
            classes.insert(class.as_str().into());
        }
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. }
        | MirOperand::UnboxString { object }
        | MirOperand::UnboxGeneric { object, .. } => {
            scan_operand(object, classes, callees);
        }
        MirOperand::Closure { fn_name, env } => {
            // 闭包值可能被间接调用 → 保守纳入被调边（其函数体在 `fns` 中独立扫描）。
            callees.insert(fn_name.clone());
            for (_, op) in env {
                scan_operand(op, classes, callees);
            }
        }
        MirOperand::Local(_)
        | MirOperand::ConstInt(_)
        | MirOperand::ConstFloat(_)
        | MirOperand::ConstString(_)
        | MirOperand::ConstBool(_)
        | MirOperand::AddrOf(_)
        | MirOperand::ConstNull
        | MirOperand::ConstDefault { .. }
        | MirOperand::FnPtr { .. }
        | MirOperand::TypeId { .. }
        | MirOperand::TypeInfoPtr { .. } => {}
    }
}

/// 把类型名转换为 `TypeId`（用于 LLVM 类型推导 / 泛型 mangle）。
///
/// 复用 typeck 的类型名约定——基元类型走预定义名，class/string 走 `Named`。
pub(super) fn type_id_from_name(name: &str) -> TypeId {
    match name {
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
        // 裸 `Action` ≡ `Func<void>`（对齐 typeck `lower_type`/`type_path_name`）。
        "Action" => TypeId::Func {
            params: Vec::new(),
            ret: Box::new(TypeId::Void),
        },
        other => TypeId::Named(other.into()),
    }
}

/// RFC 006 V3：递归收集静态字段初始化器引用的**静态字段宿主类**。
///
/// 遍历初始化器表达式树，遇到 `Expr::Field { receiver: Ident(类名), .. }` 且
/// receiver 为当前模块含静态字段的类/struct 时，将该类记入依赖集合（去重）。
/// 供类级依赖图的直接引用边使用。函数体穿透由 `analyze_static_init_deps` 另行完成。
pub(super) fn collect_static_init_deps(expr: &Expr, class_names: &[Ident], out: &mut Vec<Ident>) {
    match expr {
        Expr::Field { receiver, .. } => {
            if let Expr::Ident(receiver_name) = &receiver.node {
                if class_names.contains(receiver_name) && !out.contains(receiver_name) {
                    out.push(receiver_name.clone());
                }
            }
            collect_static_init_deps(&receiver.node, class_names, out);
        }
        Expr::New { args, obj_init, .. } => {
            for arg in args {
                collect_static_init_deps(&arg.node, class_names, out);
            }
            if let Some(inits) = obj_init {
                for (_, e) in inits {
                    collect_static_init_deps(&e.node, class_names, out);
                }
            }
        }
        Expr::Call { func, args, .. } => {
            collect_static_init_deps(&func.node, class_names, out);
            for arg in args {
                collect_static_init_deps(&arg.node, class_names, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_static_init_deps(&receiver.node, class_names, out);
            for arg in args {
                collect_static_init_deps(&arg.node, class_names, out);
            }
        }
        Expr::Unary { expr, .. } => collect_static_init_deps(&expr.node, class_names, out),
        Expr::Binary { left, right, .. } => {
            collect_static_init_deps(&left.node, class_names, out);
            collect_static_init_deps(&right.node, class_names, out);
        }
        Expr::Index {
            receiver, index, ..
        } => {
            collect_static_init_deps(&receiver.node, class_names, out);
            collect_static_init_deps(&index.node, class_names, out);
        }
        Expr::Cast { expr, .. } => collect_static_init_deps(&expr.node, class_names, out),
        Expr::Coalesce { left, right, .. } => {
            collect_static_init_deps(&left.node, class_names, out);
            collect_static_init_deps(&right.node, class_names, out);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            collect_static_init_deps(&cond.node, class_names, out);
            collect_static_init_deps(&then_branch.node, class_names, out);
            collect_static_init_deps(&else_branch.node, class_names, out);
        }
        Expr::NullCond { access, .. } => collect_static_init_deps(&access.node, class_names, out),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir::{BlockId, Linkage, MirBlock, MirStatement};

    fn sp<T>(node: T) -> ast::Spanned<T> {
        ast::Spanned::new(node, ast::Span::DUMMY)
    }

    fn empty_layouts() -> ProgramLayouts {
        ProgramLayouts {
            classes: IndexMap::new(),
            structs: IndexMap::new(),
            enums: Default::default(),
            enum_variants: Default::default(),
            interfaces: Default::default(),
            variants: Default::default(),
            static_fields: Vec::new(),
            observable_properties: Default::default(),
            type_full_names: Default::default(),
        }
    }

    fn mk_body(statements: Vec<MirStatement>) -> MirCfgBody {
        MirCfgBody {
            params: vec![],
            ret: TypeId::Int,
            param_count: 0,
            locals: Default::default(),
            entry: BlockId(0),
            blocks: IndexMap::from([(
                BlockId(0),
                MirBlock {
                    id: BlockId(0),
                    statements,
                    terminator: MirTerminator::Return(None),
                },
            )]),
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: Linkage::External,
            parallelize: false,
            loop_backedges: Default::default(),
            foreach_loops: Vec::new(),
            spill_set: Default::default(),
        }
    }

    /// `class C { static X _f = Register() }`，其中 `Register` 的函数体读写
    /// `Registry._byOwner`（`MirOperand::StaticField` + `StaticFieldSet`）。断言
    /// `Registry.__sinit` 排在 `C.__sinit` 之前（方案 B 的核心：穿透被调函数体）。
    #[test]
    fn callee_body_static_field_penetration() {
        let mut layouts = empty_layouts();
        layouts.static_fields = vec![
            StaticFieldLayout {
                class: "Registry".into(),
                field: "_byOwner".into(),
                ty: "int".into(),
                init: None,
                is_lazy: false,
            },
            StaticFieldLayout {
                class: "C".into(),
                field: "_f".into(),
                ty: "int".into(),
                init: Some(sp(Expr::Call {
                    func: Box::new(sp(Expr::Ident("Register".into()))),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                })),
                is_lazy: false,
            },
        ];

        let body = mk_body(vec![
            MirStatement::Assign {
                place: mir::LocalId(0),
                rvalue: MirRvalue::Use(MirOperand::StaticField {
                    class: "Registry".into(),
                    field: "_byOwner".into(),
                }),
            },
            MirStatement::StaticFieldSet {
                class: "Registry".into(),
                field: "_byOwner".into(),
                value: MirRvalue::Use(MirOperand::ConstNull),
            },
        ]);
        let fns: Vec<(String, MirCfgBody)> = vec![("Register".to_string(), body)];

        let by_class: IndexMap<Ident, Vec<&StaticFieldLayout>> = layouts
            .static_fields
            .iter()
            .map(|sf| (sf.class.clone(), vec![sf]))
            .collect();

        let result = analyze_static_init_deps(&by_class, &fns);
        assert!(
            result.warnings.is_empty(),
            "expected no warnings, got: {:?}",
            result.warnings
        );
        let deps_c = result
            .deps
            .get(&Ident::from("C"))
            .expect("C has deps entry");
        assert!(
            deps_c.contains(&"Registry".into()),
            "C must depend on Registry (via Register body), got: {deps_c:?}"
        );

        let order = topological_sort(&result.deps);
        let idx_reg = order
            .iter()
            .position(|c| c.as_str() == "Registry")
            .expect("Registry in order");
        let idx_c = order
            .iter()
            .position(|c| c.as_str() == "C")
            .expect("C in order");
        assert!(
            idx_reg < idx_c,
            "Registry must init before C, got order: {order:?}"
        );
    }

    /// 间接调用：`A._f = F1()`，`F1` 体内调用 `F2`，`F2` 读取 `Registry` 静态字段。
    /// 断言依赖沿 F1 → F2 传递（fixpoint 闭包）。
    #[test]
    fn transitive_indirect_callee_penetration() {
        let mut layouts = empty_layouts();
        layouts.static_fields = vec![
            StaticFieldLayout {
                class: "Registry".into(),
                field: "_byOwner".into(),
                ty: "int".into(),
                init: None,
                is_lazy: false,
            },
            StaticFieldLayout {
                class: "A".into(),
                field: "_f".into(),
                ty: "int".into(),
                init: Some(sp(Expr::Call {
                    func: Box::new(sp(Expr::Ident("F1".into()))),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                })),
                is_lazy: false,
            },
        ];

        let read_registry = |via: Option<&str>| {
            let mut stmts = Vec::new();
            if let Some(c) = via {
                stmts.push(MirStatement::Assign {
                    place: mir::LocalId(0),
                    rvalue: MirRvalue::Call {
                        func: c.to_string(),
                        args: vec![],
                    },
                });
            }
            stmts.push(MirStatement::Assign {
                place: mir::LocalId(1),
                rvalue: MirRvalue::Use(MirOperand::StaticField {
                    class: "Registry".into(),
                    field: "_byOwner".into(),
                }),
            });
            mk_body(stmts)
        };
        let fns: Vec<(String, MirCfgBody)> = vec![
            ("F2".to_string(), read_registry(None)),
            ("F1".to_string(), read_registry(Some("F2"))),
        ];

        let by_class: IndexMap<Ident, Vec<&StaticFieldLayout>> = layouts
            .static_fields
            .iter()
            .map(|sf| (sf.class.clone(), vec![sf]))
            .collect();

        let result = analyze_static_init_deps(&by_class, &fns);
        assert!(
            result.warnings.is_empty(),
            "expected no warnings, got: {:?}",
            result.warnings
        );
        let deps_a = result
            .deps
            .get(&Ident::from("A"))
            .expect("A has deps entry");
        assert!(
            deps_a.contains(&"Registry".into()),
            "A must depend on Registry transitively (A → F1 → F2), got: {deps_a:?}"
        );
    }

    /// 初始化序依赖环：`A` 初始化器调用 `F_ReadB`（读取 B 静态字段），`B` 初始化器
    /// 调用 `F_ReadA`（读取 A 静态字段）。断言产出 `arc-sinit-001` 环诊断，且拓扑序
    /// 不无限循环（环内按声明序回退）。
    #[test]
    fn init_order_cycle_produces_diagnostic() {
        let mut layouts = empty_layouts();
        layouts.static_fields = vec![
            StaticFieldLayout {
                class: "A".into(),
                field: "_a".into(),
                ty: "int".into(),
                init: Some(sp(Expr::Call {
                    func: Box::new(sp(Expr::Ident("F_ReadB".into()))),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                })),
                is_lazy: false,
            },
            StaticFieldLayout {
                class: "B".into(),
                field: "_b".into(),
                ty: "int".into(),
                init: Some(sp(Expr::Call {
                    func: Box::new(sp(Expr::Ident("F_ReadA".into()))),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                })),
                is_lazy: false,
            },
        ];

        let read = |class: &str| {
            mk_body(vec![MirStatement::Assign {
                place: mir::LocalId(0),
                rvalue: MirRvalue::Use(MirOperand::StaticField {
                    class: class.to_string(),
                    field: "_x".into(),
                }),
            }])
        };
        let fns: Vec<(String, MirCfgBody)> = vec![
            ("F_ReadB".to_string(), read("B")),
            ("F_ReadA".to_string(), read("A")),
        ];

        let by_class: IndexMap<Ident, Vec<&StaticFieldLayout>> = layouts
            .static_fields
            .iter()
            .map(|sf| (sf.class.clone(), vec![sf]))
            .collect();

        let result = analyze_static_init_deps(&by_class, &fns);
        let cycle_warning = result.warnings.iter().any(|w| {
            matches!(w, StaticInitDiagnostic::InitCycle { .. }) && w.code() == "arc-sinit-001"
        });
        assert!(
            cycle_warning,
            "expected arc-sinit-001 cycle diagnostic, got: {:?}",
            result.warnings
        );
        // 环成员结构化携带（A、B 均须在列）。
        let members: Option<&[Ident]> = result.warnings.iter().find_map(|w| match w {
            StaticInitDiagnostic::InitCycle { members } => Some(members.as_slice()),
            _ => None,
        });
        assert!(
            members.is_some_and(|m| m.contains(&"A".into()) && m.contains(&"B".into())),
            "cycle diagnostic must carry both members, got: {:?}",
            result.warnings
        );
        // 拓扑序必须包含全部类且不无限循环。
        let order = topological_sort(&result.deps);
        assert_eq!(order.len(), 2, "both cycle members must still be ordered");
    }

    /// 跨包/外部调用目标（`fns` 中无对应函数体）→ `arc-sinit-002` 保守降级诊断。
    #[test]
    fn unresolvable_callee_produces_diagnostic() {
        let sf = StaticFieldLayout {
            class: "C".into(),
            field: "_f".into(),
            ty: "int".into(),
            init: Some(sp(Expr::Call {
                func: Box::new(sp(Expr::Ident("ExternalFn".into()))),
                args: vec![],
                type_args: vec![],
                params_span: None,
            })),
            is_lazy: false,
        };
        let by_class: IndexMap<Ident, Vec<&StaticFieldLayout>> =
            IndexMap::from([("C".into(), vec![&sf])]);

        let result = analyze_static_init_deps(&by_class, &[]);
        let warn = result.warnings.iter().any(|w| {
            matches!(w, StaticInitDiagnostic::UnresolvedCallee { .. })
                && w.code() == "arc-sinit-002"
        });
        assert!(
            warn,
            "expected arc-sinit-002 diagnostic, got: {:?}",
            result.warnings
        );
        // 跨包符号结构化携带。
        let symbol = result.warnings.iter().find_map(|w| match w {
            StaticInitDiagnostic::UnresolvedCallee { symbol } => Some(symbol.as_str()),
            _ => None,
        });
        assert_eq!(
            symbol,
            Some("ExternalFn"),
            "symbol must be carried, got: {symbol:?}"
        );
    }
}
