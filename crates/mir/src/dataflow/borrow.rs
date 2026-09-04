//! 借用分析（前向 dataflow）—— NLL 核心（RFC 036 §2.1）。
//!
//! - gen = loan 创建点（`&mut`/`&T` 借用；MIR 中为 `AddrOf` / Span 构造等）
//! - kill = loan 引用值的 last use 点（基于 `LiveVarAnalysis` + `reference_local`）
//! - 前向、并集 meet：`OUT = (IN − kill) ∪ gen`
//! - 边界（函数入口）= 空集
//!
//! **S4 精化**（相对 S3）：
//! 1. `Loan.reference_local`：按引用值本身的活跃性追踪 kill（非被借用 place）。
//! 2. `compute_loan_kills` 纳入 terminator 的 last use（`TERMINATOR_IDX` sentinel）。
//! 3. `extract_loans` 覆盖 FieldGet/IndexGet/SoaFieldGet 隐式共享读。
//! 4. `detect_conflicts` 调整为「先 kill 再检查 gen」语义。
//! 5. 新增 `detect_iterator_invalidation`。
//!
//! **适用范围**（RFC 036 §2.4）：struct / value 借用 + Span 借用；
//! **不**管 class ARC 循环（RFC 005 管）。

use std::collections::HashSet;

use crate::dataflow::live_var::{operand_locals, stmt_uses, terminator_uses, LiveVarAnalysis};
use crate::dataflow::{run_worklist, DataflowAnalysis, Direction};
use crate::types::*;

use indexmap::IndexMap;

/// terminator 在 `Point` 中的 idx sentinel（RFC 036 §2.1 S4）。
///
/// `Point = (BlockId, usize)`；terminator 没有传统 stmt idx，用 `usize::MAX`
/// 约定。`gen_at`/`kill_at`/`detect_conflicts` 均识别此 sentinel。
pub const TERMINATOR_IDX: usize = usize::MAX;

/// Loan 标识（函数内唯一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LoanId(pub u32);

/// 借用类别（mutable 排他 / shared 共享）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoanKind {
    Mutable,
    Shared,
}

/// 一个借用：作用于 `place`（被借用的 local），类别 `kind`，创建于 `origin`。
///
/// `reference_local`（S4）：loan 创建出的引用值所在的 local（如 `L1 = &L0`
/// 中的 `L1`；SpanFromArray 的结果 local）。按引用本身的活跃性追踪 kill。
/// 若 loan 来源不产生独立 local（罕见），保留 `None` 退化为 S3 行为。
#[derive(Clone, Debug)]
pub struct Loan {
    pub id: LoanId,
    pub kind: LoanKind,
    pub place: LocalId,
    /// 创建点：(block, stmt_idx)。
    pub origin: (BlockId, usize),
    /// S4：引用值所在 local（kill 追踪目标）。
    pub reference_local: Option<LocalId>,
    /// RFC 036 修复：调用边界借用（`ref`/`out` 实参）——语句级作用域，
    /// kill 点在 origin 语句的下一位置，不跨语句存活。
    pub statement_scoped: bool,
}

/// 程序点（语句位置）。terminator 位置用 `(block, TERMINATOR_IDX)`。
pub type Point = (BlockId, usize);

/// 借用冲突。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowConflict {
    /// 触发冲突的新 loan。
    pub loan: LoanId,
    /// 已活跃的冲突 loan。
    pub conflicting: LoanId,
    pub place: LocalId,
    pub kind: ConflictKind,
    /// 冲突发生点。
    pub point: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// 新 mutable 与已有 loan（任意类别）冲突。
    MutableVsExisting,
    /// 新 shared 与已有 mutable 冲突。
    SharedVsMutable,
}

/// 迭代器失效（RFC 036 §2.1 示例：`foreach (var x in v) { v.Add(x); }`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IteratorInvalidation {
    pub container: LocalId,
    pub method: String,
    pub point: Point,
}

/// 借用分析。Fact = `HashSet<LoanId>`（活跃 loan 集合）。
///
/// `gen` / `kill` 表在构造时预计算：
/// - `gen`：扫描 CFG 中 loan 创建模式（`AddrOf` / Span 构造 / 隐式共享读）。
/// - `kill`：基于 `LiveVarAnalysis` 结果，对每个 loan 的 `reference_local`
///   （或退化到 `place`）找 last use，含 terminator 位置。
pub struct BorrowAnalysis {
    /// 完整 loan 表（LoanId → Loan）。
    pub loans: IndexMap<LoanId, Loan>,
    /// 每个程序点的 gen（loan 创建）。
    pub gen: IndexMap<Point, Vec<LoanId>>,
    /// 每个程序点的 kill（loan last use）。
    pub kill: IndexMap<Point, Vec<LoanId>>,
}

impl BorrowAnalysis {
    /// 从 CFG 构造：提取 loans + 计算 gen/kill。
    pub fn from_cfg(
        cfg: &MirCfgBody,
        closure_mutated: &std::collections::HashMap<String, std::collections::HashSet<String>>,
    ) -> Self {
        let (loans, gen) = extract_loans(cfg, closure_mutated);
        let kill = compute_loan_kills(cfg, &loans);
        Self { loans, gen, kill }
    }

    pub fn gen_at(&self, block: BlockId, idx: usize) -> &[LoanId] {
        self.gen
            .get(&(block, idx))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn kill_at(&self, block: BlockId, idx: usize) -> &[LoanId] {
        self.kill
            .get(&(block, idx))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

impl DataflowAnalysis for BorrowAnalysis {
    type Fact = HashSet<LoanId>;

    fn direction(&self) -> Direction {
        Direction::Forward
    }

    fn boundary_fact(&self) -> Self::Fact {
        HashSet::new()
    }

    fn meet_identity(&self) -> Self::Fact {
        HashSet::new()
    }

    fn meet(&self, a: &Self::Fact, b: &Self::Fact) -> Self::Fact {
        a.union(b).copied().collect()
    }

    fn transfer_statement(
        &self,
        block: BlockId,
        idx: usize,
        _stmt: &MirStatement,
        in_fact: &Self::Fact,
    ) -> Self::Fact {
        // OUT = (IN − kill) ∪ gen
        let mut fact = in_fact.clone();
        for k in self.kill_at(block, idx) {
            fact.remove(k);
        }
        for g in self.gen_at(block, idx) {
            fact.insert(*g);
        }
        fact
    }

    fn transfer_terminator(
        &self,
        block: BlockId,
        _term: &MirTerminator,
        fact: &Self::Fact,
    ) -> Self::Fact {
        // S4：terminator 位置可能 kill（reference_local 的 last use）。
        // loan 不在终结符创建（gen_at(block, TERMINATOR_IDX) 实践中为空）。
        let mut fact = fact.clone();
        for k in self.kill_at(block, TERMINATOR_IDX) {
            fact.remove(k);
        }
        for g in self.gen_at(block, TERMINATOR_IDX) {
            fact.insert(*g);
        }
        fact
    }
}

/// 扫描 CFG 提取 loan 创建点。
///
/// **S4 覆盖的借用模式**：
/// - `MirOperand::AddrOf(l)`：`&local` 用于 `ref`/`out` 参数传递 → Mutable loan，
///   `reference_local = Some(stmt.place)`（赋值目标即引用值容器）。
/// - `MirRvalue::SpanFromArray { array, mutable, .. }`：Span 借用数组 buffer。
///   `mutable=true` → Mutable；`false` → Shared。`reference_local = Some(stmt.place)`。
/// - `MirRvalue::SpanSlice { span, mutable, .. }`：子 Span 借用，类别同上。
/// - `MirRvalue::FieldGet { object, .. }` / `IndexGet { array, .. }` /
///   `SoaFieldGet { array, .. }`：隐式共享读 → Shared loan on object/array，
///   `reference_local = Some(stmt.place)`（保守；FieldGet 等同 `&self.field`）。
pub fn extract_loans(
    cfg: &MirCfgBody,
    closure_mutated: &std::collections::HashMap<String, std::collections::HashSet<String>>,
) -> (IndexMap<LoanId, Loan>, IndexMap<Point, Vec<LoanId>>) {
    let mut loans: IndexMap<LoanId, Loan> = IndexMap::new();
    let mut gen: IndexMap<Point, Vec<LoanId>> = IndexMap::new();
    let mut next_id: u32 = 0;

    let mut create_loan = |place: LocalId,
                           kind: LoanKind,
                           point: Point,
                           reference_local: Option<LocalId>,
                           statement_scoped: bool| {
        let id = LoanId(next_id);
        next_id += 1;
        loans.insert(
            id,
            Loan {
                id,
                kind,
                place,
                origin: point,
                reference_local,
                statement_scoped,
            },
        );
        gen.entry(point).or_default().push(id);
    };

    for (&block_id, block) in cfg.blocks.iter() {
        for (idx, stmt) in block.statements.iter().enumerate() {
            let point = (block_id, idx);
            // S4：赋值目标作为 reference_local（引用值的容器）。
            let assign_target = match stmt {
                MirStatement::Assign { place, .. } => Some(*place),
                _ => None,
            };
            // 1) AddrOf 出现在 rvalue/operand 中 → Mutable loan。
            //    RFC 036 误报修复：嵌套在 `Call`/`MethodCall`/`New` 实参中的
            //    AddrOf（`EnsureInitialized(ref target, …)`）是**调用边界借用**——
            //    调用返回即结束，不跨语句。此前统一以 assign_target 为
            //    reference_local，导致结果 local（v1）仍活跃时二次 `ref target`
            //    误报 `E_BORROW_CONFLICT`。此类 loan 标记 statement_scoped，
            //    kill 点 = origin 语句的下一位置（见 compute_loan_kills）。
            for (place, scoped) in addrof_places_in_stmt(stmt) {
                let reference_local = if scoped { None } else { assign_target };
                create_loan(place, LoanKind::Mutable, point, reference_local, scoped);
            }
            // 2) Span 构造 rvalue → 按 mutable 标记决定类别。
            for (place, kind) in span_loans_in_stmt(stmt) {
                create_loan(place, kind, point, assign_target, false);
            }
            // 3) S4：FieldGet/IndexGet/SoaFieldGet 隐式共享读 → Shared loan。
            for place in implicit_read_places_in_stmt(stmt) {
                create_loan(place, LoanKind::Shared, point, assign_target, false);
            }
            // 4) 闭包捕获借用（RFC 036 补全）：闭包若修改某个 ByRef 捕获变量，
            //    则闭包创建点在捕获变量上持有 Mutable loan，存活期 = 闭包值的
            //    last use（`reference_local = assign_target`）。此借用使
            //    「闭包外迭代/读取容器，闭包内修改容器」这类穿越闭包边界的
            //    借用冲突得以被 `detect_conflicts` 捕获（此前漏检）。
            //    闭包作为调用实参（`ForEach(lambda)` 等）时 loan 标记
            //    `statement_scoped`——调用边界借用，与 AddrOf 修复一致。
            for (place, kind, scoped) in closure_capture_loans_in_stmt(stmt, closure_mutated) {
                let reference_local = if scoped { None } else { assign_target };
                create_loan(place, kind, point, reference_local, scoped);
            }
        }
    }

    (loans, gen)
}

/// 收集语句中 `AddrOf(l)` 引用的 local（loan 创建信号）。
/// 返回 `(local, statement_scoped)`：`statement_scoped=true` 表示该 AddrOf
/// 嵌套在 `Call`/`MethodCall`/`New`/`IndirectCall` 实参中（调用边界借用，
/// 调用返回即结束，不跨语句；见 RFC 036 误报修复）。
fn addrof_places_in_stmt(stmt: &MirStatement) -> Vec<(LocalId, bool)> {
    let mut v = Vec::new();
    match stmt {
        MirStatement::Assign { rvalue, .. } => collect_addrof_rvalue(rvalue, &mut v),
        MirStatement::Return(Some(rv)) => collect_addrof_rvalue(rv, &mut v),
        MirStatement::FieldSet { object, value, .. } => {
            collect_addrof_operand(object, &mut v);
            collect_addrof_rvalue(value, &mut v);
        }
        MirStatement::StaticFieldSet { value, .. } => collect_addrof_rvalue(value, &mut v),
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            collect_addrof_operand(array, &mut v);
            collect_addrof_operand(index, &mut v);
            collect_addrof_rvalue(value, &mut v);
        }
        MirStatement::Await { task, .. } => collect_addrof_rvalue(task, &mut v),
        MirStatement::Throw { value } => collect_addrof_rvalue(value, &mut v),
        _ => {}
    }
    v
}

fn collect_addrof_operand(op: &MirOperand, out: &mut Vec<(LocalId, bool)>) {
    match op {
        MirOperand::AddrOf(l) => out.push((*l, false)),
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. } => collect_addrof_operand(object, out),
        MirOperand::Closure { env, .. } => {
            for (_, o) in env {
                collect_addrof_operand(o, out);
            }
        }
        _ => {}
    }
}

/// 调用实参中的 AddrOf：标记 statement_scoped=true（调用返回即结束借用）。
fn collect_addrof_call_arg(op: &MirOperand, out: &mut Vec<(LocalId, bool)>) {
    match op {
        MirOperand::AddrOf(l) => out.push((*l, true)),
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. } => collect_addrof_call_arg(object, out),
        _ => {}
    }
}

