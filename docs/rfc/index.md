# Arc 设计决策（RFC）

> 本目录是 Arc **唯一权威**的设计决策记录。每份 RFC 描述一项**已接受**的最终设计：背景、决策、边界。全篇表述**确定性设计契约**，不含实现进度、状态标记或排期（实现规划见 [实现规划](../plan.md)）。
>
> **组织原则：一主题一文档，互不重叠。** 编号按主题聚合，非研发顺序。当多个旧文档讲同一件事时，已合并为单一文档。
>
> **待裁决提案**：尚在草案/裁决阶段的提案（非已接受 RFC）见 [proposals 索引](proposals/index.md)。

## I · 语言核心（001–012）

| 编号 | 主题 | 边界（不在此篇） |
|------|------|----------------|
| [001 语言宪章](001-language-charter.md) | 定位、四元方程、五条信条、边界 | 具体语法见 002+ |
| [002 语法表面与编码标准](002-surface-contract.md) | 词法、编码决定、命名规范、前导类型 | 类型见 004 |
| [003 词法与语法](003-lexicon-syntax.md) | 声明、语句、表达式、运算符 | 类型见 004 |
| [004 类型系统](004-type-system.md) | 基元、命名类型、泛型、可空、模式匹配 | 内存见 005 |
| [005 内存模型与资源安全](005-memory-model.md) | ARC、移动、借用、Span、循环收集、弱引用、资源确定性 | 对象见 006 |
| [006 对象模型](006-object-model.md) | class、vtable、接口、partial、record、静态成员 | 内存见 005 |
| [007 集合、字符串与数值](007-collections-strings-numerics.md) | 集合表达式、List/Dictionary、字符串、数值类型 | 运算符见 003 |
| [008 委托、闭包与方法组](008-delegates-closures.md) | lambda、捕获语义、方法组、AsyncStream | 异步见 009 |
| [009 异步与并发模型](009-async-concurrency.md) | 状态机、Task、EventLoop、并发集合、线程 | 运行时原语见 014 |
| [010 异常与资源管理](010-exceptions-resources.md) | zero-cost EH、try/catch、using、IDisposable | 内存确定性见 005 |
| [011 表达式树与查询语言](011-expression-trees-query.md) | `Expression<T>`、Provider、Enumerable/Queryable、LINQ | 领域翻译见 039 |
| [012 编译期元编程](012-compile-time-metaprogramming.md) | attribute、GenerateTo、comptime 子集 | 语言类型见 004 |

## II · 编译与运行时（013–019）

| 编号 | 主题 | 边界（不在此篇） |
|------|------|----------------|
| [013 编译管线架构](013-compiler-pipeline.md) | ast→parse→hir→typeck→mir→codegen 单向管线 | 语言类型见 004–012 |
| [014 运行时 ABI](014-runtime-abi.md) | `rt_*` 符号面、内存管理原语、平台接口 | 语言级内存语义见 005 |
| [015 LLVM 原生后端](015-llvm-backend.md) | LLVM IR 文本后端、代码生成、链接、覆盖率插桩 | 管线见 013；覆盖率机制见 [references](015-llvm-backend/references/index.md) |
| [016 验证式 FFI 与 Native 加载](016-verified-ffi.md) | `.ani` 契约、符号验证、`.ani` 加载模型 | 内存模型见 005 |
| [017 编译产物、包体系与类型身份](017-build-artifacts-packages.md) | 源码打包、动态库、跨库身份、热卸载、跨库符号共享策略（混合式）、动态库 Entry 根集可达性裁剪 | CLI 见 031；SDK 布局见 [017 references](017-build-artifacts-packages/references/index.md) |
| [018 类型体系与反射元数据](018-type-reflection-metadata.md) | Type 体系、只读元数据、无反射调用 | 语言类型见 004 |
| [019 自举路线图](019-self-hosting.md) | 用 Arc 写 Arc 编译器、子集边界 | 编译管线见 013；子集边界见 [019 references](019-self-hosting/references/index.md) |

