# 03 架构总览

Arc 是独立的系统级编程语言，拥有自己的词法、语法、类型系统、内存模型与运行时 ABI。当前编译器以 Rust 实现，仅为引导（bootstrap）手段；长期目标为自托管——最终用 Arc 源码实现编译器。

## 编译器管线

编译器以单向 DAG 组织，无循环依赖、无跨层跳跃：

```
ast → parse → hir → typeck → mir → codegen → arc
```

| 阶段 | 职责 |
|------|------|
| `parse` | 词法 + 语法：源码 → AST |
| `hir` | 高级 IR：AST → HIR，命名解析、符号表、Query 脱糖 |
| `typeck` | 类型检查、泛型单态化、宏展开、借用检查 |
| `mir` | 中级 IR：typed HIR → MIR CFG |
| `codegen` | 代码生成：MIR → LLVM IR → 目标文件 |
| `arc` | CLI 编排与链接 |

**后端**：LLVM IR 文本后端是唯一代码生成路径；clang 将 `.ll` 编译为目标文件，与运行时对象链接为原生二进制或动态库。Arc 是原生 LLVM 语言，无其他备选后端。

**编译顺序**：Query 脱糖 → 词法/语法 → HIR → typeck Pass（宏容器跳过方法体）→ 宏展开 → 完整 typeck → 借用检查 → MIR → 发布裁剪 → 代码生成 → 链接。

**CLI 子命令**：`arc parse`（AST 打印）、`arc check`（typeck + borrowck）、`arc build`（完整管线 + 链接）、`arc run`（build + 执行）。

**确定性**：相同输入（源码、flags、target、工具链版本）产生相同 MIR 与等价二进制；codegen 禁止非确定性随机或时间依赖。

## 运行时 ABI

运行时以纯 C 实现（`runtime/`），通过 `rt_*` 符号面与编译器对接：

- **内存管理**：`struct` 栈上分配按值移动；`class` 堆上分配、由引用计数（ARC）管理；可选循环收集器兜底
- **Task/异步**：异步函数编译为显式状态机，配合事件循环（EventLoop）调度
- **集合/字符串/IO**：`rt_list_*`、`rt_dict_*`、`rt_str_*`、`rt_file_*` 等基础原语
- **平台能力**：平台相关能力在运行时 `platform/` 与能力系统中声明

## 标准库分层

标准库以 Arc 源码组织于 `std/`，按 C# 命名空间惯例划分。编译器不内嵌 std 实现；用户程序通过 `using` 显式引用。

| 层 | 命名空间 | 内容 |
|----|----------|------|
| 根 | `Arc` | `Console`、`Math`、`Array`、`Task`、`EventLoop`、`Tensor` |
| 核心 | `Arc.Collections` / `Arc.IO` / `Arc.Linq` / `Arc.Text` | 集合、文件 IO、查询、文本与序列化 |
| 独立子库 | `Arc.Data` / `Arc.Diagnostics` | 数据库基础设施、诊断 |
| 领域 | `Arc.Orm` / `Arc.UI` / `Arc.Net` / `Arc.Agent` / `Arc.AI` / `Arc.Drawing` | ORM、UI、网络、AI 宿主（`Arc.Agent`）、AI 推理（`Arc.AI`）、图像 |

**架构红线**：编译器核心（7 个核心 crate）禁止包含任何领域能力（SQL/ORM/JSON 等翻译逻辑）。领域翻译由 std 库以 Arc 语言实现（如 `SqlTranslator.as`），编译器仅提供通用机制（表达式树构建、类型检查、代码生成）。

## 与智能时代的协作

Arc 面向人机协作，产出面向智能体的语义产物：

- **`.arcgr`**：语义索引格式，供 AI 工具链原生消费结构化语义
- **`.ani`**：声明式 FFI 契约，AI 生成 FFI 调用时可在编译期验证符号可靠性
- **结构化诊断**：编译器输出机器可读的精确错误，附带修复建议

---

上一节：[02 设计哲学](02-philosophy.md) · 下一节：[04 差异化价值](04-differentiation.md)