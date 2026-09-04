# 冲突分支（方案 B · 极致严谨）

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md)（§2 · §10 · 宣称纪律）· [冲突织物](conflict-fabric.md)（L1 底座）· [并行子代理（P3）](parallel-subagents.md)（`AIMergeTransaction` 设计态 · N4 禁自动选胜者）· [AIRfc](airfc.md) · [可执行 DoD](definition-of-done.md) · [真实场景运转协议](scenario-operation.md)（A.1 / A.2 / A.6 / B3 / B9 / 场景 3.5）。本子项定义 **两大机制之方案 B：极致严谨的冲突分支机制** 的设计契约——三级冲突（L1 资源租约 / L2 Spec 矛盾 / L3 git 合并）统一到一条「机器检测拒绝 → 人 CCB 裁决」链；分支模型、`AIMergeTransaction` git 两阶段提交、跨进程演进路径与 RFC 决策点显式化。**诚实缺口（未实现）**：L3 合并冲突三方裁决、`merge:*` 决策事件、`AIRfc.BranchId`、合并门 CI/headless 接线均仍为设计态；在此缺口收束前，不据此宣称「冲突分支 / 主分支保护完成」（§0 / §9 / §10 门闩）。
>
> **设计来源**：已完成的两大机制系统性设计 + 「git 分支迭代与合并」只读推导（6 细分场景：卡 1 开分支迭代 / 卡 2 分支内开发 / 卡 3 分支合并 / 卡 4 解决冲突 / 卡 5 合并后回归 / 卡 6 多分支并行）。本子项是其 RFC 级结构化落盘，**未臆造新架构**。

## §0 宣称门闩

未满足下列全部项前，**禁止**宣称「冲突分支 / 方案 B 完成 / 已收敛 / 主分支保护可用」：

| # | 门闩 | 未过时 |
|---|------|--------|
| 1 | L1 冲突织物可信：`AICoordinator` 三 Kind 一表 + 后到拒绝 + Commit 持约已落地（[conflict-fabric](conflict-fabric.md)）；**单进程非目标**不越界宣称 | 禁在多实例场景宣称安全 |
| 2 | **禁自动选胜者**：L2 / L3 一律升级人 CCB 裁决；机器只检测与拒绝（P3 N4 延伸）；L1 后到拒绝本身不选胜者 | 禁「自动裁决」宣称 |
| 3 | 本子项 §2–§8 类型契约与 043 分层 / api-sketch 归属一致；命名一律 `AI` 前缀、归属 `Arc.Agent.Harness` | 禁 API 定稿宣称 |
| 4 | `AIMergeTransaction` 统一工作区合并 + 分支合并两种场景为**一种两阶段提交模式**，不并行第二套合并事务 | 禁「双事务」宣称 |
| 5 | 合并门唯一权威：合并后完整 D0–D7 全勾才可 `Completed`；D7 人评审不可布尔假确认（SR-3 收口后） | 禁绕过合并门 |
| 6 | 跨进程租约**未立 RFC 决策前不默认实现**（§6）；演进路径 §9 每步（B1–B4）以其场景五面推演闭环为验收 | 禁「多实例并发安全」宣称 |

写代码前另须过 [llm-gates](llm-gates.md) 与 043 开篇读前门闩。

## §1 目标与非目标

**目标**

| # | 目标 |
|---|------|
| G1 | 三级冲突（资源租约 / 逻辑矛盾 / git 合并）**统一仲裁**：机器只检测与拒绝，裁决唯一入口 = 人（CCB），禁自动选胜者 |
| G2 | git 分支 ↔ AIRfc / 工作项可映射；绿点 / 门状态按分支隔离；基线所有权明确 |
| G3 | `AIMergeTransaction` 落地为 git 两阶段提交（staging → 原子 commit），失败 `merge --abort` + 绿点回滚 |
| G4 | 合并前冲突预检（引用图 / 文件集重叠）→ 合并中三方裁决 → 裁决后完整 D0–D7 回归 |
| G5 | 跨进程演进路径与 RFC 决策点显式化（不默认实现） |
| G6 | 一致性：租约不越界、合并原子、回滚可恢复、决策可审计 |

