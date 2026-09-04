# RFC 020 标准库架构与拆分

## 背景

Arc 标准库以 Arc 源码组织于 `std/`，按 `.as` 模块划分。编译器不内嵌 std 实现；用户程序通过 `using` 显式引用 std 模块。核心（BCL）与领域（协议/UI/ORM 等）能力需在物理目录、命名空间、依赖方向与可见性四个维度上给出统一治理骨架，保证能力显式、依赖单向、内部面隔离。

## 设计决策

### 子库拆分与命名空间

标准库按 C# 命名空间惯例组织，每个子库根一份 `arc.toml`。包图加载时校验包名与目录一致（禁止嵌套方言包根，如 `std/Orm/SQLite/`）。

| 类别 | 命名空间 | `using` 形式 | 成员 |
|------|----------|-------------|------|
| 根命名空间 | `Arc` | `using Arc;` | `Console`、`Math`、`Array`、`Tensor<T>`、`Task<T>`、`EventLoop`、`Convert`、`Guid`、`DateTime`、`TimeSpan`、`BitConverter`、`Buffer`、`Environment`、`IComparable<T>`、`IFormatProvider`、`IFormattable` |
| 子命名空间 | `Arc.Collections` | `using Arc.Collections;` | `List<T>`、`Dictionary<K,V>`、`HashSet<T>`、`Span<T>`、`Queue<T>`、`Stack<T>`、`LinkedList<T>`、`SortedSet<T>`、`SortedDictionary<K,V>`、`Collection<T>`、`ReadOnlyCollection<T>`（含 `Concurrent/` 并发面） |
| 子命名空间 | `Arc.IO` | `using Arc.IO;` | `File`、`Directory`、`Path`、`Stream`、`FileStream`、`MemoryStream` |
| 子命名空间 | `Arc.Linq` | `using Arc.Linq;` | `Enumerable`、`IQueryProvider`、`Queryable`、表达式树节点 |
| 子命名空间 | `Arc.Text` | `using Arc.Text;` | `StringBuilder`、`Encoding`、`Base64`、`Hex` + `Json/`、`Xml/`、`Yaml/`、`Serialization/`、`Protobuf/` |
| 子命名空间 | `Arc.Diagnostics` | `using Arc.Diagnostics;` | `Stopwatch` |
| 子命名空间 | `Arc.Runtime` | `using Arc.Runtime;` | `Assembly`、`AssemblyLoadContext`、`IAssemblyLifecycle`、`AppContext`（运行时/产物面，语义见 [017](017-build-artifacts-packages.md)） |
| 独立子库 | `Arc.Data` | `using Arc.Data;` | 数据库基础设施：`IDbConnection`、`IDbTransaction`、`IDbProvider`、`DatabaseKind`、`IDataReader`、`IDbConnectionPool`、`DataTable`/`DataRow`/`DataColumn`/`ColumnType` |
| 子命名空间 | `Arc.Orm` | `using Arc.Orm;` | `DbContext`、`SqlTranslator`、实体映射契约 |
| 方言子库 | `Arc.Orm.SQLite` 等 | `using Arc.Orm.SQLite;` | `SqliteProvider`（平级 `std/Orm.SQLite/`） |
| 领域子命名空间 | `Arc.Net` | `using Arc.Net;` | `HttpClient`、`WebSocketClient`、`TcpClient`、`TcpListener`、`NetworkStream`、`UdpClient` |
| 领域子库 | `Arc.Net.P2P` | `using Arc.Net.P2P;` | P2P 栈：`PeerId`、`Multiaddr`、`PeerStore`、`Topology`、`Transport` |
| 领域子库 | `Arc.Security` | `using Arc.Security;` | `CSPRNG`、Hash/HMAC（含 `Arc.Security.Cryptography`：`AesGcm`/`Rsa`/`ECDiffieHellman`/`X509Certificate2`/`TlsClientSession`） |
| 领域子库 | `Arc.DI` | `using Arc.DI;` | `ServiceCollection`、`IServiceProvider`、`IServiceScope`、`ServiceLifetime` |
| 子命名空间 | `Arc.Globalization` | `using Arc.Globalization;` | `CultureInfo`、`NumberFormatInfo`、`DateTimeFormatInfo` |
| 领域子命名空间 | `Arc.Drawing` | `using Arc.Drawing;` | `RgbColor`、`PixelFormat`、`ImageFormat`、`Bitmap`、`ImageDecoder`、`Font`、`QrCodeWriter`、`QrCodeReader`、`BarcodeWriter`、`BarcodeReader` |
| 领域子库 | `Arc.AI` | `using Arc.AI;` | 会话、工具宿主 |
| 领域子库 | `Arc.UI` | `using Arc.UI;` | 声明式 GUI 框架 |

