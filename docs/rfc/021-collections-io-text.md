# RFC 021 集合、IO 与文本

## 背景

标准库核心三块——容器库、文件目录 IO、文本编码——是上层领域库的基础。设计目标：与 C# BCL 对齐的契约面、热路径可证伪、单一惯用法。集合/IO/文本全部以 `rt_*` ABI 对接运行时，编译器提供泛型与索引器降级机制。

## 设计决策

### Arc.Collections 容器库

动态容器经 C# 索引器表面访问，负载由 `rt_*` ABI 隐藏。

| 类型 | 命名空间 | 载体 | 说明 |
|------|----------|------|------|
| `List<T>` | `Arc.Collections` | `rt_list_*` | 动态数组；C# 索引器 `list[i]` 直访 buffer；值类型经 codegen 直降 `RtList` |
| `Dictionary<K,V>` | `Arc.Collections` | `rt_dict_*` | 关联表；索引器 `dict[k]`；连续表链 + 整键内联 |
| `HashSet<T>` | `Arc.Collections` | `rt_set_*` | 哈希集合；连续 entries 表 + 桶索引链；集合运算 |
| `Queue<T>` / `Stack<T>` | `Arc.Collections` | 同 `rt_list_*` 体系 | FIFO / LIFO |
| `LinkedList<T>` | `Arc.Collections` | `rt_linked_list_*` | 双向链表；节点为不透明句柄透传 |
| `SortedSet<T>` | `Arc.Collections` | `rt_sorted_set_*` | 排序集合；标量 inttoptr 装箱 |
| `SortedDictionary<K,V>` | `Arc.Collections` | `rt_sorted_dict_*` | 排序字典；标量 inttoptr 装箱 |
| `Collection<T>` | `Arc.Collections` | 委托 `List<T>` | ObjectModel 集合包装 |
| `ReadOnlyCollection<T>` | `Arc.Collections` | 无独立 ABI | 只读 List 包装；ctor/Count/索引器/Contains |
| `Span<T>` / `ReadOnlySpan<T>` | `Arc.Collections` | 编译器内建 | Length/索引/Slice/AsReadOnly/IsEmpty/Empty/CopyTo/TryCopyTo/ToArray/Fill/Clear/foreach |
| `ListEnumerator<T>` | `Arc.Collections` | — | 列表枚举器 |

资源安全：`Span<T>` 与借用由语言内存模型保证（见语言内存规范）；`List<T>`等动态容器的确定性释放由 ARC 语义负责。

### Arc.IO 文件目录 IO

对标 C# `System.IO`，lowering 至 `rt_file_*`/`rt_dir_*`/`rt_path_*`。

| 类型 | 命名空间 | 成员 | ABI |
|------|----------|------|-----|
| `File` | `Arc.IO` | `ReadAllText`/`WriteAllText`/`Exists`/`Delete`/`AppendAllText`/`Copy`/`Move`/`ReadAllBytes`/`WriteAllBytes`/`ReadAllLines` | `rt_read_file`/`rt_write_file`/`rt_file_*` |
| `Directory` | `Arc.IO` | `CreateDirectory`/`Exists`/`Delete`/`GetFiles`（含 `searchPattern`）/`GetDirectories` | `rt_dir_*` |
| `Path` | `Arc.IO` | `Combine`/`GetDirectoryName`/`GetFileName`/`GetExtension`/`ChangeExtension`/`HasExtension`/`GetTempPath` | `rt_path_*` |
| `Stream` | `Arc.IO` | 抽象流；`Read`/`Write`/`CopyTo`/释放 | — |
| `FileStream` | `Arc.IO` | 文件流；同步面 + 真异步面 `ReadAsync`/`WriteAsync`/`FlushAsync`（文件 I/O 线程池卸载 + 完成投递，见 [014](014-runtime-abi.md) FileStream 真异步节） | `rt_file_stream_*`（async：`rt_file_stream_*_async`） |
| `MemoryStream` | `Arc.IO` | 内存流；`ToArray`/`Write`/`Read` | — |

`NetworkStream.Read(byte[], int, int)`/`Write(byte[], int, int)` 闭环见网络篇（byte[] 显式长度、读返回实际字节数、EOF→0、部分读语义、写失败抛 `IOException`）。

### Arc.Text 文本编码

| 类型 | 命名空间 | 说明 |
|------|----------|------|
| `StringBuilder` | `Arc.Text` | 可变字符串累加 |
| `Encoding` | `Arc.Text` | UTF-8 编解码；`GetByteCount`/编码/解码 |
| `Base64` | `Arc.Text` | `ToBase64`/`FromBase64`；**Base64 单一惯用法**（不在 `Convert` 双轨） |
| `Hex` | `Arc.Text` | `ToHexString(byte[])`/`FromHexString(string)`；**hex 单一惯用法**（不在 `Convert`/安全域双轨） |

字符串面：`s[i]` 索引、`Split`/`Join`/`ToCharArray`、`IsNullOrEmpty`/`IsNullOrWhiteSpace`、`IndexOf`/`LastIndexOf`、`Pad*`/`Trim*`、`StartsWith`/`EndsWith(char)`、`Compare`/`CompareOrdinal`、`Concat`/`FromCharCount`。字符分类/大小写（`char.IsDigit`/`IsLetter`/`ToUpper`/`ToLower`…）经 `rt_char_*`（ASCII/`ctype` 子集）。

### 门禁与回归

集合/IO/文本面以非 Skip 端到端用例为权威证据（`std_collections_e2e`/`file_io_e2e`/`directory_e2e`/`path_e2e`/`stream_io_e2e`/`std_text_e2e`/`text_encoding_utf8_e2e`/`span_e2e` 等），禁止以 Skip 冒充完备。

## 边界

- 本文档只讲集合/IO/文本**库**的用户面与 ABI；`List<T>` 等表达式的语言级集合表达式、字符串字面量与数值语义属语言核心（见 [007 集合、字符串与数值](007-collections-strings-numerics.md)）。
- 并发容器（`Arc.Collections.Concurrent`）见 [024 并发集合](024-concurrent-collections.md)。
- 序列化家族（Json/Xml/Yaml）与 `Arc.Text.Json`/`Xml` 见 [022 异步任务与 LINQ/序列化](022-async-linq-serialization.md)；Protobuf 二进制见 [030](030-protobuf.md)。
- 数学/张量归 [023](023-math-tensor-di.md)；`Stopwatch` 等诊断面见 [023]。
- 二进制 Protobuf、数据库翻译不在此篇。

---

上一节：[020 标准库架构与拆分](020-std-architecture.md) · 下一节：[022 异步任务与 LINQ/序列化](022-async-linq-serialization.md)