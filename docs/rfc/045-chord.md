# RFC 045 插件内核（Arc.Chord）

> **注（2026-09-01 更名）**：`Arc.Plugins` 更名 **`Arc.Chord`**——几何上 chord 是 arc 上两点的连线（内核即插件与宿主间的连接物），音乐上 chord 是多声部协奏，词根 corde（心弦）与 Cordis 同族；`std/Plugins/` → `std/Chord/`，命名空间 `Arc.Chord`，随本 RFC 修订同一变更集落地。
>
> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令不再可用；现行验证矩阵为
> `cargo test --workspace`（运行时面 `cargo test -p arc-tests --features full-rt`），
> 详见仓库根 `CHANGELOG.md`。

## 背景

Cordis（Koishi 生态、北大 + DeepSeek-AI 论文《A Programming Paradigm for Spatiotemporal Composability》）的插件模型以**依赖注入**与**可逆副作用**为双核心：一切操作（注册副作用、提供服务、订阅事件、安装插件）都发生在 `Context` 上；每个插件拥有独立子上下文与作用域；作用域释放时按 LIFO 逆序撤销其全部副作用。

Arc 语言体系已具备插件化 / 热插拔的底层能力：

- **程序集热卸载**（RFC 017）：`AssemblyLoadContext` + `IAssemblyLifecycle`，二进制级加载/卸载钩子；

- **依赖注入容器**（RFC 023）：`Arc.DI` 的服务注册/解析/作用域/装饰器。

**决策前提（核心裁决收尾）**：声明式贡献机制已整体清除出编译器核心（收集器 / 载体发射 / 记账注册器全删）——跨包装配的唯一形态是**源码打包下的显式静态注册**（RFC 012/037）；本内核是**运行期动态装配的唯一权威**，贡献面的一切动态语义（注册/撤销/依赖/广播）只能经本内核承载，编译器零参与。

本 RFC 决策**进程内插件内核** `Arc.Chord`（`std/Chord/`，命名空间 `Arc.Chord`）：在语言层之上提供作用域树、副作用账本、动态服务、事件、事务与热替换。二进制热卸载（RFC 017）与内核**正交**——`Reload` 可与 ALC 卸载组合使用，内核自身不感知二进制形态。

## 架构

四层职责单一向上依赖，贡献语义单轨：

```
┌──────────────────────────────────────────────────────────────┐
│ 领域层（UI / AI / Web / Harness …）                            │
│   贡献点 = 类型化服务（D11）；插件经 Contribute 注册条目，         │
│   卸载/失败/事务回滚自动撤销——宿主无需手写撤销逻辑                │
├──────────────────────────────────────────────────────────────┤
│ Arc.Chord 插件内核（std/Chord/）——运行期动态装配唯一权威          │
│   Context 树 · 可逆副作用账本 · 动态服务 · 事件 · 事务 · 热替换    │
├──────────────────────────────────────────────────────────────┤
│ 显式静态注册（编译期，RFC 012/037）                              │
│   [Inject]/[AITool] → 合成 __AIToolHost / AddXxx 类型化绑定；     │
│   编译器核心零贡献机制（残留清收 commit 5236d0c4）                │
├──────────────────────────────────────────────────────────────┤
│ 语言核心 + ALC 热卸载（RFC 017；与本内核正交组合，见 D8.1）        │
└──────────────────────────────────────────────────────────────┘
```

内核模型一句话：**一切操作都发生在** **`Context`** **上，一切操作都可逆。**

## 设计决策

### D1 Context 即一切

- `Context` 是唯一操作入口；`Plugin(...)` 安装插件 = 创建**子上下文**（天然形成上下文树）。

- 根上下文由用户 `new Context()` 创建；`Start()` 启动并级联子上下文。

- 每个上下文绑定一个 `Scope`（`IScope`）：`Uid` / `Name` / `Status` / `Config` / `Error`。

### D2 副作用唯一账本

所有高级 API 一律**实现为副作用**（Cordis 同构）：

