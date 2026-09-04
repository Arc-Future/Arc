# 09 查询语言

Arc Query 语言（LINQ 风格）提供声明式数据变换。**同一条语法**在 `IEnumerable<T>` 与 `IQueryable<T>` 上走两条语义路径，由编译器静态分派。

Arc 是**编译型语言**：Queryable、ExpressionTree 及相关特性均面向**编译期树化**，而非运行时解析。性能契约是零/低开销抽象——Queryable 树化在编译期完成（typeck 构建 `ExpressionIr`），翻译在运行时由 `IQueryProvider` 遍历 `Expression` 对象树完成，**不是**用户程序热路径上的源码解析器。

## 编译期树化 vs 运行时解释

| 维度 | 运行时解释（Arc **不采用**） | 编译期树化（Arc **采用**） |
|------|------------------------------|----------------------------|
| Enumerable Lambda | 委托调用、虚派发、装箱迭代器 | 单态化循环；`Where`/`Select` 内联为 C/LLVM 控制流 |
| Queryable `Expression<T>` | 运行时解析源码、动态拼 SQL | typeck 构建 `ExpressionIr`；codegen 生成运行时构造 `Expression` 对象的代码 |
| Provider | 用户程序内解析源码节点 | **运行时**由 Provider 遍历 `Expression` 对象树，生成 SQL 或执行计划 |
| 热路径开销 | 每查询分配 + 解析 + 间接调用 | 树化在编译期完成；运行时仅遍历静态结构对象（可缓存翻译结果） |
| 可优化性 | 黑盒，难以跨调用优化 | 编译期树结构已知，翻译结果可缓存 |

**原则**：编译器核心止于树化 IR 与运行时构造代码产出；方言翻译（SQL 等）在 `std/Orm` 的 Provider 中于**运行时**完成（遍历 `Expression` 对象树），不得渗入 `crates/mir` / `crates/codegen` 作为编译期展开逻辑。

## Query comprehension

```as
var adults = from u in users
             where u.Age >= 18
             select u.Name;
```

子句（**诚实子集**）：

| 子句 | Enumerable 状态 |
|------|-----------------|
| `from x in source` | **Stable**（`T[]` / `List<T>`） |
| `where pred` | **Stable**（MIR 内联） |
| `select proj` | **Stable**（MIR 内联；foreach 流式） |
| 方法链 `.Where` / `.Select` | **Stable**（与 query 同路径） |
| 终端 `.Any` / `.Count` / `.First` / `.FirstOrDefault` | **Stable**（0 参或单谓词；`T[]` / `List<T>`；可接 Where/Select 前缀；MIR 编译期展开；**非** Queryable；空序列 `First` → `rt_panic`；空/无匹配 `FirstOrDefault` → `default(T)`） |
| 赋值物化 `List<T> xs = from …` | **Draft**：MIR `materialize_linq_chain_to_list` 已有；赋值目标 typeck 仍后置 |
| `orderby key` / 多键 `orderby k1, k2` | **Stable**（无捕获 key：`T[]` / `List<T>`；MIR 物化排序——缓冲 `List<T>` + `rt_list_sort` comparator，数组/List 源共用；`int`/数值/`bool`/`char`/`string`/可 `CompareTo` 类 key；`descending` 取反；连续多 key 折叠为单 comparator 依次生效——ThenBy 语义） |
| `let` / `join` / `groupby` | **Stable 最小面**（MIR 特化物化：`let` 绑定；`join` inner join 等值；`group … by … [into g]` 产物 `Grouping<K,T>`；证据 `linq_let_join_groupby_e2e`） |
| `ToList` 等其余终端 | **后置**（扩展解析债） |

## 方法链形式

与 comprehension 在 **Where/Select/OrderBy** 上语义对齐（MIR `try_lower_linq_chain`）；`OrderBy` 同 query 路径——缓冲排序（key 无捕获时生效；捕获 key / 无支持比较类型诚实跳过）：

```as
// Stable：query 或方法链
foreach (var name in from u in users where u.Age >= 18 select u.Name) { ... }
foreach (var n in nums.Where(x => x > 10).Select(x => x * 2)) { ... }

// Stable：终端（数组 / List；MIR 展开；非 Queryable）
bool any = nums.Any(x => x > 10);
int n = nums.Count();
int first = nums.Where(x => x > 10).First();
int orDef = nums.FirstOrDefault(x => x > 100); // 无匹配 → 0
```

`ToList` / 泛型扩展方法体路径仍后置；本面不冒充全量 LINQ。

## Enumerable 路径

接收者为数组 / `List<T>`（`IEnumerable<T>` 物化源）时：

