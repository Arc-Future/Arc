//! High-level IR: name resolution and symbol tables.

mod builder;
mod linq_desugar;
mod module;
mod yield_desugar;

pub use builder::{HirBuilder, HirError};
pub use linq_desugar::{desugar_program, desugar_query};
pub use module::*;
pub use yield_desugar::desugar_yield_program;
