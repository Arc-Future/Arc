//! RFC 044：yield 迭代器脱糖（AST→AST 状态机合成）。
//!
//! 分层：`entry`（遍历/分类/挂接）、`rename`（提升变量改名+表达式校验）、
//! `cfg`（方法体→微型 CFG）、`emit`（状态机类合成与驱动发射）。

mod cfg;
mod emit;
mod entry;
mod rename;

pub use entry::desugar_yield_program;
