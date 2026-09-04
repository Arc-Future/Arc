# RFC 038 AI 宿主

## 背景

Arc.Agent 是 Arc 的端侧进程内**开箱即用**的可信 AI 宿主能力集：强类型、单一惯用法、异步优先、会话协议状态、工具流式接管、人机协同门闩、唯一内置结构化记忆 `AIWiki`、CodeAct 代码执行、MCP 协议接入、多模态内容与工作区管理。它**不是** Agent / RAG / MultiAgent 框架，也**不是**可插拔 Memory 乐高——认知在模型内演化，契约、会话、边界、确认门闩与 Wiki 记忆在 Arc 内固化。

宿主体系统一命名空间 `Arc.Agent`（含 `Arc.Agent.DeepSeek`/`Arc.Agent.OpenAI`/`Arc.Agent.Agnes`/`Arc.Agent.MCP` 模型 Provider 与工具源）；纯推理引擎（`Tensor`/`IAIModel`/`Arc.AI.Onnx`/`Arc.AI.Iree`）见 [041](041-ai-inference.md)，不占此篇。

## 设计决策

### 1. 定位与命名

| 面 | 正道 | 拒绝 |
|----|------|------|
| 命名空间 | `Arc.Agent`（宿主）+ `Arc.Agent.DeepSeek/OpenAI/Agnes/MCP`（Provider/工具源） | `Ai*` 双名 shim；`Arc.AI` 占 Agent 宿主（`Arc.AI` 归推理） |
| 命名 | `AIHost`/`AISession`/`AIWiki`/`[AITool]`/`AIToolSet` 等两字母全大写 | `Ai*`/`IModelBackend` 双名 |
| 开箱 | 依赖 `Arc.Agent` 后仅构造 Provider 与工具即可多轮 + 工具 + 流式/HITL/Wiki | 拼装 MemoryStore/Retriever/Agent/Prompt 乐高 |
| 异步 | `*Async` + `CancellationToken` 为正道 | `Run`/`RunAsync` 同步孪生；`.Result`/`.Wait()` |

实现门禁：**禁止空桩**（无 `todo!()`/空体/Skip 冒充）、**禁止绕过**（Session/HITL/stream/capability）、**禁止降级**（整包缓冲代替流式、同步代替 async、假 Provider 矩阵）。

### 2. 架构拓扑

```text
应用
  └─ AIHost                        ← 唯一入口（Provider + 工具 + 选项 + 可选 AIWiki）
       ├─ AIWiki                   ← 跨会话结构化记忆（唯一内置；非 RAG 乐高）
       ├─ AISession                ← 回合协议状态（transcript / 状态机 / 预算 / HITL）
       ├─ AIToolSet                  ← 编译期工具清单 + capability 分派
       └─ IAIChatClient         ← CompleteAsync / StreamEventsAsync → IAsyncEnumerable<AIStreamEvent>
              ↕ AwaitingHuman（异步等待，不自旋）
         人类：ApproveAsync / Edit / ProvideInputAsync / RejectAsync → Resume
```

用户面正道：

```as
using Arc.Agent;

AIHost host = AIHost.Create(provider, new DeviceTools(), AISessionOptions.Default);
host.Wiki.Upsert("device/battery", "上次读数 87%");   // 跨会话
using AISession session = host.CreateSession();
AIReply reply = await session.RunAsync("读一下电池", ct);
```

`RunAsync` 内含 tool 回合、可选 HITL 暂停、可选工具参数流式接管；用户不得自建 Agent 图或 Memory 管线。

### 3. 会话状态机（AISession）

| 状态 | 语义 |
|------|------|
| `Idle` | 可接受新的 `RunAsync` |
| `Completing` | 模型往返 |
| `StreamingTools` | 工具名已知、参数增量到达（可接管） |
| `AwaitingTools` | 完整 call 就绪 |
| `AwaitingHuman` | 人机门闩，等待 `ApproveAsync`/`RejectAsync` |
| `DispatchingTools` | 沙箱校验通过、执行副作用 |
| `Done` / `Failed` / `Cancelled` | 终止态 |

