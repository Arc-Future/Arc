//! LLVM function and call-site attributes (RFC 015 Phase B.7).
//!
//! Centralizes attribute strings so emitters don't sprinkle raw `nounwind`/
//! `norecurse`/`willreturn` literals across the codebase. Attributes are
//! chosen per function class:
//! - **Pure runtime helpers** (`rt_list_size`, `rt_str_length`, ...): `readonly`
//!   + `nounwind` + `willreturn`, enabling LLVM to CSE and hoist them.
//! - **Allocation/deallocation** (`malloc`, `rt_arc_inc/dec`, `rt_list_push`):
//!   `nounwind` only — they mutate state.
//! - **Throwing / opaque runtime** (`rt_throw`, sync user-callback `rt_*`):
//!   no `nounwind` for call-graph (see [`is_known_nounwind_external`]).
//! - **User functions**: module-level call-graph fixpoint (RFC 015 Phase B.7).
//!   A function is `nounwind` only when it has no local `Throw`/`TryCatch` and
//!   every call resolves to a known `nounwind` callee (module-defined or
//!   whitelisted `rt_*` / libc leaf). Virtual / interface / indirect /
//!   unknown externals stay opaque (`may_throw`) so unwinding through
//!   intermediate frames is never marked `nounwind` (avoids
//!   `STATUS_BAD_STACK` on Windows). Zero-cost EH (`invoke`/`landingpad`) is
//!   still deferred — see RFC 015 Phase B.8.
//!
//! The attribute set is emitted as a comma-separated suffix appended to the
//! `define` line by `emit_sync_function`.

use mir::{MirRvalue, MirStatement};
use std::collections::{HashMap, HashSet};

/// Attribute set for a user-defined function.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FnAttrs {
    pub nounwind: bool,
    pub norecurse: bool,
    pub willreturn: bool,
    pub readonly: bool,
}

impl FnAttrs {
    /// Render as a space-prefixed attribute string (empty when no attrs set).
    pub fn render(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.nounwind {
            parts.push("nounwind");
        }
        if self.norecurse {
            parts.push("norecurse");
        }
        if self.willreturn {
            parts.push("willreturn");
        }
        if self.readonly {
            parts.push("readonly");
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" {}", parts.join(" "))
        }
    }
}

/// Per-function summary used by the call-graph fixpoint.
#[derive(Clone, Debug, Default)]
struct FnThrowSummary {
    /// Local `Throw` / `TryCatch` (or nested region) without looking at callees.
    local_throw: bool,
    /// Windows zero-cost EH：`TryFinally` 会发射 `cleanuppad`，要求所在函数
    /// 附带 personality。无论 finally 体是否 nounwind，SEH 路径都需要
    /// `uwtable` + `__CxxFrameHandler3`（否则 clang 报
    /// "CleanupPadInst needs to be in a function with a personality"）。
    seh_finally: bool,
    /// Resolved static callees (module keys: free name / `Class::M` / `__ctor::…`).
    callees: Vec<String>,
    /// Virtual / iface / indirect / unresolved call — must assume may throw.
    opaque_call: bool,
}

/// Infer attributes for a user function from a precomputed nounwind map.
///
/// `nounwind_map` is produced by [`analyze_module_nounwind`]. Missing keys
/// default to **not** nounwind (safe under may-throw unknown externals).
pub fn infer_user_fn_attrs(name: &str, nounwind_map: &HashMap<String, bool>) -> FnAttrs {
    FnAttrs {
        nounwind: nounwind_map.get(name).copied().unwrap_or(false),
        ..Default::default()
    }
}

/// Module-level call-graph `nounwind` analysis (RFC 015 Phase B.7).
///
/// Returns `name → nounwind`. Seeds are functions with local throw/try or an
/// opaque call; may-throw then propagates to callers until fixpoint.
pub fn analyze_module_nounwind(fns: &[(String, mir::MirCfgBody)]) -> HashMap<String, bool> {
    analyze_module_nounwind_impl(fns, true)
}

