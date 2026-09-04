//! Builtin `Arc.Vector<T, N>` static method emission (RFC 021 Phase 2).
//!
//! Maps `Vector.<method>(args)` calls directly to LLVM vector instructions:
//! - `Add`/`Sub`/`Mul` → `fadd`/`fsub`/`fmul <N x T>`
//! - `Fma` → `call <N x T> @llvm.fmuladd.v<N>f<T>`
//! - `Get` → `extractelement <N x T>, i32`
//! - `Set` → `insertelement <N x T>, T, i32`
//!
//! No runtime ABI — pure LLVM vector ops. clang `-O2` lowers to SIMD instructions
//! (e.g. `vaddps`, `vfmadd213ps`) on AVX2 targets.

use super::*;
use ast::TypeId;
use mir::MirOperand;

impl<'a> FnEmitter<'a> {
    /// Try to emit a `Vector.<method>(args)` call as LLVM vector instructions.
    ///
    /// Called from `emit_method_call_typed` when `receiver_type == "Vector"`.
    /// The vector type `<N x T>` is inferred from the first argument (a Vector
    /// local), not from `expected`, because `Get` returns a scalar `T` while
    /// `Add`/`Sub`/`Mul`/`Fma`/`Set` return `Vector<T, N>`.
    /// Returns `Some((type, value))` if handled, `None` if the method is unknown.
    pub(super) fn try_emit_vector_call(
        &mut self,
        method: &str,
        args: &[MirOperand],
        _expected: &TypeId,
    ) -> Option<TyVal> {
        // Infer (elem_llvm_ty, n) from the first argument — a Vector local.
        let (elem_ty, n) = match args.first() {
            Some(MirOperand::Local(id)) => match self.local_type(*id) {
                TypeId::Vector { elem, n } => {
                    let e = llvm_type_of(&elem, self.layouts);
                    (e, n)
                }
                _ => return None,
            },
            _ => return None,
        };
        let vec_ty = format!("<{n} x {elem_ty}>");

        match method {
            "Add" | "Sub" | "Mul" => {
                let (_, a) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, b) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let op = match method {
                    "Add" => "fadd",
                    "Sub" => "fsub",
                    "Mul" => "fmul",
                    _ => unreachable!(),
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = {op} {vec_ty} {a}, {b}"));
                Some((vec_ty, tmp))
            }
            "Fma" => {
                let (_, a) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, b) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, c) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let intrinsic = fmuladd_intrinsic(&elem_ty, n);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call {vec_ty} {intrinsic}({vec_ty} {a}, {vec_ty} {b}, {vec_ty} {c})"
                ));
                Some((vec_ty, tmp))
            }
            "Get" => {
                let (_, v) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, i) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = extractelement {vec_ty} {v}, i32 {i}"));
                Some((elem_ty.to_string(), tmp))
            }
            "Set" => {
                let (_, v) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, i) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (val_ty, val) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                // Coerce scalar value to vector element type (e.g. double literal → float).
                let (_, val) = self.coerce_value(&val_ty, val, &elem_ty);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = insertelement {vec_ty} {v}, {elem_ty} {val}, i32 {i}"
                ));
                Some((vec_ty, tmp))
            }
            _ => None,
        }
    }
}

/// LLVM FMA intrinsic name for `<N x T>`: `@llvm.fmuladd.v4f32` / `@llvm.fmuladd.v8f64`.
fn fmuladd_intrinsic(elem_ty: &str, n: u32) -> String {
    let suffix = match elem_ty {
        "float" => "f32",
        "double" => "f64",
        _ => "f64",
    };
    format!("@llvm.fmuladd.v{n}{suffix}")
}
