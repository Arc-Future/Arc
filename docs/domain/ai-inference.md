# Arc.AI

## 概述

`Arc.AI` 是 Arc 的 AI 推理引擎域：宿主张量 `Tensor` + 统一执行接口 `IAIModel` 构成共享抽象核心（`std/AI/Core/`），其上提供两个**互补、互不替代**的推理后端——`Arc.AI.Onnx`（ONNX Runtime，解释型 + EP 可插拔）与 `Arc.AI.Iree`（IREE，AOT 编译型 + 多后端 GPU）。

两后端经 `.ani` `load="auto"` 运行时加载外部 DLL，编译器核心零领域逻辑。业务侧统一经共享抽象层 `Arc.AI` 的 `IAIModel` 消费，不感知后端差异。

### 命名空间与后端定位

| 命名空间 | 目录 | 定位 |
|----------|------|------|
| `Arc.AI` | `std/AI/Core/` | 宿主张量 `Tensor` + `IAIModel` 抽象（共享核心） |
| `Arc.AI.Onnx` | `std/AI/Onnx/` | ONNX Runtime 推理（`OnnxModelFactory`/`SessionOptions`） |
| `Arc.AI.Iree` | `std/AI/Iree/` | IREE 推理（`IreeModelFactory`，`.vmfb` 执行） |

| 维度 | `Arc.AI.Onnx` | `Arc.AI.Iree` |
|------|---------------|---------------|
| 执行模型 | 即时解释 + EP 插件 | 提前编译（AOT）+ 多后端 |
| 编译时机 | 无（运行期加载图） | `iree-compile` 产出 `.vmfb` |
| GPU | 依赖 EP（如 DirectML） | 一等公民（Vulkan/CUDA 等） |
| 部署形态 | 解释器 + 模型文件 | 编译产物 `.vmfb`（零编译开销） |
| 适用 | 灵活、快速上手、EP 生态 | 高性能、异构、边缘/GPU |

**单一惯用法**：`Arc.AI.Onnx` 只服务 ONNX 图，`Arc.AI.Iree` 只服务 `.vmfb` 执行——各读各自域，禁双轨 API。业务侧若需「同一模型两种后端」，经共享抽象 `IAIModel` 选择适配器。

## 快速开始

### 1. 经共享抽象推理（以 ONNX 为例）

业务侧只接触 `IAIModel` 与 `Tensor`：

```as
using Arc.AI;
using Arc.AI.Onnx;
using Arc.Collections;

// 门闩：库可用时灰化可选功能；不可用时走降级路径
if (!OnnxModelFactory.IsAvailable) {
    Console.WriteLine("ONNX 未安装，功能灰化");
    return;
}

using IAIModel runner = OnnxModelFactory.Create("model.onnx");

// 按位置构造输入张量（形状 [1,3,224,224] 行主序）
List<long> shape = new List<long>();
shape.Add(1); shape.Add(3); shape.Add(224); shape.Add(224);
List<float> data = new List<float>();
for (int i = 0; i < 3 * 224 * 224; i++) { data.Add(0.0f); }
Tensor input = Tensor.CreateFloat(shape, data);

List<Tensor> inputs = new List<Tensor>();
inputs.Add(input);
List<Tensor> outputs = await runner.RunAsync(inputs, ct);

List<float> scores = outputs[0].ReadFloat();   // 读回宿主缓冲
```

### 2. ONNX 专用配置

需要配置执行提供程序、线程数与图优化时使用 `SessionOptions`：

```as
using Arc.AI.Onnx;

SessionOptions options = new SessionOptions();
options.SetGraphOptimizationLevel(GraphOptimizationLevel.Extended);
options.SetIntraOpNumThreads(4);
options.UseExecutionProvider(ExecutionProvider.Cpu);   // 或 ExecutionProvider.DirectML（Windows GPU）

using IAIModel runner = OnnxModelFactory.Create("model.onnx", options);
```

### 3. IREE 开箱即用闭环

IREE 需要先编译模型为 `.vmfb`，再加载执行：

```
iree-import-onnx model.onnx → model.mlir
iree-compile --iree-hal-target-backends=llvm-cpu -o model.vmfb model.mlir
```

