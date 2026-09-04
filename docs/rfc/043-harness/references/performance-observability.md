# 性能观测与性能信号（AIPerfMonitor · 增强信号 · D9 演进）

> **命名约定**：性能门称 **D9 性能门**（`AIDoDGateKind.D9Perf`）；D8 归属 [definition-of-done](definition-of-done.md) 的真实接入冒烟门。

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md) §3 · §6 · §10；承接 [可执行 DoD](definition-of-done.md)（D0/D3 性能信号增强 + D9 演进）、[真实场景运转协议](scenario-operation.md)（B7 性能与可观测性门 · A.7 非功能需求进验收 · B11 AI 降级）、[回合小结](work-summary.md)。本子项是 043 体系的**能力子项**（性能观测面），与 [信号日志](signal-log.md)（日志分级 / LLM 上下文筛选 / 工具输出门面）同组落盘，共享演进 P1–P3 节奏。
>
> **性质**：**P1（采集落盘）与 P3（D9 性能门）已具备能力面**；P2 仍为设计态（诚实缺口）。P1/P3 能力面以本文件 §P1 / §P3 为准；本文其余 P2 设计元素（`AIPerfStage` / `AIPerfSeverity` / `AIPerfExitKind` / `perf:anomaly` 决策事件 / `AIDoDFixFeedback.PerfSignals` / 绿点附性能摘要 / `AIToolOutput` 门面）仍为设计态，**不得**据此宣称能力「完成 / Completed / 已收敛」。落地按演进 P1–P3 逐阶段排期（plan.md 登记为 `PF-1–PF-3`），每阶段以 [场景闭环推演验收协议](scenario-drive-acceptance.md) 五面推演闭环为判据。

## §0 宣称门闩

