# RFC 002 语法表面与编码标准

## 背景

冻结 Arc **表面语法与编码**的用户决策，作为全书与实现的单一权威来源。目标：每种意图只有一种推荐表达（单一惯用法）、源码存在唯一规范形式（确定性格式）。基准为 C# / .NET 表面惯例；`crates/` 内 Rust 标识符遵循 Rust 惯例，**不得**反向污染 Arc 源码风格。

## 设计决策

### 闭包捕获语义（C# 对齐）

- **普通局部变量**：闭包按引用捕获（`ByRef`）——闭包内对捕获变量的重新绑定写回外层变量（C# 闭包语义）。
- **`this`**：按值捕获（`ByValue`，RFC 006 G2）——避免宿主方法返回后栈槽悬垂。
- **循环体局部**（`while`/`for`/`foreach` 体内声明的变量）：按值捕获（快照）——循环槽跨迭代复用，`ByRef` 会让每个迭代的闭包读到**最后一次**赋值的对象。C# 中循环变量逐迭代独立，闭包捕获各自迭代的快照，二者语义一致。闭包创建点对快照的引用对象 `rt_arc_inc` 持强引用（env 生命周期进程级，随 env 泄漏，与既有 `this` 捕获模型一致）。

### 已决事项（单一来源）

| # | 议题 | 决策 |
|---|------|------|
| 1 | 源文件扩展名 | **`.as`** 为规范与工具链唯一扩展名（非 `.arc`） |
| 2 | `using` 语义 | **全面 C# 风格**：`using Arc;`、`using Alias = X.Y;` 命名空间/类型导入；**非**文件包含式 `#include` |
| 3 | 借用表面语法 | **接近 C#**：用户源码无 `&T` / `&mut T`；借用语义仅在 MIR / borrowck 内部 |
| 4 | `namespace` | 点分 **`namespace A.B.C;`**（file-scoped）或 **`namespace A.B.C { }`**（块）；**拒绝**单段 `namespace X; { }` |
| 5 | `let` / `mut` | **拒绝**；仅 **`var`**（右推断）或 **前导类型**（`int x = 1`） |
| 6 | 表达式树 | `Expression<Func<...>>` + 普通 Lambda `=>`；**无** `expression` 关键字；不用 `expr` |
| 7 | `edition` | 可选 `edition = "1"`（供未来 breaking 语法版本切换） |
| 8 | 包名 vs 命名空间根 | 库模块须声明 `namespace`，根须与 `[package].name` 一致；入口 `main.as` 可省略（全局命名空间） |
| 9 | 集合初始化 | **唯一惯用法**：C# 12 `[e1, e2, ...]`；**硬拒绝** `new T[] { }`、`new[] { }`、前导类型 + `{ }`、裸 `{ }` |
| 10 | 分支语法 | **`switch`/`case`/`default`/`break`**（C#）；**拒绝** Rust 式 `match` |
| 11 | foreach + LINQ | LINQ 查询作迭代源须括号或先赋给变量 |
| 12 | 自动属性 | `{ get; set; }` / `{ get; }` |
| 12b | 表达式体成员 | `Type P => e;` / `get => e;` / `set => stmt;` / `Ret M() => e;` |
| 13 | 接口属性 | `Type Name { get; }`；访问用 `.Name`（非 `.Name()`） |
| 14 | `this` | 实例方法/构造器有隐式 `this`；静态/顶层函数无 |
| 15 | 索引器 | `T this[params] { get; set; }`；元数据名 `Item` → `get_Item`/`set_Item` |

### 命名规范

| 项 | 规范 |
|----|------|
| 类型 / 方法 / 属性 / 命名空间 | **PascalCase**（`UserStore`、`GetUser`） |
| 接口 | `I` + PascalCase（`IRepository`） |
| 参数 / 局部 | **camelCase**（`userId`、`itemCount`） |
| 私有字段 | `_` + camelCase（`_connectionString`） |
| 源文件 | 与主类型同名（`User.as`） |

### 前导类型

所有类型位置采用 **类型在前**，`var` 仅在初始化表达式可推断时使用：

```as
int count = 0;
string message = "ready";
void Main() { }
Task<int> compute();
var sum = a + b;   // 右推断
```

### 拒绝项（Rust 渗透，不得进入 `.as` 表面）

| Rust-ism | 拒绝理由 | Arc 目标 |
|----------|----------|----------|
| `fn` 关键字 | C# 无 `fn`，函数用前导返回类型 | `async Task<T> name()` |
| `let` / `let mut` | C# 用 `var` 或显式类型 | 仅 `var` 或前导类型 |
| `::` 路径分隔 | C# 用 `.` 作限定名 | `Arc.Console` |
| `->` 函数类型 | C# 用 `Func<>` / 委托 | `Func<T, bool>` |
| 后缀类型注解 `name: Type` | C# 为前导类型 | `User[] users = [...]` |
| `match` 关键字 | C# 用 `switch`/`case`/`break` | **`switch`/`case`/`default`/`break`** |
| `&` / `&mut` 借用语法 | C# 无显式借用；借用语义对齐 C# | 仅 MIR/borrowck 内部 |
| snake_case 用户标识符 | C# 用 camelCase / PascalCase | 持续 lint |
| `mod` / `use` 模块 | Arc 用 `namespace` / `using` | 不进入 `.as` |
| `expr` 短关键字 | C# 生态用 `Expression<>` | 统一为 `Expression<>` |
| 文件包含式 `using` | C# `using` 是导入，非 `#include` | 全面 C# `using` |

### 拒绝项（C# 历史双轨，硬拒）

| 形式 | 拒绝理由 | Arc 唯一惯用法 |
|------|----------|----------------|
| `new T[] { }` / `new[] { }` / 前导类型 + `{ }` | C# 历史多写法 | **`[e1, e2, ...]`** |
| 裸 `{ }` 表达式 | 非标准集合表达式 | `[...]` |
| 裸 `Point { X = 1 }`（无 `new`） | 结构/类初始化须 `new` | `new Point() { X = 1 }` |

### 异步一体原则（Arc 专属）

涉及 I/O 的方法仅提供**异步签名**，无同步副本（消除 C# EF Core `ToList()`/`ToListAsync()` 双重 API 历史冗余）：

| 规则 | 规范 |
|------|------|
| 返回 `Task`/`Task<T>` | 必须 `Async` 后缀 |
| 无 I/O 的纯状态操作 | 同步命名，无 `Async` 后缀 |
| `CancellationToken` | 所有 async 方法必须提供，参数名统一 `cancellationToken` |

## 边界

- 具体产生式与示例见 [词法与语法](003-lexicon-syntax.md)。
- 类型与泛型见 [类型系统](004-type-system.md)。
- 集合表达式细节见 [集合、字符串与数值](007-collections-strings-numerics.md)。

---

上一节：[001 语言宪章](001-language-charter.md) · 下一节：[003 词法与语法](003-lexicon-syntax.md)