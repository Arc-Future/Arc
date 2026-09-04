# 03 对象模型 Actor+Component

> 所属：[RFC 049 Illusory 游戏引擎(../../049-illusory-engine.md)。本文是文档链第三环，定义对象模型的确切形态：Actor 壳、ActorId、GameplayTags、IComponent、组件仓库与实现组织。**承载 M1 壳 / M2 仓库**。
>
> 前置依赖：[01 章程与能力全景](01-charter-capability.md) · [02 命名与接口规范](02-api-conventions.md)。
> 阅读顺序：本文 → [04 仿真核心](04-simulation-core.md)。

## 1. 句法摘要

对象模型分四个值面与一套仓库：

| 面    | 类型                        | 语义                                 | 里程碑          |
| ---- | ------------------------- | ---------------------------------- | ------------ |
| 身份   | `ActorId`                 | 全局唯一、单调递增，值语义                      | M1           |
| 标签   | `GameplayTags`            | 不可变标签包，值语义                         | M1           |
| 挂点协议 | `IComponent`              | 数据/能力挂点标记                          | M1 壳 / M2 仓库 |
| 壳    | `Actor`                   | 纯数据壳（Id + Transform + Tags + 组件仓库） | M1           |
| 仓库   | `ActorRegistry`（internal） | Actor/组件生命周期与检索唯一管辖                | M1 壳 / M2 仓库 |

## 2. ActorId——身份锚点

值语义的身份类型，确定性/回放/网络引用的锚点。

```as
public readonly struct ActorId : IEquatable<ActorId>, IHashable<ActorId> {
    private readonly int _value;

    public static readonly ActorId None;

    public int Value { get { return _value; } }

    internal ActorId(int value) { _value = value; }

    public static bool Equals(ActorId a, ActorId b) { return a._value == b._value; }

    public static int GetHashCode(ActorId value) {
        return HashCode.HashValue(value._value);
    }
}
```

契约：

- **值语义**，可实现 `IEquatable`/`IHashable`，可作字典键。

- **单调递增**：底层序号从 1 起，由 World 内部 `ActorRegistry` 分配回收。开发者不可自行构造（构造器 `internal`）。

- **`None`**（== `default`）表示无效/未分配哨兵。

- **`int Value`** 底层序号，getter-only；序号类型用 `int`（Arc 当前 `uint` 不支持混合算术）。

- 实例分配与回收只经 World 注册表，绕过生命周期约束的构造一律禁止。

## 3. GameplayTags——标签包

不可变、值语义的标签集合，作为能力/组件查询与行为互斥的判据。

```as
public readonly struct GameplayTags {
    private readonly HashSet<string> _tags;

    internal GameplayTags(HashSet<string> tags) { _tags = tags; }

    public bool IsEmpty { get { return _tags == null || _tags.Count == 0; } }
    public bool Has(string tag)                      // 是否含标签
    public GameplayTags Add(string tag)              // 返回新实例，本实例不变
    public GameplayTags Remove(string tag)           // 返回新实例，本实例不变
    public bool Overlaps(GameplayTags other)         // 是否有交集
}
```

契约：

- **不可变快照**：`Add`/`Remove` 返回新实例而非原地修改，避免共享集合别名/竞态。

- **仅作元数据判据**：对齐 Unreal GameplayTags，不装载行为逻辑——标签只回答问题"是不是/has/overlaps"，不决定"怎么做"。

- 支持 `IsEmpty`、`Has(string)`、`Add`/`Remove`、`Overlaps(GameplayTags)`。

- 暂以 `HashSet<string>` 承载（M1）；后续如需层级标签，改内部实现不动外部契约。

## 4. IComponent——挂点协议

M1/M2 为**纯数据标桩**，不预先耦合驱动逻辑。

```as
public interface IComponent {
}
```

契约：

- 唯一作用：标记"这是一个可挂到 Actor 上的组件"。

- **不预先耦合驱动**：M1/M2 不定义 `Begin/Update` 之类驱动接口。驱动论证与接口在 [05 行为模型](05-behavior-model.md)（M3）引入。

- 具体组件以用户类/引擎内置类实现此接口，如 `HealthComponent`、`SkeletonComponent`、`AnimatorComponent`（见 [06](06-skeletal-animation.md)）。

## 5. Actor——纯数据壳

差异化靠挂组件而非派生子类；壳本身不装行为。

```as
public class Actor {
    public ActorId Id { get; }
    public Transform Transform { get; private set; }
    public GameplayTags Tags { get; private set; }

    internal Actor(ActorId id, Transform transform, GameplayTags tags) {
        this.Id = id;
        this.Transform = transform;
        this.Tags = tags;
    }
}
```

契约：

- **只读身份**：`Id` getter-only，创建即定。

- **空间与标签由 World 内部维护**：`Transform`/`Tags` 用 `{ get; private set; }`，对开发者只读。组件如要改 Actor 空间，经引擎编排接口由 World 统一落地（未来扩展），避免壳数据被随意改写破坏确定性。

- **实例仅经** **`World.SpawnActor`** **创建**（构造器 `internal`），禁止任意 `new Actor(...)`。

## 6. 组件仓库——挂载与检索

组件仓库设在 `ActorRegistry`（internal），M1 以每 Actor 单列表承载，`TryGetComponent<T>` 线性扫描：

```as
internal class ActorRegistry : IRunnable {
    private readonly Dictionary<ActorId, Actor> _actors;
    private readonly Dictionary<ActorId, List<IComponent>> _components;

    internal void AddComponent(ActorId actorId, IComponent component);
    internal bool RemoveComponent(ActorId actorId, IComponent component);
    internal bool TryGetComponent<T>(ActorId actorId, out T component) where T : IComponent;
}
```

契约：

- **增删经 World 门面**：开发者不直接触碰注册表（internal），统一经 `IWorld.AddComponent/RemoveComponent/TryGetComponent<T>`。

- **按类型检索**：`TryGetComponent<T>` 返回该 Actor 上第一个 `T` 实例；缺失返回 false（`out` 赋 `null`）。

- **M1 线性扫描**：`List<IComponent>` 顺序检索，正确性优先。

- **M2 升级按类型索引**：为高频查询建 `Type → 组件槽` 索引，等线性扫描成为实测瓶颈再切，避免过早优化；同时启用 `T` 泛型化分派。

- 组件以引用语义挂载（`List<IComponent>`），同一组件可挂一个 Actor 一份。

## 7. 实现组织

```
std/Illusory/Core/
├── World/
│   ├── IWorld.as                 // 门面接口
│   ├── WorldOptions.as           // 世界组态（驱动细节见 04）
│   ├── Actor.as
│   ├── ActorId.as
│   ├── GameplayTags.as
│   ├── IComponent.as
│   └── Impl/
│       ├── Worlds.as             // 创建门面（public static）
│       ├── World.as              // IWorld 实现（internal）
│       └── ActorRegistry.as      // 仓库（internal）
```

目录-命名空间映射沿用 `std/Illusory/Core → Arc.Illusory`。`World/` 放公开 API，`World/Impl/` 放 internal 编排；`Impl/` 内文件可声明 `namespace Arc.Illusory`（命名空间与目录解耦，仅作物理职责组织）。

## 8. 生命周期入口

| 操作       | 入口                                             | 语义           |
| -------- | ---------------------------------------------- | ------------ |
| 创建       | `Worlds.Create(WorldOptions)`                  | 返回 `IWorld`  |
| 生成 Actor | `IWorld.SpawnActor(Transform[, GameplayTags])` | 构造必成功，返回新壳   |
| 销毁 Actor | `IWorld.TryDestroyActor(ActorId)`              | 不存在返回 false  |
| 取 Actor  | `IWorld.TryGetActor(ActorId, out Actor)`       | 命中返回 true    |
| 挂/卸组件    | `IWorld.AddComponent / RemoveComponent`        | 经 World 统一入口 |
| 取组件      | `IWorld.TryGetComponent<T>(ActorId, out T)`    | 经 World 统一入口 |

> **不在此篇**：World 门面与仿真步进的完整契约见 [04 仿真核心](04-simulation-core.md)；行为驱动见 [05](05-behavior-model.md)。

