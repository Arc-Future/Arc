//! Native 契约符号表与类型映射（RFC 016 M1/M2 + M3 扩展）。
//!
//! 将 `NativeModule` 转换为 LLVM IR 类型字符串与符号表，供
//! `emit_decl.rs` 生成 `declare`、`emit_call.rs` 生成 `call`、
//! `verify_symbols.rs` 做存在性校验。
//!
//! M1 白名单类型直接映射到 LLVM IR 类型，不依赖 `ProgramLayouts`——
//! 白名单固定且简单，无需复杂类型解析。
//!
//! M3 扩展（RFC 016 M2 同期推进）：`object` → `ptr`（FFI Marshal 专用根类型，
//! 对应 C `void*`）。值类型实参在 codegen 经 `rt_box_create` 装箱为 ptr 传入。
//!
//! M3 扩展（RFC 016 §3.3）：`NativePtr` 透明指针 → `ptr`（按值传递）；
//! 契约 struct → `%struct.<Name>`（按值传递，LLVM struct 类型）。
//! `native_type_to_llvm` 返回 `String` 以支持 `%struct.<Name>` 格式化。
//!
//! M3 扩展（RFC 016 §3.3 List<T> marshal）：`List<T>` 形参展开为
//! `ptr buffer, i32 size` 两个 LLVM 参数（零拷贝）。`param_marshal` 字段
//! 记录每个原始参数的 marshal 策略，`emit_call` 据此生成
//! `rt_list_buffer_and_size` 调用并传递 buffer+size。

use ast::{CallingConv, NativeModule, NativeTypeDecl, NativeTypeKind, ParamDirection, Type};
use std::collections::HashMap;

/// RFC 016 M3 §3.3 List<T> marshal：原始参数的 marshal 策略。
///
/// `param_marshal` 长度 = 原始参数数量（不展开）；`param_llvms` 长度 =
/// 展开后的 LLVM 参数数量。`emit_call` 根据 `param_marshal` 决定每个原始
/// 参数生成多少个 LLVM 参数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ParamMarshal {
    /// 普通参数：按 `param_directions` 决定值传递或传地址。
    /// 对应 1 个 LLVM 参数。
    Normal,
    /// `List<T>` 参数：展开为 `ptr buffer, i32 size` 两个 LLVM 参数。
    /// codegen 在 call 前发射 `rt_list_buffer_and_size(handle, &buf, &size)`
    /// 获取内部 buffer 指针和元素数量，零拷贝传递给 C 函数。
    /// 对应 2 个 LLVM 参数（buffer + size）。
    List,
    /// `byte[]` 参数（RFC 025 S4）：RtArrayHeader 载体，直接传 payload 指针
    /// （header 位于 payload-8）。`In`/`Out`/`InOut` 均传 payload 指针——C shim
    /// 经 `arr_len(data)` 读 header 得知容量并写入 payload，Arc byte[] 变量
    /// 持同一 payload 指针，写入即对 Arc 可见。对应 1 个 LLVM 参数（ptr）。
    ByteArray,
}

/// 单个 native 函数的 LLVM IR 发射信息。
pub(crate) struct NativeSymbolInfo {
    /// C 符号名（如 `puts`）。
    pub symbol: String,
    /// 返回类型 LLVM IR 字符串（如 `i32`、`void`、`ptr`、`%struct.Point`）。
    pub ret_llvm: String,
    /// 参数类型 LLVM IR 字符串列表（展开后：List<T> → 2 项）。
    pub param_llvms: Vec<String>,
    /// 参数方向列表（RFC 016 M2）：与原始参数同序（不展开）。
    /// codegen 据此决定值传递（`In`）还是传地址（`Out`/`InOut`）。
    pub param_directions: Vec<ParamDirection>,
    /// RFC 016 M3：调用约定。codegen 据此在 `declare`/`call` 前 emit `stdcallcc ` 前缀。
    pub calling_conv: CallingConv,
    /// RFC 016 M3 §3.3 List<T> marshal：每个原始参数的 marshal 策略。
    /// 长度 = 原始参数数量。`emit_call` 据此决定参数展开方式。
    pub param_marshal: Vec<ParamMarshal>,
    /// RFC 016 M1：每个原始参数的 callback 类型名（若为 callback 类型）。
    /// 长度 = 原始参数数量。`None` 表示非 callback 参数；`Some(cb_name)` 表示
    /// 该参数类型为 `native callback <cb_name>`，codegen 应生成 trampoline
    /// 将 Arc 函数指针适配到 C ABI。
    pub param_callback_types: Vec<Option<String>>,
}

