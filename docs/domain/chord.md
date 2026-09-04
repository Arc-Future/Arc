# Arc.Chord 插件内核

## 概述

`Arc.Chord`（`std/Chord/`）是 Arc 的**进程内插件内核**，对标 Cordis（Koishi 生态）的插件模型，并以可逆副作用、反应式注入、副作用事务与热替换四项原语超越之；贡献点（D11）与依赖声明（D12）把 VSCode/Eclipse 的扩展点模型与 Cordis 的时间可组合性统一到同一副作用账本。设计决策权威出处：[RFC 045](../rfc/045-chord.md)。

> 命名：chord 是 arc 上两点的连线（内核即插件与宿主间的连接物），亦是多声部的和弦；词根 corde（心弦）与 Cordis 同族。原名 `Arc.Plugins`（2026-09-01 更名）。

内核模型一句话：**一切操作都发生在 `ChordContext` 上，一切操作都可逆**。

- **ChordContext**：操作入口，安装音即创建子上下文（上下文树）；
- **可逆副作用**：`Effect` 注册即执行、释放按 LIFO 逆序撤销，`On`/`Provide`/`SetConfig`/`Tone`/`Contribute` 全部构建其上；
- **服务**：类型即契约（`typeof(T).FullName` 派生键，DI 兜底），`Provide`/`GetService`/`Inject`（含反应式 `InjectReactive`）；
- **贡献机制**：容器与贡献项双面热插拔，`Contribute` 注册条目、卸载自动注销；
- **依赖声明**：`IToneRequirements.Requires` 挂起准入，启动序由依赖图推导；
- **事件**：`Emit`（自身+后代）/ `Bubble`（自身+祖先）/ `Once` / `prepend` / `Waterfall`（瀑布管道）；
- **事务**：`BeginTransaction` 批量副作用原子提交或回滚；
- **热替换**：`Reload` 先装新、成功后再卸旧，音原位热交换。

### 三族一体（一个方案，三个角色面）

API 按架构角色分为三族——**一个方案，三个角色面**，非三个方案：

| 族 | 角色 | 成员 | 依赖 |
|----|------|------|------|
| **Contribute 族** | 被组装的**物料**（扩展点与扩展项） | `IContribute` / `ContributeOptions` / `IContributeHost` / `IContributeRegistry` / `ContributeRegistry` | 不依赖任何族（纯契约，可独立于上下文经 DI 直用） |
| **Tone 族** | 执行组装的**单元**（可编排功能单元契约） | `ITone` / `IToneRequirements` | 依赖 Contribute（音贡献物料）与 Chord（Apply 签名） |
| **Chord 族** | 承载组装的**引擎**（编排 + 生命周期） | `IChordContext` / `ChordContext` / `ChordContextExtensions` | 依赖前两族（引擎实现编排） |

三族是一套「宿主—单元—物料」组合模式的角色词汇分区（对标 VSCode 的 extension host / extension / contributes 三角）。**一体性由三条不变量保证**：单一账本（一切操作同一条效果账本）、单一上下文树（唯一解析域）、单一解析习语（类型即契约 + DI 兜底）；依赖方向无环分层（Contribute ← Tone ← Chord）。若三族出现独立注册表、独立生命周期或跨族适配器，即告方案碎裂——目前不存在。

> 二进制级热卸载由 RFC 017 `AssemblyLoadContext` 承载，与本内核正交；`Reload` 可与 ALC 卸载组合使用。

## 架构位置

```
领域层（UI / AI / Web）── 贡献点 = 类型化服务，Contribute 注册即副作用
    ↑
Arc.Chord 内核 ── ChordContext 树 · 副作用账本 · 动态服务 · 事件 · 事务 · 热替换
    ↑
显式静态注册（编译期，RFC 012/037）── [Inject]/[AITool] 类型化绑定；编译器零贡献机制
    ↑
语言核心 + ALC 热卸载（RFC 017）── 与内核正交组合
```

