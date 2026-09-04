# 05 行为模型 async+BehaviorRunner

> 所属：[RFC 049 Illusory 游戏引擎(../../049-illusory-engine.md)。本文是文档链第五环，定义行为层：async 行为如何用 `await ctx.WaitTick()` 表达流程，`BehaviorRunner` 如何把 async 推进绑定到确定性步印。**承载 M3（BehaviorRunner + BehaviorContext + Signal）与 M4（能力协议 + 数据驱动）**。
>
> 前置依赖：[03 对象模型](03-object-model.md) · [04 仿真核心](04-simulation-core.md)。
> 阅读顺序：本文 → [06 动作表现](06-skeletal-animation.md)。

## 1. 设计动机

行为是「可动元素」的灵魂：人物的移动、怪物的巡逻、门的开关、拾取物的交互。业界三条路径（Unity 协程、Godot await、GAS 生命周期）各占一侧，本引擎取其融合：

- **易上手**：以 `async` 方法书写的流程既直观又可读。

- **可组合**：行为可拆分、可派生、可取消。

- **确定性**：async 的挂起/恢复绑定到 `SimulationTick`，从而可记录、可回放、可预测。

**零新机制**：异步基础直接复用 Arc 既有的 `Task`/`CancellationToken`/`Async` 契约（RFC 009），`BehaviorRunner` 只是把"何时恢复"绑定到步印而非帧率/墙钟。

## 2. 术语

| 术语                | 含义                                                                   |
| ----------------- | -------------------------------------------------------------------- |
| `BehaviorContext` | 一次行为执行的上下文，封装当前 Actor、当前 `SimulationTick` 与取消令牌                      |
| `WaitTick()`      | 把执行挂起到下一个固定步印的等待原语                                                   |
| `BehaviorRunner`  | 内部调度器，按固定步进驱动已挂起的所有行为恢复                                              |
| `Signal`          | 轻量广播信标，行为间/对象间通信（Arc 无事件机制，见 [02 §8](02-api-conventions.md#8-无事件机制)） |
| 能力协议              | 行为同时占有 Actor 时的优先/互斥/打断规则（M4）                                        |

## 3. BehaviorContext

行为执行上下文，随当前步印注入。

```as
public class BehaviorContext {
    public Actor Actor { get; }            // 挂行为的主体
    public SimulationTick Tick { get; }    // 当前已推进步印
    public CancellationToken CancellationToken { get; }   // 随行为生命周期

    public Task WaitTick();
    public Task WaitSeconds(float seconds);          // 按固定步印换算为整数步，保证确定性
    public void Cancel();                            // 取消当前行为
}
```

契约：

- **只能经注入获得**，开发者不可自行构造（避免脱离调度拉起行为）。

- **时间语义**：`WaitTick()` 挂起到下一固定步印；`WaitSeconds` 一律换算为**整数个固定步**（`ceil(seconds / DeltaTime)`），保证确定性，不触墙钟。

- **取消**：`WaitTick`/`WaitSeconds` 均接受/响应上下文取消令牌；行为被取消时以协作式抛出而终止，不崩溃。

## 4. 行为书写（开发者视角）

行为即一个返回 `Task` 的 `async` 方法，方法名以 `Async` 后缀并接受上下文与取消令牌：

```as
public async Task PatrolAsync(BehaviorContext ctx, CancellationToken ct) {
    while (true) {
        MoveTo(ctx.Actor, waypoints[0]);
        await ctx.WaitSeconds(2.0f);
        MoveTo(ctx.Actor, waypoints[1]);
        await ctx.WaitSeconds(2.0f);
    }
}
```

要点（遵循 [02 §6](02-api-conventions.md#6-异步方法)）：

- 方法名 `Async` 后缀。

- 行为内部所有"等待"都用 `ctx.WaitTick/WaitSeconds`，不直接读墙钟。

- **单一惯用法**：该引擎只有 async behavior 一种行为写法，不并存裸协程 API。

## 5. BehaviorRunner——步进驱动（internal）

内部调度器，把已挂起的多个行为与固定步进绑在一起。

```as
internal class BehaviorRunner : IRunnable {
    internal void Attach(ActorId actor, Func<BehaviorContext, CancellationToken, Task> behavior);
    internal void Detach(ActorId actor);
    internal void Cancel(ActorId actor);

    public void Begin(SimulationTick tick);   // 步启动
    public void Update(SimulationTick tick);  // 恢复等待满一个固定步的行为
    public void End(SimulationTick tick);     // 记账/快照
}
```

契约：

- 作为 `IRunnable` 接入 `World` 三相（[04 §3](04-simulation-core.md#3-irunnable三相驱动钩子)），缺失这一步，行为就没有推进时钟。

- `Update` 相位统一恢复无阻塞、步数已满足的行为；一次步内可推进多个行为。

- 上下文以当前 `tick` 重建注入，保证行为内的 `Tick` 与仿真同步。

- internal：开发者经 `IWorld` 的能力协议（M4）挂接行为，不直接触调度器。

## 6. Signal——跨对象通信

Arc 无事件机制（[02 §8](02-api-conventions.md#8-无事件机制)），本引擎以 `Signal` 模式替代 `event`：

```as
public class Signal<T> {
    private readonly List<Action<T>> _handlers;
    public void Subscribe(Action<T> handler);
    public void Unsubscribe(Action<T> handler);
    public void Broadcast(T value);
}
```

- 对象广播轻量信标，接收方以委托订阅。不建事件中心/总线，不做反射式订阅。

- 行为间协作（如"攻击到达 → 广播 HitSignal"）经此流转；结合结构见 [06 §7](06-skeletal-animation.md)。

## 7. 数据驱动模板（M4）

能力模板一律数据驱动，禁止把需持续维护的数据硬编码：

- 行为参数（移动速度、等待秒数、动画 Clip 引用）从数据模板装载，模板在 M4 定格式。

- 模板与预制体格式登记在 [01 §7 能力缺口](01-charter-capability.md#7-能力缺口登记)，M4 再固化，不先建格式。

## 8. M3/M4 门禁（验收判据）

| 里程碑       | 判据                                       |
| --------- | ---------------------------------------- |
| M3 挂起/恢复  | `await ctx.WaitTick()` 恰在下一固定步恢复一次，非帧率驱动 |
| M3 取消     | `Cancel()`/令牌取消后行为协作式终止，不残留未被驱动的手续       |
| M3 Signal | 广播后订阅方恰收到一次，退订后不再收到                      |
| M4 能力协议   | 优先/互斥/打断规则在同时占有 Actor 时按契约裁决，胜者行为不被打断    |
| M4 数据驱动   | 替换模板参数改变行为，不经改代码                         |

> **不在此篇**：对象壳与 Actor 生命周期见 [03](03-object-model.md)；固定步进编排见 [04](04-simulation-core.md)；动作表现为 [06](06-skeletal-animation.md)。

