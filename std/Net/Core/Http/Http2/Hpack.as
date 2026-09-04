// S2 (RFC 033 §2.4): Arc.Net — HPACK 编解码（RFC 7541）。
//
// 纯 Arc 实现。语言缺位运算（§0.1 不得倒逼语言洞）：掩码/续位以除法/取模算术仿真。
//
// 诚实边界（RFC 7541 允许的合法子集）：
//   - 静态表：完整 61 项（HpackStaticTable）。
//   - 动态表：最小——**解码**支持（增量索引表项 + 超限驱逐，上限 4096 字节）；
//     编码不写入动态表（仅用静态表 + 不索引字面量，HPACK 合规、可证伪）。
//   - Huffman：**解码**完整（HuffmanCodec 257 符号）；**编码不启用** Huffman
//     （普通字面量编码合法；压缩比非 S2 目标）。
//   - 表大小更新指令（SETTINGS_HEADER_TABLE_SIZE 联动）解码支持，钳制 ≤4096。

namespace Arc.Net;

using Arc.Collections;
using Arc.Text;

/// <summary>RFC 7541 HPACK 编解码器。单连接实例（动态表按连接状态演进）。</summary>
internal class Hpack {
    private const int HeaderTableSizeLimit = 4096;

    // 动态表：最近插入在表头（index 0）。元素 = string[] { name, value }。
    private List<string[]> _dynamicTable;
    private int _dynamicSize;

    public Hpack() {
        _dynamicTable = new List<string[]>();
        _dynamicSize = 0;
    }

    // ── 字节读写小工具 ──

    private static int Pow2(int i) {
        int r = 1;
        int n = 0;
        while (n < i) { r = r * 2; n = n + 1; }
        return r;
    }

    /// <summary>按 RFC 7541 §5.1 编码整数（前缀 N 位，其余 7 位一组续位）。</summary>
    private static void EncodeInteger(List<byte> out_, int prefix, int flags, int value) {
        int maxPrefix = Pow2(prefix) - 1;
        if (value < maxPrefix) {
            out_.Add((byte)(flags + value));
            return;
        }
        out_.Add((byte)(flags + maxPrefix));
        value = value - maxPrefix;
        while (value >= 128) {
            out_.Add((byte)((value % 128) + 128));
            value = value / 128;
        }
        out_.Add((byte)value);
    }

    /// <summary>编码 HPACK 字符串（§5.2；非 Huffman——诚实边界）。</summary>
    private static void EncodeString(List<byte> out_, string s) {
        byte[] bytes = Encoding.GetBytes(s);
        EncodeInteger(out_, 7, 0, bytes.Length);
        int i = 0;
        while (i < bytes.Length) {
            out_.Add(bytes[i]);
            i = i + 1;
        }
    }

    private static void AppendBytes(List<byte> out_, byte[] bytes) {
        int i = 0;
        while (i < bytes.Length) {
            out_.Add(bytes[i]);
            i = i + 1;
        }
    }

    // ── 编码侧 ──

    /// <summary>请求/响应头列表 → HPACK 块（含头字段序列的合并语义由调用方决定）。</summary>
    internal byte[] EncodeHeaders(Http2HeaderList headers) {
        List<byte> block = new List<byte>();
        int i = 0;
        while (i < headers.Count) {
            string name = headers.GetName(i);
            string value = headers.GetValue(i);
            int exact = HpackStaticTable.Find(name, value);
            if (exact > 0) {
                // 索引表示（§6.1）
                EncodeInteger(block, 7, 128, exact);
            } else {
                int nameIdx = HpackStaticTable.FindName(name);
                if (nameIdx > 0) {
                    // 不索引字面量 + 索引名（§6.2.1）
                    EncodeInteger(block, 4, 0, nameIdx);
                    EncodeString(block, value);
                } else {
                    // 不索引字面量 + 新名（§6.2.1）
                    EncodeInteger(block, 4, 0, 0);
                    EncodeString(block, name);
                    EncodeString(block, value);
                }
            }
            i = i + 1;
        }
        return block.ToArray();
    }

    // ── 解码侧 ──

