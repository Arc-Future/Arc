//! Type checker for Arc with IEnumerable / IQueryable dual-path dispatch.
mod attr_table;
/// Typed HIR 借用检查（MIR lower 之前）。
mod borrow;
/// Stub facade 类名 SSoT（typeck / MIR / codegen 契约）。
mod builtin_facade;
mod call_args;
mod check_call_bind;
mod check_expr;
mod check_stmt;
mod checker;
/// RFC 004 §D9 / RFC 037 M2：隐式 variant 构造（typeck AST 重写）。
mod coerce_variant;
/// RFC 017 残余：集合表达式目标类型 `List<T>`（`[…]` → 数组中转 + Add）。
mod collection_expr_list;
mod comptime;
mod error;
/// RFC 017 M4-link Phase B: 跨 `.ao` 包符号注册。
mod external_symbols;
/// RFC 005 里程碑④：编译期声明级字段环检测（`arc-cycle-001` warning 通道）。
mod field_cycle;
mod field_keyword;
mod generics;
mod layout;
/// RFC 009 M4: 宏特性代码注入体系——typeck 侧识别与目录构建。
mod macro_eval;
mod match_pat;
mod method_group; // RFC 008：方法组 → 委托（自由函数 / 静态 / 实例脱糖为 lambda）。
mod null_flow;
mod oop_types;
mod operator_overload; // RFC 003：用户运算符重载 → `op_*` 静态调用脱糖。
mod out_flow;
/// RFC 006 M2：record `with` / 值相等重写。
mod record_m2;
pub mod registry;
/// RFC 006：目标类型 `new()`（typeck AST 填类型）。
mod target_typed_new;
mod type_id;
mod typed;

pub use attr_table::{
    AttributeTable, AttributeTarget, AttributeTargetsBit, BuiltinMeta, ResolvedArg,
    ResolvedAttribute, BUILTIN_ATTR_TYPE,
};
pub use borrow::{BorrowChecker, BorrowError};
pub use builtin_facade::{
    classify_builtin_facade, codegen_handler_hint, is_builtin_facade, split_qualified_method,
    BuiltinFacadeKind,
};
/// 委托 mangle 名的递归 demangle（与 `mangle_type_suffix` 互逆）——mir/codegen
/// 侧委托形参/返回类型解析复用（嵌套 `Func_`/`Action_` 组单一事实源）。
pub use check_expr::{demangle_func_type_depth, demangle_func_type_with};
pub use checker::check_async_spill::{analyze_spill_candidates, SpillSet};
pub use checker::type_size_table::{TypeSizeTable, SPILL_THRESHOLD};
/// RFC 009 M4-7: typeck Pass 模式（Skeleton = Pass 2 骨架，Full = Pass 4 完整）。
pub use checker::MacroPassMode;
pub use checker::TypeChecker;
pub use error::{TypeError, TypeWarning};
pub use external_symbols::{
    ExternalSymbolEntry, ExternalSymbolKind, ExternalTypeRef, ExternalVariantCase,
};
pub use generics::{
    mangle_generic, mangle_type_suffix, resolve_instantiated_type_name, type_id_to_field_name,
};
pub use layout::{
    abi_size_align, abi_size_of, layouts_from_registry, ClassLayout, FieldLayout, InterfaceLayout,
    MethodLayout, ProgramLayouts, PropertyLayout, StaticFieldLayout, StructLayout, VariantLayout,
    VirtualSlot, HEADER_SIZE,
};
pub use macro_eval::{
    evaluator::{make_generator_context, EvalError, Evaluator, Value},
    splice::{parse_expansion, rewrite_program_span, SpliceError},
    whitelist::Whitelist,
    MacroCatalog, MacroContainer, MacroFeature, MacroFeatureCtor, MacroFeatureCtorParam,
    MacroRegistration, MacroSlot, SourceGenerator,
};
pub use oop_types::*;
pub use type_id::{LinqPath, TypeId};
pub use typed::{ExprTypeTable, FnLinkage, TypedBlock, TypedExpr, TypedFn, TypedStmt};
#[cfg(test)]
mod tests;