/// 内部实现；`seh_force_may_throw` 控制 Windows SEH 路径下 `TryFinally` 是否
/// 强制 may-throw（cleanuppad 需要 personality）。Windows 恒为 true。
fn analyze_module_nounwind_impl(
    fns: &[(String, mir::MirCfgBody)],
    seh_force_may_throw: bool,
) -> HashMap<String, bool> {
    let defined: HashSet<&str> = fns.iter().map(|(n, _)| n.as_str()).collect();
    let summaries: Vec<(String, FnThrowSummary)> = fns
        .iter()
        .map(|(name, body)| (name.clone(), summarize_body(body, &defined)))
        .collect();

    let mut may_throw: HashSet<String> = HashSet::new();
    for (name, sum) in &summaries {
        if sum.local_throw || sum.opaque_call || (seh_force_may_throw && sum.seh_finally) {
            may_throw.insert(name.clone());
        }
    }

    // Propagate: if any callee may throw, caller may throw.
    let mut changed = true;
    while changed {
        changed = false;
        for (name, sum) in &summaries {
            if may_throw.contains(name) {
                continue;
            }
            if sum.callees.iter().any(|c| may_throw.contains(c)) {
                may_throw.insert(name.clone());
                changed = true;
            }
        }
    }

    let mut out = HashMap::with_capacity(summaries.len());
    for (name, _) in &summaries {
        out.insert(name.clone(), !may_throw.contains(name));
    }
    out
}

fn summarize_body(body: &mir::MirCfgBody, defined: &HashSet<&str>) -> FnThrowSummary {
    let mut sum = FnThrowSummary::default();
    for block in body.blocks.values() {
        for stmt in &block.statements {
            absorb_stmt(stmt, defined, &mut sum);
        }
        if matches!(block.terminator, mir::MirTerminator::Throw(_)) {
            sum.local_throw = true;
        }
    }
    sum
}

fn absorb_stmt(stmt: &MirStatement, defined: &HashSet<&str>, sum: &mut FnThrowSummary) {
    match stmt {
        MirStatement::Throw { .. } | MirStatement::TryCatch { .. } => {
            sum.local_throw = true;
        }
        MirStatement::TryFinally { body, finally } => {
            // Windows SEH：cleanuppad 无条件生成，函数须带 personality。
            sum.seh_finally = true;
            for s in body {
                absorb_stmt(s, defined, sum);
            }
            for s in finally {
                absorb_stmt(s, defined, sum);
            }
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                absorb_stmt(s, defined, sum);
            }
            for s in else_body {
                absorb_stmt(s, defined, sum);
            }
        }
        MirStatement::While { body, cond, .. } => {
            absorb_rvalue(cond, defined, sum);
            for s in body {
                absorb_stmt(s, defined, sum);
            }
        }
        MirStatement::LinqForeach { body, .. } => {
            for s in body {
                absorb_stmt(s, defined, sum);
            }
        }
        MirStatement::Assign { rvalue, .. } => absorb_rvalue(rvalue, defined, sum),
        MirStatement::Return(Some(rv)) => absorb_rvalue(rv, defined, sum),
        MirStatement::FieldSet { value, .. }
        | MirStatement::StaticFieldSet { value, .. }
        | MirStatement::IndexSet { value, .. } => {
            absorb_rvalue(value, defined, sum);
        }
        MirStatement::Await { task, .. } => absorb_rvalue(task, defined, sum),
        _ => {}
    }
}

fn absorb_rvalue(rv: &MirRvalue, defined: &HashSet<&str>, sum: &mut FnThrowSummary) {
    match rv {
        MirRvalue::Call { func, .. } => resolve_callee(func, defined, sum),
        MirRvalue::MethodCall {
            target_fn,
            is_virtual,
            receiver_type,
            method,
            impl_class,
            ..
        } => absorb_method_call(
            target_fn.as_deref(),
            *is_virtual,
            receiver_type,
            method,
            impl_class.as_deref(),
            defined,
            sum,
        ),
        MirRvalue::NullCondMethod {
            target_fn,
            is_virtual,
            receiver_type,
            method,
            impl_class,
            ..
        }
        | MirRvalue::ForceDerefMethod {
            target_fn,
            is_virtual,
            receiver_type,
            method,
            impl_class,
            ..
        } => absorb_method_call(
            target_fn.as_deref(),
            *is_virtual,
            receiver_type,
            method,
            impl_class.as_deref(),
            defined,
            sum,
        ),
        MirRvalue::New {
            class,
            args,
            ctor_params,
        } => {
            // ctor 重载 mangle：无参 `__ctor::Class`；有参 `__ctor::Class_<arity>`；
            // MIR 判定同参数量碰撞时（ctor_params 非空）按签名
            // `__ctor::Class_<arity>_<p0>...` 消歧（与 codegen/typeck 一致）。
            let ctor = if ctor_params.is_empty() {
                if args.is_empty() {
                    format!("__ctor::{class}")
                } else {
                    format!("__ctor::{class}_{}", args.len())
                }
            } else {
                format!(
                    "__ctor::{class}_{}_{}",
                    ctor_params.len(),
                    ctor_params.join("_")
                )
            };
            resolve_callee(&ctor, defined, sum);
        }
        MirRvalue::IndirectCall { .. } => {
            sum.opaque_call = true;
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for e in elements {
                if let mir::ArrayLitElement::Value(inner) = e {
                    absorb_rvalue(inner, defined, sum);
                }
            }
        }
        // LinqChain / ExpressionTreeConst are not direct throw sites; nested
        // calls appear as separate MethodCall/Call nodes when lowered.
        _ => {}
    }
}

