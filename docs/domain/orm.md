# Arc.Orm

## 概述

`Arc.Orm` 是 Arc 的对象关系映射层。它以**表达式树**承载查询意图、经 **`SqlTranslator`** 翻译为 SQL、由**方言 Provider** 适配不同数据库、以**实体物化**把行映射为强类型对象、以**连接管理**统一生命周期。它是纯标准库领域层：编译器只负责表达式树的编译期构建与类型检查，翻译逻辑全部由 `Arc.Orm` 在 Arc 语言内实现。

`Arc.Orm.SQLite` 为方言实现，与 `Arc.Orm` 平级；其余方言（`Arc.Orm.PostgreSQL`、`Arc.Orm.MongoDB` 等）平级平铺。开发者经 `Arc.Orm` 统一消费，不感知方言差异（换方言不换查询面）。

### 分层架构

| 层 | 命名空间/包 | 职责 |
|----|-------------|------|
| 查询基座 | `Arc.Linq` / `Arc.Linq.Expressions` | 表达式树构建与查询执行约定 |
| 数据库基础设施 | `Arc.Data` | `IDbConnection`/`IDbTransaction`/`IDbProvider` + `DataTable`/`DataRow`/`DataColumn` |
| ORM 核心 | `Arc.Orm` | `DbContext`、`DbSet<T>`、`SqlTranslator`、`ChangeTracker` |
| 方言 | `Arc.Orm.SQLite` 等 | 方言 Provider（如 `SqliteProvider`） |

依赖单向：`Arc.Orm` 依赖 `Arc.Linq` 与 `Arc.Data`；方言依赖 `Arc.Orm` 与 `Arc.Linq`；方言 Provider 相互平级、互不依赖。

## 快速开始

### 1. 定义实体

实体是普通类，经 `ColumnAttribute` 等特性声明列映射：

```as
using Arc;
using Arc.ComponentModel;

public class User {
    public int Id;
    [Column("name")] public string Name;
    public int Age;
}
```

### 2. 定义 DbContext

`DbContext` 是 ORM 会话基类，绑定方言 Provider 与表：

```as
using Arc.Orm;
using Arc.Orm.SQLite;

public class AppDbContext : DbContext {
    public AppDbContext() {
        this.SetProvider(new SqliteProvider("app.db"));
        this.SetContextTypeName("AppDbContext");
    }
}
```

### 3. CRUD

```as
using Arc.Orm;
using Arc.Collections;

using AppDbContext db = new AppDbContext();

// 新增
db.Set<User>().Add(new User { Name = "Alice", Age = 30 });

// 提交
int changes = await db.SaveChangesAsync(ct);

// 查询（表达式树 → SQL）
var adults = db.Set<User>()
    .Where(u => u.Age >= 18)
    .Select(u => u.Name)
    .ToList();

// 更新 / 删除（经变更追踪）
db.Set<User>().Update(user);
db.Set<User>().Remove(user);
await db.SaveChangesAsync(ct);
```

### 4. 直接执行（SQLite 方言面）

方言 Provider 也暴露连接级数据行读取：

```as
using Arc.Orm.SQLite;
using Arc.Data;

SqliteProvider provider = new SqliteProvider(":memory:");
using IDbConnection conn = provider.CreateConnection();
conn.Open();

DataTable table = conn.QueryDataRows("SELECT * FROM Users");
foreach (DataRow row in table.Rows) {
    Console.WriteLine(row["Name"]);
}
conn.Close();
```

## 核心 API

### DbContext —— ORM 会话基类

| 成员 | 说明 |
|------|------|
| `SetProvider(IDbProvider)` | 绑定方言 Provider |
| `SetContextTypeName(string)` | 设置上下文类型名（触发模型缓存） |
| `Set<T>()` | 返回 `DbSet<T>` 实体集门面 |
| `SaveChangesAsync()` / `SaveChangesAsync(ct)` | 异步提交变更 |
| `Tracker` | 变更追踪器（`ChangeTracker`） |
| `TrackAdd(entity, typeName)` | 向变更追踪器添加实体 |
| `PendingCount()` | 当前挂起变更数 |

`DbContext` 实现 `IDisposable`。

### DbSet<T> —— 实体集门面

| 成员 | 说明 |
|------|------|
| `Add(T entity)` | 标记实体为 Added |
| `Update(T entity)` | 标记实体为 Modified |
| `Remove(T entity)` | 标记实体为 Deleted |
| 查询链 | 继承自 `EntityQueryable<T>`：`Where`/`Select`/`OrderBy`/`ToList` 等 |

查询经强类型 `Expression<T>` 编译期校验属性/类型；`SqlTranslator` 运行期把表达式树翻译为 SQL 与参数列表，规避拼接注入。

### SqlTranslator —— 表达式树 → SQL

`SqlTranslator.Translate(expression) → (sql, parameters)` 按 NodeType 分派遍历表达式树。常见翻译规则：

| 表达式 | 翻译结果 |
|--------|----------|
| `u.Field >= N` | `Field >= N` |
| `u.Field == "str"` | `Field = 'str'` |
| `u.Age >= 18 && u.Active` | `Age >= 18 AND Active` |
| 捕获变量（外部变量引用） | 参数化占位符 `@pN`（规避注入） |

方言差异（如 PostgreSQL `ILIKE`、SQL Server `TOP`）收敛在方言 Provider 内，核心 `SqlTranslator` 产出通用骨架、方言补齐语法差异。

### ChangeTracker 与实体状态

`ChangeTracker` 维护实体状态（`EntityState`）：`Add` → Pending → `SaveChanges` 时落库。实体物化把行映射为强类型实体 `T`（经 `IQueryProvider.Execute<T>`），列映射用 `ColumnAttribute` 等特性声明（自动属性）。

### 方言 Provider

| 方言 | 命名空间 | 说明 |
|------|----------|------|
| SQLite | `Arc.Orm.SQLite` | 首方言；`SqliteProvider` + `SqliteConnection` + `SqliteTransaction` |
| PostgreSQL | `Arc.Orm.PostgreSQL` | 平级方言包 |
| MongoDB | `Arc.Orm.MongoDB` | 平级方言包 |

`SqliteProvider` 实现 `IDbProvider`（`Kind`/`ProviderName`/`CreateConnection()`/`ExecuteDataRows(Expression)`），支持 `:memory:` 与文件连接、连接级事务、`?` 参数绑定。

## 边界

- **表达式树机制**（`Expression<T>` 构建、Provider、Enumerable/Queryable）见规范章；本册只讲 ORM 如何消费翻译。
- **JSON/XML/YAML 序列化**见 `Arc.Text` 家族；**Protobuf** 见 `Arc.Text.Protobuf`。
- **DI / 连接生命周期原语**见 [di.md](di.md)。
- **编译器核心零领域能力**为架构红线；SQL 翻译逻辑全部落在 `std/` 领域层。
- **完整 SQL Provider 全面**（新方言、Provider 级事务、完整实体 codegen 物化）**当前不提供**；方言 Provider 平级平铺、按需另立。

---

上一节：[ai-inference.md](ai-inference.md) · 下一节：[web.md](web.md)