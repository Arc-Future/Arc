# Arc.DI

## 概述

`Arc.DI` 是 Arc 的依赖注入容器（`std/DI`），对标 .NET `Microsoft.Extensions.DependencyInjection`。它以 `ServiceCollection` 注册服务、以 `ServiceProvider` 解析服务、以 `ServiceLifetime` 定义生命周期、以 `IServiceScope` 承载作用域。配合 `[Inject]` 特性可编译期自动注册。

`Arc.DI` 是纯标准库领域层，面向极致性能、生命周期安全与高并发安全；Web 宿主（`WebApplication`）、AI 宿主等多处复用同一容器（见 [web.md](web.md)、[ai-host.md](ai-host.md)）。

### 核心接口

| 契约 | 职责 |
|------|------|
| `IServiceCollection` | 服务注册集合（`Add` + `Build`） |
| `IServiceProvider` | 服务解析（`GetService`/`GetKeyedService`） |
| `IServiceScope` / `IServiceScopeFactory` | 作用域与作用域工厂 |
| `ServiceLifetime` | 生命周期枚举（`Singleton`/`Scoped`/`Transient`） |
| `[Inject]` | 编译期自动注册标记 |

## 快速开始

### 1. 注册与解析

```as
using Arc.DI;

IServiceCollection services = new ServiceCollection();

// 接口 → 实现类型（codegen 生成工厂）
services.AddScoped<IUserService, UserService>();
services.AddSingleton<IConfig, AppConfig>();
services.AddTransient<ILogger, ConsoleLogger>();

// 自实现类型（TService == TImpl）
services.AddScoped<AuditService>();

// 工厂委托
services.AddSingleton<IEmailSender>(sp => new SmtpEmailSender(sp.GetRequiredService<IConfig>()));

IServiceProvider provider = services.Build();

// 解析
IUserService users = provider.GetRequiredService<IUserService>();
```

### 2. 构造器注入

服务实现经构造器注入依赖，容器自动解析：

```as
public class UserService : IUserService {
    private readonly ILogger _logger;

    public UserService(ILogger logger) {
        _logger = logger;
    }
}
```

### 3. 作用域

`Scoped` 服务在同一作用域内共享实例：

```as
using (IServiceScope scope = provider.CreateScope()) {
    IServiceProvider sp = scope.GetServiceProvider();
    OrderService orders = sp.GetRequiredService<OrderService>();  // 每请求一个实例
}
```

### 4. `[Inject]` 自动注册

`[Inject]` 标记类自动注册进 DI（仅显式标记者注册，非全量盲扫）：

```as
using Arc.DI;

[Inject(typeof(IUserService))]                       // 注册为 IUserService（默认 Scoped）
public class UserService : IUserService { }

[Inject<IService>]                                   // 泛型便捷写法，等价 [Inject(typeof(IService))]
public class ServiceImpl : IService { }

[Inject(ServiceLifetime.Singleton)]
public class GlobalCache { }
```

## 核心 API

### 服务注册（IServiceCollection + 扩展方法）

| 方法 | 生命周期 | 说明 |
|------|----------|------|
| `AddTransient<TService, TImpl>()` / `AddTransient<TService>()` | Transient | 每次解析返回新实例 |
| `AddScoped<TService, TImpl>()` / `AddScoped<TService>()` | Scoped | 同一作用域内共享 |
| `AddSingleton<TService, TImpl>()` / `AddSingleton<TService>()` / `AddSingleton<TService>(instance)` | Singleton | 全容器共享单例 |
| `AddKeyedTransient/Scoped/Singleton<TService, TImpl>(key)` | 对应 | 命名服务（keyed service） |
| 工厂委托重载 `AddXxx<TService>(Func<IServiceProvider,TService>)` | 对应 | 用户提供工厂，无需 codegen |

`IServiceCollection.Add(ServiceDescriptor)` 为原子方法；`Build()` 构建 `IServiceProvider`（编译期固化服务描述符表）。`Build()` 对描述符列表做**快照**——Provider 持有独立副本，构建后对集合的 `Add` 不再影响已构建 Provider（对齐 .NET）。

同名服务（或同名 keyed 服务）可多注册；解析时**最后注册优先**（后注册覆盖，对齐 .NET）。`GetServices` 按注册顺序返回全部实例。

### 服务解析（IServiceProvider + 扩展方法）

