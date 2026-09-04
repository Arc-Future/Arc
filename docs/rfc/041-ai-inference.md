# RFC 041 AI 推理（Arc.AI）

## 背景

`Arc.AI` 是 Arc 的**推理引擎**领域：宿主张量 + `IAIModel` 共享核心，配 `Arc.AI.Onnx`（ONNX Runtime）与 `Arc.AI.Iree`（IREE）两个互补后端。推理引擎经 `.ani` `load="auto"` **运行时加载外部 DLL**，编译器核心 7 crate 零领域逻辑、零改动（架构红线）。Agent 宿主（`Arc.Agent`）见 [038](038-ai-host.md)。

## 设计决策

### 1. 定位：两个互补后端（禁双轨 API 混乱）

| 维度 | `Arc.AI.Onnx`（ONNX Runtime） | `Arc.AI.Iree`（IREE） |
|------|-------------------------------|------------------------|
| 执行模型 | 即时解释 + EP 插件 | 提前编译（AOT）+ 多后端 |
| 编译时机 | 无（运行期加载图） | `iree-compile` 产出 `.vmfb` |
| GPU | 依赖 EP（DirectML） | 一等公民（Vulkan/CUDA 等） |
| 部署形态 | 解释器 + 模型文件 | 编译产物 `.vmfb`（零编译开销） |
| 适用 | 灵活、快速上手、EP 生态 | 高性能、异构、边缘/GPU |

**单一惯用法**：`Arc.AI.Onnx` 只服务 ONNX 图（`InferenceSession`），`Arc.AI.Iree` 只服务 `.vmfb` 执行（`IreeSession`）——各读各自域，禁双轨 API。业务侧若需「同一模型两种后端」，经共享抽象层 `Arc.AI` 的 `IAIModel` 选择适配器，不在各后端内部提供「另一后端路径」。

### 2. 命名空间与包

| 命名空间 | 目录 | 内容 |
|----------|------|------|
| `Arc.AI` | `std/AI/Core/` | 宿主张量 `Tensor` + `IAIModel` 抽象（共享核心） |
| `Arc.AI.Onnx` | `std/AI/Onnx/` | `InferenceSession`/`OnnxTensor`/`SessionOptions` + 枚举/异常 |
| `Arc.AI.Iree` | `std/AI/Iree/` | `IreeSession`/`IreeBufferView`/`IreeDevice`/`IreeModule` + 枚举/异常 |

依赖单向：`Arc` ← `Arc.AI` ← `Arc.AI.Onnx`/`Arc.AI.Iree`（后端 → 共享核心）。Agent 宿主（`Arc.Agent`）可消费 `Arc.AI` 但**不依赖**（依赖方向 Agent → `Arc.AI` 可选）。小模型基础设施扩展命名空间（`Arc.AI.Models` / `Arc.Agent.Models`）见 §7.6。

### 3. 独立 crate 体系（.ani 运行时加载）

两后端同构，完全镜像 zxing「外部依赖不 vendored」路径：

| 层 | ONNX | IREE | 说明 |
|----|------|------|------|
| std 包 | `std/AI/Onnx/` | `std/AI/Iree/` | 门面 `.as`（public）+ `XxxNative.as`（internal） |
| 原生契约 | `crates/arc/native/onnx.ani` | `crates/arc/native/iree.ani` | `load="auto"` + `library = Environment.GetEnvironmentVariable("ARC_XXX_LIB")` + `Native.IsAvailable` 门闩 |
| C ABI shim | `crates/runtime-onnx/` | `crates/runtime-iree/` | `extern "C"` 包 C++ API → 精简 C 契约；版本锁定锚点 |
| vendoring | `fetch/build-onnx-native.ps1` | `fetch/build-iree-native.ps1` | 产物落 `target/xxx-native/`（不进仓库） |
| e2e | `onnx_unavailable_e2e` / `onnx_runner_e2e` | `iree_unavailable_e2e` / `iree_infer_e2e` | 降级链 + 真库路径 |

**降级链**：未装库 → `Native.IsAvailable("xxx") == false` → 门面抛 `OnnxNotAvailableException`/`IreeNotAvailableException`；未门闩的原生调用抛 `NativeLibraryNotFoundException`（显式失败面，绝不崩溃）。编译器核心零改动（`.ani` 走既有 `load` 机制，Track H 零改动）。

C ABI shim 原则：异常状态 shim 内闭环（对 Arc 只回 `int` + `last_error(List<byte>)`，永不裸透传原生状态对象）；句柄所有权 = 创建即调用方所有、`Dispose()` 幂等释放；零拷贝缓冲经 `rt_list_buffer_and_size` ABI；错误串两遍取回；shim 对死版本编译（`VENDOR.md` 记录版本/SHA256）。

### 4. 共享抽象层（Arc.AI）

| 抽象 | 职责 |
|------|------|
| `Tensor` | 宿主值类型：`Shape`/`ElementType`/`Rank`/`Total` + 类型化读写（`ReadFloat()`/`ReadInt64()` 等），仅承载宿主可见张量，抽象掉「ONNX Value vs IREE buffer_view」所有权/设备内存差异 |
| `IAIModel` | `Run(inputs) → outputs` + 元数据（`InputCount`/`OutputCount`/`GetInputName(i)`）；ONNX/IREE 各自实现适配器 |

```as
public interface IAIModel {
    Task<Tensor[]> RunAsync(Tensor[] inputs, CancellationToken ct);
}
```

诚实边界：共享抽象只暴露「宿主张量 + 结果张量」，**不承诺跨设备零拷贝**；高性能路径可直接用各后端原生类型（不强制抽象）。

### 5. IREE 开箱即用闭环

| 段 | 机制 | 产物 |
|----|------|------|
| 编译段 | `iree-import-onnx model.onnx → .mlir → iree-compile --iree-hal-target-backends=llvm-cpu -o model.vmfb` | `.vmfb` |
| 执行段 | `iree.ani` `load="auto"` → `IreeSession` 加载 `.vmfb`、构造 buffer_view、`invoke`、读回 `Tensor` | 推理结果 |

运行期只加载 `.vmfb`，不依赖编译器；编译段仅在需要产出 `.vmfb` 时用。驱动降级语义：`iree_device_driver_available` 探测驱动加载，`iree_create_device` 失败 → 门面确定性回退 `local-task`，绝不崩溃。

### 6. 拒绝项

| 项 | 裁决 |
|----|------|
| ONNX/IREE 融合执行引擎、统一算子调度器 | 拒绝——两后端互补不融合 |
| 内嵌 `IREECompiler.dll`（重量级 LLVM） | 拒绝——首期走外部 `iree-compile` |
| 双轨 API（后端内部提供另一后端路径） | 拒绝——各读各自域，经 `IAIModel` 选择适配器 |
| 跨设备零拷贝承诺 | 拒绝——诚实边界，高性能路径用后端原生类型 |
| 新外部依赖未经批准 | 拒绝——GPL 等非宽松许可否决；`VENDOR.md` 登记 |

### 7. 小模型基础设施（本地领域小模型）

#### 7.1 定位与设计原则