fn collect_addrof_rvalue(rv: &MirRvalue, out: &mut Vec<(LocalId, bool)>) {
    match rv {
        MirRvalue::Use(op) => collect_addrof_operand(op, out),
        MirRvalue::Binary { left, right, .. } => {
            collect_addrof_operand(left, out);
            collect_addrof_operand(right, out);
        }
        MirRvalue::Call { args, .. } | MirRvalue::New { args, .. } => {
            for a in args {
                collect_addrof_call_arg(a, out);
            }
        }
        MirRvalue::FieldGet { object, .. } => collect_addrof_operand(object, out),
        MirRvalue::MethodCall { receiver, args, .. } => {
            collect_addrof_operand(receiver, out);
            for a in args {
                collect_addrof_call_arg(a, out);
            }
        }
        // Span 借用由 span_loans_in_stmt 单独处理（携带 mutable 标记）。
        MirRvalue::SpanFromArray { .. }
        | MirRvalue::SpanFromStack { .. }
        | MirRvalue::SpanSlice { .. } => {}
        // 其余变体递归其中嵌套的 operand。
        MirRvalue::MakeIface { object, .. }
        | MirRvalue::MakeIfaceDyn { object, .. }
        | MirRvalue::AdaptIface { object, .. } => collect_addrof_operand(object, out),
        MirRvalue::StructLit { fields, .. } => {
            for (_, o) in fields {
                collect_addrof_operand(o, out);
            }
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for e in elements {
                match e {
                    ArrayLitElement::Value(rv) => collect_addrof_rvalue(rv, out),
                    ArrayLitElement::Spread(op) => collect_addrof_operand(op, out),
                }
            }
        }
        MirRvalue::IndexGet { array, index, .. } => {
            collect_addrof_operand(array, out);
            collect_addrof_operand(index, out);
        }
        MirRvalue::SoaFieldGet { array, index, .. } => {
            collect_addrof_operand(array, out);
            collect_addrof_operand(index, out);
        }
        MirRvalue::LinqChain(chain) => collect_addrof_operand(&chain.source, out),
        MirRvalue::IndirectCall { func, args } => {
            collect_addrof_operand(func, out);
            for a in args {
                collect_addrof_call_arg(a, out);
            }
        }
        MirRvalue::Coalesce { left, right } => {
            collect_addrof_operand(left, out);
            collect_addrof_operand(right, out);
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            collect_addrof_operand(cond, out);
            collect_addrof_operand(then_val, out);
            collect_addrof_operand(else_val, out);
        }
        MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            collect_addrof_operand(receiver, out);
            collect_addrof_operand(default, out);
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            collect_addrof_operand(receiver, out);
            for a in args {
                collect_addrof_operand(a, out);
            }
            collect_addrof_operand(default, out);
        }
        MirRvalue::ForceDerefField { receiver, .. } => collect_addrof_operand(receiver, out),
        MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            collect_addrof_operand(receiver, out);
            for a in args {
                collect_addrof_operand(a, out);
            }
        }
        MirRvalue::Box { src, .. } | MirRvalue::Unbox { src, .. } => {
            collect_addrof_operand(src, out)
        }
        MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(p) = payload {
                collect_addrof_operand(p, out);
            }
        }
        MirRvalue::VariantTag { scrutinee, .. } | MirRvalue::VariantExtract { scrutinee, .. } => {
            collect_addrof_operand(scrutinee, out)
        }
        MirRvalue::SpanFill { span, value, .. } => {
            collect_addrof_operand(span, out);
            collect_addrof_operand(value, out);
        }
        MirRvalue::SpanClear { span, .. } => collect_addrof_operand(span, out),
        MirRvalue::SpanCopyTo { src, dest, .. } | MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            collect_addrof_operand(src, out);
            collect_addrof_operand(dest, out);
        }
        MirRvalue::SpanToArray { span, .. } => collect_addrof_operand(span, out),
        MirRvalue::ExpressionTreeConst { .. } | MirRvalue::FnPtr { .. } => {}
        MirRvalue::NewArray { length, .. } => collect_addrof_operand(length, out),
    }
}

/// 收集语句中 Span 构造 rvalue 产生的 (place, kind) 借用。
fn span_loans_in_stmt(stmt: &MirStatement) -> Vec<(LocalId, LoanKind)> {
    let mut v = Vec::new();
    let rv = match stmt {
        MirStatement::Assign { rvalue, .. } => rvalue,
        _ => return v,
    };
    match rv {
        MirRvalue::SpanFromArray { array, mutable, .. } => {
            for p in operand_locals(array) {
                v.push((
                    p,
                    if *mutable {
                        LoanKind::Mutable
                    } else {
                        LoanKind::Shared
                    },
                ));
            }
        }
        MirRvalue::SpanSlice { span, mutable, .. } => {
            for p in operand_locals(span) {
                v.push((
                    p,
                    if *mutable {
                        LoanKind::Mutable
                    } else {
                        LoanKind::Shared
                    },
                ));
            }
        }
        _ => {}
    }
    v
}