会话状态：`SessionId`/`Transcript`/`Turn`/`Tools`/`Capabilities`/`Budget`（`MaxTurns`/`MaxMessages`）/`Options`/`PendingHuman`/`ActiveToolStream`。仅 `Idle`/`Done` 可接受新 `RunAsync`；超 `MaxTurns` → `Failed`；`CancellationToken` → `Cancelled`；已执行副作用不自动回滚。窗口策略超预算用单一定道裁剪，**不引入向量检索**。

### 4. 工具与流式接管

`[AITool]` 编译期生成工具清单（`AIToolSet`），schema 在编译期确定，拒绝反射 Invoke。长参数工具（如 `fs_write` 写大文本）支持**流式接管**，边收边消费、低拷贝，避免整段 JSON 缓冲（性能反模式）：

```as
interface IAIToolStreamHandler {
    Task<AIToolStreamDisposition> OnToolCallStartAsync(AIToolCallStart start, CancellationToken ct);
    Task OnToolArgDeltaAsync(AIToolArgDelta delta, CancellationToken ct);
    Task<AIToolResult> OnToolCallEndAsync(AIToolCallEnd end, CancellationToken ct);
}
enum AIToolStreamDisposition { Buffer, TakeOver, Reject }
```

`Buffer`（默认全量拼装）/`TakeOver`（handler 消费增量）/`Reject`（立即失败）单一枚举，禁双轨。`IAIChatClient` 的 `CompleteAsync`/`StreamEventsAsync` 均异步主路径。

**流式主惯用法（IAsyncEnumerable，RFC 008 单一写法）**：Provider 经 `StreamEventsAsync(request, ct)` 返回 `IAsyncEnumerable<AIStreamEvent>` 拉模型异步序列——事件集 `TextDelta`/`ReasoningDelta`/`ToolCallStart`/`ToolArgDelta`/`ToolCallEnd`/`Usage`/`Completed`/`Error`，每个流恰以一个 `Completed` 或 `Error` 终结事件收尾；取消中途表现为 `Error("Cancelled", ...)`。消费侧（宿主 `AISessionStreamCollector` 与业务端）经 `MoveNextAsync` 逐事件拉取，天然背压、不阻塞线程。SSE 线协议解析复用 `std/Net` 通用 `SseDecoder`（对标 .NET `System.Net.Http.SseParser` 分层），Provider 只做 SSE 字段 → 领域事件映射，禁各自维护行式解析。

### 5. 人机协同（HITL）

进入 `AwaitingHuman` 时填充 `PendingHuman`（`AIHumanRequest`：原因、工具草稿 `AIToolCall?`、可选提示、截止策略）。正道 API 全异步：

| API | 语义 |
|-----|------|
| `ApproveAsync(AIToolCall? edited, ct)` | 通过（可带编辑后的 call） |
| `RejectAsync(string? reason, ct)` | 拒绝，写回 transcript |
| `ProvideInputAsync(string text, ct)` | 人类补充输入后回 `Completing` |

`AwaitingHuman` 期间非阻塞等待（异步 pause/resume，禁自旋/`Thread.Sleep`）；不执行工具副作用；可 `Cancel` → `Cancelled`。触发策略单一配置面：`[AITool(RequireApproval = true)]` 或 Session 级策略。审批记录进 transcript 保证可审计。**禁止**第二套 HITL 包与绕过门闩。

### 6. 内置记忆（AIWiki）

`AIWiki` 是**唯一**内置结构化记忆：按路径存取的结构化、可审计、路径化页面表（灵感来自 LLM Wiki——用页面承载长期事实，不用检索链冒充智能）。进程内 `Get`/`Upsert`/`Delete`/`List` 为零等待纯内存（可同步）；落盘引入 I/O 时新增 `*Async` 正道。

```as
class AIWiki {
    AIWikiPage? Get(string path);
    void Upsert(string path, string body, AIWikiMeta? meta = null);
    bool Delete(string path);
    IReadOnlyList<string> List(string? pathPrefix = null);
}
```

`AIHost.Wiki` 为 Host 级默认；`CreateSession` 可覆盖或共享。`AISessionOptions.WikiPathsToAttach` 显式附页进请求（非自动检索）。**拒绝** `IMemory`/`MemoryStore`/`IRetriever`/向量 RAG 乐高；Wiki ≠ Session（Session 管本轮协议与 transcript，Wiki 管跨会话事实）。

