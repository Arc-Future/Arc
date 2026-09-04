//! RFC 017 M4-link Phase B：跨 `.ao` 包外部符号的 LLVM IR `declare` 发射。
//!
//! 当主程序 `arc build --dep <AO>` 时，typeck 从 `.ao` `exports[]` 注册的
//! 方法/自由函数符号在外部 lib.o 中定义。codegen 在用户 `.o` 的 IR 中为
//! 这些外部函数发射 `declare <ret> @<symbol>(<params>)`，使链接器在
//! `link_objects_to_executable` 阶段能从 lib.o 解析符号定义。
//!
//! ## 发射的符号种类
//!
//! - `Method`/`StaticMethod`/`Function`：按签名发射 `declare`
//! - `Class`/`Struct`：发射默认 ctor `declare void @__ctor_<Class>(ptr)`
//!   ——消费方 `new Calculator()` 会调用 `@__ctor_Calculator`，
//!   其定义来自 lib.o（用户自定义 ctor）或 lib.o 的 emit_module 第6步
//!   默认空 ctor（无显式 ctor 时）。无论哪种，消费方均 declare 即可。
//!
//! 仅函数符号需要 declare；类型条目（Interface/Enum/Variant/Module）无需 declare。
//!
//! ## 符号 mangling
//!
//! - `Method`/`StaticMethod`：`name = "<class>.<method>"` →
//!   `mangle_method(class, method)` = `"<class>_<method>"`（与 emit_call 一致）
//! - `Function`：`name = "<fqn>"`，按 `::` → `_` 替换 + `.` → `_` 处理
//!   （自由函数 export_collector 暂未导出，此处为前瞻性支持）
//! - `Class`/`Struct` ctor：`__ctor::<Class>` → `mangle_fn_name` → `__ctor_<Class>`
//!
//! ## LLVM 类型映射
//!
//! | ExternalTypeRef | LLVM type |
//! |-----------------|-----------|
//! | Int             | i32       |
//! | Long            | i64       |
//! | Float           | float     |
//! | Double          | double    |
//! | Bool            | i1        |
//! | String/Null/Object/Named/... | ptr |
//! | Unit            | void      |
//!
//! 复合类型（List/Variant/GenericParam）兜底为 `ptr`——跨包方法调用的
//! 参数/返回类型只可能是基元或用户类型指针，复合类型在 M4-link Phase B
//! 阶段不参与跨 .ao 符号调用。

use typeck::{ExternalSymbolEntry, ExternalSymbolKind, ExternalTypeRef};

use super::mangle::{mangle_fn_name, mangle_method};

