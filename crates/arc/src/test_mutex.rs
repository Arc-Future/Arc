//! 测试互斥：`$ARC_MIRROR`/`$ARC_HOME` 等**全局环境变量**的测试须串行执行。
//!
//! Rust `cargo test` 默认并行运行，`set_var`/`remove_var` 是进程级操作——
//! 并行测试互相污染环境变量导致偶发漂移（如 `pkg_cache` 的
//! `build_resolve_version_miss_is_hard_error` 与 `_restores_from_arc_mirror`
//! 竞态：mirror 测试设置 `$ARC_MIRROR` 的窗口内，hard-error 测试误判
//! 「可从 mirror 恢复」而非 hard error）。所有 env 敏感测试获取此锁串行化。
#[cfg(test)]
pub(crate) static ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
