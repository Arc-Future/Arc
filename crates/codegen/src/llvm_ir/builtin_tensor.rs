//! Builtin `Arc.Tensor<T>` stub emission (RFC 021 Phase 1).
//!
//! All Tensor<T> methods (ctor, Get/Set, property getters, binary ops) are
//! dispatched through stub-based code generation — `try_emit_tensor_stub`
//! produces LLVM IR function definitions that codegen calls through the
//! normal method-call path.
//!
//! Binary ops (Add/Sub/Mul/Matmul) are instance methods: `a.Add(b)` calls
//! `@Tensor_<elem>_Add(ptr %self, ptr %other)` which loads both handles,
//! invokes the corresponding `rt_tensor_*` runtime function, allocates a
//! new Tensor object, and returns it.

use super::*;
use crate::llvm_ir::types::{parse_tensor_elem, tensor_elem_llvm_ty, tensor_elem_size};

/// Runtime ABI function for each instance binary op.
const TENSOR_BINARY_OPS: &[(&str, &str)] = &[
    ("Add", "@rt_tensor_add"),
    ("Sub", "@rt_tensor_sub"),
    ("Mul", "@rt_tensor_mul"),
    ("Matmul", "@rt_tensor_matmul"),
];

/// Try to emit a `Tensor<T>` stub function definition.
///
/// Generates LLVM IR for ctor, instance methods (Get/Set/property getters),
/// and binary ops (Add/Sub/Mul/Matmul). Returns `None` for unknown methods.
pub(super) fn try_emit_tensor_stub(name: &str) -> Option<String> {
    let mangled = mangle_fn_name(name);
    let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
    let class_name = class_name.split("::").next().unwrap_or(class_name);
    let elem_suf = parse_tensor_elem(class_name)?;
    let elem_ty = tensor_elem_llvm_ty(elem_suf);
    let elem_size = tensor_elem_size(elem_suf);

    if name.contains("__ctor") {
        return Some(format!(
            "define void @{mangled}(ptr %self, i32 %rows, i32 %cols) {{\n\
             entry:\n\
             \x20 %handle = call ptr @rt_tensor_create(i32 %rows, i32 %cols, i32 {elem_size})\n\
             \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
             \x20 store ptr %handle, ptr %hp\n\
             \x20 ret void\n\
             }}\n"
        ));
    }

    let method = name.split("::").nth(1).unwrap_or("");
    Some(match method {
        "Get" => format!(
            "define {elem_ty} @{mangled}(ptr %self, i32 %i, i32 %j) {{\n\
             entry:\n\
             \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
             \x20 %handle = load ptr, ptr %hp\n\
             \x20 %result = alloca {elem_ty}\n\
             \x20 call void @rt_tensor_get(ptr %handle, i32 %i, i32 %j, ptr %result)\n\
             \x20 %r = load {elem_ty}, ptr %result\n\
             \x20 ret {elem_ty} %r\n\
             }}\n"
        ),
        "Set" => format!(
            "define void @{mangled}(ptr %self, i32 %i, i32 %j, {elem_ty} %v) {{\n\
             entry:\n\
             \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
             \x20 %handle = load ptr, ptr %hp\n\
             \x20 %v_addr = alloca {elem_ty}\n\
             \x20 store {elem_ty} %v, ptr %v_addr\n\
             \x20 call void @rt_tensor_set(ptr %handle, i32 %i, i32 %j, ptr %v_addr)\n\
             \x20 ret void\n\
             }}\n"
        ),
        "get_Rank" => tensor_int_getter_stub(&mangled, "@rt_tensor_rank"),
        "get_Rows" => tensor_int_getter_stub(&mangled, "@rt_tensor_rows"),
        "get_Cols" => tensor_int_getter_stub(&mangled, "@rt_tensor_cols"),
        "get_Total" => tensor_int_getter_stub(&mangled, "@rt_tensor_total"),
        _ => {
            let rt_fn = TENSOR_BINARY_OPS
                .iter()
                .find(|(m, _)| *m == method)
                .map(|(_, f)| *f)?;
            return Some(tensor_binary_op_stub(&mangled, rt_fn));
        }
    })
}

fn tensor_int_getter_stub(mangled: &str, rt_fn: &str) -> String {
    format!(
        "define i32 @{mangled}(ptr %self) {{\n\
         entry:\n\
         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
         \x20 %handle = load ptr, ptr %hp\n\
         \x20 %r = call i32 {rt_fn}(ptr %handle)\n\
         \x20 ret i32 %r\n\
         }}\n"
    )
}

fn tensor_binary_op_stub(mangled: &str, rt_fn: &str) -> String {
    format!(
        "define ptr @{mangled}(ptr %self, ptr %other) {{\n\
         entry:\n\
         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
         \x20 %h1 = load ptr, ptr %hp\n\
         \x20 %op = getelementptr inbounds i8, ptr %other, i32 16\n\
         \x20 %h2 = load ptr, ptr %op\n\
         \x20 %rh = call ptr {rt_fn}(ptr %h1, ptr %h2)\n\
         \x20 %obj = call ptr @malloc(i64 24)\n\
         \x20 store i32 1, ptr %obj\n\
         \x20 %vt = getelementptr inbounds i8, ptr %obj, i32 8\n\
         \x20 store ptr null, ptr %vt\n\
         \x20 %np = getelementptr inbounds i8, ptr %obj, i32 16\n\
         \x20 store ptr %rh, ptr %np\n\
         \x20 ret ptr %obj\n\
         }}\n"
    )
}
