# UnitTest.Deferred

L3 **已解禁**（2026-07-30 · [RFC 036](../../docs/rfc/036-maturity.md)）后，本目录仍暂存未达契约门槛的测试（不随 examples/UnitTest 默认跑）。

领域：P2P/Net/UI/DI 等（L3）。**保持诚实隔离**（有边界 Sprint 另排；**≠** 假开全家桶；**≠** M4）；禁止 Skip 顶绿。默认验证入口见 [examples/README.md](../README.md) / 示例保留政策（仓库开发纪律文档）。

已迁回核心（不再作为 Deferred 缺口）：Expression.Eval*、跨文件 partial、Tasks/EventLoop/QIF async Fact、Threading（含 ManagedThreadId）、Signal（含 OnChanging）、Variant（UnitTest/Arc/VariantTests.as）、QIF Assert 通过路径 + Assert.Skip（UnitTest/QIF/AssertTests.as，无 [Fact(Skip)]）、**Reflection M3+**（UnitTest/Core/ReflectionTests.as：typeof / TypeId / Name / GetMethods·GetFields·GetProperties 名枚举）。Concurrent / Security 与核心套件重复副本已删除。

仍挂账：**无 L2 Core Deferred**。OopGenericInherit / Variant / ContentVariant（D9 ContentLike）/ Reflection M2 已迁回核心。RFC 052 Name/FullName/GetMethods·GetFields·GetProperties 名枚举 = 工具链 M3+（非本目录 Skip 挂账）。

## OopBug / Variant / ContentVariant（本刀）

| 切片 | 状态 | 说明 |
|------|------|------|
| OopGenericInherit（基类泛型实例方法） | ✅ 已迁回 `UnitTest/Core/OopGenericInheritTests.as` | 无 Skip |
| Variant 基础构造/switch | ✅ 已迁回 `UnitTest/Arc/VariantTests.as` | 无 Skip |
| Content-like §D9 语言切片 | ✅ 已迁回 `UnitTest/Arc/ContentVariantTests.as` | 本地 `ContentLike`：let/return/自由函数与实例方法实参/字段/属性/`switch (prop)`；无 Skip |
| SimpleContent / Arc.UI.Content 集成 | ⛔ 硬阻塞（L3） | 依赖 `Arc.UI`；且 `Content` 同时有 `Text of string` 与 `Resource of string`，按 RFC 031 §D9 歧义规则**拒绝** `string` 隐式构造。禁止 Skip 顶绿；**禁**回迁核心套件 |

编译器修复（同变更集）：候选形参含 variant 时实例方法走 `bind`+§D9；公开字段赋值补隐式构造；`switch` scrutinee 对 custom-accessor 属性走 getter（避免 FieldGet 回退 `int`）。

## Tasks / EventLoop

| 切片 | 状态 | 说明 |
|------|------|------|
| QIF `[Fact] async` + EventLoop Delay | ✅ 已迁回 `UnitTest/Arc/EventLoopTests.as` | 宿主消费 `is_async` 生成 `await`；含 async 时强制串行 + `Environment.Exit` 传失败码 |
| 同步 Task API | ✅ `UnitTest/Arc/TaskTests.as` | FromResult / WhenAll / Cancel |
| e2e | ✅ 无 Skip | `async_tasks_e2e` / `event_loop_e2e` / `cancellation_e2e` / `task_api_e2e` |

L2 Tasks **Stable** 面见 [RFC 009](../../docs/rfc/009-async-concurrency.md)。禁止用 Skip 顶绿。


## Threading / Signal / Parallel 分流（L2 · 诚实标注）

| 切片 | 状态 | 说明 |
|------|------|------|
| Parallel（Semaphore / Monitor / Lock） | ✅ 已在 `UnitTest/Arc/ParallelTests.as` | Deferred 重复副本已删 |
| Threading Mutex / Sleep(0) / ManagedThreadId | ✅ 已迁回 `UnitTest/Arc/ThreadingTests.as` | 非 Skip e2e；静态属性 MIR→`Thread.ManagedThreadId`→`rt_thread_current_id` |
| Signal 构造 / Value / TrySet / Set / OnChanging / 泛型工厂 | ✅ 已迁回 `UnitTest/Arc/SignalTests.as` | 非 Skip；真实 `__ctor_Signal_*`；`List<Func_*>` 非 ARC + mangled Func 间接调用 ptr ABI |

已迁回核心套件：ObjectModel、Variance、Expression.Eval*、Variant、QIF Assert（通过路径）；见 RFC 009 表。

核心套件禁止 `[Fact(Skip)]` 掩盖缺口。`Assert.Skip` 运行时跳过（`QIF_SKIP:`）由 `AssertTests.Skip_RecordsAsSkipped` 验证，记为 Skipped 而非 Fail——**≠** Fact Skip；integrate 核心 `UnitTest`：**449 / 448 pass / 1 skip**（仅此 `Assert.Skip`；含 Reflection M2）。门禁裁决见 实现规划 里程碑状态。

