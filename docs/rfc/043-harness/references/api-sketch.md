# API 草图（AIRfc · DoD · 归属）

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md) §2 · §3 · §10 · §11；语义权威见 [AIRfc 体系](airfc.md)、[可执行 DoD](definition-of-done.md)、[回合小结](work-summary.md)。本子项给出**可伪代码实现**的类型草图与包归属；**不是**已验收 API 契约。首切片 `std/AI/Agent.Harness` 为试探，不得据此宣称终态。

## §0 宣称门闩

| 宣称 | 条件 |
|------|------|
| 「API 草图已锁」 | 本文 §1–§8 与 043 架构锁一致；未在业务代码中落地不算实现完成 |
| 「AIRfc / DoD API 已完成」 | 须对应 [收敛迁移](convergence-migration.md) 相关里程碑验收通过；**禁止**仅有本草图即宣称完成 |
| 「与首切片一致即终态」 | **禁止**——过渡残留（如 `HarnessEventLog`）与历史试探不得当终态；正道见 §2–§8 |

## §1 分层与类型归属总表

```text
Arc.Agent                         ← 冲突织物 / AIPlan / 会话事件 / AITool
  └─ Arc.Agent.Harness            ← AIRfc 运行时 + DoD 门骨架 + AIWorkSummary
        └─ Arc.Agent.Harness.Coding ← quality.* / D0–D7 判定（IAIDoDGateEvaluator）
```

| 类型 | 归属包 |
|------|--------|
| `AILeaseKind` / `AILeaseKey` / 升维后的 `AICoordinator` 租约 API | `Arc.Agent` |
| `AIPlan` / `AIPlanGate` / `AITaskRun` | `Arc.Agent` |
| 会话事件写入面（承接原 Harness 轨迹） | `Arc.Agent` |
| `AIRfc` / Spec 面类型 / `AIRfcWorkItem` / `AIRfcRuntime` | `Arc.Agent.Harness` |
| `AIWorkSummary` | `Arc.Agent.Harness` |
| `AIDoDGateKind` / `AIDoDGateStatus` / `AIDoDGateResult` / `AIDoDOrchestrator` / `IAIDoDGateEvaluator` | `Arc.Agent.Harness`（骨架） |
| `IAIDoDGateEvaluator` 的 Coding 实现；`quality.*` 工具 | `Arc.Agent.Harness.Coding` |

## §2 AIRfc（聚合根）

Plan 面 = **`AIPlan` 引用**（持有 `PlanId` + 可选运行时解析句柄），**禁止**内嵌平行计划结构。

```as
// 归属：Arc.Agent.Harness
namespace Arc.Agent.Harness;
using Arc.Agent; // AIPlan

/// <summary>意图面：可感知结果（非技术细节）。</summary>
public class AIIntentionSpec {
    public string Text;
}

/// <summary>设计面：远见 / 收敛 / 结构 / 模式 / 决策理由。</summary>
public class AIDesignSpec {
    public string Foresight;
    public string Convergence;
    public string Structure;
    public string Patterns;
    public string Rationale;
}

/// <summary>验收面：场景 + 断言（测试先行锁定）。</summary>
public class AIAcceptanceSpec {
    public string Scenarios;
    public string Assertions;
}

/// <summary>
/// AIRfc 聚合根：跨任务、跨版本的需求与交付唯一事实源。
/// Plan 面仅引用 Arc.Agent.AIPlan，不复制步骤状态机。
/// </summary>
public class AIRfc {
    public string RfcId;
    public int Revision;
    public AIIntentionSpec Intention;
    public AIDesignSpec Design;
    public AIAcceptanceSpec Acceptance;

    /// <summary>所引用 AIPlan 的稳定标识（会话内可再解析为 AIPlan 实例）。</summary>
    public string PlanId;

    /// <summary>
    /// 可选：已解析的计划句柄；序列化/跨会话以 PlanId 为准。
    /// null = 尚未 AttachPlan。
    /// </summary>
    public AIPlan? Plan;

    public List<AIRfcWorkItem> WorkItems;
}
```

