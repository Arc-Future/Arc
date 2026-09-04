//! RFC 016 M1：native callback trampoline 生成。
//!
//! 当 Arc 无捕获 lambda 作为实参传给 `.ani` 契约中声明为 `native callback` 类型
//! 的 native 函数形参时，本模块生成「trampoline」函数：
//!
//! - trampoline 函数签名匹配 C ABI（不含 `__env__` 参数）
//! - trampoline 体调用原始 lambda 函数
//! - trampoline 的函数指针传给 C 端
//!
//! M1 仅支持无捕获 lambda（`FnPtr`）；有捕获 lambda 在 codegen 报错。

use std::collections::{HashMap, HashSet};

/// native callback 类型名 → LLVM IR 签名信息
pub(crate) type NativeCallbackTable = HashMap<String, NativeCallbackIrInfo>;

/// 单个 native callback 的 LLVM IR 发射信息。
pub(crate) struct NativeCallbackIrInfo {
    /// 返回类型 LLVM IR 字符串（如 `i32`、`void`、`ptr`）。
    pub ret_llvm: String,
    /// 参数类型 LLVM IR 字符串列表（按 C ABI 顺序）。
    pub param_llvms: Vec<String>,
}

/// RFC 016 M1：模块级 trampoline IR 累积器。
///
/// FnEmitter 在 `try_emit_native_call` 中按需推入 trampoline IR；
/// ModuleEmitter 在 `emit_module` 末尾统一发射。
/// 按 trampoline 名去重（同一 (lambda, callback) 对仅发射一次），
/// 避免链接期「duplicate symbol」错误。
pub(crate) struct NativeTrampolineAccumulator {
    /// 已发射的 trampoline 名集合，用于跨函数去重。
    seen: HashSet<String>,
    /// 有序 IR 列表，保持模块输出确定性。
    irs: Vec<String>,
    /// RFC 016 M2：模块级单调递增的 TLS slot 计数器（跨函数唯一，
    /// 嵌套回调不冲突；超过 RT_FFI_MAX_CALLBACK_SLOTS 报错）。
    next_slot: i32,
}

impl NativeTrampolineAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            seen: HashSet::new(),
            irs: Vec::new(),
            next_slot: 0,
        }
    }

    /// 分配下一个 TLS callback slot（模块级唯一）。
    pub(crate) fn alloc_slot(&mut self) -> i32 {
        let slot = self.next_slot;
        self.next_slot += 1;
        slot
    }

    /// 已分配的 slot 总数（用于发射时校验上限）。
    pub(crate) fn slot_count(&self) -> i32 {
        self.next_slot
    }

    /// 若 `tramp_name` 未发射过，则推入 IR 并返回 `true`；否则返回 `false`。
    pub(crate) fn try_push(&mut self, tramp_name: &str, ir: String) -> bool {
        if self.seen.insert(tramp_name.to_string()) {
            self.irs.push(ir);
            true
        } else {
            false
        }
    }

    /// 是否为空（无 trampoline 待发射）。
    pub(crate) fn is_empty(&self) -> bool {
        self.irs.is_empty()
    }

    /// 借用 IR 列表用于模块级发射。
    pub(crate) fn irs(&self) -> &[String] {
        &self.irs
    }
}

/// 将 Arc native 类型名映射为 LLVM IR 类型名。
/// RFC 016 §3.3 类型白名单：基元/string/NativePtr/契约 struct。
fn native_param_type_to_llvm(ty: &ast::Type) -> String {
    match ty {
        ast::Type::Named { path, .. } => {
            if let Some(last) = path.last() {
                match last.as_str() {
                    "int" => "i32".into(),
                    "long" => "i64".into(),
                    "short" => "i16".into(),
                    "byte" => "i8".into(),
                    "uint" => "i32".into(),
                    "ushort" => "i16".into(),
                    "sbyte" => "i8".into(),
                    "char" => "i32".into(),
                    "bool" => "i1".into(),
                    "float" => "float".into(),
                    "double" => "double".into(),
                    "void" => "void".into(),
                    _ => "ptr".into(),
                }
            } else {
                "ptr".into()
            }
        }
        _ => "ptr".into(),
    }
}

/// 从 `NativeModule` 列表构建 callback 类型表。
pub(crate) fn build_callback_table(modules: &[ast::NativeModule]) -> NativeCallbackTable {
    let mut table = HashMap::new();
    for module in modules {
        for cb in &module.callbacks {
            let ret_llvm = match &cb.ret {
                Some(t) => native_param_type_to_llvm(&t.node),
                None => "void".to_string(),
            };
            let param_llvms: Vec<String> = cb
                .params
                .iter()
                .map(|p| native_param_type_to_llvm(&p.ty.node))
                .collect();
            table.insert(
                cb.name.to_string(),
                NativeCallbackIrInfo {
                    ret_llvm,
                    param_llvms,
                },
            );
        }
    }
    table
}

