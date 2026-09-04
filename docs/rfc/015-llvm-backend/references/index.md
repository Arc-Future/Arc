# 015 LLVM 原生后端 · 渐进式披露子项（references）

> 本目录承载 [015 LLVM 原生后端(../../015-llvm-backend.md) 的**能力子项**。015 主文档保留架构级表述；深度设计、契约细节下沉至此，按需钻取。**一子项一文档，互不重叠**；子项仅补细节，不与主文档重复表述既有决策。

| 子项 | 内容 | 关联主文档章节 |
|------|------|---------------|
| [覆盖率（LLVM source-based coverage）](coverage.md) | `-fprofile-instr-generate -fcoverage-mapping` 插桩机制、`arc test --coverage` 流程、`.profraw → .profdata → lcov` 报告格式、与 QIF 正交关系、验收标准、非目标、待拍板决策点 | 015 §设计决策 · 调试信息 |
| [IR 映射（IR-first 设计基准）](ir-mapping.md) | IR-first 方法论；逐语言功能的最佳实践 LLVM IR 总表（async→`llvm.coro.*`、异常→`invoke`/`landingpad`、闭包→SROA、内建数学→LLVM intrinsic 等）；当前手工 lowering 差距与验收基准 | 015 §设计决策 · 代码生成路径 |

---

[返回 015 主题入口(../../015-llvm-backend.md) · [返回 RFC 索引](../../index.md)