| API                | 效果                             |
| ------------------ | ------------------------------ |
| `Effect(callback)` | 注册时立即执行 `callback()`，保存返回的撤销句柄 |
| `On/Once`          | 订阅事件，撤销 = 退订                   |
| `Provide`          | 提供服务，撤销 = 撤销提供                 |
| `SetConfig`        | 设置配置，撤销 = 恢复旧值                 |
| `Timeout/Interval` | 定时回调，撤销 = 取消                   |
| `Plugin`           | 安装插件，撤销 = 卸载（释放子上下文）           |
| `Inject`           | 依赖注入执行，撤销 = 丢弃注入               |
| `Contribute`（D11）  | 向贡献点注册条目，撤销 = 移除条目             |

- 注册即执行（同步）；释放按 **LIFO 逆序**撤销；单个句柄可独立撤销（幂等）。

- 撤销执行异常安全：批量撤销时单个失败不中断其余撤销（对齐 `Arc.DI` Dispose 异常安全）。

### D3 服务为可撤销阴影注册

- `Provide(name, instance)`：本地同名覆盖并保存旧条目；撤销恢复旧条目（**后写优先**：被更新的 Provide 覆盖后，旧句柄撤销为 no-op）。

- `GetService(name)` / `HasService(name)`：沿**祖先链**上溯（本地优先）；子上下文提供的服务对其后代可见（Cordis 隔离模型）。

- `Provide` 返回撤销句柄；上下文释放时其全部服务自动撤销。

### D4 注入反应式

- `Inject(names, callback)`：全部依赖可用 → 立即执行；否则**挂起等待**；任一依赖在等待期间消失 → 注入丢弃（永不执行）。

- `InjectReactive(names, callback)`（**Cordis 之外的新原语**）：执行后若依赖服务被撤销且链上不再可用 → 自动回滚回调的副作用（效果区间撤销）；重新可用 → 自动重跑。

- 回调在**所属上下文**上注册副作用（纳入该上下文账本与事务）。

- 注入句柄可丢弃：未执行 → 移除等待；已执行 → 仅标记（回调副作用由作用域账本管理，Cordis 语义）。

### D5 事件双向

- `On(name, listener)` / `On(name, listener, prepend)` / `Once(name, listener)`。

- `Emit(name, payload)`：自身 + **后代**（DFS，快照遍历，容忍中途退订/卸载）。

- `Bubble(name, payload)`：自身 + **祖先**。

- `EmitSelf(name, payload)`：仅自身。

- 监听器签名 `Action<object?>`；类型化形态经扩展方法 `On<T>`/`Emit<T>`/`Once<T>` 提供（`(T)payload` 转换）。

### D5.1 瀑布事件（waterfall，对齐 Cordis 四模式）

Cordis 事件四模式 `emit / waterfall / parallel / serial` 中，`emit` 由 D5 承载；`waterfall` 是中间件管道刚需（Agent 工具流水线、pre-execute 拦截），**内核单线程同步模型下零成本补齐**：

```as
namespace Arc.Chord;

// Context（核心面）
// 订阅瀑布：handler(payload, next) 串联；不调 next 即拦截（短路）。
IDisposable OnWaterfall(string name, Func<object?, Func<object?, object?>, object?> handler);

// 触发瀑布：按注册序串联（prepend 插队），末端无监听时 next 为恒等（原样返回）。
object? Waterfall(string name, object? payload);
```

- 串联序 = 注册序（`prepend` 插队到队首），`next(payload)` 显式委托下一环。

- 瀑布订阅同样经副作用账本承载（撤销 = 退订）；类型化形态 `OnWaterfall<T>` / `Waterfall<T>` 经扩展方法提供。

- `parallel` / `serial` 维持边界外（宿主 EventLoop 集成，见边界）。

### D6 副作用事务（Cordis 不具备）

- `BeginTransaction()` 返回**事务上下文**（父 = 当前上下文）。

