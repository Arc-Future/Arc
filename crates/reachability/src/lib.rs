//! `reachability`：L2 入口可达性分析 Pass（RFC 034 M2）。
//!
//! ## 位置
//!
//! 编译流水线：`parse → hir → typeck → reachability → mir → codegen`。
//!
//! 本 crate 位于 `crates/typeck` 与 `crates/mir` 之间，消费 typeck 产出的
//! 带类型 HIR + 全项目符号表 + 跨包 `.ao` metadata `exports[]`，输出
//! `.arcgr` ReferenceGraph 多维字段。
//!
//! ## IR-agnostic 设计
//!
//! 本 crate 为纯算法库，**不直接依赖** `hir` / `typeck` 等 IR crate——
//! 输入是抽象的边集合（`Vec<Edge>`）+ 入口集合（`Vec<EntryPoint>`）+
//! 符号全集 + 虚分派保守组；输出是 `arcgr::ReferenceGraph`。
//!
//! HIR → 边集合的收集逻辑归 `arc` crate 的 `arcgr.rs` 模块（Step 3），
//! 避免 reachability 与具体 IR 形态耦合，符合 Arc 单职责原则。
//!
//! ## 保守策略（RFC 034 M2 设计权威）
//!
//! - **入口标记**：`[Entry]` 属性 + `main` + library exports + FFI exports
//! - **虚分派保守**：接口方法被引用时，所有实现方法保守标记可达
//! - **动态库导出**：通过 `EntryPointKind::LibraryExport` 标记入口
//! - **跨包可达性**：跨包引用目标作为入口的传递闭包正常 BFS 传播
//!
//! ## 输出
//!
//! 四项多维字段（[ReferenceGraph](arcgr::ReferenceGraph)）：
//! - `entry_points`：入口点集合
//! - `reachable_symbols`：可达符号 ID（排序）
//! - `unreachable_symbols`：不可达符号 ID（排序）
//! - `edges`：引用图边集

pub mod analyzer;

pub use analyzer::{analyze, AnalysisInput, AnalysisReport, VirtualDispatchGroup};
