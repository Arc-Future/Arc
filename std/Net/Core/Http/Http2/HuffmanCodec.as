// S2 (RFC 033 §2.4): Arc.Net — RFC 7541 §B 静态 Huffman 表 + 解码树。
//
// 纯 Arc 实现。语言缺位运算（§0.1 不得倒逼语言洞）：位提取以除法/取模算术仿真，
// 与 WebSocket S1 掩码 XOR 算术仿真同例。
//
// 诚实边界：完整 257 符号静态 Huffman 表（解码必做）；客户端**不主动 Huffman 编码**
//（请求头以非 Huffman 字面量编码，HPACK 合规）；解码接收对端 Huffman 串。

namespace Arc.Net;

using Arc.Collections;

/// <summary>RFC 7541 §B 静态 Huffman 编解码（解码树 + 算术位提取）。</summary>
internal class HuffmanCodec {
    /// <summary>EOS 符号（256）——结束标记。</summary>
    private const int EOS = 256;

    private static int[] _codes;
    private static int[] _lengths;
    private static bool _ready;

    // 解码树：节点数组（0 = 空子节点哨兵；root 恒为节点 0，symbol -1 表内部节点）。
    private static List<int> _treeLeft;
    private static List<int> _treeRight;
    private static List<int> _treeSymbol;

    /// <summary>2^i（i ∈ [0,30]；int 内）。</summary>
    private static int Pow2(int i) {
        int r = 1;
        int n = 0;
        while (n < i) { r = r * 2; n = n + 1; }
        return r;
    }

    /// <summary>懒初始化表与解码树。</summary>
    internal static void Ensure() {
        if (_ready) { return; }
        _codes = BuildCodes();
        _lengths = BuildLengths();
        BuildTree();
        _ready = true;
    }

    private static int AddNode() {
        List<int> left = _treeLeft;
        List<int> right = _treeRight;
        List<int> symbol = _treeSymbol;
        left.Add(0);
        right.Add(0);
        symbol.Add(-1);
        return left.Count - 1;
    }

    private static void BuildTree() {
        _treeLeft = new List<int>();
        _treeRight = new List<int>();
        _treeSymbol = new List<int>();
        List<int> left = _treeLeft;
        List<int> right = _treeRight;
        List<int> symbol = _treeSymbol;
        int[] codes = _codes;
        int[] lengths = _lengths;
        AddNode(); // root = node 0
        int sym = 0;
        while (sym <= EOS) {
            int code = codes[sym];
            int len = lengths[sym];
            int node = 0;
            int i = 0;
            while (i < len) {
                int bit = (code / Pow2(len - 1 - i)) % 2;
                if (bit == 0) {
                    int child = left[node];
                    if (child == 0) {
                        child = AddNode();
                        left[node] = child;
                    }
                    node = child;
                } else {
                    int child = right[node];
                    if (child == 0) {
                        child = AddNode();
                        right[node] = child;
                    }
                    node = child;
                }
                i = i + 1;
            }
            symbol[node] = sym;
            sym = sym + 1;
        }
    }

    /// <summary>
    /// Huffman 解码 HPACK 串（byte[] → byte[]）。EOS 符号视为解码错误返回 null；
    /// 末尾 ≤7 位全 1 padding（EOS 前缀）允许（RFC 7541 §5.2）。
    /// </summary>
    internal static byte[] Decode(byte[] data) {
        Ensure();
        if (data == null || data.Length == 0) { return Http2ByteUtils.ZeroBytes(0); }
        List<byte> out_ = new List<byte>();
        List<int> left = _treeLeft;
        List<int> right = _treeRight;
        List<int> symbol = _treeSymbol;
        int node = 0;
        int totalBits = data.Length * 8;
        int bitIdx = 0;
        while (bitIdx < totalBits) {
            int b = (int)data[bitIdx / 8];
            int bitPos = bitIdx % 8;
            int bit = (b / Pow2(7 - bitPos)) % 2;
            int next;
            if (bit == 0) { next = left[node]; }
            else { next = right[node]; }
            if (next == 0) { return null; } // 非法码字
            node = next;
            bitIdx = bitIdx + 1;
            int sym = symbol[node];
            if (sym >= 0) {
                if (sym == EOS) { return null; } // EOS 出现 = 解码错误
                out_.Add((byte)sym);
                node = 0;
            }
        }
        // 结束在内部节点 = 剩余位是 EOS 前缀 padding，允许。
        return out_.ToArray();
    }