- 事务内的一切副作用（On/Provide/SetConfig/Plugin/Inject/Contribute…）记入事务账本。

- `Commit()`：效果条目、子上下文、挂起注入**原子迁移**到父上下文；生命周期钩子按父状态即时生效。

- 未 Commit 即 `Dispose()`：全部回滚。

- 支持嵌套（事务内再开事务，逐层合并）。

### D7 失败回滚

- 插件 `apply` 抛异常：已注册副作用**全部撤销**、`Scope.Status = Failed`、`Scope.Error` 记录消息；**不向安装方抛异常**（宿主韧性，插件失败不拖垮宿主）。

- `Effect(callback)` 回调抛异常：不记录条目，异常向调用方传播。

### D8 热替换 Reload

- `Reload(oldContext, apply, config)`：**先装新、成功后再卸旧**（原位替换，保持插件顺序）。

- 失败时旧插件保持运行（新插件安装失败不影响旧插件）。

- 新插件以 `InjectReactive` 消费旧插件服务可实现服务连续性的热交换（内核提供的编排原语）。

### D8.1 与二进制热卸载的组合契约（RFC 017）

内核 Reload 与 ALC 二进制代数（[017 热卸载](017-build-artifacts-packages.md)）**正交组合**：内核编排服务面（切换/回滚），ALC 编排二进制生命周期（Load/Unload/代数）；组合点在插件安装/卸载回调内显式驱动 ALC，内核不感知二进制形态，ALC 不感知服务拓扑。

**换代编排序列**（二进制插件热重载的标准序）：

1. 编译新代 dll 至独立路径（不覆盖旧代文件——旧代卸载前被 OS 锁定）；
2. `alc.Load(newPath)` 加载新代（Load 本身不校验类型身份——符号在调用期才解析）；
3. **Entry 烟测（指纹门禁）**：对新代调用 `Entry<T>()`——同名异构（布局漂移）在此显式失败（`EntryPointNotFoundException`，符号含布局指纹段）→ 编排器捕获 → 进入回滚（旧代保持运行）；烟测通过 = 新代类型身份与布局经实证兼容；
4. 内核 `Reload`：新插件 apply 成功（服务面已切新代）后，于卸旧回调中执行 `alc.Unload(oldPath)`；
5. 旧代卸载前置（017 前置条件）：服务面已无旧代对象引用（内核效果撤销 + InjectReactive 断开）+ 宿主跨界引用置 null；跨模块依赖由**卸载顺序护栏**（017 `E_UNLOAD_DEPENDED`）与拓扑序 `UnloadAll` 兜底；
6. 在途调用由 ALC Freeze 的 in-flight 收敛等待（017 §2.4）。

**失败回滚映射**：新代 `Load` 抛 `IOException` / Entry 抛 `EntryPointNotFoundException`（指纹不匹配）/ apply 抛异常 → 内核 D7 回滚，旧代二进制与服务面均保持——**回滚零成本**（旧代从未卸载，无状态抢救问题）。

**状态迁移分层**：当前换代为冷切换（旧代对象 tombstone 全灭，新代从零开始）。保状态能力按三层演进：

- **L1 兼容性判定（已实施）**：编译器为全部自定义 Named 类型（classes + structs）计算 `entry_layout_signature`（FNV-1a-64 布局传递闭包，含嵌套字段类型的深层变化），以 `#layouts:` 自描述前缀字段物化进 `__arc_package_meta`（`Type:sig;...` 子表）；运行时原语 `Assembly.GetLayoutSignature(typeName)` 与 `AssemblyHotReload.IsLayoutCompatible(old, new, typeName)`——同名类型同指纹 → 结构兼容；任一未物化（旧产物）或指纹异 → **保守拒绝**（未知 ≠ 兼容）。

- **L2 应用层状态搬运（已实施，见 l2\_hot\_reload\_batch 的 hr\_state\_handover）**：兼容判定通过后，宿主在换代窗口内从旧代对象读出状态、写入新代对象（字段级显式搬运）——语义诚实：迁移路径对使用者可见、可审计。