fn absorb_method_call(
    target_fn: Option<&str>,
    is_virtual: bool,
    receiver_type: &str,
    method: &str,
    impl_class: Option<&str>,
    defined: &HashSet<&str>,
    sum: &mut FnThrowSummary,
) {
    // Virtual / override dispatch: any override may throw; must not mark the
    // intermediate frame nounwind (unwinding through it → BAD_STACK).
    if is_virtual {
        sum.opaque_call = true;
        return;
    }
    let class = impl_class.unwrap_or(receiver_type);
    let key = format!("{class}::{method}");
    // Facade methods are desugared inline by codegen (try_emit_*), not a real call
    // to `target_fn`——`target_fn` points at an empty nounwind stub. Classify via
    // `builtin_facade_call_nounwind` first, so facade methods desugared to throwing
    // code (e.g. CancellationToken::ThrowIfCancellationRequested → rt_throw) are
    // treated as may-throw, not resolved to the nounwind stub.
    if typeck::is_builtin_facade(class) && !builtin_facade_call_nounwind(&key) {
        sum.opaque_call = true;
        return;
    }
    if let Some(tfn) = target_fn {
        resolve_callee(tfn, defined, sum);
        return;
    }
    resolve_callee(&key, defined, sum);
}

fn resolve_callee(name: &str, defined: &HashSet<&str>, sum: &mut FnThrowSummary) {
    if defined.contains(name) {
        sum.callees.push(name.to_string());
    } else if is_known_nounwind_external(name) || builtin_facade_call_nounwind(name) {
        // Known nounwind leaf (`rt_*` whitelist / libc / facade lowering) —
        // no may-throw edge.
    } else {
        // Unknown external / FFI / may-throw `rt_*` — conservative under sjlj.
        sum.opaque_call = true;
    }
}

/// Strip optional `module.` prefix used by some MIR builtin calls
/// (e.g. `rt_resources.rt_os_now_ticks` → `rt_os_now_ticks`).
fn external_base_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// Runtime symbols that must **not** be treated as nounwind callees.
///
/// Closed audit (2026-07-28): among `crates/runtime`, only `rt_throw` raises
/// natively (unwinds). Additional entries are **same-stack user-callback**
/// invokers — a throw inside the callback would unwind through the Arc caller
/// frame, so marking that frame `nounwind` would recreate `STATUS_BAD_STACK`.
///
/// When adding a new `rt_*` that unwinds or sync-calls arbitrary user code,
/// append it here (or keep the call opaque). Documented future risk:
/// `rt_qif_try_run` Phase B3 setjmp capture.
const RT_MAY_THROW: &[&str] = &[
    // EH (native raise)
    "rt_throw",
    // List predicate / comparer callbacks (same-stack)
    "rt_list_find_get",
    "rt_list_find_all",
    "rt_list_exists",
    "rt_list_find_index",
    "rt_list_find_last_index",
    "rt_list_true_for_all",
    "rt_list_last_index_of",
    "rt_list_for_each",
    "rt_list_remove_all",
    "rt_list_sort",
    "rt_list_binary_search_cmp",
    // Array predicate callbacks (same-stack; int[] Stable)
    "rt_array_exists",
    "rt_array_find_int",
    "rt_array_find_last_int",
    "rt_array_find_index",
    "rt_array_find_last_index",
    "rt_array_true_for_all",
    "rt_array_for_each",
    "rt_array_sort_int",
    "rt_array_binary_search_int",
    "rt_array_find_all_int",
    "rt_array_convert_all_int",
    // Parallel sync bodies
    "rt_parallel_for",
    "rt_parallel_foreach",
    // QIF sync runners (call user test fn; future setjmp)
    "rt_qif_try_run",
    "rt_qif_run_all",
    // CTS may fire callback synchronously
    "rt_cts_register",
    "rt_cts_register_lf",
    "rt_cts_node_try_fire",
    // Concurrent dict factories (sync)
    "rt_concurrent_dict_get_or_add",
    "rt_concurrent_dict_get_or_add_val",
    "rt_concurrent_dict_add_or_update_aa",
    "rt_concurrent_dict_add_or_update_pf",
];