**规则**：基础能力类归属根命名空间 `Arc`，与 C# 的 `System` 一致；需独立组织的能力放入子命名空间。`using Arc;` 导入根命名空间下所有直接成员；`using Arc.Collections;` 导入集合子命名空间成员。

### 目录布局

```
std/
├── Arc/                    # Arc（隐式引入）
│   ├── arc.toml
│   ├── Console.as · Math/ · Convert.as · Guid.as · DateTime.as · …
│   ├── Collections/        # namespace Arc.Collections（+ Concurrent/）
│   ├── IO/                 # namespace Arc.IO
│   ├── Linq/               # namespace Arc.Linq（Enumerable + Queryable / Expression）
│   ├── Text/               # namespace Arc.Text（+ Json/Xml/Yaml/Encoding/Base64/Hex/Protobuf）
│   ├── Tasks/              # Task / EventLoop 等
│   ├── Diagnostics/        # Stopwatch 等
│   ├── Globalization/      # namespace Arc.Globalization
│   └── Reflection/         # typeof / Type 元数据面
├── Net/                    # Arc.Net（显式依赖）
├── Net.P2P/                # Arc.Net.P2P（显式依赖）
├── Security/               # Arc.Security（显式依赖）
├── Data/                   # Arc.Data（数据库基础设施独立库）
├── Orm/                    # Arc.Orm 抽象框架层（平级包根）
├── Orm.SQLite/             # Arc.Orm.SQLite（平级方言子库）
├── Orm.PostgreSQL/         # Arc.Orm.PostgreSQL（平级）
├── Orm.Mongo/              # Arc.Orm.Mongo（平级）
├── AI/                     # Arc.AI
├── DI/  ·  UI/             # Arc.DI · Arc.UI 领域子库
├── Drawing/                # Arc.Drawing（独立图像处理面）
└── QIF/                    # 质检框架
```

**命名**：树与 `arc.toml` / `using` 为产品缩写全大写（`Arc.Orm.SQLite`、`Arc.Orm.PostgreSQL`）；以树为准，语法面与散文命名不偏离目录。

### 层与依赖

| 层 | 目录 / 命名空间 | 职责 |
|----|-----------------|------|
| 核心（BCL） | `std/Arc/` 下 Collections、IO、Text、Tasks、Diagnostics、Math、Linq Enumerable、Globalization、Reflection | 基础能力，契约清晰 + 门禁回归 |
| 领域 | `std/Net/`、`std/Security/`、`std/Data/`、`std/Orm*`、`std/DI/`、`std/UI/Core/`、`std/AI/`、`std/Drawing/`、`std/Net/P2P/` | 协议与领域翻译，显式依赖 |

依赖方向单向：核心包零依赖领域包；领域包依赖核心包；方言子库依赖抽象框架层。数据库/ORM 翻译遵循「编译器核心零领域能力」红线——编译器仅提供通用机制（表达式树构建、类型检查、代码生成），领域翻译由 std 以 Arc 语言实现。

### internal 边界纪律

可见性以**库设计意图**为唯一依据：判断"该类型是不是用户使用该库时的自然操作对象/契约"——是 → `public`（即使当前无示例引用）；否（框架内部实现所需）→ `internal`（即使被某个 public 签名引用，该签名本身即泄漏面，应一并收窄）。不以"是否被引用"决定暴露。

| 准则 | 判定 |
|------|------|
| R0 · C# 标准参照 | 对标 .NET/C# 同类 public → `public`；对标 internal（布局算法/渲染管线/调度器/注册表/`Runtime*` 实现/runner/桥/协议握手）→ `internal` |
| R1 · 用户面证据（佐证） | examples / UnitTest / 文档用户代码直接引用 → 佐证 `public`；无引用 ≠ internal |
| R2 · 工具链注入面 | 编译器生成代码注入用户程序并**按名字引用**的类型 → `public`（工具链注入契约面） |
| 默认 | 新类型默认无修饰符（`internal`），仅当满足 R0/R1/R2 之一才显式 `public` |