| 字段 | 契约 |
|------|------|
| `RfcId` | 程序级稳定 id；跨任务共享同一 AIRfc |
| `Revision` | Spec 任一面变更 → 统一步进 +1；旧版只读审计 |
| `Intention` / `Design` / `Acceptance` | 内部必填面，禁止拆成四套平行管理系统 |
| `PlanId` + `Plan?` | Plan 面 = AIPlan 引用；`PlanId` = `AIPlan.Id`（创建时分配、跨修订不变，租约键 `"plan:"+Id` 同源）；`AttachPlan` 前 `Plan` 可为 null |

## §3 AIRfcWorkItem

```as
// 归属：Arc.Agent.Harness
namespace Arc.Agent.Harness;

/// <summary>AIRfc 下的可并行工作项；可绑定不同 Session / TaskRun。</summary>
public class AIRfcWorkItem {
    public string WorkItemId;
    public string RfcId;
    public string Title;
    public string? SessionId;
    public string? TaskRunId;
    public AIRfcWorkItemStatus Status; // Open / InProgress / Blocked / Done / Failed / Cancelled
}
```

| 字段 | 契约 |
|------|------|
| `WorkItemId` | 工作项稳定 id |
| `RfcId` | 归属 AIRfc |
| `SessionId` / `TaskRunId` | 可选绑定；跨任务并行时各绑各的 |
| 写路径 | 变更须经冲突织物 `AILeaseKind.RfcSpec`（见 §7） |

## §4 AIRfcRuntime

签名级运行时；实现落在 Harness 基座，租约经 `AICoordinator` 获取。

```as
// 归属：Arc.Agent.Harness
namespace Arc.Agent.Harness;
using Arc.Agent;

public class AIRfcRuntime {
    /// <summary>进程内登记；RfcSpec 租约经 AICoordinator 接线属 H-2d。</summary>
    public AIRfcRuntime() { /* ... */ }

    /// <summary>创建 AIRfc（Revision = 1）；写入会话事件 airfc:created。</summary>
    public AIRfc Create(
        string rfcId,
        AIIntentionSpec intention,
        AIDesignSpec design,
        AIAcceptanceSpec acceptance);

    /// <summary>纠偏升版：Spec 面增量更新 → Revision+1；事件 airfc:revised。</summary>
    public AIRfc Revise(
        string rfcId,
        AIIntentionSpec? intention,
        AIDesignSpec? design,
        AIAcceptanceSpec? acceptance,
        string reason);

    /// <summary>绑定已有 AIPlan（Plan 面 = 引用，不拷贝步骤）。</summary>
    public AIRfc AttachPlan(string rfcId, AIPlan plan);

    /// <summary>登记/绑定工作项（可跨 Session）。</summary>
    public AIRfcWorkItem BindWorkItem(
        string rfcId,
        string workItemId,
        string title,
        string? sessionId,
        string? taskRunId);

    /// <summary>多来源需求冲突（Active → Contested，A.1）；冲突期间禁修订。</summary>
    public AIRfc MarkContested(string rfcId, string reason);

    /// <summary>冲突解决（Contested → Active）。</summary>
    public AIRfc ResolveContested(string rfcId);

    /// <summary>进入冻结窗口（Active → Frozen，A.2）；冻结期间禁 Revise/Reject。</summary>
    public AIRfc FreezeRfc(string rfcId, string reason);

    /// <summary>解冻（Frozen → Active）。</summary>
    public AIRfc UnfreezeRfc(string rfcId, string reason);

    /// <summary>D7 通过收口终态（Active/Frozen → Closed）；禁再 Revise/Reject。</summary>
    public AIRfc CloseRfc(string rfcId, string reason);

    /// <summary>撤单终态（Active/Frozen/Rejected → Cancelled，A.9）。</summary>
    public AIRfc CancelRfc(string rfcId, string reason);

    /// <summary>序列化全部 AIRfc 为 JSON（AIRfc 持久化；含 Revision/Status/WorkItems/PlanId）。</summary>
    public string Serialize();

    /// <summary>反序列化恢复登记表（跨会话重建聚合根，非 transcript 重放冒充）。</summary>
    public bool Restore(string json);
}
```

