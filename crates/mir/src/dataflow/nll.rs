//! NLL pass 入口（RFC 036 §2.1 / §2.5 / RFC 005 §2.3）。
//!
//! 在 MIR 上编排 `BorrowAnalysis` + 冲突检测 + 迭代器失效检测，
//! 通过 `diagnostics` 模块转译为 P3 用户友好诊断。
//!
//! **无条件启用**（RFC 036 §2.5 / RFC 005 §2.3）：NLL 恒启用，无逃生舱
//!（⑤ 已移除 CLI 开关）。`arc` crate 的 `prepare_compilation` 在 MIR
//! 生成后调用 `run_nll_check_module`，若返回非空诊断则编译失败。
//!
//! **适用范围**（RFC 036 §2.4）：struct / value 借用 + Span 借用；
//! **不**管 class ARC 循环（RFC 005 管）。

use crate::dataflow::diagnostics::{build_diagnostics_for_fn, NllDiagnostic};
use crate::types::{LocalId, MirCfgBody, MirOperand, MirStatement};

use indexmap::IndexMap;

/// 对单个函数运行 NLL 检查，返回诊断列表。
///
/// 流程（RFC 036 §2.1）：
/// 1. `BorrowAnalysis::from_cfg`：扫描 loan 创建点 + 计算 gen/kill 表。
/// 2. `detect_conflicts`：跑前向 dataflow，检测活跃 loan 冲突。
/// 3. `detect_iterator_invalidation`：扫描 `LinqForeach` body 中的修改方法调用。
/// 4. 通过 `diagnostics::build_diagnostics_for_fn` 转译为 P3 措辞。
///
/// 返回空 `Vec` 表示该函数无 NLL 违规。
pub fn run_nll_check(
    fn_name: &str,
    cfg: &MirCfgBody,
    closure_mutated: &std::collections::HashMap<String, std::collections::HashSet<String>>,
) -> Vec<NllDiagnostic> {
    build_diagnostics_for_fn(fn_name, cfg, closure_mutated)
}

/// 对整个模块运行 NLL 检查，返回所有诊断（按 MIR 函数顺序）。
///
/// 供 `arc::pipeline::prepare_compilation` 调用（NLL 无条件启用，RFC 005 §2.3）；
/// 非空诊断列表 → 编译失败。
///
/// 先构建「闭包 → 被修改捕获变量」映射，再逐函数检查。闭包体（lifted
/// `__lambda_rt_N`）若修改某个 ByRef 捕获变量，则宿主函数中该闭包存活期间
/// 对该捕获变量持有可变借用（RFC 036 §2.4 仅覆盖 struct/value/Span；此处补
/// 闭包捕获的 class 引用直改检）。映射供 `extract_loans` 在闭包创建点生成
/// 捕获 loan。
pub fn run_nll_check_module(mir_fns: &[(String, MirCfgBody)]) -> Vec<NllDiagnostic> {
    let closure_mutated = build_closure_mutation_map(mir_fns);
    let mut all = Vec::new();
    for (name, cfg) in mir_fns {
        all.extend(run_nll_check(name, cfg, &closure_mutated));
    }
    all
}

