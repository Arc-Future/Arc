//! Native 契约的 LLVM IR 发射子模块（RFC 016）。
//!
//! 子模块拆分：
//! - [`symbols`]：符号表构建与 AST `Type` → LLVM IR 类型映射
//! - [`emit_decl`]：LLVM `declare` 与契约 struct 类型定义发射
//! - [`link`]：链接器 `-l<name>` 标志注入
//! - [`verify_symbols`]：编译期符号验证（M2 新增）
//! - [`runtime_load`]：RFC 016 运行时库加载统一模型（懒解析器 + 间接调用表）
//!
//! 门面仅 `mod` + `pub(crate) use`，不含实现代码。

mod emit_decl;
mod link;
mod runtime_load;
mod symbols;
pub(crate) mod verify_symbols;

pub(crate) use emit_decl::{emit_native_decls, emit_native_struct_types};
pub(crate) use link::native_link_libs;
pub(crate) use runtime_load::{build_runtime_infos, emit_runtime_load_support, RuntimeModuleInfos};
pub(crate) use symbols::{build_native_symbol_table, NativeSymbolTable, ParamMarshal};