/// S4：收集 FieldGet/IndexGet/SoaFieldGet 中 object/array 的 local（隐式共享读）。
///
/// 亦覆盖 `get_Item` 容器元素读取（`v[i]` 索引器脱糖为 `get_Item` MethodCall，
/// 对容器而言是共享读）——否则「闭包持有可变借用 + 迭代读取容器」的穿越
/// 借用冲突无法被捕获（get_Item 不产生 loan，见闭包捕获借用补全）。
fn implicit_read_places_in_stmt(stmt: &MirStatement) -> Vec<LocalId> {
    let rv = match stmt {
        MirStatement::Assign { rvalue, .. } => rvalue,
        _ => return Vec::new(),
    };
    let mut v = Vec::new();
    match rv {
        MirRvalue::FieldGet { object, .. } => v.extend(operand_locals(object)),
        MirRvalue::IndexGet { array, .. } => v.extend(operand_locals(array)),
        MirRvalue::SoaFieldGet { array, .. } => v.extend(operand_locals(array)),
        MirRvalue::MethodCall {
            receiver, method, ..
        } if method == "get_Item" => {
            v.extend(operand_locals(receiver));
        }
        _ => {}
    }
    v
}

/// 收集语句中闭包修改捕获变量产生的 `(place, kind, statement_scoped)` 借用。
///
/// 对每个 `MirOperand::Closure { fn_name, env }`，若 `fn_name` 在
/// `closure_mutated` 映射中（即该闭包体修改了某捕获变量），则对每个被修改的
/// ByRef 捕获变量在闭包创建点生成 `Mutable` loan，作用于捕获变量的外层 local。
///
/// - 仅 ByRef 捕获（ByValue 是 env 拷贝，不直改外层 local；`closure_mutated`
///   构建时也只统计 ByRef，此处双保险）。
/// - 借用的存活期由上层 `reference_local = assign_target` 决定——闭包值
///   活跃期间捕获 loan 保持存活，闭包值 last use 后释放。
/// - 闭包位于**调用实参/接收者**中（如 `list.ForEach(x => …)`）时 loan 标记
///   `statement_scoped`（调用边界借用，与 AddrOf 的 RFC 036 修复一致）：
///   调用返回即结束，防止调用结果 place 的 last use 延伸至函数尾 → 误报
///   `E_BORROW_CONFLICT`（如 `ForEach 修改 merged 后 return merged`）。
fn closure_capture_loans_in_stmt(
    stmt: &MirStatement,
    closure_mutated: &std::collections::HashMap<String, std::collections::HashSet<String>>,
) -> Vec<(LocalId, LoanKind, bool)> {
    let mut closures: Vec<(String, &Vec<(ast::LambdaCapture, MirOperand)>, bool)> = Vec::new();
    collect_closures_in_stmt(stmt, &mut closures);

    let mut v = Vec::new();
    for (fn_name, env, in_call) in closures {
        let Some(mutated_names) = closure_mutated.get(&fn_name) else {
            continue;
        };
        for (cap, src) in env {
            if cap.mode == ast::CaptureMode::ByRef && mutated_names.contains(cap.name.as_str()) {
                if let Some(base) = receiver_base_local(src) {
                    v.push((base, LoanKind::Mutable, in_call));
                }
            }
        }
    }
    v
}

/// 递归收集语句中出现的闭包 `(fn_name, env)` 引用。
/// 第三元为 `in_call`（闭包是否位于调用实参/接收者中，见
/// `collect_closures_in_operand`）。
fn collect_closures_in_stmt<'a>(
    stmt: &'a MirStatement,
    out: &mut Vec<(String, &'a Vec<(ast::LambdaCapture, MirOperand)>, bool)>,
) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => collect_closures_in_rvalue(rvalue, false, out),
        MirStatement::Return(Some(rv)) => collect_closures_in_rvalue(rv, false, out),
        MirStatement::FieldSet { value, .. } => collect_closures_in_rvalue(value, false, out),
        MirStatement::IndexSet { value, .. } => collect_closures_in_rvalue(value, false, out),
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_closures_in_stmt(s, out);
            }
            for s in else_body {
                collect_closures_in_stmt(s, out);
            }
        }
        MirStatement::While { body, .. } | MirStatement::LinqForeach { body, .. } => {
            for s in body {
                collect_closures_in_stmt(s, out);
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_closures_in_stmt(s, out);
            }
            for s in catch_body {
                collect_closures_in_stmt(s, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_closures_in_stmt(s, out);
            }
            for s in finally {
                collect_closures_in_stmt(s, out);
            }
        }
        _ => {}
    }
}

/// 递归收集 rvalue 中嵌套 operand 的闭包引用（覆盖闭包作实参/字段等常见形态）。
///
/// `in_call`：rvalue 是否位于调用实参上下文中（嵌套的调用实参须强制标记，
/// 如 `f(outer(g(inner)))` 的内层闭包）。
fn collect_closures_in_rvalue<'a>(
    rv: &'a MirRvalue,
    in_call: bool,
    out: &mut Vec<(String, &'a Vec<(ast::LambdaCapture, MirOperand)>, bool)>,
) {
    match rv {
        MirRvalue::Use(op) => collect_closures_in_operand(op, in_call, out),
        MirRvalue::Binary { left, right, .. } => {
            collect_closures_in_operand(left, in_call, out);
            collect_closures_in_operand(right, in_call, out);
        }
        MirRvalue::Call { args, .. } | MirRvalue::New { args, .. } => {
            for a in args {
                collect_closures_in_operand(a, true, out);
            }
        }
        MirRvalue::MethodCall { receiver, args, .. } => {
            collect_closures_in_operand(receiver, true, out);
            for a in args {
                collect_closures_in_operand(a, true, out);
            }
        }
        MirRvalue::IndirectCall { func, args } => {
            collect_closures_in_operand(func, true, out);
            for a in args {
                collect_closures_in_operand(a, true, out);
            }
        }
        MirRvalue::FieldGet { object, .. } | MirRvalue::IndexGet { array: object, .. } => {
            collect_closures_in_operand(object, in_call, out)
        }
        MirRvalue::MakeIface { object, .. }
        | MirRvalue::MakeIfaceDyn { object, .. }
        | MirRvalue::AdaptIface { object, .. } => collect_closures_in_operand(object, in_call, out),
        MirRvalue::StructLit { fields, .. } => {
            for (_, o) in fields {
                collect_closures_in_operand(o, in_call, out);
            }
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            collect_closures_in_operand(cond, in_call, out);
            collect_closures_in_operand(then_val, in_call, out);
            collect_closures_in_operand(else_val, in_call, out);
        }
        MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            collect_closures_in_operand(receiver, in_call, out);
            collect_closures_in_operand(default, in_call, out);
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            collect_closures_in_operand(receiver, true, out);
            for a in args {
                collect_closures_in_operand(a, true, out);
            }
            collect_closures_in_operand(default, in_call, out);
        }
        MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            collect_closures_in_operand(receiver, true, out);
            for a in args {
                collect_closures_in_operand(a, true, out);
            }
        }
        _ => {}
    }
}

/// 收集 operand 中的闭包引用；`MirOperand::Closure` 为叶（env 内的捕获源
/// 是普通外层 local，不再嵌套闭包，故不递归 env）。
///
/// `in_call`：闭包是否位于**调用实参/接收者**（`Call`/`MethodCall`/`New`/
/// `IndirectCall` 等）中。调用边界借用——闭包值仅在调用期间存活，调用返回
/// 即结束。此类闭包修改捕获变量产生的 loan 须标记 `statement_scoped`
/// （与 AddrOf 的 RFC 036 调用边界修复一致），否则 closure loan 以调用
/// 结果 place 为 reference_local，其 last use 可能延伸到函数尾 → 误报
/// `E_BORROW_CONFLICT`（如 `list.ForEach(x => merged.Add(x)); return merged;`）。
fn collect_closures_in_operand<'a>(
    op: &'a MirOperand,
    in_call: bool,
    out: &mut Vec<(String, &'a Vec<(ast::LambdaCapture, MirOperand)>, bool)>,
) {
    match op {
        MirOperand::Closure { fn_name, env } => out.push((fn_name.clone(), env, in_call)),
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. } => {
            collect_closures_in_operand(object, in_call, out)
        }
        _ => {}
    }
}

