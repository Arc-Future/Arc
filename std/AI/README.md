# std/AI（AI 解决方案目录 · 目录聚合标准）

**本目录 = 所有 AI 相关包的解决方案根**，对齐 `std/Web/`（Core/Grpc/SignalR 子包）同构。
权威设计：[RFC 038](../../docs/rfc/038-ai-host.md)（Agent 宿主 · `Arc.Agent`）、[RFC 041](../../docs/rfc/041-ai-inference.md)（推理引擎 ONNX + IREE · `Arc.AI`）。
架构红线：**编译器核心 7 crate 零领域逻辑**；两体系均经 `.ani` `load="auto"` 运行时加载外部 DLL（RFC 034，Track H 零编译器改动）。

## 两大部分 · 两个顶层命名空间

| 大部分 | 命名空间（顶层） | 职责 |
|--------|------------------|------|
| **Agent 体系** | `Arc.Agent` + `Arc.Agent.DeepSeek/OpenAI/Agnes/MCP` | 会话编排 / 工具 / 上下文 / 记忆 / 模型 Provider |
| **推理体系** | `Arc.AI` + `Arc.AI.Onnx/Iree` | 宿主张量 / `IAIModel` / ONNX / IREE |

**判别规则**：Agent 体系统一 `Arc.Agent.*`；推理体系统一 `Arc.AI.*`——两个顶层命名空间各自独立、互不冲突，名字精简、一眼可辨。小模型基础设施（[041 §7](../../docs/rfc/041-ai-inference.md) / [038 §14](../../docs/rfc/038-ai-host.md)）延续此规则：统一门面 `AIModels` 与域子面归 `Arc.AI.Models`（推理侧），Agent 侧仅多模态结果部件归 `Arc.Agent.Models`（模型工具封装是用户代码，框架不提供）。

## 目录结构

```
std/AI/
├── Agent/                  # Arc.Agent           Agent 宿主核心（Context/Human/Host/Memory/Messages/Providers/Sessions/Skills/Tasks/CodeAct/Tools/Workspace）
│     ├── arc.toml
│     └── ...
├── Agent.DeepSeek/         # Arc.Agent.DeepSeek   Agent 模型 Provider（真实 API）
├── Agent.OpenAI/           # Arc.Agent.OpenAI     Agent 模型 Provider（OpenAI 官方 API）
├── Agent.Agnes/            # Arc.Agent.Agnes       Agent 模型 Provider（云端 LLM 供应商 · OpenAI 兼容协议 · 推理模型）
├── Agent.MCP/              # Arc.Agent.MCP        Agent 工具源
├── Agent.Harness/          # Arc.Agent.Harness    基座（RFC 043：AIRfc / DoD 门骨架）
├── Agent.Harness.Coding/   # Arc.Agent.Harness.Coding  quality.* / D0–D7 判定（H-2b 已落地）
├── Agent.Models/           # Arc.Agent.Models     多模态结果部件（AIAudioPart/AIVectorPart · 规划中（041 §7 P3 待排期）；模型工具封装是用户代码）
├── Core/                   # Arc.AI               推理共享核心（Tensor 宿主张量 / IAIModel 统一执行接口 / AIModelRegistry 统一模型运行时 / AIModelService 基座）
│     ├── arc.toml
│     ├── Tensor.as
│     └── IAIModel.as
├── Models/                 # Arc.AI.Models        统一门面 AIModels + 域子面（Ocr/Asr/Embed/Clip/Face/Tts/Vision）+ 请求/响应强类型模型（OpenAI 协议对齐）+ 值类型 · 规划中（041 §7 P2 待排期）
├── Onnx/                   # Arc.AI.Onnx          ONNX 后端（自 std/AI.ONNX 迁入）
└── Iree/                   # Arc.AI.Iree          IREE 后端（RFC 041 · M-I0 门闩/M-I1 执行后端已落地）
```

## 命名 / 分层原则

