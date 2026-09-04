# RFC 030 Protobuf 二进制序列化

## 背景

Arc 需要 protobuf **wire 编解码子集**以支撑 libp2p 应用层协议（identify、DHT、gossipsub、Circuit Relay v2、SignedEnvelope/PeerRecord）的字节级互操作。设计目标：以 varint、zigzag、length-delimited、nested message、packed repeated 为交付边界，**纯 Arc 实现**（无 vendored、无代码生成器），API 形态对齐 C# `Google.Protobuf` 精华 + Arc 单一惯用法，以 `byte[]` 为工作载体（显式长度、含内部 0x00 完整往返）。与既有文本序列化家族（Json/Xml）并列但独立——二进制 wire 面唯一归位 `Arc.Text.Protobuf`，不双轨。

## 设计决策

### 定位与命名空间

| 维度 | 裁决 |
|------|------|
| 命名空间 | `Arc.Text.Protobuf`（`std/Arc/Text/Protobuf/` 子目录 · 核心 Arc 包内 Text 家族 · 不新增 arc.toml） |
| 参照 | C# `Google.Protobuf` 的 `CodedOutputStream`/`CodedInputStream` 核心语义，按 Arc 品味收敛为静态方法 + 显式游标形态 |
| 形态 | 单一静态类 `ProtoWire`（对齐 `Encoding`/`BitConverter`/`Math` 静态门面先例；无可变流对象状态） |
| 写侧 | `List<byte>` 累加 + `ToArray()` 收口（Arc 禁 `new T[expr]` 动态尺寸） |
| 读侧 | `offset` + `out int bytesRead` 显式游标（`bytesRead <= 0` = 越界/格式错误；不抛异常，调用方判定） |
| 载体 | `byte[]`（显式长度 · 含内部 0x00 完整往返） |
| 无符号域 | 内部以 `ulong` 承载；位运算缺失 → 算术仿真（`% 128`/`/ 128` · WebSocket XOR 仿真先例） |

### API 表面（`Arc.Text.Protobuf.ProtoWire`）

```as
namespace Arc.Text.Protobuf;

// 写侧：向 List<byte> 累加；读侧：offset + out bytesRead（<=0 = 越界/格式错误）
public static class ProtoWire {
    // varint（LEB128 · 无符号全域 0–2⁶⁴-1）
    public static void WriteVarInt(List<byte> buffer, ulong value);
    public static void WriteVarInt64(List<byte> buffer, long value);  // 负数按 64 位符号扩展（10 字节）
    public static void WriteVarInt32(List<byte> buffer, int value);   // int32 负数同 int64 语义
    public static ulong ReadVarInt(byte[] data, int offset, out int bytesRead);

    // zigzag（sint32/sint64 → 无符号交错）
    public static void WriteZigZag32(List<byte> buffer, int value);
    public static void WriteZigZag64(List<byte> buffer, long value);
    public static int ReadZigZag32(byte[] data, int offset, out int bytesRead);
    public static long ReadZigZag64(byte[] data, int offset, out int bytesRead);

    // field tag：tag = fieldNumber * 8 + wireType（字段号 ≥ 1 · wireType ∈ {0,1,2,5}）
    public static void WriteTag(List<byte> buffer, int fieldNumber, int wireType);

    // length-delimited（field type 2）：tag + 长度 varint + payload（含内部 0x00 完整往返）
    public static void WriteLengthDelimited(List<byte> buffer, int fieldNumber, byte[] payload);
    public static byte[] ReadLengthDelimited(byte[] data, int offset, out int bytesRead);  // 返回 payload

    // nested message：wire 形态同 length-delimited；嵌套 message 由调用方先编码再整体嵌入
    public static void WriteNested(List<byte> buffer, int fieldNumber, byte[] message);
    public static byte[] ReadNested(byte[] data, int offset, out int bytesRead);

    // packed repeated（标量 repeated · field type 2）：tag + 长度 + 连续 varint 值
    public static void WritePackedRepeated(List<byte> buffer, int fieldNumber, byte[] packedValues);
}
```

### 语言能力缺口处理（不得倒逼语言洞）

语言面缺口不改语言，全部以既有先例算术仿真规避：