    /// <summary>HPACK 块 → 头列表。失败返回 false（非法码流）。</summary>
    internal bool DecodeHeaders(byte[] block, Http2HeaderList result) {
        ByteReader reader = new ByteReader(block);
        while (!reader.Eof()) {
            int b = reader.Next();
            if (b >= 128) {
                // 索引表示（§6.1）
                int idx = DecodeInteger(reader, 7, b);
                if (idx <= 0) { return false; }
                string[] entry = Lookup(idx);
                if (entry == null) { return false; }
                result.Add(entry[0], entry[1]);
            } else if (b >= 64) {
                // 增量索引字面量（§6.2.1/6.2.2）
                int idx = DecodeInteger(reader, 6, b);
                string name = "";
                if (idx == 0) {
                    byte[] nb = DecodeString(reader);
                    if (nb == null) { return false; }
                    name = Encoding.GetString(nb);
                } else {
                    string[] entry = Lookup(idx);
                    if (entry == null) { return false; }
                    name = entry[0];
                }
                byte[] vb = DecodeString(reader);
                if (vb == null) { return false; }
                string value = Encoding.GetString(vb);
                AddDynamic(name, value);
                result.Add(name, value);
            } else if (b >= 32) {
                // 动态表大小更新（§6.3）
                int size = DecodeInteger(reader, 5, b);
                ApplySizeUpdate(size);
            } else if (b >= 16) {
                // 永不索引字面量（§6.2.3）
                int idx = DecodeInteger(reader, 4, b);
                string name = "";
                if (idx == 0) {
                    byte[] nb = DecodeString(reader);
                    if (nb == null) { return false; }
                    name = Encoding.GetString(nb);
                } else {
                    string[] entry = Lookup(idx);
                    if (entry == null) { return false; }
                    name = entry[0];
                }
                byte[] vb = DecodeString(reader);
                if (vb == null) { return false; }
                result.Add(name, Encoding.GetString(vb));
            } else {
                // 不索引字面量（§6.2.1）
                int idx = DecodeInteger(reader, 4, b);
                string name = "";
                if (idx == 0) {
                    byte[] nb = DecodeString(reader);
                    if (nb == null) { return false; }
                    name = Encoding.GetString(nb);
                } else {
                    string[] entry = Lookup(idx);
                    if (entry == null) { return false; }
                    name = entry[0];
                }
                byte[] vb = DecodeString(reader);
                if (vb == null) { return false; }
                result.Add(name, Encoding.GetString(vb));
            }
        }
        return true;
    }

    private static int DecodeInteger(ByteReader reader, int prefix, int firstByte) {
        int maxPrefix = Pow2(prefix) - 1;
        int value = firstByte % (maxPrefix + 1);
        if (value < maxPrefix) { return value; }
        int m = 0;
        while (true) {
            int c = reader.Next();
            value = value + (c % 128) * Pow2(m);
            m = m + 7;
            if (c < 128) { break; }
        }
        return value;
    }

    private static byte[] DecodeString(ByteReader reader) {
        if (reader.Eof()) { return null; }
        int b = reader.Next();
        bool huffman = b >= 128;
        int len = DecodeInteger(reader, 7, b);
        if (len < 0 || reader.Position + len > reader.ByteCount) { return null; }
        byte[] raw = Http2ByteUtils.ZeroBytes(len);
        int i = 0;
        while (i < len) {
            raw[i] = reader.Next();
            i = i + 1;
        }
        if (huffman) {
            return HuffmanCodec.Decode(raw);
        }
        return raw;
    }

    /// <summary>表索引（静态 1..61；动态 ≥62，62=最近表项）→ 表项。</summary>
    private string[] Lookup(int index) {
        if (index >= 1 && index <= 61) {
            return [HpackStaticTable.GetName(index), HpackStaticTable.GetValue(index)];
        }
        int dyn = index - 62;
        if (dyn >= 0 && dyn < _dynamicTable.Count) {
            return _dynamicTable[dyn];
        }
        return null;
    }

    private void AddDynamic(string name, string value) {
        int entrySize = 32 + Encoding.GetByteCount(name) + Encoding.GetByteCount(value);
        _dynamicTable.Insert(0, [name, value]);
        _dynamicSize = _dynamicSize + entrySize;
        EvictIfNeeded();
    }

    private void ApplySizeUpdate(int size) {
        if (size > HeaderTableSizeLimit) {
            _dynamicTable.Clear();
            _dynamicSize = 0;
            return;
        }
        EvictIfNeeded();
    }

    private void EvictIfNeeded() {
        while (_dynamicSize > HeaderTableSizeLimit && _dynamicTable.Count > 0) {
            _dynamicSize = _dynamicSize - this.LastEntrySize();
            _dynamicTable.RemoveAt(_dynamicTable.Count - 1);
        }
    }

    private int LastEntrySize() {
        string[] last = _dynamicTable[_dynamicTable.Count - 1];
        return 32 + Encoding.GetByteCount(last[0]) + Encoding.GetByteCount(last[1]);
    }
}

/// <summary>HPACK 码流只读游标。</summary>
///
/// 字段名 `Length` 在 codegen 会被误降级为 `rt_array_length(receiver)`（读对象头
/// 前 8 字节脏值），故命名为 `ByteCount`（与 Http2Frame 同因；见该文件头注记）。
internal class ByteReader {
    public byte[] Data;
    public int Position;
    public int ByteCount;

    public ByteReader(byte[] data) {
        Data = data;
        ByteCount = data.Length;
        Position = 0;
    }

    public bool Eof() { return Position >= ByteCount; }

    public byte Next() {
        byte[] data = Data;
        byte b = data[Position];
        Position = Position + 1;
        return b;
    }
}
