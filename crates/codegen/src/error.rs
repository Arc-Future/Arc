//! Codegen 错误类型。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodegenError {
    #[error("LLVM error: {0}")]
    Llvm(String),
    #[error("no main function found (executable projects require exactly one)")]
    NoMain,
    #[error("multiple main functions found: {0} (executable projects require exactly one)")]
    MultipleMain(String),
    #[error("target machine error")]
    TargetMachine,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// 编译期完整性门（tree-shake 闭环）：发射出的 IR 引用了**既未定义也未声明**
    /// 的符号（典型为 reachability 过度裁剪导致 ARC 函数被剪除但仍被引用）。
    /// 具名诊断由 message 携带（以 `arc-prune-001` 开头，供 CLI 渲染）。
    #[error("{0}")]
    Completeness(String),
    /// 非 Windows 目标上的 try/catch 编译门（`arc-eh-001`）。
    ///
    /// Windows SEH 是 1.0 唯一实现的 zero-cost EH 面；POSIX Itanium 属
    /// 里程碑⑨ / 1.1+（RFC 010）。message 携带 `arc-eh-001` 前缀与
    /// 命中函数/源文件（供 CLI 渲染与回归断言）。
    #[error("{0}")]
    UnsupportedTryCatch(String),
}
