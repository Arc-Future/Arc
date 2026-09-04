//! L2 批量运行时测试助手（门控在 `feature = "full-rt"`）。
//!
//! 移植自 `crates/arc-integration/tests/support.rs` 的 `build_and_run_batch`，
//! 现复用 `lib.rs` 的 [`assert_compiles_and_runs_batch`]（单文件合并策略）：
//! N 个 case → 单次编译 + 单次运行 → [`BatchCaseResult`] 逐 case 断言。

#![cfg(feature = "full-rt")]

use crate::assert_compiles_and_runs_batch_with_deps;

pub struct BatchCase<'a> {
    pub name: &'a str,
    pub src: &'a str,
}

pub struct BatchCaseResult {
    pub name: String,
    pub passed: bool,
    pub stdout: String,
    pub error: Option<String>,
}

pub fn build_and_run_batch(batch: &str, cases: &[BatchCase]) -> Vec<BatchCaseResult> {
    build_and_run_batch_with_deps(batch, cases, &[])
}

/// 批级 std 子库依赖（`extra_deps` 为 `(包名, 相对 std/ 的目录)`；
/// 所有 case 合并为单文件单 arc.toml，依赖天然为批级别）。
pub fn build_and_run_batch_with_deps(
    batch: &str,
    cases: &[BatchCase],
    extra_deps: &[(&str, &str)],
) -> Vec<BatchCaseResult> {
    let case_refs: Vec<(&str, &str)> = cases.iter().map(|c| (c.name, c.src)).collect();
    let results = assert_compiles_and_runs_batch_with_deps(batch, &case_refs, extra_deps);
    results
        .into_iter()
        .map(|r| BatchCaseResult {
            name: r.name,
            passed: r.passed,
            stdout: r.stdout,
            error: r.error,
        })
        .collect()
}

pub fn batch_case_result<'a>(results: &'a [BatchCaseResult], name: &str) -> &'a BatchCaseResult {
    results
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("batch case `{name}` has no result"))
}
