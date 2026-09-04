# 13 标准库架构

Arc 标准库以 Arc 源码组织于 `std/`，按 `.as` 模块划分。编译器不内嵌 std 实现；用户程序通过 `using` 显式引用 std 模块。

## 核心 vs 领域

| 层级 | 目录 / 命名空间 | 说明 |
|------|-----------------|-------------|
| **核心（L2）** | `std/Arc/` 下：Collections、IO、Text、Tasks、Exceptions、Console/Math/基元、LINQ **Enumerable** 扩展 | 契约清晰 + 回归测试；facade 须有 runtime/codegen 回归，禁止半成品冒充完备 |
| **领域（L3）** | `std/Orm/`、`std/UI/Core/`、`std/Net/`（协议扩展）、`std/Security/`（深化）、`std/QIF/` **增强**、`std/DI/` 高级特性 | 各子库独立有边界演进；**≠** 假开 Provider 全家桶；仍禁 C 档冒充 / 编译器嵌领域 / 双轨粉饰；完整 SQL Provider / UI 扩张为独立能力面 |

**Queryable SQL / Orm Provider** 属领域翻译，遵守「编译器核心无领域能力」红线；实现可保留骨架。未实现的 Provider 面必须显式抛 `NotImplementedException`（诚实边界；禁空 List / null 假面）；**禁**假开 Provider 全家桶。

## Arc.Net / Arc.Net.P2P 能力面与诚实边界

Arc.Net（`std/Net/`，`using Arc.Net;`）与 Arc.Net.P2P（`std/Net.P2P/`，`using Arc.Net.P2P;`）为显式依赖的领域子库。各能力面按最终设计锁定 API 与诚实边界：未实现的面返回真 `NotImplementedException`（禁返回空 / `null` 假面冒充实现）；本地闭环通过 ≠ 实网 / 生态互操作，验收通过前禁宣称。

| 面 | 设计 API 与能力边界 |
|----|------------------|
| Uri / UriBuilder / Cookie | 纯逻辑类型；须显式 `Arc.Net` 依赖 |
| TcpClient / TcpListener | 同步环回客户端 / 服务器：Start→Connect→Accept→Send/Receive；为同步 I/O 面（C 级 async 面仍见），非完备 socket API |
| `NetworkStream` | byte[] 缓冲面：`Read(byte[], int, int)` / `Write(byte[], int, int)`（读返回实际字节数 / EOF→0 / 部分读语义；写全量、失败抛 `IOException`），client↔server 双向环回闭环；诚实边界：底层 string 传输原语 NUL 终止——含内部 0x00 的二进制对传不在本面 |
| HttpClient（HTTP/1.1） | 本地 HTTP/1.1 请求/响应闭环（请求行 / 状态行 / 正文断言）；chunked（含 chunk 扩展与 trailer 头解码）、keep-alive 复用（同一 TCP 连接连续请求）、POST + Content-Length、状态行/头部规范解析（`Version` / 原因短语 / 大小写不敏感头名 / obs-fold）；诚实边界：实网、TLS、HTTP/2、WebSocket、管线化由各自能力面承载 |
| WebSocket 客户端（ws:// · RFC 6455） | `std/Net/WebSocket/`（`WebSocketClient` + 消息/状态/opcode）；RFC 6455 Upgrade 握手（`Sec-WebSocket-Key/Accept`）；客户端掩码强制（真实 XOR 解掩码）；文本帧往返 / Ping/Pong / Close 握手；诚实边界：ws:// 无 TLS（wss 为独立安全面）；permessage-deflate / 分片续帧 / 服务器端不在本面 |
| HttpListener | 不在本面（不提供；禁宣称已交付） |
| TLS（mbedTLS） | `TlsClientSession` + `X509Certificate2` facade + `rt_crypto_tls_*` / `rt_crypto_x509_*` ABI：全握手 + 加密往返 + ALPN `h2`（本地 mbedTLS 测试服务器）；诚实边界：0-RTT / 会话恢复 / 完整证书链校验 / 服务器端 / 实网互操作不在本面 |
| HTTP/2（h2c） | 帧层 + HPACK + 并发多流（本地 h2c 服务器）；诚实边界：h2 via ALPN 随 HttpClient 版本协商落地；服务器端不在本面 |
| HTTP/3（QUIC） | `quic_tls13` / `http3_quic` / `http3_codec` 底座（ngtcp2 + OpenSSL QUIC-TLS）：本地闭环；诚实边界：0-RTT / 连接迁移 / QPACK 动态表完整 / 拥塞控制 / 服务器端 / 实网互操作不在本面——禁宣称「HTTP/3 已实现」 |
| WebSocket wss | 独立安全面（wss）未实现 |
| WebTransport | `std/Net/WebTransport/`：W1 over HTTP/2 + W2 over HTTP/3；协议为 IETF 草案（draft-ietf-webtrans-http3·http2），仅本地闭环验收，禁任何「WebTransport 已实现」宣称 |
| `Arc.Net.P2P` | `TCPTransport.DialAsync` 真实最小实现：TCP 连接 + Noise XK initiator 会话（复用 Multiaddr / `rt_noise_*` ABI）；`Multiaddr.GetValue` / `InMemoryPeerStore` / `FullMeshTopology` 可证伪；诚实边界：ListenAsync / Relay / DHT / ICE / 其余 Transport 显式抛 `NotImplementedException`（禁假绿）；STUN 面为假面 stub（恒返回空 / PortRestrictedCone）——比显式未实现更危险，须替换为诚实实现 |

**成熟化方向**：Arc.Net.P2P 成熟化对标 go-libp2p 能力面（Noise XX · QUIC / WebSocket / 流复用 · DHT 协议化 · STUN 假面替换 · gossipsub / relay / ICE / AutoNAT / Identify / ping / ResourceMgr 等）；验收以生态互操作级为核心判据——本地闭环 ≠ 生态互操作，验收通过前禁宣称。

## 命名空间约定

Arc 标准库遵循 C# 命名空间惯例：