/// 基于 LiveVar 计算每个 loan 的 kill 点（S4：reference_local 的 last use）。
///
/// **算法**（S4 精化）：
/// 1. 跑 LiveVar 得到每块的 IN/OUT（活跃 local 集合）。
/// 2. 对每个块：
///    a. 处理 terminator：若 tracked local 在 terminator 使用且不在块 OUT →
///    terminator 是 last use → kill at (block, TERMINATOR_IDX)。
///    b. 逆序遍历 statements：若 tracked local 在 stmt 使用且不在 stmt 后活跃 →
///    stmt i 是 last use → kill at (block, i)。
/// 3. tracked local = `loan.reference_local`（若 Some），否则退化到 `loan.place`（S3）。
/// 4. 仅 kill origin 在此点之前已创建的 loan。
fn compute_loan_kills(
    cfg: &MirCfgBody,
    loans: &IndexMap<LoanId, Loan>,
) -> IndexMap<Point, Vec<LoanId>> {
    let live = run_worklist(&LiveVarAnalysis, cfg);

    let mut kill: IndexMap<Point, Vec<LoanId>> = IndexMap::new();

    // RFC 036 修复：statement_scoped loan（调用边界借用）在 origin 语句的
    // 下一位置 kill——调用返回即结束借用，不跨语句存活。直接注入 kill 点。
    // 若该语句是块内**最后一条**（idx+1 == len），`(block, idx+1)` 既不在
    // statements 循环也不在 terminator（TERMINATOR_IDX）——kill 永不应用，
    // loan 跨块泄漏 → 误报 E_BORROW_CONFLICT（如 `list.ForEach(修改捕获)`
    // 是块末语句时，后续块读捕获变量被误拒）。归一化到 TERMINATOR_IDX。
    for (id, loan) in loans {
        if loan.statement_scoped {
            let (b, idx) = loan.origin;
            let stmt_count = cfg.blocks[&b].statements.len();
            let kill_point = if idx + 1 >= stmt_count {
                (b, TERMINATOR_IDX)
            } else {
                (b, idx + 1)
            };
            kill.entry(kill_point).or_default().push(*id);
        }
    }

    // 按 tracked local 分组 loan，便于按 local 查 kill。
    let mut loans_by_local: IndexMap<LocalId, Vec<LoanId>> = IndexMap::new();
    for (id, loan) in loans {
        let tracked = loan.reference_local.unwrap_or(loan.place);
        loans_by_local.entry(tracked).or_default().push(*id);
    }

    for (&block_id, block) in cfg.blocks.iter() {
        // 块出口活跃集 = LiveVar 的 out_fact（后向分析中 out_fact = 块出口）。
        let block_out = live[&block_id].out_fact.clone();

        // 1) 处理 terminator：terminator 入口活跃集 = transfer_terminator(block_out)。
        let term_uses: HashSet<LocalId> = terminator_uses(&block.terminator).into_iter().collect();
        for (local, loan_ids) in &loans_by_local {
            // tracked local 在 terminator 使用 且 不在块出口活跃 → terminator 是 last use。
            if term_uses.contains(local) && !block_out.contains(local) {
                for lid in loan_ids {
                    let origin = loans[lid].origin;
                    // 仅 kill 在 terminator 之前已创建的 loan（同块 stmt idx < TERMINATOR_IDX
                    // 恒真；跨块 origin.0 != block_id 也恒真）。
                    if origin.0 != block_id || origin.1 < TERMINATOR_IDX {
                        kill.entry((block_id, TERMINATOR_IDX))
                            .or_default()
                            .push(*lid);
                    }
                }
            }
        }
        // terminator 入口活跃集 = block_out ∪ term_uses（后向 transfer = out ∪ gen）。
        let mut after = block_out.clone();
        for u in &term_uses {
            after.insert(*u);
        }

        // 2) 逆序遍历 statements：after = 语句 S 的「出口活跃集」。
        for (i, stmt) in block.statements.iter().enumerate().rev() {
            let before = LiveVarAnalysis.transfer_statement(block_id, i, stmt, &after);
            let stmt_uses_set: HashSet<LocalId> = stmt_uses(stmt).into_iter().collect();
            for (local, loan_ids) in &loans_by_local {
                // tracked local 在 stmt 使用 且 不在 stmt 后活跃 → last use。
                if stmt_uses_set.contains(local) && !after.contains(local) {
                    for lid in loan_ids {
                        let origin = loans[lid].origin;
                        if origin.0 != block_id || origin.1 < i {
                            kill.entry((block_id, i)).or_default().push(*lid);
                        }
                    }
                }
            }
            after = before;
        }
    }

    // 去重（同一 local 多个 loan 可能重复 push）。
    for v in kill.values_mut() {
        v.sort();
        v.dedup();
    }
    kill
}

/// 检测借用冲突：在 loan 创建点，检查与已活跃 loan 是否冲突。
///
/// **S4 语义**（RFC 036 §2.1）：同一程序点「先 kill 再检查 gen」——
/// 若旧 loan 的引用在该点已死（last use 结束），新 loan 创建时旧 loan 已释放，不冲突。
/// 若旧 loan 的引用仍活跃（last use 在更后），新 loan 与之共存 → 冲突。
///
/// 规则：
/// - 新 Mutable loan：与 place 上任意已活跃 loan 冲突。
/// - 新 Shared loan：与 place 上已活跃 Mutable loan 冲突（允许多个 Shared）。
pub fn detect_conflicts(analysis: &BorrowAnalysis, cfg: &MirCfgBody) -> Vec<BorrowConflict> {
    let facts = run_worklist(analysis, cfg);
    let mut conflicts = Vec::new();

    for (&block_id, block) in cfg.blocks.iter() {
        // 前向遍历：维护块内当前活跃 loan 集合。
        let mut live = facts[&block_id].in_fact.clone();
        for (i, _stmt) in block.statements.iter().enumerate() {
            // S4：先应用 kill（引用 last use 在此点结束 → 已死）。
            for k in analysis.kill_at(block_id, i) {
                live.remove(k);
            }
            // 再检查 gen 冲突：新 loan 与仍活跃的旧 loan 冲突则记录。
            // 同点多个新 loan 逐个加入 live，使后续新 loan 能与前一个新 loan 冲突。
            for &new_loan in analysis.gen_at(block_id, i) {
                let new = &analysis.loans[&new_loan];
                for &existing in &live {
                    let ex = &analysis.loans[&existing];
                    if ex.place != new.place {
                        continue;
                    }
                    let conflict_kind = match (new.kind, ex.kind) {
                        (LoanKind::Mutable, _) => Some(ConflictKind::MutableVsExisting),
                        (LoanKind::Shared, LoanKind::Mutable) => {
                            Some(ConflictKind::SharedVsMutable)
                        }
                        (LoanKind::Shared, LoanKind::Shared) => None,
                    };
                    if let Some(kind) = conflict_kind {
                        conflicts.push(BorrowConflict {
                            loan: new_loan,
                            conflicting: existing,
                            place: new.place,
                            kind,
                            point: (block_id, i),
                        });
                    }
                }
                live.insert(new_loan);
            }
        }
        // terminator 位置：仅应用 kill（terminator 不产生 loan）。
        for k in analysis.kill_at(block_id, TERMINATOR_IDX) {
            live.remove(k);
        }
    }

    conflicts
}

/// 检测迭代器失效（RFC 036 §2.1）。
///
/// 语义契约：仅当**源级枚举**（`foreach` / LINQ 查询）存活期间被枚举容器被
/// 修改才报失效——
/// - `foreach (var x in v) { v.Add(x); }`：C# 枚举器版本检查会抛异常 → 报；
/// - `for (int i = 0; i < n; i++) { v[i]…; v.Add(…); }`：索引读（`get_Item`）
///   不持有枚举器，C# 合法 → 不报。
///
/// 枚举循环由 lowering 合成时写入的 `MirStatement::While::foreach_source`
/// 溯源标记（用户手写循环恒为 `None`），分两路检测：
/// 1. **展平循环**：`to_cfg` 将溯源透传至 `MirCfgBody::foreach_loops`
///    （`(header, 枚举容器)`）。对每个溯源循环求自然循环体（多个 backedge
///    ——含 `continue`——合并），体内（含内层循环：外层枚举存活期内被修改
///    同样失效）扫描对容器的 mutator 调用。
/// 2. **区域语句**：`LinqForeach`（容器取 `chain.source`）与因 try 内嵌
///    break/continue 而保留为嵌套语句的 `While { foreach_source: Some }`——
///    递归扫描块内语句定位。
///
/// 容器同一性按**完整位置路径**（base local + 字段链）判定：仅比 base local
/// 会把 `this._a` 与 `this._b` 误判为同一容器。
pub fn detect_iterator_invalidation(cfg: &MirCfgBody) -> Vec<IteratorInvalidation> {
    let mut result = Vec::new();
    let mutators: [&str; 7] = [
        "Add", "Remove", "RemoveAt", "Clear", "Insert", "AddRange", "Sort",
    ];

    // 1) 展平的溯源枚举循环。
    if !cfg.foreach_loops.is_empty() {
        let preds = build_predecessor_map(cfg);
        for (header, source) in &cfg.foreach_loops {
            let Some(container) = place_path(source) else {
                continue;
            };
            // 该 header 的自然循环体：汇合全部指向它的 backedge。
            let mut body: HashSet<BlockId> = HashSet::new();
            for &backedge_src in cfg.loop_backedges.iter() {
                let hits_header = matches!(
                    cfg.blocks.get(&backedge_src).map(|b| &b.terminator),
                    Some(MirTerminator::Goto(h)) if h == header
                );
                if hits_header {
                    body.extend(natural_loop_blocks(&preds, *header, backedge_src));
                }
            }
            if body.is_empty() {
                continue;
            }
            for &bb in &body {
                if let Some(block) = cfg.blocks.get(&bb) {
                    for (idx, s) in block.statements.iter().enumerate() {
                        scan_for_invalidation(s, &container, &mutators, (bb, idx), &mut result);
                    }
                }
            }
        }
    }

    // 2) 区域语句中的源级枚举（LinqForeach / region 保留的 foreach While）。
    for (&block_id, block) in cfg.blocks.iter() {
        for (idx, stmt) in block.statements.iter().enumerate() {
            scan_region_enumerations(stmt, &mutators, (block_id, idx), &mut result);
        }
    }

    result
}