编译期静态装配与运行期动态装配分工：**结构在编译期定死（零反射），变化在运行期编排（可逆）**——两层共享「撤销 = 对称反操作」的单一语义。

## 快速开始

### 1. 根上下文与副作用

```as
using Arc.Chord;

ChordContext app = new ChordContext();

// 副作用：注册时立即执行，返回撤销句柄
IDisposable handle = app.Effect(() => {
    Console.WriteLine("effect started");
    return new DisposableAction(() => Console.WriteLine("effect reverted"));
});

handle.Dispose();   // 单独撤销
app.Dispose();      // 上下文释放：全部副作用 LIFO 撤销
```

### 2. 服务与注入

```as
ChordContext app = new ChordContext();

// 提供服务（撤销句柄可撤销提供；上下文释放自动撤销）
app.Provide("greeter", new Greeter());

// 取服务：沿祖先链上溯
Greeter g = (Greeter)app.GetService("greeter");

// 注入：依赖就绪即执行；未就绪挂起等待
app.Inject(new string[] { "greeter" }, ctx => {
    ctx.On("hello", _ => Console.WriteLine("hello injected"));
});

// 反应式注入：依赖消失自动回滚回调副作用，重新可用自动重跑
app.InjectReactive(new string[] { "greeter" }, ctx => {
    ctx.SetConfig("mode", "fast");
});
```

### 3. 事件与瀑布

```as
ChordContext app = new ChordContext();
ChordContext child = app.Tone(ctx => {
    ctx.On("hello", payload => Console.WriteLine("child heard: " + payload));
});

app.Emit("hello", "world");     // 自身 + 后代（child 听到）
child.Bubble("hello", "up");    // 自身 + 祖先（app 听到）

// 瀑布：中间件管道，next 委托下一环；不调 next 即拦截
app.OnWaterfall("request", (payload, next) => {
    return next("[" + payload + "]");
});
object? result = app.Waterfall("request", "ping");
```

### 4. 插件与生命周期

```as
ChordContext app = new ChordContext();

// 函数形态插件：返回撤销句柄；无清理需求用 Action 形态
ChordContext plugin = app.Tone(ctx => {
    ctx.On("ready", _ => Console.WriteLine("plugin running"));
    return new DisposableAction(() => Console.WriteLine("plugin unloaded"));
});

// 对象形态插件
public class HealthPlugin : ITone {
    public string Name { get { return "health"; } }
    public void Apply(ChordContext ctx, object? config) {
        ctx.Provide("health", new HealthService());
    }
}
app.Tone(new HealthPlugin());

// 生命周期：ready → start → 级联子上下文；Stop 逆序卸载
app.OnReady(() => Console.WriteLine("ready"));
app.OnStart(() => Console.WriteLine("start"));
app.Start();

plugin.Dispose();   // 卸载单个插件（副作用逆序撤销）
app.Stop();         // 整体卸载
```

### 5. 贡献机制与依赖声明

```as
// 宿主：定义插件容器（扩展点宿主）并注册统一注册器
public class MenuContributeHost : IContributeHost {
    public string Id { get { return "ui.menus"; } }
    private List<IContribute> _entries = new List<IContribute>();
    public void Register(IContribute contribute, ContributeOptions? options) { this._entries.Add(contribute); }
    public void Unregister(IContribute contribute) { this._entries.Remove(contribute); }
}
app.Provide<IContributeRegistry>(new ContributeRegistry());
app.AddHost(new MenuContributeHost());

// 音：一行贡献（含组织元数据），卸载自动注销；依赖未就绪则音先声明再贡献
public class BuildTone : ITone, IToneRequirements {
    public string Name { get { return "build"; } }
    public List<string> Requires { get { return new List<string>(new string[] { "ui.menus" }); } }
    public void Apply(ChordContext ctx, object? config) {
        ctx.Contribute("ui.menus", new MenuContribute("编译"), new ContributeOptions { Order = 10, ParentId = "file" });
    }
}
app.Tone(new BuildTone());
```

