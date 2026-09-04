# 03 编码与语法标准

本节是 Arc **表面语法与命名**的权威对照表：明确采纳 C# 惯例、拒绝 Rust 渗透。实现与文档冲突时，以本节 + [04 词法与语法](04-lexicon-syntax.md) 为准。

**基准**：C# / .NET 表面惯例。`crates/` 内 Rust 标识符遵循 Rust 惯例，**不得**反向污染 Arc 源码风格。

## 已决事项

| # | 议题 | 决策 |
|---|------|------|
| 1 | 源文件扩展名 | **`.as`** 为规范与工具链唯一扩展名（非 `.arc`） |
| 2 | `using` 语义 | **全面 C# 风格**：`using Arc;`、`using Alias = X.Y;` 等命名空间/类型导入；**非**文件包含式 `#include` |
| 3 | 借用表面语法 | **接近 C#**：用户源码无 `&T` / `&mut T`；借用语义仅在 MIR / `borrowck` 内部 |
| 4 | 过渡 `namespace X; { }` | **拒绝**单段 `namespace X; { }`；采纳 C# 点分 **`namespace A.B.C;`**（file-scoped）或 **`namespace A.B.C { }`**（块） |
| 5 | `let` / `mut` | **拒绝**；仅 **`var`**（右推断）或 **前导类型**（`int x = 1`、`User[] users = [...]`） |
| 6 | `expression` vs `expr` | **C# 命名**：`Expression<Func<...>>` + 普通 Lambda；**无** `expression` 关键字；**不用** Rust 式 `expr` |
| 7 | `arc.toml` `edition` | 可选 **`edition = "1"`**；类似 Rust edition，供未来 breaking 语法版本切换；默认 `"1"`，**尚无行为分支** |
| 8 | 包名 vs 命名空间根 | **库模块**须声明 `namespace`，根须与 `[package].name` 一致；**入口 `main.as` 可省略**（全局命名空间，对齐 C# `Program.cs`） |
| 9 | 数组 / 集合初始化 | **唯一惯用法**：C# 12 **`[e1, e2, ...]`**（含 `var v = [e1, e2]`）。**硬拒绝** `new T[] { }`、`new[] { }`、前导类型 + `{ }`、裸 `{ }` |
| 10 | 分支语法 | **`switch`/`case`/`default`/`break`**（C#）；**拒绝** `match` |
| 11 | foreach + LINQ | LINQ 作 `foreach` 源须 `(from ...)` 或变量 |
| 12 | 自动属性 | `{ get; set; }` / `{ get; }` |
| 12b | 表达式体成员 | `Type P => e;` / `get => e;` / `set => stmt;` / `Ret M() => e;`（脱糖为既有访问器/方法块） |
| 13 | 接口属性 | `Type Name { get; }`；`.Name` 非 `.Name()` |
| 14 | `this` | 实例方法/构造器有 `this`；静态/顶层函数无 |
| 15 | 索引器 | C# `T this[params] { get; set; }`；元数据名 `Item` → `get_Item`/`set_Item` |

## Arc 采纳（来自 C#）