/// native 方法调用名 → 符号信息映射。
/// 键格式为 `<module>.<fn>`（如 `libc.puts`）。
pub(crate) type NativeSymbolTable = HashMap<String, NativeSymbolInfo>;

/// 判断 `NativeTypeDecl` 是否为 `Struct` 声明（按值传递的契约 struct）。
///
/// `OpaquePtr`（`native type Name;`）按 `ptr` 传递，不进入 struct 名单；
/// 仅 `Struct`（`native type Name { ... };`）需要 emit `%struct.<Name>` 类型。
fn is_struct_decl(d: &NativeTypeDecl) -> bool {
    matches!(d.kind, NativeTypeKind::Struct { .. })
}

/// 判断 AST `Type` 是否为 `List<T>` 泛型实例（RFC 016 M3 §3.3 List<T> marshal）。
///
/// 识别两种形式：
/// - `Type::Named { path: ["List"], generics: [T] }`：`.ani` 原始声明形式。
/// - `Type::Named { path: ["List_<T>"], generics: [] }`：单态化后形式
///   （path.len() == 1 且 name 以 `List_` 开头，如 `List_int`）。
///
/// typeck `lower_type` 在 class_templates 已加载时把 `List<int>` lower 为
/// `TypeId::Named("List_int")`，但 AST `Type` 保持原始 `List<int>` 形式
/// （native contract 的 AST 不会被 lower 改写）。`build_native_symbol_table`
/// 直接遍历 AST `NativeFn.params`，所以主要识别第一种形式。
fn is_list_type(ty: &Type) -> bool {
    match ty {
        Type::Named { path, generics } if path.len() == 1 => {
            let name = path[0].as_str();
            // 原始形式：`List<int>` → path=["List"], generics=[int]
            (name == "List" && !generics.is_empty())
                // 单态化形式：`List_int` → path=["List_int"], generics=[]
                || name.starts_with("List_")
        }
        _ => false,
    }
}

/// RFC 025 S4：判断 AST `Type` 是否为 `byte[]`（RtArrayHeader 载体）。
///
/// 识别 `Type::Array { inner: Named("byte") }`（`byte[]` → parse 把 LBracket 后缀
/// 包成 Array，inner 为 Named("byte")）。`byte[]` 作为 native 形参时直接传
/// payload 指针（`ParamMarshal::ByteArray`），与 rt_crypto_*/rt_quic_* C shim
/// `arr_len(data)` 读 header 的形态一致。
fn is_byte_array_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Array { inner }
            if matches!(&inner.node, Type::Named { path, generics } if path.len() == 1 && path[0] == "byte" && generics.is_empty())
    )
}

/// M1 白名单 AST `Type` → LLVM IR 类型字符串。
///
/// M3 扩展：`object` → `ptr`（对应 C `void*`，FFI Marshal 专用根类型）。
/// M3 扩展（RFC 016 §3.3）：`NativePtr` → `ptr`；契约 struct → `%struct.<Name>`。
/// 非白名单类型回退到 `ptr`（typeck 已在编译期拒绝，此处防御性兜底）。
///
/// 返回 `String` 而非 `&'static str`，因为契约 struct 需要格式化为 `%struct.<Name>`。
pub(crate) fn native_type_to_llvm(ty: &Type, contract_struct_names: &[String]) -> String {
    match ty {
        Type::Named { path, .. } if path.len() == 1 => {
            let name = path[0].as_str();
            match name {
                "int" => "i32".into(),
                "long" => "i64".into(),
                "short" => "i16".into(),
                "byte" => "i8".into(),
                "char" => "i32".into(),
                "float" => "float".into(),
                "double" => "double".into(),
                "bool" => "i1".into(),
                "string" => "ptr".into(),
                // RFC 006 M2 / RFC 016 M3：object 对应 C `void*`
                "object" => "ptr".into(),
                // RFC 016 M3：NativePtr 内置透明指针，对应 C `void*`
                "NativePtr" => "ptr".into(),
                _ => {
                    // RFC 016 M3：契约 struct → %struct.<Name>
                    if contract_struct_names.iter().any(|s| s == name) {
                        format!("%struct.{name}")
                    } else {
                        "ptr".into()
                    }
                }
            }
        }
        Type::Nullable { .. } => "ptr".into(),
        _ => "ptr".into(),
    }
}