/// 把 `ExternalSymbolEntry` 列表转为 LLVM IR `declare` 语句字符串。
///
/// 跳过非函数条目（Interface/Enum/Variant/Module/Field/Constant/Property）
/// ——这些不是函数符号，无需 declare。Class/Struct 额外发射默认 ctor declare。
///
/// 同时跳过 builtin facade 类（Console/File/Directory/...）的方法——
/// 这些方法由 codegen 拦截器（`try_emit_console_static` 等）直接发射
/// `define`，若再发射 `declare` 会触发 LLVM `invalid redefinition` 错误。
/// 即便 `.ao` 包含这些条目（因 `export_collector` 不感知 builtin 状态），
/// codegen 也应过滤掉避免重复定义。
pub fn emit_external_decls(
    entries: &[ExternalSymbolEntry],
    local_symbols: &std::collections::HashSet<String>,
) -> String {
    let mut out = String::new();
    let has_any = entries.iter().any(|e| {
        (is_function_symbol(e.kind) || is_ctor_entry(e.kind) || is_class_with_ctor(e.kind))
            && !is_builtin_entry(e)
            && !is_locally_defined(e, local_symbols)
    });
    if !has_any {
        return out;
    }
    out.push_str("; ---- RFC 017 M4-link Phase B: cross-.ao external symbols ----\n");
    // RFC 038 M2：同名重载（同一类同一方法名、不同形参数量）会 mangle 出同一符号
    // 却带不同签名。LLVM 对同符号多次 declare（不同签名）报 `invalid redefinition`。
    // 接口抽象方法（如 `IFormattable.ToString()` 与 `IFormattable.ToString(string,
    // IFormatProvider)`）重载在跨包消费中经虚表分派，从不按符号直调，故按符号名去重、
    // 只保留首个 declare 即可——定义由链接的 lib.o 提供，签名与未直调符号无关。
    // 同名实方法若需按符号直调且多签名，应走 arity mangling（超出本函数职责）。
    //
    // 本地已定义符号（`local_symbols`）：std 子库消费核心 Arc 时，注入源码
    // （`Arc.Collections`）或模板单态化会在本模块发射 `define`（Weak/Lazy/
    // TaskCompletionSource 等跨包泛型的单态化体，多为 `linkonce_odr` 弱符号）。
    // 对这些符号再 declare 会与本地 define 冲突（LLVM `invalid redefinition`）——
    // 本模块已提供定义，无需外部解析，故跳过。
    let mut emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in entries {
        if is_builtin_entry(entry) || is_locally_defined(entry, local_symbols) {
            continue;
        }
        if is_function_symbol(entry.kind) {
            // 先计算符号名用于去重（与发射一致）。
            let Some(symbol) = decl_symbol_for_entry(entry) else {
                continue;
            };
            if !emitted.insert(symbol.clone()) {
                // 同名符号已 declare——跳过后续重载，避免 `invalid redefinition`。
                continue;
            }
            if let Some(line) = emit_decl_for_entry(entry) {
                out.push_str(&line);
                out.push('\n');
            }
            continue;
        }
        // RFC 038 M2：构造函数条目——发射 arity-mangled ctor declare。
        //   `__ctor_<Class>`（无参）/ `__ctor_<Class>_<arity>`（有参），
        //   signature = (ptr receiver, <params>)。定义来自被链接的 lib.o。
        if entry.kind == ExternalSymbolKind::Constructor {
            let Some(line) = emit_ctor_decl(entry) else {
                continue;
            };
            let symbol = decl_ctor_symbol(entry).unwrap_or_default();
            if !emitted.insert(symbol) {
                continue;
            }
            out.push_str(&line);
            out.push('\n');
            continue;
        }
        // Class/Struct：发射默认 ctor declare（`__ctor_<Class>`）
        if is_class_with_ctor(entry.kind) {
            let ctor_mangled = mangle_fn_name(&format!("__ctor::{}", entry.name));
            if !emitted.insert(ctor_mangled.clone()) {
                continue;
            }
            out.push_str(&format!("declare void @{ctor_mangled}(ptr)\n"));
        }
    }
    out
}

/// 判断条目要发射的符号是否已在当前模块本地定义（`fns` 单态化/注入源码产物）。
///
/// 符号名计算与 [`decl_symbol_for_entry`] / [`decl_ctor_symbol`] / Class ctor 分支
/// 完全一致；本地符号集由调用方从 `fns`（`mangle_fn_name`）构建，二者同源。
fn is_locally_defined(
    entry: &ExternalSymbolEntry,
    local_symbols: &std::collections::HashSet<String>,
) -> bool {
    if local_symbols.is_empty() {
        return false;
    }
    match entry.kind {
        ExternalSymbolKind::Method
        | ExternalSymbolKind::StaticMethod
        | ExternalSymbolKind::Function => {
            decl_symbol_for_entry(entry).is_some_and(|s| local_symbols.contains(&s))
        }
        ExternalSymbolKind::Constructor => {
            decl_ctor_symbol(entry).is_some_and(|s| local_symbols.contains(&s))
        }
        ExternalSymbolKind::Class | ExternalSymbolKind::Struct => {
            let ctor = mangle_fn_name(&format!("__ctor::{}", entry.name));
            local_symbols.contains(&ctor)
        }
        _ => false,
    }
}

fn is_function_symbol(kind: ExternalSymbolKind) -> bool {
    matches!(
        kind,
        ExternalSymbolKind::Method
            | ExternalSymbolKind::StaticMethod
            | ExternalSymbolKind::Function
    )
}

/// 是否为构造函数条目（RFC 038 M2）。
fn is_ctor_entry(kind: ExternalSymbolKind) -> bool {
    kind == ExternalSymbolKind::Constructor
}

/// 计算构造函数条目将发射的 LLVM 符号名（供去重；与 [`emit_ctor_decl`] 一致）。
///
/// 按 arity mangling（与 codegen `emit_new`/`attr.rs` 一致）：
/// - 无参：`__ctor::<Class>` → `__ctor_<Class>`
/// - 有参：`__ctor::<Class>_<arity>` → `__ctor_<Class>_<arity>`
fn decl_ctor_symbol(entry: &ExternalSymbolEntry) -> Option<String> {
    let (class, _) = entry.name.rsplit_once('.')?;
    let arity = ctor_arity(entry)?;
    let raw = if arity == 0 {
        format!("__ctor::{class}")
    } else {
        format!("__ctor::{class}_{arity}")
    };
    Some(mangle_fn_name(&raw))
}