**定位（不取代 LLM 核心）**：`Arc.AI` 始终以**大模型推理框架为核心**（`Tensor`/`IAIModel`/`Onnx`/`Iree`，即 LLM 与推理主干）。小模型基础设施是建立在同一核心之上的**附加能力**——本地领域小模型（ASR / OCR / TTS / CLIP / 人脸 / 文本嵌入 / 多模态理解）以「被统一管理的一类资产」形态接入：既被 `Arc.AI` 统一管理（注册 / 懒加载 / 引用计数 / 内存预算 / 热卸载 / 统计审计），又能被 `Arc.Agent` 作为工具/能力消费（[038 §14](038-ai-host.md)）。它**不改变** `IAIModel` 张量契约、**不炸开** Tensor 类型、**不引入**领域类型进核心——大模型与小模型共用同一推理运行时，小模型只是语义服务层的增量。约束：16G 普通电脑（内存预算默认 4–6 GiB）、单线程宿主、编译器核心 7 crate 零领域逻辑。

| 原则 | 决策 |
|------|------|
| **张量层不动，语义层扩展** | `IAIModel` 是 §4 已接受契约（`RunAsync(List<Tensor>, ct)`），保持原始位置序张量契约，禁领域语义污染；多模态语义在 `Arc.AI.Models` 统一门面的域子面表达（值类型 ↔ Tensor 翻译） |
| **Tensor 最小扩展（P1）** | 新增 `TensorElementType.Int16`（PCM int16 音频、部分量化推理输出）；图像（UInt8 `[H,W,C]`）、文本 token（Int64）、向量（Float32）已被现有元素类型覆盖；`String` 张量（tokenizer 直出）不在本面（后续能力），按需再议 |
| **热卸载对齐 RFC 017 闭环** | 对齐 [017](017-build-artifacts-packages.md) `rt_library_*`（ref ledger / call enter-leave / root scan / weak neutralize / `rt_library_unload_hot`） |
| **Agent 侧零新回路** | 会话内集成复用 `[AITool]`/`AIToolSet`/`AICapabilitySet`/`AIContextEngine` 既有回路；统一门面直调不经工具回路（038 §14） |

**为什么这么设计**：小模型基础设施是「被统一管理的一类资产」，不是「每个域一套自嗨封装」。把生命周期/预算/错误收在推理侧一个注册表 + 一个服务基座，把 Agent 会话内消费收在既有工具回路（用户 `[AITool]` 自封装）——一个模型一种用法，无双轨。

#### 7.2 统一模型运行时（AIModelRegistry）

进程级模型组合根，与 `AIHost` 同构（构造注入，`Dispose` 幂等）。

| 面 | 决策 |
|----|------|
| 形态 | 进程级组合根；`AIModelRegistryOptions` 承载预算/卸载策略 |
| 注册 | `Register(AIModelRegistration)` 静态声明；`ModelId` 为唯一键（如 `"ocr/paddleocr"`、`"asr/whisper-small"`）；重复注册按名覆盖 |
| 后端解耦 | 注册表**不引用任何后端包**（依赖方向红线）。加载经 `Func<AIModelRegistration, IAIModel>` 工厂；后端包提供 `OnnxAIModelFactory`/`IreeAIModelFactory`（实现 `IAIModelFactory`）与 `UseOnnx()`/`UseIree()` 装配助手，组合根注入 |
| 懒加载 | `Acquire(modelId)` 首次命中才创建底层 runner（`.onnx` 会话 / `.vmfb`）；创建成本一次性摊销 |
| 引用计数 | 每次 `Acquire` +1；`AIModelHandle.Dispose()` −1；归零 → 进入可卸载候选（策略裁决：立即卸载 / 温窗保持 `WarmKeepSeconds` 后卸载） |
| 内存预算 | `AIModelBudget` 记账常驻字节（注册表按 `SizeBytes` 估算 + 峰值工作区）；超限 → 按 LRU 驱逐空闲模型或拒绝加载（`AIModelBudgetExceededException`）；16G 普通电脑默认 4–6 GiB，可配置 |
| 预热 | `WarmUpAsync(modelId, ct)` 提前加载并常驻；`AIModelCache` 语义即「注册表 + 温窗 + LRU」 |
| 统计/审计 | `LoadedCount` / `ResidentBytes` / `GetStats(modelId)`（调用数/延迟/命中/驱逐）；`SetEvents(AIModelRegistryEvents)` |
| 并发 | 注册表操作遵循单线程宿主约束（Host 驱动）；模型实例自身保持既有「单实例串行化」（内部 `Lock`，同 `InferenceSession`） |

**热卸载闭环（对齐 RFC 017 `rt_library` 热卸载 + RFC 005 ARC）：**

| 环节 | 机制 |
|------|------|
| 卸载前置 | 引用计数归零 + 策略放行 + **在途调用收敛**（门面内部 per-handle 活动调用计数，等价 `rt_library_call_enter/leave` 语义） |
| 外部引用 | 活跃 `AIModelHandle` 即注册表持强引用的根；`AIModelWeakHandle` 经宿主弱登记表，卸载时中和（`Weak<T>.TryGet()` 确定性返回 null，不复活） |
| 根扫描 | 注册表是模型对象的**单一根所有者**；卸载序列 = 释放全部会话句柄 → 触发对象域 ARC 归零 → 组件层清理 |
| 动态库包 | 领域模型包若以 `--dynamic` 分发 → 走 `rt_library_unload_hot`（Freeze → 在途收敛 → ledger 归零 → weak 中和 → 释放根 → dlclose → tombstone） |
| 诚实边界 | 原生推理库（onnx.dll）进程级卸载属硬问题（全局状态），明确不做 `dlclose`；以「每实例 Dispose + 在途收敛」为闭环，进程级原生库卸载留待有真实需求再立 RFC |

**API 草图（`Arc.AI`）：**

```as
public class AIModelRegistryOptions {
    public long MemoryBudgetBytes;        // 16G 机器建议 4–6 GiB；0 = 不设限
    public AIModelEvictionPolicy Eviction; // None / Lru
    public int WarmKeepSeconds;            // 引用归零后保持常驻窗口（0 = 立即卸载）
    public int MaxConcurrentPerModel;      // 单模型并发上限（默认 1，串行化）
}

public class AIModelRegistration {
    public string ModelId;                 // 唯一键："asr/whisper-small"
    public string DisplayName;
    public AIModelKind Kind;               // Asr/Tts/Ocr/Clip/Face/Embedding/Vision/Generic
    public AIModelQuantization Quantization; // Float32/Float16/Int8/Int4
    public long SizeBytes;                 // 常驻内存估算（预算计账）
    public string Capability;              // 默认 "ai.Model"
    public AIModelLoadPolicy LoadPolicy;   // Lazy / Eager / Warm
    public Func<AIModelRegistration, IAIModel> Factory; // 后端注入点
}

public class AIModelRegistry : IDisposable {
    public void Register(AIModelRegistration reg);
    public AIModelHandle Acquire(string modelId);          // 懒加载 + refcount+1
    public Task WarmUpAsync(string modelId, CancellationToken ct);
    public AIModelBudget Budget { get; }
    public int LoadedCount { get; }
    public long ResidentBytes { get; }
    public AIModelStats GetStats(string modelId);
    public void SetEvents(AIModelRegistryEvents events);
    public void Dispose();                                 // 幂等：释放全部
}

public class AIModelHandle : IDisposable {
    public string ModelId { get; }
    public IAIModel Runner { get; }     // 语义面仍经 IAIModel 消费
    public AIModelStatus Status { get; }    // Cold/Warming/Ready/Evicted/Failed
    public void Dispose();                  // refcount-1；归零且策略放行 → 可卸载
}
```

