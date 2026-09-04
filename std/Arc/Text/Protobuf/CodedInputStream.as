namespace Arc.Text.Protobuf;

using Arc;
using Arc.Collections;

/// <summary>
/// Protobuf 消息层 typed 读取流——包裹 <c>byte[]</c> 载荷，以显式位置游标逐字段读取。
/// 对标 C# <c>Google.Protobuf.CodedInputStream</c>。所有读取方法返回 bool 成功；
/// 失败时置 <see cref="Failed"/> 并保持/推进游标不定，调用方（消息实现）判定并中止。
/// 读侧委托底层 <see cref="ProtoWire"/> 原语（varint/zigzag）；fixed 与 float/double
/// 走 <see cref="Arc.BitConverter"/> 小端读；string 经 <c>Arc.Text.Encoding</c> UTF-8 解码。
/// </summary>
public class CodedInputStream {
    private byte[] _data;
    private int _pos;
    private bool _failed;

    /// <summary>创建读取流，游标自 0 起始。</summary>
    public CodedInputStream(byte[] data) {
        _data = data;
        _pos = 0;
        _failed = false;
    }

    /// <summary>是否已读到末尾（游标越过载荷末端）。</summary>
    public bool Eof {
        get {
            byte[] d = _data; // byte[] 字段直读不可靠 → 拷局部
            return _pos >= d.Length;
        }
    }

    /// <summary>读取过程中是否发生过失败。</summary>
    public bool Failed {
        get { return _failed; }
    }

    /// <summary>当前游标位置（字节）。</summary>
    public int Position {
        get { return _pos; }
    }

    // ── varint 族 ──

    /// <summary>读无符号 64 位 varint。</summary>
    public bool ReadVarInt(out ulong value) {
        value = 0;
        int bytesRead;
        byte[] d = _data; // byte[] 字段直读不可靠 → 拷局部再传参
        int p = _pos;     // int 字段直读不可靠 → 拷局部再传参
        ulong v = ProtoWire.ReadVarInt(d, p, out bytesRead);
        if (bytesRead <= 0) { this.Fail(); return false; }
        _pos = _pos + bytesRead;
        value = v;
        return true;
    }

    /// <summary>读 int32（低 32 位截断）。</summary>
    public bool ReadInt32(out int value) {
        value = 0;
        ulong v;
        if (!this.ReadVarInt(out v)) { return false; }
        value = (int)(long)v;
        return true;
    }

    /// <summary>读 int64（位型重解释）。</summary>
    public bool ReadInt64(out long value) {
        value = 0;
        ulong v;
        if (!this.ReadVarInt(out v)) { return false; }
        value = (long)v;
        return true;
    }

    /// <summary>读 uint32（0–2³²-1，以 long 承载）。</summary>
    public bool ReadUInt32(out long value) {
        value = 0;
        ulong v;
        if (!this.ReadVarInt(out v)) { return false; }
        value = (long)v;
        return true;
    }

    /// <summary>读 uint64。</summary>
    public bool ReadUInt64(out ulong value) {
        value = 0;
        return this.ReadVarInt(out value);
    }

    /// <summary>读 bool（非零为真）。</summary>
    public bool ReadBool(out bool value) {
        value = false;
        ulong v;
        if (!this.ReadVarInt(out v)) { return false; }
        value = v != 0;
        return true;
    }

    /// <summary>读 enum（int32 语义）。</summary>
    public bool ReadEnum(out int value) {
        value = 0;
        return this.ReadInt32(out value);
    }

    /// <summary>读 sint32（zigzag）。</summary>
    public bool ReadZigZag32(out int value) {
        value = 0;
        int bytesRead;
        byte[] d = _data; // byte[] 字段直读不可靠 → 拷局部再传参
        int v = ProtoWire.ReadZigZag32(d, _pos, out bytesRead);
        if (bytesRead <= 0) { this.Fail(); return false; }
        _pos = _pos + bytesRead;
        value = v;
        return true;
    }

    /// <summary>读 sint64（zigzag）。</summary>
    public bool ReadZigZag64(out long value) {
        value = 0;
        int bytesRead;
        byte[] d = _data; // byte[] 字段直读不可靠 → 拷局部再传参
        long v = ProtoWire.ReadZigZag64(d, _pos, out bytesRead);
        if (bytesRead <= 0) { this.Fail(); return false; }
        _pos = _pos + bytesRead;
        value = v;
        return true;
    }

    // ── fixed 族 ──

    /// <summary>读 fixed32（4 字节小端）。</summary>
    public bool ReadFixed32(out int value) {
        value = 0;
        byte[] b = this.Slice(4);
        if (b == null) { this.Fail(); return false; }
        value = this.ToHost32(b);
        return true;
    }

