//! String literal collection and LLVM IR global emission.
//!
//! Standalone helpers that walk a `MirCfgBody` to gather every `ConstString`
//! operand into a shared pool, then emit `@.str.N` global constants. The pool
//! is shared across all `FnEmitter`s so identical literals reuse one global.

use mir::{MirCfgBody, MirOperand, MirRvalue, MirStatement, MirTerminator};
use std::collections::HashMap;

/// Collect all string literals from a MirCfgBody (for global emission).
pub(super) fn collect_string_literals(
    body: &MirCfgBody,
    literals: &mut Vec<String>,
    seen: &mut HashMap<String, String>,
) {
    for block in body.blocks.values() {
        for stmt in &block.statements {
            collect_strings_from_stmt(stmt, literals, seen);
        }
        // Collect from terminators (Return/Throw/CondBr may carry ConstString)
        match &block.terminator {
            MirTerminator::Return(Some(op)) | MirTerminator::Throw(op) => {
                collect_strings_from_operand(op, literals, seen);
            }
            MirTerminator::CondBr { cond, .. } => {
                collect_strings_from_operand(cond, literals, seen);
            }
            _ => {}
        }
    }
}

fn collect_strings_from_stmt(
    stmt: &MirStatement,
    literals: &mut Vec<String>,
    seen: &mut HashMap<String, String>,
) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => collect_strings_from_rvalue(rvalue, literals, seen),
        MirStatement::FieldSet { value, .. } => collect_strings_from_rvalue(value, literals, seen),
        // RFC 006 M3：静态字段赋值（`Counter._count = ...` / `_count = ...`）
        // 走 StaticFieldSet，与 FieldSet 对偶——必须递归收集字面量，
        // 否则 intern_string 回退到 @.str.0（静默错串，与 FieldSet 同类 bug）。
        MirStatement::StaticFieldSet { value, .. } => {
            collect_strings_from_rvalue(value, literals, seen)
        }
        MirStatement::IndexSet { value, .. } => collect_strings_from_rvalue(value, literals, seen),
        MirStatement::Return(Some(rv)) => collect_strings_from_rvalue(rv, literals, seen),
        MirStatement::Throw { value } => collect_strings_from_rvalue(value, literals, seen),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_strings_from_stmt(s, literals, seen);
            }
            for s in catch_body {
                collect_strings_from_stmt(s, literals, seen);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_strings_from_stmt(s, literals, seen);
            }
            for s in finally {
                collect_strings_from_stmt(s, literals, seen);
            }
        }
        MirStatement::LinqForeach { chain, body, .. } => {
            collect_strings_from_linq(chain, literals, seen);
            for s in body {
                collect_strings_from_stmt(s, literals, seen);
            }
        }
        // P1-B2：`catch when` 脱糖为 If；while 等同理须递归收集字面量，
        // 否则 intern_string 回退到 @.str.0（静默错串）。
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_strings_from_stmt(s, literals, seen);
            }
            for s in else_body {
                collect_strings_from_stmt(s, literals, seen);
            }
        }
        MirStatement::While { body, .. } => {
            for s in body {
                collect_strings_from_stmt(s, literals, seen);
            }
        }
        // `await F("lit")`：任务 rvalue 里可含 ConstString 参数，必须递归收集，
        // 否则 intern_string 回退到 @.str.0（静默错串 / 未定义全局）。
        MirStatement::Await { task, .. } => {
            collect_strings_from_rvalue(task, literals, seen);
        }
        _ => {}
    }
}

