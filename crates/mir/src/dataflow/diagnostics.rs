//! NLL 诊断 P3 错误转译层（RFC 036 §2.3 / §2.7）。
//!
//! 将内部 `BorrowConflict` / `IteratorInvalidation` 转译为用户友好诊断：
//! - **不**暴露「borrow」「loan」「lifetime」术语（P3 约束）
//! - 转译为「引用」「作用域」「此处使用了引用」等友好措辞
//! - 诊断码与 §2.7 表对齐：`E_BORROW_CONFLICT` / `E_ITERATOR_INVALIDATION`
//!
//! **设计约束**（AGENTS.md / RFC 036）：
//! - 编译器诊断属编译器自身职责，非领域能力（不违反架构红线）
//! - 单文件单职责：本模块仅做术语转译；冲突检测在 `borrow.rs`，pass 编排在 `nll.rs`
//! - 不过度工程：仅覆盖 RFC 036 §2.7 表中 S4 范围内的诊断码
//!   （`E_USE_AFTER_MOVE` 由 HIR borrowck 管；`E_SPAN_OUTLIVES_BUFFER` /
//!   `E_LIFETIME_ANNOTATION` 后置）

use crate::dataflow::borrow::{BorrowConflict, ConflictKind, IteratorInvalidation, Loan, LoanKind};
use crate::types::{LocalId, MirCfgBody};
use ast::Ident;

use indexmap::IndexMap;

/// NLL 诊断码（RFC 036 §2.7）。
///
/// S4 范围仅包含 `BorrowConflict` 与 `IteratorInvalidation`；其余诊断码
/// （`E_USE_AFTER_MOVE` / `E_SPAN_OUTLIVES_BUFFER` / `E_LIFETIME_ANNOTATION`）
/// 由其他 pass 或后置实现处理，不在此枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NllDiagnosticCode {
    /// `E_BORROW_CONFLICT`：mutable 引用与其他引用冲突（RFC 036 §2.7）。
    BorrowConflict,
    /// `E_ITERATOR_INVALIDATION`：迭代器失效（RFC 036 §2.7）。
    IteratorInvalidation,
}

impl NllDiagnosticCode {
    /// RFC 036 §2.7 诊断码字符串（如 `E_BORROW_CONFLICT`）。
    pub fn code_str(self) -> &'static str {
        match self {
            NllDiagnosticCode::BorrowConflict => "E_BORROW_CONFLICT",
            NllDiagnosticCode::IteratorInvalidation => "E_ITERATOR_INVALIDATION",
        }
    }
}

/// NLL 诊断（P3 转译后的用户友好形式）。
///
/// 不含内部术语；`message` 字段直接面向最终用户。
#[derive(Clone, Debug)]
pub struct NllDiagnostic {
    pub code: NllDiagnosticCode,
    /// 用户友好消息（P3 转译后；不含「borrow」「loan」「lifetime」术语）。
    pub message: String,
    /// 所在函数名（MIR 函数名，用于定位）。
    pub fn_name: String,
    /// 涉及的 local id（供上层添加 span / 行号定位）。
    pub local: Option<LocalId>,
}