- Lambda 为普通 `=>`；**编译期**将 `Where`/`Select` **单态化**为特化循环，由 `codegen` 直接发射控制流——**不是**运行时 LINQ 委托链
- 优先零额外堆抽象（foreach 流式）；赋值目标为 `List<T>` 时物化

```as
void demoEnumerable(List<User> users) {
    foreach (var name in from u in users
                         where u.Active
                         select u.Name) {
        Console.WriteLine(name);
    }
}
```

`typeck` 设置 `LinqPath::Enumerable`。

## Queryable 路径

接收者为 `IQueryable<T>` 时（**目标语义**；现行为 **L3 骨架**）：

- 谓词与投影 Lambda 在 `IQueryable<T>` 链上由 typeck 编译为表达式树
- **编译期**解析整条查询链、构建 `ExpressionIr`；codegen 为每个 Lambda 生成运行时构造 `Expression` 对象树的代码
- 结果仍为 `IQueryable<U>`，可继续链式组合；链上各 Lambda 在编译期完成树化
- Provider 在**运行时**遍历 `Expression` 对象树生成方言 SQL 或执行计划——树化在编译期完成，翻译在运行时执行

> **诚实**：语言 Phase A（`Expression<Func>`）已绿 ≠ Orm / Provider 产品可用。`AsQueryable` 已从 Stable/公开面撤下（禁 null stub）；完整 SQL 链属 L3，须单目标有边界 Sprint；**禁**假开全家桶。

```as
void demoQueryable(IQueryable<User> users) {
    var q = users
        .Where(u => u.Age >= 18)
        .OrderBy(u => u.Name)
        .Select(u => u.Name);
}
```

`typeck` 设置 `LinqPath::Queryable`；Enumerable 路径上的普通 Lambda 不得误用于需要树化的 Queryable 签名。

## 脱糖规则（概要）

comprehension：

```
from x in s where p select f(x)
  ≡ s.Where(x => p).Select(x => f(x))
```

Queryable 路径仍用普通 `=>`：由接收者类型（`IQueryable<T>`）与形参类型（`Expression<Func<...>>`）在 typeck 树化；**无** `expression` 关键字（硬拒绝）。

`crates/linq` 在 HIR/MIR 之前完成脱糖，输出标准方法调用节点。

## Provider

Queryable 数据源通过 `IQueryProvider` 提交表达式树：

```as
class DataContext {
    public IQueryProvider Provider;
}
```

**编译器职责止于树化**：`crates/ast`（`expr_tree.rs`）构建 `ExpressionIr`，`typeck` 将 `Expression<T>` Lambda 树化为 IR，`codegen` 生成运行时构造 `Expression` 对象树的代码（见[表达式树](10-expression-trees.md)）。Provider **实现**在**运行时**遍历 `Expression` 对象树并翻译为存储或远程执行计划。

**SQL 翻译分层**：Queryable 路径采用编译期树化 + 运行时翻译双阶段：typeck 阶段构建 `ExpressionIr`；运行时由 `std/Orm/SqlTranslator.as`（`Arc.Orm`）遍历 `Expression` 对象树生成方言 SQL。具体数据库提供程序（如 `SqliteProvider`）属于平级方言子库（`std/Orm.SQLite/`，命名空间 `Arc.Orm.SQLite`），实现 `IQueryProvider` 并提供运行时连接管理，**不得**硬编码在 `crates/mir` 或 `crates/codegen` 中。

## 性能契约

| 路径 | 编译期 | 用户程序运行时 |
|------|--------|----------------|
| Enumerable | 脱糖 + Lambda 内联为循环 | 仅执行特化循环体；无委托、无解释器 |
| Queryable | 树化（typeck 构建 `ExpressionIr`）；codegen 生成构造 `Expression` 对象的代码 | Provider 遍历 `Expression` 对象树翻译 SQL 并执行；翻译结果可缓存 |
| `Expression<T>` Lambda | typeck 树化为 `ExpressionIr` | 运行时构造 `Expression` 对象树供 Provider 遍历 |

违反上表（例如在热路径调用 `rt_expr_tree_summary()` 动态格式化树）是语义错误——表达式树的翻译与格式化一律在构建时完成，运行时不得动态解析或格式化。

## 标准库

- `std/Arc/Linq/Enumerable.as` — Enumerable 契约面（MIR 展开；`namespace Arc.Linq`）
- `std/Arc/Linq/` — `IQueryProvider` / `IQueryable<T>` 与表达式树节点（Queryable 接口层）

## 禁止混用

同一调用链上不得混用 Enumerable 与 Queryable 语义；接收者类型在链首确定整条路径。

---

上一节：[08 异步与任务](08-async-tasks.md) · 下一节：[10 表达式树](10-expression-trees.md)