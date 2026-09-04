# Arc Agent Harness · 渐进式披露子项（references）

> 本目录承载 [043 Coding Agent Harness 工程(../../043-harness.md) 的**能力子项**。043 主文档保留架构级表述；深度设计下沉至此。**一子项一文档，互不重叠**。

## LLM 必读顺序

动手写 Harness / AIRfc / Coding 相关代码前，按序阅读（与 043 开篇「读前门闩」一致）：

1. [043 主文档(../../043-harness.md) — 读前门闩 · 宣称纪律 · 分层 · 试探≠终态
2. [AIRfc 体系](airfc.md) — 宣称门闩 · 类型契约 · 双 Revision · 禁令
3. [LLM 门闩](llm-gates.md) — 何时可写 / 不可写代码
4. [冲突织物](conflict-fabric.md) — 与 038 Plan / Tool 共用租约
5. [API 草图](api-sketch.md) · [包布局](package-layout.md) — 再动实现
6. [收敛迁移](convergence-migration.md) — 试探类型淘汰清单（禁当终态抄）
7. 按需：[DoD](definition-of-done.md) · [纠偏协议](anchor-correction-protocol.md) · [设计评审](design-review.md) · [协作确认点](collaboration-checkpoints.md) · [测试先行](testing-first.md) · [回合小结](work-summary.md) · [并行子代理](parallel-subagents.md)（P3 设计契约）· [子代理管理](subagent-management.md)（两大机制 · 方案 A：`AISubAgentManager` reconcile 治理）· [冲突分支](conflict-branch.md)（两大机制 · 方案 B：三级冲突统一仲裁 + 分支模型）· [真实场景运转协议](scenario-operation.md)（真实交付场景判定与缺口，文档协议层）· [场景闭环推演验收协议](scenario-drive-acceptance.md)（交付判据 = 五面推演闭环，非测试全绿）· [性能观测与性能信号](performance-observability.md)（`AIPerfMonitor` 设计；增强信号 → P3 D9 门）· [信号日志与 LLM 上下文筛选](signal-log.md)（`AISignalLog` 分级/筛选 + `AIToolOutput` 门面）· [计划树（AIPlan 树形状态树）](plan-tree.md)（`AIPlanNode`/`AIPlanTree`/六态 + 根专属 Verifying）· [Harness LLM 复盘报告](harness-llm-lessons.md)（踩坑清单 A1–A8 + 改进机制 B1–B8 + 落地方案 P0/P1/P2；教训沉淀，非能力契约）

未读完 1–4 即扩写业务代码 = 违反门闩。

## 子项目录

| 子项 | 内容 | 关联主文档 |
|------|------|------------|
| [AIRfc 体系](airfc.md) | 小型 PM / 需求本尊、Spec 面、双 Revision、复用 AIPlan、冲突织物摘要 | 043 宣称纪律 · §2 · §10 |
| [并行子代理（P3）](parallel-subagents.md) | WorkItems → 任务图（DependsOn + Scope）；每工作项一子代理容器；汇总门唯一权威 | 043 §2 · airfc §3 · conflict-fabric · DoD |
| [子代理管理（方案 A）](subagent-management.md) | P3 升级：`AIParallelCoordinator` → `AISubAgentManager` reconcile 循环；生命周期状态机 / 监督恢复 / 消息与决策同步双通道 / 动态派发 / `TotalBudget` 收束梯 / 汇总门增强 | 043 §2 · parallel-subagents · conflict-fabric · scenario-operation（3.1/3.2/3.3/A.9/B11/3.5） |
| [计划树（AIPlan 树形状态树）](plan-tree.md) | `AIPlan` 从扁平 `List<AITaskStep>` 升级为树形状态树：`AIPlanNode`/`AIPlanTree`/`AIPlanNodeStatus` 六态 + 根专属 `Verifying`；`ParentId` 分组树（聚合）+ `DependsOn` 顺序 + fail-closed 聚合；汇总门在根（D0–D7 根聚合过、叶只交小结） | 043 §2 · airfc · parallel-subagents · subagent-management · DoD |
| [冲突分支（方案 B）](conflict-branch.md) | 三级冲突（L1 租约 / L2 Spec 矛盾 / L3 git 合并）统一仲裁（机器检测拒绝 → 人 CCB 裁决，禁自动选胜者）；分支模型 / 合并门 / `AIMergeTransaction` git 两阶段提交 / 冲突预检三方裁决 / 跨进程演进 RFC 决策点 | 043 §2 · conflict-fabric · parallel-subagents N4 · scenario-operation（A.1/A.2/A.6/B3/B9） |
| [真实场景运转协议](scenario-operation.md) | 043 到真实交付场景的协议层：38 个真实场景（入口 7 · 执行/验收 8 · A 组 10 · B 组 12 + 并入 B3 的「git 分支迭代与合并」深度子面）运转判定、统一共同缺口、阶段 A–H 路线图、判定基准 | 043 全文 · DoD · airfc · parallel-subagents · subagent-management · conflict-branch · testing-first |
| [场景闭环推演验收协议](scenario-drive-acceptance.md) | 交付判据 = 场景五面（A 输入 / B 真实代码路径 / C LLM 视角 / D 工具调用 / E 上下文）推演闭环，非测试全绿；L0 核心场景 1.1/1.2/2.1/2.3/3.4/4.1 推演卡与断点定位；阶段 A–H 修复项验收入口 | 043 全文 · scenario-operation · definition-of-done · airfc |
| [性能观测与性能信号](performance-observability.md) | `AIPerfMonitor` / `AIPerfSignal` / `AIPerfStage` / `AIPerfAnomaly` / `AIPerfSeverity` / `AIPerfRun` 设计：Stopwatch 墙钟 + `rt_proc_get_stats` 新 ABI 取内存/CPU + 超时熔断 + 退出信号分类；性能信号模型与异常分类表；进 DoD 落点（增强信号 → P3 D9 门）；L2 回喂/绿点接线 | 043 §3 · §6 · DoD D0/D3 · scenario-operation B7/A.7 · signal-log |
| [信号日志与 LLM 上下文筛选](signal-log.md) | `AISignalLog` / `AISignalEntry` / `AISignalLevel` 分级（Debug/Info/Warn/Error）+ KeySignal 筛选 + 落盘 `target/scratch/arc-logs/` + `BuildLlmView(tokenBudget)` 字符代理约束；`AIToolOutput` 工具输出门面（exit + 摘要 + perf 摘要 + 日志路径）；与 `AITurnRunner` / `QualityTools` 接线 | 043 §3 · §10 · DoD · scenario-operation B7/A.7/B11 · performance-observability |
| [LLM 门闩](llm-gates.md) | 写代码前硬门闩、宣称禁令 | 043 读前门闩 |
| [冲突织物](conflict-fabric.md) | AIRfc / AIPlan / AITool 共用租约（后到拒绝） | 043 §10 · 038 |
| [API 草图](api-sketch.md) | 聚合根与包面公开签名草图 | 043 §10 · airfc §3 |
| [包布局](package-layout.md) | `Harness` / `Harness.Coding` 目录与职责（quality 属 Coding） | 043 §10 |
| [收敛迁移](convergence-migration.md) | 试探 → 终态迁移；禁抄 HarnessAnchor 等 | 043 §11 |
| [可执行 DoD 与验证闭环](definition-of-done.md) | D0–D7、L2/L3、绿点回滚；Completed ⇔ D0–D7 | 043 §3 · §5 · §6 |
| [纠偏协议](anchor-correction-protocol.md) | 修复/纠偏分流、偏差检测、决策轨迹 | 043 §4 |
| [设计先行与设计评审](design-review.md) | 远见/收敛/模块化/零冗余 | 043 §7 |
| [协作确认点](collaboration-checkpoints.md) | 高沟通价值决策点 | 043 §8 |
| [测试先行与纠偏同步](testing-first.md) | 验收测试锁定；随 AIRfc 升版同步 | 043 §9 |
| [回合小结与偏差判定](work-summary.md) | 每单元小结与偏差判定 | 043 §6 · §8 |
| [Harness 工程自审报告](harness-self-review.md) | 用 Harness 方法论审 Harness 建设（意图/设计/DoD/计划/小结/协作/宣称 + 偏差清单 + 纠偏建议） | 043 全文 · 038 §12–§13 · plan.md（过程回顾，非能力契约） |
| [Harness LLM 复盘报告](harness-llm-lessons.md) | 体系建设踩坑清单（A1–A8）+ 改进机制（B1–B8）+ 落地方案（P0/P1/P2）+ 一句话总结 | 043 全文 · plan.md（教训沉淀，非能力契约） |

---

[返回 043(../../043-harness.md) · [返回 RFC 索引](../../index.md)