```as
using Arc.AI;
using Arc.AI.Iree;

if (!IreeModelFactory.IsAvailable) {
    Console.WriteLine("IREE 未安装，功能灰化");
    return;
}
IreeModelFactory.RequireAvailable();   // 受保护启用守卫，不可用抛 IreeNotAvailableException

using IAIModel runner = IreeModelFactory.Create("model.vmfb", "main");

// 输入/输出均为宿主张量，经 IAIModel 统一消费
List<Tensor> outputs = await runner.RunAsync(inputs, ct);
```

运行期只加载 `.vmfb`，不依赖编译器；编译段仅在需要产出 `.vmfb` 时使用。

### 降级链

未装库时：门闩 `IsAvailable == false` → 门面抛 `OnnxNotAvailableException`/`IreeNotAvailableException`（显式失败面，绝不崩溃）。开发者可用门闩做可选功能灰化，或用 `RequireAvailable()` 做受保护启用。

## 核心 API

### Tensor —— 宿主张量

| 成员 | 说明 |
|------|------|
| `CreateFloat(shape, data)` / `CreateDouble` / `CreateInt32` / `CreateInt64` / `CreateByte` | 静态工厂创建，一对一映射宿主类型化缓冲 |
| `ReadFloat()` / `ReadDouble` / `ReadInt32` / `ReadInt64` / `ReadByte` | 类型化读取（行主序） |
| `Shape` | 各维度尺寸（未知维为 -1） |
| `ElementType` | 元素数据类型（`TensorElementType`） |
| `Rank` | 阶数（维度数） |
| `Total` | 元素总数（shape 乘积） |

### IAIModel —— 统一执行接口

| 成员 | 说明 |
|------|------|
| `RunAsync(List<Tensor> inputs, ct)` | 唯一执行入口，返回位置序输出张量列表 |
| `InputCount` / `OutputCount` | 模型输入/输出张量数量 |
| `GetInputName(i)` / `GetOutputName(i)` | 张量名（无名字后端返回空串） |
| `GetInputElementType(i)` / `GetOutputElementType(i)` | 元素类型（无元数据返回 `Undefined`） |
| `GetInputShape(i)` / `GetOutputShape(i)` | 形状（未知维为 -1；无形状返回空表） |

`IAIModel` 继承 `IDisposable`。输入按位置提供（数量须等于 `InputCount`），按名字映射到后端命名输入。

### ONNX 后端（Arc.AI.Onnx）

| 类型 | 说明 |
|------|------|
| `OnnxModelFactory` | 唯一公开入口；`IsAvailable` 门闩 + `Create(modelPath[, options])` → `IAIModel` |
| `SessionOptions` | `SetIntraOpNumThreads`/`SetInterOpNumThreads`/`SetGraphOptimizationLevel`/`UseExecutionProvider` |
| `ExecutionProvider` | `Cpu`（基线）/ `DirectML`（Windows GPU 后端） |
| `GraphOptimizationLevel` | 图优化级别（推理推荐 `Extended`） |

> `InferenceSession`/`OnnxTensor` 等句柄细节为内部实现，业务侧只经 `OnnxModelFactory` 获得 `IAIModel` 面。

### IREE 后端（Arc.AI.Iree）

| 类型 | 说明 |
|------|------|
| `IreeModelFactory` | 唯一公开入口；`IsAvailable` 门闩 + `RequireAvailable()` + `Create(modulePath, functionName)` → `IAIModel` |

> `IreeSession`/`IreeBufferView` 等句柄细节为内部实现，业务侧只经工厂消费。

## 边界

- **Agent 宿主**（会话/工具/HITL/Wiki/CodeAct/MCP）见 [ai-host.md](ai-host.md)；本册只讲推理引擎。
- **`.ani` 原生加载 / FFI 契约**见规范章的验证式 FFI。
- **编译器核心 7 crate 零领域逻辑**为架构红线；推理引擎经 `.ani` 运行时加载外部 DLL。

---

上一节：[ai-host.md](ai-host.md) · 下一节：[orm.md](orm.md)