/// 扫描每个函数体，识别「闭包函数 → 被修改的捕获变量名」集合。
///
/// 对每个 lifted 闭包体（`captures` 非空），遍历其 MIR statements，收集
/// 修改信号的捕获变量名：
/// - `Add`/`Remove`/`Clear`/`Insert`/`Sort`/`AddRange`/`RemoveAt` 等 mutator
///   MethodCall，其 receiver 是捕获 local；
/// - `FieldSet { object, .. }` / `IndexSet { array, .. }` 的宿主是捕获 local。
///
/// 仅统计 `CaptureMode::ByRef`（ByValue 捕获是 env 拷贝，不直改外层 local）。
fn build_closure_mutation_map(
    mir_fns: &[(String, MirCfgBody)],
) -> std::collections::HashMap<String, std::collections::HashSet<String>> {
    use std::collections::{HashMap, HashSet};

    let mut map: HashMap<String, HashSet<String>> = HashMap::new();
    let mutators: [&str; 7] = [
        "Add", "Remove", "RemoveAt", "Clear", "Insert", "AddRange", "Sort",
    ];

    for (fn_name, cfg) in mir_fns {
        if cfg.captures.is_empty() {
            continue;
        }
        // 捕获 local id → 捕获变量名（仅 ByRef）。
        let mut byref_captured: IndexMap<LocalId, String> = IndexMap::new();
        for (lid, _, cap) in &cfg.captures {
            if cap.mode == ast::CaptureMode::ByRef {
                byref_captured.insert(*lid, cap.name.as_str().to_string());
            }
        }
        if byref_captured.is_empty() {
            continue;
        }
        let mut mutated: HashSet<String> = HashSet::new();
        for block in cfg.blocks.values() {
            for stmt in &block.statements {
                collect_capture_mutation(stmt, &byref_captured, &mutators, &mut mutated);
            }
        }
        if !mutated.is_empty() {
            map.insert(fn_name.clone(), mutated);
        }
    }
    map
}

/// 递归扫描语句，收集对捕获 local 的修改信号。
///
/// **FieldSet 不触发**（RFC 036 §2.4）：ByRef 捕获的都是引用类型
///（class/string/array/func/...，见 `capture_mode_for`），FieldSet 修改的是
/// 堆对象内部状态，不直改外层 local（局部变量仍指向同一对象）。
/// 仅 mutator MethodCall（Add/Remove/...）和 IndexSet 触发——前者修改容器
/// 内部状态可能引发迭代器失效，后者修改数组元素同样可能失效。
fn collect_capture_mutation(
    stmt: &MirStatement,
    byref_captured: &IndexMap<LocalId, String>,
    mutators: &[&str],
    out: &mut std::collections::HashSet<String>,
) {
    use crate::types::MirRvalue;
    match stmt {
        MirStatement::Assign {
            rvalue: MirRvalue::MethodCall {
                receiver, method, ..
            },
            ..
        } => {
            if mutators.contains(&method.as_str()) {
                if let Some(base) = receiver_base_local(receiver) {
                    if let Some(name) = byref_captured.get(&base) {
                        out.insert(name.clone());
                    }
                }
            }
        }
        MirStatement::IndexSet { array, .. } => {
            if let Some(base) = receiver_base_local(array) {
                if let Some(name) = byref_captured.get(&base) {
                    out.insert(name.clone());
                }
            }
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_capture_mutation(s, byref_captured, mutators, out);
            }
            for s in else_body {
                collect_capture_mutation(s, byref_captured, mutators, out);
            }
        }
        MirStatement::While { body, .. } | MirStatement::LinqForeach { body, .. } => {
            for s in body {
                collect_capture_mutation(s, byref_captured, mutators, out);
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_capture_mutation(s, byref_captured, mutators, out);
            }
            for s in catch_body {
                collect_capture_mutation(s, byref_captured, mutators, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_capture_mutation(s, byref_captured, mutators, out);
            }
            for s in finally {
                collect_capture_mutation(s, byref_captured, mutators, out);
            }
        }
        _ => {}
    }
}

