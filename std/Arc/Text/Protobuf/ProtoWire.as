namespace Arc.Text.Protobuf;

using Arc.Collections;

/// <summary>
/// Protobuf wire 编解码子集（RFC 030 · 纯 Arc · 无 vendored）。
/// 定位：P2P 应用层协议（identify/DHT/gossipsub/relay SignedEnvelope/PeerRecord）互操作必需的
/// wire 最小面——varint / zigzag / length-delimited / nested message / packed repeated。
/// 非完整 protobuf 编译器：无代码生成器、无 field presence/oneof 完整面、无反射；嵌套 message
/// 由调用方先编码再整体嵌入。
/// 归属：核心 Arc 包的 `Arc.Text` 序列化家族（`std/Arc/Text/Protobuf/`，与 Json/Xml 并列）——
/// protobuf 消息模型/编解码作为 `Arc.Text` 家族成员，gRPC 框架则置于网络侧 `Arc.Net.Grpc`。
/// 写侧以 List&lt;byte&gt; 累加（ToArray 收口）；读侧以 offset + out int bytesRead 显式游标
/// （bytesRead &lt;= 0 = 越界/格式错误，调用方判定，不抛异常）。
/// 语言缺口处理（031/033 信条：不得倒逼语言洞）：位运算缺失以算术仿真规避（% 128 / / 128 ·
/// WebSocket XOR 先例）；ulong 运算走 Arc.Math 静态面；byte[] 载体验证均在入参数组上完成。
/// </summary>
public static class ProtoWire {
    /// <summary>varint 编码（LEB128 · 无符号全域 0–2⁶⁴-1）。</summary>
    public static void WriteVarInt(List<byte> buffer, ulong value) {
        long s = (long)value; // 位型重解释：借符号比较探测 value &gt;= 2⁶³
        while (s < 0 || s >= 128) {
            long rem = s % 128;
            if (rem < 0) { rem = rem + 128; } // 负值 srem 修正为无符号低位
            buffer.Add((byte)(rem + 128));
            value = ulong.Divide(value, 128);
            s = (long)value;
        }
        buffer.Add((byte)value);
    }

    /// <summary>varint 编码（int64 语义：负数按 64 位符号扩展 · 10 字节形态）。</summary>
    public static void WriteVarInt64(List<byte> buffer, long value) {
        WriteVarInt(buffer, (ulong)value);
    }

    /// <summary>varint 编码（int32 语义：负数同 int64 符号扩展）。</summary>
    public static void WriteVarInt32(List<byte> buffer, int value) {
        WriteVarInt(buffer, (ulong)value);
    }

    /// <summary>varint 解码（LEB128 · 无符号全域 0–2⁶⁴-1）。bytesRead &lt;= 0 = 越界/超过 10 字节。</summary>
    public static ulong ReadVarInt(byte[] data, int offset, out int bytesRead) {
        bytesRead = 0;
        ulong result = 0;
        ulong factor = 1;
        int i = offset;
        int count = 0;
        bool done = false;
        while (i < data.Length && !done) {
            if (count >= 10) { break; }
            int b = data[i];
            int low7 = b % 128;
            ulong part = (ulong)low7;
            ulong term = ulong.Multiply(part, factor);
            result = ulong.Add(result, term);
            if (b < 128) {
                bytesRead = count + 1;
                done = true;
            } else {
                factor = ulong.Multiply(factor, 128);
                i = i + 1;
                count = count + 1;
            }
        }
        if (done) { return result; }
        bytesRead = 0;
        return 0;
    }

    /// <summary>zigzag 编码（sint32）：交错非负（n &lt;&lt; 1，负值取 ~(n &lt;&lt; 1) 之算术等价式）。</summary>
    public static void WriteZigZag32(List<byte> buffer, int value) {
        long enc;
        if (value >= 0) {
            enc = (long)value * 2;
        } else {
            enc = 0 - (long)value * 2 - 1;
        }
        WriteVarInt(buffer, (ulong)enc);
    }

    /// <summary>zigzag 编码（sint64）：交错非负。</summary>
    public static void WriteZigZag64(List<byte> buffer, long value) {
        long enc;
        if (value >= 0) {
            enc = value * 2;
        } else {
            enc = 0 - value * 2 - 1;
        }
        WriteVarInt(buffer, (ulong)enc);
    }