**简洁示例**：一个模型注册 + 获取，懒加载与引用计数对调用方透明：

```as
AIModelRegistry registry = new AIModelRegistry(AIModelRegistryOptions.Default);
registry.Register(new AIModelRegistration {
    ModelId = "ocr/paddleocr", Kind = AIModelKind.Ocr, SizeBytes = 512 * 1024 * 1024,
    Factory = reg => OnnxModelFactory.Create("models/paddleocr.onnx"),
});
using AIModelHandle handle = registry.Acquire("ocr/paddleocr");   // 懒加载 + refcount+1
List<Tensor> outputs = await handle.Runner.RunAsync(inputs, ct);
// handle.Dispose() → refcount-1；归零且策略放行 → 进入可卸载候选
```

#### 7.3 统一服务基座与异常层次（AIModelService）

统一接口的落点：**抽象基类 + 统一执行骨架**（与 `AIToolHandler` 同构；纯 interface 跨程序集分派存在已知编译器缺口，遵循既有抽象基类惯用法）。

```as
public class AIModelServiceOptions {
    public int TimeoutMs;                   // 单次调用超时（默认取注册表默认）
    public int MaxRetries;                  // 幂等推理重试；默认 0（不重试非幂等）
    public int RetryBackoffMs;
    public AIModelCost CostClass;           // Fast/Medium/Slow（Agent 回合调度用）
    public bool TrackUsage;                 // 调用数/延迟/内存统计
}

public abstract class AIModelService {
    protected AIModelService(AIModelRegistry registry, string modelId, AIModelServiceOptions options);

    // 统一执行骨架：Acquire → 超时(CancellationTokenSource) → 重试(幂等) →
    // 序列化(单实例锁) → 执行 → 释放 → 统计
    protected Task<AIModelResult> ExecuteAsync(
        Func<IAIModel, Task<AIModelResult>> work, CancellationToken ct);
}
```

**统一异常层次**（对齐 §3 门闩降级链既有模式）：

```text
AIModelException
 ├─ AIModelNotAvailableException    （Native.IsAvailable == false，门闩灰化）
 ├─ AIModelLoadException            （加载/初始化失败）
 ├─ AIModelTimeoutException         （超时，可重试）
 ├─ AIModelBudgetExceededException  （内存预算/回合预算超限）
 ├─ AIModelInferenceException       （后端推理失败，包装底层错误）
 └─ AIModelCancelledException       （协作式取消）
```

**为什么这么设计**：超时/重试/预算/序列化/统计一处落地，门面内部域实现只写「语义翻译 + 调用」。重试仅幂等推理（嵌入/OCR 单输入默认允许，指数退避）；TTS 等非幂等默认 `MaxRetries = 0`。后端错误在服务层收敛为 `AIModelError`，不裸透原生状态。

**错误契约（对齐 OpenAI error object）**：`AIModelError` 持 `Message`/`Type`/`Code`/`Param`（对应 OpenAI `{"error": {"message", "type", "param", "code"}}`），门面据此映射统一异常层次：

| 条件 | Arc 异常 |
|------|----------|
| `Type = rate_limit_error`（429 速率超限） | `AIModelBudgetExceededException` |
| `Type = insufficient_quota`（配额/预算不足） | `AIModelBudgetExceededException` |
| `Type = server_error`（5xx） | `AIModelInferenceException`（可重试） |
| 请求级超时 / `Type = timeout` | `AIModelTimeoutException`（可重试） |
| `Type = invalid_request_error`（400 校验失败） | `AIModelException` |
| `Native.IsAvailable == false`（门闩未装） | `AIModelNotAvailableException` |

`Param` 指出错字段（如 `"input"`）、`Code` 携带服务端错误码；映射表之外的 `Type` 一律收敛为 `AIModelException`（不臆造子类）。

#### 7.4 Tensor.Int16 扩展与语义 I/O

| 面 | 决策 |
|----|------|
| Tensor 最小扩展 | `TensorElementType.Int16` + `CreateInt16`/`ReadInt16`（P1）；覆盖 PCM int16 音频与部分量化推理输出 |
| 语义 I/O | `AIAudioInput`/`AIImageInput`/`AIVector` 定义在 `Arc.AI.Models`，由服务内部翻译为 Tensor 进 `IAIModel`；媒体预处理/后处理（WAV/PCM 解码、resize/normalize、tokenizer）是 std 库代码，不碰编译器 |
| 单一惯用法 | `IAIModel` 保持原始位置序张量契约不变；不炸开 Tensor 类型，不引入领域类型进核心（禁双轨） |

**为什么这么设计**：图像（UInt8）、token（Int64）、向量（Float32）已被现有元素类型覆盖，唯一缺的宿主面是 PCM 音频 Int16——最小增量，不为全模态炸开类型。

#### 7.5 统一模型服务门面与域子面（AIModels · Arc.AI.Models）

**设计理念（统一门面，而非每域一类）**：小模型能力对外的调用面是一个**统一模型服务门面** `AIModels`——用户 `AIModels models = new AIModels(registry);` 一次构造，随后 `models.Asr.TranscribeAsync(...)` / `models.Ocr.RecognizeAsync(...)` / `models.Embed(...)`——**域是门面上的子面，不是独立类**。门面内部按域分发到实现（经注册表取句柄 + §7.3 服务骨架），对外是**单一入口、单一装配、统一错误/预算/缓存**。

**对照两种方案**（统一门面 vs 每域一类）：

| 方案 | 形态 | 代价 |
|------|------|------|
| 每域一类（现状草案，否决） | `new AIOcrService(registry,...)` / `new AIAsrService(...)` / `new AIEmbeddingService(...)` …… 7 次构造 | 7 套构造、7 套 options、7 处装配与注册——样板高、不一致；批量/进度/缓存等横切面各自为政、无法收敛 |
| **统一门面（推荐）** | `new AIModels(registry)` 一次构造；域方法是门面方法；`models.Ocr`/`models.Asr` 是门面的域子面（强类型返回） | 单一装配点、共享注册表/预算/异常/进度/缓存；跨域横切面一处落地、行为一致 |

**为什么统一门面更优**：小模型基础设施是「被统一管理的一类资产」，不是「每个域一套自嗨封装」。生命周期/预算/错误本来就收敛在注册表与服务基座（§7.2/§7.3），调用面再散成 7 个服务类等于把收敛推回零散——样板高、装配面分裂、横切面（批量/进度/缓存/预算）无法一致。统一门面与仓库既有组合根品味同构（`AIHost` 一次 `Create` 注入全部、`HttpClient` 薄门面 + 合理默认），用户从一个入口看到全部能力。