/// 取 operand 的 base local（`Local` / `this`/`Field`/`Iface` 链的根）。
fn receiver_base_local(op: &MirOperand) -> Option<LocalId> {
    match op {
        MirOperand::Local(l) => Some(*l),
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. } => receiver_base_local(object),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataflow::diagnostics::NllDiagnosticCode;
    use crate::types::{
        BlockId, Linkage, LinqChain, LocalId, MirBlock, MirCfgBody, MirOperand, MirRvalue,
        MirStatement, MirTerminator,
    };
    use ast::BinOp;
    use indexmap::IndexMap;

    fn empty_closure_map() -> std::collections::HashMap<String, std::collections::HashSet<String>> {
        std::collections::HashMap::new()
    }

    /// 构造单块 CFG（含给定 statements + Return(None) terminator）。
    fn one_block_cfg(stmts: Vec<MirStatement>) -> MirCfgBody {
        let entry = BlockId(0);
        let mut blocks = IndexMap::new();
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: stmts,
                terminator: MirTerminator::Return(None),
            },
        );
        MirCfgBody {
            params: vec![],
            ret: typeck::TypeId::Void,
            param_count: 0,
            locals: IndexMap::new(),
            entry,
            blocks,
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
            spill_set: typeck::SpillSet::empty(),
        }
    }

    /// 简单冲突场景：两个 mutable AddrOf 同 place，无 last use 间隔 → 冲突。
    #[test]
    fn run_nll_check_detects_conflict() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let l2 = LocalId(2);
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Assign {
                place: l2,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Return(None),
        ]);
        let diags = run_nll_check("Main", &cfg, &empty_closure_map());
        assert_eq!(diags.len(), 1, "expected 1 conflict, got {:?}", diags);
        assert_eq!(diags[0].code, NllDiagnosticCode::BorrowConflict);
        assert_eq!(diags[0].fn_name, "Main");
        // P3：消息不含 borrow / loan / lifetime 术语。
        assert!(!diags[0].message.contains("borrow"));
        assert!(!diags[0].message.contains("loan"));
    }

    /// NLL last-use kill：`L1 = &L0; L2 = L1; L3 = &L0;` → 不冲突。
    #[test]
    fn run_nll_check_allows_reborrow_after_last_use() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let l2 = LocalId(2);
        let l3 = LocalId(3);
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Assign {
                place: l2,
                rvalue: MirRvalue::Use(MirOperand::Local(l1)),
            },
            MirStatement::Assign {
                place: l3,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Return(None),
        ]);
        let diags = run_nll_check("Main", &cfg, &empty_closure_map());
        assert!(
            diags.is_empty(),
            "S4: re-borrow after last use must NOT produce diagnostics, got {:?}",
            diags
        );
    }

    /// 迭代器失效：`foreach (var x in v) { v.Add(x); }` → 1 个 E_ITERATOR_INVALIDATION。
    #[test]
    fn run_nll_check_detects_iterator_invalidation() {
        let v = LocalId(0);
        let x = LocalId(1);
        let cfg = one_block_cfg(vec![MirStatement::LinqForeach {
            var: "x".into(),
            chain: LinqChain {
                source: MirOperand::Local(v),
                source_len: None,
                operators: vec![],
            },
            body: vec![MirStatement::Assign {
                place: LocalId(2),
                rvalue: MirRvalue::MethodCall {
                    receiver: MirOperand::Local(v),
                    method: "Add".into(),
                    args: vec![MirOperand::Local(x)],
                    receiver_type: "List".into(),
                    impl_class: None,
                    target_fn: None,
                    is_virtual: false,
                    params: vec![],
                },
            }],
        }]);
        let diags = run_nll_check("Main", &cfg, &empty_closure_map());
        assert_eq!(diags.len(), 1, "expected 1 invalidation, got {:?}", diags);
        assert_eq!(diags[0].code, NllDiagnosticCode::IteratorInvalidation);
        assert!(diags[0].message.contains(".ToList()"));
        // P3：不暴露术语。
        assert!(!diags[0].message.contains("borrow"));
        assert!(!diags[0].message.contains("loan"));
    }

    /// 多函数模块：`run_nll_check_module` 聚合所有诊断。
    #[test]
    fn run_nll_check_module_aggregates_diagnostics() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let l2 = LocalId(2);

        // fn A：有冲突
        let cfg_a = one_block_cfg(vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Assign {
                place: l2,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Return(None),
        ]);

        // fn B：无冲突（单 loan）
        let cfg_b = one_block_cfg(vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Return(None),
        ]);

        let mir_fns = vec![("A".to_string(), cfg_a), ("B".to_string(), cfg_b)];
        let diags = run_nll_check_module(&mir_fns);
        assert_eq!(diags.len(), 1, "only fn A should produce a diagnostic");
        assert_eq!(diags[0].fn_name, "A");
    }

    /// 无 loan 的函数 → 无诊断。
    #[test]
    fn run_nll_check_no_loans_no_diagnostics() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Binary {
                    op: BinOp::Add,
                    left: MirOperand::Local(l0),
                    right: MirOperand::ConstInt(1),
                },
            },
            MirStatement::Return(None),
        ]);
        let diags = run_nll_check("Main", &cfg, &empty_closure_map());
        assert!(
            diags.is_empty(),
            "no loans → no diagnostics, got {:?}",
            diags
        );
    }

    /// RFC 036 §2.4 捕获语义：ByRef 捕获的是引用类型（class/string/...），
    /// 闭包内 FieldSet 修改的是堆对象**内部状态**，不直改外层 local
    /// （局部变量仍指向同一对象）→ 不得作为闭包修改信号。
    /// 仅 mutator MethodCall（Add/Remove/...）与 IndexSet 触发（容器/数组
    /// 内部结构变化可能引发迭代器失效）。
    #[test]
    fn fieldset_on_byref_capture_is_not_mutation_signal() {
        let mut byref: IndexMap<LocalId, String> = IndexMap::new();
        byref.insert(LocalId(0), "counter".into());
        let mutators: [&str; 7] = [
            "Add", "Remove", "RemoveAt", "Clear", "Insert", "AddRange", "Sort",
        ];
        let mut mutated = std::collections::HashSet::new();

        // `counter.Value = 1`（FieldSet on class capture）→ 不触发。
        collect_capture_mutation(
            &MirStatement::FieldSet {
                object: MirOperand::Local(LocalId(0)),
                class: "AnimCounter".into(),
                field: "Value".into(),
                value: MirRvalue::Use(MirOperand::ConstInt(1)),
            },
            &byref,
            &mutators,
            &mut mutated,
        );
        assert!(
            mutated.is_empty(),
            "FieldSet on class capture must NOT be a mutation signal, got {mutated:?}"
        );

        // `v.Add(x)`（mutator MethodCall on capture）→ 仍触发。
        collect_capture_mutation(
            &MirStatement::Assign {
                place: LocalId(2),
                rvalue: MirRvalue::MethodCall {
                    receiver: MirOperand::Local(LocalId(0)),
                    method: "Add".into(),
                    args: vec![MirOperand::Local(LocalId(1))],
                    receiver_type: "List".into(),
                    impl_class: None,
                    target_fn: None,
                    is_virtual: false,
                    params: vec![],
                },
            },
            &byref,
            &mutators,
            &mut mutated,
        );
        assert!(
            mutated.contains("counter"),
            "mutator MethodCall on capture must still trigger, got {mutated:?}"
        );
    }

    /// `v[0] = x`（IndexSet on capture）→ 仍触发（数组元素修改可能失效迭代器）。
    #[test]
    fn indexset_on_byref_capture_is_mutation_signal() {
        let mut byref: IndexMap<LocalId, String> = IndexMap::new();
        byref.insert(LocalId(0), "arr".into());
        let mutators: [&str; 7] = [
            "Add", "Remove", "RemoveAt", "Clear", "Insert", "AddRange", "Sort",
        ];
        let mut mutated = std::collections::HashSet::new();

        collect_capture_mutation(
            &MirStatement::IndexSet {
                array: MirOperand::Local(LocalId(0)),
                index: MirOperand::ConstInt(0),
                elem_type: typeck::TypeId::Int,
                value: MirRvalue::Use(MirOperand::ConstInt(1)),
            },
            &byref,
            &mutators,
            &mut mutated,
        );
        assert!(
            mutated.contains("arr"),
            "IndexSet on capture must still trigger, got {mutated:?}"
        );
    }
}