| 领域 | Arc 规范 | C# 对应 | 示例 |
|------|----------|---------|------|
| 声明 | **前导类型** | 同左 | `int count = 0;` `void Main() { }` |
| 局部推断 | `var`（类型在右推断） | `var` | `var sum = a + b;` |
| 类型声明 | `class` / `record` / `struct` / `interface` / `enum` | 同左 | `public class User { }` / `record Point(int X, int Y);` |
| 访问修饰符 | `public` / `private` / `protected` / `internal` | 同左 | `private int _count;` |
| 继承 | 单类 + 多接口，`:` 列表 | 同左 | `class D : Base, I1, I2` |
| 成员调用 | **点号** `.` | 同左 | `Console.WriteLine(msg);` |
| 命名空间 | `namespace` + `using` | 同左（含 file-scoped） | `namespace Arc;` |
| 异步 | `async` / `await`，`Task<T>` | 同左 | `async Task<int> load() { }` |
| 查询 | `from` / `where` / `orderby` / `select` | LINQ comprehension | `from x in xs where x.Active select x` |
| 表达式树 Lambda | **`Expression<Func<...>>`** + 普通 `=>` | `Expression<>` 编译期树化 | `Expression<Func<User, bool>> p = u => u.Age >= 18` |
| 构造函数 | `new Type(args)` / 目标类型 `new(args)` | 同左（C# 9+） | `new Rectangle(10, 20)`；`Rectangle r = new(10, 20)` |
| 命名：类型/方法/属性 | **PascalCase** | 同左 | `UserStore`, `GetUser` |
| 命名：接口 | `I` + PascalCase | 同左 | `IRepository` |
| 命名：参数/局部 | **camelCase** | 同左 | `userId`, `itemCount` |
| 命名：私有字段 | `_` + camelCase | 同左 | `_connectionString` |
| 命名：源文件 | 与主类型同名 | 同左 | `User.as` |
| 泛型 | 尖括号 `List<T>` | 同左 | `Task<User>`, `IEnumerable<T>` |
| 委托类型（规范） | `Func<T, R>` / 方法组 | `Func<>` / `Action<>` | `Func<User, bool>` |
| 数组（规范） | `T[]` 或 `List<T>` | 同左 | `User[] users = ...` |
| 数组创建（规范） | 见下表 | C# 12 集合表达式（唯一） | `int[] x = [1, 2]`、`var v = [1, 2]` |
| 分支 | `switch` / `case` / `default` / `break` | 同左 | `switch (x) { case 0: break; default: break; }` |
| 属性 | `{ get; }` / `{ get; set; }`（自动属性 → 字段后备）；自定义 `{ get {…} set {…} }`；表达式体 `T P => e;` / `get => e;` / `set => lhs = value;` | 同左 | `public int Value => _value;` |
| 方法 | 块体或表达式体 `Ret M(...) => e;`（`void` → 语句；非 void → `return`） | 同左 | `public int Doubled() => Value * 2;` |
| 接口属性 | `Type Name { get; }`；访问用 `.Name` | 同左 | `shape.Name`（非 `Name()`） |
| 索引器 | `T this[params] { get/set }`；用法 `obj[i]` | 同左 | `public T this[int i] { get; set; }` |
| `this` | 实例方法/构造器隐式 `this`；静态方法与顶层函数无 `this` | 同左 | `this.Width` 于实例方法内 |
| 扩展方法 | `static class` + `static` 方法，首参 `this T receiver`；调用 `r.Ext()` 脱糖为 `ExtClass.Ext(r, …)` | 同左 | `r.Ext()` |
| foreach + LINQ | LINQ 查询须括号或先赋给变量 | 同左 | `foreach (var x in (from u in xs select u.N))` |
| 错误传播 | `?` 后缀（Arc 扩展，单一形式） | 无直接对应 | `var v = load()?;` |
| 异步方法命名 | 返回 `Task`/`Task<T>` 的方法 **`Async` 后缀**；所有 I/O 方法必须提供 `CancellationToken` 重载 | `Async` 后缀 + `CancellationToken` | `ToListAsync()` / `ToListAsync(CancellationToken ct)` |
| 数据访问 | `DbContext` scoped + `IDbProvider` singleton；`DatabaseKind` 枚举多后端 | `DbContext` / `DbSet<T>` | `db.Users.Where(...).ToListAsync()` |

### 数组字面与初始化（`[…]` 唯一）

| 形式 | 示例 | 元素类型来源 | `var` 可用 |
|------|------|--------------|------------|
| 集合表达式 `[…]` | `int[] nums = [1, 2];` | 声明中的 `T[]` 或元素推导 | 是（`var v = [1, 2]` → `int[]`） |
| `new T[] { }` / `new[] { }` / 前导类型 + `{ }` | （历史 C#） | — | **硬拒绝**（须改用 `[...]`） |
| 裸 `{ }` 表达式 | `var v = { 1, 2 };` | — | **硬拒绝** |
| 对象构造 | `new User()` | `new` 后的类型名 | 是 |
| 对象初始化器 | `new Point() { X = 1 }` | `new` + 构造 + `{ }` 字段赋值 | 是 |

### 对象/数组创建（C# 对齐）

| 形式 | 示例 | Arc | C# |
|------|------|-----|-----|
| `new T()` | `new User()` | ✅ | ✅ |
| `new T(args)` | `new Counter(42)` | ✅ | ✅ |
| `new T() { fields }` | `new Point() { X = 1, Y = 2 }` | ✅ | ✅ |
| 集合表达式 `[…]` | `int[] nums = [10, 20]` / `var v = [10, 20]` | ✅（唯一） | ✅（C# 12+） |
| `new T[] { }` / `new[] { }` / 前导类型 + `{ }` | `new int[] { 10, 20 }` 等 | ❌ 硬拒绝 | ✅（历史） |
| 裸 `{ }` 表达式 | `var v = { 10, 20 }` | ❌ | ❌ |
| 裸 struct/class 字面 | `Point { X = 1 }` / `User { Age = 1 }` | ❌（须 `new Point() { ... }`） | ❌ |