### 7. CodeAct 与执行

CodeAct 提供**沙箱内代码执行**：宿主把模型生成的动作代码（在已声明 capability 内）经沙箱执行，工具与代码统一走 capability 分派与 HITL 门闩。沙箱拒绝能力外操作 → 写 transcript 失败消息或回合 `Failed`（单一锁定）。**禁止**绕过 capability 检查执行工具。

### 8. MCP 协议接入

`Arc.Agent.MCP` 以 MCP（Model Context Protocol）对接外部工具源/上下文：宿主通过 MCP 客户端接入符合协议的远端工具与资源，统一纳入 `AIToolSet` 与 capability 体系。MCP 工具源与本地 `[AITool]` 工具在会话内同构消费，不另起第二套工具回路。

### 9. 多模态与工作区

| 面 | 决策 |
|----|------|
| 多模态 | `AIMessage`/`AIRequest` 承载多模态内容（文本/图像等），规范化强类型出入，禁 `object` 袋 |
| 工作区 | 宿主提供工作区会话面（冲突规避、任务回环），多轮任务在会话内管理状态 |
| 沙箱 | tool 必须落在已声明 `capability`；沙箱可审计 |

### 10. Provider 槽

`IAIChatClient` 为唯一 Provider 槽：`CompleteAsync(AIRequest, ct)` + `StreamEventsAsync(AIRequest, ct) → IAsyncEnumerable<AIStreamEvent>`（事件集见 §4）。换 Provider 不换 Host/Session API。Provider 为**完整最小可证伪**实现（真实跑通 Complete/tool 协议），禁空壳接口与假多厂商矩阵。依赖方向单向：`Arc` ← `Arc.Agent` ← `Arc.Agent.<Name>`。

### 11. 与 Harness / AIRfc 的边界

[043](043-harness.md) 的 **`AIRfc`**（小型项目管理 / 需求本尊）与领域 Harness **消费**本 RFC 的 `AIPlan` / `[AITool]` / HITL / 会话事件 / 写协调（`AICoordinator`），**禁止**平行再造。硬约束：`AIRfc` + `AIPlan` + `AITool` **共用**跨会话多任务冲突织物（由本宿主协调面升维提供；契约见下 §13 与 [conflict-fabric](043-harness/references/conflict-fabric.md)）。Harness 细节见 043，不占本篇。

### 12. Plan 编排（AIPlan / AIPlanGate）

宿主提供复杂任务的**结构化计划面**；Harness / AIRfc **直接复用**，禁止平行 `PlanSpec` 或第二套状态机。

| 类型 | 职责 |
|------|------|
| `AIPlan` | 计划本尊：Goal / Analysis / Steps / Verification + 修订号；仅数据结构与状态机 |
| `AIPlanStatus` | `Pending` → `Approved` → `Executing` → `Verifying` → `Completed`；另有 `Rejected`（须修订重审） |
| `AIPlanGate` | Host 级门闩：创建/修订/批准/拒绝/步进；经 `Blocks(capability)` 供调度层统一拦截写能力 |
| `AIPlanContextProvider` | 当前计划读/展示同源 |
| `AIPlanTools` | 内置 `plan` / `revise_plan` / `mark_step_done`（经 Gate `InstallTools`） |

**与 HITL / `PlanGatedCapabilities`：**

| 面 | 契约 |
|----|------|
| 启用条件 | `AISessionOptions.PlanGatedCapabilities` 非空 → Host 装配 `AIPlanGate` |
| 拦截语义 | 能力 ∈ 受约束集合 **且** 计划为 `Pending`/`Rejected` → 调度层拒绝副作用（对模型可见） |
| 无计划 | 简单任务不拦（只读 / 未启用 / 无计划一律放行） |
| HITL | 计划审批走 Gate 的 `Approve`/`Reject`（可与会话 `AwaitingHuman` 并存）；**禁止**第二套审批 API |
| 正交 | PlanGate 管「未批准能否副作用」；冲突织物管「谁可改这份资源」（见 §13）——二者叠加，禁互替 |