- **L3 透明对象图迁移（已实施，设计见 [047](047-object-graph-migration.md)）**：rt 层 `rt_arc_retype` 头重绑原语——重绑不改地址不变量（引用值全部不变、计数天然保持）、字段指纹 + vtable 形状双重判定、walk 复用枚举、收集器与根扫描交互契约、迁移编排与反向回滚。std 门面 `AssemblyHotReload.MigrateInstances`（判定失败 -3 保守拒绝 → 降级 L2 或拒绝换代）；验证锚点 `hr_transparent_migration` + `hr_virtual_dispatch_after_migration`（迁移后基类虚分派命中新代实现）。

**验收要点**：组合序列红绿——新代指纹不匹配 → 回滚且旧代持续服务；正序换代全链路（载新 → 切面 → 卸旧）绿；`E_UNLOAD_DEPENDED` 在编排误序时拒载并报告依赖方。

### D9 生命周期

- `OnReady/OnStart/OnStop` 注册钩子（已过阶段 → 立即执行）；`Start()` 启动（ready → start → 级联子上下文）。

- 父已启动时安装的插件子上下文在 apply 成功后立即启动。

- `Stop()`/`Dispose()`：子上下文先于自身释放（逆序），随后自身效果 LIFO 撤销，最后 stop 钩子（逆序）与状态标记。

### D10 定时器

- `Timeout(callback, delayMs)` / `Interval(callback, periodMs)`：专用工作线程 + 协作式取消（短睡眠轮询取消标志）。

- 回调在**工作线程**执行；内核状态机操作须由使用者回送主线程（内核单线程同步模型，线程边界文档声明）。

### D11 贡献机制：插件容器热插拔（零新内核原语）

VSCode `contributes` / Eclipse extension point 的运行期化，**容器与贡献项双面热插拔**：插件容器（扩展点宿主，钩子）与贡献项（挂件）都可在运行期动态加载/卸载。机制四件套为纯库契约（**不引用上下文，剥离语言核心**），可逆性由 D2 副作用账本在扩展层组合；构建于 D3 服务之上（注册表经类型化服务解析），天然获得可逆性、事务性与热替换。

**与 IChordContext 的融合（基座 + 增强）**：`IContributeRegistry` 本身注册进 `ServiceCollection`（DI 单例），任何宿主/服务无需经过上下文即可直接 `Add`/`Remove`（容器拆装）与 `Register`/`Unregister`（扩展拆装）——组装/拆装即热插拔，基座自成一体。`IChordContext` 体系在其上增强编排能力（账本可逆性、事务、失败回滚、依赖准入、反应式注入、热替换、作用域树隔离）——两层为「基座 + 增强」，非平行双轨；扩展层 `ctx.Contribute`/`AddHost` 即「注册表操作 + 账本托管」的组合。

**命名家族**：Contribute 家族（`IContribute` / `ContributeOptions` / `IContributeHost` / `IContributeRegistry` / `ContributeRegistry`）+ Chord 家族（`IChordContext` / `ChordContext` / `ChordContextExtensions`）+ Tone 家族（`ITone` / `IToneRequirements`）。

```as
namespace Arc.Chord;

/// <summary>贡献点——插件交付给插件容器的扩展项（以唯一 Id 区分）。</summary>
public interface IContribute {
    string Id { get; }
}

/// <summary>贡献注册元数据（结构体，值类型）：分组 / 排序 / 父级归属。</summary>
public struct ContributeOptions {
    ContributeOptions(int groupId, int order, string? parentId);
    int GroupId { get; }           // 同组归入同一功能区
    int Order { get; }             // 组内执行 / 展示顺序
    string? ParentId { get; }      // 父级贡献标识（null 为顶层）
}

/// <summary>插件容器——承载一类贡献项的扩展点宿主（钩子）。</summary>
public interface IContributeHost {
    string Id { get; }             // 容器唯一标识（扩展点名，调度键）
    void Register(IContribute contribute, ContributeOptions options);
    void Unregister(IContribute contribute);  // 与 Register 严格对称
}

/// <summary>统一注册表——容器与贡献项的唯一调度入口。</summary>
public interface IContributeRegistry {
    void Add(IContributeHost host);    // 容器热插拔（运行期动态扩展功能域）
    void Remove(IContributeHost host); // 容器移除
    void Register(string hostId, IContribute contribute, ContributeOptions options);
    void Unregister(string hostId, IContribute contribute);
    bool HasHost(string hostId);
}
```

