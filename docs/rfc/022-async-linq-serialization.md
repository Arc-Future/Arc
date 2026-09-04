# RFC 022 异步任务与 LINQ/序列化

## 背景

异步任务库、LINQ 查询面与序列化家族是标准库的横切能力。设计目标：异步组合子齐全、查询声明式（编译期树化 + 运行时翻译归 std）、序列化开发者体验一致（三门面统一命名与形状）。

## 设计决策

### Arc.Tasks 异步任务库

| 面 | 成员/形态 |
|----|-----------|
| 任务类型 | `Task<T>`/`Task`/`TaskStatus`；async 函数经 LLVM 协程 lowering（见 [009](009-async-concurrency.md)），Task 为协程 resume/destroy 句柄的运行时包装 |
| 组合子 | `FromResult`/`Delay`/`WhenAll`/`WhenAny`/`Wait`/`WaitAll`/`WaitAny`；组合子收 `params ReadOnlySpan<Task>` |
| 取消 | `CancellationToken`/`CancellationTokenSource`；`FromException`→`IsFaulted`/`Exception` |
| 事件循环 | `EventLoop`：`poll_events`/`run`/`should_close` |

`await` 仅作用于编译器内建的 `Task<T>`（无第三方可等待类型协议），延续经 `await` 组合表达。异步方法一律 `Async` 后缀；接受 `CancellationToken` 为**可选**惯例（需要取消的 API 显式携带，不强制全量）。`Yield` 调度器让步面不在本设计面内（以组合子覆盖需要）。

### Enumerable 与 Queryable

**Enumerable**（`std/Arc/Linq/Enumerable.as`，`namespace Arc.Linq`）：接收 `IEnumerable<T>`/数组/`List<T>`，Lambda 为运行时委托形态但 MIR 特化循环；query comprehension 脱糖为方法链。

| 面 | 契约 |
|----|------|
| `from`/`where`/`select` | 查询语法 → 方法链 |
| `Any`/`Count`/`First`/`FirstOrDefault` | 0 参或单谓词；可接 Where/Select；空 `First` 抛运行时而 `FirstOrDefault` 返回 `default(T)` |
| `OrderBy`/`orderby` | 无捕获 key 真排序（缓冲 `List<T>` + `rt_list_sort` comparator；`descending` 取反） |
| 赋值物化 | `List<T> xs = from …` 经 MIR 物化 |

**Queryable**（`std/Arc/Linq/` 接口层，`Arc.Linq`）：`IQueryProvider`/`IQueryable<T>`/`Expression` 类层次。编译期树化 + 运行时翻译双阶段：typeck 构建 `ExpressionIr`，codegen 生成运行时构造 `Expression` 对象树的代码；运行时由 `std/Orm/SqlTranslator.as`（Arc 实现）遍历对象树生成方言 SQL（见数据库篇）。表达式树节点与 `crates/expr` IR 对应，节点种类扩展须同步更新。

### 序列化家族统一约定（Json / Xml / Yaml）

`Arc.Text.{Json|Xml|Yaml}` 三门面统一命名与形状，开发者体验一致。

| 层 | 约定 |
|----|------|
| 门面类名 | `XxxSerializer`（`JsonSerializer`/`XmlSerializer`/`YamlSerializer`）；方法形状 `Serialize(value)`/`Serialize(value, options)`，读路径 `Deserialize`/`Parse` |
| 选项类 | `XxxSerializerOptions`，统一 `Default` 静态属性；`XxxWriterOptions`/`XxxReaderOptions` 归低层 |
| 低层读写 | `XxxReader`/`XxxWriter` + 各自 `XxxWriterOptions`/`XxxTokenType` |
| 就地读 | 读路径 `Deserialize(text, IXxxDeserializable)` 就地填充 |

**诚实差异（文档化，非伪装一致）**：Json/Xml 为**契约优先**——`IXxxSerializable.WriteXxx(XxxWriter)`/`IXxxDeserializable.ReadXxx(XxxReader)` 流式接口，类型手写序列化钩子。Yaml 为 **DOM 优先**——以 `YamlNode` 文档树为一等公民（`YamlSerializer.Parse`/`Serialize`），不引入流式 `IYamlSerializable`。门面类名、方法形状、选项类命名仍与家族统一；选项字段仅保留对该格式有意义的成员（如 Yaml 无紧凑模式，故无 `WriteIndented`，仅 `IndentChars`）。

```as
using Arc.Text.Json;

class Point : IJsonSerializable {
    public int X;
    public int Y;
    public void WriteJson(JsonWriter writer) { writer.WriteNumber(X); writer.WriteNumber(Y); }
    public void ReadJson(JsonReader reader) { X = reader.ReadInt32(); Y = reader.ReadInt32(); }
}

Point p = JsonSerializer.Deserialize<Point>("{...}");
JsonSerializer.Serialize(p);
```

泛型 `Deserialize<T>` 经 `new T()` + 约束接口分派解锁；属性注解 / 源生成不在本设计面内。

## 边界

- 本文档讲 Task/Enumerable/Queryable 与序列化**家族统一约定**；二进制 Protobuf 见 [030](030-protobuf.md)；数据库/ORM 翻译见 [039 ORM 与 SQL 翻译](039-orm.md)。
- 表达式树机制（`Expression<T>`、Provider、LINQ）详见 [011 表达式树与查询语言](011-expression-trees-query.md)；此处只承载标准库侧类型归属。
- 并发集合与线程原语见 [024 并发集合](024-concurrent-collections.md) 与语言并发模型。
- 泛型约束（`where T : IComparable<T>` 等）见 [020](020-std-architecture.md) 与语言类型规范。

---

上一节：[021 集合、IO 与文本](021-collections-io-text.md) · 下一节：[023 数学、张量与依赖注入](023-math-tensor-di.md)