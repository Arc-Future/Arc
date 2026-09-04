//! Mid-level IR for Arc.
//!
//! LINQ / ExpressionTree lowering follows the compile-time expansion contract
//! (`docs/rfc/011-expression-trees-query.md`, RFC 011):
//! - `LinqChain` + `LinqForeach`: Enumerable path; consumed by codegen for specialized loops.
//! - `ExpressionTreeConst`: Queryable path; tree is input to codegen rodata emission only—
//!   not an instruction to interpret the tree at user-program runtime.
//!
//! **对外契约**：`lower_module` 仅产出 [`MirCfgBody`]。嵌套 If/While 在 crate
//! 内部经 `to_cfg` 展平；`TryCatch`/`LinqForeach` 等为有意保留的 region 语句。

mod arc_optimize;
mod cfg;
mod liveness;
mod lower;
mod types;

pub mod dataflow;
pub mod field_check;

pub use arc_optimize::{find_byref_captured_locals, find_dead_arc_locals};
pub use ast::TypeId;
pub use liveness::cross_await_live_locals;
pub use lower::{
    collect_concrete_class_refs, collect_iface_generic_instances, lower_module,
    resolve_generic_class_template_by_name,
};
pub use types::*;
