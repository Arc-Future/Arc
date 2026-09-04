# Arc.Agent

## 概述

`Arc.Agent` 是 Arc 的端侧进程内**开箱即用**的可信 AI 宿主能力集：强类型、单一惯用法、异步优先、会话协议状态机、工具流式接管、人机协同门闩（HITL）、唯一内置结构化记忆 `AIWiki`、CodeAct 代码执行、MCP 协议接入、多模态内容与工作区管理。

它**不是** Agent / RAG / MultiAgent 框架，也**不是**可插拔 Memory 乐高——认知在模型内演化，契约、会话、边界、确认门闩与 Wiki 记忆在 Arc 内固化。本册讲如何基于 `Arc.Agent` 开发 Agent。

宿主体系统一命名空间 `Arc.Agent`（含 `Arc.Agent.DeepSeek`/`Arc.Agent.OpenAI`/`Arc.Agent.Agnes`/`Arc.Agent.MCP` 模型 Provider 与工具源）。纯推理引擎（`Tensor`/`IAIModel`/`Arc.AI.Onnx`/`Arc.AI.Iree`）见 [ai-inference.md](ai-inference.md)，不占此篇。

### 命名与拓扑

```
应用
  └─ AIHost                        ← 唯一入口（Provider + 工具 + 选项 + 可选 AIWiki）
       ├─ AIWiki                   ← 跨会话结构化记忆（唯一内置）
       ├─ AISession                ← 回合协议状态（transcript / 状态机 / 预算 / HITL）
       ├─ AIToolSet                  ← 编译期工具清单 + capability 分派
       └─ IAIChatClient         ← CompleteAsync / StreamEventsAsync → IAsyncEnumerable<AIStreamEvent>
              ↕ AwaitingHuman（异步等待，不自旋）
         人类：ApproveAsync / Edit / ProvideInputAsync / RejectAsync → Resume
```

## 快速开始

依赖 `Arc.Agent` 后，仅构造 Provider 与工具即可获得多轮 + 工具 + 流式/HITL/Wiki 能力。

### 1. 声明工具

用 `[AITool]` 特性声明工具，schema 在编译期确定（拒绝反射 Invoke）：

```as
using Arc.Agent;
using Arc.Agent.Tools;

[AITool("device.read_battery")]
public class ReadBatteryTool {
    public string Execute() {
        return "87%";
    }
}
```

### 2. 构造宿主并运行会话

```as
using Arc.Agent;
using Arc.Agent.DeepSeek;
using Arc.Agent.Sessions;

// 构造 Provider（此处以 DeepSeek 为例）
DeepSeekChatClient provider = new DeepSeekChatClient(DeepSeekOptions.Default);

AIHost host = AIHost.Create(provider, AISessionOptions.Default);
host.Wiki.Upsert("device/battery", "上次读数 87%");   // 跨会话记忆

using AISession session = host.CreateSession();
AIReply reply = await session.RunAsync("读一下电池", ct);
```

`RunAsync` 内含 tool 回合、可选 HITL 暂停、可选工具参数流式接管；用户无需自建 Agent 图或 Memory 管线。

### 3. Arc.Agent.MCP 接入外部工具源

`Arc.Agent.MCP` 以 MCP（Model Context Protocol）对接外部工具源/上下文：宿主通过 MCP 客户端接入符合协议的远端工具，统一纳入 `AIToolSet` 与 capability 体系。MCP 工具源与本地 `[AITool]` 工具在会话内同构消费，不另起第二套工具回路。

## 核心 API

### AIHost —— 唯一入口

| 成员 | 说明 |
|------|------|
| `AIHost.Create(provider, options)` | 构造宿主；`options` 为宿主级 `AISessionOptions`（单一事实源） |
| `AIHost.Create(provider, tools)` | 同时绑定工具集 |
| `host.Wiki` | 宿主级默认 `AIWiki` |
| `host.Options` | 宿主级选项 |
| `host.Coordinator` | 写协调器（跨会话冲突规避、原子提交） |
| `host.Context` | 共享 Context 组合根（跨会话复用） |
| `host.CreateSession()` / `CreateSession(options)` | 创建会话，选项继承宿主默认 |
| `host.CreateTaskRun(maxSteps)` | 创建有界回合的长时间任务 |
| `host.CreateCodeAct(provider)` | 装配 CodeAct：注册 `codeact` 工具进 AIToolSet 并授予 `codeact.CodeAct` capability，返回 `AICodeAct` 门面（配置超时/输出上限） |

`AIHost` 实现 `IDisposable`；`Dispose()` 释放宿主级 Context 组合根（幂等）。