    private static int[] BuildCodes() {
        return [
            0x1ff8, 0x7fffd8, 0xfffffe2, 0xfffffe3, 0xfffffe4, 0xfffffe5, 0xfffffe6, 0xfffffe7,
            0xfffffe8, 0xffffea, 0x3ffffffc, 0xfffffe9, 0xfffffea, 0x3ffffffd, 0xfffffeb, 0xfffffec,
            0xfffffed, 0xfffffee, 0xfffffef, 0xffffff0, 0xffffff1, 0xffffff2, 0x3ffffffe, 0xffffff3,
            0xffffff4, 0xffffff5, 0xffffff6, 0xffffff7, 0xffffff8, 0xffffff9, 0xffffffa, 0xffffffb,
            0x14, 0x3f8, 0x3f9, 0xffa, 0x1ff9, 0x15, 0xf8, 0x7fa,
            0x3fa, 0x3fb, 0xf9, 0x7fb, 0xfa, 0x16, 0x17, 0x18,
            0x0, 0x1, 0x2, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
            0x1e, 0x1f, 0x5c, 0xfb, 0x7ffc, 0x20, 0xffb, 0x3fc,
            0x1ffa, 0x21, 0x5d, 0x5e, 0x5f, 0x60, 0x61, 0x62,
            0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a,
            0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72,
            0xfc, 0x73, 0xfd, 0x1ffb, 0x7fff0, 0x1ffc, 0x3ffc, 0x22,
            0x7ffd, 0x3, 0x23, 0x4, 0x24, 0x5, 0x25, 0x26,
            0x27, 0x6, 0x74, 0x75, 0x28, 0x29, 0x2a, 0x7,
            0x2b, 0x76, 0x2c, 0x8, 0x9, 0x2d, 0x77, 0x78,
            0x79, 0x7a, 0x7b, 0x7ffe, 0x7fc, 0x3ffd, 0x1ffd, 0xffffffc,
            0xfffe6, 0x3fffd2, 0xfffe7, 0xfffe8, 0x3fffd3, 0x3fffd4, 0x3fffd5, 0x7fffd9,
            0x3fffd6, 0x7fffda, 0x7fffdb, 0x7fffdc, 0x7fffdd, 0x7fffde, 0xffffeb, 0x7fffdf,
            0xffffec, 0xffffed, 0x3fffd7, 0x7fffe0, 0xffffee, 0x7fffe1, 0x7fffe2, 0x7fffe3,
            0x7fffe4, 0x1fffdc, 0x3fffd8, 0x7fffe5, 0x3fffd9, 0x7fffe6, 0x7fffe7, 0xffffef,
            0x3fffda, 0x1fffdd, 0xfffe9, 0x3fffdb, 0x3fffdc, 0x7fffe8, 0x7fffe9, 0x1fffde,
            0x7fffea, 0x3fffdd, 0x3fffde, 0xfffff0, 0x1fffdf, 0x3fffdf, 0x7fffeb, 0x7fffec,
            0x1fffe0, 0x1fffe1, 0x3fffe0, 0x1fffe2, 0x7fffed, 0x3fffe1, 0x7fffee, 0x7fffef,
            0xfffea, 0x3fffe2, 0x3fffe3, 0x3fffe4, 0x7ffff0, 0x3fffe5, 0x3fffe6, 0x7ffff1,
            0x3ffffe0, 0x3ffffe1, 0xfffeb, 0x7fff1, 0x3fffe7, 0x7ffff2, 0x3fffe8, 0x1ffffec,
            0x3ffffe2, 0x3ffffe3, 0x3ffffe4, 0x7ffffde, 0x7ffffdf, 0x3ffffe5, 0xfffff1, 0x1ffffed,
            0x7fff2, 0x1fffe3, 0x3ffffe6, 0x7ffffe0, 0x7ffffe1, 0x3ffffe7, 0x7ffffe2, 0xfffff2,
            0x1fffe4, 0x1fffe5, 0x3ffffe8, 0x3ffffe9, 0xffffffd, 0x7ffffe3, 0x7ffffe4, 0x7ffffe5,
            0xfffec, 0xfffff3, 0xfffed, 0x1fffe6, 0x3fffe9, 0x1fffe7, 0x1fffe8, 0x7ffff3,
            0x3fffea, 0x3fffeb, 0x1ffffee, 0x1ffffef, 0xfffff4, 0xfffff5, 0x3ffffea, 0x7ffff4,
            0x3ffffeb, 0x7ffffe6, 0x3ffffec, 0x3ffffed, 0x7ffffe7, 0x7ffffe8, 0x7ffffe9, 0x7ffffea,
            0x7ffffeb, 0xffffffe, 0x7ffffec, 0x7ffffed, 0x7ffffee, 0x7ffffef, 0x7fffff0, 0x3ffffee,
            0x3fffffff
        ];
    }

    private static int[] BuildLengths() {
        return [
            13, 23, 28, 28, 28, 28, 28, 28,
            28, 24, 30, 28, 28, 30, 28, 28,
            28, 28, 28, 28, 28, 28, 30, 28,
            28, 28, 28, 28, 28, 28, 28, 28,
            6, 10, 10, 12, 13, 6, 8, 11,
            10, 10, 8, 11, 8, 6, 6, 6,
            5, 5, 5, 6, 6, 6, 6, 6,
            6, 6, 7, 8, 15, 6, 12, 10,
            13, 6, 7, 7, 7, 7, 7, 7,
            7, 7, 7, 7, 7, 7, 7, 7,
            7, 7, 7, 7, 7, 7, 7, 7,
            8, 7, 8, 13, 19, 13, 14, 6,
            15, 5, 6, 5, 6, 5, 6, 6,
            6, 5, 7, 7, 6, 6, 6, 5,
            6, 7, 6, 5, 5, 6, 7, 7,
            7, 7, 7, 15, 11, 14, 13, 28,
            20, 22, 20, 20, 22, 22, 22, 23,
            22, 23, 23, 23, 23, 23, 24, 23,
            24, 24, 22, 23, 24, 23, 23, 23,
            23, 21, 22, 23, 22, 23, 23, 24,
            22, 21, 20, 22, 22, 23, 23, 21,
            23, 22, 22, 24, 21, 22, 23, 23,
            21, 21, 22, 21, 23, 22, 23, 23,
            20, 22, 22, 22, 23, 22, 22, 23,
            26, 26, 20, 19, 22, 23, 22, 25,
            26, 26, 26, 27, 27, 26, 24, 25,
            19, 21, 26, 27, 27, 26, 27, 24,
            21, 21, 26, 26, 28, 27, 27, 27,
            20, 24, 20, 21, 22, 21, 21, 23,
            22, 22, 25, 25, 24, 24, 26, 23,
            26, 27, 26, 26, 27, 27, 27, 27,
            27, 28, 27, 27, 27, 27, 27, 26,
            30
        ];
    }
}