**门面 API 草图（`Arc.AI.Models` —— 统一门面，替代 7 个领域服务类；请求/响应强类型模型 OpenAI 协议对齐，见下「请求/响应模型」）**：

```as
public class AIModels : IDisposable {
    public AIModels(AIModelRegistry registry); // 唯一构造

    // 域子面（强类型、内部实现经注册表获取句柄 + §7.3 服务骨架）
    public AIOcrFace Ocr { get; }      // RecognizeAsync(AIOcrRequest) / RecognizeBatchAsync(IReadOnlyList<AIOcrRequest>)（批量+进度+缓存键）
    public AIAsrFace Asr { get; }      // TranscribeAsync(AITranscribeRequest) / TranscribeBatchAsync(IReadOnlyList<AITranscribeRequest>) / TranscribeStreamAsync
    public AIEmbedFace Embed { get; }  // EmbedAsync(AIEmbedRequest)（Input[] 即批量）/ EmbedOneAsync
    public AIClipFace Clip { get; }    // MatchAsync(AIClipMatchRequest) / EmbedImageAsync / EmbedTextAsync
    public AIFaceFace Face { get; }    // DetectAsync(AIFaceDetectRequest) / DetectBatchAsync / VerifyAsync
    public AITtsFace Tts { get; }      // SynthesizeAsync(AITtsRequest)（非幂等默认不重试）
    public AIVisionFace Vision { get; }// UnderstandAsync(AIUnderstandRequest)

    // 统一面
    public AIModelRegistry Registry { get; }
    public AIModelBudget Budget { get; }     // 读预算/统计（041 §7.2 不变）
    public AIModelsOptions Options { get; }  // 全局默认（每域默认模型/批量大小/进度默认/缓存策略/重试）
    public void Dispose();                   // 幂等
}
```

要点：**域子面（Ocr/Asr/...）是轻量只读面**——内部经 registry 取句柄 + 服务骨架，共享门面全局 `Options`；不重复 7 套构造、不重复 7 处 options 装配。域方法是门面方法，用户最少代码即 `models.Ocr.RecognizeBatchAsync(...)`。

**域子面与主方法**：

| 域 | 子面 | 主方法 | 输出 |
|----|------|--------|------|
| ASR | `AIAsrFace` | `TranscribeAsync(AITranscribeRequest, ct)`；`TranscribeBatchAsync`（本地循环批量+进度）；流式 `TranscribeStreamAsync`（契约 §7.9） | `AITranscribeResult`（Text/Language/DurationSeconds/Segments/Words/Usage） |
| OCR | `AIOcrFace` | `RecognizeAsync(AIOcrRequest, ct)`；`RecognizeBatchAsync`（本地循环批量+进度+缓存键） | `AIOcrResult`（Text + Lines[{Text,Box,Quad,Confidence}]/Usage） |
| TTS | `AITtsFace` | `SynthesizeAsync(AITtsRequest, ct)`；流式 `SynthesizeStreamAsync`（契约 §7.9，非幂等默认不重试） | `AITtsResult`（Audio） |
| CLIP | `AIClipFace` | `MatchAsync(AIClipMatchRequest, ct)` / `EmbedImageAsync` / `EmbedTextAsync` | `AIClipMatchResult`（Candidates[{Text,Score}]）/ `AIVector` |
| 人脸 | `AIFaceFace` | `DetectAsync(AIFaceDetectRequest, ct)` / `DetectBatchAsync` / `VerifyAsync(a, b)` | `AIFaceDetectResult`（Faces[{Box,Landmarks,Confidence,Embedding?}]；身份在应用层） |
| 嵌入 | `AIEmbedFace` | `EmbedAsync(AIEmbedRequest, ct)`（Input[] 批量）/ `EmbedOneAsync` | `AIEmbedResult`（Data[{Index,Vector}]/Model/Usage） |
| 多模态理解 | `AIVisionFace` | `UnderstandAsync(AIUnderstandRequest, ct)` | `AIUnderstandResult`（Text/FinishReason/Usage） |

**统一能力面（跨域一致 · 场景驱动结论）**：10 场景（会议转写/字幕/实时/OCR 归档/嵌入/人脸/CLIP/TTS/图片问答/流水线）显示 API 重心 = **批量 + 进度 + 取消 + 结构化 + 可缓存 + 预算**。门面把横切面收敛为跨域一致契约，禁每域各搞一套：

| 面 | 契约 |
|----|------|
| **批量** | 批量 = **本地循环 + `data[index]` 对齐**（非 OpenAI `/v1/batches` 异步作业）；所有批量域方法统一签名：`IReadOnlyList<TRequest>` 入参 + `IProgress<AIModelProgress>?` 按条进度 + `ct` + 可选缓存键（`RecognizeBatchAsync`/`TranscribeBatchAsync`/`DetectBatchAsync`）；单条 `*Async`/`*OneAsync` 为批量退化情形 |
| **进度** | 统一 `AIModelProgress`（当前/总数/阶段）；**默认零开销**——不传回调即不收集，长任务（会议转写/批量 OCR）传入才启用 |
| **取消** | 全部方法 `CancellationToken`（仓库异步一体纪律）；流式调用持有 `AIModelHandle` 贯穿流生命周期 |
| **缓存** | 幂等域（OCR/嵌入/人脸/CLIP）统一暴露**缓存键钩子**（内容哈希，用户自管缓存本体）；TTS 非幂等默认关 |
| **错误** | 统一 `AIModelException` 层次（§7.3 不变）；后端错误收敛为 `AIModelError`（Message/Type/Code/Param，§7.3），不裸透原生状态 |
| **预算** | 统一在注册表（§7.2 不变）；门面只读 `Budget`（统计/记账可审计） |

**为什么这么设计**：批量/进度/缓存/预算的**横切一致性**只能由「一个门面管全部域」达成；每域一个服务类时这些契约必然漂移。门面把「一个模型一种用法」从注册表（§7.2）延伸到调用面（§7.5），无双轨。

**值类型**（`Arc.AI.Models`，强类型、禁 `object` 袋）：

```as
public class AIAudioInput {
    public List<float> Samples;    // PCM float（-1..1）
    public int SampleRate;         // 16000 / 24000 ...
    public int Channels;           // 1 / 2
    // 静态工厂：FromPcmFloat / FromPcmInt16 / FromWav(bytes) / FromFile(path) / FromBase64(bytes)
}

public class AIImageInput {
    public Tensor Data;              // UInt8 [H,W,C] 或 Float32 [1,3,H,W]（字段名 Data：规避字段名遮蔽类型名，见 §7.8）
    public int Width; public int Height; public int Channels;
    // 静态工厂：FromPixels / FromFile / FromBase64
}

public class AIVector {
    public List<float> Values;
    public int Dimension { get; }
    public static AIVector FromTensor(Tensor t);
    public float CosineSimilarity(AIVector other);
}

public class AIRect { public float X; public float Y; public float Width; public float Height; }
public class AIPoint { public float X; public float Y; }
```