/// 提取构造函数形参数量；签名非 Method 返回 `None`。
fn ctor_arity(entry: &ExternalSymbolEntry) -> Option<usize> {
    match &entry.type_sig {
        ExternalTypeRef::Method { params, .. } => Some(params.len()),
        _ => None,
    }
}

/// 为单个构造函数条目发射 arity-mangled ctor `declare` 行。
/// 返回 `None` 表示签名无法解析（该条目本就跳过）。
fn emit_ctor_decl(entry: &ExternalSymbolEntry) -> Option<String> {
    let (class, _) = entry.name.rsplit_once('.')?;
    let ExternalTypeRef::Method { params, .. } = &entry.type_sig else {
        return None;
    };
    let arity = params.len();
    let raw = if arity == 0 {
        format!("__ctor::{class}")
    } else {
        format!("__ctor::{class}_{arity}")
    };
    let symbol = mangle_fn_name(&raw);
    // ctor 包含 receiver 作第 0 个参数（与 emit_new 一致）
    let mut all_params = Vec::with_capacity(params.len() + 1);
    all_params.push(llvm_type_of(&ExternalTypeRef::Named {
        fqn: class.to_string(),
        generic_args: vec![],
    }));
    for p in params {
        all_params.push(llvm_type_of(p));
    }
    let params_str = all_params.join(", ");
    Some(format!("declare void @{symbol}({params_str})"))
}

/// 判断是否为需要发射 ctor declare 的类型条目（Class/Struct）。
///
/// Interface/Enum/Variant 不可实例化（无 ctor）；Module 是 static class
/// （C# static class 无实例 ctor）。仅 Class/Struct 可被 `new` 实例化，
/// 需要发射 `declare void @__ctor_<Class>(ptr)` 供 `emit_new` 调用解析。
fn is_class_with_ctor(kind: ExternalSymbolKind) -> bool {
    matches!(kind, ExternalSymbolKind::Class | ExternalSymbolKind::Struct)
}

/// 判断条目是否属于 builtin facade 类（Console/File/Directory/...）。
///
/// 这些类的方法由 codegen 拦截器直接发射 `define`，跨 `.ao` 不应再 declare。
fn is_builtin_entry(entry: &ExternalSymbolEntry) -> bool {
    match entry.kind {
        ExternalSymbolKind::Method | ExternalSymbolKind::StaticMethod => {
            // name = "<class>.<method>"
            let Some((class, _)) = entry.name.rsplit_once('.') else {
                return false;
            };
            typeck::is_builtin_facade(class)
        }
        // RFC 038 M2：构造函数条目 `name = "<class>.ctor"`——builtin facade 类的
        // ctor 由 emit_module 第6步发射 `define linkonce_odr`，跨 .o 可消解，
        // 无需 declare（同 Class/Struct 分支理由）。
        ExternalSymbolKind::Constructor => {
            let Some((class, _)) = entry.name.rsplit_once('.') else {
                return false;
            };
            typeck::is_builtin_facade(class)
        }
        // Class/Struct ctor 也按 builtin 过滤——builtin facade 类的 ctor
        // 由 emit_module 第6步发射 `define linkonce_odr`，跨 .o 重复可消解，
        // 无需 declare（declare + linkonce_odr define 会让链接器优先选 linkonce_odr）。
        // 但 declare 一个被 linkonce_odr 定义覆盖的符号在 LLVM 中是合法的；
        // 为避免冗余 declare，仍跳过。
        ExternalSymbolKind::Class | ExternalSymbolKind::Struct => {
            typeck::is_builtin_facade(&entry.name)
        }
        _ => false,
    }
}

/// 计算函数条目将发射的 LLVM 符号名（供去重；与 [`emit_decl_for_entry`] 一致）。
/// 返回 `None` 表示签名无法解析（该条目本就跳过）。
fn decl_symbol_for_entry(entry: &ExternalSymbolEntry) -> Option<String> {
    match entry.kind {
        ExternalSymbolKind::Method | ExternalSymbolKind::StaticMethod => {
            let (class, method) = entry.name.rsplit_once('.')?;
            Some(mangle_method(class, method))
        }
        ExternalSymbolKind::Function => Some(mangle_fn_name(&entry.name.replace('.', "::"))),
        _ => None,
    }
}