**机制基础**：

- `internal` = 同一包可见；跨包访问由 typeck 硬拒绝（成员 + `internal class` 类型级）。
- 顶层无修饰符默认 `internal`；成员级 `internal` 可用。
- `std/` 每个子目录是独立包，`std/Arc` 是隐式基座包。
- **InternalsVisibleTo**（对标 C# `[assembly: InternalsVisibleTo]`）：由被访问方包在 `arc.toml` 声明 `internals_visible_to = ["包名", ...]`，列出的包可访问其 `internal`；未列出的包仍被硬拒绝。测试程序经此验证 std 包的 internal 实现，普通用户程序仍被拒——解决"internal 不可测"矛盾。
- **包边界即隔离墙**：`std/UI/Core` 内的 `internal` 类型对用户程序与其他 std 包均不可见，强于纯命名空间的 `Arc.UI.Internal`。
- **CompilerServices 模式**：编译器需要 public 的类型显式放入 `Arc.CodeGeneration` / `Arc.UI.Internal` 命名空间，与用户面物理隔离，`internal` 修饰符补上类型级强制。
- **抽象元数据面与实现解耦**：`Type`/`MethodInfo`/`FieldInfo`/`PropertyInfo` 为 public 抽象面；`RuntimeType` 等为工具链注入面，语义定位 internal（`typeof` 降级构造需要 public，属工具链注入契约）。

**典型裁决**：`ServiceProvider`/`ServiceScope` 为容器具体实现，用户经 `IServiceProvider`/`IServiceScope` 接口使用 → `internal`；`ServiceCollection`/`IServiceProvider` 等用户入口 → `public`。P2P 协议内部机制（`StreamMuxer`/`HeartbeatService`/`ConnectionMonitor`/内部拓扑实现）→ `internal`；用户操作结果句柄（`STUNResult`/`ICECandidate`/`PeerConnectionState`）→ `public`。数据库 Provider 契约（`IDbProvider`/`IDataReader`/`DataTable`）→ `public`（自定义 Provider 扩展面）。`[Builtin]` 拦截桩按符号名拦截，可 `internal`（同包调用即满足）。

### 编译器与标准库边界

| 层 | 职责 | 示例 |
|----|------|------|
| 编译器 `crates/*` | 词法、类型、`ExpressionIr` 树化、运行时构造代码生成 | `crates/ast`、`crates/codegen` |
| 标准库 `std/` | 运行时 API、I/O 封装、LINQ 接口、`Expression` 类层次、`SqlTranslator` | `IQueryProvider.Execute`、`SqlTranslator.Translate` |

模块解析（import/using）见语言命名空间规范。新增公开 API 须更新对应规范小节、添加测试；语义级变更提交 RFC；新类型默认 `internal`，按 R0/R1/R2 判定后再公开。

## 边界

- 本文档只定义**架构、分域、依赖与可见性治理**；各子库的具体类型面与 API 详见 [021 集合、IO 与文本](021-collections-io-text.md) 至 [030 Protobuf 二进制序列化](030-protobuf.md)。
- 集合/IO/文本库见 [021](021-collections-io-text.md)；异步任务与序列化家族见 [022](022-async-linq-serialization.md)；Math/Tensor/DI 见 [023](023-math-tensor-di.md)；并发集合见 [024](024-concurrent-collections.md)；网络协议层见 [025](025-networking.md)；加密安全见 [026](026-cryptography-security.md)；本地化资源见 [027](027-localization-resources.md)；反射面见 [028](028-type-reflection.md)；图像图形见 [029](029-imaging-graphics.md)；Protobuf 见 [030](030-protobuf.md)。
- 语言级集合表达式、字符串与数值语义属语言核心，不在此篇（见 [007 集合、字符串与数值](007-collections-strings-numerics.md)）。
- 领域翻译（ORM/SQL、Web 框架、推理引擎）的实现细节不在标准库架构篇内。

---

上一节：[019 自举路线图](019-self-hosting.md) · 下一节：[021 集合、IO 与文本](021-collections-io-text.md)