**非目标**

| # | 非目标 | 理由 |
|---|--------|------|
| N1 | 跨进程租约默认实现 | 冲突织物单进程非目标（[conflict-fabric §2](conflict-fabric.md)）；演进须 RFC 决策 |
| N2 | 自动裁决任何 L2 / L3 冲突 | 禁自动选胜者（P3 N4 延伸） |
| N3 | 实现自己的 merge 算法 | 复用 git 三方合并（`merge --no-commit`） |
| N4 | 完整 git flow / Release 管理 | B4 发布管理属阶段 F（[scenario-operation §6](scenario-operation.md)）；本机制只到「分支迭代 + 合并」 |

## §2 多级冲突模型与统一仲裁

```text
L1 资源租约（已落地）      L2 逻辑冲突（新增判定）        L3 合并冲突（新增）
  ToolPath/Plan/RfcSpec      Spec 方向矛盾 / Acceptance     git 双改同文件/区域
  后到拒绝                   互斥 / 计划步进矛盾            三方合并冲突标记
        │                          │                          │
        └──────────────► 统一仲裁链 ◄──────────────────────────┘
     机器检测（可判则判）→ 登记 AIConflictRecord（决策轨迹）
     → 平凡面自动处置（仅 L1 后到拒绝属自动互斥，不选胜者）
     → L2/L3 一律升级人 CCB 裁决（AIConflictResolver）
     → airfc:resolved / merge:resolved 事件 → 新基线
```

| 级 | 冲突面 | 机制 | 处置 |
|---|--------|------|------|
| **L1** | 资源租约（`ToolPath` / `Plan` / `RfcSpec`） | `AICoordinator` 三 Kind 一表 + 后到拒绝 + 可审计（**已落地**） | 被拒者 Failed + 小结 → 汇总门红 → 人裁决；**本方案不发明第二套锁** |
| **L2** | Spec 逻辑矛盾（A.1 顺序矛盾零判定缺口） | Revision 事件带 **Spec 字段级结构化 diff**（`AIAcceptanceSpec.Items` 已结构化，机器可比）→ 同 acceptance 项被反方向覆盖 → 标 `AIRfcStatus.Contested` | `/conflict <rfcId>` 列出并行方向 / owner → 人 CCB 裁决 → `airfc:resolved` 写新 Revision 基线 |
| **L3** | git 合并冲突（同文件双改） | git 三方合并冲突标记（`merge --no-commit` 后检测） | 冲突清单 + base/ours/theirs 三方视图 → 人 CCB 裁决（§5）→ 裁决后重跑合并门 |

> **语义正交声明**：ToolPath 租约 = **单进程写时互斥**；分支冲突 = **跨分支合并时裁决**。两者不互替（[scenario-operation §4.0](scenario-operation.md) 合并映射表）。

## §3 分支模型

**分支 ↔ AIRfc 映射**：一个分支承载一个或多个 AIRfc（多任务可共支迭代）；`AIBranchLease` 保证同名分支唯一（`git checkout -b` + 租约登记）。

```text
AIBranch
  ├── BranchName   "feature/<rfcId>-<topic>"
  ├── RfcIds[]     本分支承载的 AIRfc
  ├── Owner        基线所有权（合并权）
  ├── BaseRef      分叉基点（默认 main）
  ├── BaseCommit   merge-base(main, branch) 记录
  ├── Status       Active | Frozen | Merged | Abandoned
  └── CheckpointDir target/scratch/arc-checkpoints/<branch>/
```

**分支隔离**：`AICheckpointStore` 的 `StoreRelDir` 参数化 → `arc-checkpoints/<branch>/`（index + checkpoint-<seq> + objects 按分支隔离）；门状态按分支独立重算（门状态持久化属阶段 C 前置，见 [scenario-operation §6 阶段 C](scenario-operation.md)）。