/// 递归定位区域语句中的源级枚举，对其循环体做失效扫描。
///
/// `LinqForeach` 与携带 `foreach_source` 的 `While` 是枚举边界；用户手写
/// `While` 与 If/Try 等仅递归透传（其中可能嵌套枚举区域语句）。
fn scan_region_enumerations(
    stmt: &MirStatement,
    mutators: &[&str],
    point: Point,
    result: &mut Vec<IteratorInvalidation>,
) {
    let enumerated: Option<(&MirOperand, &Vec<MirStatement>)> = match stmt {
        MirStatement::LinqForeach { chain, body, .. } => Some((&chain.source, body)),
        MirStatement::While {
            body,
            foreach_source: Some(source),
            ..
        } => Some((source, body)),
        MirStatement::While {
            body,
            foreach_source: None,
            ..
        } => {
            for s in body {
                scan_region_enumerations(s, mutators, point, result);
            }
            return;
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body.iter().chain(else_body) {
                scan_region_enumerations(s, mutators, point, result);
            }
            return;
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body.iter().chain(catch_body) {
                scan_region_enumerations(s, mutators, point, result);
            }
            return;
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body.iter().chain(finally) {
                scan_region_enumerations(s, mutators, point, result);
            }
            return;
        }
        _ => return,
    };
    if let Some((source, body)) = enumerated {
        if let Some(container) = place_path(source) {
            for s in body {
                scan_for_invalidation(s, &container, mutators, point, result);
            }
        }
    }
}

/// 递归扫描嵌套语句中的 MethodCall，检测对 `container` 位置的 mutator 调用。
fn scan_for_invalidation(
    stmt: &MirStatement,
    container: &(LocalId, Vec<String>),
    mutators: &[&str],
    point: Point,
    result: &mut Vec<IteratorInvalidation>,
) {
    match stmt {
        MirStatement::Assign {
            rvalue: MirRvalue::MethodCall {
                receiver, method, ..
            },
            ..
        } => {
            if mutators.contains(&method.as_str())
                && place_path(receiver).as_ref() == Some(container)
            {
                result.push(IteratorInvalidation {
                    container: container.0,
                    method: method.clone(),
                    point,
                });
            }
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                scan_for_invalidation(s, container, mutators, point, result);
            }
            for s in else_body {
                scan_for_invalidation(s, container, mutators, point, result);
            }
        }
        MirStatement::While { body, .. } => {
            for s in body {
                scan_for_invalidation(s, container, mutators, point, result);
            }
        }
        MirStatement::LinqForeach { body, .. } => {
            for s in body {
                scan_for_invalidation(s, container, mutators, point, result);
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                scan_for_invalidation(s, container, mutators, point, result);
            }
            for s in catch_body {
                scan_for_invalidation(s, container, mutators, point, result);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                scan_for_invalidation(s, container, mutators, point, result);
            }
            for s in finally {
                scan_for_invalidation(s, container, mutators, point, result);
            }
        }
        _ => {}
    }
}

/// 提取 operand 的完整位置路径：`(base local, 字段链)`。
///
/// `v` → `(v, [])`；`this._map` → `(this, ["_map"])`。转型（Iface/UnboxIface）
/// 不改变对象身份，视作透明。调用结果/常量等非确定位置返回 `None`——
/// 无法证明同一性时宁可漏报不误报。
fn place_path(op: &MirOperand) -> Option<(LocalId, Vec<String>)> {
    match op {
        MirOperand::Local(l) => Some((*l, Vec::new())),
        MirOperand::Field { object, field, .. } => {
            let (base, mut path) = place_path(object)?;
            path.push(field.clone());
            Some((base, path))
        }
        MirOperand::Iface { object, .. } | MirOperand::UnboxIface { object, .. } => {
            place_path(object)
        }
        _ => None,
    }
}

/// 提取 receiver operand 链的 base local（如 `v` / `v.Field` → `v`）。
fn receiver_base_local(op: &MirOperand) -> Option<LocalId> {
    match op {
        MirOperand::Local(l) => Some(*l),
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. } => receiver_base_local(object),
        _ => None,
    }
}

/// 终结符的后继块集合。
fn block_successors(term: &MirTerminator) -> Vec<BlockId> {
    match term {
        MirTerminator::Goto(t) => vec![*t],
        MirTerminator::CondBr {
            then_bb, else_bb, ..
        } => vec![*then_bb, *else_bb],
        MirTerminator::Return(_) | MirTerminator::Throw(_) | MirTerminator::Unreachable => vec![],
    }
}

/// 构建前驱映射：block → 其前驱块列表。
fn build_predecessor_map(cfg: &MirCfgBody) -> IndexMap<BlockId, Vec<BlockId>> {
    let mut preds: IndexMap<BlockId, Vec<BlockId>> = IndexMap::new();
    for (&bb, block) in cfg.blocks.iter() {
        for succ in block_successors(&block.terminator) {
            preds.entry(succ).or_default().push(bb);
        }
    }
    preds
}

