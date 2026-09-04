# RFC 019 · M4 Parser 子集边界（Arc ↔ Rust `crates/parse`）

> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令不再可用；现行验证矩阵为
> `cargo test --workspace`（运行时面 `cargo test -p arc-tests --features full-rt`），
> 详见仓库根 `CHANGELOG.md`。

> 本文是 [019 自举](../../019-self-hosting.md) §4「子集边界」的**权威载体**，定义 M4（parser 子集）对照的约定边界、验收名与禁止项。

- **权威**：[019 自举](../../019-self-hosting.md) §4 · [003 词法与语法](../../003-lexicon-syntax.md) · [002 语法表面与编码标准](../../002-surface-contract.md) · 实现规划
- **对照面**：Arc `compiler/parser/` ↔ Rust `crates/parse`（`Parser::parse_program` + `dump_parse`）

---

## 1. 目标与非目标

| | 约定 |
|--|------|
| **目标** | 对**固定 fixture**，Arc parser dump 与 Rust `dump_parse` 在**本子集**内**字节级一致** |
| **证伪** | `cargo test -p arc-integration --test arc_parser_parity_e2e` **非 Skip** 且绿 |
| **明确排除** | **全语法 100% 对等**；用「Rust parse 能过的任意 `.as`」当 M4 验收 |
| **明确不做（直至 Mn）** | 删除 Rust `crates/parse` 的 parser 模块 / 默认 CLI 改走 Arc 自举链 |

---

## 2. 对照契约（与 M3 对称）

| 项 | 约定 |
|----|------|
| **首个重写单元** | `parser`（最小）：输入 `string` → 结构化 dump（非完整 `ast` crate 二进制 ABI） |
| **目录** | `compiler/parser/`（`Program.as` + `arc.toml`；lexer 对偶 `compiler/lexer/`） |
| **Fixture 根** | [`crates/parse/fixtures/`](../../../../crates/parse/fixtures/)（相对仓库根；**单一事实源**） |
| **构建** | `cargo run -p arc -- build compiler/parser/Program.as -o arc_parser`（Rust bootstrap 链） |
| **Rust 权威** | `crates/parse`：`Parser::parse_program` + `dump_parse` |
| **成功标准** | fixture 上 Arc dump ↔ Rust dump **diff 为零** |
| **验收 e2e** | **`arc_parser_parity_e2e`**（须非 `#[ignore]` / Skip） |
| **CLI 默认** | 仍为 Rust bootstrap；`arc parse` / `arc build` 默认路径不变 |

### 2.1 Dump 格式

权威实现落地于 `crates/parse`（`dump_parse.rs`，与 `dump_lex` 并列）。Arc parser 与 `arc_parser_parity_e2e` **必须**对齐同一文本契约；**改格式须同 PR** 更新 Rust dump、Arc parser、fixture/e2e 与本节——禁止单侧漂移。

| 规则 | 约定 |
|------|------|
| 行模型 | **一行一个节点摘要**；缩进用 **2 空格**表示父子 |
| 节点名 | 稳定标签（如 `Program` / `Namespace` / `Fn` / `Class` / `Struct` / `Enum` / `Stmt.If` / `Expr.Binary`），**不**打印源码 span / file_id |
| 载荷 | `Tag` + 可选 `\t` + 转义字段（字段顺序固定，见 `dump_parse.rs` 冻结实现） |
| 转义 | 与 M3 `dump_lex` 相同：`\\` `\n` `\t` `\r` |
| 子集外构造 | fixture **不得**包含；若误入，Rust 侧可 parse 成功但 **e2e 不得**将该文件列入对照集 |
| 错误路径 | M4 **不要求**非法输入诊断字节级对齐（诚实非阻塞；可另记） |

---

## 3. 约定子集：纳入的产生式

对照粒度 = **产生式 / AST 形状**，不是「能编译通过的任意程序」。下列均须有对应 fixture 行覆盖（可合并进少数 `.as` 文件）。

### 3.1 模块 / 项（Item）

| ID | 产生式 | 形状约束 |
|----|--------|----------|
| I1 | `using Path;` | 无 alias；无 `global using` |
| I2 | `namespace N { … }` | 单段或 dotted path；**无** `capability` |
| I3 | 自由函数 `Ret Name(params) { body }` | 见 P* / S* / E*；顶层或 namespace 内 |
| I4 | `class Name { fields; methods }` | 见下；**无** 基类 / 接口列表 / `partial` |
| I5 | `struct Name { fields; methods }` | 同 class 字段/方法约束 |
| I6 | `enum Name { CaseA, CaseB }` | **仅**无载荷枚举成员；**无** 方法体 |
| I7 | 类型成员：实例字段 | `Type name;` / `Type name = init;`；可见性 `public`/`private`（或默认） |
| I8 | 类型成员：实例方法 | 同 I3 形参/语句约束；**无** `virtual`/`override`/`static`/`async` |

### 3.2 形参与类型（Param / Type）

| ID | 产生式 | 形状约束 |
|----|--------|----------|
| P1 | 形参 `Type name` | 无默认值；无 `ref`/`out`/`in`/`params`/`this` |
| P2 | 返回类型 | 具名类型或 `void` |
| T1 | 具名类型 | `int`/`bool`/`string`/`char`/`void` 或简单标识 / dotted 路径（无泛型实参） |
| T2 | 数组类型 | `T[]`（一维）及嵌套 `T[][]`（交错数组的数组）；**无** 多维 `[,]` / `*` 指针 |

### 3.3 语句（Stmt）