**基线所有权**：`Owner` 声明 + D7 人评审；合并权 = 基线 owner / CCB，非 owner 合入须显式人批准（阶段 G 角色模型落地前以 owner + D7 兜底）。

**分支生命周期**：Active（迭代）→ Frozen（冻结窗口，与 A.2 `/freeze` 联动）→ Merged（合并完成）或 Abandoned（撤单 / 废弃）。

## §4 合并机制

**合并门** = 汇总门（合并后完整 D0–D7）+ CI（headless，B1 场景）+ 人评审（D7 ↔ PR review approval）。

**`AIMergeTransaction` 落地为 git 两阶段提交**（[parallel-subagents §8 未落地面①](parallel-subagents.md)）：

```text
Stage 1（staging，不写 HEAD）：
  git checkout target   → git merge --no-commit --no-ff source
  → 三方合并结果进 index（中间态可整体撤销）
  → 文本冲突 → 冲突文件清单 → 升级人三方裁决（见 §5）
Stage 2（commit，原子）：
  冲突全解 + 合并门全绿 → git commit（一次原子提交）
  任一失败 → git merge --abort 整体回滚 + 绿点兜底（checkpoint-<branch>/）
```

两阶段提交**统一两种合并场景**（收敛而非第二套事务）：

| 场景 | Stage 1（staging） | Stage 2（原子 Move） |
|---|---|---|
| 工作区内子代理产物合并（[parallel-subagents §3.5](parallel-subagents.md) 原设计态） | per-run staging（`target/scratch/`） | 统一 Move → 冲突整体回滚 + 升级人 |
| 跨分支 git 合并（本方案） | `merge --no-commit`（分支 commit 即 staging） | 统一 `git commit` → `merge --abort` 回滚 |

**合并失败回滚**：`git merge --abort`（回到 merge 前 index / 工作区）+ 绿点兜底（非 git 环境 / index 损坏时用 `checkpoint-<branch>/` 快照恢复，复用 3.4 已闭环的多绿点 + 大文件内容寻址能力）。合并前**强制基线绿点** `CheckpointGreenAsync("pre-merge")`；合并成功打合并绿点 + `checkpoint:merge` 事件。

## §5 冲突检测与裁决

| 阶段 | 检测 | 处置 |
|---|---|---|
| 合并前预检（`PreviewAsync`） | **引用图 / 文件集重叠分析**：两分支改动文件集 ∩（`.arcgr` 引用图相关文件 / 工作项 `Scope` 预声明） | 无重叠 → 放行；有重叠 → 登记潜在 `AIConflictRecord`（可先串行化：等一方先合，或升级人预裁决） |
| 合并中检测 | git 三方合并冲突标记（`--no-commit` 后 `git status --porcelain` UU/AA / 冲突文件） | 升级人**三方裁决**：提供 base / ours / theirs 三版 + 涉及工作项 / 验收；机器不得选胜者 |
| 合并后回归 | 合并门：完整 D0–D7（D1 引用完整性、D4 合并后总 diff 覆盖 / 越界、D3 全量测试、D5 跨分支证据、D7 人评审） | 红 → L2 迭代（≤3 轮）或整体回滚 |

```as
// 归属：Arc.Agent.Harness
public enum AIConflictKind { LeaseConflict, SpecContradiction, MergeConflict }

public class AIConflictRecord {
    public string ConflictId;
    public AIConflictKind Kind;
    public List<string> Resources;   // 路径 / acceptance 项 / 冲突文件
    public List<string> Parties;     // 会话 / 分支
    public string Evidence;          // hash / diff 摘要
    public string Status;            // Open | Resolved | Escalated | Rejected
}

public class AIConflictResolver {
    public List<AIConflictRecord> Open();
    public AIConflictRecord Record(AIConflictKind kind, string detail);
    public bool ResolveAsync(string conflictId, string decision, string reason, string resolvedBy); // 人 CCB
    public bool RejectAsync(string conflictId, string reason);
}
```