| 类别 | 命名空间 | `using` 形式 | 成员 |
|------|----------|-------------|------|
| 根命名空间 | `Arc` | `using Arc;` | `Console`、`Math`、`Array`、`Tensor<T>`、`Task<T>`、`EventLoop`、`IComparable<T>`（**无**根 `Window` stub） |
| 子命名空间 | `Arc.IO` | `using Arc.IO;` | `File`、`Directory`、`Path`（与 C# `System.IO` 对齐） |
| 核心命名空间 | `Arc.Collections` | `using Arc.Collections;` | `List<T>`、`Dictionary<K,V>`、`SortedDictionary<K,V>`、`LinkedList<T>`、`SortedSet<T>`、`Collection<T>`、`ReadOnlyCollection<T>`、`ListEnumerator<T>` |
| 子命名空间 | `Arc.Linq` | `using Arc.Linq;` | `IQueryProvider`、`Queryable`、表达式树节点 |
| 独立子库 | `Arc.Data` | `using Arc.Data;` | **数据库基础设施**：`IDbConnection`（`State`/`ConnectionTimeout`/`Database`）、`IDbTransaction`（`Connection`/`IsolationLevel`/同步·异步 Commit/Rollback）、`IDbProvider`（`DatabaseKind` 粗分类 + `ProviderName` 开放名）、`DatabaseKind`/`ConnectionState`/`IsolationLevel` + 通用读取/连接池契约 `IDataReader`/`IDbConnectionPool` + C#-aligned 数据面 `DataTable`/`DataRow`/`DataColumn`/`ColumnType`（`Int`/`Long`/`Double`/`Bool`/`String`/`DateTime`/`Guid`；`DataRow` 全类型访问器 + NULL 语义 + 类型校验；`DataTable` 行/列集合操作与枚举；独立库 `std/Data/`；ORM 框架层 `Arc.Orm` 依赖之） |
| 子命名空间 | `Arc.Diagnostics` | `using Arc.Diagnostics;` | `Stopwatch`（对标 C# `System.Diagnostics.Stopwatch`） |
| 子命名空间 | `Arc.Orm` | `using Arc.Orm;` | `DbContext`、`SqlTranslator`、实体映射契约 |
| 方言子库 | `Arc.Orm.SQLite` | `using Arc.Orm.SQLite;` | `SqliteProvider`（平级 `std/Orm.SQLite/`） |
| 领域子命名空间 | `Arc.Drawing` | `using Arc.Drawing;` | `RgbColor`、`PixelFormat`、`ImageFormat`、`Bitmap`、`ImageDecoder`、`Font`、`QrCodeWriter`、`QrCodeReader`、`QrCodeErrorCorrection`、`BarcodeWriter`、`BarcodeReader`（独立于 UI 帧内绘制 `Arc.UI.Rendering` DrawList IR） |

**规则**：基础能力类（`Console`、`Math`、`Tensor`、`Task`、`EventLoop` 等）归属根命名空间 `Arc`，与 C# 的 `System` 命名空间一致。集合与查询等需独立组织的能力放入子命名空间。`using Arc;` 导入根命名空间下所有直接成员；`using Arc.Collections;` 导入集合子命名空间成员。

## 目录布局

`std/` 按平级子库组织；每个子库根一份 `arc.toml`。包图加载时校验包名与目录一致（禁止嵌套方言包根，如 `std/Orm/SQLite/`）。

```
std/
├── Arc/                    # Arc（隐式引入）
│   ├── arc.toml
│   ├── Console.as · Convert.as · Guid.as · DateTime.as · …  # namespace Arc
│   ├── Math/               # Math/Tensor/…（仍 namespace Arc）
│   ├── Collections/        # namespace Arc.Collections（+ Concurrent/）
│   ├── IO/                 # namespace Arc.IO
│   ├── Linq/               # namespace Arc.Linq（Enumerable 契约 + Queryable / Expression）
│   ├── Text/               # namespace Arc.Text（+ Json/Xml/Encoding/Base64）
│   ├── Tasks/              # Task / EventLoop 等
│   ├── Diagnostics/        # Stopwatch 等
│   └── …
├── Net/                    # Arc.Net（显式依赖）
├── Net.P2P/                # Arc.Net.P2P（显式依赖；未实现面显式 NotImplementedException）
├── Security/               # Arc.Security（显式依赖）
├── Orm/                    # Arc.Orm 抽象框架层（平级包根）
│   ├── arc.toml
│   ├── SqlTranslator.as
│   ├── DbContext.as
│   └── …
├── Orm.SQLite/             # Arc.Orm.SQLite（平级方言子库；非 std/Orm/SQLite/）
│   ├── arc.toml
│   └── SqliteProvider.as
├── Orm.PostgreSQL/         # Arc.Orm.PostgreSQL（平级方言子库；Provider 未落地，仅保留包骨架，无 .as）
├── Orm.Mongo/              # Arc.Orm.Mongo（平级方言子库；Provider 未落地，显式 NotImplementedException）
├── AI/                     # Arc.AI（Host/Session/Wiki + AIToolSet/沙箱）
├── DI/ · UI/               # L3 领域子库
├── Drawing/                # Arc.Drawing（Color/Bitmap/Image/Font/QrCodeWriter/QrCodeReader/BarcodeWriter/BarcodeReader；运行时后端 crates/runtime-drawing/）
├── QIF/                    # Arc.QIF（L1 稳定面 = 验证底座；L2–L7 增强为独立能力面）
```

**包名大小写**：树与 `arc.toml` / `using` 为 `Arc.Orm.SQLite` · `Arc.Orm.PostgreSQL`（产品缩写全大写）；**以树为准**，勿为「对齐散文」改 `using`/`namespace` 破编译。

## 模块职责