**`Completed` 纪律（宣称门闩）：**

- `AIPlanStatus.Completed` **仅允许**在 [043 DoD](043-harness/references/definition-of-done.md)（D0–D7 全勾）驱动后写入；宿主 **禁止**仅凭 `mark_step_done` 步进满额或模型自报「假完成」。
- 宿主可保留步进进度；**终态 Completed 的权威判定在 Harness/DoD**，见 [043](043-harness.md)。
- `mark_step_done` 满额只把计划推进到 **`Verifying`**（待判定态）；转入 `Completed` 的唯一受控入口 = `AIPlanGate.CompleteByDoD()`，由 Harness 汇总门 `AIHarnessSession.CompletePlanAfterDoDAsync` 在 D0–D7 全勾后调用。

实现对照：`std/AI/Agent/Tasks/AIPlan.as`、`AIPlanGate.as`。

### 13. 冲突织物（AICoordinator 升维）

跨会话、多任务并行时，**唯一**冲突门面是升维后的 `AICoordinator`（现有路径写协调的泛化）。三种资源共用同一套 Acquire / Release / 冲突语义，只换键空间与 Commit 载荷。

| 面 | 契约 |
|----|------|
| `AILeaseKind` | `{ ToolPath, Plan, RfcSpec }` —— 三 Kind 一表，禁各搞各的锁 |
| `Acquire` | 登记写意图；冲突 → `Acquired=false`（**后到拒绝**，可审计；**锁死**，不排队、不自旋） |
| `Release` / `ReleaseSession` | 显式放锁；会话结束释放其全部登记 |
| `Commit*` | 持租约下提交（路径：staging→原子落盘；Plan/RfcSpec：校验仍持有后执行突变）；**Commit 不自动放锁** |
| 与 PlanGate | 正交叠加（§12）：未批准仍可被 Gate 拦；已批准仍须通过租约仲裁 |
| 消费方 | `AIRfc` / `AIPlan` / `AITool` **只消费、禁止第二套锁** |

**会话决策事件（追加面目标 API）：**

```text
AISession.AppendDecisionEvent(kind, detail)   // 或 Host 级等价 append-only 面
kind ∈ { airfc:* , approval , checkpoint:* }
```

终态唯一决策轨迹；禁止永久 `HarnessEventLog` 双轨。细节与三 Kind 验收用例见 [conflict-fabric](043-harness/references/conflict-fabric.md)（043 references；本节约宿主面契约）。

### 14. 小模型能力调用（统一门面）

**定位**：小模型能力以「**统一门面直调**」为第一惯用法——开发者在自己的代码里 `new AIModels(registry)` 一次构造、经域子面直接调用（`models.Ocr.RecognizeBatchAsync(...)`），不走工具回路。Agent 会话内可选挂工具是第二路径，但框架**不提供 AIModelTool 封装**：框架只提供注册表与统一门面（[041 §7](041-ai-inference.md)），工具封装是用户代码（用普通 `[AITool]` 自行封装，克制、不过度）。

推理侧统一运行时（`AIModelRegistry` + 统一门面 `AIModels` 与域子面、内存预算与热卸载）见 [041 §7](041-ai-inference.md)；本节约宿主侧消费面：**开发者如何优雅调用** + **Agent 会话内可选集成**。

#### 14.1 适用场景与调用路径选择

先看场景再选路径——判据一句话：**结果消费方是应用代码 → 门面直调；结果消费方是会话内的 LLM → 用户自封装 `[AITool]`**。

