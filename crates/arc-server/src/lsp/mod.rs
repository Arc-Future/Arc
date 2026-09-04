//! `lsp` 模块：LSP 3.17 协议层。
//!
//! - [`json_rpc`]：JSON-RPC 2.0 编解码 + stdio 传输层
//! - [`method_dispatcher`]：方法路由（M0 仅 `initialize`/`initialized`/`shutdown`/`exit`）

pub mod json_rpc;
pub mod method_dispatcher;