- **短命名（两顶层命名空间）**：Agent = `Arc.Agent`，推理 = `Arc.AI`；最深层仅 `Arc.Agent.DeepSeek` / `Arc.Agent.Agnes` / `Arc.Agent.Harness` / `Arc.AI.Onnx`（3 段），无多余嵌套。
- **AI 前缀命名收敛**：非 Provider 公开类型一律 `AI` 前缀（`AISession`/`AITool`/`AIDoDGateResult`/`AILeaseKey`/`AIToolSet` 等），简短明确、识别度高；Provider 包（`Arc.Agent.DeepSeek` / `Arc.Agent.OpenAI` / `Arc.Agent.Agnes` / `Arc.Agent.MCP` / `Arc.Agent.Harness.Coding`）保留非 AI 前缀技术名（`DeepSeekChatClient` / `OpenAIChatClient` / `AgnesChatClient` / `QualityTools` / `CodingDoDGateEvaluator`）。
- **接口命名（`IAI*`）**：非 Provider 接口一律 `I` + `AI` 前缀（`IAIChatClient` / `IAIDoDGateEvaluator` / `IAIModelFactory` / `IAIFixRoundProvider`）。2026-08-16 收官审计统一：`AIModelBackend`→`IAIModelFactory`、`IFixRoundProvider`→`IAIFixRoundProvider`、绿点类型 `Checkpoint*`→`AICheckpoint*`（`AICheckpointSnapshot`/`AICheckpointIndex`/`AICheckpointIndexEntry`/`AICheckpointRollbackOutcome`/`AICheckpointFileEntry`）。
- **分层原则（基类在上、派生在下）**：Agent 基类（`IAIChatClient`/`AIToolHandler`/`AIContextHost`）在 `Arc.Agent`，Provider/Harness 派生在 `Arc.Agent.DeepSeek` / `Arc.Agent.Harness` 等；推理基类（`Tensor`/`IAIModel`）在 `Arc.AI`，后端在 `Arc.AI.Onnx/.Iree`。子命名空间引用穿透父命名空间（正向），无反向反模式。
- **各读各自域 · 禁双轨 API**：`Arc.AI.Onnx` 只服务 ONNX 图，`Arc.AI.Iree` 只服务 `.vmfb`；业务侧经共享 `Arc.AI.IAIModel` 统一消费、不感知后端差异（RFC 041 §1.1）。

## 依赖方向（单向）

```
Arc.Agent.DeepSeek / Arc.Agent.OpenAI / Arc.Agent.Agnes / Arc.Agent.MCP / Arc.Agent.Harness  →  Arc.Agent
Arc.AI.Onnx / Arc.AI.Iree                                 →  Arc.AI
Arc.AI.Models     →  Arc.AI（统一门面 AIModels + 域子面 · 零 Agent 依赖）
Arc.Agent.Models  →  Arc.Agent + Arc.AI.Models（多模态结果部件 · 模型工具封装是用户代码）
Arc.Agent  →  Arc.AI（可选，本地模型即经 IAIModel 消费）
```

- **推理体系完全独立**：`Arc.AI.*` 不依赖 Agent（`Arc.Agent`）；Agent 可消费推理，方向单向。
- **小模型是附加能力，非取代 LLM 核心**：`Arc.AI` 仍以 `Tensor`/`IAIModel`/`Onnx`/`Iree` 大模型推理框架为核心；`Arc.AI.Models`（统一门面 `AIModels` 与域子面）复用同一推理运行时，优雅接入本地小模型（ASR/OCR/TTS/CLIP/人脸/嵌入）。见 [041 §7](../../docs/rfc/041-ai-inference.md)。
- **统一门面是唯一入口**：小模型能力对外一个 `AIModels`（域是门面子面，非独立类）；模型工具封装是用户代码（普通 `[AITool]`），框架不提供 `AIModelTool`。
- 物理聚合在 `std/AI/` 下，各子包 `arc.toml` 相对依赖路径随迁移同步更新。

## 状态