语义细则：

- **双面热插拔**：容器经 `Add`/`Remove` 运行期增删（发布后扩展新功能域，无需停机）；贡献项经 `Register`/`Unregister` 按容器 Id 定向注册。
- **元数据透传**：`ContributeOptions`（GroupId/Order/ParentId，结构体值类型）随 Register 透传给容器，由容器编排层级与顺序。
- **账本组合**：扩展层 `ctx.AddHost(host)` / `ctx.Contribute(hostId, contribute, options)` 以 `Effect` 承载——撤销 = `Remove`/`Unregister`；音卸载/失败回滚/事务回滚自动注销。
- **严格解析**：注册表未就绪或容器未注册 → 抛异常 → 触发 D7 失败回滚。

宿主示例（菜单贡献点）：

```as
// 宿主：定义并注册容器
public class MenuContributeHost : IContributeHost {
    public string Id { get { return "ui.menus"; } }
    private List<IContribute> _entries = new List<IContribute>();
    public void Register(IContribute contribute, ContributeOptions options) { this._entries.Add(contribute); }
    public void Unregister(IContribute contribute) { this._entries.Remove(contribute); }
}
app.AddHost(new MenuContributeHost());

// 插件：一行贡献，卸载自动注销
Context plugin = app.Tone(ctx => {
    ctx.Contribute("ui.menus", new MenuContribute("编译"), new ContributeOptions(0, 10, "file"));
    return null;
});
```

### D12 音依赖声明（时间可组合性准入）

Cordis 的核心优势——**依赖图显式化，加载顺序由服务依赖推导而非手动编排**——以可选声明面补齐：

```as
namespace Arc.Chord;

/// <summary>
/// 可选依赖声明：对象形态音实现之，内核按声明准入；
/// 未实现视为无声明（零破坏，既有 ITone 实现不受影响）。
/// </summary>
public interface IToneRequirements {
    /// <summary>依赖的服务名列表；全部就绪（祖先链可达）方可执行 Apply。</summary>
    List<string> Requires { get; }
}
```

语义细则：

- **挂起准入**：安装时逐项 `HasService` 校验——全部就绪 → 立即 apply（现行行为）；任一缺失 → 音子上下文保持 `Pending`（不执行 apply），任一依赖经 `Provide` 出现 → 自动启动。

- **声明即契约**：`Requires` 中的服务在音存活期由内核经 `InjectReactive` 同款语义守望（依赖被撤销且链上不再可达 → 与 D4 反应式一致，由音自身副作用账本回滚承载；内核不额外复制回滚机制）。

- **单一惯用法**：需依赖声明的音采用对象形态实现 `ITone` + `IToneRequirements`；函数形态保持 `Tone(apply)` / `Tone(apply, config)` 原样（不增重载——依赖编排属结构化契约，非脚本便利面）。

- **启动序推导**：`Start()` 级联时同样按声明校验，未就绪子上下文延后至依赖就绪——加载顺序由依赖图推导（Cordis 时间可组合性），替代手动编排。

### D14 类型化服务与 DI 融合

**类型即契约，通道按语义名**：服务/配置/贡献注册表走类型派生键（与 Arc.DI 心智同构）；事件/瀑布走语义名（Cordis 同构路由面）。同一注册表、同一可逆语义，两种键拼法各有管辖域——非双轨。

