# AGENTS.md

通用 LLM 开发准则见 Cursor 内置规则。以下为 **Arc 项目**约定。

## 规则索引（`.cursor/rules/`）

| 规则 | 范围 | 要点 |
|------|------|------|
| `arc-core` | 全局（alwaysApply） | 核心契约、禁止项、开发纪律、扩展性、验证 |
| `arc-iteration` | 全局（alwaysApply） | 五步循环、当前主线状态、速记禁止、提交推送 |
| `arc-workspace-hygiene` | 全局（alwaysApply） | 工作区卫生、产物落点、脚本归属 |
| `arc-rust` | `**/*.rs` | lib.rs 禁堆码、单文件单职责、模块化拆分 |
| `arc-language` | `.as` / 前端 crate / 规范章 | C# 基准语法与命名；std 编码规范契约 |
| `arc-ui` | `std/UI/**` / `crates/runtime-ui/**` / `**/*.arml` | **最高优先级**：渲染唯一对接 wgpu，禁止软件光栅/GDI/stub 降级方案 |
| `arc-docs` | `docs/**` | 文档驱动流程、书籍结构、中文写作 |

**动手前必读：** [docs/preface.md](docs/preface.md) → [docs/SUMMARY.md](docs/SUMMARY.md)（全书目录）→ 相关 [RFC](docs/rfc/index.md) / [领域文档](docs/domain/index.md) → 对应小节。文档为唯一权威；实现与 RFC 冲突时先对齐文档。

## 当前主线与状态

权威出处：[RFC 036 成熟度](docs/rfc/036-maturity.md) · 实现规划（内部文档，未随公开仓库分发） · `arc-core` / `arc-iteration` 规则（进度状态统一收敛于此，不在本文件重复维护）。

- **里程碑**：F0–M3 ✅（资产不回滚）；**M4 可排期、未开工**（单 M · 子集边界 · 不自动开干；非现行主线全力项）。
- **成熟度宪章（RFC 036）**：三硬要求（H1 底层稳定 · H2 极致性能 · H3 高阶可编译）齐备是能力解锁前提；**基础面默认冻结**（语言核心 / `rt_*` ABI / `std/Arc` Stable，破坏性变更须先 RFC）；**宣称纪律**（未经验收协议不得宣称、验收通过后方可声明）。
- **自举（RFC 019）**：禁止删 Rust 词法/parser、禁止切默认 CLI（直至 Mn）；**不**宣称 C# 完备对等；**单一惯用法**（一语义一写法，文档不得暗示双轨）。

## 编码规范（对标 C# 优雅简洁 · 强约束 · 跨会话/多人必守）

标准库（`std/`）与所有 `.as` 代码**必须**遵循对标 C# 的优雅简洁写法，禁止退回原始/样板式写法。完整细则见 [arc-language.mdc](.cursor/rules/arc-language.mdc)。核心契约：

| # | 契约 | 要求 |
|---|------|------|
| 1 | **`this.` 成员前缀** | 公开成员（字段/属性/方法，含虚方法、静态方法调用同对象实例）访问带 `this.`；内部字段 `_field`（`_` 前缀私有/保护字段）**裸访问（无 `this.`）**，仅与参数/局部变量冲突时用 `this.` 消歧 |
| 2 | **自动属性** | getter-only `{ get; }` 默认（构造期/初值即定即只读）；外部可变才 `{ get; set; }`，类内可变用 `{ get; private set; }`、POCO 用 `{ get; init; }`。初值 `= expr` 必须携带非默认信息——等价于类型默认值（`= 0`/`= false`/`= null`）的初值是冗余噪声，一律省略。**`[Builtin]` stub 自动属性有条件启用**——codegen 拦截其 `get_X`（或静态全名直射 `rt_*`）时可用自动属性（如 `Thread.IsAlive`/`CurrentThread`）；未拦截的必须保留显式 getter 死代码体（`{ get { return 0; } }`，如 `MemoryMappedFile.Length`），否则运行时恒返回 0。判别详见 `arc-language.mdc` |
| 3 | **可空 `?` / null 防御** | 可空必须显式 `?` 标注并妥善空判；禁止不明确可空风险 |
| 4 | **异步规范** | 异步方法必须 `Async` 后缀 + 必须接受 `CancellationToken`；禁止同步 I/O 副本 |
| 5 | **控制流大括号** | `if`/`else`/`switch`/`while`/`for`/`foreach` 一律 `{}` 括起，禁止省略；**`switch` 的每个 `case`/`default` 分支体也必须 `{}` 括起**（禁止裸语句列表）；采用 **Allman 风格**（左花括号独立成行，C# 官方推荐） |
| 6 | **命名规范** | 类型/方法/属性/常量/枚举成员 **PascalCase**；参数/局部变量 **camelCase**；私有字段 **`_camelCase`**；接口 **`I` 前缀**；文件名与主类型同名；禁止匈牙利命名、C 风格常量、无意义缩写 |
| 7 | **注释规范** | **面向开发者编写**，说明「为什么」而非复述代码；公开 API 用 `///` 文档注释并注释独立成行；**禁止**易失效、低价值、过期、占位性注释（如 `TODO`/`临时`/过时说明） |
| 8 | **通则** | 凡「更先进/优雅/简洁且语义等价」一律采用；命名规范化；禁止双轨写法 |
| 9 | **禁止回退** | 禁止以「无法编译」为由退回原始写法；优雅化与功能开发解耦、可独立验收 |

> 本契约随仓库版本化分发，对每个会话与协作者生效。动 `std/**` 或任何 `.as` 前必读 `arc-language` 完整细则。

**不标准即纠正：** 迭代开发过程中，凡涉及/触及不符合上述编码规范（命名、结构、风格、契约）的现有代码，**必须在同一变更集内纠正**，禁止以「历史遗留 / 非本次改动」为由跳过。例外：若改动落入冻结的稳定面（语言核心 / `rt_*` ABI / `std/Arc` Stable），须遵循 RFC 036 流程，不得静默顺手改（见 `arc-core` / `arc-iteration`「禁顺手改」）。

**验证：** `cargo test --workspace`；管线/端到端变更 `cargo test -p arc-tests`（运行时/进程面加 `--features full-rt`）。

## Crate 架构（13 Cargo + 1 C 资源）

编译器核心 7 crate：`ast` → `parse` → `hir` → `typeck` → `mir` → `codegen` → `arc`（CLI）；配套：`arc-tests`（分层测试：L1 进程内编译快测 / L2 批量运行时 `full-rt` 门控）、`arc-ssr`（SSR 模板编译）、`reachability`（L2 入口可达性分析）、`arc-server`（LSP）、`arcgr`（`.arcgr` 语义索引）、`arc-ui`（声明式 UI，`.arml` 解析/typeck/inspect）；`runtime/` 纯 C 资源（`runtime.c` + `rt_abi.h`）。

**架构红线：** 编译器核心 crate 禁止包含任何领域能力（SQL/ORM/JSON 等翻译逻辑）。领域翻译由 std 库以 Arc 语言实现（如 `std/Orm/SqlTranslator.as`），编译器仅提供通用机制（表达式树构建、类型检查、代码生成）。

分层契约、七条原则、反模式清单与新增 crate 评审见 `arc-rust` / `arc-core`；所有 `lib.rs` ≤80 行（门面仅 `mod` + `pub use`）。