    /// <summary>zigzag 解码（sint32）。bytesRead &lt;= 0 = 输入无效。</summary>
    public static int ReadZigZag32(byte[] data, int offset, out int bytesRead) {
        ulong raw = ReadVarInt(data, offset, out bytesRead);
        if (bytesRead <= 0) { return 0; }
        ulong half = ulong.Divide(raw, 2);
        ulong doubleHalf = ulong.Multiply(half, 2);
        ulong rem = ulong.Subtract(raw, doubleHalf);
        if (rem == 0) {
            return (int)half;
        }
        ulong negPart = ulong.Subtract((ulong)0, half);
        ulong oneNeg = ulong.Subtract(negPart, (ulong)1);
        return (int)((long)oneNeg);
    }

    /// <summary>zigzag 解码（sint64）。bytesRead &lt;= 0 = 输入无效。</summary>
    public static long ReadZigZag64(byte[] data, int offset, out int bytesRead) {
        ulong raw = ReadVarInt(data, offset, out bytesRead);
        if (bytesRead <= 0) { return 0; }
        ulong half = ulong.Divide(raw, 2);
        ulong doubleHalf = ulong.Multiply(half, 2);
        ulong rem = ulong.Subtract(raw, doubleHalf);
        if (rem == 0) {
            return (long)half;
        }
        ulong negPart = ulong.Subtract((ulong)0, half);
        ulong oneNeg = ulong.Subtract(negPart, (ulong)1);
        return (long)oneNeg;
    }

    /// <summary>field tag 编码：tag = fieldNumber * 8 + wireType（字段号 &gt;= 1 · wireType ∈ {0,1,2,5}）。</summary>
    public static void WriteTag(List<byte> buffer, int fieldNumber, int wireType) {
        WriteVarInt(buffer, (ulong)(fieldNumber * 8 + wireType));
    }

    /// <summary>length-delimited 编码（field type 2）：tag + 长度 varint + payload（含内部 0x00 完整往返）。</summary>
    public static void WriteLengthDelimited(List<byte> buffer, int fieldNumber, byte[] payload) {
        int len = payload.Length;
        WriteTag(buffer, fieldNumber, 2);
        WriteVarInt(buffer, (ulong)len);
        int i = 0;
        while (i < len) {
            buffer.Add(payload[i]);
            i = i + 1;
        }
    }

    /// <summary>length-delimited 解码（field type 2）：返回 payload；bytesRead 含 tag+长度+载荷。</summary>
    public static byte[] ReadLengthDelimited(byte[] data, int offset, out int bytesRead) {
        bytesRead = 0;
        int tagLen;
        ReadVarInt(data, offset, out tagLen);
        if (tagLen <= 0) { return null; }
        int lenStart = offset + tagLen;
        int lenLen;
        ulong lenVal = ReadVarInt(data, lenStart, out lenLen);
        if (lenLen <= 0) { return null; }
        int payloadStart = lenStart + lenLen;
        long lenLong = (long)lenVal;
        if (lenLong < 0) { return null; }
        int length = (int)lenLong;
        if (payloadStart + length > data.Length) { return null; }
        List<byte> result = new List<byte>();
        int i = 0;
        while (i < length) {
            result.Add(data[payloadStart + i]);
            i = i + 1;
        }
        bytesRead = payloadStart + length - offset;
        return result.ToArray();
    }

    /// <summary>nested message 编码：protobuf wire 中嵌套消息与 length-delimited 同形（tag + 长度 + 子消息字节）。</summary>
    public static void WriteNested(List<byte> buffer, int fieldNumber, byte[] message) {
        WriteLengthDelimited(buffer, fieldNumber, message);
    }

    /// <summary>nested message 解码：返回子消息字节（调用方再行解析）。</summary>
    public static byte[] ReadNested(byte[] data, int offset, out int bytesRead) {
        bytesRead = 0;
        return ReadLengthDelimited(data, offset, out bytesRead);
    }

    /// <summary>packed repeated 编码（标量 repeated · field type 2）：tag + 长度 + 连续 varint 值。</summary>
    public static void WritePackedRepeated(List<byte> buffer, int fieldNumber, byte[] packedValues) {
        WriteTag(buffer, fieldNumber, 2);
        WriteVarInt(buffer, (ulong)packedValues.Length);
        int i = 0;
        while (i < packedValues.Length) {
            buffer.Add(packedValues[i]);
            i = i + 1;
        }
    }
}