**请求/响应模型（`Arc.AI.Models` · 对齐 OpenAI 生态标准）**：请求字段名即 OpenAI 参数（PascalCase 命名）；所有结果统一回显 `Model` + `AIUsage`，`AIUsage` 计数为 `int?`（模型未上报时为 `null`）。本地扩展（进度/预算/缓存/耗时）走**方法参数**与**显式扩展字段**，不冒充 OpenAI 参数进请求体（见下「设计原则」）。

**端点对齐与三档标注**（每域标注：**对齐** = OpenAI 标准端点，字段一一对应；**自定义** = 无标准端点，对齐领域惯例并显式标注；**扩展** = 本地显式扩展字段，进方法参数/显式字段、不进请求体）：

| 域 | 对齐端点 / 领域惯例 | 标注 |
|----|--------------------|------|
| ASR | `/v1/audio/transcriptions` | **对齐** |
| Embedding | `/v1/embeddings` | **对齐** |
| TTS | `/v1/audio/speech` | **对齐** |
| Vision（多模态理解） | `/v1/chat/completions`（多模态 content parts） | **对齐** |
| OCR | 无标准端点 → Tesseract 惯例 | **自定义**（领域惯例） |
| CLIP | 无标准端点 → 嵌入 + 余弦惯例 | **自定义**（领域惯例） |
| Face | 无标准端点 → Face++/InsightFace 惯例 | **自定义**（领域惯例） |

> 请求内 `Model` 字段对齐 OpenAI（显式模型标识）；本地门面下**可省略**——由组合根 `AIModelsOptions` 每域默认 `ModelId` 填充，显式设置按名覆盖（下例与 §7.8 示例均省略 `Model`）。

```as
// ASR —— 对齐 /v1/audio/transcriptions
public class AITranscribeRequest {
    public string Model;
    public AIAudioInput Input;      // 音频（FromFile/FromBase64/FromWav 值类型工厂）
    public string? Language;        // ISO-639-1（"zh"）
    public string? Prompt;          // 术语/口音引导提示
    public AITranscribeResponseFormat ResponseFormat;            // 默认 VerboseJson
    public List<AITimestampGranularity>? TimestampGranularities; // Segment / Word
    public float? Temperature;      // 0..1
}

public class AITranscribeResult {
    public string Model;            // 回显
    public string Text;
    public string? Language;
    public double? DurationSeconds;
    public List<AITranscribeSegment>? Segments;
    public List<AITranscribeWord>? Words;   // granularity 含 Word 时
    public AIUsage Usage;
}

public class AITranscribeSegment {  // verbose_json 段级（OpenAI 原值，非泛称「置信度」）
    public int Index;
    public string Text;
    public double StartSeconds; public double EndSeconds;   // ↔ OpenAI start/end
    public float AvgLogprob;        // 段级对数概率
    public float NoSpeechProb;      // 无语音概率
}

public class AITranscribeWord {     // granularity 含 Word 时的时间戳词级
    public string Text;
    public double StartSeconds; public double EndSeconds;   // ↔ OpenAI start/end
    public float AvgLogprob;        // 词级对数概率
    public float NoSpeechProb;      // 无语音概率
}

// Embedding —— 对齐 /v1/embeddings
public class AIEmbedRequest {
    public string Model;
    public List<string> Input;      // Input[]：条数由数组长度决定
    public int? Dimensions;         // 可选降维
    public AIEncodingFormat EncodingFormat;   // Float（默认）/ Base64
}

public class AIEmbedResult {
    public string Model;            // 回显
    public List<AIEmbeddingData> Data;   // data[{index, vector}]
    public AIUsage Usage;
}

public class AIEmbeddingData {
    public int Index;               // 对应 Input 位置
    public AIVector Vector;         // EncodingFormat=Base64 时解码后仍为 Vector
}

// TTS —— 对齐 /v1/audio/speech
public class AITtsRequest {
    public string Model;
    public string Input;            // 合成文本
    public string Voice;            // 声线（模型相关）
    public AITtsResponseFormat ResponseFormat;   // 默认 Mp3
    public float Speed;             // 0.25..4.0，默认 1.0
    public string? Instructions;    // 语音风格指令
}

public class AITtsResult {
    public string Model;            // 回显
    public AIAudioInput Audio;      // 合成音频（PCM float + SampleRate）
    public AITtsResponseFormat ResponseFormat;  // 回显
    public AIUsage Usage;           // TTS 无 token 计数 → 各计数为 null
}

// 多模态理解 —— 对齐 /v1/chat/completions（content parts）
public class AIUnderstandRequest {
    public string Model;
    public string? SystemPrompt;
    public List<AIUnderstandPart> Input;      // content parts（文本/图像）
    public AIResponseFormat? ResponseFormat;  // JsonObject / JsonSchema
    public int? MaxTokens;
}

public class AIUnderstandResult {
    public string Model;            // 回显
    public string Text;
    public string FinishReason;     // stop / length / content_filter
    public AIUsage Usage;
}

public abstract class AIUnderstandPart { }   // content part 基类（禁 object 袋）
public class AIUnderstandTextPart : AIUnderstandPart { public string Text; }
public class AIUnderstandImagePart : AIUnderstandPart { public AIImageInput Image; }  // FromFile/FromBase64

// OCR —— 自定义域：无 OpenAI 端点，对齐 Tesseract 惯例
public class AIOcrRequest {
    public string Model;
    public AIImageInput Input;      // 图像（FromFile/FromBase64）
    public string? Language;        // Tesseract lang（"chi_sim+eng"）
}

public class AIOcrResult {
    public string Model;            // 回显
    public string Text;             // 拼接全文
    public List<AIOcrLine> Lines;
    public AIUsage Usage;
}

public class AIOcrLine {
    public string Text;
    public AIRect Box;              // 行包围盒（x/y/width/height）
    public List<AIPoint> Quad;      // 4 角点（旋转文本）
    public float Confidence;        // 引擎置信度刻度（0..1）
}

// CLIP —— 自定义域：无 OpenAI 端点，对齐嵌入+相似度惯例
public class AIClipMatchRequest {
    public string Model;
    public AIImageInput Image;      // 查询图像
    public List<string> Candidates; // 候选文本（零样本分类）
}

public class AIClipMatchResult {
    public string Model;            // 回显
    public List<AIClipCandidate> Candidates;
    public AIUsage Usage;
}

public class AIClipCandidate {
    public string Text;
    public float Score;             // 余弦相似度/概率
}

// 人脸 —— 自定义域：无 OpenAI 端点，对齐检测惯例；身份在应用层，不进模型结果
public class AIFaceDetectRequest {
    public string Model;
    public AIImageInput Input;      // 图像（FromFile/FromBase64）
}

public class AIFaceDetectResult {
    public string Model;            // 回显
    public List<AIFaceDetection> Faces;
    public AIUsage Usage;
}

public class AIFaceDetection {
    public AIRect Box;              // 人脸包围盒
    public List<AIPoint> Landmarks; // 关键点（眼睛/鼻/嘴）
    public float Confidence;        // 检测置信度
    public AIVector? Embedding;     // 可选：识别用嵌入（身份判定在应用层 VerifyAsync）
}

// 公共 Usage —— OpenAI usage 对齐 + 本地显式扩展
public class AIUsage {
    public int? PromptTokens;
    public int? CompletionTokens;
    public int? TotalTokens;
    public long? DurationMs;        // 本地扩展：本次调用耗时
    public long? PeakMemoryBytes;   // 本地扩展：峰值内存
}
```