### 6. 事务与热替换

```as
ChordContext app = new ChordContext();

// 副作用事务：Commit 原子合并；Dispose 回滚
ChordContext tx = app.BeginTransaction();
tx.Provide("a", new A());
tx.On("hello", _ => Console.WriteLine("tx listener"));
tx.Commit();   // 全部生效于 app

// 热替换：先装新、成功后再卸旧
ChordContext oldPlugin = app.Tone(ctx => ctx.Provide("greeter", new Greeter()));
ChordContext fresh = app.Reload(oldPlugin, ctx => ctx.Provide("greeter", new GreeterV2()));
```

## 插件写法推导（三族一体的用户面）

每条写法从模型推导：**Chord 族决定入口与生命周期，Tone 族决定单元结构，Contribute 族决定物料形态，消费面统一走类型即契约**。

### 推导律

1. **入口唯一**：一切编排从 `ChordContext` 开始——音不 new 引擎、不逃逸传入的 `ctx`，副作用只落在自己的子上下文上（Chord 族职责）。
2. **单元两形态**：有生命周期/可复用/需单测 → 对象形态 `ITone`；胶水装配 → 函数形态。依赖前置声明 `IToneRequirements`，启动序交给依赖图（Tone 族职责）。
3. **物料纯数据**：贡献项只携带 `Id` 与 `ContributeOptions`，不含行为；消费由容器宿主做（Contribute 族职责）。
4. **消费类型化**：服务消费一律 `Inject<T>` 值直入回调；事件/瀑布走语义名 + 类型化载荷（契约按类型、通道按语义名）。
5. **清理走账本**：函数形态返回撤销句柄、对象形态在 Apply 内 `Effect`——禁止手写对称注销（账本托管是增强层的存在理由）。

### 宿主组合根（三族接线，只写一次）

```as
IServiceCollection services = new ServiceCollection();
services.AddSingleton<IConfig, AppConfig>();
services.AddSingleton<IContributeRegistry>(new ContributeRegistry());   // Contribute 族：基座直注
services.AddSingleton<IChordContext>(sp => new ChordContext(sp));        // Chord 族：引擎持容器
IServiceProvider host = services.Build();

IChordContext app = host.GetRequiredService<IChordContext>();
app.AddHost(new MenuContributeHost());   // 容器注册（或经音 AddHost 走账本）
app.Start();
```

裸用基座（纯 DI 宿主）时，任何服务可直取 `IContributeRegistry` 拆装扩展点——不经上下文即热插拔。

### 音（对象形态）：结构化功能单元

```as
public class OrderTone : ITone, IToneRequirements {
    public string Name { get { return "order"; } }

    // 依赖前置：类型派生键（typeof(T).FullName），启动序由依赖图推导
    public List<string> Requires {
        get { return new List<string>(new string[] { typeof(IOrderRepository).FullName }); }
    }

    public void Apply(ChordContext ctx, object? config) {
        // 消费：值直入回调——无查找、无强转、无判空样板
        ctx.Inject<IOrderRepository>((c, repo) => {
            ctx.OnWaterfall<Order>("order.submit", (order, next) => {
                repo.Validate(order);
                return next(order);
            });
        });

        // 组装：一行贡献（结构体元数据），卸载自动注销
        ctx.Contribute("ui.menus", new MenuContribute("订单"), new ContributeOptions(0, 10, "file"));
    }
}
app.Tone(new OrderTone());
```

### 音（函数形态）：轻量胶水

```as
app.Tone((ChordContext ctx) => {
    ctx.Provide<IClock>(new FakeClock());     // 工厂提供亦可：ctx.Provide<IClock>(() => new FakeClock())
    ctx.On("tick", _ => { });
});                                           // 无清理需求；有则返回 DisposableAction
```

### 反模式（写法禁区）