- **类型派生键**：`Provide<T>(T)` / `GetService<T>()` / `HasService<T>()` / `Inject<T>(Action<Context,T?>)` / `InjectReactive<T>` 以 `typeof(T).FullName` 派生键（RFC 028 `Type.FullName` 唯一限定名契约；RtTypeInfo.full_name 自 RFC 018 M2 起发射真实 `Ns.Type` 点分限定名）。字符串名形态全保留（运行期动态名场景）。

- **工厂提供**：`Provide<T>(Func<T> factory)` 按需构造——首次解析时构造并缓存（MEDI 工厂语义同构，未消费不构造）。

- **DI 兜底解析**：`ChordContext` 可持有 `IServiceProvider`——`IChordContext` 注册进 `ServiceCollection`（`AddSingleton<IChordContext>(sp => new ChordContext(sp))`），宿主服务经构造注入消费编排面。解析次序：**动态阴影链优先（可逆层，音可覆盖），DI 容器兜底（静态层，不可变）**。

- **DI 依赖恒就绪**：DI 可解析的注入依赖立即执行（静态层不可变，无挂起/回滚语义）；动态依赖维持 D4 挂起/回滚语义。

- **前提落地**：codegen 发射 `full_name`/`ns` 真实限定名（layout 层 `type_full_names` 由 HIR namespace 经 `type_fqn` 拼接）；`name` 字段与 `type_id` 哈希输入不变（RFC 026 `type_name_to_id` 勿动共识）。

### D13 组合即数据（方向性能力，另立 RFC）

宿主装配清单（`arc.toml` `[contributes]` 段声明插件与贡献点绑定，编译期生成**显式静态注册**代码）为方向性能力：声明式外观、静态注册实质，不复活编译器收集器。需要沙箱与绑定语义专门设计，不授权立即实现（对齐 RFC 012「编译期变换器」纪律）。

## 边界

| 不做                            | 归属                                           |
| ----------------------------- | -------------------------------------------- |
| 配置 Schema 校验                  | 宿主层（本内核仅承载 Config 读写与撤销）                     |
| 权限过滤器 DSL（Koishi filter）      | 上层框架                                         |
| 异步事件分派 / parallel / serial 事件 | 宿主 EventLoop 集成（waterfall 为唯一内核内建扩展，D5.1）    |
| 二进制热卸载                        | RFC 017 `AssemblyLoadContext`（与 Reload 正交组合） |
| 定时回调线程安全保证                    | 使用者（D10）                                     |
| 装配清单组合（D13）                   | 另立 RFC                                       |
| 贡献点条目类型校验                     | 贡献点实现（`IContributionPoint<TEntry>` 泛型约束承载）   |

## 验收要点

1. 副作用 LIFO 撤销与单句柄撤销幂等；
2. 服务阴影注册、祖先链可见性、撤销恢复旧值；
3. Inject 挂起/就绪/丢弃；InjectReactive 回滚与重跑；
4. Emit 后代广播与 Bubble 祖先冒泡、Once 自退订、prepend 顺序；
5. 事务 Commit 合并与 Dispose 回滚、嵌套事务；
6. 插件失败回滚 + Failed 状态；Reload 先装新后卸旧；
7. 生命周期钩子顺序与级联启动；
8. e2e 全绿（现行矩阵 `cargo test --workspace`）；
9. D11：容器经 `AddHost` 注册、贡献经 `Contribute` 定向投递（撤销 = `Unregister`/`Remove` 严格对称）；注册器未就绪或容器未注册 → 失败回滚；事务内贡献原子生效/回滚；容器运行期 `Add`/`Remove` 热插拔；
10. D12：声明缺失 → Pending 挂起，Provide 唤醒启动；Start 级联按依赖图推导；
11. D5.1：Waterfall 注册序串联、prepend 插队、短路拦截、末端恒等；撤销 = 退订；
12. D14：类型化服务以 `typeof(T).FullName` 为键（动态优先、DI 兜底）；工厂提供按需构造；`IChordContext` 注册进 `ServiceCollection` 后宿主经构造注入消费；DI 可解析注入恒就绪。