裁决全程入决策轨迹（`conflict:*` / `merge:*` / `airfc:resolved` 事件），**全事件可审计**。

## §6 跨进程演进路径（冲突织物单进程非目标 → 演进路径）

```text
现状：单进程 AICoordinator 登记表（多会话共享；跨进程不共享）
  │
  ▼ 演进（按需，立 RFC）
Phase B-a：repo 级租约文件（.arcagent/leases/）
  - 每租约 = 文件（原子创建 O_EXCL + TTL + 心跳续约 + 过期回收）
  - 语义与 AICoordinator 对齐：后到拒绝、可审计、Commit 持约
  - 适用：单仓库多实例（本地 REPL + CI 并发）
Phase B-b：server 面（独立进程持租约表 + 决策轨迹 + 合并仲裁）
  - 适用：多仓库 / 多机器
  - 更重，需求驱动再启动
```

**RFC 决策点（显式列出，不默认实现）：**

1. 租约持久化选型：repo 级 `.arcagent/leases/` 文件锁 vs server 面？
2. 租约 TTL 与续约：心跳间隔 / 崩溃后回收窗口 / 时钟偏差处理？
3. 跨进程决策轨迹合并：append-only JSONL 并发 append 原子性（单行原子写）？
4. 跨进程合并仲裁：谁持有 `git` 写权（工作区锁）？

**禁令**：跨进程租约未立 RFC 前，不得宣称多实例并发安全（B3 / B9 同源，[scenario-operation §7](scenario-operation.md)）。

## §7 一致性保证（极致严谨清单）

| 保证 | 机制 |
|---|---|
| 租约不越界 | 单进程单一 `AICoordinator` 登记表；Commit 需持有租约（`CommitRfcSpec` / `CommitAsync` 已做）；Commit 不自动放锁（编辑间隙保护已做）；跨进程 = 原子文件创建 + TTL + 心跳回收（§6 演进面） |
| 合并原子 | `merge --no-commit` 中间态 → 统一 `git commit` 一次提交；staging 失败 → `merge --abort` 整体回滚 |
| 回滚可恢复 | 合并失败 → `merge --abort` + 分支级绿点（`checkpoint-<branch>/`）兜底；多绿点 + 大文件内容寻址已闭环（场景 3.4） |
| 决策可审计 | 全部冲突检测 / 裁决 / 合并事件入决策轨迹（`conflict:*` / `merge:*` / `airfc:resolved`）；覆写审计（`AICoordinator` audit）已存在 |
| 禁自动选胜者 | L2 / L3 升级人 CCB；机器只检测拒绝 |
| 合并门唯一权威 | 合并后完整 D0–D7 全勾才可 `Completed`；D7 人评审不可布尔假确认（SR-3 收口后） |

## §8 API 草图

> 归属 `Arc.Agent.Harness`（基座；git 操作经薄服务注入，Coding 判定经 `IAIDoDGateEvaluator` 注入）；`AIDecisionEventKind` 扩展（`conflict:*` / `merge:*`）属 `Arc.Agent`；合并门判定信号属 `Arc.Agent.Harness.Coding`。以下为**设计契约**，非已验收 API。