**枚举（PascalCase ↔ OpenAI 原值，禁自创拼写）**：

| 枚举 | 成员（OpenAI 原值） |
|------|---------------------|
| `AITranscribeResponseFormat` | `Json`("json") / `Text`("text") / `Srt`("srt") / `VerboseJson`("verbose_json") / `Vtt`("vtt") |
| `AITimestampGranularity` | `Segment`("segment") / `Word`("word") |
| `AITtsResponseFormat` | `Mp3`("mp3") / `Opus`("opus") / `Aac`("aac") / `Flac`("flac") / `Wav`("wav") / `Pcm`("pcm") |
| `AIEncodingFormat` | `Float`("float") / `Base64`("base64") |
| `AIResponseFormat` | `JsonObject`("json_object") / `JsonSchema`("json_schema") |

**执行参数（不进请求体）**：方法签名 `(request, CancellationToken)` + 可选 `IProgress<AIModelProgress>`（当前/总数/阶段）+ 可选预算/缓存选项（`AIModelCacheOptions` 等）；本地路径经 `FromFile`/`FromBase64`/`FromWav` 值类型工厂，媒体解码在服务内部。

**设计原则（OpenAI 对齐 · 关键裁决）**：

| 裁决 | 内容 |
|------|------|
| **批量 ≠ `/v1/batches` 作业** | 本地批量 = 方法内循环 + `data[index]` 对齐（如 `AIEmbedResult.Data[i].Index`）+ `IProgress<AIModelProgress>` 按条进度；不是提交异步批次作业，无批次 ID/状态轮询 |
| **`response_format` 不跨域统一枚举** | 每域独立枚举（`AITranscribeResponseFormat`/`AITtsResponseFormat`/`AIResponseFormat`），域间不互用、不抽象公共基类 |
| **本地扩展不冒充 OpenAI 参数** | 进度/预算/缓存/耗时走方法参数与显式扩展字段（`AIUsage.DurationMs`/`PeakMemoryBytes`），不塞进请求体当 OpenAI 参数 |
| **自定义域显式标注** | OCR / 人脸 / CLIP 无 OpenAI 端点 → 对齐领域惯例（Tesseract `Confidence`、嵌入+相似度 `Score`），类型注释显式标注 |
| **身份在应用层** | 人脸结果只含检测/嵌入（`Faces[{Box,Landmarks,Confidence,Embedding?}]`），身份判定由应用层 `VerifyAsync` 完成，不进模型结果 |

**编码规范附件 · 命名映射（Arc PascalCase ↔ OpenAI 原值）**：

| Arc 字段 | OpenAI 原值 |
|---------|-------------|
| `Model` | `model` |
| `Input` | `input`（ASR `file`/`url`，Vision `messages[].content`） |
| `Language` / `Prompt` / `Temperature` | `language` / `prompt` / `temperature` |
| `ResponseFormat` / `TimestampGranularities` | `response_format` / `timestamp_granularities` |
| `Text` / `DurationSeconds` | `text` / `duration` |
| `Segments` / `Words` | `segments` / `words` |
| `StartSeconds` / `EndSeconds` | `start` / `end` |
| `AvgLogprob` / `NoSpeechProb` | `avg_logprob` / `no_speech_prob` |
| `Dimensions` / `EncodingFormat` | `dimensions` / `encoding_format` |
| `Data` / `Index` / `Vector` | `data` / `index` / `embedding` |
| `Voice` / `Speed` / `Instructions` | `voice` / `speed` / `instructions` |
| `SystemPrompt` / `MaxTokens` / `FinishReason` | `messages[role=system].content` / `max_tokens` / `finish_reason` |
| `Usage` / `PromptTokens` / `CompletionTokens` / `TotalTokens` | `usage` / `prompt_tokens` / `completion_tokens` / `total_tokens` |
| `Message` / `Type` / `Code` / `Param` | `message` / `type` / `code` / `param` |

> 命名即契约：实现必须按上表映射（含段/词时间戳字段 `StartSeconds`/`EndSeconds` ↔ OpenAI `start`/`end`）；禁自创拼写（如 `responseformat` 拼错为 `response_format`、`max_tokens` 错位）。

**量化与内存预算（16G 普通电脑）**：注册表 `AIModelQuantization`（Int4/Int8 优先）+ `SizeBytes` 计账 + LRU 驱逐；子面不感知（预算在注册表）。**批量** = 本地循环（`models.Embed.EmbedAsync(AIEmbedRequest)`、`models.Asr.TranscribeBatchAsync(List<AITranscribeRequest>)`、`models.Ocr.RecognizeBatchAsync(List<AIOcrRequest>)`）；**流式** = `IAsyncEnumerable`（[008](008-delegates-closures.md)）消费，流式调用持有 `AIModelHandle` 贯穿流生命周期。

#### 7.6 分层依赖与包归属

| 命名空间 | 目录 | 内容 | 依赖 |
|----------|------|------|------|
| `Arc.AI` | `std/AI/Core/` | `Tensor`(+Int16)、`IAIModel`、`AIModelRegistry`/`Registration`/`Handle`/`Backend`、`AIModelService` 基座、`AIModelBudget`/`Policy`、异常层次 | `Arc` |
| `Arc.AI.Onnx` | `std/AI/Onnx/` | `OnnxModelFactory` + `OnnxAIModelFactory` | `Arc.AI` |
| `Arc.AI.Iree` | `std/AI/Iree/` | `IreeModelFactory` + `IreeAIModelFactory` | `Arc.AI` |
| `Arc.AI.Models` | `std/AI/Models/`（子目录 Asr/Ocr/Tts/Clip/Face/Embedding/Vision） | 统一门面 `AIModels` + 域子面（`AIOcrFace`/`AIAsrFace`/...）+ 请求/响应强类型模型（OpenAI 对齐）+ 值类型 | `Arc.AI`（后端经工厂注入） |
| `Arc.Agent.Models` | `std/AI/Agent.Models/` | 多模态结果部件 `AIAudioPart`/`AIVectorPart`（如需要）+ 可选提示 `AIModelContextProvider`；**不提供 `AIModelTool` / 领域模型工具**（工具封装是用户代码，038 §14.3） | `Arc.Agent` + `Arc.AI.Models` |

依赖单向：`Arc.Agent.Models` → `Arc.Agent` + `Arc.AI.Models`；`Arc.AI.Models` → `Arc.AI`；`Arc.Agent` → `Arc.AI`（可选）；`Arc.AI.Onnx`/`Arc.AI.Iree` → `Arc.AI`。