impl NllDiagnostic {
    /// 将 `BorrowConflict` 转译为 `NllDiagnostic`（P3 措辞）。
    ///
    /// 转译规则（RFC 036 §2.3）：
    /// - 「mutable borrow conflict」→「此处已被修改引用，无法同时读取 '{name}'」
    /// - 「shared vs mutable」→「此处已被修改引用，无法同时读取 '{name}'」
    ///
    /// `locals` 提供 `LocalId → (变量名, 类型)` 映射，用于将内部 local id
    /// 转译为用户可见的变量名；缺失时退化为 `L<id>` 形式。
    pub fn from_conflict(
        conflict: &BorrowConflict,
        loans: &IndexMap<crate::dataflow::LoanId, Loan>,
        fn_name: &str,
        locals: &IndexMap<LocalId, (Ident, crate::TypeId)>,
    ) -> Self {
        // P3：根据冲突类别选择友好措辞；不暴露「mutable / shared borrow」术语。
        let place_name = local_name(conflict.place, locals);
        let new_loan = &loans[&conflict.loan];
        let existing_loan = &loans[&conflict.conflicting];

        let action = match conflict.kind {
            // 新 mutable 与已有任意 loan 冲突——强调「修改」动作。
            ConflictKind::MutableVsExisting => "此处已被修改引用",
            // 新 shared 与已有 mutable 冲突——强调「读取」受「修改」阻碍。
            ConflictKind::SharedVsMutable => "此处已被修改引用",
        };

        // 二级提示：根据 loan kind 给出「修改」/「读取」语义（不出现 borrow 术语）。
        let (new_action, existing_action) = match (new_loan.kind, existing_loan.kind) {
            (LoanKind::Mutable, LoanKind::Mutable) => ("修改", "修改"),
            (LoanKind::Mutable, LoanKind::Shared) => ("修改", "读取"),
            (LoanKind::Shared, LoanKind::Mutable) => ("读取", "修改"),
            (LoanKind::Shared, LoanKind::Shared) => ("读取", "读取"),
        };

        let message = format!(
            "{action}，无法同时读取 '{place_name}'（此处尝试{new_action}，已有{existing_action}引用仍在作用域内）"
        );

        Self {
            code: NllDiagnosticCode::BorrowConflict,
            message,
            fn_name: fn_name.to_string(),
            local: Some(conflict.place),
        }
    }

    /// 将 `IteratorInvalidation` 转译为 `NllDiagnostic`（P3 措辞）。
    ///
    /// 转译规则（RFC 036 §2.3 / §2.7）：
    /// - 「iterator invalidation」→「容器 '{name}' 在迭代期间被修改；建议先 .ToList() 快照」
    pub fn from_invalidation(
        inv: &IteratorInvalidation,
        fn_name: &str,
        locals: &IndexMap<LocalId, (Ident, crate::TypeId)>,
    ) -> Self {
        let container_name = local_name(inv.container, locals);
        // P3：建议性措辞，引导用户用 .ToList() 快照规避。
        let message = format!(
            "容器 '{container_name}' 在迭代期间被修改（调用了 .{method}()）；建议先 .ToList() 快照再迭代",
            method = inv.method
        );

        Self {
            code: NllDiagnosticCode::IteratorInvalidation,
            message,
            fn_name: fn_name.to_string(),
            local: Some(inv.container),
        }
    }
}

/// 从 `locals` 表查 local 名；缺失时退化为 `L<id>`（内部标记，便于调试）。
fn local_name(id: LocalId, locals: &IndexMap<LocalId, (Ident, crate::TypeId)>) -> String {
    locals
        .get(&id)
        .map(|(n, _)| n.as_str().to_string())
        .unwrap_or_else(|| format!("L{}", id.0))
}

/// P3 术语禁令扫描（RFC 036 §2.3）：检查诊断消息是否暴露了内部术语。
///
/// 禁止出现的术语（用户面不可见）：
/// - 英文：`borrow` / `loan` / `lifetime`（大小写不敏感，覆盖 `Borrow`/`BORROW` 等）
/// - 中文：`借用` / `生命周期`
///
/// 命中任一术语返回 `Err(描述)`；全部通过返回 `Ok(())`。
/// 供 `translate_*` / `from_*` 输出做 P3 合规校验，亦供 CI 扫描复用。
///
/// **注意**：本函数扫描的是诊断 `message` 体（用户面措辞），**不**含诊断码
/// 前缀（如 `E_BORROW_CONFLICT` 是内部码，允许保留 `BORROW`；管线层拼接
/// `"{code}: {message}"` 时，扫描应仅作用于 `{message}` 部分）。
pub fn scan_for_forbidden_terms(msg: &str) -> Result<(), String> {
    let lower = msg.to_lowercase();
    // 英文术语大小写不敏感；中文术语不受 to_lowercase 影响。
    let forbidden: [&str; 5] = ["borrow", "loan", "lifetime", "借用", "生命周期"];
    for term in forbidden {
        if lower.contains(term) {
            return Err(format!(
                "NLL 诊断消息暴露了内部术语 '{term}'（RFC 036 §2.3 P3 禁令）：{msg}"
            ));
        }
    }
    Ok(())
}

