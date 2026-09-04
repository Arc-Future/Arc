# 015 子项 · IR 映射（IR-first 设计基准）

> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令不再可用；现行验证矩阵为
> `cargo test --workspace`（运行时面 `cargo test -p arc-tests --features full-rt`），
> 详见仓库根 `CHANGELOG.md`。

> 本子项定义了 Arc 编译器 **IR-first 方法论**及其产出：对每个语言功能，先确立「最佳实践 LLVM IR 形态」，再据以封装 `codegen` 的发射能力。它是 实现规划 技术债登记栏目。