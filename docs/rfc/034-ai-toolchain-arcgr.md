# RFC 034 AI 原生工具链与 .arcgr

## 背景

AI 优先裁决从语言层延伸到**产物层**与**工具链层**（信条「为人机共写而生」）。四立场：

| 立场 | 内容 |
|------|------|
| 编译器是 AI 语义后端 | 编译器产出语义，AI 工具链消费语义，不自行猜测 |
| 产物即接口 | `.arcgr` / JSON 诊断是可被机器消费的公开接口 |
| Token 是一等公民 | 语义产物面向 token 预算设计，渐进披露避免一次性灌满 |
| 渐进式披露 | 信息按 L0–L4 分层，按需浮现（见 §5） |

`.arcgr` 是 Arc 的**语义索引产物**，由 `crates/arcgr` 生成（`arc inspect` 从**源码**直接产出，随源码仓库分发，不随二进制分发；见 [031](031-compiler-cli.md)），供 AI 工具链、LSP（见 [033](033-lsp.md)）与包消费方读取。

## 设计决策

### 1. `.arcgr` 语义产物

`.arcgr` 记录源码的语义图，核心为符号表与引用表，并输出引用图（`reference_graph`）。

| 组件 | 内容 |
|------|------|
| `SymbolTable` | 符号声明集合（类型、方法、字段、参数等），含类型与 span |
| `ReferenceTable` | 符号引用集合（每次引用的目标符号与位置） |
| `reference_graph` | 引用图输出（四项：符号→定义、引用→符号、符号→引用、跨文件引用聚合等） |
| protobuf | `.arcgr` 以 protobuf 编码，机器可读可校验 |

`.arcgr` 由 `arcgr` crate 在 typeck 之后收集（`collect_arcgr_file`），与 `arc build` 共享同一语义源，保证产物与编译器语义零分歧。产物内嵌服务期信息（`.xml` 文档注释）供消费方使用。

### 2. `arc inspect` 命令

`arc inspect <file> [--format human|json] [--emit PATH]` 输出语义索引可达性摘要（源码模式：运行 parse → hir → typeck → `collect_arcgr_file`）。

```bash
arc inspect examples/CompilerSmoke/Program.as                    # human 摘要
arc inspect examples/CompilerSmoke/Program.as --format json --emit hello.arcgr
```

`--format json` 输出结构化 JSON；`--emit PATH` 将 `.arcgr` 落盘。

### 3. 语义查询层

在 `.arcgr` 之上提供语义查询，供 AI 与开发者按需取语义：

| 查询 | 语义 |
|------|------|
| 定位（locate） | 符号定位到声明 |
| 解释（explain） | 符号/表达式的类型与语义解释 |
| 查询（query） | 按谓词检索符号/引用 |
| 上下文清单（overview） | 项目语义上下文摘要（L0/L1 渐进披露） |

查询均支持 `--format json`，与 `arc inspect` 共享同一 `.arcgr` 数据底座。

### 4. JSON 诊断

诊断输出支持 `--message-format json`：parse / typeck / borrowck 诊断序列化为结构化 JSON，与 LSP `publishDiagnostics` 同源（见 [033](033-lsp.md)）。AI 工具链据此解析错误、定位 span、生成修复建议。结构化诊断口径见 [user-guide 14 结构化诊断](../user-guide/14-structured-diagnostics.md)「机器可读输出」。

### 5. 渐进式披露（L0–L4）

| 层 | 内容 | 消费 |
|----|------|------|
| L0 | 项目级上下文清单（overview）：结构概览、符号统计 | 上下文预载 |
| L1 | 符号级语义（SymbolTable / ReferenceTable 子集） | 定位/引用 |
| L2 | 引用图与分析 | 影响面推理 |
| L3 | 证据包（span、AST 片段） | 修复建议 |
| L4 | 全量语义（完整 `.arcgr`） | 深度分析 |

L0/L1 为默认披露面；更高层按需经 `--emit` 落盘或显式查询触达。

### 6. 拒绝项

| 项 | 裁决 |
|----|------|
| 语义产物含运行时反射 | 拒绝——`.arcgr` 为编译期静态语义，不含反射元数据 |
| 产物与编译器语义分叉 | 拒绝——`.arcgr` 由编译器收集，非独立生成 |
| IntentMeta（如 `[HotPath]`/`[Stable]` 标注） | 不在本设计面内 |
| 证据包 / Span-patch / `arc fix` / lint | 不在本设计面内 |

## 边界

- **LSP 服务**（消费 `.arcgr` 的语义 provider）见 [033](033-lsp.md)；本 RFC 只讲 `.arcgr` 产物与 AI/CLI 消费。
- **调试器**（`ArcDebuggerContext` 引用 `.arcgr` SymbolTable）见 [035](035-debugger.md)。
- **`.arcgr` 打包分发**见 [031](031-compiler-cli.md) 与产物格式 [017](017-build-artifacts-packages.md)。

---
上一节：[033 LSP 服务化](033-lsp.md) · 下一节：[035 调试器与 MIR 解释器](035-debugger.md)