| 缺口 | 规避 |
|------|------|
| 位运算缺失（`&`/`\|`/`<<`/`>>`/`~` 均无） | 7 位分组以 `% 128`/`/ 128` 算术仿真；tag 以 `fieldNumber * 8 + wireType`；zigzag 以乘 2/取负算术等价式 |
| `long` 字面量无 `L` 后缀 | 显式构造 `(long)常量`（`TimeSpan.as` 同例） |
| `int→long` 不隐式加宽 | 显式 `(long)` cast（`TimeSpan.as` 同例） |
| `ulong` 无 `%`/`/` 运算符 | `ulong.Add/Subtract/Multiply/Divide` 静态面；余数以 `Subtract(value, Multiply(Divide(value,b), b))` |
| `byte[]` 字段直读 `.Length`/索引不可靠 | **先拷贝局部**再读（典型于 `CodedInputStream` 字段直读） |
| `List<T[]>` 泛型数组元素 Add 损坏 | 用 blob+lengths 平坦表示（packed repeated 以单个连续 `byte[]` 承载）或引用类；不引入 `List<byte[]>` |
| `from` 为保留字 | 参数更名（如 `fromOffset`） |

### 消息层（typed 编解码面）

在 wire 原语之上补足消息层契约，作为 gRPC codec 载体面（`Arc.Text.Protobuf` 命名空间）：

| 类型 | 职责 |
|------|------|
| `IMessage` | 消息契约接口（`WriteTo(CodedOutputStream)` / `MergeFrom(CodedInputStream)`，无反射手写实现，对齐 `IJsonSerializable` 先例） |
| `CodedOutputStream` | typed 写入流（varint 族 / fixed 族 / tag / length-delimited / nested / packed，`ToArray()` 收口） |
| `CodedInputStream` | typed 读取流（显式游标 · `Failed` 失败态 · 字段直读不可靠 → 局部拷贝规避） |
| `MessageCodec` | 门面（`Serialize(IMessage)` / `Deserialize<T>`（`where T : IMessage, new()`）/ `MergeInto`） |

**设计决策**：

- **纯 Arc、无反射**：`WriteTo`/`MergeFrom` 手写实现，无反射促发执行；未知字段跳过非完整面。
- **诚实边界**：gRPC 5 字节消息分帧属网络层 `Arc.Net.Grpc`（本面仅消息 ⇄ 字节）。float/double（wire type 5/1）经 `BitConverter` float/double 面编解码；浮点非默认值守卫须用 `!(x == 0)`（`!=` 在 Arc 有序语义下对 NaN 为假）。
- **非负约束**：varint 原始值语义按 protobuf 为无符号域；`WriteVarInt64` 对负数按「64 位符号扩展」编码（与 protobuf int64 一致，10 字节形态）；`WriteZigZag*` 为所有符号值提供标准交错。
- **显式失败**：读侧越界/格式错误以 `bytesRead <= 0` 或 `Failed` 态返回，调用方判定，不静默。

## 边界

- 本文档讲 `Arc.Text.Protobuf` 二进制 **wire 编解码子集与消息层**；文本序列化家族（Json/Xml/Yaml 及统一约定）见 [022 异步、LINQ 与序列化家族](022-async-linq-serialization.md)。
- 二维码/条码的编码语义见 [029 图像与图形](029-imaging-graphics.md)，与 protobuf 二进制编解码无关。
- **仅 wire 子集**：无 `protoc` 等价代码生成器；无 field presence/oneof 完整面；无反射/描述符表；无 map 字段；无 fixed32/fixed64 之外 wire type 5/1 的完整面。**不宣称达 Google.Protobuf 级**。
- **嵌套 message 由调用方组合**：`WriteNested` 仅负责「tag + 长度 + 已编码子消息」，子消息编码由调用方先构造。
- **packed repeated 语义**：写侧要求调用方预编码各元素 varint 为连续 `byte[]`；读侧提供 `ReadVarInt` 游标供调用方循环解析。
- 串行化/反序列化的通用门面与选项约定（`XxxSerializer`/`XxxSerializerOptions`）见 [022](022-async-linq-serialization.md)。

---

上一节：[029 图像与图形](029-imaging-graphics.md) · 下一节：[031 编译器 CLI 与构建](031-compiler-cli.md)