//! Abstract syntax tree for the Arc language.

mod expr;
mod expr_tree;
mod items;
mod native;
mod span;
mod stmt;
mod ty;
mod type_id;

pub use expr::*;
pub use expr_tree::*;
pub use items::*;
pub use native::*;
pub use span::*;
pub use stmt::*;
pub use ty::*;
pub use type_id::*;

pub use smol_str::SmolStr;

pub type Ident = SmolStr;