## 仍 Deferred（诚实阻塞）

| 切片 | 层 | 阻塞 |
|------|----|------|
| ~~`Core/ReflectionTests`~~ | 工具链（RFC 052） | **已迁回** M3+ 最小面（typeof/TypeId/Name/GetMethods·GetFields·GetProperties 名枚举）；残余 PropertyType/自定义属性/继承合并属另轨，**不**阻塞 L2 门禁、**不**用 Skip 挂账 |
| ~~OopBug/OopGenericInheritTests~~ | L1 OOP | **已迁回** UnitTest/Core/OopGenericInheritTests.as（本目录副本已删） |
| `OopBug/SimpleContentTest` | L3 UI | `Arc.UI` 不完整（Rectangle/Shape GetValue）；**禁**回迁顶绿 |
| `Arc/DependencyPropertyTests` | L3 UI | **无 [Fact]**（已去 Skip 假绿）；可证伪 DP 元数据见 `ui_skeleton_honesty_e2e`；GetValue/Window wrapper 另排 |
| ~~Arc/ContentVariantTests~~ | L1/L2 | **已迁回** UnitTest/Arc/ContentVariantTests.as（ContentLike D9；非 Arc.UI ContentControl） |
| `Arc/MinimalUITests` | L3 UI | Deferred 隔离（Window 属性面）；**禁**回迁顶绿；布局/DP 元数据骨架见 `ui_skeleton_honesty_e2e` |
| `Arc/DITests` | L3 DI | 已解禁 · 有边界 Sprint 另排；**禁**回迁顶绿 |
| `Arc/NetworkingTests` | Net MVP | Uri/Cookie 真实断言（非 Fact-Skip）；权威 e2e：`net_e2e`（含 `net_tcp_loopback_mvp`）；Http GET/Dns 仍后置 |
| `P2P*` | L3 Net | **无 [Fact]**（已去 Skip+`Assert.True(true)` 假绿）；可证伪：`l3_honesty_sweep_e2e`；禁开 P2P 新里程碑 |

## Concurrent / Task.Run（L2 残余关账 · wave5）

| 切片 | 状态 | 说明 |
|------|------|------|
| Concurrent* 基本 API | ✅ 核心 `UnitTest/Arc/ConcurrentCollectionTests` | 非 Skip；非压力面 |
| Concurrent* M6 压力 | ✅ Stable | `concurrent_bench_e2e`；禁止 Skip 名义覆盖；ns/C# 为软目标 |
| `Task.Run` 默认池 | ✅ | `task_run_e2e` + TaskTests / EventLoopTests；非 Skip |
| 显式 ThreadPoolScheduler **基本 API** | ✅ Stable | `threadpool_scheduler_e2e` + `ThreadPoolSchedulerTests`；非 Skip |
| ThreadPoolScheduler **Destroy / NUMA ctor / 压力最小 / 协作抢占** | ✅ Stable | 已升格；**非**永久 Draft（见 RFC 009 口径 B） |
| ThreadPool **深度基准**（跨 socket / 强制抢占 / 吞吐对照） | **可选 Q 软目标**（非 Draft） | 移出 L2 关账面；不堵塞口径 A/B |

清残余战役 ✅ 已关账（tip `696b97e`）；非阻塞残余总表见 实现规划 缺陷登记。

## 标准库就绪（对照 tip `82f7f9b`）

本目录**不**承载 L2 Core Deferred。权威勾选见 实现规划（与 [preface](../../docs/preface.md) / 仓库开发纪律文档 同口径）：

| 档 | 状态 |
|----|------|
| P0 硬门槛 | ✅ |
| 治理加深 | ✅（A∩B · B1–B4） |
| 大规模 std 前置条件齐 | ✅ **已齐**（有治理的大规模可开） |
| L3 / 本目录仍 Deferred | ✅ L3 **已解禁**（2026-07-30）但本目录仍诚实隔离；有边界 Sprint 另排；**不计**「大规模齐」；仍禁 Skip 顶绿 |

**判定**：硬门槛 ✅ / 治理加深 ✅（A∩B）——有治理的大规模可开；云 KMS / `arc.toml git=` / 完整 PKI 仍 **C**；L3 **已解禁**（2026-07-30 · 有边界 Sprint；本目录仍隔离；≠假开里程碑；≠业界领先）。

## Arc.Net（Http/Tcp MVP · 诚实）

| 切片 | 状态 | 说明 |
|------|------|------|
| `Arc/NetworkingTests` | MVP 纯逻辑 | Uri/Cookie 真实断言；**已删**「`using Arc.Net not yet available`」Fact-Skip 假绿 |
| 权威 e2e | 已退场 | 原 `crates/arc-integration/tests/net_e2e.rs`（Uri/Cookie/UriBuilder + `net_tcp_loopback_mvp`）随 arc-integration 退场（a2627a0f），未迁入 arc-tests |
| 协议扩张 / P2P 新里程碑 | **禁止** | 本目录 P2P* 仍 Deferred；不在本切片推进 |
