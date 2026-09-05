# 11 编译模型

Arc 编译器以 Rust workspace 组织，CLI 入口为 `crates/arc`。管线纯 AOT：单次编译产出目标平台原生二进制。

## Arc 是独立语言，非 Rust DSL

Arc 拥有自己的词法、语法、类型系统、内存模型与运行时 ABI（见[第二至十章](03-encoding-standard.md)）。当前编译器以 **Rust 实现**，仅为 **Phase 0 自举（bootstrap）** 手段：用宿主语言快速验证语义与管线，**不是**将 Arc 定义为 Rust 宏、属性或内嵌 DSL。

**长期目标：自托管（self-hosting）**——编译器最终用 Arc 源码实现，由当前 Rust 编译器编译出下一代 `arc`。各编译阶段 crate 保持窄接口与单向依赖，便于逐阶段用 Arc 重写而不改变语言语义。

| 阶段 | 编译器实现 | 说明 |
|------|------------|------|
| Phase 0（当前） | Rust `crates/*` | 引导实现；验证规范与集成测试 |
| Phase 1+ | Arc 源码 `compiler/`（规划） | 自托管；Rust 编译器仅作 bootstrap 链一环 |

自托管未达成前，文档与 RFC 仍为语言唯一权威；Rust 实现不得引入文档未描述的行为。

## Crate 依赖方向

编译器核心 DAG：

```
ast → parse → hir → typeck → mir → codegen → arc
                                            ↗
                                 arc-tests ──
```

辅助 crate（非核心管线阶段，由 `arc` / 工具链消费）：

| Crate | 职责 |
|-------|------|
| `reachability` | 发布裁剪用入口可达性分析 |
| `arcgr` | `.arcgr` 语义图格式 |
| `arc-server` / `arc-ui` | LSP / UI 工具面（规划中） |
| `runtime/` | 纯 C ABI 资源（无 `.rs`） |

禁止循环依赖与跨层跳跃（如 `parse → typeck`）；各 crate 仅暴露窄 public API。

## 编译阶段

`arc::pipeline::prepare_compilation` 编排顺序如下：

| 顺序 | 阶段 | Crate | 输入 → 输出 |
|------|------|-------|-------------|
| 1 | Query 脱糖 | `hir` | AST Query 节点 → 方法调用（`hir::desugar_program`） |
| 2 | 词法 + 语法 | `parse` | 源码 → AST |
| 3 | 高级 IR | `hir` | AST → HIR（命名解析、符号表） |
| 4 | 类型检查 Pass 2 | `typeck` | HIR → `TypedFn[]`（骨架；宏容器跳过方法体） |
| 5 | 宏展开 Pass 3 | `typeck` | `run_pass3`：M4 splice + M5 Source Generator（无宏时 no-op） |
| 6 | 类型检查 Pass 4 | `typeck` | `run_pass4`：宏容器与生成代码完整 typeck |
| 7 | 借用检查 | `typeck`（`borrow/`） | 检查 `TypedFn.body`（typed HIR），**非** MIR CFG |
| 8 | 中级 IR | `mir` | `TypedFn[]` → `MirCfgBody`（对外唯一形；内部 scratch 展平 If/While；region 语句保留） |
| 9 | 发布裁剪 | `arc` + `reachability` | 过滤不可达 MIR 函数 |
| 10 | 代码生成 | `codegen` | MIR → LLVM IR 文本（`.ll`）→ clang 目标文件 |
| 11 | 链接 | CLI + 系统链接器 | 对象 + `runtime.o` + native 库 |

表达式树（`Expression<T>`）在 typeck / mir 中作为 **IR 节点**处理，不含 SQL 等 Provider 领域翻译（领域逻辑在 `std/`）。

## CLI 子命令与阶段

| 命令 | 停止阶段 |
|------|----------|
| `arc parse` | AST 打印 |
| `arc check` | typeck Pass 2–4 + borrowck |
| `arc build` | 完整管线 + 链接 |
| `arc run` | build + 执行 |

实现见 `crates/arc/src/main.rs` 与 `crates/arc/src/pipeline.rs`。

## 后端

`codegen` 使用 **LLVM IR 文本后端**（`llvm_ir/`）作为唯一代码生成路径：

- MIR → LLVM IR 文本（`.ll`），由 `codegen::compile_module` / `compile_module_to_object` 等完成
- clang 将 `.ll` 编译为目标文件，与 `runtime.o` 链接为原生二进制或动态库

Arc 是原生 LLVM 语言，不包含 C 后端或其他备选后端。

## 交叉编译

`arc build -r <RUNTIME>`（亦可用 `--target`）指定目标运行时标识（如 `x86_64-unknown-linux-gnu`）。`crates/arc/src/target.rs` 解析 host 与 target 差异。

**现状**：交叉编译管线 **未实现**；当前以 host 桌面三元组为主。`wasm32-unknown-unknown` / `wasm32-wasip*` 须 **硬错误**「未实现」，禁止 silent 当 native 编译。WASM 链接 **须** runtime 子集且 **无** `platform.o`。

**平台能力边界（1.0）**：zero-cost 异常处理（`try/catch`）当前仅在 **Windows 目标**实现（SEH 面）。非 Windows 目标（Linux/macOS 等 POSIX）上，可达函数含 `try/catch` 时编译报 `arc-eh-001` 硬错误（POSIX Itanium 面属里程碑⑨ / 1.1+，见 [RFC 010](../rfc/010-exceptions-resources.md)）——禁止按 Windows 语义静默误编或降级为 panic。`try/finally`（无 catch）与 `throw` 在非 Windows 目标走内联 finally 链与 `rt_panic`，不受本门限制。

## 诊断

各阶段错误统一经 `codespan-reporting` 渲染，带文件名、行号与标签。

## 分层测试

`crates/arc-tests` 承载端到端回归测试，分两层：**L1 进程内快测**——`arc::compile_file` 进程内编译（`assert_compiles` / `assert_rejected`，默认 `cargo test`），覆盖基础语法与 OOP、语义特性、控制流与异步、数据类型、类型系统、模块与 IO、查询路径（Queryable Phase A：`expression` 拒绝 + `Expression<Func>` compile/run）等，含 check 路径 + MIR 快照断言；**L2 批量运行时**——`build_and_run_batch` 编译一次运行一次，`--features full-rt` 门控。测试为文档与实现一致性的回归屏障。

## 确定性要求

相同输入（源码、flags、target、工具链版本）应产生相同 MIR 与等价二进制。禁止在 codegen 引入非确定性随机或时间依赖。

---

上一节：[10 表达式树](10-expression-trees.md) · 下一节：[12 运行时 ABI](12-runtime-abi.md)