/// 生成 trampoline 函数 LLVM IR。
///
/// trampoline 签名匹配 C ABI（不含 `__env__`），内部调用原始 lambda。
///
/// **参数适配**：C ABI 按值传递基元类型（i32/i64/...）；Arc lambda 按指针
/// 传递所有参数（`ptr %arg`，lambda 体内 `load T, ptr %arg`）。trampoline
/// 需为每个**非 ptr** 参数分配栈槽，存入 C 端传入的值，再传指针给 lambda。
/// `ptr` 参数直接透传（已是指针，无需再取地址）。
///
/// 例：callback 签名 `IntBinOp(int a, int b) -> int`：
/// ```llvm
/// define i32 @__tramp_...(i32 %arg0, i32 %arg1) {
/// entry:
///   %slot0 = alloca i32
///   store i32 %arg0, ptr %slot0
///   %slot1 = alloca i32
///   store i32 %arg1, ptr %slot1
///   %r = call i32 @__lambda(ptr %slot0, ptr %slot1)
///   ret i32 %r
/// }
/// ```
pub(crate) fn emit_trampoline(
    lambda_name: &str,
    cb_info: &NativeCallbackIrInfo,
    trampoline_name: &str,
) -> String {
    let param_llvms = &cb_info.param_llvms;
    let ret_llvm = &cb_info.ret_llvm;

    // C ABI 参数列表（trampoline 形参）
    let mut param_strs: Vec<String> = Vec::new();
    // 调用 lambda 时的实参列表
    // - 非 ptr 参数：alloca + store，传 slot 指针
    // - ptr 参数：直接透传
    let mut call_arg_strs: Vec<String> = Vec::new();
    let mut body_lines: Vec<String> = Vec::new();

    for (i, pty) in param_llvms.iter().enumerate() {
        param_strs.push(format!("{pty} %arg{i}"));
        if *pty == "ptr" {
            // ptr 参数已是指针，直接透传
            call_arg_strs.push(format!("ptr %arg{i}"));
        } else {
            // 基元类型按值传入，但 lambda 期望指针——分配栈槽存入再传指针
            let slot = format!("%slot{i}");
            body_lines.push(format!("  {slot} = alloca {pty}"));
            body_lines.push(format!("  store {pty} %arg{i}, ptr {slot}"));
            call_arg_strs.push(format!("ptr {slot}"));
        }
    }
    let body = if body_lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", body_lines.join("\n"))
    };
    let call_args = call_arg_strs.join(", ");

    if ret_llvm == "void" {
        format!(
            "define void @{trampoline_name}({}) {{\n\
             entry:\n\
             {body}\
             \x20 call void @{lambda_name}({call_args})\n\
             \x20 ret void\n\
             }}\n",
            param_strs.join(", "),
        )
    } else {
        format!(
            "define {ret_llvm} @{trampoline_name}({}) {{\n\
             entry:\n\
             {body}\
             \x20 %r = call {ret_llvm} @{lambda_name}({call_args})\n\
             \x20 ret {ret_llvm} %r\n\
             }}\n",
            param_strs.join(", "),
        )
    }
}

