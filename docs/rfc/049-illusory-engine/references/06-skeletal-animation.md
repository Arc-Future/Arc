# 06 动作表现：2D 骨骼小人动画

> 所属：[RFC 049 Illusory 游戏引擎(../../049-illusory-engine.md)。本文是文档链第六环，定义动作表现的关键能力：**人物骨架 + 贴图部件**构建动态人物。采用 Spine/DragonBones 型 **2D 骨骼小人**——骨骼旋转与位移驱动贴图部件拼装，**无网格蒙皮、无顶点权重**。仍在对象模型与确定性核心之上，构成确定性驱动下的求值管线。**内容面向，非 M1 必经；进入实现以子库 RFC 细化排期**。
>
> 前置依赖：[03 对象模型](03-object-model.md) · [04 仿真核心](04-simulation-core.md) · [05 行为模型](05-behavior-model.md)。
> 阅读顺序：本文 → [07 VR 与网络方向预留](07-vr-networking-directions.md)。

## 1. 选型与目标

- **选型**：2D 骨骼小人（Spine/DragonBones）。理由——资产轻（无 Mesh 绑定/蒙皮权重）、导入易、与「人物骨架+贴图」表述完全一致，最快可用；确定性不因去蒙皮而受损。
- **核心机制**：骨架是**变换层级**（骨骼=相对父级的局部 T/R/S），贴图部件（Part）挂到某根骨骼上；当骨随动画旋转/位移，挂其上的部件随之变换，拼装出动态人物。
- **目标**：数据、控制、求值、渲染四层解耦；资产跨模型复用；**求值确定**（由 `SimulationTick` 驱动）；渲染托底 `Arc.UI` wgpu 面，不新造后端。

## 2. 四层流水线

| 层 | 职责 | 支撑 |
|----|------|------|
| 数据 | 骨架/部件/Clip 资产，描述"长什么样" | `RigAsset`、`SlotAsset`、`AnimationClip` |
| 控制 | 选择当前播哪个 Clip、混入权重 | `AnimatorComponent`（状态机或行为驱动目标姿势） |
| 求值 | Clip→Pose，骨架空间分层合成 | `RigEvaluator` |
| 渲染 | Pose→部件放置→绘制 | `SpritePartRenderer` → `Arc.UI`/wgpu 面 |

## 3. 数据资产——描述层

| 资产 | 内容 | 关键约定 |
|------|------|---------|
| `RigAsset` | 骨骼关节名/层级、父骨、每骨**本地绑定变换** | 只存**本地**相对父级的 T/R/S，全局姿势运行时层级合成 |
| `SlotAsset` | 部件挂到哪根骨、部件中心偏移/镜像翻转 | 一个部件对应一个或多个三角形/矩形贴图；无顶点权重 |
| `AnimationClip` | 系列关键帧，每帧含各骨**局部 T/R/S** | **只存局部姿态**，可求解、可插值、可导出 |

设计决策：

- **Clip 只存局部姿态**，求值时做骨架空间层级合成——同 Rig 换 Clip 不改绑定姿势，资产跨模型复用。
- **去网格蒙皮**：2D 骨骼小人无顶点加权；部件是贴图四边形，只随骨变换，不做顶点蒙皮。骨架层级深、希望平滑形变的极少数情形（如脸颊）留作后续方向，不纳入本面。
- 资产一律**数据驱动**（[02 §10](02-api-conventions.md#10-注释与可空) + [01 §5](01-charter-capability.md#5-架构纪律)），禁止硬编码需持续维护的动画数据。

## 4. 组件挂接——控制层

以既有 Actor+Component 模型承载（[03 §4](03-object-model.md#4-icomponent挂点协议)）：

| 组件 | 职责 | 驱动 |
|------|------|------|
| `SkeletonComponent` | 持有当前 Rig/Slot 引用、每骨**本地姿势槽**与动画时钟 | 数据引用 |
| `AnimatorComponent` | 状态机/行为目标姿势选择、混合权重、`Play`/`CrossFade` | 行为/[05](05-behavior-model.md) |
| `SpritePartRenderer` | 持有绘制所需的部件贴图集、把骨姿势落成带排序的绘制指令 | 渲染 |

- `SkeletonComponent` 每步由求值器写骨姿势槽，是唯一"合法"的骨架状态权威（确定性由它保证）。
- `AnimatorComponent` 由 `BehaviorRunner` 驱动（行为调 `PlayAsync` 切换状态），把"想播什么"交出去，不直接写骨。
- **不能直接改 Actor 骨状态**：骨姿势只经 `SkeletonComponent` 走求值器，禁止其它路径任意写，否则破坏确定性。

## 5. 骨架求值管线（求值层）

物化一个 `Clip → Pose → 部件放置` 的标准求值管线（对齐 Spine/DragonBones 求解）：

1. **取样本**：按动画时钟在 `AnimationClip` 关键帧间插值，得每骨的目标局部 T/R/S。
2. **空间合成**：沿骨架层级把局部姿势合成到模型空间——每骨的**全局变换** = 父骨全局 × 本骨局部（缓存每个父链）。
3. **部件放置**：对每个 `SlotAsset`，按所挂骨的全局变换 + 部件中心偏移放置其贴图四边形；按深度（骨层级/构建顺序）组织绘制顺序。

```as
public class RigEvaluator {
    public void Evaluate(AnimationClip clip, float animationTime, SkeletonComponent skeleton);
    public void PlaceParts(SkeletonComponent skeleton, List<DrawCommand> drawOut);
}
```

- **求值由步印驱动**：`SkeletonComponent` 在 `IRunnable.Update` 相位按当前 `SimulationTick.Time` 求值，保证与行为/物理同源、可复现。
- **插值是 SimTick 域内**：动画时钟换算基于 `SimulationTick`，不触墙钟。

## 6. 渲染——表现层（托底，不自建后端）

引擎**不新造渲染后端**（红线，见 [01 §5](01-charter-capability.md#5-架构纪律)）。绘制：

- `SpritePartRenderer` 把 `RigEvaluator` 产出的部件指令组装成 `Arc.UI`/wgpu `DrawList` 提交（RFC 037）。
- **2.5D 关键：深度排序与视觉半立**——用**画家算法**把同层部件按深度/向后 Y 排序，另支持显式 `renderOrder` 让角色前后半身/穿模可控；相机本体锁自由视角或固定朝向即得 2.5D 画面（镜头矩阵交渲染层，不见仿真核心）。
- 骨姿势矩阵每步由求值器写入，混合权重在求值期落到相同时钟，视觉与仿真同帧对齐。

## 7. 与行为/信号协作

- `AnimatorComponent` 状态切换由行为触发：`await ctx` 内 `AnimatorComponent.PlayAsync(clip, ct)`。
- 打击/受击等协作走 [05 §6 Signal](05-behavior-model.md#6-signal跨对象通信)：攻击到达广播 `HitSignal`（含被击者 `ActorId`），被击者行为切换受击 Clip 并在求值层混入。

## 8. 实现排期（进入实现时以子库 RFC 细化）

一经立项 `std/Illusory/Animation → Arc.Illusory.Animation`，按递增可用性排：

| 阶段 | 交付 | 验收 |
|------|------|------|
| A0 | `RigAsset`/`SlotAsset`/`AnimationClip` 数据模型 + `RigEvaluator` 求值（纯数据，无可视） | 同 Clip 换 Rig 姿势正确；同步印求值两次输出一致 |
| A1 | `SpritePartRenderer` 把部件经 `Arc.UI`/wgpu 绘制；画家排序 + `renderOrder` | 部件随骨变换正确、深度序稳定 |
| A2 | `AnimatorComponent` 播放控制 + 状态/混合 + 行为驱动 | `PlayAsync`/`CrossFade` 生效，`BehaviorRunner+Signal` 切换正确 |

## 9. 门禁（验收判据）

| 判据 | 断言 |
|------|------|
| 局部→全局合成 | 同 Rig 换 Clip 不改绑定姿势，视觉正确 |
| 确定性 | 同一步印输入求值两次输出一致（骨姿势/部件放置逐位一致） |
| 组件权威 | 骨姿势只经 `SkeletonComponent` 求值写入，无他径 |
| 托底渲染 | 部件经 `Arc.UI`/wgpu 面绘制（画家序在线），无自建后端 |
| 数据驱动 | 换 Clip/部件经数据或 `PlayAsync` 参数，不经改代码 |

> **不在此篇**：对象壳与组件仓库见 [03](03-object-model.md)；固定步进见 [04](04-simulation-core.md)；行为驱动与 Signal 见 [05](05-behavior-model.md)；VR 高频姿态衔接见 [07](07-vr-networking-directions.md)。