/// 自然循环块集合：能到达 `backedge_src` 且不经过 `header` 的所有块（含 `backedge_src`）。
///
/// 标准自然循环算法：从 `backedge_src` 逆向 BFS，跳过 `header`。
fn natural_loop_blocks(
    preds: &IndexMap<BlockId, Vec<BlockId>>,
    header: BlockId,
    backedge_src: BlockId,
) -> HashSet<BlockId> {
    let mut body = HashSet::new();
    body.insert(backedge_src);
    let mut worklist = vec![backedge_src];
    while let Some(bb) = worklist.pop() {
        for &pred in preds.get(&bb).map(|v| v.as_slice()).unwrap_or(&[]) {
            if pred != header && body.insert(pred) {
                worklist.push(pred);
            }
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataflow::run_worklist;
    use indexmap::IndexMap;

    fn empty_closure_map() -> std::collections::HashMap<String, std::collections::HashSet<String>> {
        std::collections::HashMap::new()
    }

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

    /// `L1 = &L0`（AddrOf 创建 Mutable loan on L0），无其他借用 → 无冲突。
    #[test]
    fn borrow_no_conflict_single_loan() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Return(None),
        ]);
        let analysis = BorrowAnalysis::from_cfg(&cfg, &empty_closure_map());
        let result = run_worklist(&analysis, &cfg);
        assert_eq!(analysis.loans.len(), 1, "exactly one AddrOf loan");
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert!(conflicts.is_empty(), "single loan must not conflict");
        let _ = result;
    }

    /// S4 NLL last-use kill：`L1 = &L0; L2 = L0 + 1; L3 = L1; return L2`。
    /// loan_0 的 reference_local = L1；L1 的 last use 在 stmt 2（`L3 = L1`）→
    /// kill loan_0 at stmt 2。单 loan 无冲突。
    #[test]
    fn borrow_nll_last_use_releases() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let l2 = LocalId(2);
        let l3 = LocalId(3);
        // L1 = &L0;        // gen loan_0；ref_local=L1
        // L2 = L0 + 1;     // 使用 L0（不影响 L1 的活跃性）
        // L3 = L1;         // L1 的 last use → kill loan_0 at stmt 2
        // return L2;       // terminator 使用 L2
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Assign {
                place: l2,
                rvalue: MirRvalue::Binary {
                    op: ast::BinOp::Add,
                    left: MirOperand::Local(l0),
                    right: MirOperand::ConstInt(1),
                },
            },
            MirStatement::Assign {
                place: l3,
                rvalue: MirRvalue::Use(MirOperand::Local(l1)),
            },
            MirStatement::Return(Some(MirRvalue::Use(MirOperand::Local(l2)))),
        ]);
        let analysis = BorrowAnalysis::from_cfg(&cfg, &empty_closure_map());
        assert_eq!(analysis.loans.len(), 1, "single AddrOf loan");
        assert_eq!(
            analysis.loans[&LoanId(0)].reference_local,
            Some(l1),
            "loan_0 reference_local must be L1"
        );
        // loan_0 应在 L1 的 last use（stmt idx 2）被 kill。
        let kill_at_s2 = analysis.kill_at(BlockId(0), 2);
        assert!(
            !kill_at_s2.is_empty(),
            "loan on L0 must be killed at L1's last use (stmt idx 2), got kill table {:?}",
            analysis.kill
        );
        assert!(kill_at_s2.contains(&LoanId(0)));
        // 单 loan → 无冲突。
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert!(conflicts.is_empty(), "single loan must not conflict");
    }

    /// 两个 mutable loan同时活跃 → 冲突。
    /// `L1 = &L0; L2 = &L0;`（L1 在两次借用间无 last use）→ 冲突。
    #[test]
    fn borrow_mutable_conflict() {
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
        let analysis = BorrowAnalysis::from_cfg(&cfg, &empty_closure_map());
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert_eq!(
            conflicts.len(),
            1,
            "two simultaneous mutable loans on L0 must conflict, got {:?}",
            conflicts
        );
        assert_eq!(conflicts[0].kind, ConflictKind::MutableVsExisting);
        assert_eq!(conflicts[0].place, l0);
    }

    /// 两个 Shared span 不冲突。
    #[test]
    fn borrow_two_shared_spans_no_conflict() {
        let l0 = LocalId(0);
        let s1 = LocalId(1);
        let s2 = LocalId(2);
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: l0,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
            },
            MirStatement::Assign {
                place: s1,
                rvalue: MirRvalue::SpanFromArray {
                    array: MirOperand::Local(l0),
                    start: None,
                    length: None,
                    mutable: false,
                },
            },
            MirStatement::Assign {
                place: s2,
                rvalue: MirRvalue::SpanFromArray {
                    array: MirOperand::Local(l0),
                    start: None,
                    length: None,
                    mutable: false,
                },
            },
            MirStatement::Return(None),
        ]);
        let analysis = BorrowAnalysis::from_cfg(&cfg, &empty_closure_map());
        assert_eq!(analysis.loans.len(), 2, "two span loans");
        assert_eq!(analysis.loans[&LoanId(0)].kind, LoanKind::Shared);
        assert_eq!(analysis.loans[&LoanId(1)].kind, LoanKind::Shared);
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert!(
            conflicts.is_empty(),
            "two shared spans on same place must NOT conflict, got {:?}",
            conflicts
        );
    }

    /// shared span 后接 mutable AddrOf → MutableVsExisting 触发。
    #[test]
    fn borrow_shared_then_mutable_conflicts() {
        let l0 = LocalId(0);
        let s1 = LocalId(1);
        let m1 = LocalId(2);
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: l0,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
            },
            MirStatement::Assign {
                place: s1,
                rvalue: MirRvalue::SpanFromArray {
                    array: MirOperand::Local(l0),
                    start: None,
                    length: None,
                    mutable: false,
                },
            },
            MirStatement::Assign {
                place: m1,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Return(None),
        ]);
        let analysis = BorrowAnalysis::from_cfg(&cfg, &empty_closure_map());
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert_eq!(
            conflicts.len(),
            1,
            "mutable loan while shared span active must conflict, got {:?}",
            conflicts
        );
        assert_eq!(conflicts[0].kind, ConflictKind::MutableVsExisting);
    }

    /// S4 精化：`L1 = &L0; L2 = L1; L3 = &L0;` —— L1 last use 在 stmt 1，
    /// stmt 2 创建新 mutable loan on L0 时旧 loan 已死 → 不冲突（S3 会误报，S4 通过）。
    #[test]
    fn borrow_nll_reborrow_after_last_use() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let l2 = LocalId(2);
        let l3 = LocalId(3);
        // L1 = &L0;     // gen loan_0 (mutable, ref_local=L1)
        // L2 = L1;      // L1 last use → kill loan_0 at stmt 1
        // L3 = &L0;     // gen loan_1 (mutable, ref_local=L3) — 旧 loan_0 已死，不冲突
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
        let analysis = BorrowAnalysis::from_cfg(&cfg, &empty_closure_map());
        assert_eq!(analysis.loans.len(), 2, "two AddrOf loans");
        assert_eq!(analysis.loans[&LoanId(0)].reference_local, Some(l1));
        assert_eq!(analysis.loans[&LoanId(1)].reference_local, Some(l3));
        // loan_0 应在 stmt 1（L1 的 last use）被 kill。
        assert!(
            analysis.kill_at(BlockId(0), 1).contains(&LoanId(0)),
            "loan_0 must be killed at stmt 1 (L1 last use), got kill {:?}",
            analysis.kill
        );
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert!(
            conflicts.is_empty(),
            "S4: re-borrow after last use must NOT conflict, got {:?}",
            conflicts
        );
    }

    /// S4：terminator 使用 loan 的引用 local → kill at terminator。
    /// `entry: L1 = &L0; goto cond`
    /// `cond:  CondBr(L1, body, exit)` — L1 在 terminator 使用 → kill loan_0 at terminator
    /// `body:  return` — body IN 不含 loan_0
    #[test]
    fn borrow_kill_at_terminator() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let entry = BlockId(0);
        let cond = BlockId(1);
        let body = BlockId(2);
        let exit = BlockId(3);
        let mut blocks = IndexMap::new();
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: vec![MirStatement::Assign {
                    place: l1,
                    rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
                }],
                terminator: MirTerminator::Goto(cond),
            },
        );
        blocks.insert(
            cond,
            MirBlock {
                id: cond,
                statements: vec![],
                terminator: MirTerminator::CondBr {
                    cond: MirOperand::Local(l1),
                    then_bb: body,
                    else_bb: exit,
                },
            },
        );
        blocks.insert(
            body,
            MirBlock {
                id: body,
                statements: vec![],
                terminator: MirTerminator::Return(None),
            },
        );
        blocks.insert(
            exit,
            MirBlock {
                id: exit,
                statements: vec![],
                terminator: MirTerminator::Return(None),
            },
        );
        let cfg = MirCfgBody {
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
        };
        let analysis = BorrowAnalysis::from_cfg(&cfg, &empty_closure_map());
        // loan_0 应在 cond 块的 terminator 位置被 kill。
        assert!(
            analysis.kill_at(cond, TERMINATOR_IDX).contains(&LoanId(0)),
            "S4: loan_0 must be killed at cond terminator (L1 last use), got kill {:?}",
            analysis.kill
        );
        let facts = run_worklist(&analysis, &cfg);
        let body_in = &facts[&body].in_fact;
        assert!(
            !body_in.contains(&LoanId(0)),
            "S4: loan killed at terminator; body IN must NOT contain loan_0, got {:?}",
            body_in
        );
    }

    /// S4：FieldGet 隐式共享读 → Shared loan on object。
    /// `L1 = L0.field;` 创建 shared loan on L0；若之后 `L2 = &L0`（mutable），冲突。
    #[test]
    fn borrow_field_get_implicit_shared() {
        let l0 = LocalId(0);
        let l1 = LocalId(1);
        let l2 = LocalId(2);
        // L1 = L0.field;   // shared loan on L0, ref_local=L1
        // L2 = &L0;        // mutable loan on L0 — 与 shared 冲突
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: l1,
                rvalue: MirRvalue::FieldGet {
                    object: MirOperand::Local(l0),
                    class: "C".into(),
                    field: "f".into(),
                },
            },
            MirStatement::Assign {
                place: l2,
                rvalue: MirRvalue::Use(MirOperand::AddrOf(l0)),
            },
            MirStatement::Return(None),
        ]);
        let analysis = BorrowAnalysis::from_cfg(&cfg, &empty_closure_map());
        // 第一个 loan 是 FieldGet 产生的 Shared loan。
        assert_eq!(analysis.loans.len(), 2, "FieldGet shared + AddrOf mutable");
        assert_eq!(analysis.loans[&LoanId(0)].kind, LoanKind::Shared);
        assert_eq!(analysis.loans[&LoanId(0)].place, l0);
        assert_eq!(analysis.loans[&LoanId(0)].reference_local, Some(l1));
        // 第二个 loan 是 AddrOf 产生的 Mutable loan。
        assert_eq!(analysis.loans[&LoanId(1)].kind, LoanKind::Mutable);
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert_eq!(
            conflicts.len(),
            1,
            "mutable loan while FieldGet shared active must conflict, got {:?}",
            conflicts
        );
        assert_eq!(conflicts[0].kind, ConflictKind::MutableVsExisting);
    }

    /// S4：迭代器失效检测。
    /// `foreach (var x in v) { v.Add(x); }` → 检测到 invalidation。
    #[test]
    fn iterator_invalidation_detected() {
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
        let invalidations = detect_iterator_invalidation(&cfg);
        assert_eq!(
            invalidations.len(),
            1,
            "iterator invalidation must be detected, got {:?}",
            invalidations
        );
        assert_eq!(invalidations[0].container, v);
        assert_eq!(invalidations[0].method, "Add");
    }

    /// S4：展平后的溯源枚举循环失效检测。
    ///
    /// `foreach (var x in v) { v.Add(x); }` 经 lower → While（`foreach_source =
    /// Some(v)`）→ `to_cfg` 展平为：
    /// ```text
    /// entry: count=v.get_Count(); idx=0; goto header
    /// header: CondBr(idx<count, body, exit)
    /// body:   elem=v.get_Item(idx); tmp=v.Add(elem); idx=idx+1; goto header [backedge]
    /// exit:   return
    /// ```
    /// `to_cfg` 记录 `foreach_loops = [(header, v)]`；自然循环体 = {body}；
    /// `v.Add(elem)` 触发 `E_ITERATOR_INVALIDATION`。
    #[test]
    fn iterator_invalidation_flattened_while() {
        let v = LocalId(0);
        let count = LocalId(2);
        let idx = LocalId(3);
        let elem = LocalId(4);
        let tmp = LocalId(5);

        let entry = BlockId(0);
        let header = BlockId(1);
        let body = BlockId(2);
        let exit = BlockId(3);

        let mut blocks = IndexMap::new();
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: vec![
                    MirStatement::Assign {
                        place: count,
                        rvalue: MirRvalue::MethodCall {
                            receiver: MirOperand::Local(v),
                            method: "get_Count".into(),
                            args: vec![],
                            receiver_type: "List".into(),
                            impl_class: None,
                            target_fn: None,
                            is_virtual: false,
                            params: vec![],
                        },
                    },
                    MirStatement::Assign {
                        place: idx,
                        rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
                    },
                ],
                terminator: MirTerminator::Goto(header),
            },
        );
        blocks.insert(
            header,
            MirBlock {
                id: header,
                statements: vec![],
                terminator: MirTerminator::CondBr {
                    cond: MirOperand::Local(idx),
                    then_bb: body,
                    else_bb: exit,
                },
            },
        );
        blocks.insert(
            body,
            MirBlock {
                id: body,
                statements: vec![
                    MirStatement::Assign {
                        place: elem,
                        rvalue: MirRvalue::MethodCall {
                            receiver: MirOperand::Local(v),
                            method: "get_Item".into(),
                            args: vec![MirOperand::Local(idx)],
                            receiver_type: "List".into(),
                            impl_class: None,
                            target_fn: None,
                            is_virtual: false,
                            params: vec![],
                        },
                    },
                    MirStatement::Assign {
                        place: tmp,
                        rvalue: MirRvalue::MethodCall {
                            receiver: MirOperand::Local(v),
                            method: "Add".into(),
                            args: vec![MirOperand::Local(elem)],
                            receiver_type: "List".into(),
                            impl_class: None,
                            target_fn: None,
                            is_virtual: false,
                            params: vec![],
                        },
                    },
                    MirStatement::Assign {
                        place: idx,
                        rvalue: MirRvalue::Binary {
                            op: ast::BinOp::Add,
                            left: MirOperand::Local(idx),
                            right: MirOperand::ConstInt(1),
                        },
                    },
                ],
                terminator: MirTerminator::Goto(header),
            },
        );
        blocks.insert(
            exit,
            MirBlock {
                id: exit,
                statements: vec![],
                terminator: MirTerminator::Return(None),
            },
        );

        let cfg = MirCfgBody {
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
            loop_backedges: [body].into_iter().collect(),
            foreach_loops: vec![(header, MirOperand::Local(v))],
            spill_set: typeck::SpillSet::empty(),
        };

        let invalidations = detect_iterator_invalidation(&cfg);
        assert_eq!(
            invalidations.len(),
            1,
            "flattened while-loop invalidation must be detected, got {:?}",
            invalidations
        );
        assert_eq!(invalidations[0].container, v);
        assert_eq!(invalidations[0].method, "Add");
    }

    /// 用户手写 `for`/`while` 循环内的「索引读 + mutator」是合法写法
    ///（索引读不持有枚举器）——无溯源标记（`foreach_loops` 为空）时
    /// **不得**凭 `get_Item` 启发式误报 `E_ITERATOR_INVALIDATION`。
    ///
    /// ```text
    /// for (int i = 0; i < n; i++) { var e = m[k]; m.Add(k, e); }
    /// ```
    #[test]
    fn iterator_invalidation_user_indexed_loop_no_false_positive() {
        let m = LocalId(0);
        let key = LocalId(1);
        let elem = LocalId(2);
        let tmp = LocalId(3);
        let i = LocalId(4);
        let n = LocalId(5);

        let entry = BlockId(0);
        let header = BlockId(1);
        let body = BlockId(2);
        let exit = BlockId(3);

        let mk_call = |place: LocalId, method: &str, recv: LocalId| MirStatement::Assign {
            place,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(recv),
                method: method.into(),
                args: vec![MirOperand::Local(key)],
                receiver_type: "Dictionary".into(),
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
        };

        let mut blocks = IndexMap::new();
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: vec![MirStatement::Assign {
                    place: i,
                    rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
                }],
                terminator: MirTerminator::Goto(header),
            },
        );
        blocks.insert(
            header,
            MirBlock {
                id: header,
                statements: vec![],
                terminator: MirTerminator::CondBr {
                    cond: MirOperand::ConstBool(true),
                    then_bb: body,
                    else_bb: exit,
                },
            },
        );
        blocks.insert(
            body,
            MirBlock {
                id: body,
                statements: vec![
                    mk_call(elem, "get_Item", m),
                    mk_call(tmp, "Add", m),
                    MirStatement::Assign {
                        place: i,
                        rvalue: MirRvalue::Binary {
                            op: ast::BinOp::Add,
                            left: MirOperand::Local(i),
                            right: MirOperand::Local(n),
                        },
                    },
                ],
                terminator: MirTerminator::Goto(header),
            },
        );
        blocks.insert(
            exit,
            MirBlock {
                id: exit,
                statements: vec![],
                terminator: MirTerminator::Return(None),
            },
        );

        let cfg = MirCfgBody {
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
            loop_backedges: [body].into_iter().collect(),
            // 用户循环：无溯源 → 检测必须静默。
            foreach_loops: Vec::new(),
            spill_set: typeck::SpillSet::empty(),
        };

        let invalidations = detect_iterator_invalidation(&cfg);
        assert!(
            invalidations.is_empty(),
            "indexed read + Add in a user loop is legal, got {:?}",
            invalidations
        );
    }

    /// 构造「外层 while 体内两个兄弟内层 while」的 CFG（CD-9 场景骨架）。
    ///
    /// ```text
    /// entry(0): Goto(h_outer)
    /// h_outer(1):  CondBr(cond, outer_body(2), outer_exit(8))
    /// outer_body(2):  [可选 v.get_Item] Goto(h_a)      ← 外层独占层
    /// h_a(3):      CondBr(cond, a_body(4), a_exit(5))
    /// a_body(4):   tmp = v.Add(x); Goto(h_a)            ← backedge_a
    /// a_exit(5):   Goto(h_b)
    /// h_b(6):      CondBr(cond, b_body(7), b_exit(9))
    /// b_body(7):   elem = v.get_Item(idx); Goto(h_b)    ← backedge_b
    /// b_exit(9):   Goto(outer_backedge(10))
    /// outer_backedge(10): Goto(h_outer)                 ← backedge_outer
    /// outer_exit(8): Return
    /// ```
    ///
    /// `outer_enum=true` 时外层为溯源枚举循环（`foreach_loops` 含
    /// `(h_outer, v)`，外层体含 lowered foreach 的 `v.get_Item` 取元素）——
    /// 用于验证「外层枚举 + 内层修改」真阳性方向。
    fn nested_sibling_loops_cfg(outer_enum: bool) -> MirCfgBody {
        let v = LocalId(0);
        let x = LocalId(1);
        let idx = LocalId(2);
        let tmp = LocalId(3);
        let elem = LocalId(4);

        let entry = BlockId(0);
        let h_outer = BlockId(1);
        let outer_body = BlockId(2);
        let h_a = BlockId(3);
        let a_body = BlockId(4);
        let a_exit = BlockId(5);
        let h_b = BlockId(6);
        let b_body = BlockId(7);
        let outer_exit = BlockId(8);
        let b_exit = BlockId(9);
        let outer_backedge = BlockId(10);

        let mk_block =
            |id: BlockId, statements: Vec<MirStatement>, terminator: MirTerminator| MirBlock {
                id,
                statements,
                terminator,
            };
        let cond_br = |then_bb: BlockId, else_bb: BlockId| MirTerminator::CondBr {
            cond: MirOperand::ConstBool(true),
            then_bb,
            else_bb,
        };
        let get_item = |place: LocalId, idx: LocalId| MirStatement::Assign {
            place,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(v),
                method: "get_Item".into(),
                args: vec![MirOperand::Local(idx)],
                receiver_type: "List".into(),
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
        };

        let mut blocks = IndexMap::new();
        blocks.insert(entry, mk_block(entry, vec![], MirTerminator::Goto(h_outer)));
        blocks.insert(
            h_outer,
            mk_block(h_outer, vec![], cond_br(outer_body, outer_exit)),
        );
        let outer_stmts = if outer_enum {
            vec![get_item(elem, idx)]
        } else {
            vec![]
        };
        blocks.insert(
            outer_body,
            mk_block(outer_body, outer_stmts, MirTerminator::Goto(h_a)),
        );
        blocks.insert(h_a, mk_block(h_a, vec![], cond_br(a_body, a_exit)));
        blocks.insert(
            a_body,
            mk_block(
                a_body,
                vec![MirStatement::Assign {
                    place: tmp,
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
                MirTerminator::Goto(h_a),
            ),
        );
        blocks.insert(a_exit, mk_block(a_exit, vec![], MirTerminator::Goto(h_b)));
        blocks.insert(h_b, mk_block(h_b, vec![], cond_br(b_body, b_exit)));
        blocks.insert(
            b_body,
            mk_block(b_body, vec![get_item(elem, idx)], MirTerminator::Goto(h_b)),
        );
        blocks.insert(
            b_exit,
            mk_block(b_exit, vec![], MirTerminator::Goto(outer_backedge)),
        );
        blocks.insert(
            outer_backedge,
            mk_block(outer_backedge, vec![], MirTerminator::Goto(h_outer)),
        );
        blocks.insert(
            outer_exit,
            mk_block(outer_exit, vec![], MirTerminator::Return(None)),
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
            loop_backedges: [a_body, b_body, outer_backedge].into_iter().collect(),
            foreach_loops: if outer_enum {
                vec![(h_outer, MirOperand::Local(v))]
            } else {
                Vec::new()
            },
            spill_set: typeck::SpillSet::empty(),
        }
    }

    /// CD-9 回归：外层体内两个兄弟内层 while 各自读写同一容器 `v`——
    /// 内层 A `v.Add`、内层 B `v.get_Item`，外层无溯源标记（用户循环）。
    /// 检测只信任溯源，**不**得凭内层索引读跨层关联误报
    /// `E_ITERATOR_INVALIDATION`。
    #[test]
    fn iterator_invalidation_nested_sibling_loops_no_false_positive() {
        let cfg = nested_sibling_loops_cfg(false);
        let invalidations = detect_iterator_invalidation(&cfg);
        assert!(
            invalidations.is_empty(),
            "sibling inner loops reading/writing the same container must not cross-link, got {:?}",
            invalidations
        );
    }

    /// 真阳性方向保持：外层循环为溯源枚举 `v`（`foreach_loops` 含
    /// `(h_outer, v)`），内层循环 `v.Add`——外层枚举存活期内被修改 →
    /// 必须报（外层覆盖内层的正确方向）。
    #[test]
    fn iterator_invalidation_outer_enumerates_inner_mutates() {
        let cfg = nested_sibling_loops_cfg(true);
        let invalidations = detect_iterator_invalidation(&cfg);
        assert_eq!(
            invalidations.len(),
            1,
            "outer enumeration invalidated by inner mutation must be detected, got {:?}",
            invalidations
        );
        assert_eq!(invalidations[0].container, LocalId(0)); // v
        assert_eq!(invalidations[0].method, "Add");
    }

    /// 闭包捕获借用（RFC 036 补全）：闭包按 ByRef 修改捕获变量 `v`，闭包值
    /// 存活期间 `v` 上持有 Mutable loan；随后 `v[0]` 隐式共享读（Shared loan）
    /// 与之冲突 → `E_BORROW_CONFLICT`。
    ///
    /// 模拟：
    /// ```text
    /// L_f = <Closure "lambda" env=[(v, ByRef, L_v)]>   // mutable loan on v, ref_local=L_f
    /// L_x = v[0]                                       // IndexGet shared loan on v → 冲突
    /// ```
    #[test]
    fn closure_capture_mutation_conflicts_with_read() {
        let v = LocalId(0);
        let f = LocalId(1);
        let x = LocalId(2);

        let cap = ast::LambdaCapture {
            name: ast::Ident::from("v"),
            ty: typeck::TypeId::Void,
            mode: ast::CaptureMode::ByRef,
        };
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: f,
                rvalue: MirRvalue::Use(MirOperand::Closure {
                    fn_name: "lambda".into(),
                    env: vec![(cap, MirOperand::Local(v))],
                }),
            },
            MirStatement::Assign {
                place: x,
                rvalue: MirRvalue::IndexGet {
                    array: MirOperand::Local(v),
                    index: MirOperand::ConstInt(0),
                    elem_type: typeck::TypeId::Void,
                },
            },
            MirStatement::Return(None),
        ]);

        let mut closure_mutated = empty_closure_map();
        closure_mutated.insert(
            "lambda".to_string(),
            ["v".to_string()].into_iter().collect(),
        );

        let analysis = BorrowAnalysis::from_cfg(&cfg, &closure_mutated);
        // 第一个 loan 是闭包产生的 Mutable loan on v；第二个是 IndexGet 的 Shared loan。
        assert_eq!(analysis.loans.len(), 2, "closure mutable + IndexGet shared");
        assert_eq!(analysis.loans[&LoanId(0)].kind, LoanKind::Mutable);
        assert_eq!(analysis.loans[&LoanId(0)].place, v);
        assert_eq!(analysis.loans[&LoanId(0)].reference_local, Some(f));

        let conflicts = detect_conflicts(&analysis, &cfg);
        assert_eq!(
            conflicts.len(),
            1,
            "closure mutable loan + shared read on v must conflict, got {:?}",
            conflicts
        );
        assert_eq!(conflicts[0].kind, ConflictKind::SharedVsMutable);
        assert_eq!(conflicts[0].place, v);
    }

    /// 闭包按 ByValue 捕获（不直改外层 local）→ 不生成捕获 loan → 无冲突。
    #[test]
    fn closure_by_value_capture_no_loan() {
        let v = LocalId(0);
        let f = LocalId(1);
        let x = LocalId(2);

        let cap = ast::LambdaCapture {
            name: ast::Ident::from("v"),
            ty: typeck::TypeId::Void,
            mode: ast::CaptureMode::ByValue,
        };
        let cfg = one_block_cfg(vec![
            MirStatement::Assign {
                place: f,
                rvalue: MirRvalue::Use(MirOperand::Closure {
                    fn_name: "lambda".into(),
                    env: vec![(cap, MirOperand::Local(v))],
                }),
            },
            MirStatement::Assign {
                place: x,
                rvalue: MirRvalue::IndexGet {
                    array: MirOperand::Local(v),
                    index: MirOperand::ConstInt(0),
                    elem_type: typeck::TypeId::Void,
                },
            },
            MirStatement::Return(None),
        ]);

        let mut closure_mutated = empty_closure_map();
        closure_mutated.insert(
            "lambda".to_string(),
            ["v".to_string()].into_iter().collect(),
        );

        let analysis = BorrowAnalysis::from_cfg(&cfg, &closure_mutated);
        // ByValue 捕获不产生 loan；只有 IndexGet 的 shared loan。
        assert_eq!(analysis.loans.len(), 1, "ByValue capture must not borrow");
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert!(
            conflicts.is_empty(),
            "ByValue capture must not conflict, got {:?}",
            conflicts
        );
    }

    /// 闭包作为**调用实参**（`list.ForEach(x => …)`）时，捕获 loan 为
    /// statement_scoped；当其所在语句是块内**最后一条**时，kill 点
    /// `(block, len)` 既不在 statements 循环也不在 terminator——必须归一化
    /// 到 TERMINATOR_IDX，否则 loan 跨块泄漏 → 后续块读捕获变量被误报
    /// `E_BORROW_CONFLICT`（`list.ForEach(x => merged.Add(x)); return merged;`
    /// 在 AISkillSet::ToToolSet 实测）。
    #[test]
    fn closure_arg_loan_last_stmt_killed_at_terminator() {
        let v = LocalId(0);
        let f = LocalId(1);

        let cap = ast::LambdaCapture {
            name: ast::Ident::from("v"),
            ty: typeck::TypeId::Void,
            mode: ast::CaptureMode::ByRef,
        };
        // 单块：ForEach（闭包作实参）是**唯一**语句 → 块末 statement_scoped loan。
        let cfg = one_block_cfg(vec![MirStatement::Assign {
            place: f,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(v),
                method: "ForEach".into(),
                args: vec![MirOperand::Closure {
                    fn_name: "lambda".into(),
                    env: vec![(cap, MirOperand::Local(v))],
                }],
                receiver_type: "List".into(),
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
        }]);
        let mut closure_mutated = empty_closure_map();
        closure_mutated.insert(
            "lambda".to_string(),
            ["v".to_string()].into_iter().collect(),
        );

        let analysis = BorrowAnalysis::from_cfg(&cfg, &closure_mutated);
        let closure_loan = analysis
            .loans
            .iter()
            .find(|(_, l)| l.kind == LoanKind::Mutable && l.statement_scoped)
            .map(|(id, _)| *id)
            .expect("closure-arg loan must exist");
        assert!(
            analysis
                .kill_at(BlockId(0), TERMINATOR_IDX)
                .contains(&closure_loan),
            "块末 statement_scoped loan 必须归一化到 TERMINATOR_IDX 处 kill，否则跨块泄漏"
        );
    }

    /// 双块行为验证：block0 末语句为闭包实参调用（修改捕获 v），block1
    /// 读 v——修复后 loan 在 terminator 处 kill，block1 读不冲突。
    #[test]
    fn closure_arg_loan_last_stmt_blocks_cross_block_conflict() {
        let v = LocalId(0);
        let f = LocalId(1);
        let x = LocalId(2);

        let cap = ast::LambdaCapture {
            name: ast::Ident::from("v"),
            ty: typeck::TypeId::Void,
            mode: ast::CaptureMode::ByRef,
        };
        let entry = BlockId(0);
        let next = BlockId(1);
        let mut blocks = IndexMap::new();
        blocks.insert(
            entry,
            MirBlock {
                id: entry,
                statements: vec![MirStatement::Assign {
                    place: f,
                    rvalue: MirRvalue::MethodCall {
                        receiver: MirOperand::Local(v),
                        method: "ForEach".into(),
                        args: vec![MirOperand::Closure {
                            fn_name: "lambda".into(),
                            env: vec![(cap, MirOperand::Local(v))],
                        }],
                        receiver_type: "List".into(),
                        impl_class: None,
                        target_fn: None,
                        is_virtual: false,
                        params: vec![],
                    },
                }],
                terminator: MirTerminator::Goto(next),
            },
        );
        blocks.insert(
            next,
            MirBlock {
                id: next,
                statements: vec![MirStatement::Assign {
                    place: x,
                    rvalue: MirRvalue::IndexGet {
                        array: MirOperand::Local(v),
                        index: MirOperand::ConstInt(0),
                        elem_type: typeck::TypeId::Void,
                    },
                }],
                terminator: MirTerminator::Return(None),
            },
        );
        let cfg = MirCfgBody {
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
        };
        let mut closure_mutated = empty_closure_map();
        closure_mutated.insert(
            "lambda".to_string(),
            ["v".to_string()].into_iter().collect(),
        );

        let analysis = BorrowAnalysis::from_cfg(&cfg, &closure_mutated);
        let conflicts = detect_conflicts(&analysis, &cfg);
        assert!(
            conflicts.is_empty(),
            "块末调用实参闭包 loan 须在 terminator 处释放，block1 读 v 不得冲突，got {conflicts:?}"
        );
    }
}