- **服务属推理、工具封装属用户**：统一门面 `AIModels`（纯推理语义）归 `Arc.AI.Models`，**不依赖 Agent**（保持「推理体系完全独立、Agent 可消费推理」单向）；Agent 会话内工具封装是用户代码（普通 `[AITool]`，038 §14.3），框架不提供模型工具封装；多模态结果部件（如需要）归 `Arc.Agent.Models`。
- **`.ani` 新原生 shim 同构**：新原生依赖（如音频解码、tokenizer）按 §3 同构——`crates/runtime-xxx/` C shim + `xxx.ani`（`load="auto"` + `library = Environment.GetEnvironmentVariable("ARC_XXX_LIB")`）+ `Native.IsAvailable` 门闩降级链 + `fetch/build-xxx-native.ps1`（产物落 `target/xxx-native/`）+ `VENDOR.md` 登记。编译器核心 7 crate 零改动。
- **命名**：非 Provider 公开类型一律 `AI` 前缀（`AIModelRegistry`/`AIModels`/`AIOcrFace`/`AIAsrFace`）；命名空间 ≤3 段（`Arc.AI.Models` / `Arc.Agent.Models` 为最深段，域不另开命名空间）。

#### 7.7 开箱即用（简洁优雅调用）

**原则**：**统一门面 `AIModels` 是唯一入口**——一次 `new AIModels(registry)` 构造、域方法是门面方法（§7.5），Agent 会话内集成是可选第二路径（用户用普通 `[AITool]` 自封装，038 §14.3）；**框架不提供 `AIModelTool` 工具封装**——只提供注册表 + 门面，工具封装是用户代码（克制，不过度）。

**最小示例 ① 纯门面调用（OCR 批量 · 无 Agent）**：

```as
using Arc.AI;
using Arc.AI.Models;

AIModelRegistry registry = new AIModelRegistry(AIModelRegistryOptions.Default);
registry.Register(new AIModelRegistration {
    ModelId = "ocr/paddleocr", Kind = AIModelKind.Ocr,
    Factory = reg => OnnxModelFactory.Create("models/paddleocr.onnx"),
});
using AIModels models = new AIModels(registry);          // 统一门面一次装配
List<AIOcrRequest> requests = pages.Select(p => new AIOcrRequest {
    Input = AIImageInput.FromFile(p),
}).ToList();
List<AIOcrResult> results = await models.Ocr.RecognizeBatchAsync(requests, null, ct);   // 批量+进度+缓存键
Console.WriteLine(results.Select(r => r.Text).Join("\n"));
```

**最小示例 ② 纯门面调用（嵌入批量 · 向量写 Wiki）**：

```as
using AIModels models = new AIModels(registry);
AIEmbedResult embed = await models.Embed.EmbedAsync(
    new AIEmbedRequest { Input = doc.Chunks }, ct);   // Data[i].Index 对齐 Input 位置
wiki.Upsert($"doc/{doc.Id}", DocChunksToSummary(doc));   // 向量本体用户侧落库，不直送 LLM
```

**最小示例 ③ Agent 会话内集成（用户 `[AITool]` 自封装）**：用户把门面子面包成普通 `[AITool]`，编译器合成 `AIToolSet`，会话内 LLM 自动调用（038 §14.3）：

```as
using Arc.Agent;
using Arc.AI.Models;

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

using AIModels models = new AIModels(registry);           // 组合根装配（7.5）
AIHost host = AIHost.Create(provider, new OcrTool(models), AISessionOptions.Default);
using AISession session = host.CreateSession();
AIReply reply = await session.RunAsync("识别这张图并转写音频", ct);   // LLM 经 ocr.recognize 调门面子面
```

**最小示例 ④ 多模态图片问答（结果进上下文）**：用户 `[AITool]` 内门面子面调用产出的文本走既有工具结果回路进 transcript；需要结构化注入时经 `AIContextEngine.AddProvider` 自注 `AIModelContextProvider`（可选提示，038 §14.4）。

#### 7.8 能力分层 P1–P4

| 层级 | 范围 |
|------|------|
| **P1 统一运行时 + 注册表** | `Tensor.Int16`；`AIModelRegistry`/`Registration`/`Handle`/`Backend`；`AIModelBudget`/`Policy`/`AIModelService` 基座；异常层次；`OnnxAIModelFactory`/`IreeAIModelFactory` 适配 |
| **P2 领域子面封装（先 OCR/嵌入/ASR）** | `Arc.AI.Models` 包；统一门面 `AIModels` + `AIOcrFace`/`AIEmbedFace`/`AIAsrFace` 子面 + 请求/响应强类型模型（OpenAI 对齐）+ 值类型 + 批量；Clip/Face/Tts/Vision 四域子面为诚实缺口——尚不属于本面（未注入真实后端前抛 `AIModelNotAvailableException`） |
| **P3 Agent 会话内集成 + 多模态上下文** | 用户 `[AITool]` 自封装（统一门面直调，038 §14.3）；`AIAudioPart`/`AIVectorPart`；`AIModelContextProvider`（可选提示）；`AISessionOptions.ModelBudget` 护栏；能力未授权 → `CapabilityDenied`（不调 handler） |
| **P4 生态** | TTS / CLIP / 人脸 / 多模态理解；批量与流式（`TranscribeStreamAsync`/`SynthesizeStreamAsync`，流式契约见 §7.9）；量化档位；`.ani` 新原生 shim（按需）；结果缓存 |

> **字段名与可空值契约**：① **`AIImageInput` 字段名 `Data`（类型 `Arc.AI.Tensor`）**——Arc 字段名遮蔽类型名（静态工厂内 `Tensor.CreateByte` 会被解析为实例字段），表单定为 `public Tensor Data;`；② **Arc 无可空值类型（int?/float?/double?）**——请求/响应模型的可选值字段以哨兵承载（`AIUsage.*` 计数与 `DurationMs`/`PeakMemoryBytes` 为 `-1`，`Temperature`/`DurationSeconds` 为 `<0`，`Dimensions` 为 `<=0`，`TimestampGranularities` 等引用字段用 `?`）；③ **最小张量契约**（tokenizer/段级时间戳为后续项）：OCR/ASR 输出文本以 `UInt8 [N]` UTF-8 字节承载，嵌入输入以 `Int64 [N, maxLen]` UTF-8 字节直通编码承载、输出 `Float32 [N, dim]` 逐行切 `AIVector`。

> **诚实标注**：`AIOcrFace` / `AIAsrFace` / `AIEmbedFace` 只提供「管道 + 调用面」，**不含真实识别/转写/嵌入能力**——真实识别需用户自备 ONNX 模型 + 前后处理（tokenizer / CTC / 段级解码），框架当前**不内置任何预训练模型与 tokenizer/解码器**；未注入真实后端即「不可识别」。不得把「仅测试 fixture 能跑通管道」当作「已具备识别能力」对外宣称。

#### 7.9 流式契约（TTS/ASR 实时场景）

**定位：语义层流式编排，不虚构底层流式协议。** ONNX Runtime `Run` 与 IREE invoke 的执行面本质是批式（一次输入完整张量集、一次产出全部输出），当前两后端均无原生流式执行 API。因此流式能力落在**域子面层分块编排**：长输入切块 → 逐块批推理 → 增量投递——首块延迟 = 首块推理耗时，而非全文耗时。`IAIModel` 契约不动（§7.2 `RunAsync` 唯一执行入口）：为其添加流式执行面在当前后端现实下只能是「永远抛 NotAvailable 的假面 API」，拒绝。