| ID | 产生式 | 形状约束 |
|----|--------|----------|
| S1 | 局部声明 | `Type name = init;` 或 `var name = init;`（`var` 仅局部） |
| S2 | 表达式语句 | `expr;` |
| S3 | `return;` / `return expr;` | |
| S4 | `if (cond) stmt/block` + 可选 `else` | cond 为子集表达式 |
| S5 | `while (cond) { … }` | |
| S6 | C 风格 `for (init; cond; inc) { … }` | 三子句均可空；init/inc 为声明或赋值级语句 |
| S7 | `break;` / `continue;` | |
| S8 | 赋值 | `lhs = rhs;`；lhs = 标识 / 字段 / 索引（子集内） |
| S9 | `switch (expr) { case …: … break; default: … }` | **仅** 常量/`enum` 成员 case；**无** 类型模式 / `when` / switch 表达式 |

### 3.4 表达式（Expr）

| ID | 产生式 | 形状约束 |
|----|--------|----------|
| E1 | 字面量 | `int` / `float` 源码切片语义 / `bool` / `string` / `char` / `null` |
| E2 | 标识与简单路径 | `x` / `A.B`（成员访问链） |
| E3 | 一元 | `-` `!`（不含 `await`/`&`） |
| E4 | 二元 | 算术 / 比较 / 逻辑 / `+` 字符串拼接（运算符表与 Rust parser 一致；不含 `??`/`?.`） |
| E5 | 调用 | `f(args)` / `recv.M(args)`；**无** 类型实参 `<…>`；**无** 命名实参 |
| E6 | 索引 | `a[i]` |
| E7 | `new T(args)` | **无** 对象初始化器 `{ Prop = … }`；**无** `new()` target-typed |
| E8 | 括号 | `(expr)` |
| E9 | `if` 表达式 | 仅当语句层已覆盖；与 S4 同源形状即可 |

### 3.5 Fixture 分层

| 层 | 文件（仓库相对路径） | 覆盖 ID |
|----|---------------------|---------|
| **smoke** | [`compiler/parser/fixtures/smoke.as`](../../../../crates/parse/fixtures/smoke.as) | I1–I3、S1–S3、E1–E6 最小竖切 |
| **types** | [`compiler/parser/fixtures/types.as`](../../../../crates/parse/fixtures/types.as) | I4–I8、T1–T2、字段初始化 |
| **control** | [`compiler/parser/fixtures/control.as`](../../../../crates/parse/fixtures/control.as) | S4–S9、E3–E4、enum+switch |

Rust `dump_parse` 单测、Arc parser CLI、`arc_parser_parity_e2e` **共用**该目录。新增 fixture 须两侧同挂。

---

## 4. 明确排除（不得进入 M4 对照集）

下列任一项出现在 M4 fixture / 宣称「已对等」→ **NOT ready** / 不得宣称 M4 达成：

| 排除类 | 例（非穷尽） |
|--------|----------------|
| **全语法 100%** | 「Rust `parse_program` 能吃的任意源」作验收 |
| 泛型 | `class List<T>`、方法/调用 `<T>`、`where` |
| 属性 / 索引器 / `init`/`required` | 含 auto-property |
| OOP 进阶 | 继承、接口、`virtual`/`override`/`abstract`、`static` 成员 |
| async | `async`/`await` |
| Lambda / 委托 / Expression | `=>`、`Func<>`、expression tree |
| 模式进阶 | `is` 模式、`switch` 表达式、属性/位置模式、`when` |
| 记录与解构 | `record`、`with`、deconstruct |
| 可空与条件访问 | `T?`、`?.`、`??`、`!.` |
| 插值 / verbatim | `$"..."`、`@"..."`、`$@"..."`（词法属 M3；**语法树**不进 M4） |
| 集合表达式 | `[e1, e2]`、`..` spread |
| Query / LINQ | `from`/`select`/… |
| 异常 / using | `try`/`catch`/`throw`、`using` 语句/声明 |
| 扩展 / 参数进阶 | `this` 扩展、`ref`/`out`/`in`、可选/命名实参、`params` |
| 特性 / 原生 | `[Attr]`、`.ani` / `native`、`variant` |
| `global using` / capability namespace | |
| 多文件项目图 | M4 仅**单文件** fixture 对照 |

**后置**：上表能力若自举后续阶段需要，另开 M* 或扩边界 RFC；**禁止**静默扩大本表而不改文档。

---

## 5. 禁止项（出现任一 → 不得宣称 M4 达成）

| 禁止 | 说明 |
|------|------|
| 删除 Rust parser | 禁止删或掏空 `crates/parse` 的 parser 模块（`parser.rs` / `item*` / `stmt` / `expr*` / `ty` 等）直至 Mn |
| 切换默认 CLI | 禁止 `arc` 默认走 Arc 自举链（直至 Mn） |
| 无边界开工 | 无本文件（或后继 Accepted 修订）即大范围实现 Arc parser |
| 宣称全语法对等 | 违反 §1 / §4 |
| 单侧改 dump | 改格式须同 PR 更新 Rust + Arc + e2e + §2.1 |
| 掩盖语言债 | 违反 [019 立宪公理](../../019-self-hosting.md) |
| 用 Skip 名义绿 | `arc_parser_parity_e2e` 须非 Skip |

---

## 边界

- 本篇只讲 M4 parser 子集边界；自举阶段划分/门禁/硬约束见 [019 自举](../../019-self-hosting.md)。
- 语法权威见 [003 词法与语法](../../003-lexicon-syntax.md) 与 [002 语法表面与编码标准](../../002-surface-contract.md)。
- 里程碑排期与状态演进见 实现规划，不属本设计契约。

---

上一节：[019 自举](../../019-self-hosting.md) · [返回 RFC 目录](../../index.md)