| 场景 | 能力（041 §7.5 域子面） | 门面直调（主推） | Agent 会话内（可选） |
|------|------|------------------|----------------------|
| 会议转写 | ASR（`models.Asr.TranscribeAsync` / `TranscribeBatchAsync`） | 应用内实时转写 / 批量归档（批量+进度+取消） | 需 LLM 总结纪要时经 `[AITool]` 自封装 `asr.transcribe` |
| 图片归档 | OCR（`models.Ocr.RecognizeAsync` / `RecognizeBatchAsync`） | 归档管线批量抽取全文（批量+进度+缓存键） | 需 LLM 理解归档内容时经 `[AITool]` 自封装 `ocr.recognize` |
| 多模态问答 | OCR / 多模态理解（`models.Vision.UnderstandAsync`） | 应用内直接问答 | 问答编排在 LLM 会话内时经 `[AITool]` 自封装 |
| 嵌入检索 | Embedding（`models.Embed.EmbedAsync`） | 检索管线批量向量化（批量+可缓存） | 向量不直送 LLM；结果写 Wiki / 自管存储 |
| 语音合成朗读 / TTS 播报 | TTS（`models.Tts.SynthesizeAsync`） | 应用内朗读 / 播报管线（非幂等默认不重试） | 播报动作在会话内编排时经 `[AITool]` 自封装 |

#### 14.2 统一门面直调（主推）

`AIModels` 是**唯一入口**（041 §7.5：统一门面，替代每域一类的 7 次构造）；域方法是门面方法、域子面强类型返回（请求/响应模型 OpenAI 协议对齐，041 §7.5：对齐/扩展/自定义三档 + 命名映射 + 关键裁决）。用户最少代码（约 3–6 行）：

```as
using Arc.AI;
using Arc.AI.Models;

AIModelRegistry registry = new AIModelRegistry(AIModelRegistryOptions.Default);
registry.Register(ocrRegistration);                          // 模型注册（041 §7.2）
using AIModels models = new AIModels(registry);              // 统一门面一次装配
List<AIOcrRequest> requests = pages.Select(p => new AIOcrRequest {
    Input = AIImageInput.FromFile(p),
}).ToList();
List<AIOcrResult> results = await models.Ocr.RecognizeBatchAsync(requests, null, ct);  // 批量+进度+缓存键
Console.WriteLine(results.Select(r => r.Text).Join("\n"));   // 全文进归档
```

- **统一面**：`models.Registry` / `models.Budget`（读预算与统计，041 §7.2 不变）/ `models.Options`（全局默认：批量大小/进度/缓存策略/重试）；`Dispose()` 幂等。
- **门闩降级**：门面子面经 `.ani` `load="auto"` 门闩——原生库未装 → `Native.IsAvailable == false` → 抛 `AIModelNotAvailableException` / `OnnxNotAvailableException`，显式失败面、绝不崩溃；应用捕获后灰化该能力（[041 §3](041-ai-inference.md) 降级链同构）。
- **生命周期**：注册表引用计数 / 内存预算 / 热卸载对调用方透明（041 §7.2）；`using` 释放门面。

#### 14.3 Agent 会话内使用（可选第二路径 · 用户 [AITool] 自封装）

若用户想在会话中让 LLM 调小模型，**用普通 `[AITool]` 自己封装**——**框架不提供 `AIModelTool` / `AIModelToolFactory`**，不预置领域模型工具；统一门面与注册表在 041，工具封装是用户代码（克制，不过度）。

最小示例（用户代码）：

```as
using Arc.Agent;
using Arc.AI.Models;

/// 用户自封装：门面子面包成普通 [AITool]，组合根注入门面实例。
class OcrTool {
    private readonly AIModels _models;

    public OcrTool(AIModels models) { _models = models; }

    [Description("识别图片中的文字")]
    [AITool("ocr.recognize", Capability = "ai.Model.Ocr")]
    public async Task<string> RecognizeAsync([Description("图片文件路径")] string imagePath) {
        AIOcrResult result = await _models.Ocr.RecognizeAsync(
            new AIOcrRequest { Input = AIImageInput.FromFile(imagePath) }, ct);
        return result.Text;
    }
}

using AIModels models = new AIModels(registry);               // 组合根装配（14.2）
AIHost host = AIHost.Create(provider, new OcrTool(models), AISessionOptions.Default);  // 编译期合成 AIToolSet
using AISession session = host.CreateSession();
AIReply reply = await session.RunAsync("识别这张图并总结", ct);
```

复用既有回路，零新设施：`[AITool]` 编译期合成 `AIToolSet` → capability fail-closed → 沙箱 / HITL / 流式接管 / 审计全部沿用（§4 / §5）；`AICapabilitySet` 未授权 → `CapabilityDenied`（不调 handler）。