**消费模式：sink 回调（推理侧现状，对齐为后续独立工作流）。** `IAsyncEnumerable` 由 RFC 008 定义，Agent 侧流式主惯用法为 `IAsyncEnumerable<AIStreamEvent>`（038 §4）；推理侧（TTS/ASR）sink 契约与 RFC 008 的对齐为后续独立工作流，此前维持域特定 sink 接口 + `Task` 完成信号（`Task` 完成 ⇔ `OnCompleted`/`OnError` 已投递），无双轨。

**API 草图**（`Arc.AI.Models`）：

```as
// TTS 流式消费：音频块增量投递
public interface IAITtsStreamConsumer {
    void OnAudioChunk(AITtsChunk chunk);    // 增量音频块（Index 0 起递增）
    void OnCompleted(AITtsResult result);   // 完成汇总（Usage.DurationMs 累计）
    void OnError(AIModelException error);   // 中途失败（已产出块不撤回）
}

// ASR 流式消费：窗口段完成即投递
public interface IAIAsrStreamConsumer {
    void OnSegment(AITranscribeSegment segment);  // 段完成投递（窗口边界）
    void OnCompleted(AITranscribeResult result);  // 汇总（Text = 段拼接）
    void OnError(AIModelException error);
}

// 域子面流式方法（§7.5 门面域子面追加；返回 Task = 完成信号，非结果本体）
public class AITtsFace {
    public Task SynthesizeStreamAsync(AITtsRequest request, IAITtsStreamConsumer consumer, CancellationToken ct);
}
public class AIAsrFace {
    public Task TranscribeStreamAsync(AITranscribeRequest request, IAIAsrStreamConsumer consumer, CancellationToken ct);
}
```

块模型 `AITtsChunk`（进 `Models/Tts/AITtsModels.as`）：`Samples`（本块 PCM float 采样，`List<float>`；对齐 §7.5 `AITtsResult.Audio` 语义值类型与 P2 张量契约——TTS 模型输出 `Float32 [M]` PCM 块，容器编码属应用层）/ `Index`（块序号，0 起递增）/ `IsFinal`（末块标记）/ `Text`（本块对应切句文本，供字幕对齐与调试）。

**分块策略（通用预处理，非模型前后处理）**：

| 域 | 策略 | 请求级参数（哨兵 = 默认） |
|----|------|--------------------------|
| TTS | 文本切句：标点边界（。！？；.!?; 与换行）+ 句长上限兜底（防无标点长文一块到底） | `AITtsRequest.MaxChunkChars`（`<=0` → 120） |
| ASR | 定长窗口分段：按采样点切顺序不重叠窗口 | `AITranscribeRequest.WindowSeconds`（`<=0` → 30.0，对齐主流 ASR 模型窗口惯例） |

切句/窗口是通用文本/音频预处理，**不是 tokenizer**（不冒充模型前后处理）；重叠对齐/VAD/说话人分离属模型知识，不在本面，按需另立设计。

**横切语义（复用 §7.3 骨架，流式专用执行路径）**：

- **句柄**：`Acquire` 一次贯穿流生命周期（§7.5 既有裁决），逐块 `RunAsync`，`finally Release`——不逐块 Acquire/Release
- **取消**：`ct` 贯穿、块间检查；已产出块不撤回（诚实边界，半成品音频/文本由消费侧自行处置）
- **重试**：TTS 非幂等（§7.3）→ 流式不自动重试；ASR 幂等 → 块级重试按 `Options.MaxRetries`（出块前完成，消费侧无感）
- **超时**：块级复用 `TimeoutMs`（单块异常覆盖）；不设全程总超时——全程时长治理属会话预算（038 §14），门面不重复建设
- **统计**：逐块 `RecordRun` 记实际执行模型（逐请求覆盖 §7.5 同语义）；`Usage.DurationMs` 为各块累计，token 计数维持 `-1` 哨兵（本地模型无 token 计量，不冒充）

**诚实边界**：

1. **编排层流式 ≠ 模型流式推理**：窗口内仍批式执行；词级 partial / 超低延迟增量需后端原生流式 shim（流式 ASR 模型 / 流式执行面），届时另行 RFC 增补 `IAIModel` 流式契约，当前不虚构
2. **同步回调即背压**：消费方 sink 阻塞 = 编排暂停（可预测的天然背压，极简哲学）；框架不建异步缓冲队列
3. **ASR 流式为段级增量**（窗口段完成即投递），满足字幕/转写实时场景；「边说边出字」的词级实时属第 1 条，不在本契约范围

**拒绝项**：

| 方案 | 拒绝理由 |
|------|----------|
| `IAsyncEnumerable<T>` 形态 | Agent 侧（038 §4）已采用 `IAsyncEnumerable<AIStreamEvent>`；推理侧维持域 sink 契约（§7.9），与 RFC 008 的对齐为后续独立工作流，避免双轨 |
| `IAIModel.RunStreamAsync` | ONNX/IREE 无原生流式执行面，落地即永抛 NotAvailable 的假面 API |
| 统一泛型 sink（`IModelStreamConsumer<T>`） | 音频块与段文本语义异构，合并即过度抽象；域特定 sink 沿用推理侧命名惯例（接口一律 `I` + `AI` 前缀） |
| 全程总超时参数 | 块级 `TimeoutMs` 已覆盖单块异常；全程时长治理属会话预算（038 §14），不重复建设 |

**验收场景**：「长文本 → `SynthesizeStreamAsync` 首块延迟 < 全文合成耗时 + 块序递增 + `OnCompleted` Usage 累计 + 中途取消后续块停止投递；长音频 → `TranscribeStreamAsync` 段增量顺序投递 + 汇总 `Text` = 段拼接；中途后端失败 → `OnError` 携带统一异常层次（§7.3）且已产出块不撤回」——假 Factory 注入即可验收编排契约（真实模型能力仍受 §7.8 诚实标注约束）。

## 边界

- **Agent 宿主**（会话/工具/HITL/Wiki/CodeAct/MCP）见 [038](038-ai-host.md)；本 RFC 只讲推理引擎。
- **小模型能力 Agent 侧**（统一门面 `AIModels` 直调 / 用户 `[AITool]` 自封装 / 多模态结果进上下文 / 回合预算护栏）见 [038 §14](038-ai-host.md)；本 RFC 只讲推理侧统一运行时与统一门面。
- **统一门面是唯一入口**：小模型能力的对外调用面只有一个 `AIModels`（域是门面上的子面，非独立类）；不另起每域门面（如 `AIAsr`/`AIOcr` 独立服务类或独立门面均拒绝）——装配/预算/错误/进度/缓存全部收敛在门面与注册表，无双轨。
- **`.ani` 原生加载 / FFI 契约**见 [016](016-verified-ffi.md)。
- **编译器核心 7 crate 零领域逻辑**为架构红线，见标准库架构。
- **`Arc.Net.P2P` 点对点网络**见 [042](042-p2p.md)（独立领域，互不依赖）。

---
上一节：[040 Web 框架与 SSR](040-web.md) · 下一节：[042 P2P 网络](042-p2p.md)