| 方法 | 前置 | 副作用 |
|------|------|--------|
| `Create` | 持有 `AILeaseKind.RfcSpec` | 新建聚合根；会话事件 |
| `Revise` | 同上；禁止用对话散落改 Spec | Revision+1；旧版只读 |
| `AttachPlan` | 计划对象来自 `Arc.Agent`；可另需 `AILeaseKind.Plan` | 只写 `PlanId`/`Plan` 引用 |
| `BindWorkItem` | `AILeaseKind.RfcSpec` | 工作项列表变更；不另起第二套 PM |

## §5 AIWorkSummary

字段语义与 [work-summary](work-summary.md) 一致；归属 Harness 基座（领域无关小结面）。

```as
// 归属：Arc.Agent.Harness
namespace Arc.Agent.Harness;

public class AIWorkSummary {
    public string UnitId;       // 单元 id
    public string Did;          // 做了什么（≤2 行语义）
    public string Alignment;    // 对齐：设计/需求 ✓ 或偏差点
    public string Verification; // 验证：命令 + 绿/红 + 覆盖
    public string Difficulty;   // 困难/绕过（必答；无 →「无」）
    public string Findings;     // 发现（必答；无 →「无」）

    public string Format();     // 决策面文本，非聊天记录
    public bool HasFindings { get; }
    public bool HasBypass { get; }
}
```

产出后写入 **Agent 会话事件**（如 `work_summary`），**禁止**写入永久 `HarnessEventLog`。

## §6 DoD：门骨架与评估器

基座只持**门种类 / 结果 / 编排骨架**；D0–D7 **判定信号**由 Coding 实现 `IAIDoDGateEvaluator` 提供。

```as
// 归属：Arc.Agent.Harness（骨架）
namespace Arc.Agent.Harness;

public enum AIDoDGateKind {
    D0Compile,
    D1Semantics,
    D2Contract,
    D3Behavior,
    D4DiffCoverage,
    D5SelfReview,
    D6AntiPattern,
    D7HumanAccept
}

public enum AIDoDGateStatus {
    Pending,    // 未跑或信号未接线 —— 不得当作 Passed
    Passed,
    Failed,
    NeedsHuman
}

public class AIDoDGateResult {
    public AIDoDGateKind Gate;
    public AIDoDGateStatus Status;
    public string Signal;
    public string Detail;
}

/// <summary>领域判定注入点；Coding 包实现，基座不写死 arc CLI。</summary>
public interface IAIDoDGateEvaluator {
    Task<AIDoDGateResult> EvaluateAsync(
        AIDoDGateKind gate,
        string project,
        AIRfc rfc,
        CancellationToken cancellationToken);
}

public class AIDoDOrchestrator {
    public AIDoDOrchestrator(string project, IAIDoDGateEvaluator evaluator) { /* ... */ }

    public async Task<AIDoDGateResult> RunGateAsync(
        AIDoDGateKind gate,
        AIRfc rfc,
        CancellationToken cancellationToken);

    public async Task<List<AIDoDGateResult>> RunAutoGatesAsync(
        AIRfc rfc,
        CancellationToken cancellationToken);

    /// <summary>Completed 可执行定义：全部 Passed；Pending ≠ Passed。</summary>
    public static bool AllPassed(List<AIDoDGateResult> results);
}
```