## III · 标准库（020–030）

| 编号 | 主题 | 边界（不在此篇） |
|------|------|----------------|
| [020 标准库架构与拆分](020-std-architecture.md) | 子库拆分、命名空间、internal 边界 | 具体库见 021–030 |
| [021 集合、IO 与文本](021-collections-io-text.md) | 容器库、文件目录、字符串编码 | 语言集合见 007 |
| [022 异步任务与 LINQ/序列化](022-async-linq-serialization.md) | Task 库、Enumerable/Queryable、序列化家族 | JSON 翻译见 039 |
| [023 数学、张量与依赖注入](023-math-tensor-di.md) | Math、Tensor、资源管理、DI 容器 | 生命周期见 020 |
| [024 并发集合](024-concurrent-collections.md) | Concurrent* 类型 | 线程模型见 009 |
| [025 网络协议层](025-networking.md) | Http/Tcp/WebSocket、QUIC | P2P 见 042 |
| [026 加密与安全](026-cryptography-security.md) | Hash/HMAC/CSPRNG、TLS 1.3、X.509 | 网络见 025 |
| [027 本地化与资源](027-localization-resources.md) | ResX CodeGen 强类型访问器、文化感知格式化 | — |
| [028 类型反射面](028-type-reflection.md) | typeof/Type 用户面、反射元数据消费 | 元数据发射见 018 |
| [029 图像与图形](029-imaging-graphics.md) | Arc.Drawing、二维码、条码 | 渲染后端见 037 |
| [030 Protobuf 二进制序列化](030-protobuf.md) | `.proto` 契约、编解码、传输 | 文本序列化见 022 |

## IV · 工具链（031–036）

| 编号 | 主题 | 边界（不在此篇） |
|------|------|----------------|
| [031 编译器 CLI 与构建](031-compiler-cli.md) | `arc` 命令、源码打包构建、产物、环境变量清单 | 产物格式见 017；SDK 布局见 [017 references](017-build-artifacts-packages/references/index.md) |
| [032 质检框架 QIF](032-qif.md) | `arc test`、Assert、Fact/Theory、七层 | 编译器管线见 013 |
| [033 LSP 服务化](033-lsp.md) | arc-server、workspace/symbol、语义索引 | `.arcgr` 见 034 |
| [034 AI 原生工具链与 .arcgr](034-ai-toolchain-arcgr.md) | `.arcgr` 语义产物、AI 工具链 | LSP 见 033 |
| [035 调试器与 MIR 解释器](035-debugger.md) | DAP、异步栈重建、MIR 解释器 | 编译管线见 013 |
| [036 成熟度与基础面稳定](036-maturity.md) | 三硬要求、基础面冻结、宣称纪律 | 具体性能门禁见 013 |

## V · 领域库（037–043）

| 编号 | 主题 | 边界（不在此篇） |
|------|------|----------------|
| [037 UI 声明式框架](037-ui.md) | ARML、渲染、虚拟化、数据驱动、自适应、自定义字体最小面 | 语言级泛型/表达式树见 011/012；字体细节见 [037-ui/references/custom-fonts](037-ui/references/custom-fonts.md) |
| [038 AI 宿主](038-ai-host.md) | 会话、工具、HITL、Wiki、CodeAct、MCP；**AIPlan/PlanGate**；**冲突织物**（Coordinator 升维）；**小模型能力调用**（统一门面 `AIModels` 直调 + Agent 会话内可选集成） | 推理见 041；Harness/AIRfc 见 043 |
| [039 ORM 与 SQL 翻译](039-orm.md) | 表达式树翻译、方言 Provider、实体物化 | 表达式树机制见 011 |
| [040 Web 框架与 SSR](040-web.md) | WebApplication、IMediator、路由、SSR | 网络协议层见 025；SSR 插槽见 [040 references](040-web/references/index.md) |
| [041 AI 推理](041-ai-inference.md) | `Arc.AI` 张量/IAIModel、Onnx/Iree 后端；**小模型基础设施**（AIModelRegistry/AIModelService/统一门面 `AIModels` 与域子面；请求/响应模型 OpenAI 协议对齐；**流式契约** TTS/ASR §7.9 sink 回调） | Agent 宿主见 038 |
| [042 P2P 网络](042-p2p.md) | `Arc.Net.P2P` 传输/协商/DHT/NAT/中继/PubSub | 协议层见 025 |
| [043 Coding Agent Harness 工程](043-harness.md) | Harness 基座 + Coding；**AIRfc**；双环/DoD；复用 AIPlan；冲突织物消费约定 | 宿主/Plan/租约见 038；[conflict-fabric](043-harness/references/conflict-fabric.md)；代码图见 034；QIF 见 032 |

