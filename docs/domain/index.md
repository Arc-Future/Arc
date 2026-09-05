# Arc 领域库

本分册是 Arc **领域库**的使用者权威参考：以「如何开发」为中心，面向使用 Arc 构建应用的开发者。与 `docs/rfc/` 的领域 RFC 分工——RFC 讲「设计决策」，本分册讲「如何使用」。每一篇均提供可运行的示例代码（`.as`/`.arml`）、核心 API 表格与边界说明。

## 领域库总览

Arc 领域库在语言核心与标准库基础面之上，按单一惯用法平级组织为若干独立子库。每个子库一个顶层命名空间，彼此依赖单向、职责清晰。

| 领域库 | 命名空间 | 目录 | 职责 | 分册 |
|--------|----------|------|------|------|
| 声明式界面 | `Arc.UI` | `std/UI/Core/` | ARML 标记、数据绑定、主题、虚拟化、CodeEditor | [ui.md](ui.md) |
| AI 宿主 | `Arc.Agent` | `std/AI/Agent/` | 会话、`[AITool]` 工具、HITL、Wiki、MCP、CodeAct、AIPlan | [ai-host.md](ai-host.md) |
| Agent Harness | `Arc.Agent.Harness` / `.Coding` | `std/AI/Agent.Harness/` | **AIRfc** 小型 PM；Coding 领域质量门（RFC 043） | [ai-host.md](ai-host.md)（§Harness） |
| AI 推理 | `Arc.AI` | `std/AI/` | `Tensor`、`IAIModel`、Onnx/Iree 推理后端 | [ai-inference.md](ai-inference.md) |
| 对象关系映射 | `Arc.Orm` | `std/Orm/` | `DbContext`、实体、查询翻译、SQLite 方言 | [orm.md](orm.md) |
| Web 框架 | `Arc.Web` | `std/Web/` | `WebApplication`、IMediator、特性路由、SSR、gRPC | [web.md](web.md) |
| 网络与 P2P | `Arc.Net` | `std/Net/`、`std/Net.P2P/` | HttpClient、WebSocket、TCP/UDP、P2P | [networking-p2p.md](networking-p2p.md) |
| 依赖注入 | `Arc.DI` | `std/DI/` | 服务注册、解析、生命周期、作用域 | [di.md](di.md) |
| 插件内核 | `Arc.Chord` | `std/Chord/` | Context/Scope、可逆副作用、动态服务与反应式注入、贡献点与依赖声明、事件与瀑布、副作用事务、热替换 | [chord.md](chord.md) |
| 拟真引擎 | 规划中 | 规划中 | 程序化生成有机自然 3D 模型（生成层·资产层·承载渲染层） | [realism-engine.md](realism-engine.md) |

## 阅读顺序

建议按如下顺序阅读，先建立宿主装配（DI、Web、AI 宿主）再深入各能力面：

1. [ui.md](ui.md) —— 界面层
2. [ai-host.md](ai-host.md) —— AI 宿主
3. [ai-inference.md](ai-inference.md) —— 推理引擎
4. [orm.md](orm.md) —— 数据访问
5. [web.md](web.md) —— Web 应用
6. [networking-p2p.md](networking-p2p.md) —— 网络编程
7. [di.md](di.md) —— 依赖注入
8. [chord.md](chord.md) —— 插件内核

## 与其它文档的分工

- **设计决策**：各领域库的取舍与架构详见 `docs/rfc/` 的领域 RFC（UI、AI 宿主、ORM、Web、AI 推理、P2P 等分册）。
- **语言与基础库**：语言级泛型、表达式树、可空与绑定机制、`Arc.Collections`/`Arc.IO`/`Arc.Text` 等基础面见[标准库架构](../user-guide/13-standard-library.md)与[语言规范](../user-guide/index.md)。
- **协议层**：HTTP/TCP/WebSocket/QUIC 的协议级细节见 `docs/rfc/025-networking.md`；P2P 见 `docs/rfc/042-p2p.md`；本分册只讲开发者在 `Arc.Net` 之上的使用方式。

## 通用开发约定

所有领域库遵循 Arc 的统一约定：

- **异步优先**：异步方法一律 `*Async` 后缀并接受 `CancellationToken`；不提供同步孪生。
- **单一惯用法**：同一意图只有一条正道，禁止多轨 API。
- **显式 > 隐式**：可空类型显式 `?` 标注并妥善空判；报错优于静默推断。
- **编译期校验**：绑定路径、路由模板、表达式树在编译期对照类型解析，绑错即报错。