| 禁区 | 正解 |
|------|------|
| 魔法字符串服务键（`Provide("greeter", …)` + 到处拼写） | 类型键：`Provide<T>` / `GetService<T>()`；字符串键仅限运行期动态名 |
| 回调内 `GetService` + 强转 + 判空 | `Inject<T>((ctx, value) => …)` 值直入 |
| 手写对称 Unregister/Remove | 账本托管：音卸载/失败/事务回滚自动注销 |
| 事件当服务用、服务当事件用 | 契约按类型（服务/配置/贡献），通道按语义名（事件/瀑布） |
| 音逃逸传入的 `ctx`（new 子上下文、落全局） | 一切副作用落在 `Apply` 收到的 `ctx` 上 |

> 现状注记：类型化注入的双参 lambda（`(ctx, value) => …`）当前受语言侧 lambda 统一缺口影响（见 plan.md 登记），语料中暂以显式单参形态或回调内取值过渡；上式为目标形态，缺口根治后即为唯一写法。

## 核心 API

### ChordContext（内核入口）

| 方法 | 说明 |
|------|------|
| `new ChordContext()` | 创建根上下文 |
| `Tone(Action<ChordContext>)` / `Tone(Func<ChordContext, IDisposable>)` / `Tone(ITone)`（各含 config 重载） | 安装插件，返回插件子上下文 |
| `Reload(oldContext, apply, config)` | 原位热替换（先装新、成功再卸旧） |
| `Effect(Func<IDisposable>)` | 注册副作用：立即执行回调，返回撤销句柄 |
| `On(name, listener)` / `On(name, listener, prepend)` / `Once(name, listener)` | 订阅事件（撤销 = 退订） |
| `Emit(name, payload)` / `Bubble(name, payload)` / `EmitSelf(name, payload)` | 事件广播：后代 DFS / 祖先冒泡 / 仅自身 |
| `OnWaterfall(name, handler)` / `Waterfall(name, payload)` | 瀑布管道：`handler(payload, next)` 串联，不调 `next` 即拦截（D5.1） |
| `Provide(name, instance)` | 提供服务（撤销 = 撤销提供，恢复旧条目） |
| `GetService(name)` / `GetLocalService(name)` / `HasService(name)` | 服务解析（祖先链上溯） |
| `Inject(names, callback)` / `InjectReactive(names, callback)` | 依赖注入：就绪即执行 / 反应式回滚重跑 |
| `SetConfig(name, value)` / `GetConfig(name)` / `HasConfig(name)` | 配置读写（撤销 = 恢复旧值；读取沿祖先链） |
| `BeginTransaction()` / `Commit()` | 副作用事务：原子合并 / 回滚 |
| `Timeout(callback, delayMs)` / `Interval(callback, periodMs)` | 定时回调（协作式取消） |
| `OnReady/OnStart/OnStop(callback)` | 生命周期钩子（已过阶段立即执行） |
| `Start()` / `Stop()` / `Dispose()` | 启动级联 / 整体卸载（stop 钩子 + LIFO 撤销） |
| `Parent` / `Scope` / `Uid` / `IsActive` / `IsDisposed` / `EffectCount` / `ChildCount` | 结构信息 |

### ITone / IToneRequirements（对象形态插件）

```as
public interface ITone {
    string Name { get; }
    void Apply(ChordContext ChordContext, object? config);
}

/// 可选依赖声明：实现之则内核按声明准入（未实现视为无声明）。
public interface IToneRequirements {
    List<string> Requires { get; }
}
```

### 贡献机制四件套（插件容器热插拔，D11）

