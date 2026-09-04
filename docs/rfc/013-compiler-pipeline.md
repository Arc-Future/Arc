# RFC 013 编译管线架构

> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令与 `crates/arc-integration/tests/` 路径
> 不再可用；现行验证矩阵为 `cargo test --workspace`（运行时面
> `cargo test -p arc-tests --features full-rt`），详见仓库根 `CHANGELOG.md`。

## 背景

Arc 是独立语言，非 Rust DSL——拥有自己的词法、语法、类型系统、内存模型与运行时 ABI。当前编译器以 **Rust workspace** 实现，CLI 入口为 `crates/arc`。管线为**纯 AOT**：单次编译产出目标平台原生二进制，无 JIT 预热。

编译器核心按 `ast → parse → hir → typeck → mir → codegen → arc` 组织为**单向管线**。LLVM IR 文本是编译器的**规范输出**，LLVM 工具链（clang/lld/lldb）负责任何优化、代码生成与调试。Arc 是**原生 LLVM 语言**，不包含 C 后端、Backend trait 抽象层、`--backend` CLI flag 或任何备选后端。

## 设计决策

### Crate 依赖方向

编译核心 DAG（单向、不可逆、无循环）：

```
ast → parse → hir → typeck → mir → codegen → arc
                                  ↗
                 arc-integration ──
```

| Crate | 职责 | 关键产物 |
|-------|------|---------|
| `ast` | 语法树与基础类型 | `Expr`/`Stmt`/`Items`/`TypeId` |
| `parse` | 词法 + 语法 | `lexer`/`Parser`/`Spanned` |
| `hir` | 高层中间表示、命名解析、符号表 | `HirModule`/`lower` |
| `typeck` | 类型检查 + 借用检查 | `checker`/`borrow`/`registry` |
| `mir` | 中层 IR + 数据流 | `MirCfgBody`/`lower`/`dataflow` |
| `codegen` | LLVM IR 文本生成 | `emit_*`/`llvm_ir` |
| `arc` | CLI 驱动 | `pipeline`/`linker`/`manifest` |

辅助 crate（非核心管线阶段，由 `arc` / 工具链消费）：

| Crate | 职责 |
|-------|------|
| `reachability` | 发布裁剪用入口可达性分析 |
| `arcgr` | `.arcgr` 语义图格式 |
| `arc-server` / `arc-ui` | LSP / UI 工具面 |
| `runtime/` | 纯 C ABI 资源（无 `.rs`） |

### 架构红线

1. **单向依赖**：`ast → parse → hir → typeck → mir → codegen → arc`；禁止反向/循环/跨层跳跃（如 `parse → typeck`）。
2. **lib.rs 门面 ≤80 行**：仅 `mod` + `pub use`，实现放子模块。
3. **无领域能力**：编译器核心 7 crate **禁止包含任何领域能力**（SQL/ORM/JSON 等翻译逻辑）。领域翻译由 std 库以 Arc 语言实现（如 `std/Orm/SqlTranslator.as`）；编译器仅提供通用机制（表达式树构建、类型检查、代码生成）。表达式树（`Expression<T>`）在 typeck / mir 中作为 **IR 节点**处理，不含 Provider 领域翻译。
4. **窄 public API**：各 crate 仅暴露窄 public 接口。

### 编译阶段

`arc::pipeline::prepare_compilation` 编排如下固定顺序：