- Agent 体系 `Arc.Agent`（原 `Arc.AI`）与 `Arc.Agent.DeepSeek/OpenAI/Agnes/MCP`（DeepSeek/MCP 原 `Arc.AI.DeepSeek/MCP`，OpenAI/Agnes 新增）：**已存在**（历史目录 `std/AI*` 已按本标准聚合改名完毕，无残留目录）。新增 Provider 状态：`Arc.Agent.OpenAI` **已落地（2026-08-16）**；`Arc.Agent.Agnes` **已落地（2026-08-16，真实连通验证）**。
- **Harness 基座** `Arc.Agent.Harness`：**AIRfc**（小型 PM / 需求本尊）+ DoD 门骨架；**Coding** 包 `Arc.Agent.Harness.Coding`（H-2b ✅）承载 `quality.*` 与 D0–D7 判定。动手前必读 [RFC 043](../../docs/rfc/043-harness.md) 与 [llm-gates](../../docs/rfc/043-harness/references/llm-gates.md)。H-2c 已删 `PlanSpec`/`HarnessAnchor`；决策轨迹（`airfc:*`/`checkpoint:*`/`work_summary`）经 `AISession.AppendDecisionEvent` 写入 Agent 会话事件（M5–M6 ✅，已删 Harness 独立事件日志双轨）。
- 推理体系 `Arc.AI`（原 `Arc.AI.Engine`）：共享核心 `Tensor`/`IAIModel` **已建**（`std/AI/Core/`，S1 收口，`core_tensor_e2e` 非 Skip 通过）；`Arc.AI.Onnx` 由 `std/AI.ONNX`（M-O0–M-O2 已实现）迁入——内部类收 `internal`、公开工厂入口（`OnnxModelFactory` 工厂 + 公开 `SessionOptions` 会话选项，S2 收口，`onnx_runner_e2e` 全量编译通过、真库待构建软跳过）；`Arc.AI.Iree`（M-I0 门闩 + M-I1 执行后端已落地，`iree_infer_e2e`/`iree_unavailable_e2e`/`iree_tensor_e2e` 编译通过；M-I2+ 待排期）。
- **小模型基础设施（RFC 041 §7 / 038 §14 · P1 已落地 2026-08-16）**：本地领域小模型（ASR/OCR/TTS/CLIP/人脸/嵌入/多模态）成为一类基础设施——推理侧 `AIModelRegistry`/`AIModelService`/统一门面 `AIModels` 与域子面 `Arc.AI.Models`（域子面批量+进度+缓存键；请求/响应强类型模型以 OpenAI 为基准，对齐/扩展/自定义三档 + 命名映射，041 §7.5），Agent 侧用户 `[AITool]` 自封装（可选）+ 多模态结果部件/上下文注入；`IAIModel` 张量层不动，语义层在门面子面扩展；`.ani` 新原生 shim 同构，编译器核心零改动。**P1（统一运行时 + 注册表）已落地**：`Tensor.Int16`（CreateInt16/ReadInt16）+ `AIModelRegistry`（懒加载/引用计数/预算 4 GiB/LRU/统计/Dispose 幂等）+ `AIModelService` 基座 + 六层异常 + `OnnxAIModelFactory`/`IreeAIModelFactory` 适配，验收 `ai_model_registry_e2e` 全绿（证据见 实现规划）。**P2（OCR/嵌入/ASR 域子面 + 门面）→ P3（Agent 会话内集成 + 多模态上下文）→ P4（TTS/CLIP/人脸 + 流式 + Skill 化）待排期**，不自动开干。
- **诚实标注（实战差距审查 P0-1）**：`AIOcrFace` / `AIAsrFace` / `AIEmbedFace` 只提供「管道 + 调用面」，**不含真实识别/转写/嵌入能力**——真实识别需用户自备 ONNX 模型 + 前后处理（tokenizer / CTC / 段级解码），框架当前**不内置任何预训练模型与 tokenizer/解码器**；未注入真实后端即「不可识别」。不得把「仅测试 fixture 能跑通管道」当作「已具备识别能力」对外宣称。