```as
public interface IContribute {
    string Id { get; }             // 贡献项唯一标识
}

public struct ContributeOptions {
    ContributeOptions(int groupId, int order, string? parentId);
    int GroupId { get; }           // 分组
    int Order { get; }             // 组内顺序
    string? ParentId { get; }      // 父级归属（null 顶层）
}

public interface IContributeHost {
    string Id { get; }             // 容器唯一标识（扩展点名，调度键）
    void Register(IContribute contribute, ContributeOptions options);
    void Unregister(IContribute contribute);  // 与 Register 严格对称
}

public interface IContributeRegistry {
    void Add(IContributeHost host);    // 容器热插拔（运行期动态扩展功能域）
    void Remove(IContributeHost host);
    void Register(string hostId, IContribute contribute, ContributeOptions options);
    void Unregister(string hostId, IContribute contribute);
    bool HasHost(string hostId);
}

// ChordContextExtensions（扩展面，账本组合：撤销 = Unregister/Remove）
public static IDisposable Contribute(this ChordContext ctx, string hostId, IContribute contribute);
public static IDisposable Contribute(this ChordContext ctx, string hostId, IContribute contribute, ContributeOptions options);
public static IDisposable AddHost(this ChordContext ctx, IContributeHost host);
```

### 贡献体系与 IChordContext 的融合（基座 + 增强）

贡献体系是**独立组装基座**：`IContributeRegistry` 本身注册进 `ServiceCollection`（DI 单例），任何宿主/服务无需经过上下文即可直接 `Add`/`Remove`（容器拆装）与 `Register`/`Unregister`（扩展拆装）——组装/拆装即热插拔，基座能力自成一体。

`IChordContext` 体系是**编排增强层**，在贡献基座之上覆盖更多能力：副作用账本可逆性（音卸载/失败自动注销贡献，无需手写 Unregister）、事务原子性（批量贡献 Commit/回滚）、依赖准入（IToneRequirements）、反应式注入、热替换、作用域树隔离。两层是「基座 + 增强」关系，非平行双轨——扩展层的 `ctx.Contribute`/`AddHost` 即「注册表操作 + 账本托管」的组合。

**命名家族**：Contribute 家族（`IContribute` / `ContributeOptions` / `IContributeHost` / `IContributeRegistry` / `ContributeRegistry`）+ Chord 家族（`IChordContext` / `ChordContext` / `ChordContextExtensions`）+ Tone 家族（`ITone` / `IToneRequirements`）——三族各自前缀自洽。

### IScope

| 成员 | 说明 |
|------|------|
| `Uid` | 全局唯一标识 |
| `Name` | 作用域名（root / tone / transaction） |
| `Status` | `ScopeStatus`：`Pending` / `Active` / `Failed` / `Disposed` |
| `Config` | 音配置对象（`Tone(apply, config)` 传入） |
| `Error` | 失败原因（Status = Failed 时有值） |

### 类型化服务与 DI 融合（D14）

| 方法 | 说明 |
|------|------|
| `Provide<T>(T instance)` | 类型即契约：键 = `typeof(T).FullName`（撤销 = 撤销提供） |
| `Provide<T>(Func<T> factory)` | 工厂提供：首次解析时构造并缓存（MEDI 工厂语义同构） |
| `GetService<T>()` / `HasService<T>()` | 类型化解析：动态阴影链优先，DI 容器兜底 |
| `Inject<T>((ctx, value) => …)` / `InjectReactive<T>(…)` | 值直入回调；DI 可解析恒就绪，动态依赖维持 D4 语义 |
| `ChordContext(IServiceProvider? services = null)` | 持有 DI 容器；`IChordContext` 注册进 `ServiceCollection` |

### 类型化扩展（ChordContextExtensions）

| 方法 | 说明 |
|------|------|
| `GetService<T>(name)` / `GetConfig<T>(name)` | 类型化解析（`(T)` 转换） |
| `On<T>(name, Action<T>)` / `Once<T>(name, Action<T>)` / `Emit<T>(name, T)` | 类型化事件 |
| `OnWaterfall<T>(name, handler)` / `Waterfall<T>(name, T)` | 类型化瀑布 |
| `Provide<T>(name, instance)` | 类型化提供（显式名形态） |
| `Contribute(hostId, contribute, options?)` / `AddHost(host)` | 贡献投递 / 容器注册（撤销 = Unregister/Remove） |
| `Inject(name, callback)` / `InjectReactive(name, callback)` | 单依赖便捷形态 |