| 序 | 阶段 | Crate | 输入 → 输出 |
|----|------|-------|-------------|
| 1 | Query 脱糖 | `hir` | AST Query 节点 → 方法调用（`hir::desugar_program`） |
| 2 | 词法 + 语法 | `parse` | 源码 → AST（加载期完成） |
| 3 | 高级 IR | `hir` | AST → HIR（命名解析、符号表） |
| 4 | 类型检查 Pass 2 | `typeck` | HIR → `TypedFn[]`（骨架；宏容器跳过方法体） |
| 5 | 宏展开 Pass 3 | `typeck` | `run_pass3`：宏 splice 展开 + Source Generator（无宏时 no-op） |
| 6 | 类型检查 Pass 4 | `typeck` | `run_pass4`：宏容器与生成代码完整 typeck |
| 7 | 借用检查 | `typeck`（`borrow/`） | 检查 `TypedFn.body`（typed HIR），**非** MIR CFG |
| 8 | 中级 IR | `mir` | `TypedFn[]` → `MirCfgBody`（对外唯一形；内部 scratch 展平 If/While；region 语句保留） |
| 9 | 发布裁剪 | `arc` + `reachability` | 过滤不可达 MIR 函数 |
| 10 | 代码生成 | `codegen` | MIR → LLVM IR 文本（`.ll`）→ clang 目标文件 |
| 11 | 链接 | CLI + 系统链接器 | 对象 + `runtime.o` + native 库 |

### CLI 子命令与阶段停点

| 命令 | 停止阶段 |
|------|----------|
| `arc parse` | AST 打印 |
| `arc check` | typeck Pass 2–4 + borrowck |
| `arc build` | 完整管线 + 链接 |
| `arc run` | build + 执行 |

### 后端

`codegen` 使用 **LLVM IR 文本后端**（`llvm_ir/`）作为唯一代码生成路径：MIR → LLVM IR 文本（`.ll`），由 `codegen::compile_module` / `compile_module_to_object` 等完成；clang 将 `.ll` 编译为目标文件，与 `runtime.o` 链接为原生二进制或动态库。文件级编译经 `arc::compile_file` 的 check 路径 + MIR 快照断言提供回归屏障。跨库入口 `Assembly.Entry<T>` 的调用点拦截与类型化间接调用发射（`__arc_entry_*` 符号）见 [017 编译产物、包体系与类型身份](017-build-artifacts-packages.md)。

### 交叉编译

`arc build -r <RUNTIME>`（亦可用 `--target`）指定目标运行时标识（如 `x86_64-unknown-linux-gnu`）；`crates/arc/src/target.rs` 解析 host 与 target 差异。主机桌面三元组为主路径；`wasm32-unknown-unknown` / `wasm32-wasip*` 目标被视为**未支持目标**，编译报**硬错误**，禁止以原生方式静默编译。WASM 链接须 runtime 子集且无 `platform.o`。

### 诊断

各阶段错误统一经 `codespan-reporting` 渲染，带文件名、行号与标签（结构化诊断）。

### 确定性要求

相同输入（源码、flags、target、工具链版本）产生相同 MIR 与等价二进制。禁止在 codegen 引入非确定性随机或时间依赖。

### 测试作为回归屏障

`crates/arc-integration` 覆盖基础语法/OOP（`hello_e2e`、`oop_demo_e2e` 等）、语义特性、控制流与异步、数据类型、类型系统、模块与 IO、LINQ 查询路径，以及经 `arc::compile_file` 的 check 路径 + MIR 快照断言（`pipeline`）。宏 Pass 单元覆盖于 `typeck/tests/macro_e2e.rs`。测试是文档与实现一致性的回归屏障。

## 管线装备架构（策略模式）

### 背景

阶段 DAG（§编译阶段）固定且单向，但**横切能力**（项目解析、依赖闭包、包引用上下文、编译调度、产物发射、测试宿主）当前全部硬编码耦合于 `arc::pipeline` 巨型单体内，无 trait/SPI 扩展点。并行测试仅存在于生成的测试运行时代码（`Parallel.For`），**编译器构建链路无并行**。本节确立「管线装备架构」：主流程保持精简的段序编排，横切能力以**装备（trait）**注入，由策略模式解耦，支持并行编译与独立替换。

### 原则

1. **主流程只编排段序**：`prepare_compilation` 只驱动 `desugar → typeck → mir → reachability → codegen` 的阶段轴，不持有任何横切能力的具体实现。
2. **能力即装备（接口）**：每项横切能力是一个窄 trait，管线段（凡消费该能力处）仅依赖 trait，不依赖具体实现；默认实现装配于管线装配点（composition root）。
3. **装备正交于段**：装备是段轴之外的「正交切片」，可并行、可替换、可独立测试。
4. **禁止双轨**：新增能力必须走装备接口，`pipeline` 不得再出现硬编码 if/else 分支式的多路径。
5. **数据经由装备传递**：跨装备共享的上下文（如 `CompileUnit`、`PackageContext`）由装备产出、经主流程传递，不建立装备间的直接依赖。