/// 为单个外部符号条目发射 `declare` 行。返回 `None` 表示签名无法解析。
fn emit_decl_for_entry(entry: &ExternalSymbolEntry) -> Option<String> {
    let (symbol, params, ret) = match entry.kind {
        ExternalSymbolKind::Method | ExternalSymbolKind::StaticMethod => {
            // name = "<class>.<method>"
            let (class, method) = entry.name.rsplit_once('.')?;
            let symbol = mangle_method(class, method);
            let ExternalTypeRef::Method {
                receiver,
                params,
                ret,
                ..
            } = &entry.type_sig
            else {
                return None;
            };
            // 方法符号包含 receiver 作第 0 个参数（与 emit_call 一致）
            let mut all_params = Vec::with_capacity(params.len() + 1);
            all_params.push(llvm_type_of(receiver));
            for p in params {
                all_params.push(llvm_type_of(p));
            }
            (symbol, all_params, llvm_type_of(ret))
        }
        ExternalSymbolKind::Function => {
            // 自由函数：name 为 FQN（如 `Lib.helper`），按 `::` → `_` + `.` → `_`
            // 兜底为函数符号。自由函数目前 export_collector 未导出，此处为前瞻性支持。
            let mangled = mangle_fn_name(&entry.name.replace('.', "::"));
            let ExternalTypeRef::Func { params, ret, .. } = &entry.type_sig else {
                return None;
            };
            let params: Vec<String> = params.iter().map(llvm_type_of).collect();
            (mangled, params, llvm_type_of(ret))
        }
        _ => return None,
    };
    let params_str = params.join(", ");
    Some(format!("declare {ret} @{symbol}({params_str})"))
}

