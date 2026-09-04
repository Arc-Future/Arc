# RFC 035 调试器与 MIR 解释器

## 背景

Arc 原生调试器通过 DAP（Debug Adapter Protocol）与编辑器通信（信条「为人机共写而生」）。调试能力建立在前置设施之上：DWARF 5 调试信息 + `.arcdbg` 私有调试数据 + `rt_backtrace` 运行时回溯 + async 状态机 lowering。表达式求值由 **MIR 解释器**承担——它是编译器核心通用机制，直接解释执行 MIR 指令序列，无需 codegen 即时编译，保证单一语义源、零 JIT 开销、安全护栏内建。

与 lldb/gdb 的关系为**互补**而非替代：lldb/gdb 消费通用 DWARF，`arc` 调试器消费私有 `.arcdbg`（含 ARC 帧折叠与 async 栈重建信息）。

## 设计决策

### 1. DAP 能力面

调试器经 DAP 暴露以下**能力面**（设计契约，非进度记录）：

| 能力 | 承接机制 |
|------|---------|
| 断点（行 / 条件 / 函数 / logpoint） | DAP 断点事件 |
| 单步（含 step over await 跨调度器） | DAP 步进请求 + async 状态机恢复 |
| 变量查看 | DAP 变量作用域链 |
| 表达式求值（Watch / evaluate） | MIR 解释器（见 §4） |
| 调用栈（ARC 帧折叠 + async 栈重建） | `.arcdbg` 帧信息 + async 状态机 `trace_stack`（见 §3） |
| 前置设施：DWARF 5 / `.arcdbg` / `rt_backtrace` | 见 §2 |

### 2. 前置设施

| 设施 | 作用 |
|------|------|
| DWARF 5 | 通用调试信息，`arc build -g` / `arc run -g` 发射；供 lldb/gdb 通用消费 |
| `.arcdbg` | 私有调试数据：ARC 帧折叠表 + async 状态机信息 |
| `rt_backtrace` | 运行时回溯，产出原生调用链 |
| async 状态机 lowering | async `Task` 的栈帧/局部变量在状态机中的保存与恢复 |

### 3. 调用栈重建

ARC 是引用计数运行时，存在多级帧折叠（编译器内联、尾调用等），原生调用栈与语言级调用栈不一一对应。

| 机制 | 内容 |
|------|------|
| ARC 帧折叠 | 依据 `.arcdbg` 的帧信息，将原生栈折叠为语言级逻辑帧 |
| async 栈重建 | 依据 async 状态机中保存的 `trace_stack` 与局部变量，重建 await 中断后的逻辑调用栈 |

async 栈重建是调用栈能力的核心：命中 await 后断点时，通过 Task 对象的 `trace_stack` 字段回溯被挂起的 async 函数链，使开发者看到完整逻辑调用栈而非碎片化原生栈。

### 4. MIR 解释器

`crates/mir/src/interpreter.rs` 实现**栈式** MIR 解释器（非基于寄存器）。MIR 已是 SSA 形式，栈式求值最直接映射——每条指令消费栈顶 N 个值，结果 push 回栈顶，无需寄存器分配器。

```
DebuggerContext (抽象接口)
      │
      ▼
MirInterpreter::evaluate(mir, ctx) → Result<Value, EvalError>
      │
      ├── 栈式求值循环
      │    常量 · 变量读取 · 算术 · 逻辑(短路) · 比较
      │    字段访问 · 方法调用(虚分派) · 属性访问 · 闭包调用
      │    variant 匹配 · if/switch 控制流 · 类型转换 · 可空流
      │
      ├── 安全护栏检查（每 N 条指令）
      │    IO/FFI 拦截 · 内存预算 · 超时
      │
      └── 结果返回 / EvalError
```

**架构定位**：
- `crates/mir` 子模块，不拆独立 crate；仅消费 `MirExpr` 数据结构，不重复定义 MIR 指令集。
- 经 `DebuggerContext` trait 与调试器解耦——解释器不感知 DWARF / `.arcdbg` / `.arcgr` 等调试领域细节；`ArcDebuggerContext`（持有 DWARF 读取器 + `.arcdbg` 读取器 + `.arcgr` SymbolTable + Task 对象指针）在调试引擎侧实现该 trait。
- 编译器核心通用机制，不含领域能力。

**能力范围**：算术（int/long/float/double）、逻辑（短路）、比较（含 string 走 `rt_str_cmp`）、变量读取（局部/参数/闭包捕获）、字段访问（struct + class 含继承）、方法调用（静态/实例虚分派）、属性访问（getter）、闭包调用、variant 模式匹配、if/switch 表达式、类型转换、可空流（`T?`/`??`/`?.`/`!.`）。

### 5. 安全护栏

表达式求值内建四道**不可关闭**护栏——这是「显式 > 隐式」的直接体现，调试器表达式不允许无限制副作用：

| 护栏 | 机制 | 触发行为 |
|------|------|---------|
| IO 拦截 | typeck 阶段拒绝非白名单调用（`FileIO`/`Net`/`Process`） | 编译期错误 |
| FFI 拦截 | typeck 阶段拒绝 `extern` 函数调用 | 编译期错误 |
| 内存预算 | 每次堆分配前查询预算 | `EvalError::MemoryBudgetExceeded`（默认 1MB，最小 64KB 最大 16MB，不可关闭） |
| 超时中断 | 每 100 条指令检查耗时 | `EvalError::Timeout`（默认 100ms，最小 10ms 最大 1000ms） |

### 6. 与 codegen JIT 关系

表达式求值走 **MIR 解释器路径**。codegen JIT 即时编译为 native 执行作为性能优化路径评估——两者并存，由调用方按场景选择。解释器路径启动开销低、无可执行内存分配，适合低频人工触发求值。

### 7. 拒绝项

| 项 | 裁决 |
|----|------|
| 表达式求值重走独立编译路径 | 拒绝——双语义源风险；统一走 MIR 解释器，单一语义源 |
| 安全护栏可关闭 | 拒绝——IO/FFI/内存/超时四道护栏强制，不可关闭 |
| LINQ 表达式树求值 | 拒绝——归查询/表达式树翻译机制 |
| async lambda 求值 | 拒绝——归运行时调度器，非解释器职责 |
| Hot Reload / Edit-and-Continue | 不在 Arc 调试器设计面内——编译期 AOT 确定性，无运行时热替换 |
| SetVariable / completions / disassemble | 不在本设计面内 |

## 边界

- **编译管线**（MIR lowering、async 状态机生成）见 [013](013-compiler-pipeline.md)；本 RFC 只讲调试器与解释器。
- **LSP**（语义 provider）见 [033](033-lsp.md)；`.arcgr`（解释器上下文引用的 SymbolTable）见 [034](034-ai-toolchain-arcgr.md)。
- **DWARF 发射**（`-g` 标志）见 [031](031-compiler-cli.md)。

---
上一节：[034 AI 原生工具链与 .arcgr](034-ai-toolchain-arcgr.md) · 下一节：[036 成熟度与基础面稳定](036-maturity.md)