### 装备清单

| # | 装备 | trait (窄接口) | 责任 | 默认实现 |
|---|------|----------------|------|----------|
| P1 | **项目管理 ProjectManager** | 枚举项目/workspace 成员、探测项目布局（`ProjectType`） | 从目录/manifest 产出待编译项目集合与构建顺序骨架 | `workspace.rs`/`scaffold.rs`；拓扑序取自 `Workspace::closure_order` |
| P2 | **依赖解析 DependencyResolver** | 由入口根解析传递依赖闭包、校验缺失边、产出构建顺序 | 包图构建与闭包计算，提供全序构建序 | `package_graph.rs`（`discover_*` / `absorb_path_dependencies`） |
| P3 | **包引用上下文 PackageContext** | 装配 `CompileUnit`：文件↔包映射、`internals_visible_to`、global usings、native 契约、外部符号 | 为每编译单元合成完整引用上下文 | `loader.rs`（`load_compile_unit`） |
| P4 | **编译调度 CompileScheduler** | 将若干编译单元按依赖/拓扑分派到串行或并行执行 | 并行编译策略；串行为隐式默认，并行经配置启用 | 串行遍历；并行为独立策略（见 §并行编译） |
| P5 | **产物发射 ArtifactEmitter** | 依 `EmitRole`（MainObject / DynamicLibrary）发射 `.ll` → 目标文件 → 链接 | 代码生成与产物收口 | `codegen::compile_module*` + `arc::linker` |
| P6 | **测试宿主 TestHost** | 合成 `__QifTestHost::Main`（Fact/Theory 收集、Order/filter、fixture、并行/串行调度） | 测试模式宿主合成 | `pipeline::generate_qif_test_main` |

**接口形式**：装备以 Rust `trait` 表达，trait 方法签名即契约；`From`/装配层默认注入具体实现。装备不得动态分派无关行为——每个 trait 只承载一个正交责任。

### 并行编译（P4）

- **语义**：并行仅作用于**相互独立的编译单元**（无依赖边的单元可并发），依赖序（P2 输出）是一致性边界——并行不得改变构建序契约。
- **默认**：串行（确定性优先，符合 RFC 013 确定性要求）。
- **启用**：经装备配置开启；并行实现复用一个 worker 池，同一编译单元内阶段仍串行（段序不变），仅单元级并发。
- **确定性**：并行下相同输入仍产出等价产物；共享状态（输出目录、命名）需经调度器协调，禁数据竞争。
- **与 QIF 的关系**：QIF 并行（`Parallel.For`）是**生成测试运行时**行为，属 P6 产物语义，与 P4 编译期并行正交，互相独立。

### 装配与解耦

- 装配点：`arc::pipeline` 以「装备束（equipment bundle）结构 + 默认构造」暴露，CLI（`main.rs`）经装配点注入默认实现。
- 装备可替换：测试用例注入替身装备（如桩 `DependencyResolver`）隔离验证主流程段序，形成单元回归屏障。
- 边界：装备接口本身属于**编译器内部 SPI**，不进 `[Builtin]` / 用户契约面；对外仍是单一 `arc build/check/test/run` 面（见 [031](031-compiler-cli.md)）。

## 边界

- 本篇只讲**编译管线与阶段职责**；LLVM 后端细节见 [015 LLVM 原生后端](015-llvm-backend.md)；语言类型见 004–012。
- 自举（用 Arc 写 Arc 编译器）见 [019 自举路线图](019-self-hosting.md)。
- 产物/包/动态库/热卸载见 [017 编译产物、包体系与类型身份](017-build-artifacts-packages.md)；CLI 命令见 031。

---
上一节：[012 编译期元编程](012-compile-time-metaprogramming.md) · 下一节：[014 运行时 ABI](014-runtime-abi.md)