# RFC 049：Illusory 游戏引擎——Actor+Component · 确定性仿真核心

状态：草案（2026-09-02）
关联：RFC 037（UI/渲染）· RFC 009（异步）· RFC 020（std 架构与 internal 边界）· RFC 023（数学/DI）· RFC 006（对象模型）· RFC 003/002（语法与命名）
落点：`std/Illusory/`（引擎族，namespace `Arc.Illusory`）· 首子库 `std/Illusory/Core/`

本 RFC 是引擎家族**唯一章程**，只做三件事：定身份、裁边界、指路。深度契约按主题下沉至 `references/`，**阅读顺序即编号顺序**（01→07），每篇依赖前篇。

## 0. 文档地图（唯一入口 · 顺序即阅读顺序）

| 序 | 主题 | 精要 | 承载 |
|----|------|------|------|
| 01 | [章程与能力全景](049-illusory-engine/references/01-charter-capability.md) | 动机、对象/行为双重选型裁决、能力域分层、复用×自建、能力缺口登记、架构纪律 | 引擎"为什么+边界" |
| 02 | [命名与接口规范](049-illusory-engine/references/02-api-conventions.md) | 命名通则、Try*+out、Async+Token、属性/可见性、Arc 最佳实践落地 | 先立规范，03–06 强制遵守 |
| 03 | [对象模型 Actor+Component](049-illusory-engine/references/03-object-model.md) | Actor 壳、ActorId、GameplayTags、IComponent、组件仓库、实现组织 | M1 壳 / M2 仓库 |
| 04 | [仿真核心 World+SimulationTick](049-illusory-engine/references/04-simulation-core.md) | IWorld 门面、WorldOptions、固定步长、SimulationTick 步印、IRunnable、确定性 | M1 |
| 05 | [行为模型 async+BehaviorRunner](049-illusory-engine/references/05-behavior-model.md) | BehaviorContext、async 行为、WaitTick、CancellationToken、Signal、调度解耦 | M3 / M4 调度 |
| 06 | [动作表现：2D 骨骼小人](049-illusory-engine/references/06-skeletal-animation.md) | 数据资产（骨架/部件/Clip）、Clip→Pose→部件放置管线、确定性、2.5D 深度排序、组件挂接 | 内容面向 |
| 07 | [VR 与网络方向预留](049-illusory-engine/references/07-vr-networking-directions.md) | 高频姿态、输入语义化、网络预测/回滚 | M5+ 方向登记 |

职责裁界：01 定"为什么与能做什么"，02 立跨域强制规范，03–06 依依赖次序定各自域的信号，07 只做方向登记、不预建实现。**编号即阅读顺序，也是合规实现顺序**：先知边界、再立写法、后构领域，每篇只依赖前篇编号。每篇末尾"不在此篇"指向相邻编号，链路单向（01→02→…→07），无网状回环。

## 1. 身份

`std` 需要一套**虚拟现实优先**的游戏引擎，组织并驱动人物/怪物/门/拾取物等「可动元素」。引擎负责**对象模型、世界组织、行为驱动与确定性步进**；渲染托底复用既有 `Arc.UI`/wgpu 面（RFC 037），不新造渲染后端。对象模型与行为模型的两个选型裁决（业界最好用/最易上手）见 [references 01](049-illusory-engine/references/01-charter-capability.md#2-对象模型与行为模型选型)。

## 2. 目录与命名空间映射

```
std/Illusory/
├── arc.toml                  (namespace Arc.Illusory —— 同 std/UI/Core → Arc.UI 约定)
└── Core/                     // 首子库
    ├── arc.toml
    ├── World/IWorld.as                // 门面接口
    ├── World/WorldOptions.as
    ├── World/Actor.as / ActorId.as
    ├── World/GameplayTags.as
    ├── World/IComponent.as
    ├── World/Impl/                    // internal 编排（Worlds 创建门面 / World 实现 / ActorRegistry）
    ├── Simulation/Simulation.as       # internal（固定步长编排）
    ├── Simulation/SimulationTick.as
    └── Simulation/IRunnable.as
```

后续子库平级扩展（各自 `arc.toml` 与命名空间）：`std/Illusory/Behaviors → Arc.Illusory.Behaviors`（05）、`std/Illusory/Animation → Arc.Illusory.Animation`（06）、`std/Illusory/UI → Arc.Illusory.UI`（**与 `Arc.UI` 区分**）。

## 3. 里程碑分期

| 里程碑 | 交付 | 契约细案 |
|--------|------|---------|
| M1 | World/Simulation/SimulationTick/Actor 骨干，固定步长 Update，SpawnActor/TryDestroyActor，IRunnable 三相 | [03](049-illusory-engine/references/03-object-model.md) · [04](049-illusory-engine/references/04-simulation-core.md) |
| M2 | Component 系统（仓库/按类型索引/增删经 World） | [03](049-illusory-engine/references/03-object-model.md) |
| M3 | 行为层 BehaviorRunner + BehaviorContext（WaitTick）+ async 行为 + Signal | [05](049-illusory-engine/references/05-behavior-model.md) |
| M4 | 能力协议（优先/互斥/打断）+ 数据驱动模板 | [01](049-illusory-engine/references/01-charter-capability.md#7-能力缺口登记) · [05](049-illusory-engine/references/05-behavior-model.md) |
| M5 | VR 姿态/输入语义化 + 可重放验证 + 性能轮 | [07](049-illusory-engine/references/07-vr-networking-directions.md) |
| M6+ | 物理/导航注入（DI）+ 网络快照/预测 | [01](049-illusory-engine/references/01-charter-capability.md#7-能力缺口登记) · [07](049-illusory-engine/references/07-vr-networking-directions.md) |

## 4. 不做清单（红线）

- **不引 ECS / 不引事件系统**——对象模型固定 Actor+Component；通信用 `Signal`/委托。
- **不新造渲染后端**——表现全托底 `Arc.UI` wgpu 面。
- **不内置物理求解器 / 不内置网络**——领域能力经 DI（RFC 023）注入。
- **不硬编码游戏数据**——预制体/动画 Clip/能力模板一律数据驱动。
- **不为未来 VR 预建表现-仿真耦合钩子**——M1–M3 不写后置债务。
- **不增加补丁式文档**——能力新增先入 [07 方向](049-illusory-engine/references/07-vr-networking-directions.md) / [01 能力缺口](049-illusory-engine/references/01-charter-capability.md#7-能力缺口登记) 登记，再固化细案；不留过时/重复文档。

## 5. 基础面事实（决策依据，细节见外部 RFC）

- 异步/取消齐备（RFC 009）：`Task`/`CancellationToken`/`Async` 后缀一等待遇，行为层零新机制。
- 数学/向量直射（RFC 023）：`Math`→LLVM intrinsic；`Arc.DI` 编译期工厂、依赖注入零反射。
- 目录-命名空间映射先例（RFC 020）：`std/UI/Core → Arc.UI`；本引擎沿用 `std/Illusory/Core → Arc.Illusory`。
- 渲染托底（RFC 037）：复用 `Arc.UI`/wgpu `WgpuRender`/`DrawList`。
- `out`/`ref` 与内部编排纪律（RFC 020）：`Try*+out` 契约成立；编排类一律 internal。

---
[返回 RFC 索引](index.md)