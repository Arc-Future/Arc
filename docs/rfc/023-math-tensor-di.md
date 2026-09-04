# RFC 023 数学、张量与依赖注入

## 背景

数学原生直射、张量计算、资源管理（计时/环境）与依赖注入是标准库的数值与组合能力。设计目标：数学零开销直射 LLVM/intrinsic、张量 facade 语义清晰、DI 编译期生成类型化工厂实现零运行时反射。

## 设计决策

### Math（根命名空间 `Arc`）

`Math` 直接 lowering 至 LLVM intrinsic / libm，**无 `rt_math_*` 中间层**——零开销直射。

| 面 | 成员 |
|----|------|
| 基础 | `Sqrt`/`Floor`/`Ceiling`/`Clamp`/`PI`/`CopySign`/`Cbrt`/`Hypot`/`IEEERemainder`/`Sign`/`Abs` |
| 三角/对数 | `Asin`/`Acos`/`Atan`/`Atan2`/`Sinh`/`Cosh`/`Tanh`/`Log10`/`Tan` |

`float` 变体、`DivRem`/`BigMul` 不在本设计面内。`Convert` 提供 `ToInt32`/`ToInt64`/`ToDouble`/`ToBoolean`/`ToString` + 进制转换（仅 2/8/10/16）。

### Tensor / Vector

| 类型 | 命名空间 | 载体 | 约束 |
|------|----------|------|------|
| `Tensor<T>` | `Arc` | `rt_tensor_*` ABI | **禁止运算符重载**，用 `Tensor.Add(a, b)` |
| `Vector<T, N>` | `Arc` | LLVM SIMD | 定长向量 |

张量融合面不在本设计面内。

### 资源管理（计时 / 环境）

| 面 | 类型 | 说明 |
|----|------|------|
| 计时 | `Arc.Diagnostics.Stopwatch` | 高精度间隔测量，`Elapsed` 为 `TimeSpan`；**计时单一惯用法**——无 `Environment.TickCount*` |
| 环境 | `Arc`（`Environment`） | Get/Set 环境变量、NewLine、ProcessorCount、Platform/`Is*`、MachineName/UserName；`rt_env_*` |

`GetFolderPath`/`ExpandEnvironmentVariables`/`ProcessId`/`ProcessPath` 不在本设计面内。`.resx` 资源与 ResX CodeGen 见 [027 本地化与资源](027-localization-resources.md)。

### 依赖注入（`Arc.DI`）

| 面 | 类型 | 可见性 |
|----|------|--------|
| 用户入口 | `ServiceCollection`/`ServiceCollectionExtensions`/`IServiceCollection`/`IServiceScope`/`IServiceScopeFactory`/`ServiceLifetime` | public |
| 用户扩展 | `ServiceProviderExtensions`（`GetService<T>`/`GetRequiredService<T>`/`GetKeyedService<T>`/`CreateScope`） | public |
| 工具链注入 | `ServiceDescriptor`（codegen 拦截 `new ServiceDescriptor(...)` 生成工厂闭包） | public |
| 容器实现 | `ServiceProvider`/`ServiceScope` | internal（用户经 `IServiceProvider`/`IServiceScope` 接口使用） |

```as
using Arc.DI;

var services = new ServiceCollection();
services.AddTransient<ILogger, ConsoleLogger>();
services.AddSingleton<IService, Service>(() => new Service());

IServiceProvider provider = services.Build();
ILogger logger = provider.GetRequiredService<ILogger>();
```

**设计决策**：

- **编译期工厂（零反射）**：codegen 为每个注册生成类型化工厂闭包（`__di_factory_TImpl`），运行时经 `IServiceProvider` 虚分派解析，无运行时反射。
- **依赖解析零分配**：工厂依赖类型经模块级 immortal `RuntimeType` 全局常量传入（refcount 置哨兵 `INT32_MAX - 1`，`rt_arc_dec` 永不归零），构造注入解析路径零堆分配；`IServiceProvider` 胖指针仅在构造器确实需要时构造。
- **生命周期**：`ServiceLifetime` 含 Transient / Scoped / Singleton；作用域经 `IServiceScopeFactory.CreateScope()` 创建，容器内 `ServiceProvider`/`ServiceScope` 为 internal 实现。Singleton 于 `Build()` 期预构造（解析路径无锁）；transient disposable 不由容器跟踪（解析方负责，对齐 .NET MEDI 语义）。
- **Keyed services**：`GetKeyedService<T>` 支持按 key 解析；key 仅字符串、按**值相等**比较（Arc 无值类型装箱，不支持 int/enum 键）。
- **多注册**：`[Inject]` 支持 `AllowMultiple`——一个实现类附加多个标记注册到多个服务接口，每标记独立成一条注册；同名服务（含同名 keyed）后注册覆盖（**最后注册优先**，对齐 .NET）；`GetServices` 按注册顺序返回全部实例。
- **构造器缺依赖**：解析时某构造器依赖未注册（`GetService` 返回 null）→ 抛 `InvalidOperationException`（含依赖类型名），不静默注入 null。
- **Build 快照**：`Build()` 对描述符列表做快照，构建后对集合的 `Add` 不再影响已构建 Provider（对齐 .NET）。
- **嵌套作用域**：作用域内可解析 `IServiceScopeFactory`（委托根容器），嵌套 `CreateScope` 可用；dispose 后解析抛 `ObjectDisposedException`。
- **循环依赖运行期检测**：注册图在运行期（`Build()` 预构造 Singleton 时）经构造栈检测循环依赖，报含环路径的 `InvalidOperationException`（如 `A -> B -> A`），而非崩溃/无限递归。
- **构造注入为主**：字段/属性注入不在本设计面内（永久排除，避免隐式依赖）。
- `InternalsVisibleTo` 使 std 包 internal 实现可被测试程序验证，用户程序仍被 typeck 硬拒绝。

**明确不做**（架构红线）：

- **全编译期依赖图内联**（Dagger 式静态图）：与运行时动态注册（工厂委托/keyed/Build 后语义）构成双轨解析路径，违反单一惯用法宪章（拒绝双轨）。
- **type_id 64 位化**：`TypeId i32` 属 `rt_*` ABI 冻结面（vtable slot0/`_typeInfoHandle`），破坏性变更须循 RFC 036 流程另议。

## 边界

- 数学/张量/DI 的详细面仅此篇；DI 生命周期细节与编译期工厂均在本篇内，不外分。
- `.resx` 资源、文化感知格式化归 [027 本地化与资源](027-localization-resources.md)。
- 数值类型与运算符语义属语言核心（见 [007 集合、字符串与数值](007-collections-strings-numerics.md)）。
- 并发集合/线程原语见 [024 并发集合](024-concurrent-collections.md)。

---

上一节：[022 异步任务与 LINQ/序列化](022-async-linq-serialization.md) · 下一节：[024 并发集合](024-concurrent-collections.md)