### 扩展方法（C# 对齐）

| 规则 | Arc | C# |
|------|-----|-----|
| 容器 | 仅 **`static class`** | 同左 |
| 方法 | **`public static`**；首参 **`this Type receiver`** | 同左 |
| 调用 | `receiver.Ext(args)` → **`ExtClass.Ext(receiver, args)`** | 同左 |
| 实例 `this` | 实例方法体内的 **`this`** 指当前对象；扩展首参 **`this T x`** 为普通参数修饰符，**非**实例 `this` | 同左 |
| `using` 导入 | `using N;` 将命名空间内扩展方法纳入作用域；ExtensionScope 三匹配规则（精确/前缀/末尾段）+ 同命名空间 enclosing 可见 | 同左 |
| 泛型扩展 | 支持 `static T Foo<T>(this T x)`；`unify_receiver` 接收者合一器 + 模板实例化单态化 | 同左 |
| 冲突消解 | 候选集合化 + C# 规则 1（更具体接收者）+ 规则 2（同命名空间优先）；`AmbiguousExtensionCall` 错误变体 | 同左 |

### 异步一体与数据访问（Arc 专属，优于 C#）

Arc **异步一体原则**：消除 C# EFCore 的 `ToList()`/`ToListAsync()` 双重 API 历史冗余。所有涉及 I/O 的方法仅提供异步签名，无同步副本。

**异步方法命名：**

| 规则 | 规范 |
|------|------|
| 返回 `Task`/`Task<T>` | 必须以 **`Async`** 结尾 |
| 无 I/O 的纯状态操作 | 同步命名，无 `Async` 后缀 |
| `CancellationToken` 参数 | 所有 async 方法必须提供 `CancellationToken` 重载；参数名统一为 `cancellationToken` |
| 重载模式 | `ToListAsync()` / `ToListAsync(CancellationToken ct)`，首调用委托给 `new CancellationToken()` |
| 统一签名目标 | `= default` 参数值已支持（编译器默认参数），统一为单签名 `Task<T> MethodAsync(CancellationToken cancellationToken = default)`；既有重载形态不强制回写 |

```as
// ✅ 重载
public async Task<List<T>> ToListAsync() {
    return await this.ToListAsync(new CancellationToken());
}
public async Task<List<T>> ToListAsync(CancellationToken cancellationToken) {
    cancellationToken.ThrowIfCancellationRequested();
    // I/O ...
}

// ❌ 禁止
public Task<List<T>> ToList() { ... }           // 无 Async 后缀
public List<T> ToList() { ... }                 // 同步 I/O
public async Task<List<T>> ToListAsync(CancellationToken ct = default) { ... }  // 需 = default 参数支持
```

**数据访问分层：**

| 层 | 类型 | 生命周期 | 线程安全 | 示例 |
|----|------|----------|----------|------|
| Provider | `IDbProvider` | **singleton** | 是 | `SqliteProvider`, `MongoProvider` |
| 会话 | `DbContext` | **scoped** | 否 | `AppDbContext` |
| 实体集 | `DbSet<T>` | 随 DbContext | 否 | `db.Users` |
| 查询构建 | `EntityQueryable<T>` | 链式中间态 | 否 | `db.Users.Where(...)` |

**多后端扩展：** `IDbProvider` 是核心抽象，`DatabaseKind` 枚举区分数据库类型。关系型（SQLite）与文档型（MongoDB）共用同一套 `DbContext` / `DbSet<T>` / `IQueryProvider` 接口。

```as
// ✅ 双后端共用核心接口
IDbProvider sqlite = new SqliteProvider("Data Source=app.db");       // Relational
IDbProvider mongo  = new MongoProvider("mongodb://localhost:27017"); // Document

// ✅ 查询构建（同步）与执行（异步）分离
var adults = await db.Users
    .Where(u => u.Age >= 18)        // 同步，组合表达式树
    .OrderBy(u => u.Name)           // 同步
    .ToListAsync();                 // 异步，触发 I/O

// ✅ SaveChangesAsync
db.Users.Add(new User { Name = "Bob" });
int rows = await db.SaveChangesAsync();
```

**元数据共享机制：**

