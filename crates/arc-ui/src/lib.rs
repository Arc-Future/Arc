//! RFC 037：声明式跨平台 GUI 框架——`.arml` 解析、类型检查、AI 协作工具。
//!
//! 本 crate 提供 `.arml`（Arc Markup Language）文件的词法分析、语法解析、
//! 类型检查能力，以及 `arc ui inspect` / `arc ui verify` CLI 工具所需的
//! 结构化输出。编译器核心 7 crate（ast/parse/hir/typeck/mir/codegen/arc）
//! 不含 UI 领域逻辑；本 crate 作为 partial class 的消费者，将 `.arml.as`
//! 用户代码与生成的 `.g.as` 代码合并为同一 partial class（依赖 RFC 037）。
//!
//! 参见 [RFC 037](../../../docs/rfc/037-ui.md)。

mod adaptive;
mod adaptive_lit;
mod ast;
mod builtin_theme_gen;
mod codegen;
mod error;
mod inspect;
mod lexer;
mod parser;
mod projection;
mod projection_arc;
mod typeck;
mod verify;

pub use adaptive::{check_adaptive, check_codebehind_pollution, AdaptiveCheck};
pub use adaptive_lit::ValueType;
pub use ast::*;
pub use builtin_theme_gen::{
    generate_colors_g_as, load_theme_colors, write_colors_g_as, COLORS_G_AS_REL, CONTROLS_ARML_REL,
    DARK_ARML_REL, LIGHT_ARML_REL,
};
pub use codegen::{generate, generate_project, CodegenOptions, GeneratedFile, ProjectOutput};
pub use error::{ArmlError, ArmlResult};
pub use inspect::{ascii_preview, inspect_json};
pub use lexer::{Lexer, Token, TokenKind};
pub use parser::Parser;
pub use projection::{
    build_projection_spec, encode_state, DimKind, DimSpec, ProjectionSpec, TokenProjection,
    UnitCode,
};
pub use projection_arc::render_spec_arc;
pub use typeck::{ComponentInfo, ComponentRegistry, PropType, TypeCheckReport, TypeChecker};
pub use verify::{
    check_codebehind_report, verify_report, verify_report_with_strict, VerificationReport,
};
