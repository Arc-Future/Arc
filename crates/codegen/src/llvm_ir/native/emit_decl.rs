//! LLVM `declare` 与契约 struct 类型发射（RFC 016 M1/M3）。
//!
//! 将 `NativeSymbolTable` 中的每个符号信息转换为 LLVM IR `declare` 声明，
//! 在 `emit_module` 中紧跟 runtime_decls 之后插入。
//!
//! RFC 016 M3：根据 `calling_conv` 在 `declare` 前 emit 调用约定前缀
//! （`stdcallcc ` 或省略默认 `ccc`）。
//!
//! RFC 016 M3 §3.3：emit 契约 struct 类型定义（`%struct.<Name> = type { ... }`），
//! 供 `declare`/`call` 中的 `%struct.<Name>` 类型引用。类型定义在 `declare` 之前
//! emit，确保 LLVM 验证器能解析前向引用。按值传递的 struct 在 call 边界由 LLVM
//! 后端优化为寄存器传递（≤16 bytes）或栈传递（>16 bytes），零额外开销。

use ast::{CallingConv, NativeModule, NativeTypeDecl, NativeTypeKind};

use super::runtime_load::RuntimeModuleInfos;
use super::symbols::{native_type_to_llvm, NativeSymbolTable};

/// 发射所有 native 函数的 LLVM `declare` 声明。
///
/// 在 `emit_module` 中紧跟 runtime_decls 之后插入。
///
/// RFC 016：生效策略为 `runtime` 的模块跳过 `declare`——其符号由懒解析器经
/// `rt_library_sym` 在运行时解析，编译期不建立外部符号引用（静态链接不需要）。
pub(crate) fn emit_native_decls(
    table: &NativeSymbolTable,
    runtime_infos: &RuntimeModuleInfos,
) -> String {
    if table.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("; ---- Native contract declarations (RFC 016) ----\n");
    for (key, info) in table {
        // RFC 016：runtime 加载模块不发射 declare（懒解析，无静态链接符号）。
        let module = key.rsplit_once('.').map(|(m, _)| m).unwrap_or(key);
        if runtime_infos.contains_key(module) {
            continue;
        }
        let params = info.param_llvms.join(", ");
        // RFC 016 M3：调用约定前缀。C 省略（默认 ccc），Stdcall 显式 emit `stdcallcc `。
        let cc_prefix = match info.calling_conv {
            CallingConv::C => "",
            CallingConv::Stdcall => "stdcallcc ",
        };
        out.push_str(&format!(
            "declare {cc_prefix}{} @{}({})\n",
            info.ret_llvm, info.symbol, params
        ));
    }
    out
}

/// 发射契约 struct 类型定义（RFC 016 M3 §3.3）。
///
/// 遍历所有模块的 `NativeTypeDecl`，对 `Struct` 类型 emit
/// `%struct.<Name> = type { <field_type_1>, <field_type_2>, ... }`。
///
/// `OpaquePtr` 类型不 emit（按 `ptr` 传递，无需类型定义）。
///
/// 类型定义在 `declare` 之前 emit，确保 LLVM 验证器能解析前向引用。
/// 字段类型通过 `native_type_to_llvm` 映射，复用白名单类型映射逻辑。
///
/// **性能考量**：按值传递的 struct 在 LLVM IR 层面是直接传 struct 值，
/// LLVM 后端会优化为寄存器传递（≤16 bytes 的小 struct）或栈传递（大 struct），
/// 无额外运行时开销。这与 C ABI 完全一致，是 FFI 边界最高效的传递方式。
pub(crate) fn emit_native_struct_types(modules: &[NativeModule]) -> String {
    let struct_decls: Vec<&NativeTypeDecl> = modules
        .iter()
        .flat_map(|m| m.types.iter())
        .filter(|d| matches!(d.kind, NativeTypeKind::Struct { .. }))
        .collect();

    if struct_decls.is_empty() {
        return String::new();
    }

    // 收集所有模块的 struct 名，供字段类型映射时判定契约 struct 引用
    let struct_names: Vec<String> = struct_decls.iter().map(|d| d.name.to_string()).collect();

    let mut out = String::new();
    out.push_str("; ---- Native contract struct types (RFC 016 M3) ----\n");
    for d in &struct_decls {
        if let NativeTypeKind::Struct { fields } = &d.kind {
            let field_llvms: Vec<String> = fields
                .iter()
                .map(|(_, fty)| native_type_to_llvm(&fty.node, &struct_names))
                .collect();
            out.push_str(&format!(
                "%struct.{} = type {{ {} }}\n",
                d.name,
                field_llvms.join(", ")
            ));
        }
    }
    out
}