### AISession —— 会话状态机

会话维护回合协议状态；`SessionId`/`Transcript`/`Turn`/`Tools`/`Capabilities`/`MaxTurns`/`MaxMessages`/`TurnsUsed`/`MessagesUsed`/`RemainingTurns`/`RemainingMessages`/`PendingHuman`/`ActiveToolStream` 等可读。仅 `Idle`/`Done` 可接受新 `RunAsync`；超预算 → `Failed`；`CancellationToken` → `Cancelled`。

| 状态 | 语义 |
|------|------|
| `Idle` | 可接受新的 `RunAsync` |
| `Completing` | 模型往返 |
| `StreamingTools` | 工具名已知、参数增量到达（可接管） |
| `AwaitingTools` | 完整 call 就绪 |
| `AwaitingHuman` | 人机门闩，等待 `ApproveAsync`/`RejectAsync` |
| `DispatchingTools` | 沙箱校验通过、执行副作用 |
| `Done` / `Failed` / `Cancelled` | 终止态 |

| 成员 | 说明 |
|------|------|
| `RunAsync(string, ct)` | 单轮主入口，内含 tool 回合 / 可选 HITL / 可选流式接管 |
| `ResumeAsync(ct)` | HITL 决策后恢复 |
| `ApproveAsync(edited, ct)` / `RejectAsync(reason, ct)` / `ProvideInputAsync(text, ct)` | 人机协同三 API |
| `PumpToolCallStart/ArgDelta/End(...)` | 工具流式接管泵 |
| `MaxTurns`/`MaxMessages`/`TurnsUsed`/`MessagesUsed`/`RemainingTurns`/`RemainingMessages` | 预算只读面（剩余额度 -1 = 不设限） |
| `Transcript` / `TotalUsage` / `Snapshot()` / `Restore(snapshot)` | 会话只读面与快照 |

### [AITool] 与 AIToolSet

`[AITool]` 编译期生成工具清单（`AIToolSet`）。工具必须在已声明 `capability` 内执行，沙箱可审计；能力外操作被拒绝并写回 transcript。长参数工具（如 `fs_write` 写大文本）支持流式接管：

```as
public interface IAIToolStreamHandler {
    Task<AIToolStreamDisposition> OnToolCallStartAsync(AIToolCallStart start, CancellationToken ct);
    Task OnToolArgDeltaAsync(AIToolArgDelta delta, CancellationToken ct);
    Task<AIToolResult> OnToolCallEndAsync(AIToolCallEnd end, CancellationToken ct);
}
```

`AIToolStreamDisposition` 三值 `Buffer`（默认全量拼装）/`TakeOver`（handler 消费增量）/`Reject`（立即失败），禁双轨。

### 人机协同（HITL）

进入 `AwaitingHuman` 时填充 `PendingHuman`（`AIHumanRequest`：原因、工具草稿 `AIToolCall?`、可选提示、截止策略）。正道 API 全异步：

| API | 语义 |
|-----|------|
| `ApproveAsync(AIToolCall? edited, ct)` | 通过（可带编辑后的 call） |
| `RejectAsync(string? reason, ct)` | 拒绝，写回 transcript |
| `ProvideInputAsync(string text, ct)` | 人类补充输入后回 `Completing` |

`AwaitingHuman` 期间非阻塞等待（异步 pause/resume，不自旋）；不执行工具副作用；可 `Cancel` → `Cancelled`。触发策略单一配置面：`[AITool(RequireApproval = true)]` 或 Session 级策略。审批记录进 transcript 保证可审计。

### AIWiki —— 唯一内置记忆

`AIWiki` 是唯一内置结构化记忆：按路径存取的结构化、可审计页面表。进程内 `Get`/`Upsert`/`Delete`/`List` 为纯内存操作。

```as
class AIWiki {
    AIWikiPage? Get(string path);
    void Upsert(string path, string body, AIWikiMeta? meta = null);
    bool Delete(string path);
    IReadOnlyList<string> List(string? pathPrefix = null);
}
```

`AIHost.Wiki` 为 Host 级默认；`CreateSession` 可覆盖或共享。`AISessionOptions.WikiPathsToAttach` 显式附页进请求（非自动检索）。Wiki ≠ Session——Session 管本轮协议与 transcript，Wiki 管跨会话事实。

### CodeAct 与执行

CodeAct 提供沙箱内代码执行：宿主把模型生成的动作代码（在已声明 capability 内）经沙箱执行，工具与代码统一走 capability 分派与 HITL 门闩。沙箱拒绝能力外操作 → 写 transcript 失败消息或回合 `Failed`（单一锁定）。