fn collect_strings_from_rvalue(
    rv: &MirRvalue,
    literals: &mut Vec<String>,
    seen: &mut HashMap<String, String>,
) {
    match rv {
        MirRvalue::Use(op) | MirRvalue::Coalesce { left: op, .. } => {
            collect_strings_from_operand(op, literals, seen)
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            collect_strings_from_operand(cond, literals, seen);
            collect_strings_from_operand(then_val, literals, seen);
            collect_strings_from_operand(else_val, literals, seen);
        }
        MirRvalue::Binary { left, right, .. } => {
            collect_strings_from_operand(left, literals, seen);
            collect_strings_from_operand(right, literals, seen);
        }
        MirRvalue::Call { args, .. } | MirRvalue::New { args, .. } => {
            for a in args {
                collect_strings_from_operand(a, literals, seen);
            }
        }
        MirRvalue::MethodCall { receiver, args, .. } => {
            collect_strings_from_operand(receiver, literals, seen);
            for a in args {
                collect_strings_from_operand(a, literals, seen);
            }
        }
        MirRvalue::StructLit { fields, .. } => {
            for (_, op) in fields {
                collect_strings_from_operand(op, literals, seen);
            }
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for el in elements {
                match el {
                    mir::ArrayLitElement::Value(rv) => {
                        collect_strings_from_rvalue(rv, literals, seen)
                    }
                    mir::ArrayLitElement::Spread(op) => {
                        collect_strings_from_operand(op, literals, seen)
                    }
                }
            }
        }
        MirRvalue::IndexGet { array, index, .. } => {
            collect_strings_from_operand(array, literals, seen);
            collect_strings_from_operand(index, literals, seen);
        }
        MirRvalue::FieldGet { object, .. } => {
            collect_strings_from_operand(object, literals, seen);
        }
        // RFC 004 M1：variant case 构造 `Value.Int(42)` / `Value.Str("lit")`。
        // payload 可携带 ConstString（如 `SetterValue.String("#FF112233")`），
        // 不收集则 intern_string 回退 @.str.0（静默错串）。
        MirRvalue::VariantConstruct {
            payload: Some(p), ..
        } => {
            collect_strings_from_operand(p, literals, seen);
        }
        MirRvalue::VariantTag { scrutinee, .. } => {
            collect_strings_from_operand(scrutinee, literals, seen);
        }
        MirRvalue::VariantExtract { scrutinee, .. } => {
            collect_strings_from_operand(scrutinee, literals, seen);
        }
        MirRvalue::MakeIface { object, .. } => {
            collect_strings_from_operand(object, literals, seen);
        }
        MirRvalue::MakeIfaceDyn { object, .. } => {
            collect_strings_from_operand(object, literals, seen);
        }
        MirRvalue::AdaptIface { object, .. } => {
            collect_strings_from_operand(object, literals, seen);
        }
        MirRvalue::Box { src, .. } => {
            collect_strings_from_operand(src, literals, seen);
        }
        MirRvalue::Unbox { src, .. } => {
            collect_strings_from_operand(src, literals, seen);
        }
        // `receiver?.field` / `receiver?.M(args)` / `receiver!.M(args)`：
        // default / args 可携带 ConstString，须递归收集。
        MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            collect_strings_from_operand(receiver, literals, seen);
            collect_strings_from_operand(default, literals, seen);
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            collect_strings_from_operand(receiver, literals, seen);
            for a in args {
                collect_strings_from_operand(a, literals, seen);
            }
            collect_strings_from_operand(default, literals, seen);
        }
        MirRvalue::ForceDerefField { receiver, .. } => {
            collect_strings_from_operand(receiver, literals, seen);
        }
        MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            collect_strings_from_operand(receiver, literals, seen);
            for a in args {
                collect_strings_from_operand(a, literals, seen);
            }
        }
        MirRvalue::IndirectCall { func, args } => {
            collect_strings_from_operand(func, literals, seen);
            for a in args {
                collect_strings_from_operand(a, literals, seen);
            }
        }
        MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            collect_strings_from_operand(array, literals, seen);
            if let Some(s) = start {
                collect_strings_from_operand(s, literals, seen);
            }
            if let Some(l) = length {
                collect_strings_from_operand(l, literals, seen);
            }
        }
        MirRvalue::SpanFromStack { elements, .. } => {
            for el in elements {
                collect_strings_from_operand(el, literals, seen);
            }
        }
        MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            collect_strings_from_operand(span, literals, seen);
            collect_strings_from_operand(start, literals, seen);
            if let Some(l) = length {
                collect_strings_from_operand(l, literals, seen);
            }
        }
        MirRvalue::SpanFill { span, value, .. } => {
            collect_strings_from_operand(span, literals, seen);
            collect_strings_from_operand(value, literals, seen);
        }
        MirRvalue::SpanClear { span, .. } => {
            collect_strings_from_operand(span, literals, seen);
        }
        MirRvalue::SpanCopyTo { src, dest, .. } => {
            collect_strings_from_operand(src, literals, seen);
            collect_strings_from_operand(dest, literals, seen);
        }
        MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            collect_strings_from_operand(src, literals, seen);
            collect_strings_from_operand(dest, literals, seen);
        }
        MirRvalue::SpanToArray { span, .. } => {
            collect_strings_from_operand(span, literals, seen);
        }
        MirRvalue::SoaFieldGet { array, index, .. } => {
            collect_strings_from_operand(array, literals, seen);
            collect_strings_from_operand(index, literals, seen);
        }
        MirRvalue::LinqChain(chain) => {
            collect_strings_from_linq(chain, literals, seen);
        }
        _ => {}
    }
}

