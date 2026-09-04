# 02 命名与接口规范

> 所属：[RFC 049 Illusory 游戏引擎(../../049-illusory-engine.md)。本文是文档链第二环，**先立写法规范**，03–06 所有领域接口与命名强制遵守。规范本身不引入新行为，只为后续所有代码提供唯一惯用法判据。
>
> 前置依赖：[01 章程与能力全景](01-charter-capability.md)。
> 阅读顺序：本文 → [03 对象模型](03-object-model.md)。

## 1. 总则

引擎全部 `.as` 文件对标 C# 优雅简洁写法，遵循 Arc 编码规范（RFC 003 + `arc-language.mdc`）。以下为引擎特化条款，与标准规范冲突时以本文为准（冲突概率极低）。

## 2. 命名通则

| 实体                       | 约定                      | 例                                                                  |
| ------------------------ | ----------------------- | ------------------------------------------------------------------ |
| 类型 / 方法 / 属性 / 常量 / 枚举成员 | **PascalCase**          | `ActorRegistry`、`SpawnActor`、`CurrentTick`、`FixedStepMilliseconds` |
| 参数 / 局部变量                | **camelCase**           | `frameDeltaMilliseconds`、`initial`、`actorId`                       |
| 私有/保护字段                  | **`_camelCase`**（前导下划线） | `_actors`、`_nextId`、`_fixedStepMilliseconds`                       |
| 接口                       | **`I`** **前缀**          | `IWorld`、`IRunnable`、`IComponent`                                  |
| 文件名                      | 与主类型同名                  | `IWorld.as`、`SimulationTick.as`                                    |

禁止项：匈牙利命名、C 风格常量（`ACTOR_MAX`）、无意义缩写（`reg`、`comp` → 用 `registry`、`component`）。

## 3. `this.` 成员前缀

- **公开成员**（字段/属性/方法，含虚方法、静态方法调用同对象实例）访问带 `this.`：

  ```as
  internal ActorRegistry() {
      this._actors = new Dictionary<ActorId, Actor>();   // 公开成员
  }
  ```

- **内部字段** **`_field`** 裸访问（无 `this.`），仅与参数/局部变量冲突时用 `this.` 消歧。

> 当前引擎实现中 `readonly struct` 的构造函数对 `_value` 等内部字段写值时可用裸访问，外部读值用 getter-only 属性。

## 4. 属性约定

| 场景            | 写法                                       | 例                                  |
| ------------- | ---------------------------------------- | ---------------------------------- |
| 构造期/初值即定 → 只读 | `public Type Name { get; }`              | `ActorId.Id`、`SimulationTick.Step` |
| 类内可变（逻辑更新）    | `public Type Name { get; private set; }` | `Actor.Transform`                  |
| 外部可变          | `public Type Name { get; set; }`         | 尽量克制；引擎核心多用前两种                     |
| POCO 初始化      | `public Type Name { get; init; }`        | 数据模板/配置类                           |

**等价默认值的冗余初值一律省略**：`int x = 0`、`bool flag = false`、`T ref = null` 不写，因为已是类型默认值。

## 5. 可失败方法：`Try* + out`

引擎所有**可能失败/可能缺失**的操作一律 `Try* + out`，不返回 null 也不抛异常作为常态：

```as
bool TryDestroyActor(ActorId id);
bool TryGetActor(ActorId id, out Actor actor);
bool TryGetComponent<T>(ActorId actorId, out T component) where T : IComponent;
```

契约：

- `out` 参数在前置 `if` 分支返回 false 时必须被赋有效值（`null` 或 `default`），不能悬空。

- 内部实现使用 `Dictionary.TryGetValue` 等标准库 `Try*` 时直接透传 `out` 参数，不额外包装。

## 6. 异步方法

异步方法名必须 `Async` 后缀，必须接受 `CancellationToken`：

```as
async Task MyBehaviorAsync(BehaviorContext ctx, CancellationToken ct);
```

引擎内行为层（[05](05-behavior-model.md)）通过 `await ctx.WaitTick()` 挂起，底层由 `BehaviorRunner` 绑定 `SimulationTick` 步进。异步 I/O 禁止做同步副本；引擎仿真（`IWorld.Update`）本身为同步无 I/O，不做 `Async` 版本。

## 7. 可见性：内部编排纪律

| 可见性        | 承载内容           | 例                                                     |
| ---------- | -------------- | ----------------------------------------------------- |
| `public`   | 门面接口、开发者可见数据结构 | `IWorld`、`Actor`、`ActorId`、`SimulationTick`           |
| `internal` | 编排实现、调度器、注册表   | `World`、`Simulation`、`ActorRegistry`、`BehaviorRunner` |

开发者只与 `public` 交互；引擎内部用 `internal` 隐藏所有状态机与调度细节。此纪律与 RFC 020 std 架构的 internal 边界一致。

## 8. 无事件机制

Arc 不引入事件机制（`event`/`EventHandler`/`+=`/`-=`）。引擎跨对象通信：

- **Signal 模式**：对象广播轻量信号，接收方以委托订阅（[05](05-behavior-model.md)）。

- **委托组合**：`List<SomeDelegate>` 或反应式管线。

- **接口回调**：系统间直接调用接口方法。

不建事件中心/总线，不做反射式订阅。

## 9. 控制流大括号

`if`/`else`/`switch`/`while`/`for`/`foreach` 一律 `{}` 括起，禁止省略；`switch` 的每个 `case`/`default` 分支体也必须 `{}` 括起。Allman 风格（左花括号独立成行）：

```as
if (fixedStepMilliseconds <= 0.0f)
{
    throw new ArgumentException("FixedStepMilliseconds must be positive.");
}
```

## 10. 注释与可空

- 公开 API 用 `///` 文档注释；注释独立成行。

- 面向开发者写「为什么」，不复述代码。

- 可空引用显式 `?` 标注：`string? tag`；空判必须妥善处理。

> **不在此篇**：对象模型细节见 [03](03-object-model.md)；仿真核心见 [04](04-simulation-core.md)；行为模型见 [05](05-behavior-model.md)。

