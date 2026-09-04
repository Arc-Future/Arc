# RFC 011 表达式树与查询语言

## 背景

Arc Query 语言（LINQ 风格）提供声明式数据变换。**同一条语法**在 `IEnumerable<T>` 与 `IQueryable<T>` 上走两条语义路径，由编译器静态分派。Arc 是编译型语言：表达式树面向**编译期树化**，而非运行时解析。

## 设计决策

### 编译期树化 vs 运行时解释

| 维度 | 运行时解释（Arc **不采用**） | 编译期树化（Arc **采用**） |
|------|------------------------------|----------------------------|
| Enumerable Lambda | 委托调用、虚派发、装箱迭代器 | 单态化循环；`Where`/`Select` 内联为控制流 |
| Queryable `Expression<T>` | 运行时解析源码、动态拼 SQL | typeck 构建 `ExpressionIr`；codegen 生成运行时构造 `Expression` 对象的代码 |
| Provider | 用户程序内解析源码节点 | **运行时**由 Provider 遍历 `Expression` 对象树，生成 SQL 或执行计划 |
| 热路径开销 | 每查询分配 + 解析 + 间接调用 | 树化在编译期完成；运行时仅遍历静态结构对象 |

**原则**：编译器核心止于树化 IR 与运行时构造代码产出；方言翻译（SQL 等）在 `std/Orm` 的 Provider 中于**运行时**完成，不得渗入 `crates/mir` / `crates/codegen` 作为编译期展开逻辑。

### 类型 `Expression<T>`

- 声明为 `Expression<Func<...>>` 的 Lambda，或传给 `IQueryable<T>` 方法的 Lambda，编译为表达式树。
- `body` 限于可树化的表达式子集（成员访问、比较、逻辑运算、常量等）。
- 不得包含语句块、局部声明或任意副作用调用。
- **无** `expression` 关键字（硬拒绝）。

```as
Expression<Func<User, bool>> pred = u => u.Age >= 18 && u.Active;

var q = db.Users
    .Where(u => u.Age >= 18)
    .OrderBy(u => u.Name)
    .Select(u => u.Name);
```

### 表达式树 IR

表达式树 IR 节点种类：`Parameter`（Lambda 参数）、`MemberAccess`（字段/属性访问）、`Binary`/`Unary`（运算）、`Constant`（字面量）、`Call`（可树化的调用子集）、`Lambda`（嵌套，受限）。typeck 在检查 `Expression<T>` Lambda 时同步构建 `ExpressionTree`。节点字段强类型化（`Type`/`MemberInfo`/`MethodInfo`）。

### Enumerable 路径

接收者为数组 / `List<T>`（`IEnumerable<T>` 物化源）时：

- Lambda 为普通 `=>`；**编译期**将 `Where`/`Select` **单态化**为特化循环，由 codegen 直接发射控制流——**不是**运行时 LINQ 委托链。
- 优先零额外堆抽象（foreach 流式）；赋值目标为 `List<T>` 时物化。

```as
void demoEnumerable(List<User> users) {
    foreach (var name in from u in users
                         where u.Active
                         select u.Name) {
        Console.WriteLine(name);
    }
}
```

### Queryable 路径

接收者为 `IQueryable<T>` 时：

- 谓词与投影 Lambda 在 `IQueryable<T>` 链上由 typeck 编译为表达式树。
- **编译期**解析整条查询链、构建 `ExpressionIr`；codegen 为每个 Lambda 生成运行时构造 `Expression` 对象树的代码。
- 结果仍为 `IQueryable<U>`，可继续链式组合。
- Provider 在**运行时**遍历 `Expression` 对象树生成方言 SQL 或执行计划。

```as
void demoQueryable(IQueryable<User> users) {
    var q = users
        .Where(u => u.Age >= 18)
        .OrderBy(u => u.Name)
        .Select(u => u.Name);
}
```

### Query comprehension 子句

| 子句 | 语义 |
|------|------|
| `from x in source` | 稳定（`T[]` / `List<T>`） |
| `where pred` | 稳定（MIR 内联） |
| `select proj` | 稳定（MIR 内联；foreach 流式） |
| 方法链 `.Where` / `.Select` | 稳定（与 query 同路径） |
| 终端 `.Any` / `.Count` / `.First` / `.FirstOrDefault` | 稳定（0 参或单谓词；MIR 编译期展开） |
| `orderby key` | 稳定（缓冲排序） |
| `let` / `join` / `groupby` | 未落地 |

### 脱糖规则

```
from x in s where p select f(x)
  ≡ s.Where(x => p).Select(x => f(x))
```

comprehension 与方法链在 **Where/Select/OrderBy** 上语义对齐。Queryable 路径仍用普通 `=>`，由接收者类型（`IQueryable<T>`）与形参类型（`Expression<Func<...>>`）在 typeck 树化。

### Provider 与 SQL 翻译分层

- 编译器职责止于树化：`crates/ast` 构建 `ExpressionIr`，typeck 树化，codegen 生成运行时构造代码。
- Provider 实现（位于 **`std/Linq`**，非编译器核心）遍历 `Expression` 对象树翻译为存储或远程执行计划。
- 通用 SQL 翻译在 `std/Orm/SqlTranslator.as`（`Arc.Orm`）；具体数据库提供程序（如 `SqliteProvider`）在平级方言子库（`std/Orm.SQLite/`，命名空间 `Arc.Orm.SQLite`），实现 `IQueryProvider` 并提供运行时连接管理，**不得**硬编码在 `crates/mir` 或 `crates/codegen`。

```as
interface IQueryProvider {
    string Translate<T>(Expression<T> expression);
}
```

### 性能契约

| 路径 | 编译期 | 用户程序运行时 |
|------|--------|----------------|
| Enumerable | 脱糖 + Lambda 内联为循环 | 仅执行特化循环体；无委托、无解释器 |
| Queryable | 树化（typeck 构建 `ExpressionIr`）；codegen 生成构造代码 | Provider 遍历 `Expression` 对象树翻译 SQL 并执行；翻译结果可缓存 |
| `Expression<T>` Lambda | typeck 树化为 `ExpressionIr` | 运行时构造 `Expression` 对象树供 Provider 遍历 |

### 禁止混用

同一调用链上不得混用 Enumerable 与 Queryable 语义；接收者类型在链首确定整条路径。

## 边界

- Lambda 与委托机制见 [委托、闭包与方法组](008-delegates-closures.md)。
- 具体 ORM / SQL 方言翻译见 [ORM 与 SQL 翻译](039-orm.md)。
- 编译期树化的元编程定位见 [编译期元编程](012-compile-time-metaprogramming.md)。

## 禁止项

- **不引入** `expression` 关键字。
- **不在编译器核心 crate 内实现 SQL 或其他查询方言翻译**。
- **不在用户热路径做运行时 AST walker / 源码解析**。

---

上一节：[010 异常与资源管理](010-exceptions-resources.md) · 下一节：[012 编译期元编程](012-compile-time-metaprogramming.md)