fn collect_strings_from_linq(
    chain: &mir::LinqChain,
    literals: &mut Vec<String>,
    seen: &mut HashMap<String, String>,
) {
    collect_strings_from_operand(&chain.source, literals, seen);
}

fn collect_strings_from_operand(
    op: &MirOperand,
    literals: &mut Vec<String>,
    seen: &mut HashMap<String, String>,
) {
    if let MirOperand::ConstString(s) = op {
        if !seen.contains_key(s) {
            seen.insert(s.clone(), format!("@.str.{}", literals.len()));
            literals.push(s.clone());
        }
    }
}

/// Emit string literal globals from a pre-collected list.
pub(super) fn emit_string_globals(literals: &[String]) -> String {
    let mut out = String::new();
    out.push_str("; ---- String literals ----\n");
    for (i, s) in literals.iter().enumerate() {
        let bytes = s.as_bytes();
        let escaped = escape_llvm_string(bytes);
        out.push_str(&format!(
            "@.str.{i} = private unnamed_addr constant [{} x i8] c\"{escaped}\\00\"\n",
            bytes.len() + 1
        ));
    }
    out
}

pub(super) fn escape_llvm_string(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\22"),
            b if b.is_ascii_graphic() || b == b' ' => out.push(b as char),
            b => out.push_str(&format!("\\{:02X}", b)),
        }
    }
    out
}

/// RFC 017 M2：codegen 期动态字符串常量累积器。
///
/// `Assembly.Entry` 调用点的符号名（`__arc_entry_*`）与异常消息在 FnEmitter
/// 发射阶段才可知（依赖泛型实参），无法经 MIR `ConstString` 预收集。本累积器
/// 按需 intern 字符串，emit_module 末尾统一发射为 `@.arc_entry_sym.{N}` 全局。
pub(crate) struct StringConstAccumulator {
    seen: HashMap<String, String>,
    names: Vec<String>,
}

impl StringConstAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            seen: HashMap::new(),
            names: Vec::new(),
        }
    }

    /// 返回去重后的全局名（`@.arc_entry_sym.{N}`）。
    pub(crate) fn intern(&mut self, s: &str) -> String {
        if let Some(g) = self.seen.get(s) {
            return g.clone();
        }
        let g = format!("@.arc_entry_sym.{}", self.names.len());
        self.seen.insert(s.to_string(), g.clone());
        self.names.push(s.to_string());
        g
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// 发射去重后的私有字符串全局常量。
    pub(crate) fn render(&self) -> String {
        let mut out = String::from("; ---- RFC 017 M2: Assembly.Entry symbol constants ----\n");
        for (i, s) in self.names.iter().enumerate() {
            let bytes = s.as_bytes();
            let escaped = escape_llvm_string(bytes);
            out.push_str(&format!(
                "@.arc_entry_sym.{i} = private unnamed_addr constant [{} x i8] c\"{escaped}\\00\"\n",
                bytes.len() + 1
            ));
        }
        out
    }
}