```as
// ✅ DbContext 子类声明实体映射（OnModelCreating 仅首次构造时调用）
public class AppDbContext : DbContext {
    public AppDbContext(IDbProvider provider) {
        this.SetProvider(provider);
        this.SetContextTypeName("AppDbContext");  // 触发 ModelCache.GetOrBuild
    }
    protected override FrozenModel OnModelCreating() {
        FrozenModel model = new FrozenModel("AppDbContext");
        model.AddEntityMap("User", "Users");
        return model;
    }
}

// ✅ DbSet 委托 ChangeTracker（scoped 私有，零装箱 struct 操作）
public DbSet<User> Users => new DbSet<User>(
    this.ChangeTracker, this.Provider, this.TableExpression("Users"), "User");
```

**高并发安全模型：**

| 层 | 并发策略 |
|----|----------|
| `IDbProvider` | singleton，内部连接池线程安全（`IDbConnectionPool` 租约/归还） |
| `ModelCache` | 静态缓存，double-check lock 首次构建，后续无锁读取（`FrozenModel` 不可变） |
| `DbContext` | scoped，单线程使用，非线程安全（用户规范） |
| `ChangeTracker` | 随 `DbContext` scoped，单线程使用，无需锁 |
| `MaterializerCache` | 静态缓存，codegen 构造时注入，运行时只读 |
| `CompiledQueryCache` | 静态缓存，首次翻译后冻结，后续无锁读取 |

## Arc 拒绝（Rust 渗透，须消除）

| Rust-ism | 为何拒绝 | Arc 目标 |
|----------|----------|----------|
| `fn` 关键字 | C# 无 `fn`；函数用前导返回类型 | `async Task<T> name()` |
| `let` / `let mut` | C# 用 `var` 或显式类型，无 `mut` | 仅 `var` 或 `Type name`；可变性由类型系统表达 |
| `::` 路径分隔 | C# 用 `.` 作限定名 | 限定名一律 `.`：`Arc.Console` |
| `->` 函数类型语法 | C# 用 `Func<>` / 委托 | `Func<T, bool>` |
| 后缀类型注解 `name: Type` | C# 为前导类型 | `User[] users = [...]` 或 `var users = [...]` |
| `println!` / 裸 `print` | C# 经 `Console` API | `Console.WriteLine(...)` |
| `match` 关键字 | C# 用 `switch`/`case`/`break` | **`switch`/`case`/`default`/`break`**；**拒绝** `match` |
| `&` / `&mut` 借用语法 | C# 无显式借用 | **已决**：用户面无 `&T`/`&mut T`；仅 MIR/borrowck |
| snake_case 用户标识符 | C# 用 camelCase / PascalCase | 持续 lint |
| `.rs` 风格模块 `mod` / `use` | Arc 用 `namespace` / `using` | 不进入 `.as` |
| `expr` 短关键字 | C# 生态用 `Expression<>` 与 `expression` 术语 | 统一为 **`expression`** |
| 文件包含式 `using` | C# `using` 指导入命名空间/类型，非 `#include` | **全面 C# `using`** |
| 单段 `namespace X; { }` | 非 C# 标准；与 file-scoped 混淆 | **拒绝**；仅点分 file-scoped 或块 |
| `new T[]` / `new[]` / `T[] x = { }` / 裸 `{ }` | C# 历史多写法；Arc 取 C# 12 `[…]` 单一惯用法 | **`[e1, e2, ...]` 唯一**；旧写法硬拒绝 |
| `Point { X = 1 }` 无 `new` | C# 结构/类初始化须 `new Type() { }` | **`new Point() { X = 1 }`** |

## 判定流程（新增语法前）

1. C# 是否有主流惯用法？→ **优先对齐**，差异写入规范章。
2. 是否仅为 Rust 编译器实现方便？→ **拒绝**进入 `.as` 表面。
3. 是否现代语言有显著更优解？→ 写入 RFC，**不**直接在 parser 落地。
4. `crates/` Rust 代码是否影响用户可见 API？→ 必须通过 `std/` 或 CLI 暴露，且符合上述对照表。

## 与相关文档

| 文档 | 关系 |
|------|------|
| [04 词法与语法](04-lexicon-syntax.md) | 具体产生式与示例 |
| [05 类型系统](05-type-system.md) | 类型与命名规范 |
| [07 对象模型](07-object-model.md) | 扩展方法、索引器、属性 |

---

上一节：[02 构建与运行](02-build-run.md) · 下一节：[04 词法与语法](04-lexicon-syntax.md)