```as
// 归属：Arc.Agent.Harness.Coding
namespace Arc.Agent.Harness.Coding;

/// <summary>用 quality.* / arc build / arcgr 等产生 D0–D7 信号。</summary>
public class CodingDoDGateEvaluator : IAIDoDGateEvaluator {
    public async Task<AIDoDGateResult> EvaluateAsync(
        AIDoDGateKind gate,
        string project,
        AIRfc rfc,
        CancellationToken cancellationToken) {
        // D0: arc build；D1: arcgr；D2: 契约扫描；D3: arc test；…
        // 未接线门诚实返回 Pending，禁止假绿
    }
}
```

| 面 | 基座 | Coding |
|----|------|--------|
| `AIDoDGateKind` / `AIDoDGateResult` / 编排顺序 / 修复轮次预算 | ✅ | 消费 |
| `arc build` / `quality.*` / `.arcgr` / 契约扫描 | ❌ | ✅ |
| `Pending` 语义 | 骨架强制 `Pending ≠ Passed` | 未接线须 Pending |

## §7 冲突织物消费面（AILeaseKind）

`AIRfc` + `AIPlan` + `AITool` **共用**升维后的 `AICoordinator`；Harness / Coding **只消费、不平行实现锁**。

```as
// 归属：Arc.Agent
namespace Arc.Agent;

public enum AILeaseKind {
    ToolPath,  // AITool 副作用路径（现有写协调升维）
    Plan,      // AIPlan 修订 / 步进
    RfcSpec    // AIRfc Spec / 工作项写
}

// 示意：统一租约键（具体字段名以实现为准）
public class AILeaseKey {
    public AILeaseKind Kind;
    public string ResourceId; // 路径 / PlanId / RfcId
    public string SessionId;
}
```

| 租约 | 消费者 | 冲突例 |
|------|--------|--------|
| `ToolPath` | `[AITool]` / Fs 写 | 两会话写同一文件 |
| `Plan` | `AIPlan` / PlanGate / `AttachPlan` 相关步进 | 并行 Task 改同一计划 |
| `RfcSpec` | `AIRfcRuntime.Create/Revise/BindWorkItem` | 两任务同时升版同一 AIRfc |

## §8 禁令：禁止出现的类型名

终态 API / 新代码 / 新文档表述中**禁止**再引入或固化下列类型（历史试探仅作迁移删除对象）：

| 禁止类型名 | 原因 |
|------------|------|
| `PlanSpec` | Plan 面 = `AIPlan` 引用，禁平行计划结构 |
| `HarnessAnchor` | 已升格为 `AIRfc`；旧名仅历史别名 |
| `HarnessEventLog` | 决策轨迹入 Agent 会话事件，禁永久双轨日志库 |
| `HarnessEvent` / `HarnessEventKind`（作为永久公共 API） | 同上；事件种类并入会话事件面 |
| `AIHarnessSession`（作为终态公共聚合） | 终端薄组装，不把试探会话壳当终态 PM 门面 |
| 第二套 `*Coordinator` / `*LeaseManager`（Harness 或 Coding 内） | 冲突织物唯一在 `Arc.Agent.AICoordinator` |
| `CodingAIRfc` / 领域私有第二套需求本尊 | Coding 不拥有平行 AIRfc |
| 基座内 `QualityTools` / `QualityCli`（终态位置） | 须在 `Arc.Agent.Harness.Coding` |
| `CheckpointSnapshot` / `CheckpointIndex` / `CheckpointIndexEntry` / `CheckpointRollbackOutcome` / `CheckpointFileEntry` | 绿点类型已按 AI 前缀收敛为 `AICheckpoint*`（归档审计）；旧名仅历史别名，禁回退 |
| `IFixRoundProvider` | 接口命名已收敛为 `IAIFixRoundProvider`（I + AI 前缀，归档审计）；旧名禁回退 |

---

[返回 043(../../043-harness.md) · [AIRfc](airfc.md) · [包布局](package-layout.md) · [收敛迁移](convergence-migration.md) · [references 索引](index.md)