/// 单函数 NLL 诊断构建：跑 `BorrowAnalysis` + 冲突检测 + 迭代器失效检测，
/// 转译为 `NllDiagnostic` 列表。
///
/// 供 `nll.rs` 的 `run_nll_check` 调用；本函数保持纯函数（无副作用），
/// 便于单测和复用。
pub fn build_diagnostics_for_fn(
    fn_name: &str,
    cfg: &MirCfgBody,
    closure_mutated: &std::collections::HashMap<String, std::collections::HashSet<String>>,
) -> Vec<NllDiagnostic> {
    let analysis = crate::dataflow::BorrowAnalysis::from_cfg(cfg, closure_mutated);
    let conflicts = crate::dataflow::borrow::detect_conflicts(&analysis, cfg);
    let invalidations = crate::dataflow::borrow::detect_iterator_invalidation(cfg);

    let mut diags = Vec::with_capacity(conflicts.len() + invalidations.len());
    for c in &conflicts {
        diags.push(NllDiagnostic::from_conflict(
            c,
            &analysis.loans,
            fn_name,
            &cfg.locals,
        ));
    }
    for inv in &invalidations {
        diags.push(NllDiagnostic::from_invalidation(inv, fn_name, &cfg.locals));
    }
    diags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataflow::borrow::LoanKind;
    use crate::dataflow::LoanId;
    use crate::types::BlockId;
    use indexmap::IndexMap;

    fn empty_locals() -> IndexMap<LocalId, (Ident, crate::TypeId)> {
        IndexMap::new()
    }

    fn make_loan(id: u32, kind: LoanKind, place: LocalId) -> Loan {
        Loan {
            id: LoanId(id),
            kind,
            place,
            origin: (BlockId(0), 0),
            reference_local: None,
            statement_scoped: false,
        }
    }

    fn make_loans_table(loans: &[Loan]) -> IndexMap<crate::dataflow::LoanId, Loan> {
        loans.iter().map(|l| (l.id, l.clone())).collect()
    }

    #[test]
    fn diagnostic_code_strings_match_rfc() {
        assert_eq!(
            NllDiagnosticCode::BorrowConflict.code_str(),
            "E_BORROW_CONFLICT"
        );
        assert_eq!(
            NllDiagnosticCode::IteratorInvalidation.code_str(),
            "E_ITERATOR_INVALIDATION"
        );
    }

    #[test]
    fn from_conflict_translates_mutable_vs_existing() {
        let l0 = LocalId(0);
        let loans = make_loans_table(&[
            make_loan(0, LoanKind::Shared, l0),
            make_loan(1, LoanKind::Mutable, l0),
        ]);
        let conflict = BorrowConflict {
            loan: LoanId(1),
            conflicting: LoanId(0),
            place: l0,
            kind: ConflictKind::MutableVsExisting,
            point: (BlockId(0), 1),
        };
        let diag = NllDiagnostic::from_conflict(&conflict, &loans, "Main", &empty_locals());
        assert_eq!(diag.code, NllDiagnosticCode::BorrowConflict);
        assert_eq!(diag.fn_name, "Main");
        assert_eq!(diag.local, Some(l0));
        // P3：不暴露 borrow / loan / lifetime 术语。
        assert!(!diag.message.contains("borrow"));
        assert!(!diag.message.contains("loan"));
        assert!(!diag.message.contains("lifetime"));
        // P3：包含友好措辞。
        assert!(diag.message.contains("修改引用"));
        assert!(diag.message.contains("L0")); // empty_locals → 退化命名
    }

    #[test]
    fn from_conflict_uses_local_name_when_available() {
        let l0 = LocalId(0);
        let loans = make_loans_table(&[
            make_loan(0, LoanKind::Shared, l0),
            make_loan(1, LoanKind::Mutable, l0),
        ]);
        let conflict = BorrowConflict {
            loan: LoanId(1),
            conflicting: LoanId(0),
            place: l0,
            kind: ConflictKind::MutableVsExisting,
            point: (BlockId(0), 1),
        };
        let mut locals = empty_locals();
        locals.insert(l0, (Ident::from("v"), crate::TypeId::Void));
        let diag = NllDiagnostic::from_conflict(&conflict, &loans, "Main", &locals);
        assert!(diag.message.contains("'v'"), "msg = {}", diag.message);
    }

    #[test]
    fn from_invalidation_translates_to_user_friendly() {
        let l0 = LocalId(0);
        let inv = IteratorInvalidation {
            container: l0,
            method: "Add".to_string(),
            point: (BlockId(0), 0),
        };
        let mut locals = empty_locals();
        locals.insert(l0, (Ident::from("v"), crate::TypeId::Void));
        let diag = NllDiagnostic::from_invalidation(&inv, "Main", &locals);
        assert_eq!(diag.code, NllDiagnosticCode::IteratorInvalidation);
        assert!(diag.message.contains("'v'"));
        assert!(diag.message.contains(".Add()"));
        assert!(diag.message.contains(".ToList()"));
        // P3：不暴露术语。
        assert!(!diag.message.contains("borrow"));
        assert!(!diag.message.contains("loan"));
    }

    #[test]
    fn from_invalidation_falls_back_to_lid_when_no_name() {
        let l5 = LocalId(5);
        let inv = IteratorInvalidation {
            container: l5,
            method: "Clear".to_string(),
            point: (BlockId(0), 0),
        };
        let diag = NllDiagnostic::from_invalidation(&inv, "Main", &empty_locals());
        assert!(diag.message.contains("L5"), "msg = {}", diag.message);
    }

    /// `scan_for_forbidden_terms` 对干净消息返回 Ok。
    #[test]
    fn scan_accepts_clean_message() {
        assert!(scan_for_forbidden_terms("此处已被修改引用，无法同时读取 'v'").is_ok());
        assert!(
            scan_for_forbidden_terms("容器 'v' 在迭代期间被修改；建议先 .ToList() 快照").is_ok()
        );
        assert!(scan_for_forbidden_terms("").is_ok());
    }

    /// `scan_for_forbidden_terms` 拦截英文禁词（大小写不敏感）。
    #[test]
    fn scan_rejects_english_forbidden_terms_case_insensitive() {
        assert!(scan_for_forbidden_terms("borrow conflict here").is_err());
        assert!(scan_for_forbidden_terms("Borrow checker failed").is_err());
        assert!(scan_for_forbidden_terms("BORROW active").is_err());
        assert!(scan_for_forbidden_terms("loan id 3").is_err());
        assert!(scan_for_forbidden_terms("LOAN still live").is_err());
        assert!(scan_for_forbidden_terms("lifetime 'a").is_err());
        assert!(scan_for_forbidden_terms("Lifetime too long").is_err());
    }

    /// `scan_for_forbidden_terms` 拦截中文禁词。
    #[test]
    fn scan_rejects_chinese_forbidden_terms() {
        assert!(scan_for_forbidden_terms("此处发生借用冲突").is_err());
        assert!(scan_for_forbidden_terms("生命周期超出作用域").is_err());
    }

    /// 现有 `from_conflict` / `from_invalidation` 输出必须通过 P3 扫描
    /// （RFC 036 §2.3 约束的回归保护）。
    #[test]
    fn from_conflict_and_invalidation_pass_p3_scan() {
        let l0 = LocalId(0);
        // conflict 诊断
        let loans = make_loans_table(&[
            make_loan(0, LoanKind::Shared, l0),
            make_loan(1, LoanKind::Mutable, l0),
        ]);
        let conflict = BorrowConflict {
            loan: LoanId(1),
            conflicting: LoanId(0),
            place: l0,
            kind: ConflictKind::MutableVsExisting,
            point: (BlockId(0), 1),
        };
        let mut locals = empty_locals();
        locals.insert(l0, (Ident::from("v"), crate::TypeId::Void));
        let conflict_diag = NllDiagnostic::from_conflict(&conflict, &loans, "Main", &locals);
        scan_for_forbidden_terms(&conflict_diag.message)
            .expect("from_conflict 输出必须通过 P3 禁词扫描");

        // invalidation 诊断
        let inv = IteratorInvalidation {
            container: l0,
            method: "Add".to_string(),
            point: (BlockId(0), 0),
        };
        let inv_diag = NllDiagnostic::from_invalidation(&inv, "Main", &locals);
        scan_for_forbidden_terms(&inv_diag.message)
            .expect("from_invalidation 输出必须通过 P3 禁词扫描");
    }
}
