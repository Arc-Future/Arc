// S4 (RFC 033 §2.6): Arc.Net — QPACK 编解码（RFC 9204）。
//
// 纯 Arc 实现。语言缺位运算（§0.1 不得倒逼语言洞）：掩码/续位以除法/取模算术仿真
// （与 std/Net/Core/Http/Http2/Hpack.as 同例）。
//
// 诚实边界（RFC 9204 允许的合法子集）：
//   - 静态表：完整 99 项（QpackStaticTable，§A.1，索引 0..98）。
//   - 动态表：最小——插入经显式 Insert（对应 encoder stream 指令解析后的状态）；
//     编码/解码双侧均支持前基动态引用（§3.2.5，rel = Base - 1 - absolute）；
//     post-Base 仅解码侧（§4.5.3/§4.5.5；编码侧 Base 恒 = RIC，无 post-Base 场景）；
//     Required Insert Count 编码/重建按 RFC 9204 §4.5.1.1 全算法
//     （含 2*MaxEntries 取模防回绕）；超限驱逐，上限 4096 字节。
//   - 字段行模式：编码 §4.5.2 / §4.5.4（静态名）/ §4.5.6；解码 §4.5.2-§4.5.6 全六种；
//     N 位（never-indexed）解码忽略——仅约束中间转发，不影响本端解码。
//   - Huffman：不支持（H 位为 1 即拒绝；§4.4 允许非 Huffman 编码）。

namespace Arc.Net;

using Arc.Collections;
using Arc.Text;

/// <summary>RFC 9204 QPACK 编解码器。单连接实例（动态表按连接状态演进）。</summary>
internal class Qpack {
    private const int TableCapacityLimit = 4096;

    // 动态表：绝对索引 0 = 最旧；新插入追加在末尾（absolute = _dynamic.Count - 1）。
    // 语言能力缺口：`List<T[]>` 数组泛型元素 Add 损坏元素（typeck 归约为
    // Named("..._arr")，元素槽尺寸错配），故以引用类 QpackEntry 承载表项。
    private List<QpackEntry> _dynamic;
    private int _dynamicSize;
    private int _dynamicLimit;
    private int _totalInserts; // RFC 9204 §4.5.1.1：解码侧 Insert Count（含已驱逐）

    public Qpack() {
        _dynamic = new List<QpackEntry>();
        _dynamicSize = 0;
        _dynamicLimit = TableCapacityLimit;
        _totalInserts = 0;
    }

    /// <summary>设置动态表容量（连接方 SETTINGS QPACK_MAX_TABLE_CAPACITY）。</summary>
    internal void SetDynamicTableCapacity(int capacity) {
        _dynamicLimit = capacity;
        while (_dynamicSize > _dynamicLimit && _dynamic.Count > 0) {
            this.EvictOldest();
        }
    }

    /// <summary>动态表插入（对应 encoder stream Insert 指令处理后的状态更新）。</summary>
    internal void Insert(string name, string value) {
        int cost = name.Length + value.Length + 32;
        while (_dynamicSize + cost > _dynamicLimit && _dynamic.Count > 0) {
            this.EvictOldest();
        }
        if (cost > _dynamicLimit) { return; } // 单条超限：不插入（§3.2.1）
        _dynamic.Add(new QpackEntry(name, value));
        _dynamicSize = _dynamicSize + cost;
        _totalInserts = _totalInserts + 1;
    }

    private void EvictOldest() {
        QpackEntry e = _dynamic[0];
        _dynamicSize = _dynamicSize - (e.Name.Length + e.Value.Length + 32);
        _dynamic.RemoveAt(0);
    }

    // ── 整数/字符串编解码（RFC 9204 §4.4 与 HPACK 相同的 varint 形态）──

    private static int Pow2(int i) {
        int r = 1;
        int n = 0;
        while (n < i) { r = r * 2; n = n + 1; }
        return r;
    }

    /// <summary>按 RFC 7541 §5.1 / RFC 9204 §4.4 编码整数（前缀 N 位）。</summary>
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

    /// <summary>编码 QPACK 字符串（§4.4；H 位恒 0——诚实边界）。</summary>
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

