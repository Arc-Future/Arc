# 04 仿真核心 World+SimulationTick

> 所属：[RFC 049 Illusory 游戏引擎(../../049-illusory-engine.md)。本文是文档链第四环，定义确定性仿真核心：IWorld 门面、WorldOptions、固定步长切分、SimulationTick 步印、IRunnable 三相与确定性契约。**承载 M1**。
>
> 前置依赖：[03 对象模型 Actor+Component](03-object-model.md)。
> 阅读顺序：本文 → [05 行为模型](05-behavior-model.md)。

## 1. 设计目标

把「时间切片」与「对象生命周期」解耦成两层，统一由 `IWorld` 门面对外：

- **时间切片层**（本文）：固定步长把墙钟帧耗时切分为等长 `SimulationTick`，保证一切时间决策可预测、可回放。

- **对象/驱动层**（[03](03-object-model.md) + [05](05-behavior-model.md)）：Actor 生命周期与行为此刻不直接决定时间，而是被步进驱动。

## 2. SimulationTick——唯一时间源

不可变步印，一切确定性计算的唯一时间源与回放/预测锚点。

```as
public readonly struct SimulationTick {
    private readonly int _step;
    private readonly float _time;
    private readonly float _deltaTime;

    public int Step { get { return _step; } }       // 单调递增步号（从 1 起）
    public float Time { get { return _time; } }     // 累计仿真时间（秒）
    public float DeltaTime { get { return _deltaTime; } }  // 恒定固定步长（秒）

    internal SimulationTick(int step, float time, float deltaTime) { ... }
}
```

契约（**确定性三不变**）：

1. **Step 单调递增**：从 1 起，只增不减，每次推进 +1。回放/网络快照以 Step 为锚。
2. **DeltaTime 恒定**：一帧即便多步，每步间隔恒为 `FixedStepMilliseconds`，不随帧率漂移。
3. **Time 依积导**：`Time == Step * DeltaTime`，无墙钟参与。

第 4 位（源码之外）的确定性格言：**物理、行为、网络快照一律引用步印，不读墙钟**。步印仅由内部 `Simulation` 生成（构造器 `internal`），对外无可变入口。

## 3. IRunnable——三相驱动钩子

系统在固定步长内分相驱动的契约。

```as
public interface IRunnable {
    void Begin(SimulationTick tick);
    void Update(SimulationTick tick);
    void End(SimulationTick tick);
}
```

契约：

- **三相固定次序**：一次推进内 `Begin → Update → End`，不可重排（诊断/快照对齐依赖此序）。

- `tick` 贯穿三相，供系统据 `Step`/`Time` 做确定性决策。

- M1 中 `ActorRegistry` 实现 `IRunnable`（三相空转），为 M3/M4 行为层与系统调度器预留驱动点。

## 4. WorldOptions——世界组态

构造 `IWorld` 时注入的参数集；构造器校验后固定不可变。

```as
public class WorldOptions {
    public float FixedStepMilliseconds { get; }             // 固定步长（毫秒），必须为正
    public IReadOnlyList<object> Services { get; }          // 注入的引擎服务列表

    public WorldOptions()                                       // 默认 60Hz：1000/60 ≈ 16.667ms
    public WorldOptions(float fixedStepMilliseconds, IReadOnlyList<object> services)  // 校验步长为正
}
```

契约：

- **步长为正是硬校验**：`fixedStepMilliseconds <= 0` 抛 `ArgumentException`。

- **默认 60Hz**：`FixedStepMilliseconds = 16.666666f`，恒定。

- **服务列表**：承载 DI 注入的引擎服务（`IInputService`/`IPhysicsWorld` 等），M2/M6 起对接既有服务容器（RFC 023）。传入时以 `IReadOnlyList<object>` 承载。

## 5. Simulation——固定步长编排（internal）

时间切片状态机，不对外暴露，仅经 `IWorld` 间接驱动。

```as
internal class Simulation {
    private readonly float _fixedStepMilliseconds;
    private float _accumulator;
    private int _step;

    internal void Update(float frameDeltaMilliseconds, IRunnable runner) {
        _accumulator += frameDeltaMilliseconds;
        while (_accumulator >= _fixedStepMilliseconds)
        {
            int nextStep = _step + 1;
            Advance(nextStep, runner);
            _accumulator -= _fixedStepMilliseconds;
            _step = nextStep;
        }
    }
}
```

**固定步长 while 累加**：

- 一帧可消耗多个步印（减速时补足欠步），余量累计到下帧，保证 `DeltaTime` 恒定不漂移。

- 每次 `Advance` 构造步印、写 `_currentTick`、按相序调用 `runner.Begin/Update/End`。

- **无 I/O、同步**：`Update` 是纯状态推进，异步一体原则下不做 Async 版本。

```as
internal SimulationTick CurrentTick { get; }    // 最近已推进步印，对外读只读仿真进度
```

## 6. IWorld——门面

引擎对外唯一入口：推进世界、生成/销毁 Actor、挂接组件。

```as
public interface IWorld {
    SimulationTick CurrentTick { get; }

    void Update(float frameDeltaMilliseconds);

    Actor SpawnActor(Transform initial);
    Actor SpawnActor(Transform initial, GameplayTags tags);
    bool TryDestroyActor(ActorId id);
    bool TryGetActor(ActorId id, out Actor actor);

    void AddComponent(ActorId actorId, IComponent component);
    bool RemoveComponent(ActorId actorId, IComponent component);
    bool TryGetComponent<T>(ActorId actorId, out T component) where T : IComponent;
}
```

契约：

- **`Update(float frameDeltaMilliseconds)`**：推进一帧，内部按固定步长切分到 `SimulationTick` 并驱动注册系统。参数为帧耗时（毫秒）；同步无 I/O。

- **`CurrentTick`**：最近已推进步印，供渲染插值/HUD/计时读取当前仿真进度（只读）。

- **生命周期**：`SpawnActor` 构造必成功；`TryDestroyActor`/`TryGetActor` 缺失返回 false（`Try* + out`，见 [02 §5](02-api-conventions.md#5-可失败方法try-out)）。

- **组件**：`AddComponent`（Actor 不存在或无组件能力时静默忽略）、`RemoveComponent`、`TryGetComponent<T>`。

创建入口：

```as
public static class Worlds {
    public static IWorld Create(WorldOptions options) { return new World(options); }
}
```

## 7. 实现组织

```
std/Illusory/Core/
├── Simulation/
│   ├── SimulationTick.as         // readonly struct
│   ├── IRunnable.as              // 三相接口
│   └── Simulation.as             // internal 编排
├── World/
│   ├── WorldOptions.as
│   ├── IWorld.as
│   └── Impl/
│       ├── World.as              // IWorld 实现：组合 Simulation + ActorRegistry（internal）
│       └── Worlds.as             // 创建门面（public static）
```

内部 `World` 把时间切片与对象生命周期组合成一个外观：`Update` 把帧耗时分给 `Simulation`，`Simulation` 驱动 `ActorRegistry`（作为 `IRunnable`）三相。

## 8. M1 门禁（验收判据）

| 判据       | 断言                                                                    |
| -------- | --------------------------------------------------------------------- |
| 固定步长切分   | 一帧消耗余量累计，多帧跨步边界正确                                                     |
| Step 单调性 | 推进后 `CurrentTick.Step` 为上一帧步伐 +1                                      |
| 三相顺序     | 单步内收到 `Begin → Update → End` 各一次且有序                                   |
| 生命周期     | `SpawnActor` 后 `TryGetActor` 命中；`TryDestroyActor` 后 `TryGetActor` 未命中 |
| 惯性/无 I/O | `Update` 不触墙钟、不产生 I/O，纯状态推进                                           |

> **验收状态（2026-09-04）**：上表五判据由 `arc-tests/tests/l3_illusory_batch.rs` 全量覆盖并**全绿**
> （sim\_fixed\_step / sim\_monotonic / actor\_lifecycle / tags\_immutable / component\_store）。
> 首批点亮过程中暴露并修复的编译器/运行时协议缺陷（标记接口 itable、Copy struct 字段悬垂、
> 重载接口方法槽序、实参接口包裹、foreach 元素类型、struct 静态字段默认值、泛型型参 `is` 折叠、
> 泛型转调单态化传播，及接口元素 List 对象身份比较）逐项入账
> [stability-2026-09-02 复盘(../../../reviews/stability-2026-09-02.md)。

> **不在此篇**：对象壳细节见 [03](03-object-model.md)；行为驱动（async + BehaviorRunner）见 [05](05-behavior-model.md)。

