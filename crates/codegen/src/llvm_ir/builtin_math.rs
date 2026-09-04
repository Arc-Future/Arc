//! Builtin `Arc.Math` static method emission (RFC 021 Phase 0).
//!
//! Maps `Math.<method>(args)` to LLVM intrinsics or libm calls — no `rt_math_*`
//! ABI. clang `-O2` inlines intrinsics to native instructions where possible.
//!
//! Honest Stable surface = methods handled here. Facade methods not listed must
//! not silently return the stub `0` body; unknown names return `None` so the
//! caller can fall through / diagnose.

use super::*;
use mir::MirOperand;

/// Unary/binary double Math methods that map 1:1 to LLVM intrinsics.
fn math_f64_intrinsic(method: &str) -> Option<(&'static str, usize)> {
    Some(match method {
        "Sqrt" => ("@llvm.sqrt.f64", 1),
        "Sin" => ("@llvm.sin.f64", 1),
        "Cos" => ("@llvm.cos.f64", 1),
        "Exp" => ("@llvm.exp.f64", 1),
        "Log" => ("@llvm.log.f64", 1),
        "Log10" => ("@llvm.log10.f64", 1),
        "Log2" => ("@llvm.log2.f64", 1),
        "Pow" => ("@llvm.pow.f64", 2),
        "Floor" => ("@llvm.floor.f64", 1),
        "Ceiling" => ("@llvm.ceil.f64", 1),
        "Round" => ("@llvm.rint.f64", 1), // C# Math.Round 默认银行家舍入（round-half-to-even）；llvm.round 是 away-from-zero，不合
        "Truncate" => ("@llvm.trunc.f64", 1),
        "Fma" => ("@llvm.fmuladd.f64", 3),
        "CopySign" => ("@llvm.copysign.f64", 2),
        _ => return None,
    })
}

/// Double Math methods lowered via libm (no LLVM intrinsic).
fn math_libm(method: &str) -> Option<(&'static str, usize)> {
    Some(match method {
        "Tan" => ("@tan", 1),
        "Asin" => ("@asin", 1),
        "Acos" => ("@acos", 1),
        "Atan" => ("@atan", 1),
        "Atan2" => ("@atan2", 2),
        "Sinh" => ("@sinh", 1),
        "Cosh" => ("@cosh", 1),
        "Tanh" => ("@tanh", 1),
        "Cbrt" => ("@cbrt", 1),
        "Hypot" => ("@hypot", 2),
        "IEEERemainder" => ("@remainder", 2),
        _ => return None,
    })
}

impl<'a> FnEmitter<'a> {
    /// Try to emit a `Math.<method>(args)` call.
    ///
    /// Returns `Some((type, value))` if handled, `None` if the method is unknown.
    pub(super) fn try_emit_math_call(
        &mut self,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        match method {
            "Abs" => return self.emit_math_abs(args),
            "Min" => return self.emit_math_min_max(args, true),
            "Max" => return self.emit_math_min_max(args, false),
            "Sign" => return self.emit_math_sign(args),
            "Clamp" => return self.emit_math_clamp(args),
            _ => {}
        }

        if let Some((intrinsic, arity)) = math_f64_intrinsic(method) {
            return Some(self.emit_math_f64_call(intrinsic, arity, args));
        }
        if let Some((func, arity)) = math_libm(method) {
            return Some(self.emit_math_f64_call(func, arity, args));
        }
        None
    }

    fn emit_math_f64_call(&mut self, callee: &str, arity: usize, args: &[MirOperand]) -> TyVal {
        let mut arg_strs: Vec<String> = Vec::with_capacity(arity);
        for i in 0..arity {
            let (_, val) =
                self.emit_operand(&args.get(i).cloned().unwrap_or(MirOperand::ConstInt(0)));
            arg_strs.push(format!("double {val}"));
        }
        let tmp = self.fresh_temp();
        self.emit(&format!(
            "{tmp} = call double {callee}({})",
            arg_strs.join(", ")
        ));
        ("double".into(), tmp)
    }