| 方法 | 说明 |
|------|------|
| `GetService<T>()` / `GetRequiredService<T>()` | 泛型解析；Required 未注册时抛异常 |
| `GetKeyedService<T>(key)` / `GetRequiredKeyedService<T>(key)` | 命名服务解析 |
| `CreateScope()` | 创建作用域，返回 `IServiceScope` |

`IServiceScope.GetServiceProvider()` 返回作用域内 `IServiceProvider`；`IServiceProvider.CreateScope()` 经 `IServiceScopeFactory` 解析创建（根容器实现之）。作用域内同样可解析 `IServiceScopeFactory`（委托根容器），故**嵌套作用域**可用。

**keyed 键域**：key 仅支持字符串，按**值相等**比较（Arc 无值类型装箱，不支持 int/enum 键）。

**构造器注入缺依赖**：解析时若某构造器依赖未注册（`GetService` 返回 null），抛 `InvalidOperationException`（含依赖类型名），不静默注入 null。

**dispose 语义**：Provider 与作用域释放后再解析一律抛 `ObjectDisposedException`（对齐 .NET）。

### 装饰器（Decorate）

`Decorate<TService, TDecorator>`（对标 .NET Scrutor 扩展，容器不内置装饰器）以 `TDecorator` 包装 `TService` 的**最后一条**注册：`TDecorator` 构造函数须含一个 `TService` 形参（接收被包装的内层实例），生命周期沿用被包装注册。可多次叠加，洋葱序为**后 `Decorate` 者在最外层**：

```as
services.AddTransient<IOrderHandler, OrderHandler>();
services.Decorate<IOrderHandler, LoggingHandler>();   // log(order(...))
services.Decorate<IOrderHandler, MetricsHandler>();   // metrics(log(order(...)))
```

未注册的服务调用 `Decorate` 抛 `InvalidOperationException`（含服务类型名）。

### ServiceLifetime

| 值 | 语义 |
|----|------|
| `Singleton` | 单例，整个根容器共享一个实例 |
| `Scoped` | 作用域，同一 `IServiceScope` 内共享一个实例 |
| `Transient` | 瞬态，每次解析返回新实例 |

> **Singleton 预构造语义**：`Build()` 时即构造全部 `Singleton` 实例（单线程），运行时解析仅做只读列表读取——零锁、零竞态。副作用影响：Singleton 构造函数在 `Build()` 执行（而非首次 `GetService`），故其构造不得依赖运行时状态。循环依赖（`A -> B -> A`）在 `Build()` 预构造时经构造栈检测并抛 `InvalidOperationException`（含环路径），不会崩溃或无限递归。

> **Transient disposable 警示**：transient 实例不由容器跟踪，实现 `IDisposable` 时由解析方负责释放（对齐 .NET MEDI；权威见 [RFC 023](../rfc/023-math-tensor-di.md)）。

### [Inject] 特性

| 字段 | 说明 |
|------|------|
| `Lifetime` | 生命周期，默认 `Scoped` |
| `ServiceType` | 服务注册键（`Type`）；null 则为类型本身（自注册） |
| `ServiceKey` | 命名服务键（keyed service）；空串表示无 key |

多注册（一个实现类同时注册到多个服务接口）：附加多个 `[Inject]` 标记（`AllowMultiple = true`），每个标记独立成一条注册。

```as
[Inject(typeof(ILogger))]
[Inject(typeof(ITracer))]
public class AuditService : ILogger, ITracer { ... }
```

`[Inject<T>]` 泛型形态提供 `[Inject<IService>]` 便捷写法，等价 `[Inject(typeof(IService))]`。

## 边界

- **Web 装配**：`WebApplication.AddServices` 复用 `std/DI`，宿主内建每请求作用域，见 [web.md](web.md)。
- **AI 宿主**：`AISessionOptions.Services` 承载服务解析，见 [ai-host.md](ai-host.md)。
- **连接生命周期原语**：与 ORM 连接管理的关系见 [orm.md](orm.md)。
- **服务注册入口**：`[Inject]` 消费方经显式注册调用 / 静态构造器装配（源码打包合并单一编译单元），见规范章 [RFC 037 §6](../rfc/037-ui.md)。
- **设计决策与产品化冲刺路线**：权威出处为 [RFC 023](../rfc/023-math-tensor-di.md)（含设计决策与明确不做项）。

---

上一节：[networking-p2p.md](networking-p2p.md) · 下一节：[index.md](index.md)