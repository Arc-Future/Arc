namespace Arc.Text.Protobuf;

using Arc;
using Arc.Collections;

    /// <summary>
    /// Protobuf 消息层 typed 写入流——将字段值按 wire 类型写入内部 <c>List&lt;byte&gt;</c>，
    /// <see cref="ToArray"/> 收口为消息字节。对标 C# <c>Google.Protobuf.CodedOutputStream</c>。
    /// 写侧以 <c>List&lt;byte&gt;</c> 累加（Arc 禁 <c>new T[expr]</c> 动态尺寸，对齐 <c>ProtoWire</c>
    /// 与 <c>Http2ByteUtils</c> 惯例）；varint/zigzag/tag 委托底层 <see cref="ProtoWire"/> 原语，
    /// fixed32/64 与 float/double 委托 <see cref="Arc.BitConverter"/>（小端化），string 经
    /// <c>Arc.Text.Encoding</c> UTF-8。未知 wire type 由调用方（消息实现）负责跳过。
    /// </summary>
public class CodedOutputStream {
    private List<byte> _buffer;

    /// <summary>创建空输出流。</summary>
    public CodedOutputStream() {
        _buffer = new List<byte>();
    }

    /// <summary>当前已写入字节数。</summary>
    public int Length {
        get { return _buffer.Count; }
    }

    /// <summary>序列化收口：返回全部已写入字节。</summary>
    public byte[] ToArray() {
        return _buffer.ToArray();
    }

    // ── varint 族（wire type 0）──

    /// <summary>写无符号 64 位 varint（LEB128 · 0–2⁶⁴-1）。</summary>
    public void WriteVarInt(ulong value) {
        ProtoWire.WriteVarInt(_buffer, value);
    }

    /// <summary>写 int32 varint（负数按 64 位符号扩展 · 10 字节）。</summary>
    public void WriteInt32(int value) {
        ProtoWire.WriteVarInt32(_buffer, value);
    }

    /// <summary>写 int64 varint（负数按 64 位符号扩展）。</summary>
    public void WriteInt64(long value) {
        ProtoWire.WriteVarInt64(_buffer, value);
    }

    /// <summary>写 uint32 varint（按 0–2³²-1 无符号 · 至多 5 字节）。</summary>
    public void WriteUInt32(long value) {
        this.WriteVarInt((ulong)value);
    }

    /// <summary>写 uint64 varint（0–2⁶⁴-1）。</summary>
    public void WriteUInt64(ulong value) {
        this.WriteVarInt(value);
    }

    /// <summary>写 bool（0/1 varint）。</summary>
    public void WriteBool(bool value) {
        if (value) { this.WriteVarInt((ulong)1); } else { this.WriteVarInt((ulong)0); }
    }

    /// <summary>写 enum（int32 varint 语义）。</summary>
    public void WriteEnum(int value) {
        this.WriteInt32(value);
    }

    /// <summary>写 sint32（zigzag 交错）。</summary>
    public void WriteZigZag32(int value) {
        ProtoWire.WriteZigZag32(_buffer, value);
    }

    /// <summary>写 sint64（zigzag 交错）。</summary>
    public void WriteZigZag64(long value) {
        ProtoWire.WriteZigZag64(_buffer, value);
    }

    // ── fixed 族（wire type 5/1 · 小端）──

    /// <summary>写 fixed32（uint32 位型 · 4 字节小端）。</summary>
    public void WriteFixed32(int value) {
        byte[] b = BitConverter.GetBytes(value);
        this.AppendLittleEndian(b, 4);
    }

    /// <summary>写 fixed64（uint64 位型 · 8 字节小端）。</summary>
    public void WriteFixed64(long value) {
        byte[] b = BitConverter.GetBytes(value);
        this.AppendLittleEndian(b, 8);
    }

    /// <summary>写 float（IEEE 754 32 位位型 · wire type 5 · 4 字节小端）。</summary>
    public void WriteFloat(float value) {
        byte[] b = BitConverter.GetBytes(value);
        this.AppendLittleEndian(b, 4);
    }

    /// <summary>写 double（IEEE 754 64 位位型 · wire type 1 · 8 字节小端）。</summary>
    public void WriteDouble(double value) {
        byte[] b = BitConverter.GetBytes(value);
        this.AppendLittleEndian(b, 8);
    }

    // ── tag / length-delimited 族 ──

    /// <summary>写 field tag：fieldNumber * 8 + wireType。</summary>
    public void WriteTag(int fieldNumber, int wireType) {
        ProtoWire.WriteTag(_buffer, fieldNumber, wireType);
    }

    /// <summary>写 bytes 字段载荷（tag 已由调用方写出）：长度 varint + 载荷字节。</summary>
    public void WriteBytes(byte[] payload) {
        int len = payload == null ? 0 : payload.Length;
        this.WriteVarInt((ulong)len);
        if (len > 0) { this.AppendBytes(payload); }
    }

    /// <summary>写 string 字段载荷（UTF-8 长度分帧）。</summary>
    public void WriteString(string s) {
        if (s == null) { this.WriteVarInt((ulong)0); return; }
        byte[] bytes = Encoding.GetBytes(s);
        this.WriteBytes(bytes);
    }

    /// <summary>写嵌套 message 字段载荷（内部再序列化后长度分帧）。null 视作空消息。</summary>
    public void WriteMessage(IMessage message) {
        if (message == null) {
            this.WriteVarInt((ulong)0);
            return;
        }
        CodedOutputStream tmp = new CodedOutputStream();
        message.WriteTo(tmp);
        this.WriteBytes(tmp.ToArray());
    }

    /// <summary>写 packed repeated 载荷（tag 已由调用方写出）：长度 varint + 连续值字节。</summary>
    public void WritePackedRepeated(byte[] packedValues) {
        this.WriteBytes(packedValues);
    }

    // ── 内部 ──

    private void AppendBytes(byte[] bytes) {
        int i = 0;
        while (i < bytes.Length) {
            _buffer.Add(bytes[i]);
            i = i + 1;
        }
    }

    private void AppendLittleEndian(byte[] hostBytes, int count) {
        if (count <= 0) { return; }
        if (BitConverter.IsLittleEndian()) {
            int i = 0;
            while (i < count) {
                _buffer.Add(hostBytes[i]);
                i = i + 1;
            }
        } else {
            int j = count - 1;
            while (j >= 0) {
                _buffer.Add(hostBytes[j]);
                j = j - 1;
            }
        }
    }
}