## 语义细则

- **LIFO 撤销**：作用域释放时，副作用按注册逆序撤销；子上下文先于父上下文释放。
- **撤销幂等**：同一句柄重复 `Dispose` 安全；批量撤销中单个失败不中断其余（异常安全）。
- **服务可见性**：`GetService` 沿祖先链上溯；子上下文提供的服务仅对其后代可见（隔离模型）。
- **后写优先**：同名服务被更新 `Provide` 覆盖后，旧句柄撤销为 no-op；配置撤销恢复旧值。
- **注入语义**：`Inject` 依赖缺失时挂起等待，等待期间依赖消失则丢弃；`InjectReactive` 在依赖消失时回滚回调副作用、重新可用时重跑；类型化注入中 DI 可解析依赖恒就绪。
- **贡献语义**：容器与贡献项双面热插拔；`Contribute` 严格解析（注册器未就绪或容器未注册 → 异常 → 失败回滚）；撤销与 `Register`/`Unregister` 严格对称；事务内贡献原子生效/回滚。
- **类型键语义**：类型化服务以 `typeof(T).FullName` 为键，动态阴影链优先、DI 容器兜底；DI 依赖静态不可变，不进入可逆账本。
- **依赖声明语义**：`Requires` 任一缺失 → 插件 `Pending` 挂起（不执行 Apply），`Provide` 唤醒启动；`Start` 级联按依赖图推导启动序。
- **失败回滚**：插件 `apply` 抛异常 → 已注册副作用全部撤销、`Status = Failed`、`Error` 记录原因，不向安装方抛异常。
- **事务语义**：Commit 将效果、子上下文、挂起注入原子迁移到父；未提交即释放则全部回滚；支持嵌套。
- **生命周期**：`Start` 执行 ready → start 钩子后级联子上下文；父已启动时新装插件立即启动；`Stop`/`Dispose` 先子后父、效果 LIFO 撤销。

## 边界

- **配置 Schema 校验**：由宿主层负责，内核仅承载读写与撤销。
- **权限过滤器 DSL**（Koishi filter）：上层框架职责。
- **异步事件分派 / parallel / serial**：内核事件同步分派（waterfall 为唯一内建扩展）；并行/异步分派由宿主 EventLoop 集成。
- **装配清单组合**（arc.toml `[contributes]` 段 → 编译期生成显式静态注册）：方向性能力，另立 RFC（RFC 045 D13）。
- **二进制热卸载**：RFC 017 `AssemblyLoadContext`；`Reload` 可与 ALC 卸载组合。
- **定时回调线程**：`Timeout`/`Interval` 回调在专用工作线程执行（协作式取消）；内核状态机操作须回送主线程。
- **反射**：Arc 无实例类型反查（`GetType()` 永久剔除，RFC 018），对象形态插件名称由 `ITone.Name` 显式提供。

## 与其它库的分工

- **Arc.DI**（[di.md](di.md)）：静态服务容器（编译期注册表）；`Arc.Chord` 服务为动态键控注入（运行期提供/撤销，类型即契约）。**D14 融合**：`ChordContext` 持有 `IServiceProvider`——`IChordContext` 注册进 `ServiceCollection`，宿主装配以 `Arc.DI` 构建根容器（静态结构），`Arc.Chord` 承载运行期编排（可逆变化）；类型化解析动态阴影链优先、DI 兜底。
- **RFC 017 程序集热卸载**：二进制级加载/卸载；`Arc.Chord` 为进程内作用域级可逆编排。
- **RFC 012/037 显式静态注册**：编译期跨包装配；`Arc.Chord` 为运行期动态装配。

---

上一节：[di.md](di.md) · 下一节：[index.md](index.md)