    /// <summary>读 fixed64（8 字节小端）。</summary>
    public bool ReadFixed64(out long value) {
        value = 0;
        byte[] b = this.Slice(8);
        if (b == null) { this.Fail(); return false; }
        value = this.ToHost64(b);
        return true;
    }

    /// <summary>读 float（4 字节小端 · IEEE 754 位型重释）。</summary>
    public bool ReadFloat(out float value) {
        value = 0;
        byte[] b = this.Slice(4);
        if (b == null) { this.Fail(); return false; }
        value = this.ToHostSingle(b);
        return true;
    }

    /// <summary>读 double（8 字节小端 · IEEE 754 位型重释）。</summary>
    public bool ReadDouble(out double value) {
        value = 0;
        byte[] b = this.Slice(8);
        if (b == null) { this.Fail(); return false; }
        value = this.ToHostDouble(b);
        return true;
    }

    // ── tag / length-delimited 族 ──

    /// <summary>读 field tag，解出 fieldNumber 与 wireType。</summary>
    public bool ReadTag(out int fieldNumber, out int wireType) {
        fieldNumber = 0;
        wireType = 0;
        ulong tag;
        if (!this.ReadVarInt(out tag)) { return false; }
        long ltag = (long)tag;
        wireType = (int)(ltag % 8);
        fieldNumber = (int)(ltag / 8);
        return true;
    }

    /// <summary>读 bytes 字段载荷（长度 varint + 载荷）。</summary>
    public bool ReadBytes(out byte[] payload) {
        payload = null;
        ulong len;
        if (!this.ReadVarInt(out len)) { return false; }
        long lenLong = (long)len;
        if (lenLong < 0) { this.Fail(); return false; }
        int n = (int)lenLong;
        byte[] b = this.Slice(n);
        if (b == null) { this.Fail(); return false; }
        payload = b;
        return true;
    }

    /// <summary>读 string 字段载荷（UTF-8 解码）。</summary>
    public bool ReadString(out string value) {
        value = "";
        byte[] b = null;
        if (!this.ReadBytes(out b)) { return false; }
        value = Encoding.GetString(b);
        return true;
    }

    /// <summary>读嵌套 message 字段载荷，就地合并进 <paramref name="message"/>。</summary>
    public bool ReadMessage(IMessage message) {
        byte[] b = null;
        if (!this.ReadBytes(out b)) { return false; }
        CodedInputStream sub = new CodedInputStream(b);
        message.MergeFrom(sub);
        return !sub.Failed;
    }

    /// <summary>读 length-delimited 载荷（bytes 同构）。</summary>
    public bool ReadLengthDelimited(out byte[] payload) {
        payload = null;
        return this.ReadBytes(out payload);
    }

    // ── 内部 ──

    private bool HasBytes(int count) {
        byte[] d = _data; // byte[] 字段直读不可靠 → 拷局部
        return _pos + count <= d.Length;
    }

    private byte[] Slice(int count) {
        if (!this.HasBytes(count)) { return null; }
        byte[] d = _data; // byte[] 字段直读不可靠 → 拷局部再索引
        List<byte> list = new List<byte>();
        int i = 0;
        while (i < count) {
            list.Add(d[_pos + i]);
            i = i + 1;
        }
        _pos = _pos + count;
        return list.ToArray();
    }

    private int ToHost32(byte[] le) {
        if (BitConverter.IsLittleEndian()) { return BitConverter.ToInt32(le, 0); }
        return BitConverter.ToInt32(this.Reverse(le), 0);
    }

    private long ToHost64(byte[] le) {
        if (BitConverter.IsLittleEndian()) { return BitConverter.ToInt64(le, 0); }
        return BitConverter.ToInt64(this.Reverse(le), 0);
    }

    private float ToHostSingle(byte[] le) {
        if (BitConverter.IsLittleEndian()) { return BitConverter.ToSingle(le, 0); }
        return BitConverter.ToSingle(this.Reverse(le), 0);
    }

    private double ToHostDouble(byte[] le) {
        if (BitConverter.IsLittleEndian()) { return BitConverter.ToDouble(le, 0); }
        return BitConverter.ToDouble(this.Reverse(le), 0);
    }

    private byte[] Reverse(byte[] src) {
        List<byte> list = new List<byte>();
        int i = src.Length - 1;
        while (i >= 0) {
            list.Add(src[i]);
            i = i - 1;
        }
        return list.ToArray();
    }

    private void Fail() {
        _failed = true;
    }
}