接线（RFC 038 §7）：`AIHost.CreateCodeAct(IAICodeActProvider)` 把 `AICodeAct` 包装为内置 `codeact` 工具注册进宿主 AIToolSet，并授予 `codeact.CodeAct` capability（fail-closed：未装配/未授权一律拒绝）。模型生成工具调用 `{"code":"..."}` 即经 `AIToolSandbox` 统一分派执行；可插拔后端 `AIProcessCodeActProvider`（独立解释器进程）与 `AINativeCodeActProvider`（`arc build` 原生编译运行）复用同一进程捕获基座。

### 多模态与工作区

`AIMessage`/`AIRequest` 承载多模态内容（文本/图像等），规范化强类型出入。宿主提供工作区会话面（冲突规避、任务回环），多轮任务在会话内管理状态。

### Provider 槽

`IAIChatClient` 为唯一 Provider 槽：`CompleteAsync(AIRequest, ct)` + `StreamEventsAsync(AIRequest, ct) → IAsyncEnumerable<AIStreamEvent>`（事件集 `TextDelta`/`ReasoningDelta`/`ToolCallStart`/`ToolArgDelta`/`ToolCallEnd`/`Usage`/`Completed`/`Error`，流恰以 `Completed` 或 `Error` 终结；取消为 `Error("Cancelled", ...)`）。换 Provider 不换 Host/Session API。SSE 线协议解析复用 `Arc.Net` 通用 `SseDecoder`，Provider 只做领域事件映射。依赖方向单向：`Arc` ← `Arc.Agent` ← `Arc.Agent.<Name>`。

## Harness 与 AIRfc（RFC 043）

Harness 分两层：**基座**（`Arc.Agent.Harness`）与**领域**（首域 `Arc.Agent.Harness.Coding`）。基座核心是 **`AIRfc`**——小型项目管理 / 需求本尊运行时（跨任务、跨版本）；计划面直接复用 `AIPlan`（宿主 [038 §12](../rfc/038-ai-host.md)），与 `AITool` 共用冲突织物（[038 §13](../rfc/038-ai-host.md) · [conflict-fabric](../rfc/043-harness/references/conflict-fabric.md)）。

| 面 | 归属 | 说明 |
|----|------|------|
| AIRfc | Harness 基座 | Spec 聚合（意图/设计/验收）+ 工作项 + Revision；Plan = `AIPlan` |
| DoD 骨架 / 小结 / 纠偏协议 | Harness 基座 | 门接口与流程；具体信号在领域包 |
| `quality.*` / D0–D7 判定 | `Arc.Agent.Harness.Coding` | Coding 领域验证器 |
| 冲突织物 | `Arc.Agent` | `AILeaseKind` ∈ {ToolPath, Plan, RfcSpec}；后到拒绝；AIRfc/Plan/Tool 禁第二套锁 |
| 终端 | `examples/ArcAgent` | 薄组合根 |

设计权威：[RFC 043](../rfc/043-harness.md) · [AIRfc](../rfc/043-harness/references/airfc.md) · [冲突织物](../rfc/043-harness/references/conflict-fabric.md) · [llm-gates](../rfc/043-harness/references/llm-gates.md) · [api-sketch](../rfc/043-harness/references/api-sketch.md)。

## 线程模型

宿主/会话采用**单线程宿主**约束：`AIHost`/`AISession`/`AIContextEngine`/`AIWiki` 由单线程驱动，跨会话并发冲突由 `AICoordinator` 规避。会话内多工具回合以「先全部启动、再按序 await 收集」实现 I/O 重叠并发（工具批并行），但共享状态（沙箱结果/调用计数）不跨线程并行——多线程并发使用上述类型是未定义行为，勿在无协调的并发上下文同时驱动同一 `AIHost`/`AISession`。

## 边界

- **纯推理引擎**（`Tensor`/`IAIModel`/`Arc.AI.Onnx`/`Arc.AI.Iree`）见 [ai-inference.md](ai-inference.md)；本册只讲宿主（会话/工具/HITL/Wiki/CodeAct/MCP）。
- **能力系统**（capability 声明与审计）见规范章；本册只讲宿主如何在 capability 内分派。
- **工具注册机制**（显式注册 / 静态构造器）见规范章 [RFC 037 §6](../rfc/037-ui.md)；本册只讲 `[AITool]` 的使用方式。

---

上一节：[ui.md](ui.md) · 下一节：[ai-inference.md](ai-inference.md)