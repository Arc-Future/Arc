# Arc 语言之书

[前言](preface.md) · [语言宣言](manifesto.md) · 实现规划

---

## 白皮书

[白皮书目录](white-paper/index.md)

- [01 语言定位](white-paper/01-positioning.md)
- [02 设计哲学](white-paper/02-philosophy.md)
- [03 架构总览](white-paper/03-architecture.md)
- [04 差异化价值](white-paper/04-differentiation.md)

## 用户手册

[用户手册目录](user-guide/index.md)

### 快速上手

- [01 安装与快速开始](user-guide/01-getting-started.md)
- [02 构建与运行](user-guide/02-build-run.md)

### 语言规范

- [03 编码与语法标准](user-guide/03-encoding-standard.md)
- [04 词法与语法](user-guide/04-lexicon-syntax.md)
- [05 类型系统](user-guide/05-type-system.md)
- [06 内存与资源](user-guide/06-memory-resources.md)
- [07 对象模型](user-guide/07-object-model.md)
- [08 异步与任务](user-guide/08-async-tasks.md)
- [09 查询语言](user-guide/09-query-language.md)
- [10 表达式树](user-guide/10-expression-trees.md)

### 编译与运行时

- [11 编译模型](user-guide/11-compilation-model.md)
- [12 运行时 ABI](user-guide/12-runtime-abi.md)
- [13 标准库架构](user-guide/13-standard-library.md)

### 人机协作

- [14 结构化诊断](user-guide/14-structured-diagnostics.md)
- [15 能力系统](user-guide/15-capability-system.md)

### 工具链

- [16 编译器 CLI](user-guide/16-compiler-cli.md)
- [17 arc.toml 项目清单](user-guide/17-arc-toml-reference.md)
- [18 Native 组件集成](user-guide/18-native-integration-guide.md)
- [19 热重载编排指南](user-guide/19-hot-reload-guide.md)

### 附录

- [附录 A 术语表](user-guide/appendix-glossary.md)
- [附录 B 符号约定](user-guide/appendix-notation.md)

## 领域库

[领域库目录](domain/index.md)

- [Arc.UI 用户界面](domain/ui.md)
- [Arc.Agent AI 宿主](domain/ai-host.md)
- [Arc.AI 推理](domain/ai-inference.md)
- [Arc.Orm 数据访问](domain/orm.md)
- [Arc.Web 网络应用](domain/web.md)
- [Arc.Net 网络与 P2P](domain/networking-p2p.md)
- [Arc.DI 依赖注入](domain/di.md)
- [Arc.Chord 插件内核](domain/chord.md)
- [Arc.UI 拟真引擎](domain/realism-engine.md)

## RFC 设计决策

[RFC 索引](rfc/index.md)

### 语言核心（001–012）

- [001 语言宪章](rfc/001-language-charter.md)
- [002 语法表面与编码标准](rfc/002-surface-contract.md)
- [003 词法与语法](rfc/003-lexicon-syntax.md)
- [004 类型系统](rfc/004-type-system.md)
- [005 内存模型与资源安全](rfc/005-memory-model.md)
- [006 对象模型](rfc/006-object-model.md)
- [007 集合、字符串与数值](rfc/007-collections-strings-numerics.md)
- [008 委托、闭包与方法组](rfc/008-delegates-closures.md)
- [009 异步与并发模型](rfc/009-async-concurrency.md)
- [010 异常与资源管理](rfc/010-exceptions-resources.md)
- [011 表达式树与查询语言](rfc/011-expression-trees-query.md)
- [012 编译期元编程](rfc/012-compile-time-metaprogramming.md)

### 编译与运行时（013–019）

- [013 编译管线架构](rfc/013-compiler-pipeline.md)
- [014 运行时 ABI](rfc/014-runtime-abi.md)
- [015 LLVM 原生后端](rfc/015-llvm-backend.md)
- [016 验证式 FFI 与 Native 加载](rfc/016-verified-ffi.md)
- [017 编译产物、包体系与类型身份](rfc/017-build-artifacts-packages.md)
- [018 类型体系与反射元数据](rfc/018-type-reflection-metadata.md)
- [019 自举路线图](rfc/019-self-hosting.md)

### 标准库（020–030）

- [020 标准库架构与拆分](rfc/020-std-architecture.md)
- [021 集合、IO 与文本](rfc/021-collections-io-text.md)
- [022 异步任务与 LINQ/序列化](rfc/022-async-linq-serialization.md)
- [023 数学、张量与依赖注入](rfc/023-math-tensor-di.md)
- [024 并发集合](rfc/024-concurrent-collections.md)
- [025 网络协议层](rfc/025-networking.md)
- [026 加密与安全](rfc/026-cryptography-security.md)
- [027 本地化与资源](rfc/027-localization-resources.md)
- [028 类型反射面](rfc/028-type-reflection.md)
- [029 图像与图形](rfc/029-imaging-graphics.md)
- [030 Protobuf 二进制序列化](rfc/030-protobuf.md)

### 工具链（031–036）

- [031 编译器 CLI 与构建](rfc/031-compiler-cli.md)
- [032 质检框架 QIF](rfc/032-qif.md)
- [033 LSP 服务化](rfc/033-lsp.md)
- [034 AI 原生工具链与 .arcgr](rfc/034-ai-toolchain-arcgr.md)
- [035 调试器与 MIR 解释器](rfc/035-debugger.md)
- [036 成熟度与基础面稳定](rfc/036-maturity.md)

### 领域库（037–043）

- [037 UI 声明式框架](rfc/037-ui.md)
- [038 AI 宿主](rfc/038-ai-host.md)
- [039 ORM 与 SQL 翻译](rfc/039-orm.md)
- [040 Web 框架与 SSR](rfc/040-web.md)
- [041 AI 推理](rfc/041-ai-inference.md)
- [042 P2P 网络](rfc/042-p2p.md)
- [043 Coding Agent Harness 工程](rfc/043-harness.md)
  - [渐进式披露子项索引](rfc/043-harness/references/index.md)
  - [AIRfc 体系](rfc/043-harness/references/airfc.md)
  - [LLM 门闩](rfc/043-harness/references/llm-gates.md)
  - [冲突织物](rfc/043-harness/references/conflict-fabric.md)
  - [API 草图](rfc/043-harness/references/api-sketch.md)
  - [包布局](rfc/043-harness/references/package-layout.md)
  - [收敛迁移](rfc/043-harness/references/convergence-migration.md)
  - [可执行 DoD](rfc/043-harness/references/definition-of-done.md)
- [044 yield 迭代器](rfc/044-yield-iterators.md)
- [045 插件内核](rfc/045-chord.md)
- [046 通道——多生产者/多消费者通信](rfc/046-channels.md)
- [048 命名管道与本机 IPC](rfc/048-named-pipes.md)
- [050 统一对象头](rfc/050-unified-object-header.md)