/// Known nounwind external leaves for call-graph analysis (B.7 deepen).
///
/// - `rt_*` not listed in [`RT_MAY_THROW`] → nounwind (incl. `rt_get_exception`/
///   `rt_panic*`: no unwind out of the callee;
///   `TryCatch` still seeds may-throw via MIR `local_throw`).
/// - Common libc helpers emitted as MIR/`Call` targets → nounwind.
/// - Everything else (native FFI, unknown symbols) → not nounwind.
pub fn is_known_nounwind_external(name: &str) -> bool {
    let base = external_base_name(name);
    if matches!(base, "malloc" | "free" | "memcpy" | "memcmp" | "strlen") {
        return true;
    }
    if base.starts_with("llvm.") {
        // Intrinsics used as call targets (e.g. memcpy) — never Arc EH.
        return true;
    }
    if base.starts_with("rt_") {
        return !RT_MAY_THROW.contains(&base);
    }
    false
}

/// Whether a builtin facade `Class.method` / `Class::method` call is a
/// known-nounwind leaf.
///
/// codegen intercepts facade methods (typeck facade classes) and lowers them
/// to `rt_*` ABI calls. Most of those leaves are non-throwing; only
/// callback-invoking facades may unwind through the Arc caller frame (mirror
/// of `RT_MAY_THROW`). Without this, `File.AppendAllText` etc. were treated
/// as opaque unknown externals, so wrapping user functions failed the
/// `nounwind` inference and cleanup funclets calling them were dropped by the
/// Windows EH backend (Milestone ⑦ fix).
fn builtin_facade_call_nounwind(name: &str) -> bool {
    let (class, method) = if let Some((c, m)) = name.split_once('.') {
        (c, m)
    } else if let Some((c, m)) = name.split_once("::") {
        (c, m)
    } else {
        return false;
    };
    if !typeck::is_builtin_facade(class) {
        return false;
    }
    // Collection facades (`List_*`, `ConcurrentDictionary_*`, …) expose
    // callback-taking methods (ForEach/Sort/GetOrAdd) that map to the
    // `rt_list_*`/`rt_concurrent_dict_*` MAY_THROW set.
    if typeck::classify_builtin_facade(class) == Some(typeck::BuiltinFacadeKind::Collection) {
        return false;
    }
    let may_throw_pair = match class {
        "Array" => matches!(
            method,
            "Exists"
                | "Find"
                | "FindLast"
                | "FindIndex"
                | "FindLastIndex"
                | "TrueForAll"
                | "ForEach"
                | "Sort"
                | "BinarySearch"
                | "FindAll"
                | "ConvertAll"
        ),
        "Parallel" => matches!(method, "For" | "ForEach"),
        "CancellationTokenSource" => matches!(method, "Register"),
        // P1（async 取消异常通道）：ThrowIfCancellationRequested 反糖为
        // `throw new OperationCanceledException()`（emit_call.rs），经 rt_throw
        // 抛异常——须标 may-throw，否则含该调用的同步函数被误标 nounwind，
        // 异常穿过无 unwind 信息帧存在 BAD_STACK 风险。
        "CancellationToken" => matches!(method, "ThrowIfCancellationRequested"),
        _ => false,
    };
    !may_throw_pair
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::TypeId;
    use indexmap::IndexMap;
    use mir::{
        BlockId, LocalId, MirBlock, MirCfgBody, MirOperand, MirRvalue, MirStatement, MirTerminator,
    };

    fn body_with_stmts(stmts: Vec<MirStatement>) -> MirCfgBody {
        let mut blocks = IndexMap::new();
        blocks.insert(
            BlockId(0),
            MirBlock {
                id: BlockId(0),
                statements: stmts,
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: TypeId::Void,
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

    fn leaf_body() -> MirCfgBody {
        body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
        }])
    }

    #[test]
    fn leaf_without_calls_is_nounwind() {
        let fns = vec![("Leaf".into(), leaf_body())];
        let map = analyze_module_nounwind(&fns);
        assert!(infer_user_fn_attrs("Leaf", &map).nounwind);
    }

    #[test]
    fn method_call_virtual_is_not_nounwind() {
        let body = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(LocalId(1)),
                method: "EvalInt".into(),
                args: vec![],
                receiver_type: "Expression".into(),
                impl_class: None,
                target_fn: Some("ParameterExpression::EvalInt".into()),
                is_virtual: true,
                params: vec![],
            },
        }]);
        let fns = vec![("Caller".into(), body)];
        let map = analyze_module_nounwind(&fns);
        assert!(!infer_user_fn_attrs("Caller", &map).nounwind);
    }

    #[test]
    fn explicit_throw_is_not_nounwind() {
        let body = body_with_stmts(vec![MirStatement::Throw {
            value: MirRvalue::Use(MirOperand::ConstNull),
        }]);
        let fns = vec![("Throws".into(), body)];
        let map = analyze_module_nounwind(&fns);
        assert!(!infer_user_fn_attrs("Throws", &map).nounwind);
    }

    #[test]
    fn static_call_to_leaf_is_nounwind() {
        let caller = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "Leaf".into(),
                args: vec![],
            },
        }]);
        let fns = vec![("Leaf".into(), leaf_body()), ("Caller".into(), caller)];
        let map = analyze_module_nounwind(&fns);
        assert!(map["Leaf"]);
        assert!(
            map["Caller"],
            "caller of nounwind leaf must itself be nounwind"
        );
    }

    #[test]
    fn static_call_chain_to_thrower_is_not_nounwind() {
        let thrower = body_with_stmts(vec![MirStatement::Throw {
            value: MirRvalue::Use(MirOperand::ConstNull),
        }]);
        let mid = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "Thrower".into(),
                args: vec![],
            },
        }]);
        let top = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "Mid".into(),
                args: vec![],
            },
        }]);
        let fns = vec![
            ("Thrower".into(), thrower),
            ("Mid".into(), mid),
            ("Top".into(), top),
        ];
        let map = analyze_module_nounwind(&fns);
        assert!(!map["Thrower"]);
        assert!(
            !map["Mid"],
            "must not mark frame that calls thrower as nounwind"
        );
        assert!(
            !map["Top"],
            "must propagate may-throw through static call chain"
        );
    }

    #[test]
    fn unresolved_external_call_is_not_nounwind() {
        let body = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "NativeLib_unknown_sym".into(),
                args: vec![],
            },
        }]);
        let fns = vec![("UsesExtern".into(), body)];
        let map = analyze_module_nounwind(&fns);
        assert!(!map["UsesExtern"]);
    }

    #[test]
    fn whitelisted_rt_leaf_call_is_nounwind() {
        let body = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "rt_str_length".into(),
                args: vec![],
            },
        }]);
        let fns = vec![("UsesRt".into(), body)];
        let map = analyze_module_nounwind(&fns);
        assert!(
            map["UsesRt"],
            "caller of whitelisted rt_* leaf must be nounwind"
        );
    }

    #[test]
    fn whitelisted_rt_chain_is_nounwind() {
        let mid = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "rt_arc_inc".into(),
                args: vec![],
            },
        }]);
        let top = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "Mid".into(),
                args: vec![],
            },
        }]);
        let fns = vec![("Mid".into(), mid), ("Top".into(), top)];
        let map = analyze_module_nounwind(&fns);
        assert!(map["Mid"]);
        assert!(map["Top"]);
    }

    #[test]
    fn rt_throw_call_is_not_nounwind() {
        let body = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "rt_throw".into(),
                args: vec![],
            },
        }]);
        let fns = vec![("ThrowsViaRt".into(), body)];
        let map = analyze_module_nounwind(&fns);
        assert!(
            !map["ThrowsViaRt"],
            "rt_throw must never be treated as nounwind leaf"
        );
    }

    #[test]
    fn rt_callback_invoker_is_not_nounwind() {
        let body = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "rt_list_for_each".into(),
                args: vec![],
            },
        }]);
        let fns = vec![("UsesCallback".into(), body)];
        let map = analyze_module_nounwind(&fns);
        assert!(
            !map["UsesCallback"],
            "same-stack callback rt_* must stay may-throw"
        );
    }

    #[test]
    fn dotted_rt_name_is_whitelisted() {
        let body = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Call {
                func: "rt_resources.rt_os_now_ticks".into(),
                args: vec![],
            },
        }]);
        let fns = vec![("Ticks".into(), body)];
        let map = analyze_module_nounwind(&fns);
        assert!(map["Ticks"]);
    }

    #[test]
    fn resolved_method_to_leaf_is_nounwind() {
        let leaf = leaf_body();
        let caller = body_with_stmts(vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(LocalId(1)),
                method: "Get".into(),
                args: vec![],
                receiver_type: "Holder".into(),
                impl_class: Some("Holder".into()),
                target_fn: Some("Holder::Get".into()),
                is_virtual: false,
                params: vec![],
            },
        }]);
        let fns = vec![("Holder::Get".into(), leaf), ("Caller".into(), caller)];
        let map = analyze_module_nounwind(&fns);
        assert!(map["Holder::Get"]);
        assert!(map["Caller"]);
    }
}
