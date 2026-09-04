//! Async spill liveness analysis（RFC 009 M3）。
//!
//! 分析 async 函数中跨 await 存活的 locals，对大型 local 标记为 spilled。
//!
//! ## 分析策略
//!
//! MVP 保守策略（与 M2 状态机当前的"全 hoist"一致）：
//! - 所有 async 函数的 local 都视为"跨 await 存活"（当前 E2E testing 用的保守假设）
//! - 对 size > SPILL_THRESHOLD 的 local 标记为 spilled
//! - spilled local 在 env struct 中由值类型替换为 ptr（8B）
//!
//! ## 设计说明
//!
//! 此模块不直接依赖 `mir` crate（typeck 不依赖 mir），而是接受通用参数：
//! - `is_async`: 函数是否 async
//! - `locals`: `(index, TypeId)` 对列表，index 对应 env 字段索引
//!
//! codegen（`emit_async_sm.rs`）按相同规则消费返回的 HashSet<usize>。
//!
//! ## 后续优化（M3+）
//!
//! - 精确 liveness 分析（IN/OUT 交点）：仅 spill 真正跨 await 存活的 locals
//! - 按活跃 case 动态判定 variant spill（仅 spill 最大 case 而非整个 variant）
//!
//! ## 架构红线
//!
//! - 此模块仅标记 spilled set（`HashSet<usize>`），不修改任何 IR 结构
//! - codegen 消费 spill set 决定 env 字段是否使用 ptr 替代值类型
//! - 运行时 ABI 不感知 spill 语义（通用 dtor_fn 机制）

use crate::checker::type_size_table::{TypeSizeTable, SPILL_THRESHOLD};
use ast::TypeId;
use std::collections::HashSet;

/// spill 候选集：跨 await 存活且 size > SPILL_THRESHOLD 的 local 索引集合。
#[derive(Clone, Debug)]
pub struct SpillSet {
    pub spilled: HashSet<usize>,
}

impl SpillSet {
    /// 空集：无 spill。
    pub fn empty() -> Self {
        SpillSet {
            spilled: HashSet::new(),
        }
    }

    /// 判断 local 是否需要 spill。
    pub fn contains(&self, id: &usize) -> bool {
        self.spilled.contains(id)
    }
}

impl Default for SpillSet {
    fn default() -> Self {
        SpillSet::empty()
    }
}

/// 分析 async 函数的 spill 候选。
///
/// MVP 保守策略：所有 async 函数的 local 都跨 await 存活。
/// 仅按 type size 过滤。
///
/// **参数**：
/// - `is_async`: 函数是否异步
/// - `locals`: `(索引, TypeId)` 列表（不含 params，仅真正的 locals）
/// - `type_sizes`: 类型大小表
pub fn analyze_spill_candidates(
    is_async: bool,
    locals: &[(usize, TypeId)],
    type_sizes: &TypeSizeTable,
) -> SpillSet {
    if !is_async {
        return SpillSet::empty();
    }

    let mut spilled = HashSet::new();

    for (idx, ty) in locals {
        // Void 类型不占用实际空间（占位 i32），跳过
        if matches!(ty, TypeId::Void) {
            continue;
        }

        let size = type_sizes.size_of_type_id(ty);
        if size > SPILL_THRESHOLD {
            spilled.insert(*idx);
        }
    }

    SpillSet { spilled }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_spill_for_small_types() {
        let locals = vec![(0, TypeId::Int), (1, TypeId::String)];
        let table = TypeSizeTable::empty(); // empty cache → all ptr=8
        let spill = analyze_spill_candidates(true, &locals, &table);
        // int=4, string=8 → both < 256
        assert!(spill.spilled.is_empty());
    }

    #[test]
    fn test_non_async_skipped() {
        let locals = vec![(0, TypeId::Int)];
        let cache = std::collections::HashMap::new();
        let table = TypeSizeTable { sizes: cache };
        let spill = analyze_spill_candidates(false, &locals, &table);
        assert!(spill.spilled.is_empty());
    }

    #[test]
    fn test_void_skipped() {
        let locals = vec![(0, TypeId::Void)];
        let cache = std::collections::HashMap::new();
        let table = TypeSizeTable { sizes: cache };
        let spill = analyze_spill_candidates(true, &locals, &table);
        assert!(spill.spilled.is_empty());
    }
}
