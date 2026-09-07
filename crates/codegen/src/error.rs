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
    /// 历史：POSIX try/catch 曾以 `arc-eh-001` 硬拒（里程碑⑨落地前）。
    /// Itanium 面已于 0.1 发布前置收口（RFC 010）；保留变体供诊断字符串兼容。
    #[error("{0}")]
    UnsupportedTryCatch(String),
}