| # | 门闩 |
|---|------|
| 1 | **P1、P3 已具备能力面；P2 为设计态**：`AIPerfMonitor` / `AIPerfSignal` / `AIPerfAnomaly` / `AIPerfRun` / `AISignalLog` 的 **P1 面**已具备（见 §P1）；**P3 D9 门已具备能力面**（见 §P3）；`AIPerfStage` / `AIPerfSeverity` / `AIPerfExitKind` / `perf:anomaly` 决策事件 / `AIDoDFixFeedback.PerfSignals` 为 **P2 设计态（诚实缺口）**；禁止据此宣称 P2 能力「完成」 |
| 2 | **分层归属**：采集在 `Arc.Diagnostics` additive 扩展（`rt_proc_get_stats` 新 ABI 消费）、基座 `Arc.Agent.Harness` 持骨架（类型 + 门槽位 + 信号日志）、Coding 持判定/筛选规则；禁止在基座焊死领域判定 |
| 3 | **增强信号不新开门**：P1/P2 的 `PerfSignals` 只做**增强回喂**，不得新增 `AIDoDGateKind` 门、不得改变 `Pending ≠ Passed` 语义；D9 性能门为 **P3 演进**（阶段 E，见 [scenario-operation](scenario-operation.md) 阶段 E） |
| 4 | **演进逐阶段验收**：P1→P3 每阶段验收 = 所属场景（B7 / A.7 / B11）五面推演闭环，非测试全绿；e2e 只是证据面之一 |
| 5 | **ABI 纪律**：`rt_proc_get_stats` 为**新增 additive ABI 符号**（不改既有 `rt_stopwatch_*` 等符号语义）；按 [RFC 036(../../036-maturity.md) 基础面冻结纪律登记排期，破坏性变更才须走 RFC 流程 |

## 目标

- 让 Harness 对「门 / 工具执行的可观测性」有**真实采集面**：墙钟耗时（`Stopwatch`）+ 进程内存 / CPU（`rt_proc_get_stats`）+ 超时熔断 + 退出信号分类。
- 性能信号进 **DoD 判定增强**：P1/P2 作为 `AIDoDGateResult.PerfSignals` / `AIDoDFixFeedback.PerfSignals` 增强信号挂在既有门上（不新开门），并进 L2 迭代回喂与绿点打点。
- 演进到 P3 后升级为 **D9 性能门**（基线版本化 + 回归阈值），补 [A.7](scenario-operation.md) 的「基准测试门」空白面（阶段 E）。

## 非目标

| 项 | 说明 |
|----|------|
| 编译器性能基线 | 不替代 plan.md H2 gate（raw 0.58 / 0.62 / 0.56）——那是**编译性能基线**，本面是 **Harness 运行期工具/门执行可观测** |
| Profiler / 热点分析 | 不做采样 / 火焰图 / 热点定位（属编译器 / 工具链专项，另立 RFC） |
| 分布式追踪 / APM | 不做跨进程 / 跨服务链路追踪；本面止于单进程内的门 / 工具执行 |
| 跨会话资源预算治理 | 不做会话级内存 / 时长强制收束——那是 `TotalBudget` / 预算面（[038 §14](../../038-ai-host.md) · [parallel-subagents](parallel-subagents.md)）范畴，与本面正交 |
| 破坏既有 ABI / 门语义 | `rt_proc_get_stats` 为 additive 新符号；P1/P2 不新增 `AIDoDGateKind` 门 |

## 类型契约与分层归属

```text
Arc.Diagnostics            ← 采集（additive 扩展）：rt_proc_get_stats ABI 消费 + 进程统计快照
  └─ Arc.Agent.Harness     ← 骨架：AIPerfMonitor / AIPerfRun / AIPerfSignal / AIPerfStage /
                             AIPerfAnomaly / AIPerfSeverity + AIDoDGateResult.PerfSignals 槽位
                               + perf:anomaly 决策事件 kind
        └─ Arc.Agent.Harness.Coding ← 判定/筛选：perf 异常阈值规则、AIDoDFixFeedback.PerfSignals
                                     消费、D9 性能门判定（P3）
```

| 层 | 内容 | 允许 | 禁止 |
|----|------|------|------|
| `Arc.Diagnostics`（additive 扩展） | `rt_proc_get_stats`（新 ABI，取进程内存 / CPU 统计）消费；进程统计快照类型（如 `ProcessStats`） | 新增类型 / 新增 ABI 符号；复用既有 `Stopwatch`（墙钟） | 改既有 `Process` / `Stopwatch` Stable 面语义；把采集判定逻辑焊进 Diagnostics |
| `Arc.Agent.Harness`（基座） | `AIPerfMonitor`（采集器骨架：墙钟 + 内存/CPU + 超时熔断 + 退出信号分类）、`AIPerfRun` / `AIPerfSignal` 等类型、`AIDoDGateResult.PerfSignals` 槽位、`perf:anomaly` 决策事件 kind | 门骨架、事件 kind、类型契约 | 焊死 Coding 阈值 / D9 判定；自造第二套事件库 |
| `Arc.Agent.Harness.Coding` | perf 异常阈值 / 筛选规则、`AIDoDFixFeedback.PerfSignals` 消费、D9 性能门判定（P3） | 领域判定规则 | 平行再造 Monitor / 事件面 |

### AIPerfStage（阶段分类）

```as
// 归属：Arc.Agent.Harness
public enum AIPerfStage {
    Gate,      // DoD 门执行（D0 build / D1 inspect / D2 契约 / D3 test / …）
    Tool,      // 单次工具调用（quality.* / fs.* / shell.*）
    FixLoop,   // L2 迭代整轮（RunFixLoopAsync）
    Compile,   // arc build 编译子阶段
    Test,      // arc test 测试子阶段
    Unknown    // 未分类
}
```

### AIPerfSeverity（严重度）

| 级别 | 判定 |
|------|------|
| `Normal` | 在软阈值内，无可报告异常 |
| `Elevated` | 超软阈值（内存 / CPU / 墙钟偏长）——记增强信号，不直接判红 |
| `Critical` | 超硬阈值 / 超时熔断 / 非零退出——增强信号标红；P3 D9 门判红依据 |

### AIPerfAnomaly（异常分类表）

| AIPerfAnomaly | 判定 | 缺省严重度 | 进 DoD 动作（P1/P2 增强信号；P3 D9 门） |
|---------------|------|-----------|-----------------------------------------|
| `Timeout` | 执行超时熔断（超时预算耗尽，`AIPerfMonitor` 超时熔断触发） | Critical | 增强信号标红；`perf:anomaly` 事件；L2 回喂；P3 D9 判红 |
| `MemorySpike` | 进程峰值内存超阈值（`rt_proc_get_stats`） | Elevated→Critical（超硬阈值） | 增强信号；回喂调优（内存维度）；P3 D9 回归依据 |
| `CpuHigh` | CPU 占用持续高位（`rt_proc_get_stats`） | Elevated | 增强信号；回喂 |
| `NonZeroExit` | 退出码非零 / 异常退出 | Critical | 与既有 D0/D3 失败路径同源；perf 侧补退出分类信号 |
| `WallClockSlow` | 墙钟耗时长于基线 N%（P3，需基线） | Elevated | **P3**：D9 回归阈值判红；P1/P2 阶段仅记时无基线判定 |

### AIPerfSignal（单条性能信号）

```as
// 归属：Arc.Agent.Harness
public class AIPerfSignal {
    public AIPerfStage Stage;
    public AIPerfSeverity Severity;
    public AIPerfAnomaly Anomaly;        // 无异常 = None
    public long ElapsedMs;               // Stopwatch 墙钟
    public long? PeakMemoryBytes;        // rt_proc_get_stats（采集可用时）
    public double? PeakCpuPercent;       // rt_proc_get_stats（采集可用时）
    public string Detail;                // 命令 / 退出码 / 信号名等
}
```

### AIPerfRun（一次运行聚合）

```as
// 归属：Arc.Agent.Harness
public class AIPerfRun {
    public string RunId;
    public string Subject;               // 门 / 工具名（如 "D0-compile" / "arc_test"）
    public AIPerfStage Stage;
    public List<AIPerfSignal> Signals;
    public bool TimedOut;                // 超时熔断标记
    public AIPerfExitKind Exit;          // ExitedNormally / SignalTerminated / Crash / Unknown
    public long ElapsedMs;

    public AIPerfSeverity MaxSeverity { get; }
    public bool HasAnomaly { get; }
    public string PerfSummary();         // 摘要文本（进 AIToolOutput / DoD 增强回喂）
}
```

> `AIPerfExitKind` 承载「退出信号分类」：正常退出（ExitedNormally）/ 信号终止（SignalTerminated，如超时熔断、外部 kill）/ 崩溃（Crash，非零退出码）/ 未知（Unknown）。

### AIPerfMonitor（基座采集器骨架）

```as
// 归属：Arc.Agent.Harness
public class AIPerfMonitor {
    public AIPerfRun Begin(string subject, AIPerfStage stage, int timeoutMs);
    public void AddStagePoint(AIPerfRun run, AIPerfStage stage);   // 子阶段墙钟点
    public void SampleProcessStats(AIPerfRun run);                 // rt_proc_get_stats 取内存/CPU
    public AIPerfRun End(AIPerfRun run, int exitCode);             // 退出信号分类 + 汇总
    public bool BreakIfTimedOut(AIPerfRun run);                    // 超时熔断
}
```

- **墙钟**：复用 `Arc.Diagnostics.Stopwatch`（单一惯用法，不另起计时）。
- **内存 / CPU**：`rt_proc_get_stats` 新 ABI（additive，落 `crates/runtime/` + `Arc.Diagnostics` 扩展采集），采当前进程峰值内存 / CPU 占用；采集不可用 → 信号字段 `null` + 诚实标注（不冒充）。
- **超时熔断**：按超时预算（`timeoutMs`）在门 / 工具执行期间探测，超限标记 `TimedOut` + 触发取消（与 [场景 A.9 撤单收束](scenario-operation.md) 的 CTS 取消语义并列，不重复造取消机制）。
- **退出信号分类**：由退出码 / 信号归入 `AIPerfExitKind`。

## 进 DoD 落点（增强信号 → P3 D9 门）

| 阶段 | 落点 | 语义 |
|------|------|------|
| P1/P2 | `AIDoDGateResult.PerfSignals`（挂既有门结果）、`AIDoDFixFeedback.PerfSignals`（失败回喂）、`perf:anomaly` 决策事件 | **增强信号，不新开门**；不改变 `Pending ≠ Passed`；`AIDoDGateKind` 面不动 |
| P3 | `AIDoDGateKind.D9Perf`（D9 性能门） | **新开门（阶段 E）**：`arc bench` 关键路径基准双跑对比 + 回归阈值，基准基线版本化（随绿点/checkpoint 落盘）；断点消除才宣称「NFR 进验收」 |

- **P1/P2 不动 `AIDoDGateKind`**：性能只做增强回喂——门结果携带 `PerfSignals` 供模型 / 人观察，失败门经 `AIDoDFixFeedback.PerfSignals` 携带性能维度进入 L2 迭代；`Pending ≠ Passed` 语义不受增强信号影响（见 [DoD §0](definition-of-done.md)）。
- **P3 D9 门（阶段 E）**：对标 [scenario-operation](scenario-operation.md) A.7「最小补件① D9 基准门（`arc bench` + 回归阈值，基准基线版本化）」与 B7「最小补件① 基准门」。D9 判定 = 当前关键路径基准 ↔ 版本化基线 diff 超回归阈值 → 红。

## 与 L2 回喂 / 绿点接线

| 面 | 接线 |
|----|------|
| L2 迭代回喂 | `AIDoDFixFeedback.PerfSignals`：失败门回喂携带性能信号（超时 / 内存峰值 / CPU），模型迭代调优有性能维度（对齐 [scenario-operation](scenario-operation.md) 2.3 / 4.3 的 L2 迭代闭环） |
| 绿点打点 | `CheckpointGreenAsync` 打点附带性能摘要（`AIPerfRun.PerfSummary`，P2+）；P3 性能基线随绿点 / checkpoint 落盘（扩展 [绿点快照](definition-of-done.md) 面），性能回归 → 回滚或升级人 |
| 决策轨迹 | `perf:anomaly` 决策事件 kind 入 `AIDecisionEventKind`（单轨，与 `airfc:*` / `checkpoint:*` 并列），可 /resume、可复盘 |

## 演进 P1–P3（plan.md 登记 `PF-1–PF-3`）

| 阶段 | 内容 | 状态 |
|------|------|------|
| **P1 采集落盘**（`PF-1`） | `AIPerfMonitor` 采集（墙钟 + `rt_proc_get_stats` + 超时熔断 + 退出分类）→ 落盘 `target/scratch/arc-logs/`（随 [AISignalLog](signal-log.md) 落盘面）；`AIDoDGateResult.PerfSignals` 增强信号挂门结果 | **能力面成立**，见 §P1 |
| **P2 筛选视图**（`PF-2`） | [AISignalLog](signal-log.md) KeySignal 筛选 + `AIToolOutput` perf 摘要（Coding）；`perf:anomaly` 决策事件；绿点附性能摘要 | ⌛ 设计态（诚实缺口） |
| **P3 基线 + D9 门**（`PF-3`） | 性能基线版本化（随绿点落盘）+ `AIDoDGateKind.D9Perf`（D9 性能门，阶段 E）；`WallClockSlow` 相对基线判定启用 | **能力面成立**，见 §P3 |

> P1/P2/P3 为设计权威演进名（用户面口径）；plan.md 排期标识 `PF-1–PF-3`（与既有 `P3 并行子代理` 里程碑名消歧）。

## §P1 能力面（采集落盘 · PF-1）

> **P1 最小可用面**（纯 additive、不改门判定）。落地实现以用户已确认研究要点为权威，与上方设计态成员（Stage/Severity/ExitKind 等）不重叠；未落地的设计元素归 P2/P3。

| 面 | 交付 |
|----|------|
| `rt_proc_get_stats` native ABI | `crates/runtime/rt_proc.c` + `crates/arc/native/rt_process.ani` 新符号（additive）：`rt_proc_get_stats(NativePtr, out long user_ms, out long kernel_ms, out long peak_memory_bytes, out int exit_reason) -> int`。Windows：`GetProcessTimes`（UserTime/KernelTime）+ `GetProcessMemoryInfo`（`K32GetProcessMemoryInfo` 动态加载，PeakWorkingSetSize）；POSIX：`getrusage`（ru_maxrss 归一字节 / ru_utime / ru_stime）+ 经 `wait_status` 保留 `WIFSIGNALED/WTERMSIG` 暴露 `exit_reason`（0=正常退出；>0=信号号；<0=未退出）。不改变既有调用语义 |
| `ProcessRunStats` | `std/Arc/Diagnostics/ProcessRunStats.as`：`ElapsedMs` / `PeakMemoryBytes` / `CpuUserMs` / `CpuKernelMs` / `ExitReason`（`ProcessExitReason`：NotExited/NormalExit/SignalTerminated）/ `ExitSignal`；`ProcessRunResult` 增可选 `Stats` + `TimedOut`（additive 字段） |
| 超时捕获运行 | `Process.RunCaptureAsync(si, int timeoutMs, ct)` / `RunCapture(si, int timeoutMs)`：`WaitForExit(timeoutMs)` → 超时 `Kill` → `TimedOut` 标记；`Stopwatch` 计墙钟；`GetRunStats()` 采集 CPU/峰值内存附到结果 |
| `AIPerfMonitor` | `std/AI/Agent.Harness/Perf/`：`RunAsync(args, project, ct)`（`ARC_COMPILER` 优先，对齐 QualityCli）+ `RunAsync(si, project, ct, timeoutMs)` 显式超时 → `AIPerfRun`（`Result` / `ElapsedMs` / `Signals` / `TimedOut` / `Crashed` / `Anomaly` / `SpawnFailed` / `SpawnError` / `LogPath`）。退出分类 `AIPerfAnomaly`：`Crash`（Windows 崩溃 NTSTATUS ≥0xC0000000 或 POSIX 信号终止）/ `Oom`（0xC0000017）/ `StackOverflow`（0xC00000FD）/ `Timeout` / `MemorySpike` / `SlowCompile`（阈值判定 P3 启用）/ `SpawnFailed`。**不改门判定** |
| `AISignalLog` | `Add(level/source/category/line/keySignal)` + `WriteAsync(name, ct)` 落盘 `<project>/target/scratch/arc-logs/<tool>-<seq>.log`（seq 从既有同名文件递增；对齐 AICheckpointStore 先例，禁写源码树）；返回路径（失败空串） |
| D0/D3 接线 | `CodingDoDGateEvaluator.EvaluateD0Async/EvaluateD3Async` 改经 `AIPerfMonitor.RunAsync`；`AIDoDGateResult` 增可选 `List<AIPerfSignal> PerfSignals`；门 Detail 附 `perf: wall=… peak_mem=… cpu_user=… cpu_kernel=… log=<path>`——**判定逻辑不变**（D0 仍 exit 码、D3 仍 `--logger json` 用例明细 + 防降级基线 + 验收对照） |

**可执行用例**：`arc_ai_perf_observability_e2e`（D0 跑 `arc build` → 门 Detail 含 wall/内存、`target/scratch/arc-logs/` 出现日志、故意崩 fixture 崩溃 0xC0000005 分类 `Crash`、sleep fixture 超时分类 `Timeout`）；`cargo build -p arc` 绿；`arc build examples/ArcAgent` / `examples/ReviewAgent` 双绿；dod_d1/d2_d4/d6/acceptance/fix_loop/checkpoint/plan_gate/decision_trail/domain_two_reuse e2e 无回归。

**挂账**（归 P2/P3）：`AIPerfStage`/`AIPerfSeverity`/`AIPerfExitKind`/`perf:anomaly` 决策事件/`AIDoDFixFeedback.PerfSignals`/绿点附性能摘要；`SlowCompile`/`MemorySpike` 阈值判定（枚举面已备，P3 进基线后启用）；`rt_proc_get_stats` POSIX 侧为 `getrusage(RUSAGE_CHILDREN)` 累计口径（非逐进程）。

## §P3 能力面（D9 性能门 · 基线版本化 + 回归阈值）

> **P3 最小可用面**（纯 additive，D9 为 P3 新增门，不改变 P1/P2「增强信号不新开门」语义）。落地实现以本节为权威。

| 面 | 交付 |
|----|------|
| `AIDoDGateKind.D9Perf` | `std/AI/Agent.Harness/DoD/AIDoDGate.as` 枚举增 `D9Perf`（P3 新增门；D0–D7 门语义不变） |
| `AIPerfBaseline` / `AIPerfBaselineKind` | `std/AI/Agent.Harness/Perf/AIPerfBaseline.as`：基线（Subject + Kind + WallMs + PeakMemoryBytes）；Kind = `FirstCompile`（首编译冷基线）/ `Incremental`（增量暖基线） |
| `AIPerfBaselineStore` | `std/AI/Agent.Harness/Perf/AIPerfBaselineStore.as`：`Record`（按 Subject+Kind upsert）/ `Find` / `HasBaseline` / `SaveAsync` / `LoadAsync`，落盘 `<project>/target/scratch/arcagent-state/perf-baseline.json`（随绿点落盘，禁源码树） |
| `D9PerfEvaluator` | `std/AI/Agent.Harness.Coding/DoD/D9PerfEvaluator.as`：纯函数 `Compare(baselineWall, baselineMem, currentWall, currentMem, thresholds)` → `D9PerfVerdict`（Passed/Warning/Failed）+ Detail；`D9PerfThresholds`（软 1.2x / 硬 1.5x 默认）；基线为 0 的维度诚实跳过（不臆造比率） |
| D9 门接线 | `CodingDoDGateEvaluator.EvaluateD9Async`：`AIPerfMonitor` 跑 `arc build` → 采集墙钟/峰值内存 → 无基线 → 建立首编译基线（Passed）→ 有基线 → `Compare` 相对增量基线（超硬 → Failed；超软 → Passed 附 warning；软回归不判红）；`EvaluateAsync` 分派 `AIDoDGateKind.D9Perf` |

**可执行用例**：`arc_ai_d9_perf_e2e`（`Compare` 纯函数五档判定——wall 1.0x Passed / 1.3x Warning / 2.0x Failed / 内存 2.0x Failed / 内存 1.3x Warning；`AIPerfBaselineStore` Record→Find→HasBaseline→Save→新 store Load 往返，`perf-baseline.json` 真实落盘）；`cargo build -p arc` 绿；`arc build examples/ArcAgent` / `examples/ReviewAgent` 双绿；`ai_model_registry_e2e` / `arc_ai_rfc_persistence_e2e` / `arc_ai_perf_observability_e2e`（P1）无回归。

**挂账**（归 P2 / 后续）：`perf:anomaly` 决策事件 / `AIDoDFixFeedback.PerfSignals` / 绿点附性能摘要 / `AIToolOutput` 门面（P2 筛选视图）；D9 门未并入 `Completed` 判定链（`RunAutoGatesAsync` 不含 D9——D9 是阶段 E 增强门，经 `EvaluateAsync(AIDoDGateKind.D9Perf)` 单独调用）；`WallClockSlow`/`MemorySpike` 相对基线判定的 AIPerfAnomaly 分类接入（枚举面已备，D9 以 `Compare` 直接承载）。

## 验收（五面推演判据，非测试全绿）

每阶段验收 = 所属场景**五面推演闭环**（[scenario-drive-acceptance](scenario-drive-acceptance.md)：A 输入 / B 真实代码路径 / C LLM 视角 / D 工具调用 / E 上下文）：

| 阶段 | 场景 | 五面推演判据 |
|------|------|-------------|
| P1 | B7（性能与可观测性门） | B 面真实：门 / 工具执行产出 `AIPerfRun` 并落盘（非空想）；D 面返回可消费：`PerfSignals` 挂门结果 / 日志路径可引用；C/E 面：模型可见性能信号 |
| P2 | B7 + A.7（NFR 进验收）+ 4.1（验收对照） | `AIToolOutput` 门面真实折叠工具输出（exit + 摘要 + perf 摘要 + 日志路径）；KeySignal 筛选后 LLM 上下文有性能维度 |
| P3 | A.7 + B7（D9 门） | D9 性能门进 DoD 判定、基线版本化、回归阈值真实；「NFR 进验收」断点消除才宣称交付 |

- **诚实边界**：任一阶段未过五面推演（如 `rt_proc_get_stats` 平台缺失、`AIPerfMonitor` 零调用）→ 该阶段不宣称交付；`rt_proc_get_stats` 采集不可用面诚实 `null` + 标注，禁止以代理值冒充。

---

[返回 references 索引](index.md) · [返回 043(../../043-harness.md) · [信号日志](signal-log.md) · [可执行 DoD](definition-of-done.md) · [场景闭环推演验收协议](scenario-drive-acceptance.md)