    /// <summary>头字段序列 → QPACK 字段段（含 prefix）。动态表精确命中用前基相对索引。</summary>
    internal byte[] EncodeHeaders(Http3HeaderList headers) {
        int count = headers.Count;
        // 两遍：先确定动态引用（静态精确命中优先；否则动态；否则字面量）。
        int maxRefAbs = -1;
        int i = 0;
        while (i < count) {
            string name = headers.GetName(i);
            string value = headers.GetValue(i);
            int st = QpackStaticTable.Find(name, value);
            if (st < 0) {
                int dyn = this.FindDynamic(name, value);
                if (dyn >= 0 && dyn > maxRefAbs) { maxRefAbs = dyn; }
            }
            i = i + 1;
        }
        int ric = 0;
        if (maxRefAbs >= 0) { ric = maxRefAbs + 1; }

        List<byte> block = new List<byte>();
        // field section prefix：Required Insert Count（8 位前缀）+ Delta Base（7 位前缀，Sign=0）。
        EncodeInteger(block, 8, 0, EncodeRequiredInsertCount(ric));
        EncodeInteger(block, 7, 0, 0);

        i = 0;
        while (i < count) {
            string name = headers.GetName(i);
            string value = headers.GetValue(i);
            int st = QpackStaticTable.Find(name, value);
            if (st >= 0) {
                // 索引字段行（§4.5.2，T=1）：首字节 0b11xxxxxx。
                EncodeInteger(block, 6, 192, st);
            } else {
                int dyn = this.FindDynamic(name, value);
                if (dyn >= 0) {
                    // 索引字段行引用动态表（前基，§4.5.2，T=0）：rel = Base - 1 - absolute。
                    int rel = ric - 1 - dyn;
                    EncodeInteger(block, 6, 128, rel);
                } else {
                    int nameIdx = QpackStaticTable.FindName(name);
                    if (nameIdx >= 0) {
                        // 字面量字段行 + 静态名字引用（§4.5.4，N=0 T=1）：首字节 0x50 起。
                        EncodeInteger(block, 4, 80, nameIdx);
                        EncodeString(block, value);
                    } else {
                        // 字面量字段行 + 字面量名字（§4.5.6，N=0 H=0）：001，名字长度 3 位前缀。
                        byte[] nameBytes = Encoding.GetBytes(name);
                        EncodeInteger(block, 3, 32, nameBytes.Length);
                        AppendBytes(block, nameBytes);
                        EncodeString(block, value);
                    }
                }
            }
            i = i + 1;
        }
        return block.ToArray();
    }

    /// <summary>动态表精确匹配（新→旧）返回绝对索引；未命中 -1。</summary>
    private int FindDynamic(string name, string value) {
        int a = _dynamic.Count - 1;
        while (a >= 0) {
            QpackEntry e = _dynamic[a];
            if (e.Name == name && e.Value == value) { return a; }
            a = a - 1;
        }
        return -1;
    }

    /// <summary>RFC 9204 §4.5.1.1：RIC → EncInsertCount（0 保持 0，否则 mod 2*MaxEntries + 1）。</summary>
    private static int EncodeRequiredInsertCount(int ric) {
        if (ric == 0) { return 0; }
        int maxEntries = TableCapacityLimit / 32;
        int fullRange = maxEntries * 2;
        return (ric % fullRange) + 1;
    }

    // ── 解码侧 ──

    /// <summary>QPACK 字段段 → 头列表。失败返回 false（非法码流/未知动态索引）。</summary>
    internal bool DecodeHeaders(byte[] block, Http3HeaderList result) {
        QpackReader reader = new QpackReader(block);
        if (reader.Eof()) { return false; }
        int encRic = DecodeInteger(reader, 8, reader.Next());
        if (reader.Eof()) { return false; }
        int deltaByte = reader.Next();
        bool sign = deltaByte >= 128; // §4.5.1.2：S 位 = 首字节 bit 7
        int delta = DecodeInteger(reader, 7, deltaByte);
        int ric = DecodeRequiredInsertCount(encRic, _totalInserts);
        if (ric < 0) { return false; }
        if (ric > _totalInserts) { return false; } // blocked（encoder stream 未到位）
        int baseIdx;
        if (sign) {
            baseIdx = ric - delta - 1;
        } else {
            baseIdx = ric + delta;
        }
        if (baseIdx < 0) { return false; }
        while (!reader.Eof()) {
            int b = reader.Next();
            if (b >= 192) {
                // 索引字段行（§4.5.2，T=1）：静态表索引。
                int idx = DecodeInteger(reader, 6, b);
                if (idx >= QpackStaticTable.EntryCount()) { return false; }
                result.Add(QpackStaticTable.GetName(idx), QpackStaticTable.GetValue(idx));
            } else if (b >= 128) {
                // 索引字段行（§4.5.2，T=0）：前基相对索引。
                int rel = DecodeInteger(reader, 6, b);
                QpackEntry entry = this.LookupDynamic(rel, baseIdx);
                if (entry == null) { return false; }
                result.Add(entry.Name, entry.Value);
            } else if (b >= 64) {
                // 字面量字段行 + 名字引用（§4.5.4）：01 N T idx(4+)；N 位解码忽略。
                int idx = DecodeInteger(reader, 4, b);
                bool nameStatic = (b / 16) % 2 == 1; // T 位
                string entryName = null;
                if (nameStatic) {
                    if (idx >= QpackStaticTable.EntryCount()) { return false; }
                    entryName = QpackStaticTable.GetName(idx);
                } else {
                    QpackEntry entry = this.LookupDynamic(idx, baseIdx);
                    if (entry == null) { return false; }
                    entryName = entry.Name;
                }
                byte[] vb = DecodeString(reader);
                if (vb == null) { return false; }
                result.Add(entryName, Encoding.GetString(vb));
            } else if (b >= 32) {
                // 字面量字段行 + 字面量名字（§4.5.6）：001 N H len(3+)。
                byte[] nb = DecodeStringWith(reader, 3, b);
                if (nb == null) { return false; }
                byte[] vb = DecodeString(reader);
                if (vb == null) { return false; }
                result.Add(Encoding.GetString(nb), Encoding.GetString(vb));
            } else if (b >= 16) {
                // 索引字段行 + post-Base 索引（§4.5.3）：0001 idx(4+)；absolute = Base + rel。
                int rel = DecodeInteger(reader, 4, b);
                int absolute = baseIdx + rel;
                if (absolute < 0 || absolute >= _dynamic.Count) { return false; }
                QpackEntry e = _dynamic[absolute];
                result.Add(e.Name, e.Value);
            } else {
                // 字面量字段行 + post-Base 名字引用（§4.5.5）：0000 N idx(3+)；N 位解码忽略。
                int rel = DecodeInteger(reader, 3, b);
                int absolute = baseIdx + rel;
                if (absolute < 0 || absolute >= _dynamic.Count) { return false; }
                QpackEntry entry = _dynamic[absolute];
                byte[] vb = DecodeString(reader);
                if (vb == null) { return false; }
                result.Add(entry.Name, Encoding.GetString(vb));
            }
        }
        return true;
    }