## VI · 语言扩展（044+）

| 编号 | 主题 | 边界（不在此篇） |
|------|------|----------------|
| [044 yield 迭代器](044-yield-iterators.md) | `yield return`/`yield break`、迭代器方法状态机合成、同步/异步序列单一惯用法 | 集合接口契约见 007/021；async 状态机见 009 |
| [045 插件内核](045-chord.md) | `Arc.Chord`：Context/Scope、可逆副作用账本、动态服务与反应式注入、贡献点（D11）与依赖声明（D12）、事件与瀑布（D5.1）、副作用事务、热替换（含 D8.1 二进制组合契约：换代门禁/回滚映射/拓扑序卸载） | 二进制热卸载见 017；跨库显式静态注册见 012/037；DI 容器见 023 |
| [046 通道——多生产者/多消费者通信](046-channels.md) | `Arc.Threading.Channels`：Channel/Reader/Writer 契约、工厂枢纽、四种背压模式、完成信号、协作取消与流式消费 | 线程模型见 009；同步阻塞面见 024；单消费者推拉适配见 008 |
| [047 透明对象图迁移](047-object-graph-migration.md) | 热重载 L3：`rt_arc_retype` 头重绑原语（重绑不改地址不变量）、vtable 形状+字段指纹双重判定、walk 复用枚举、收集器交互、迁移编排与回滚 | 二进制热卸载见 017；组合契约见 045 D8.1；内存模型见 005 |
| [048 命名管道与本机 IPC](048-named-pipes.md) | 跨平台硬要求：`rt_pipe_*` 双后端（Windows named pipe / POSIX FIFO，语义收敛契约+名字规范化+双平台验证矩阵）、`Arc.Net.Pipes` 门面（NamedPipeServer/ClientStream : Stream）；字节流单一惯用法、与 Channels 分层关系、四期里程碑（M2 异步面前置 accept-null 债务回归门） | 传输面家族见 025；Reactor 见 009；ABI 约定见 014；进程内 MPMC 见 046 |
| [049 Illusory 游戏引擎](049-illusory-engine.md) | VR 引擎：Actor+Component 对象模型、async 行为+`BehaviorRunner`（固定步长驱动）、`World`+`SimulationTick` 确定性仿真核心；VR 输入语义化/网络预测预留；std/Illusory/Core → `Arc.Illusory` 映射 | 异步见 009；渲染托底见 037；DB/internal 边界见 020/023；对象模型见 006 |
| [050 统一对象头](050-unified-object-header.md) | runtime 句柄内存身份物理化：`{magic, kind, ...}` 16/24B 头 + `rt_arc_inc/dec` 三层守卫（下界哨兵/magic/kind）+ 模式 A 创建点宏化迁移；豁免清单降级为优化语义，逐案判定破洞（Nested 泛型/泛型 async 参数）物理封死；M-a/b/c 分期与回归红线 | ARC 见 005；冻结面流程见 036；模式全量归因见 stability review (internal record) |

---

[返回全书目录](../SUMMARY.md) · [白皮书](../white-paper/index.md) · [用户手册](../user-guide/index.md)