```as
// 归属：Arc.Agent.Harness
namespace Arc.Agent.Harness;

public enum AIBranchStatus { Active, Frozen, Merged, Abandoned }

public class AIBranch {                       // §3 分支实体（详见 §3 图）
    public string BranchName;                 // "feature/<rfcId>-<topic>"
    public List<string> RfcIds;
    public string Owner;                      // 基线所有权（合并权）
    public string BaseRef;                    // 分叉基点（默认 main）
    public string BaseCommit;                 // merge-base(main, branch)
    public AIBranchStatus Status;
    public string CheckpointDir;              // target/scratch/arc-checkpoints/<branch>/
}

public class AIBranchLease {                  // 分支租约：同名唯一 + 合并权
    public bool Acquire(string branchName, string owner);
    public void Release(string branchName);
    public string HolderOf(string branchName);
}

public class AIMergeController {              // 合并控制器：两阶段提交 → git 语义
    public async Task<AIMergePreview> PreviewAsync(AIBranch source, AIBranch target, CancellationToken ct);  // 预检（引用图 / 文件集重叠）
    public async Task<AIMergeTransaction> BeginAsync(AIBranch source, AIBranch target, CancellationToken ct); // Stage1: merge --no-commit
    public async Task<AIMergeGateResult> RunMergeGateAsync(AIMergeTransaction tx, CancellationToken ct);      // 合并门：汇总门 D0–D7 + 人评审
    public async Task<AIMergeOutcome> CommitAsync(AIMergeTransaction tx, CancellationToken ct);               // Stage2: git commit（原子）
    public async Task<AIMergeOutcome> AbortAsync(AIMergeTransaction tx, CancellationToken ct);                // merge --abort + 绿点回滚
}

public class AIMergeTransaction {
    public string Id;
    public string SourceBranch;
    public string TargetBranch;
    public string State;                       // Staging | GatePending | GateFailed | Ready | Committed | Aborted
    public List<string> Conflicts;             // 冲突文件
}

public enum AIConflictKind { LeaseConflict, SpecContradiction, MergeConflict }
public class AIConflictRecord { /* §5 */ }
public class AIConflictResolver { /* §5：人 CCB 裁决唯一入口 */ }
```

**包归属**：`AIBranch` / `AIBranchLease` / `AIMergeController` / `AIMergeTransaction` / `AIConflictRecord` / `AIConflictResolver` → `Arc.Agent.Harness`；`AIDecisionEventKind` 扩展（`conflict:*` / `merge:*`）→ `Arc.Agent`；合并门判定信号 → `Arc.Agent.Harness.Coding`（经既有 `IAIDoDGateEvaluator`，不写死 arc CLI）。**前置项**：SR-1 `AIPlan.Id` 落地（§10）。

## §9 演进路径（B1–B4）

> 每步「完成」= **其场景五面推演闭环**（[scenario-drive-acceptance](scenario-drive-acceptance.md)）——B 面（真实代码路径）落定 + 五面无断点；e2e 只是 B 面证据之一。依赖：S0 地基（SR-1 + 状态扩展 + 持久化前置）；B2 起依赖 [subagent-management A3](subagent-management.md)（跨分支收束需干预能力）。

| 步 | 内容 | 依赖 | 验收（场景五面推演闭环） |
|---|---|---|---|
| B1 | SR-1 `AIPlan.Id` + 状态扩展（`AIRfcStatus` 增 Frozen/Closed/Cancelled/Contested + `AIRfcWorkItemStatus` 增 Failed/Cancelled）+ `/conflict` + `airfc:resolved` + CCB 裁决 | S0 地基 | **A.1**：需求来源冲突裁决闭环（并发拦截 + 顺序矛盾判定 + CCB 裁决）；**A.2** 冻结 / Closed 生命周期 |

> **L2 冲突检测 / 裁决面（最终契约）**：`AISpecConflictDetector`（字段级结构化 diff：`AIAcceptanceSpec.Items` 条目级比对，异来源覆盖同 acceptance 项 → 反方向覆盖信号）+ `AIConflictRecord` / `AIConflictResolver`（`Record` / `Open` / `ResolveAsync`（必须 `resolvedBy`，禁自动选胜者）/ `RejectAsync`）+ `AIRfc.Source`（来源追踪）+ `AIRfcRuntime.ResolveContestedWithSpec / RejectContested`（新 Revision 基线 / 拒绝联动）+ `conflict:detected|resolved|rejected` / `airfc:resolved` 决策事件 + `/conflict` REPL（列出 Open 冲突方向/来源/evidence + resolve/reject 人 CCB 交互）。冻结与冲突裁决互不干扰。
| B2 | `AIBranch` + `AIBranchLease` + 分支级绿点隔离（`arc-checkpoints/<branch>/`）+ `AIMergeController` 两阶段提交 + `merge --abort` 回滚 | [subagent-management A3](subagent-management.md)（跨分支收束能力） | **B3**：主分支保护 / 合并事务可回滚；**A.6** 分支隔离 + 基线所有权 |

