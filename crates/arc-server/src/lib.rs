//! `arc-server`：Arc LSP 服务化进程（RFC 038）。
//!
//! 通过 JSON-RPC over stdio 暴露 [LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
//! 协议给通用编辑器（VS Code / Vim / Zed / Helix）消费。
//!
//! ## 架构红线（RFC 038 §架构红线）
//!
//! 1. **arc-server 是独立 crate**：不污染编译器核心 7 crate
//! 2. **复用编译器 crate**（M1+）：复用 `crates/parse`/`hir`/`typeck`/`borrowck`/`arcgr`，不重写语义
//! 3. **复用查询层接口**（M1+）：[RFC 034](../../../docs/rfc/034-ai-toolchain-arcgr.md) M3 查询接口是单一实现
//! 4. **单进程多线程**：LSP 协议层单线程接收消息，索引层多线程并发查询
//! 5. **workspace 隔离**（M4+）：多 workspace 独立 `.arcgr` 内存状态，全局缓存 `.ao` 索引共享只读
//! 6. **显式报错**：跨包符号未找到、`.ao` metadata 损坏等场景显式 LSP 错误响应
//! 7. **禁止半成品技术债**：所有里程碑必须功能完整可验证
//!
//! ## M0 范围
//!
//! - `lsp/json_rpc.rs`：JSON-RPC 2.0 编解码 + stdio 传输层（Content-Length header + body）
//! - `lsp/method_dispatcher.rs`：方法路由（`initialize`/`initialized`/`shutdown`/`exit`）
//! - `workspace.rs`：workspace 状态骨架（多 workspace 管理容器）
//! - `main.rs`：stdio 主循环（读取消息 → 分发 → 响应 → shutdown 退出）
//!
//! ## 非目标（M0 不实现）
//!
//! - 任何语义 LSP 方法（M1+）
//! - 加载 `.arcgr`（M1+）
//! - 增量索引（M2）
//! - 跨包查询（M3）
//! - 多 workspace（M4）

pub mod lines;
pub mod lsp;
pub mod project;
pub mod semantic;
pub mod syntax;
pub mod workspace;

pub use lsp::{json_rpc, method_dispatcher};
pub use semantic::{Position, Range, SemanticIndex};
pub use syntax::{TextDocument, TextDocumentContentChangeEvent};