/// 生成有捕获 lambda 的 TLS trampoline 函数 LLVM IR（RFC 016 M2）。
///
/// trampoline 签名匹配 C ABI（不含 `__env__`），内部从 TLS slot 取
/// `arc_closure`，间接调用 `fn_ptr(env, ...)`。参数适配与 `emit_trampoline`
/// 相同：非 ptr 参数 alloca+store 转指针，ptr 参数透传。
///
/// ```llvm
/// define i32 @__tramp_tls_0_CbFn___lambda(ptr %arg0, ptr %arg1) {
/// entry:
///   %slot0 = alloca i32
///   store i32 %arg0, ptr %slot0
///   %closure = call ptr @rt_ffi_get_callback(i32 0)
///   %fn = getelementptr %arc_closure, ptr %closure, i32 0, i32 0
///   %fnp = load ptr, ptr %fn
///   %env = getelementptr %arc_closure, ptr %closure, i32 0, i32 1
///   %envp = load ptr, ptr %env
///   %r = call i32 %fnp(ptr %envp, ptr %slot0, ptr %arg1)
///   ret i32 %r
/// }
/// ```
pub(crate) fn emit_tls_trampoline(
    slot: i32,
    _lambda_name: &str,
    cb_info: &NativeCallbackIrInfo,
    trampoline_name: &str,
) -> String {
    let param_llvms = &cb_info.param_llvms;
    let ret_llvm = &cb_info.ret_llvm;

    let mut param_strs: Vec<String> = Vec::new();
    let mut call_arg_strs: Vec<String> = Vec::new();
    let mut body_lines: Vec<String> = Vec::new();

    for (i, pty) in param_llvms.iter().enumerate() {
        param_strs.push(format!("{pty} %arg{i}"));
        if *pty == "ptr" {
            call_arg_strs.push(format!("ptr %arg{i}"));
        } else {
            let slot_name = format!("%slot{i}");
            body_lines.push(format!("  {slot_name} = alloca {pty}"));
            body_lines.push(format!("  store {pty} %arg{i}, ptr {slot_name}"));
            call_arg_strs.push(format!("ptr {slot_name}"));
        }
    }

    // 从 TLS slot 取 closure 并拆出 fn_ptr/env。
    body_lines.push(format!(
        "  %closure = call ptr @rt_ffi_get_callback(i32 {slot})"
    ));
    body_lines.push("  %fn = getelementptr %arc_closure, ptr %closure, i32 0, i32 0".to_string());
    body_lines.push("  %fnp = load ptr, ptr %fn".to_string());
    body_lines.push("  %env = getelementptr %arc_closure, ptr %closure, i32 0, i32 1".to_string());
    body_lines.push("  %envp = load ptr, ptr %env".to_string());
    let body = format!("{}\n", body_lines.join("\n"));
    let call_args = call_arg_strs.join(", ");

    if ret_llvm == "void" {
        format!(
            "define void @{trampoline_name}({}) {{\n\
             entry:\n\
             {body}\
             \x20 call void %fnp(ptr %envp, {call_args})\n\
             \x20 ret void\n\
             }}\n",
            param_strs.join(", "),
        )
    } else {
        format!(
            "define {ret_llvm} @{trampoline_name}({}) {{\n\
             entry:\n\
             {body}\
             \x20 %r = call {ret_llvm} %fnp(ptr %envp, {call_args})\n\
             \x20 ret {ret_llvm} %r\n\
             }}\n",
            param_strs.join(", "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trampoline_void_ret() {
        let cb = NativeCallbackIrInfo {
            ret_llvm: "void".into(),
            param_llvms: vec!["ptr".into(), "ptr".into()],
        };
        let ir = emit_trampoline("__lambda_cmp", &cb, "__tramp_CmpFn___lambda_cmp");
        assert!(ir.contains("define void @__tramp_CmpFn___lambda_cmp(ptr %arg0, ptr %arg1)"));
        assert!(ir.contains("call void @__lambda_cmp"));
        assert!(ir.contains("ret void"));
    }

    #[test]
    fn trampoline_typed_ret() {
        // ptr 参数透传，不分配栈槽
        let cb = NativeCallbackIrInfo {
            ret_llvm: "i32".into(),
            param_llvms: vec!["ptr".into(), "ptr".into()],
        };
        let ir = emit_trampoline("__lambda_cmp", &cb, "__tramp_CmpFn___lambda_cmp");
        assert!(ir.contains("define i32 @__tramp_CmpFn___lambda_cmp"));
        assert!(ir.contains("%r = call i32 @__lambda_cmp(ptr %arg0, ptr %arg1)"));
        assert!(ir.contains("ret i32 %r"));
    }

    #[test]
    fn trampoline_primitive_args_boxed() {
        // 基元类型 (i32) 参数需 alloca+store 转为指针
        let cb = NativeCallbackIrInfo {
            ret_llvm: "i32".into(),
            param_llvms: vec!["i32".into(), "i32".into()],
        };
        let ir = emit_trampoline("__lambda_sub", &cb, "__tramp_IntBinOp___lambda_sub");
        assert!(ir.contains("define i32 @__tramp_IntBinOp___lambda_sub(i32 %arg0, i32 %arg1)"));
        assert!(ir.contains("%slot0 = alloca i32"));
        assert!(ir.contains("store i32 %arg0, ptr %slot0"));
        assert!(ir.contains("%slot1 = alloca i32"));
        assert!(ir.contains("store i32 %arg1, ptr %slot1"));
        assert!(ir.contains("%r = call i32 @__lambda_sub(ptr %slot0, ptr %slot1)"));
        assert!(ir.contains("ret i32 %r"));
    }

    #[test]
    fn accumulator_dedups_by_name() {
        let mut acc = NativeTrampolineAccumulator::new();
        let ir1 = "define void @t1 {}\n".to_string();
        let ir2 = "define void @t2 {}\n".to_string();
        assert!(acc.try_push("t1", ir1.clone()));
        assert!(!acc.try_push("t1", ir1.clone())); // 同名重复，跳过
        assert!(acc.try_push("t2", ir2));
        assert_eq!(acc.irs().len(), 2);
        assert!(!acc.is_empty());
    }
}