    /// `Math.Abs`: double → `@llvm.fabs.f64`; int → `@llvm.abs.i32`; long → `@llvm.abs.i64`.
    fn emit_math_abs(&mut self, args: &[MirOperand]) -> Option<TyVal> {
        let (arg_ty, arg_val) =
            self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));

        let tmp = self.fresh_temp();
        match arg_ty.as_str() {
            "double" => {
                self.emit(&format!(
                    "{tmp} = call double @llvm.fabs.f64(double {arg_val})"
                ));
                Some(("double".into(), tmp))
            }
            "i64" => {
                self.emit(&format!(
                    "{tmp} = call i64 @llvm.abs.i64(i64 {arg_val}, i1 false)"
                ));
                Some(("i64".into(), tmp))
            }
            _ => {
                self.emit(&format!(
                    "{tmp} = call i32 @llvm.abs.i32(i32 {arg_val}, i1 false)"
                ));
                Some(("i32".into(), tmp))
            }
        }
    }

    /// `Math.Min` / `Math.Max` overload by operand LLVM type.
    fn emit_math_min_max(&mut self, args: &[MirOperand], is_min: bool) -> Option<TyVal> {
        let (a_ty, a_val) =
            self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
        let (_, b_val) =
            self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));

        let tmp = self.fresh_temp();
        match a_ty.as_str() {
            "double" => {
                let intrinsic = if is_min {
                    "@llvm.minnum.f64"
                } else {
                    "@llvm.maxnum.f64"
                };
                self.emit(&format!(
                    "{tmp} = call double {intrinsic}(double {a_val}, double {b_val})"
                ));
                Some(("double".into(), tmp))
            }
            "i64" => {
                let cmp = self.fresh_temp();
                let pred = if is_min { "slt" } else { "sgt" };
                self.emit(&format!("{cmp} = icmp {pred} i64 {a_val}, {b_val}"));
                self.emit(&format!(
                    "{tmp} = select i1 {cmp}, i64 {a_val}, i64 {b_val}"
                ));
                Some(("i64".into(), tmp))
            }
            _ => {
                let cmp = self.fresh_temp();
                let pred = if is_min { "slt" } else { "sgt" };
                self.emit(&format!("{cmp} = icmp {pred} i32 {a_val}, {b_val}"));
                self.emit(&format!(
                    "{tmp} = select i1 {cmp}, i32 {a_val}, i32 {b_val}"
                ));
                Some(("i32".into(), tmp))
            }
        }
    }

    /// `Math.Sign`: returns -1 / 0 / 1.
    fn emit_math_sign(&mut self, args: &[MirOperand]) -> Option<TyVal> {
        let (arg_ty, arg_val) =
            self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));

        let cmp_neg = self.fresh_temp();
        let cmp_pos = self.fresh_temp();
        let neg_sel = self.fresh_temp();
        let tmp = self.fresh_temp();

        if arg_ty == "double" {
            self.emit(&format!(
                "{cmp_neg} = fcmp olt double {arg_val}, 0.000000e+00"
            ));
            self.emit(&format!(
                "{cmp_pos} = fcmp ogt double {arg_val}, 0.000000e+00"
            ));
            self.emit(&format!("{neg_sel} = select i1 {cmp_neg}, i32 -1, i32 0"));
            self.emit(&format!(
                "{tmp} = select i1 {cmp_pos}, i32 1, i32 {neg_sel}"
            ));
        } else {
            let ty = if arg_ty == "i64" { "i64" } else { "i32" };
            self.emit(&format!("{cmp_neg} = icmp slt {ty} {arg_val}, 0"));
            self.emit(&format!("{cmp_pos} = icmp sgt {ty} {arg_val}, 0"));
            self.emit(&format!("{neg_sel} = select i1 {cmp_neg}, i32 -1, i32 0"));
            self.emit(&format!(
                "{tmp} = select i1 {cmp_pos}, i32 1, i32 {neg_sel}"
            ));
        }
        Some(("i32".into(), tmp))
    }

    /// `Math.Clamp(value, min, max)` → max(min(value, max), min).
    /// double → minnum/maxnum；int/long → icmp/select（假定 `min <= max`）。
    fn emit_math_clamp(&mut self, args: &[MirOperand]) -> Option<TyVal> {
        let (v_ty, v) =
            self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
        let (_, lo) = self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
        let (_, hi) = self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
        let t = self.fresh_temp();
        let tmp = self.fresh_temp();
        match v_ty.as_str() {
            "double" => {
                self.emit(&format!(
                    "{t} = call double @llvm.maxnum.f64(double {v}, double {lo})"
                ));
                self.emit(&format!(
                    "{tmp} = call double @llvm.minnum.f64(double {t}, double {hi})"
                ));
                Some(("double".into(), tmp))
            }
            "i64" => {
                let cmp_lo = self.fresh_temp();
                let cmp_hi = self.fresh_temp();
                self.emit(&format!("{cmp_lo} = icmp slt i64 {v}, {lo}"));
                self.emit(&format!("{t} = select i1 {cmp_lo}, i64 {lo}, i64 {v}"));
                self.emit(&format!("{cmp_hi} = icmp sgt i64 {t}, {hi}"));
                self.emit(&format!("{tmp} = select i1 {cmp_hi}, i64 {hi}, i64 {t}"));
                Some(("i64".into(), tmp))
            }
            _ => {
                let cmp_lo = self.fresh_temp();
                let cmp_hi = self.fresh_temp();
                self.emit(&format!("{cmp_lo} = icmp slt i32 {v}, {lo}"));
                self.emit(&format!("{t} = select i1 {cmp_lo}, i32 {lo}, i32 {v}"));
                self.emit(&format!("{cmp_hi} = icmp sgt i32 {t}, {hi}"));
                self.emit(&format!("{tmp} = select i1 {cmp_hi}, i32 {hi}, i32 {t}"));
                Some(("i32".into(), tmp))
            }
        }
    }
}