> **分支模型 / 合并两阶段提交面（最终契约）**：`AIBranch`（BranchName/RfcIds/Owner/BaseRef/BaseCommit/Status/CheckpointDir）+ `AIBranchStatus`（Active/Frozen/Merged/Abandoned + wire 编解码）+ `AIBranchLease`（同名唯一 Create 拒绝重名 + Owner 合并权 HolderOf/CanMerge + 状态迁移 SetStatus/MarkMerged/MarkAbandoned）+ 分支级绿点隔离（`AICheckpointStore` 参数化 `StoreRelDir` 双构造器）+ `AIMergeController`/`AIMergeTransaction`（一种两阶段提交模式：`PreviewAsync` merge-base + `git diff --name-only` 双方改动集交集（潜在冲突信号）→ `BeginAsync` `git checkout target` + pre-merge 基线绿点 + `git merge --no-commit --no-ff source` + `git diff --name-only --diff-filter=U` 冲突文件集 → `RunMergeGateAsync` 汇总门 D0–D7（经 `AIDoDOrchestrator.RunAutoGatesAsync`，Pending≠Passed）→ `CommitAsync` 合并权校验 + `git commit` 原子提交 + MarkMerged + 合并绿点 → `AbortAsync` `git merge --abort` 整体回滚 + 分支级绿点兜底）。git 操作统一经 `Process.RunCaptureAsync`（`git` 直调，非裸 shell 语义）；只消费 `AIDoDOrchestrator`/`AICheckpointStore`/`AIBranchLease`，不发明第二套锁。
>
> **B2 边界（挂账，归 B3）**：L3 三方裁决（base/ours/theirs 三版 + 人 CCB，§5）；`merge:*` 决策事件（`AIDecisionEventKind` 扩展）；`AIRfc.BranchId` 绑定字段；合并门接线 CI/headless + 人评审 D7；跨进程租约（B4）。
| B3 | 合并门接线（汇总门 + CI/headless + 人评审 D7）+ 合并前冲突预检（引用图 / 文件集重叠，§5 `PreviewAsync`）+ 三方裁决 | B2 + [subagent-management A5](subagent-management.md) | **B3**：合并门 = 汇总门 + CI 状态检查 + 人评审；**B1** headless 产物可消费 |
| B4 | 跨进程租约 RFC 决策点（`.arcagent/leases/` vs server 面，§6） | B3 | **B9**：多实例协调 RFC 评审通过（文档化决策点）才算 |

## §10 前置项（SR-1 `AIPlan.Id`）

| # | 前置项 | 来源 | 说明 |
|---|--------|------|------|
| SR-1 | `AIPlan` 引入程序级稳定 `Id`（跨修订不变），`AIPlanGate` 租约键与 `AIRfc.AttachPlan` 统一到 `"plan:"+Id`，消除 Goal 合成 | [harness-self-review §9 TOP2](harness-self-review.md) / D-02 / D-09；实现规划 | P3 与方案 B 共同前置：多分支 / 多会话计划面冲突裁决（§2 L2）依赖稳定 Plan 租约键；跨会话 `AIPlan` 引用（B1 状态扩展）亦依赖之 |

> **诚实缺口（未实现）**：L3 合并冲突三方裁决（`AIMergeConflictDetector` + 三方视图 + 人裁决）、`merge:*` 决策事件、`AIRfc.BranchId`、合并门 CI/headless 接线，仍为设计态；在此缺口收束前，不得宣称「冲突分支 / 主分支保护完成」（§0 / §9 / §10 门闩）。

---

[返回 043(../../043-harness.md) · [冲突织物](conflict-fabric.md) · [子代理管理（方案 A）](subagent-management.md) · [真实场景运转协议](scenario-operation.md) · [references 索引](index.md)
