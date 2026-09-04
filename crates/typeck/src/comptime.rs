//! RFC 012：comptime 有限子集——编译期常量求值（仅整型 / bool / string 字面量运算）。
//!
//! 本模块是 comptime 的**纯函数解释器**：给定一个表达式，若其可折叠为编译期常量
//! （整型 / bool / string 字面量及其二元/一元运算），则返回 [`ConstValue`]；否则
//! 返回 `None`。typeck 在 `check_expr` 遇到 `Expr::Comptime` 时调用本模块折叠，
//! 折叠失败即报编译错误——运行期不产生任何求值（零开销）。
//!
//! 边界（RFC 012 §不立宪）：不实现 comptime 函数、comptime 类型构造、comptime if、
//! comptime 反射、comptime 代码生成。本 Phase 仅折叠字面量运算。

use crate::oop_types::ConstValue;
use ast::{BinOp, Expr, UnaryOp};

/// 折叠 comptime 表达式为编译期常量；不可折叠返回 `None`。
pub fn eval_comptime(expr: &Expr) -> Option<ConstValue> {
    match expr {
        Expr::IntLit(n) => Some(ConstValue::Int(*n)),
        Expr::FloatLit(ast::FloatLitValue::Double(f)) => Some(ConstValue::Float(*f)),
        Expr::FloatLit(ast::FloatLitValue::Float(f)) => Some(ConstValue::Float(*f as f64)),
        Expr::BoolLit(b) => Some(ConstValue::Bool(*b)),
        Expr::StringLit(s) => Some(ConstValue::String(s.clone())),
        Expr::CharLit(c) => Some(ConstValue::Int(*c as i64)),
        Expr::Unary { op, expr } => eval_unary(*op, eval_comptime(&expr.node)?),
        Expr::Binary { op, left, right } => {
            let l = eval_comptime(&left.node)?;
            let r = eval_comptime(&right.node)?;
            eval_binary(*op, l, r)
        }
        _ => None,
    }
}

fn eval_unary(op: UnaryOp, v: ConstValue) -> Option<ConstValue> {
    match (op, v) {
        (UnaryOp::Neg, ConstValue::Int(n)) => Some(ConstValue::Int(n.wrapping_neg())),
        (UnaryOp::Neg, ConstValue::Float(f)) => Some(ConstValue::Float(-f)),
        (UnaryOp::Not, ConstValue::Bool(b)) => Some(ConstValue::Bool(!b)),
        (UnaryOp::BitNot, ConstValue::Int(n)) => Some(ConstValue::Int(!n)),
        _ => None,
    }
}

fn eval_binary(op: BinOp, l: ConstValue, r: ConstValue) -> Option<ConstValue> {
    use ConstValue as C;
    match (op, l, r) {
        // ── 整型算术 ──
        (BinOp::Add, C::Int(a), C::Int(b)) => Some(C::Int(a.wrapping_add(b))),
        (BinOp::Sub, C::Int(a), C::Int(b)) => Some(C::Int(a.wrapping_sub(b))),
        (BinOp::Mul, C::Int(a), C::Int(b)) => Some(C::Int(a.wrapping_mul(b))),
        (BinOp::Div, C::Int(a), C::Int(b)) if b != 0 => Some(C::Int(a / b)),
        (BinOp::Mod, C::Int(a), C::Int(b)) if b != 0 => Some(C::Int(a % b)),
        // ── 浮点算术 ──
        (BinOp::Add, C::Float(a), C::Float(b)) => Some(C::Float(a + b)),
        (BinOp::Sub, C::Float(a), C::Float(b)) => Some(C::Float(a - b)),
        (BinOp::Mul, C::Float(a), C::Float(b)) => Some(C::Float(a * b)),
        (BinOp::Div, C::Float(a), C::Float(b)) => Some(C::Float(a / b)),
        // ── 字符串拼接 ──
        (BinOp::Add, C::String(a), C::String(b)) => Some(C::String(a + &b)),
        // ── 整型比较 → bool ──
        (BinOp::Eq, C::Int(a), C::Int(b)) => Some(C::Bool(a == b)),
        (BinOp::NotEq, C::Int(a), C::Int(b)) => Some(C::Bool(a != b)),
        (BinOp::Lt, C::Int(a), C::Int(b)) => Some(C::Bool(a < b)),
        (BinOp::Le, C::Int(a), C::Int(b)) => Some(C::Bool(a <= b)),
        (BinOp::Gt, C::Int(a), C::Int(b)) => Some(C::Bool(a > b)),
        (BinOp::Ge, C::Int(a), C::Int(b)) => Some(C::Bool(a >= b)),
        // ── 浮点比较 → bool ──
        (BinOp::Eq, C::Float(a), C::Float(b)) => Some(C::Bool(a == b)),
        (BinOp::NotEq, C::Float(a), C::Float(b)) => Some(C::Bool(a != b)),
        (BinOp::Lt, C::Float(a), C::Float(b)) => Some(C::Bool(a < b)),
        (BinOp::Le, C::Float(a), C::Float(b)) => Some(C::Bool(a <= b)),
        (BinOp::Gt, C::Float(a), C::Float(b)) => Some(C::Bool(a > b)),
        (BinOp::Ge, C::Float(a), C::Float(b)) => Some(C::Bool(a >= b)),
        // ── bool 逻辑 ──
        (BinOp::Eq, C::Bool(a), C::Bool(b)) => Some(C::Bool(a == b)),
        (BinOp::NotEq, C::Bool(a), C::Bool(b)) => Some(C::Bool(a != b)),
        (BinOp::And, C::Bool(a), C::Bool(b)) => Some(C::Bool(a && b)),
        (BinOp::Or, C::Bool(a), C::Bool(b)) => Some(C::Bool(a || b)),
        // ── 整型位运算 ──
        (BinOp::BitAnd, C::Int(a), C::Int(b)) => Some(C::Int(a & b)),
        (BinOp::BitOr, C::Int(a), C::Int(b)) => Some(C::Int(a | b)),
        (BinOp::BitXor, C::Int(a), C::Int(b)) => Some(C::Int(a ^ b)),
        (BinOp::Shl, C::Int(a), C::Int(b)) if b >= 0 => Some(C::Int(a.wrapping_shl(b as u32))),
        (BinOp::Shr, C::Int(a), C::Int(b)) if b >= 0 => Some(C::Int(a >> b)),
        _ => None,
    }
}
