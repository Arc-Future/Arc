# 前言

欢迎阅读 **《Arc 语言之书》**——Arc 语言的权威参考。Arc 是面向人机协作时代的**纯 AOT 编译型**系统级编程语言：强类型、**C# 惯用表面语法**，核心抽象（类型、LINQ、`Expression<T>`、编译期元编程）在**编译期与链接期**完成，而非运行时解释。

## 本书读者

本书服务两类读者，他们常常阅读相同页面，但目的不同：

**语言使用者**希望理解 Arc 是什么、为何存在、如何编写正确程序。请从[白皮书](white-paper/index.md)出发，继而阅读[用户手册](user-guide/index.md)的[语言规范](user-guide/05-type-system.md)与[编译与运行时](user-guide/11-compilation-model.md)。

**编译器开发者**需要了解源码如何变为原生二进制、运行时 ABI 如何约定、诊断如何结构化输出。请重点阅读[编译模型](user-guide/11-compilation-model.md)、[运行时 ABI](user-guide/12-runtime-abi.md)与[工具链](user-guide/16-compiler-cli.md)。

两类读者均可查阅[领域库](domain/index.md)与[RFC 设计决策](rfc/index.md)。

## 如何阅读

本书按渐进式披露组织，章节层层递进，也可按需跳转：

1. **白皮书** — Arc 定位、哲学、架构与差异化
2. **用户手册** — 快速上手、语言规范、编译与运行时、工具链
3. **领域库** — UI / AI / ORM / Web / 网络 / DI 的使用手册
4. **RFC** — 已接受的设计决策

## 约定

| 约定 | 含义 |
|------|------|
| `.as` | Arc 源文件扩展名 |
| `arc` | AOT 编译器 CLI（`crates/arc`） |
| `void` | 无返回值类型 |
| `Task<T>` | 异步计算句柄 |
| `Expression<T>` | 表达式树类型 |
| 等宽字体 | 源码、CLI 参数、路径、运行时符号 |

代码块尽量完整可编译。仓库路径：编译器在 `crates/`，标准库在 `std/`。

## 文档与实现

编译器管线（parse → HIR → typeck → borrowck → MIR → codegen）已在 `crates/arc-tests` 分层验证（L1 进程内编译快测 / L2 批量运行时 `full-rt` 门控）。文档与实现冲突时，以对应主题的**已接受 RFC** 为准。

**开发过程必须严格参照本书**；变更语言行为时，文档与代码在同一变更中更新。

当前编译器为 **Rust bootstrap 实现**（`crates/*`）。Arc 是独立语言，非 Rust DSL；长期愿景为用 Arc 源码实现编译器（[自举](rfc/019-self-hosting.md)）。

---

下一节：[语言宣言](manifesto.md) · 或进入[白皮书](white-paper/index.md)