/// 把 `ExternalTypeRef` 映射为 LLVM IR 类型字符串。
fn llvm_type_of(ty: &ExternalTypeRef) -> String {
    match ty {
        ExternalTypeRef::Int | ExternalTypeRef::UInt | ExternalTypeRef::UShort => "i32".into(),
        ExternalTypeRef::Long | ExternalTypeRef::ULong => "i64".into(),
        ExternalTypeRef::SByte => "i8".into(),
        ExternalTypeRef::Float => "float".into(),
        ExternalTypeRef::Double => "double".into(),
        ExternalTypeRef::Bool => "i1".into(),
        ExternalTypeRef::String
        | ExternalTypeRef::Null
        | ExternalTypeRef::Object
        | ExternalTypeRef::Named { .. }
        | ExternalTypeRef::GenericParam { .. }
        | ExternalTypeRef::List { .. }
        | ExternalTypeRef::Variant { .. }
        | ExternalTypeRef::Func { .. } => "ptr".into(),
        ExternalTypeRef::Unit => "void".into(),
        // Method 变体在外部函数 declare 中不作为参数/返回类型出现，
        // 此处兜底为 ptr 防御性返回。
        ExternalTypeRef::Method { .. } => "ptr".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use typeck::{ExternalSymbolEntry, ExternalSymbolKind, ExternalTypeRef};

    /// 空本地符号集——测试默认「全部视为外部」。
    fn no_locals() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    #[test]
    fn empty_entries_emit_nothing() {
        let out = emit_external_decls(&[], &no_locals());
        assert!(out.is_empty());
    }

    #[test]
    fn only_type_entries_emit_nothing() {
        // Interface/Enum/Variant/Module 是纯类型条目——既非函数符号也非
        // 可实例化类型（Class/Struct），不发射任何 declare。
        //
        // 注意：Class/Struct 会发射默认 ctor declare
        // (`declare void @__ctor_<Class>(ptr)`)——这是 RFC 017 M4-link Phase B
        // 的设计：消费方 `new Calculator()` 需要 declare `__ctor_Calculator`
        // 符号。本测试用 Interface 验证纯类型条目路径（非 Class/Struct）。
        let entry = ExternalSymbolEntry {
            name: "IQueryable".into(),
            kind: ExternalSymbolKind::Interface,
            visibility: ast::Visibility::Public,
            type_sig: ExternalTypeRef::Named {
                fqn: "IQueryable".into(),
                generic_args: vec![],
            },
        };
        let out = emit_external_decls(&[entry], &no_locals());
        assert!(out.is_empty());
    }

    #[test]
    fn method_entry_emits_declare_with_receiver() {
        let entry = ExternalSymbolEntry {
            name: "Calculator.Compute".into(),
            kind: ExternalSymbolKind::Method,
            visibility: ast::Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: "Calculator".into(),
                    generic_args: vec![],
                }),
                params: vec![ExternalTypeRef::Int],
                ret: Box::new(ExternalTypeRef::Int),
                is_virtual: false,
            },
        };
        let out = emit_external_decls(&[entry], &no_locals());
        assert!(
            out.contains("declare i32 @Calculator_Compute(ptr, i32)"),
            "expected Calculator_Compute declare, got: {out}"
        );
    }

    #[test]
    fn static_method_entry_emits_declare() {
        let entry = ExternalSymbolEntry {
            name: "Foo.StaticM".into(),
            kind: ExternalSymbolKind::StaticMethod,
            visibility: ast::Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: "Foo".into(),
                    generic_args: vec![],
                }),
                params: vec![],
                ret: Box::new(ExternalTypeRef::String),
                is_virtual: false,
            },
        };
        let out = emit_external_decls(&[entry], &no_locals());
        assert!(
            out.contains("declare ptr @Foo_StaticM(ptr)"),
            "expected Foo_StaticM declare, got: {out}"
        );
    }

    #[test]
    fn void_return_emits_void() {
        let entry = ExternalSymbolEntry {
            name: "Logger.Flush".into(),
            kind: ExternalSymbolKind::Method,
            visibility: ast::Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: "Logger".into(),
                    generic_args: vec![],
                }),
                params: vec![],
                ret: Box::new(ExternalTypeRef::Unit),
                is_virtual: false,
            },
        };
        let out = emit_external_decls(&[entry], &no_locals());
        assert!(
            out.contains("declare void @Logger_Flush(ptr)"),
            "expected void Logger_Flush, got: {out}"
        );
    }

    #[test]
    fn overloaded_methods_dedup_symbol_into_single_declare() {
        // RFC 038 M2：同名重载（同符号不同签名）——`IFormattable.ToString()` 与
        // `IFormattable.ToString(string, IFormatProvider)` 都 mangle 成
        // `IFormattable_ToString`。若重复 declare（不同签名），LLVM 报
        // `invalid redefinition`。接口抽象方法经虚表分派、从不按符号直调，
        // 故去重只保留首个 declare 即可。
        let entry0 = ExternalSymbolEntry {
            name: "IFormattable.ToString".into(),
            kind: ExternalSymbolKind::Method,
            visibility: ast::Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: "IFormattable".into(),
                    generic_args: vec![],
                }),
                params: vec![],
                ret: Box::new(ExternalTypeRef::String),
                is_virtual: true,
            },
        };
        let entry2 = ExternalSymbolEntry {
            name: "IFormattable.ToString".into(),
            kind: ExternalSymbolKind::Method,
            visibility: ast::Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: "IFormattable".into(),
                    generic_args: vec![],
                }),
                params: vec![
                    ExternalTypeRef::String,
                    ExternalTypeRef::Named {
                        fqn: "IFormatProvider".into(),
                        generic_args: vec![],
                    },
                ],
                ret: Box::new(ExternalTypeRef::String),
                is_virtual: true,
            },
        };
        let out = emit_external_decls(&[entry0, entry2], &no_locals());
        // 同一符号只 declare 一次，不得出现 `invalid redefinition`。
        let count = out.matches("@IFormattable_ToString").count();
        assert_eq!(count, 1, "expected single declare, got: {out}");
        assert!(
            out.contains("declare ptr @IFormattable_ToString(ptr)"),
            "expected first overload declare kept, got: {out}"
        );
    }

    #[test]
    fn locally_defined_symbols_skip_declare() {
        // 跨包消费时注入源码 / 模板单态化会在本模块发射 `define`（如
        // `__ctor_Weak_1`、`TaskCompletionSource_bool_SetResult`）——对这些符号
        // 再 declare 会与本地 define 冲突（LLVM `invalid redefinition`），须跳过。
        let ctor = ExternalSymbolEntry {
            name: "Weak.ctor".into(),
            kind: ExternalSymbolKind::Constructor,
            visibility: ast::Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: "Weak".into(),
                    generic_args: vec![],
                }),
                params: vec![ExternalTypeRef::Named {
                    fqn: "SomeUserType".into(),
                    generic_args: vec![],
                }],
                ret: Box::new(ExternalTypeRef::Unit),
                is_virtual: false,
            },
        };
        let method = ExternalSymbolEntry {
            name: "TaskCompletionSource_bool.SetResult".into(),
            kind: ExternalSymbolKind::Method,
            visibility: ast::Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: "TaskCompletionSource_bool".into(),
                    generic_args: vec![],
                }),
                params: vec![ExternalTypeRef::Bool],
                ret: Box::new(ExternalTypeRef::Unit),
                is_virtual: false,
            },
        };
        // 本地符号集 = 本模块发射的 define 符号（MIR 名 mangled 后与 declare 同源）。
        let mut locals = no_locals();
        locals.insert("__ctor_Weak_1".to_string());
        locals.insert("TaskCompletionSource_bool_SetResult".to_string());
        let out = emit_external_decls(&[ctor, method], &locals);
        assert!(
            !out.contains("__ctor_Weak_1") && !out.contains("TaskCompletionSource_bool_SetResult"),
            "locally-defined symbols must not be declared, got: {out}"
        );
    }
}