/// 从 `NativeModule` 列表构建符号表。
///
/// RFC 016 M3：先收集所有模块的契约 struct 名，供 `native_type_to_llvm` 判定。
/// 这些 struct 名在所有模块间共享（一个模块声明的 struct 可作为另一个模块函数的参数）。
///
/// RFC 016 M1：同时收集所有模块声明的 `native callback` 名，供参数类型判定。
pub(crate) fn build_native_symbol_table(modules: &[NativeModule]) -> NativeSymbolTable {
    // 收集所有模块声明的契约 struct 名（OpaquePtr 不在此列，按 ptr 传递）。
    let struct_names: Vec<String> = modules
        .iter()
        .flat_map(|m| {
            m.types
                .iter()
                .filter(|d| is_struct_decl(d))
                .map(|t| t.name.to_string())
        })
        .collect();
    // RFC 016 M1：收集所有模块声明的 callback 类型名。
    let callback_names: Vec<String> = modules
        .iter()
        .flat_map(|m| m.callbacks.iter().map(|cb| cb.name.to_string()))
        .collect();

    let mut table = HashMap::new();
    for module in modules {
        for fn_decl in &module.functions {
            let key = format!("{}.{}", module.name, fn_decl.name);
            let symbol = fn_decl
                .symbol
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| fn_decl.name.to_string());
            let ret_llvm = match &fn_decl.ret {
                Some(t) => native_type_to_llvm(&t.node, &struct_names),
                None => "void".to_string(),
            };
            // RFC 016 M3 §3.3 List<T> marshal：对 `List<T>` 参数展开为
            // `ptr buffer, i32 size` 两个 LLVM 参数；其他参数保持原样。
            // `param_marshal` 长度 = 原始参数数量，`param_llvms` 长度 = 展开后数量。
            let mut param_llvms: Vec<String> = Vec::new();
            let mut param_marshal: Vec<ParamMarshal> = Vec::new();
            let mut param_callback_types: Vec<Option<String>> = Vec::new();
            for p in &fn_decl.params {
                if is_byte_array_type(&p.ty.node) {
                    // RFC 025 S4：`byte[]` → 单 LLVM 参数 `ptr`（payload 指针）。
                    param_llvms.push("ptr".into());
                    param_marshal.push(ParamMarshal::ByteArray);
                    param_callback_types.push(None);
                } else if is_list_type(&p.ty.node) {
                    param_llvms.push("ptr".into()); // buffer
                    param_llvms.push("i32".into()); // size
                    param_marshal.push(ParamMarshal::List);
                    param_callback_types.push(None);
                } else {
                    param_llvms.push(native_type_to_llvm(&p.ty.node, &struct_names));
                    param_marshal.push(ParamMarshal::Normal);
                    // RFC 016 M1：若参数类型名匹配某 callback 名，记录 callback 类型名。
                    let cb_ty = match &p.ty.node {
                        Type::Named { path, .. } if path.len() == 1 => {
                            let name = path[0].as_str();
                            if callback_names.iter().any(|c| c == name) {
                                Some(name.to_string())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    param_callback_types.push(cb_ty);
                }
            }
            let param_directions = fn_decl.params.iter().map(|p| p.direction).collect();
            table.insert(
                key,
                NativeSymbolInfo {
                    symbol,
                    ret_llvm,
                    param_llvms,
                    param_directions,
                    calling_conv: fn_decl.calling_conv,
                    param_marshal,
                    param_callback_types,
                },
            );
        }
    }
    table
}