#### 14.4 多模态结果进上下文

| 结果形态 | 进上下文路径 |
|----------|--------------|
| 文本 / JSON（OCR 全文、ASR 转写） | 门面调用方（应用或 `[AITool]`）把文本放回消息 / 工具结果 → transcript 既有回路 |
| 图像 / 音频 | `AIMessage.ContentParts` 扩展部件（P3 增 `AIAudioPart` / `AIVectorPart`，如需要）供支持多模态的 Provider 序列化 |
| 向量 | 不直送 LLM（远端 Provider 一般不收向量）；以 `Summary` 文本块进上下文，向量本体可写 `AIWiki`（**不引入宿主向量检索**，§6 边界保持） |

`AIModelContextProvider` **降级为可选提示**（非框架强制装配）：用户需要时经 `AIContextEngine.AddProvider` 自注，把结构化结果以 `Kind="Knowledge"` + 稳定 Priority 的 `AIContextBlock` 注入；不需要就不挂。框架不预置模型上下文 Provider。

```as
// Arc.Agent（多模态部件扩展 · AIContentPart 派生，如需要）
public class AIAudioPart : AIContentPart {   // OpenAI content type="input_audio"
    public string Data; public string Format;   // Data = base64 编码音频；Format "wav"/"mp3"
}

// 本地模型结果部件（向量摘要进上下文展示）
public class AIVectorPart : AIContentPart {
    public AIVector Vector;                 // 引用 Arc.AI.Models.AIVector
    public string Summary;                  // 文本摘要（进上下文展示）
}
```

#### 14.5 回合预算（可选护栏）

`AISessionOptions.ModelBudget` 保留为**可选护栏**：仅当用户给会话挂了小模型 `[AITool]` 时用于防超预算（每回合模型调用次数 / 累计成本上限）；超限 → 工具结果失败（`ModelBudgetExceeded`）对模型可见（非静默），模型可换路。**默认不强制**——纯门面直调不经会话回合，预算不适用。

**为什么这么设计**：本地模型 CPU 推理慢，挂工具进会话回合时模型可能频繁触发昂贵能力，需要可选护栏与调度信号；护栏失败对模型可见（可换路），不静默吞。

#### 14.6 与 041 §7 的边界

| 面 | 归属 |
|----|------|
| 注册表 / 服务基座 / 统一门面 `AIModels` 与域子面 `Arc.AI.Models` / 值类型 / 异常层次 | [041 §7](041-ai-inference.md)——推理侧，本 RFC 不重复 |
| 内存预算 / 热卸载 | [041 §7](041-ai-inference.md) |
| 开发者统一门面直调、Agent 会话内可选集成、多模态结果进上下文、回合预算护栏 | 本 §14 |
| 模型工具封装 | **用户代码**（普通 `[AITool]`），038 / 041 均不提供 |

## 边界

- **纯推理引擎**（`Tensor`/`IAIModel`/`Arc.AI.Onnx`/`Arc.AI.Iree` 推理后端）见 [041](041-ai-inference.md)；本 RFC 只讲宿主（会话/工具/HITL/Wiki/CodeAct/MCP/Plan/冲突织物）。
- **小模型基础设施推理侧**（`AIModelRegistry`/`AIModelService`/统一门面 `AIModels` 与域子面 `Arc.AI.Models`）见 [041 §7](041-ai-inference.md)；本 RFC §14 只讲开发者统一门面直调 + Agent 会话内可选集成（多模态结果进上下文 / 回合预算护栏）。
- **Coding Agent Harness / AIRfc** 见 [043](043-harness.md)；冲突消费约定见 [conflict-fabric](043-harness/references/conflict-fabric.md)。
- **能力系统**（capability 声明与审计）见对应协作章节；本 RFC 只讲宿主如何在 capability 内分派。
- **表达式树 / 查询翻译**见 [011](011-expression-trees-query.md)。
- **工具注册机制**（显式注册 / 静态构造器）见 [037 §6](037-ui.md)；具体领域装配见 037/040。

---
上一节：[037 UI 声明式框架](037-ui.md) · 下一节：[039 ORM 与 SQL 翻译](039-orm.md)