    /// <summary>RFC 9204 §4.5.1.1：EncInsertCount + 解码侧 Insert Count(tni) → RIC。
    /// 返回 ≤0 表示非法编码。</summary>
    private static int DecodeRequiredInsertCount(int encRic, int tni) {
        if (encRic == 0) { return 0; }
        int maxEntries = TableCapacityLimit / 32;
        int fullRange = maxEntries * 2;
        if (encRic > fullRange) { return -1; }
        int maxValue = tni + maxEntries;
        int maxWrapped = (maxValue / fullRange) * fullRange;
        int ric = maxWrapped + encRic - 1;
        if (ric > maxValue) {
            if (ric <= fullRange) { return -1; }
            ric = ric - fullRange;
        }
        return ric;
    }

    /// <summary>前基相对索引 → 动态表项（§3.2.5）：absolute = Base - 1 - rel。</summary>
    private QpackEntry LookupDynamic(int rel, int baseIdx) {
        if (rel >= baseIdx) { return null; } // rel ≥ Base 须用 §4.5.3/§4.5.5 post-Base 形式
        int absolute = baseIdx - 1 - rel;
        if (absolute < 0 || absolute >= _dynamic.Count) { return null; }
        return _dynamic[absolute];
    }

    private static int DecodeInteger(QpackReader reader, int prefix, int firstByte) {
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

    private static byte[] DecodeString(QpackReader reader) {
        if (reader.Eof()) { return null; }
        int b = reader.Next();
        return DecodeStringWith(reader, 7, b);
    }

    /// <summary>字符串字面量（§4.1.2），前缀 P 位首字节已读（H 位 = bit P）。</summary>
    private static byte[] DecodeStringWith(QpackReader reader, int prefix, int firstByte) {
        if ((firstByte / Pow2(prefix)) % 2 == 1) { return null; } // Huffman（H=1）不支持——诚实边界
        int len = DecodeInteger(reader, prefix, firstByte);
        if (len < 0 || reader.Position + len > reader.ByteCount) { return null; }
        byte[] raw = Http3ByteUtils.ZeroBytes(len);
        int i = 0;
        while (i < len) {
            raw[i] = (byte)reader.Next();
            i = i + 1;
        }
        return raw;
    }
}

    /// <summary>顺序字节读取器（QPACK 解码）。</summary>
    internal class QpackReader {
        private byte[] _data;
        private int _pos;
        private int _len;

        public QpackReader(byte[] data) {
            _data = data;
            _pos = 0;
            // 语言：`byte[]` 字段直读不支持 .Length/索引（typeck 归约为 Named("byte_arr")），
            // 长度在构造期经参数取一次（与 Http2Frame 同例）。
            _len = data.Length;
        }

        internal bool Eof() { return _pos >= _len; }
        internal int Position { get { return _pos; } }
        internal int ByteCount { get { return _len; } }

        internal int Next() {
            if (this.Eof()) { return -1; }
            byte[] d = _data;
            int v = d[_pos];
            _pos = _pos + 1;
            return v;
        }
    }

    /// <summary>QPACK 动态表项（引用类承载，规避 List<T[]> 语言能力缺口）。</summary>
    internal class QpackEntry {
        public string Name;
        public string Value;

        public QpackEntry(string name, string value) {
            Name = name;
            Value = value;
        }
    }