| 模块 | 命名空间 | 职责 |
|------|----------|------|
| `Console` | `Arc` | `Console.WriteLine`；lowering 至 `rt_println` |
| `Array` | `Arc` | `Copy`/`Clear`/`Reverse`；`int[]` 的 `IndexOf`/`LastIndexOf`/`Resize`；见 [12 运行时 ABI](12-runtime-abi.md) |
| `File` | `Arc.IO` | `File.ReadAllText` / `WriteAllText` / `Exists` / `Delete` / `AppendAllText` / `Copy` / `Move` / `ReadAllBytes` / `WriteAllBytes` / `ReadAllLines`；lowering 至 `rt_file_*` / `rt_read_file` / `rt_write_file` |
| `Directory` | `Arc.IO` | `Directory.CreateDirectory` / `Exists` / `Delete` / `GetFiles`（含 searchPattern）/ `GetDirectories`；lowering 至 `rt_dir_*` |
| `Path` | `Arc.IO` | `Path.Combine` / `GetDirectoryName` / `GetFileName` / `GetExtension` / `ChangeExtension` / `HasExtension` / `GetTempPath`；lowering 至 `rt_path_*` |
| `Window` / `Application` | `Arc.UI` | 不提供根命名空间 `std/Window.as`；`Arc.UI.Components.Window` + `Application` 见 [UI 骨架诚实](#ui-l3-skeleton-honesty)；显示链路不在本面 |
| `UI/` | `Arc.UI` | L3 声明式 GUI 骨架：`Thickness`/`LayoutSize`/DP 元数据；禁框架扩张冒充完备 |
| `Math` | `Arc` | `Math.Sqrt`/`Floor`/`Clamp`/`PI`/`CopySign`/`Cbrt`/`Hypot`/`IEEERemainder` 等诚实子集；**LLVM intrinsic / libm 直射**，无 `rt_math_*` |
| `Convert` | `Arc` | `ToInt32`/`ToInt64`/`ToDouble`/`ToBoolean`/`ToString` + `ToByte`/`ToUInt32`/`ToUInt64`/`ToChar`；进制 `ToInt32(string, fromBase)`/`ToString(int, toBase)`（仅 2/8/10/16）；**Base64 单一惯用法 = `Arc.Text.Base64`**（不在 Convert 双轨） |
| `BitConverter` / `Buffer` | `Arc` | BitConverter：主机端序 `IsLittleEndian()`/`GetBytes(int\|long\|float\|double)`/`ToInt32`/`ToInt64`/`ToSingle`/`ToDouble`（`rt_bitconverter_*`）+ 位型重释 `SingleToInt32Bits`/`Int32BitsToSingle`/`DoubleToInt64Bits`/`Int64BitsToDouble`（codegen bitcast 内建）；Buffer：`BlockCopy(byte[])`→`rt_array_copy`；short/bool 与任意 Array 字节拷贝不在本面 |
| `Guid` / `DateTime` / `TimeSpan` | `Arc` | Guid：Parse/TryParse/ToString(D\|N\|B\|P)/`ToByteArray`/`FromByteArray`（.NET 混合端序；**无** `Guid(byte[])` ctor）；DateTime：部件/`ToString` 格式子集/Parse·TryParse/SpecifyKind；TimeSpan：算术 + Parse·TryParse（含纯天数整数；与 ToString 往返） |
| `Types/Random` | `Arc.Types` | 伪随机 LCG（`Next`/`NextBytes`/`NextDouble`/`NextInt64`；**非** CSPRNG）；同 seed 可复现；`NextBytes` = 每字节 `Next() % 256`；xoshiro/`Shared` 不在本面 |
| `Tensor<T>` | `Arc` | 张量 facade；`rt_tensor_*` ABI；**禁止运算符重载**，用 `Tensor.Add(a,b)` |
| `Task<T>` | `Arc` | 异步计算句柄；状态机由 codegen 生成 |
| `EventLoop` | `Arc` | 事件循环；`poll_events`/`run`/`should_close` |
| `char`（分类/大小写） | `Arc`（内置基元） | `char.IsDigit`/`IsLetter`/`IsWhiteSpace`/`IsUpper`/`IsLower`/`ToUpper`/`ToLower` → `rt_char_*`（ASCII/`ctype` 子集；扩展 Unicode 不在本面） |
| `Environment` | `Arc` | Get/Set 环境变量、NewLine、ProcessorCount、Platform/`Is*`、MachineName/UserName 等；`rt_env_*`；**计时单一惯用法 = Stopwatch**（无 `TickCount*`）；不提供 `GetFolderPath`/`ExpandEnvironmentVariables`/`ProcessId`/`ProcessPath` |
| `Diagnostics/Stopwatch` | `Arc.Diagnostics` | 高精度间隔测量（C# `Stopwatch`）；`rt_stopwatch_*`；**计时单一惯用法**（无 `Environment.TickCount*`） |
| `IComparable<T>` | `Arc` | 泛型约束基础接口；`CompareTo(T other) -> int`；基元类型由编译器内置视为已实现 |
| `Collections/List<T>` | `Arc.Collections` | 动态数组；C# 索引器 `list[i]`（`get_Item`/`set_Item` → 直访 buffer）；`rt_list_*` |
| `Collections/Dictionary<K,V>` | `Arc.Collections` | 关联表；C# 索引器 `dict[k]`（`get_Item`/`set_Item` → `rt_dict_*`） |
| `Collections/SortedDictionary<K,V>` | `Arc.Collections` | 排序字典最小面；`rt_sorted_dict_*`；标量 inttoptr 装箱 |
| `Collections/LinkedList<T>` | `Arc.Collections` | 双向链表最小面；`rt_linked_list_*`；节点为不透明句柄透传 |
| `Collections/SortedSet<T>` | `Arc.Collections` | 排序集合最小面；`rt_sorted_set_*`；标量 inttoptr 装箱 |
| `Collections/Collection<T>` | `Arc.Collections` | ObjectModel 集合包装最小面；委托 `List<T>` |
| `Collections/ReadOnlyCollection<T>` | `Arc.Collections` | 只读 List 包装最小面；ctor/Count/索引器/Contains；无独立 ABI |
| `Linq/Enumerable` | `Arc.Linq` | `Where`、`Select`、`Any`/`Count`/`First`/`FirstOrDefault` 等 Enumerable 面（MIR 展开；空类契约） |
| `Text/Json` | `Arc.Text.Json` | Writer（数组/嵌套/`EscapeForwardSlash`）、Reader（Skip、bool）、`Serialize` + `Deserialize(string, IJsonDeserializable)` + 泛型 `Deserialize<T>`（`where T : IJsonDeserializable, new()`，依赖 RFC 004 约束解锁）+ 手写 `IJsonDeserializable.ReadJson`；**禁**注解/源生成 |
| `Text/Xml` | `Arc.Text.Xml` | Writer/Reader（`GetAttribute`、EmptyElement 诚实反转义）/`Serialize` + 手写 `IXmlDeserializable.ReadXml`；**禁** `Deserialize<T>`/注解 |
| `Text/Yaml` | `Arc.Text.Yaml` | **门面统一 + DOM 优先**：`YamlSerializer.Parse`→`YamlNode` 文档树 / `Serialize(YamlNode[, options])`；`YamlNode` 整树读取（Get/GetString/GetBoolean/GetInt32/GetItems）承载 Agent Skills frontmatter 底座；**禁** `Deserialize<T>`/注解/流式 `IYamlSerializable`（见[序列化家族约定](#序列化家族统一约定json--xml--yaml)） |
| `Linq/provider` | `Arc.Linq` | Queryable 路径；**Provider 接口与 SQL 等翻译实现** |
| `Linq/Expressions/nodes` | `Arc.Linq` | 与 `crates/expr` IR 对应的节点类型声明 |
| `Orm/SqlTranslator` | `Arc.Orm` | ORM 抽象层 SQL 翻译；运行时遍历 `Expression` 对象树 |
| `Orm.SQLite/SqliteProvider` | `Arc.Orm.SQLite` | SQLite Provider；运行时翻译 SQL |

## 序列化家族统一约定（Json / Xml / Yaml）

开发者体验一致：`Arc.Text.{Json|Xml|Yaml}` 三门面统一命名与形状。

| 层 | 约定 |
|----|------|
| **门面类名** | `XxxSerializer`（`JsonSerializer` / `XmlSerializer` / `YamlSerializer`）；方法形状 `Serialize(value)` / `Serialize(value, options)`，读路径 `Deserialize`/`Parse` |
| **选项类** | `XxxSerializerOptions`，统一 `Default` 静态属性；`XxxWriterOptions` / `XxxReaderOptions` 归低层 |
| **低层读写** | `XxxReader` / `XxxWriter` + 各自 `XxxWriterOptions` / `XxxTokenType` |
| **获取读/写** | 读路径 `Deserialize(text, IXxxDeserializable)` 就地填充；泛型 `Deserialize<T>` 为 JSON 家族专属（依赖 RFC 004 约束解锁），注解 / 源生成不纳入门面约定 |

**诚实差异（文档化，非伪装一致）**：Json/Xml 为**契约优先**——`IXxxSerializable.WriteXxx(XxxWriter)` / `IXxxDeserializable.ReadXxx(XxxReader)` 流式接口，类型手写序列化钩子。Yaml 为 **DOM 优先**——YAML 缩进结构天然适合整树读操作，故以 `YamlNode` 文档树为一等公民（`YamlSerializer.Parse`/`Serialize`），**不**引入流式 `IYamlSerializable`。门面类名、方法形状、选项类命名仍与家族统一；选项字段仅保留对该格式有意义的成员（如 Yaml 无紧凑模式，故无 `WriteIndented`，仅 `IndentChars`）。

## 编译器与标准库边界

| 层 | 职责 | 示例 |
|----|------|------|
| 编译器 `crates/*` | 词法、类型、`ExpressionIr` 树化、运行时构造代码生成 | `crates/ast`（`expr_tree.rs`）、`codegen` |
| 标准库 `std/` | 运行时 API、I/O 封装、LINQ 接口、`Expression` 类层次、`SqlTranslator` | `IQueryProvider.Execute`、`SqlTranslator.Translate` |

**Queryable 的 SQL 翻译采用编译期树化 + 运行时翻译双阶段**：typeck 阶段构建 `ExpressionIr`，codegen 生成运行时构造 `Expression` 对象树的代码；运行时由 `std/Orm/SqlTranslator.as`（Arc 实现）遍历 `Expression` 对象树生成方言 SQL；具体数据库提供程序（`SqliteProvider` 等）属于平级方言子库（如 `std/Orm.SQLite/`），实现 `IQueryProvider` 并提供运行时连接管理。

模块解析（import/using）见 [04 词法与语法](04-lexicon-syntax.md)。

## 泛型约束系统

Arc 支持 C# `where` 子句完整语义：

- **接口约束** `where T : IInterface`（含泛型接口 `IComparable<T>`）
- **基类约束** `where T : BaseClass`
- **`class` 引用类型约束**（`is_reference_type` 判定；string/数组/class 满足，排除基元/struct）
- **`struct` 值类型约束**（`is_value_type` 判定；基元/struct 满足，排除 string/class）
- **`new()` 构造约束**（`NominalType.constructors` 元数据 + CtorSig；值类型隐式满足；引用类型须有 public 无参构造；`new()` 必须是同 param 最后一个约束——C# 规范强制，由 `validate_new_constraint_last` 校验）
- **多约束组合** `where T : A, B`（Parser lookahead 区分新 param 与同 param 下一约束；typeck 层零改动——多 TypeConstraint 共享 param 天然全部校验）

约束在泛型实例化时由 typeck 校验（`check_constraints`），失败抛 `ConstraintNotSatisfied`。

### 约束接口

`IComparable<T>`（`std/IComparable.as`，命名空间 `Arc`）是泛型约束的基础接口：

```as
public interface IComparable<T> {
    int CompareTo(T other);
}
```

### 约束校验规则

typeck 在 `instantiate_generic_class` / `instantiate_generic_fn` 中调用 `check_constraints`（`crates/typeck/src/checker/check_generics.rs`），对每个约束 `T : Bound`：

1. **基元类型**（`int`/`long`/`short`/`byte`/`char`/`float`/`double`/`bool`/`string`）：编译器内置视为已实现 `IComparable`/`IComparable<T>`/`IEquatable`/`IEquatable<T>`，无需显式声明。这是编译器语义规则，对齐 C# 中基元类型自动实现这些接口的行为。基元满足规则改用精确 mangle 后缀校验（`is_known_primitive_mangle_suffix`），避免用户定义同名前缀接口误判。
2. **命名类型**（类/结构体）：通过 `is_subtype` 检查是否显式实现接口（`registry.implements_interface`）。

### 使用示例

```as
using Arc;

// 泛型类约束
class SortedList<T> where T : IComparable<T> {
    public SortedList(T a, T b) { }
}

// 泛型函数约束
T MaxOf<T>(T a, T b) where T : IComparable<T> {
    return a;
}

// 多参数约束（逗号分隔）
class Pair<T, U> where T : IComparable<T>, U : IComparable<U> {
    public T First;
    public U Second;
}

void Main() {
    SortedList<int> list = new SortedList<int>(3, 7);  // int 天然满足 IComparable<int>
    int m = MaxOf<int>(10, 20);
}
```

补充场景：

- **场景 7**：`Container<T> where T : class` → `Container<string>`（引用类型约束，string 满足）
- **场景 8**：`Box<T> where T : struct` → `Box<int>`（值类型约束，int 满足）
- **场景 9**：`Factory<T> where T : new()` → `Factory<Product>`（Product 有 public 无参构造；值类型隐式满足）
- **场景 10**：`Repository<T> where T : class, new()` → `Repository<Product>`（多约束组合：引用类型 + 构造约束）

### 已知限制

- **泛型类模板的接口实现检查**：泛型类模板（如 `class ComparableBox<T> : IComparable<T> where T : IComparable<T>`）在实例化时由 `instantiate_generic_class` 调用 `check_generic_interface_impls` 校验接口实现。`substitute_class_def` 替换 bases 中的类型参数（如 `IComparable<T>` → `IComparable<int>`），然后实例化泛型接口并比较方法签名。非泛型类（如 `class Score : IComparable<int>`）由 `check_generic_interface_impls` 在定义时校验。
- **接口 where 子句**：`interface IContainer<T> where T : IComparable<T>` 的约束检查通过 `interface_templates`（`IndexMap<Ident, InterfaceDef>`）存储接口模板的 `where_clause`，在 `instantiate_generic_interface` 中调用 `check_constraints` 校验。同时 `instantiate_generic_interface` 从 `interface_templates` 获取完整 AST（保留泛型参数），用 `substitute_type_ast` + `lower_type` 重新构建方法签名，避免 `type_path_name` 丢弃泛型参数导致的签名不匹配。

## Enumerable 与 Queryable（诚实边界）

**Enumerable**（单一惯用法：`std/Arc/Linq/Enumerable.as`，`namespace Arc.Linq`）：接收 `IEnumerable<T>` / 数组 / `List<T>`，Lambda 为运行时委托形态但 **MIR 特化循环**；`crates/linq` 将 query comprehension 脱糖为方法链。

| 面 | 能力面与诚实边界 |
|----|------------------|
| `from` / `where` / `select` | 查询子句基础面 |
| `Any` / `Count` / `First` / `FirstOrDefault` | 数组 + `List`；0 参或单谓词；可接 Where/Select；空 `First`→`rt_panic`；空/无匹配 `FirstOrDefault`→`default(T)`；非 Queryable；异常对象 throw 面不在本面 |
| 赋值物化 `List<T> xs = from …` | MIR 物化支持；赋值目标类型校验不在本面 |
| `orderby` / `OrderBy` | 真排序：无捕获 key 时缓冲 `List<T>` + `rt_list_sort` comparator（数组/List 源；数值/`bool`/`char`/`string`/可 `CompareTo` key；`descending` 取反）；捕获 key 或不可比较类型诚实跳过；**多键排序**：连续 `orderby k1, k2`（或 `OrderBy` 链）折叠为单 comparator 依次生效——对标 C# `OrderBy(...).ThenBy(...)`，不依赖 qsort 稳定性 |
| `let` / `join` / `groupby` | 查询子句多变量流由 MIR 特化物化（编译期展开）：`let` 引入绑定供后续子句引用；`join` 为 inner join（等值 `on outer.key == inner.key`，内层源 `List<T>`；join 前的 orderby/groupby 缓冲在续流阶段重放 join 恢复 inner 绑定）；`group … by … [into g]` 产物 `Grouping<K,T>`（`std/Arc/Linq/Grouping.as`；首次出现序；等值判定走 key 的 `Compare == 0`）；`join … into` group join 不在本面 |

**Queryable**（`std/Arc/Linq/` 接口层）+ **Orm**（`std/Orm*`）：

| 面 | 能力面与诚实边界 |
|----|------------------|
| 语言 `expression` | 拒绝 `expression`；`Expression<Func<…>>` build-and-run |
| `IQueryProvider` / `IQueryable<T>` / `Expression` 类层次 | 接口与节点类型可加载；**≠** 产品级 ORM |
| `Queryable.AsQueryable` | 不在本面（不提供；禁 `return null` 假面） |
| `ChangeTracker` / `SqlTranslator`（手搓树） | 骨架可证伪 |
| `EntityMap<T>` / `ModelCache.Get` 热路径 | 泛型字段 mono（泛型类静态 `ConcurrentDictionary<string, T>` 字段 + 初始化器替换）与 `GetValueOrDefault` 命中/缺省语义真实验证（可证伪） |
| `DbContext.SaveChangesAsync` | 显式失败：有挂起 → 返回 `-1` 且不 Accept（禁 return 1）；无挂起 → 0 |
| `SqliteProvider` 连接 + prepare/step + DataTable 物化 + 绑定 + 连接级事务 | `:memory:` MVP 可证伪；`ExecuteAsync<T>` / Provider 级事务显式 NotImplemented；**≠** 完整 SQL Provider |
| `MongoProvider` CreateConnection / Execute* | `NotImplementedException`（禁空 List / null） |
| 方言 Provider 完整实体物化 / 事务 / 新方言 | 不在本面（不扩 `IDbProvider`） |

> **禁止**：用「Queryable 树化已绿」「Orm typeck 过」暗示 **完整 ORM 可写库**。

## 表达式树节点

`std/Arc/Linq/Expressions/` 声明与 `crates/expr` IR 对应的 Arc 侧类型；codegen 将编译期树写入 rodata / 运行时构造代码。节点种类扩展须同步更新 `crates/expr` 与规范。

## 演进契约

公开 API 演进遵循 RFC 036 成熟度流程与 实现规划（详见 `arc-core` / `arc-iteration` 规则），本文件仅承载最终设计：

1. 新增公开 API 须同步更新本章或对应规范小节
2. 添加 `examples/` 与 `arc-tests` 回归测试
3. 语义级变更提交 RFC
4. **可见性纪律**：新类型默认无修饰符（`internal`）；仅当满足判定准则的 R0（C# 标准参照为 public）或 R1（用户面证据）或属 R2（工具链注入契约）之一，才显式 `public`；对标 C# internal 的类型须给出超出 C# 的 Arc 特设理由

## 与 runtime 关系

std 通过 `rt_*` ABI 调用运行时；平台相关能力在 `crates/runtime/platform/` 与 capability 系统中声明（见[能力系统](15-capability-system.md)）。

## Arc.Orm 框架构建指南（L3 · 骨架）

Arc.Orm 是基于 Arc 语言核心能力构建的 ORM 框架层。本节说明如何利用[编译期树化 + 运行时翻译](09-query-language.md)范式构建数据库 Provider。

### 三层架构

| 层 | 归属 | 职责 | 示例 |
|----|------|------|------|
| 编译器核心 | `crates/ast`（`expr_tree.rs`）、`crates/typeck`、`crates/codegen` | `ExpressionIr` 树化 + 运行时构造 `Expression` 对象代码生成 | 编译器内 |
| 标准库接口 | `std/Arc/Linq/` (`Arc.Linq`) | `IQueryProvider`、`IQueryable<T>`、`Expression<T>` 类型声明、`Expression` 类层次 | `std/Arc/Linq/`、`std/Arc/Linq/Expressions/` |
| 数据库基础设施 | `std/Data/` (`Arc.Data`) | **独立库**：`IDbConnection`/`IDbTransaction`/`IDbProvider`/`DatabaseKind` + C#-aligned `DataTable`/`DataRow`/`DataColumn`/`ColumnType`，与 ORM 框架层解耦、供任意数据访问层复用。`IDbProvider` 以 `DatabaseKind`（封闭枚举）作粗粒度分类、以 `ProviderName`（开放字符串，如 "SQLite"/"MySQL"/"PostgreSQL"/"MongoDB"）承载具体提供程序名——新数据库涌现只需返回新字符串，无需改枚举 | `std/Data/IDbProvider.as`、`std/Data/DataTable.as`、`std/Data/DataRow.as`、`std/Data/DataColumn.as` |
| ORM 框架 | `std/Orm/` (`Arc.Orm`) | `SqlTranslator`（Arc 实现）；连接管理、命令执行、结果映射契约 | `std/Orm/SqlTranslator.as`、`std/Orm/DbContext.as` |
| ORM 方言 | `std/Orm.SQLite/` 等（平级） | 具体 Provider | `std/Orm.SQLite/SqliteProvider.as` |

**关键设计**：编译器核心负责编译期树化（typeck 构建 `ExpressionIr`）与运行时构造代码生成（codegen）；框架层实现 `IQueryProvider` 接口，在运行时遍历 `Expression` 对象树翻译 SQL——无需编写任何编译器代码，ORM 框架完全用 Arc 开发。

### 构建一个 Provider 的步骤

#### 1. 创建 Provider 类

在 `std/Orm.SQLite/` 下创建 `.as` 文件，声明 `namespace Arc.Orm.SQLite;`：

```as
namespace Arc.Orm.SQLite;

using Arc.Linq;
using Arc.Linq.Expressions;
using Arc.Orm;

class SqliteProvider : IQueryProvider {
    public SqliteProvider() {}

    // 运行时翻译：遍历 Expression 对象树生成 SQL
    // SqlTranslator.Translate 内部按 NodeType 分派 + 访问器遍历
    public IEnumerable<T> Execute<T>(Expression expression) {
        var (sql, parameters) = SqlTranslator.Translate(expression);
        return ExecuteSql<T>(sql, parameters);
    }

    public R ExecuteScalar<R>(Expression expression) {
        var (sql, parameters) = SqlTranslator.Translate(expression);
        return ExecuteScalarSql<R>(sql, parameters);
    }

    public IQueryable<T> CreateQuery<T>(Expression expression) {
        return new SqliteQueryable<T>(this, expression);
    }
}
```

#### 2. 使用 Provider

在用户程序中通过 `using Arc.Orm;` 导入，构建查询并执行：

```as
using Arc;
using Arc.Orm;
using Arc.Orm.SQLite;

struct User {
    public int Age;
    public string Name;
}

void Main() {
    SqliteProvider provider = new SqliteProvider();
    var users = provider.Users
        .Where(u => u.Age >= 18)
        .Select(u => u.Name)
        .ToList();
    // 运行时: SqlTranslator.Translate(expr) → "SELECT Name FROM Users WHERE Age >= 18"
}
```

#### 3. 编译期树化 + 运行时翻译流程

```
用户代码: db.Users.Where(u => u.Age >= 18).Select(u => u.Name).ToList()
    ↓ parse
    ↓ hir (lower_program)
    ↓ typeck (check_module)
    │   ├── Where(u => u.Age >= 18) → Lambda 树化为 ExpressionIr
    │   │   └── Binary(>=, Member(Parameter(u), Age), Constant(18))
    │   ├── Select(u => u.Name) → Lambda 树化为 ExpressionIr
    │   │   └── Member(Parameter(u), Name)
    │   └── 识别 Capture 节点（外部变量引用）
    ↓ mir (lower_module)
    ↓ codegen
    │   ├── 为每个 Lambda 生成运行时构造 Expression 对象的代码：
    │   │   var whereExpr = new LambdaExpression(
    │   │       [new ParameterExpression("u")],
    │   │       new BinaryExpression(">=", ...))
    │   └── Where()/Select()/ToList() 调用保持不变
运行时:
    ├── Where(whereExpr) → Provider.CreateQuery(combinedExpr)
    ├── Select(selectExpr) → Provider.CreateQuery(combinedExpr)
    └── ToList()
        ├── Provider.Execute<T>(combinedExpr)
        ├── SqlTranslator.Translate(expr) 遍历 Expression 对象树
        │   └── 生成 SQL: "SELECT Name FROM Users WHERE Age >= 18"
        └── ExecuteSql<T>(sql) 执行查询
```

### SqlTranslator 能力与限制

`SqlTranslator`（`std/Orm/SqlTranslator.as`）通过 NodeType 分派 + 虚方法访问器（GetLeft/GetRight/GetMember 等）遍历 `Expression` 对象树，无需 `is`/`as` 下转。Binary/Unary 节点按 per-op `NodeType`（Add/Subtract/Equal/AndAlso/Not/Negate 等）标识运算符（无 `Op` 字符串字段）。

| 支持的节点 | 翻译规则 | 示例 |
|-----------|----------|------|
| `u.Field >= N` | `Field >= N` | `u.Age >= 18` → `WHERE Age >= 18` |
| `u.Field == "str"` | `Field = 'str'` | `u.Name == "alice"` → `WHERE Name = 'alice'` |
| `&&` / `||` | `AND` / `OR` | `u.Age >= 18 && u.Active` → `WHERE Age >= 18 AND Active` |
| `==` / `!=` | `=` / `<>` | — |
| 布尔常量 | `TRUE` / `FALSE` | `u.Active` → `Active`（MemberAccess 直接取字段名） |
| Capture（捕获变量） | 参数化占位符 `@pN` | `u.Age >= threshold` → `WHERE Age >= @p0` |

**实现约束（能力依赖）**：

- `LambdaExpression.Parameters` 与 `MethodCallExpression.Args` 字段的填充依赖 `List<T>` backing store
- 完整 SQL 执行（连接 SQLite + 执行查询 + 结果映射）依赖错误处理（finally/using）+ IO/File
- `SqlTranslator` 为 Arc 标准库实现，方言特化通过子类或独立翻译器类实现

### 扩展为方言特化 Provider

当通用 `SqlTranslator` 不足以覆盖方言差异时（如 PostgreSQL 的 `ILIKE`、SQL Server 的 `TOP`），在 Provider 类中提供运行时翻译或子类化 `SqlTranslator`：

```as
namespace Arc.Orm;

using Arc.Linq;
using Arc.Linq.Expressions;

class PostgresProvider : IQueryProvider {
    // 使用 PostgresSqlTranslator（SqlTranslator 子类）覆盖方言差异
    public IEnumerable<T> Execute<T>(Expression expression) {
        var translator = new PostgresSqlTranslator();
        var (sql, parameters) = translator.Translate(expression);
        return ExecuteSql<T>(sql, parameters);
    }

    public IQueryable<T> CreateQuery<T>(Expression expression) {
        return new PostgresQueryable<T>(this, expression);
    }
}

class PostgresSqlTranslator : SqlTranslator {
    // override 方言特化方法（如 ILIKE 翻译）
}
```

---

## Arc.UI（L3 · 骨架）

<a id="ui-l3-skeleton-honesty"></a>

### UI 骨架能力面与诚实边界

`std/UI/Core/` 提供 L3 声明式 GUI 骨架；包内诚实地图见 `std/UI/Core/README.md`。

| 能力面 | 说明 |
|--------|------|
| 可证伪面 | `Thickness` / `LayoutSize`；`DependencyPropertyRegistry.NextId` + `RegisterProperty<T>` |
| 未落地面（诚实边界） | `Element.GetValue`/`SetValue`；Window 属性 wrapper；Content variant 与 `Arc.UI.Content` 集成；window 生命周期 / content variant / MinimalUI / DependencyProperty 测试面——不得以测试忽略（Skip/Deferred）冒充完备（禁当绿证，禁回迁 `UnitTest`） |
| 禁止冒充完备 | UI 框架扩张 / 假完备空挂；「碾压/业界领先/超越 WPF」宣称一律禁止 |

### 自定义字体（骨架 · 最小面）

对标 WPF：先按族名注册字体文件，再在 ARML 里写 `FontFamily`。设计权威见 [RFC 037 §9](../rfc/037-ui.md) / [custom-fonts](../rfc/037-ui/references/custom-fonts.md)。**不宣称完备**（无 pack URI、无 `FontStyle`、无 HarfBuzz/彩色 emoji）。

**注册**（启动期、首次使用该族之前）：

```as
using Arc.UI;

// Normal 面
bool ok = Application.Current.Fonts.RegisterFamily(
    "AppSans", "Assets/Fonts/AppSans-Regular.ttf");
if (!ok)
{
    // 失败须处理：禁静默当成功
}

// Normal + Bold 换面
ok = Application.Current.Fonts.RegisterFamily(
    "AppSans",
    "Assets/Fonts/AppSans-Regular.ttf",
    "Assets/Fonts/AppSans-Bold.ttf");
```

路径相对 **app/project base**（含 `arc.toml` 的项目根 / 运行时应用基目录）；字体文件须在 `bin/<config>/` 下按相同相对路径可见。

**ARML 消费**：

```arml
<TextBlock FontFamily="AppSans" FontSize="16" FontWeight="Bold"
      Text="自定义字体" />
```

| 要点 | 说明 |
|------|------|
| `FontFamily` | **族名**（已注册名或平台默认如 `"Segoe UI"`），不是文件路径 |
| `FontWeight` | 本面至少 `"Normal"` / `"Bold"`；`FontStyle` **不在本面** |
| 未注册名 | 回退默认族；不把未注册名当成已加载自定义字体 |
| 禁双轨 | UI 不用 `Arc.Drawing.Font`；离屏成像仍走 Drawing |

## 能力面与诚实边界补充（关键模块）

**诚实边界纪律**：宣称以非 Skip e2e 为准；**禁止**用 Skip / 未跑冒充完备；**禁止**无对照基准宣称「业界领先」。

| 模块 | 能力面与诚实边界 |
|------|-----|
| `List` / `Dictionary` / `HashSet` | List：`FindIndex`/`FindLastIndex`/`TrueForAll`/`LastIndexOf`；Dict：`ContainsValue`/`Add`/`TryGetValue`/`Keys`/`Values` 为真 `rt_dict_*`（禁静默 false）；`GetEnumerator` 链接兜底 `rt_panic`（热路径 emit_builtin）；HS：集合运算；诚实边界：定制 comparer ctor 不在本面 |
| `Arc.Array` | Copy/Clear/Reverse；IndexOf/LastIndexOf/Empty/Resize；Exists/Find*/TrueForAll/ForEach/Sort/BinarySearch/FindAll/ConvertAll（`int[]`）；搜索/谓词/`Empty`/`Resize`/`FindAll`/`ConvertAll` 限定 `int[]`；诚实边界：`Join`/泛型 Empty/跨类型 ConvertAll/定制比较器不在本面（禁空 stub） |
| `Queue` / `Stack` | 先进先出 / 后进先出集合 |
| `LinkedList` / `SortedSet` / `SortedDictionary` | 最小面；诚实边界：定制比较器 / 集合运算不在本面 |
| `Collection` / `ReadOnlyCollection` | ROC：IndexOf/CopyTo；诚实边界：`IList` 通用包装 / Items 不在本面 |
| `Span` / `ReadOnlySpan` | Length/索引/Slice(start\|start,len)/AsReadOnly/IsEmpty/Empty/CopyTo/TryCopyTo/ToArray/Fill/Clear/**foreach**；诚实边界：显式 GetEnumerator/IEnumerator、内容相等不在本面；borrowck 覆盖受限 |
| Concurrent* | `GetOrAdd(Func)`/`AddOrUpdate`；Dict `Values`/`ToArray` 未挂面 → 链接 stub `rt_panic`；已挂面链接 stub = 真 `rt_concurrent_dict_*`；Stack Arc `PushRange` 不在本面 |
| `File` / `Directory` / `Path` / Stream | ChangeExtension/HasExtension/ReadAllBytes/WriteAllBytes/CopyTo/ReadAllLines/GetTempPath/**GetFiles**/GetFiles(searchPattern)/**GetDirectories**/MemoryStream.ToArray；诚实边界：`SearchOption` / `GetDirectories(searchPattern)` / Move / 当前目录不在本面 |
| `StringBuilder` / string / Base64·Hex / UTF-8 | `s[i]`/`ToCharArray`→UTF-8 码元（**非** C# UTF-16；与 Length 对齐）；`Split(char\|string)` + `string.Join(string\|char, string[])`；`ToCharArray(start,length)`；Pad/Trim 单字符重载 + `Trim(params char[])`/`Trim(char[])` + `CompareOrdinal`；`Split(char\|string, StringSplitOptions)`；`Split(params char\|char[])` 多分隔符；`Split(sep, count, options)`；诚实边界：`Split(string[])`、两参 count、Invariant 文化不在本面 |
| `Json` / `Xml` 序列化 | Writer/Reader/`Serialize`；加深：数组·Skip·bool·`/` 转义；Xml `GetAttribute`/EmptyElement；手写 Read*；Json 含 `Deserialize(string, IJsonDeserializable)` 与泛型 `Deserialize<T>`（依赖 RFC 004 约束解锁）；诚实边界：属性注解 / 源生成 / Xml 工厂 Deserialize 不在本面（未开语言后门冒充 L3）；Options 部分为诚实字段；`Serialize`/`Deserialize` concrete→接口须显式装箱 |
| Tasks / EventLoop / Cancel | 泛型 `WhenAll<T>(Task<T>[])` 结果收集为数组面；诚实边界：`Yield` 不在本面（调度器让步不在本面；禁 null stub） |
| Exceptions / Console / Math | 非 C# `System.Math` 完备对等；诚实边界：`float` 变体 / `DivRem`/`BigMul` 不在本面；`ArgumentException(message, paramName)` 两参 ctor 不提供 |
| `char` Is*/ToUpper/ToLower | ASCII/`ctype` 子集（`rt_char_*`）；诚实边界：扩展 Unicode / `IsLetterOrDigit`/`IsControl`/`IsPunctuation` 不在本面 |
| `Environment` | 未设置返回空串（非 C# null）；属性形态不在本面（方法面）；无 `TickCount*`；不提供 `ProcessId`/`ProcessPath`/`GetFolderPath`/`ExpandEnvironmentVariables`/`Enum.Parse`/`GetNames` |
| `Convert` / `Guid` / `DateTime` / `TimeSpan` | Convert：无 `ChangeType`/`ToDateTime`/`ToBase64*`（Base64→`Arc.Text.Base64`）；`ToInt32(double)` 向零截断（非 banker's）；进制仅 2/8/10/16；溢出无 `OverflowException`（`FormatException`）；`ToInt64(string, fromBase)` 不在本面；DateTime：无 ParseExact/时区换算/`ToUniversalTime`；TimeSpan：无文化/`ParseExact`/`hh:mm`；Guid：无 `Guid(byte[])` ctor（单一惯用法 = `FromByteArray`）；无 ParseExact |
| `BitConverter` / `Buffer` | `IsLittleEndian`；`GetBytes(int\|long\|float\|double)`/`ToInt32`/`ToInt64`/`ToSingle`/`ToDouble` 主机端序往返；`SingleToInt32Bits`/`Int32BitsToSingle`/`DoubleToInt64Bits`/`Int64BitsToDouble` 位型重释（NaN/Inf/-0 位级保留）；`BlockCopy(byte[])`；诚实边界：short/bool、任意 `Array` 字节级 BlockCopy 不在本面 |
| `Arc.Types.Random` | LCG（算术）；**非** CSPRNG（密码学→`Arc.Security`）；`NextBytes` = `Next() % 256`/byte（无 bitwise；null→`ArgumentNullException`）；诚实边界：xoshiro/`Shared` 不在本面 |
| `Arc.Diagnostics.Stopwatch` | 高精度间隔；`Elapsed` 为 `TimeSpan`；无 `Environment.TickCount*`（单一惯用法） |
| `Arc.ComponentModel` | 字面量 ctor + 属性回读；`MaxLength.Max`（非 C# `Length`）；非 L3 DI 全量；诚实边界：本地化 typeof+nameof / `GetCustomAttributes` / DI 自动注册不在本面 |
| `Arc.CommandLine` | 选项匹配 Parse、`PrintHelp(IConsole)`；诚实边界：完整 System.CommandLine 对等不在本面；禁假完备空挂 |
| LINQ Enumerable | query + Where/Select + OrderBy 真排序（含多键 ThenBy）+ Any/Count/First/FirstOrDefault + `let`/`join`/`groupby`（产物 `Grouping<K,T>`）；数组/List；诚实边界：赋值物化目标类型校验不在本面；OrderBy 捕获 key 或不可比较类型诚实跳过 |
| LINQ Queryable / Orm | 拒绝 `expression` / `Expression<Func>` build-and-run；ChangeTracker/SqlTranslator 骨架可证伪；SqliteProvider execute-MVP；诚实边界：`AsQueryable` 不在本面；SaveChanges 挂起显式失败；SQLite `:memory:` MVP；Mongo CreateConnection/Execute → `NotImplementedException` |
| Arc.Net.P2P | 见 [Arc.Net / Arc.Net.P2P](#arcnet--arcnetp2p-能力面与诚实边界)；诚实边界：Multiaddr/Store/Topology/Relay 等未实现面显式 NotImplementedException；禁 P2P 假绿 |
| Threading / ThreadPoolScheduler | `lock {}` 糖、`Mutex.TryLock`、`Interlocked` Increment/Exchange/CompareExchange、`Thread.Sleep(ms>0)`、`Monitor.TryEnter`；诚实边界：Interlocked 仅 `int`（long/泛型不在本面）；`Monitor` Pulse/Wait Arc 面较薄 |
| `Lazy<T>` / `LazyInitializer` | 主线程缓存 / `Lazy<string>` / worker / 并发首次；实现 `Lock`+`Monitor` 单惯用法（**无** mode 枚举双轨）；诚实边界：无 `Lazy(T)` 预置值 ctor；完整 C# Lazy 面不在本面 |
| Reflection | 归工具链侧（非 L2 标准库承载）：自定义属性 / 签名类型等 |
| **QIF 验证底座** | Equal long/Δ、StartsWith/EndsWith、Contains·DoesNotContain 子串、List 元素 All/Any/SequenceEqual/Contains；诚实边界：不提供 L3 QIF L2–L7 全家桶；动态 host / L4–L7 / DI fixture 不在本面 |
| **Arc.Security（Hash/HMAC/CSPRNG）** | MD5/SHA1/SHA256/SHA512 NIST；HMAC-SHA256 RFC 4231/ASCII；CSPRNG 长度+DistinctDraws；string-in/hex-out；诚实边界：AES / RSA / PBKDF2 / 云 KMS / 完整 PKI 不在本面 |
| `Arc.Drawing` | 成像 decode/roundtrip/prune · QR 生成 vector/roundtrip/prune · 1D writer/prune · quirc decode/prune · zxing decode/unavailable/prune · draw_primitives/font_metrics/draw_prune；诚实边界：`Save(Stream, ImageFormat)` 依赖 Stream 消费点就绪后提供；BMP/TGA 编码、stb_rect_pack 字形 atlas 打包、相机照片增强、fuzz 不在本面 |

---

上一节：[12 运行时 ABI](12-runtime-abi.md) · 下一节：[14 结构化诊断](14-structured-diagnostics.md)