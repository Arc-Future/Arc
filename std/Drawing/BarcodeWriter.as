// RFC 029 M3：1D 条形码生成（EAN-13 / Code39 / Code128）——纯 Arc 图案表实现。
//
// 设计（对齐 RFC 029 §1.4 ② + §3 M3）：
//   - 图案生成 / 校验位计算均为**纯函数**，与 Bitmap 解耦（可无 UI 独立验证）；
//   - 公有 API 返回 Bitmap（白底黑条、左右 quiet zone ≥10 模块、模块宽 2px）；
//   - 零 `[Builtin]`、零 `rt_barcode_*` ABI——本里程碑不引入任何 C ABI；
//   - 范围外（诚实标注，不实现）：BMP/TGA 编码（M1 标注 Draft）、
//     Code128 混合子集中途切换（Shift/Switch codes，本实现单子集自动选择）。
//     解码见 BarcodeReader（M4 原生 1D + M5 zxing 兜底）。
//
// 图案表以私有静态方法返回单条字符串字面量（Arc 静态类不允许字段——
// `check_static_class` 拒绝任何 field，const 亦不可；方法内联字面量随
// 可达性一并裁剪，与 `filter_reachable_mir_fns` 的 IR 级树剪配合）。
//
// 数据来源：EAN-13 L/G/R 编码表、Code39 9 元素图案表、Code128 值→图案表
// 均为标准公开规范；测试向量见 target/e2e/m3_selfcheck/（原
// barcode_writer_e2e 已随 arc-integration 退场，a2627a0f）。

namespace Arc.Drawing;

using Arc;
using Arc.Collections;

/// <summary>
/// 1D 条形码生成器（RFC 029 M3 · 纯 Arc）。支持 EAN-13 / Code39 / Code128。
///
/// 图案生成与校验位计算为纯函数（`Ean13CheckDigit`/`Ean13Pattern`/
/// `Code39Pattern`/`Code128Checksum`/`Code128Pattern`）——它们仅服务于
/// 无 UI 的单元验证（供包内测试/自检），**已声明 `internal`**，仅包内可见，
/// **不是**双轨 API：用户面唯一入口是 <see cref="EncodeEan13"/> / <see cref="EncodeCode39"/> /
/// <see cref="EncodeCode128"/> 三个返回 Bitmap 的方法。
/// </summary>
public static class BarcodeWriter {
    // ══════════ 纯逻辑辅助（供测试/内部验证；非双轨 API）══════════

    /// <summary>EAN-13 校验位（模 10 · 权重 1/3）。<paramref name="digits"/> 必须为前 12 位数字。
    /// **内部实现细节**（`internal`）——供包内测试/自检，非公共 API。</summary>
    internal static int Ean13CheckDigit(string digits) {
        if (digits == null || digits.Length != 12) {
            throw new ArgumentException("Ean13CheckDigit: 需要恰好 12 位数字");
        }
        int sum = 0;
        int i = 0;
        while (i < 12) {
            int d = _Digit(digits[i]);
            if (i % 2 == 0) {
                sum = sum + d;       // 权重 1
            } else {
                sum = sum + d * 3;   // 权重 3
            }
            i = i + 1;
        }
        int check = (10 - (sum % 10)) % 10;
        return check;
    }

    /// <summary>EAN-13 完整图案（95 模块 int[]，1 = 黑条 0 = 白空；含起止/中间 guard，不含 quiet zone）。
    /// 输入 12 位时自动补校验位；13 位时核验校验位。**内部实现细节**（`internal`）。</summary>
    internal static int[] Ean13Pattern(string digits) {
        if (digits == null) {
            throw new ArgumentException("Ean13Pattern: 输入为空");
        }
        string data = digits;
        if (digits.Length == 12) {
            data = digits + Ean13CheckDigit(digits).ToString();
        } else if (digits.Length == 13) {
            int expect = Ean13CheckDigit(digits.Substring(0, 12));
            int last = _Digit(digits[12]);
            if (last != expect) {
                throw new ArgumentException("Ean13Pattern: 校验位核验失败");
            }
        } else {
            throw new ArgumentException("Ean13Pattern: 需要 12 或 13 位数字");
        }

        int first = _Digit(data[0]);
        string mask = _EanMaskTable().Substring(first * 6, 6);

        List<int> result = new List<int>();
        BarcodeWriter._AddBits(result, "101");              // 起始 guard
        int i = 1;
        int k = 0;
        while (k < 6) {                          // 左半 6 位（L/G 由首数字掩码决定）
            int d = _Digit(data[i]);
            int kind = 0;
            if (mask[k] == '1') { kind = 1; }
            BarcodeWriter._AddPattern(result, _EanBits(d, kind));
            i = i + 1;
            k = k + 1;
        }
        BarcodeWriter._AddBits(result, "01010");            // 中间 guard
        k = 0;
        while (k < 6) {                          // 右半 6 位（R 编码）
            int d = _Digit(data[i]);
            BarcodeWriter._AddPattern(result, _EanBits(d, 2));
            i = i + 1;
            k = k + 1;
        }
        BarcodeWriter._AddBits(result, "101");              // 结束 guard
        return result.ToArray();
    }

    /// <summary>Code39 完整图案（int[]：**模块级 bar/space 序列**，宽元素 3 模块、
    /// 窄元素 1 模块，bar/space 交替，首尾 `*` guard + 字符间窄空 gap）。
    /// 字符集：0-9 / A-Z / - . 空格 $ / + % *。**内部实现细节**（`internal`）。</summary>
    internal static int[] Code39Pattern(string text) {
        if (text == null || text == "") {
            throw new ArgumentException("Code39Pattern: 输入为空");
        }
        // 元素级序列（1 = 宽元素，0 = 窄元素）：[ * ][gap][c0][gap][c1]...[cN][gap][ * ]。
        List<int> elements = new List<int>();
        BarcodeWriter._AddPattern(elements, _Code39CharPattern('*'));
        int i = 0;
        while (i < text.Length) {
            elements.Add(0);                     // 字符间窄空 gap
            BarcodeWriter._AddPattern(elements, _Code39CharPattern(text[i]));
            i = i + 1;
        }
        elements.Add(0);                         // 尾 '*' guard 前的窄空 gap
        BarcodeWriter._AddPattern(elements, _Code39CharPattern('*'));

        // 展开为模块级序列：宽元素 = 3 模块、窄元素 = 1 模块；元素 bar/space
        // 交替（Code39 每字符 5 bar + 4 space，首元素为 bar）。_Render 按模块 2px
        // 落位——修复「全元素等宽渲染」的非标准缺陷（宽/窄未区分）。
        List<int> modules = new List<int>();
        bool isBar = true;
        int k = 0;
        while (k < elements.Count) {
            int width = 1;
            if (elements[k] == 1) { width = 3; }
            int w = 0;
            while (w < width) {
                if (isBar) { modules.Add(1); } else { modules.Add(0); }
                w = w + 1;
            }
            isBar = !isBar;
            k = k + 1;
        }
        return modules.ToArray();
    }

    /// <summary>Code128 校验符（模 103 加权）。<paramref name="values"/> 首元素为 start code，
    /// 其后为数据值；返回校验符值（0..102）。**内部实现细节**（`internal`）。</summary>
    internal static int Code128Checksum(int[] values) {
        if (values == null || values.Length < 1) {
            throw new ArgumentException("Code128Checksum: 空输入");
        }
        int sum = values[0];   // start code 权重 1
        int i = 1;
        while (i < values.Length) {
            sum = sum + values[i] * i;
            i = i + 1;
        }
        return sum % 103;
    }

    /// <summary>Code128 完整图案（int[]：start + 数据 + 校验符 + stop）。
    /// 子集自动选择：C（全部数字且偶数位）优先，其次 B（可打印 ASCII 32..126），
    /// 再次 A（控制字符 0..95）；其余抛异常。**内部实现细节**（`internal`）。</summary>
    internal static int[] Code128Pattern(string text) {
        if (text == null || text == "") {
            throw new ArgumentException("Code128Pattern: 输入为空");
        }
        int[] values = _Code128Values(text);
        int checksum = Code128Checksum(values);
        List<int> result = new List<int>();
        BarcodeWriter._AddPattern(result, _Code128ValuePattern(values[0]));
        int i = 1;
        while (i < values.Length) {
            BarcodeWriter._AddPattern(result, _Code128ValuePattern(values[i]));
            i = i + 1;
        }
        BarcodeWriter._AddPattern(result, _Code128ValuePattern(checksum));
        BarcodeWriter._AddPattern(result, _Code128ValuePattern(106));
        return result.ToArray();
    }

    // ══════════ 公有 API（返回 Bitmap）══════════

    /// <summary>EAN-13 编码为 Bitmap。12 位输入自动计算第 13 位校验位；13 位核验。</summary>
    public static Bitmap EncodeEan13(string digits) {
        return _Render(Ean13Pattern(digits));
    }

    /// <summary>Code39 编码为 Bitmap（字符集见 <see cref="Code39Pattern"/>）。</summary>
    public static Bitmap EncodeCode39(string text) {
        return _Render(Code39Pattern(text));
    }

    /// <summary>Code128 编码为 Bitmap（子集 A/B/C 自动选择）。</summary>
    public static Bitmap EncodeCode128(string text) {
        return _Render(Code128Pattern(text));
    }

    // ══════════ 私有实现 ══════════

    /// <summary>EAN-13 L 编码表（7 位/字符 · 0-9 顺序拼接）。</summary>
    private static string _EanLTable() {
        return "0001101001100100100110111101010001101100010101111011101101101110001011";
    }

    /// <summary>EAN-13 G 编码表。</summary>
    private static string _EanGTable() {
        return "0100111011001100110110100001001110101110010000101001000100010010010111";
    }

    /// <summary>EAN-13 R 编码表（L 的按位取反）。</summary>
    private static string _EanRTable() {
        return "1110010110011011011001000010101110010011101010000100010010010001110100";
    }

    /// <summary>首数字 → 左半 6 位 G 集合掩码（6 位/数字，'1' = 该位用 G 编码）。</summary>
    private static string _EanMaskTable() {
        return "000000001011001101001110010011011001011100010101010110011010";
    }

    /// <summary>Code39 数字 0-9 表（9 元素/字符，1 = 宽）。</summary>
    private static string _Code39DigitsTable() {
        return "000110100100100001001100001101100000000110001100110000001110000000100101100100100001100100";
    }

    /// <summary>Code39 大写 A-Z 表。</summary>
    private static string _Code39UpperTable() {
        return "100001001001001001101001000000011001100011000001011000000001101100001100001001100000011100100000011001000011101000010000010011100010010001010010000000111100000110001000110000010110110000001011000001111000000010010001110010000011010000";
    }

    /// <summary>Code128 值 0-105 表（11 位/值）；106（Stop）13 位独立处理。</summary>
    private static string _Code128Table() {
        return "11011001100110011011001100110011010010011000100100011001000100110010011001000100110001001000110010011001001000110010001001100010010010110011100100110111001001100111010111001100100111011001001110011011001110010110010111001100100111011011100100110011101001110110111011101001100111001011001110010011011101100100111001101001110011001011011011000110110001101100011011010100011000100010110001000100011010110001000100011010001000110001011010001000110001010001100010001010110111000101100011101000110111010111011000101110001101000111011011101110110110100011101100010111011011101000110111000101101110111011101011000111010001101110001011011101101000111011000101110001101011101111010110010000101111000101010100110000101000011001001011000010010000110100001011001000010011010110010000101100001001001101000010011000010100001101001000011001011000010010110010100001111011101011000010100100011110101010011110010010111100100100111101011110010010011110100100111100101111010010011110010100111100100101101101111011011110110111101101101010111100010100011110100010111101011110100010111100010111101010001111010001010111011110101111011101110101111011110101110110100001001101001000011010011100";
    }

    /// <summary>EAN-13 单字符图案。kind：0=L，1=G，2=R。</summary>
    private static int[] _EanBits(int digit, int kind) {
        string table = _EanLTable();
        if (kind == 1) { table = _EanGTable(); }
        else if (kind == 2) { table = _EanRTable(); }
        return _FromBits(table.Substring(digit * 7, 7));
    }

    /// <summary>Code39 单字符 9 元素图案（1 = 宽）。</summary>
    private static int[] _Code39CharPattern(char c) {
        if (c >= '0' && c <= '9') {
            return _FromBits(_Code39DigitsTable().Substring(((int)c - 48) * 9, 9));
        }
        if (c >= 'A' && c <= 'Z') {
            return _FromBits(_Code39UpperTable().Substring(((int)c - 65) * 9, 9));
        }
        string bits = "";
        if (c == '-') { bits = "010000101"; }
        else if (c == '.') { bits = "110000100"; }
        else if (c == ' ') { bits = "011000100"; }
        else if (c == '$') { bits = "010101000"; }
        else if (c == '/') { bits = "010100010"; }
        else if (c == '+') { bits = "010001010"; }
        else if (c == '%') { bits = "000101010"; }
        else if (c == '*') { bits = "010010100"; }
        else {
            throw new ArgumentException("Code39Pattern: 不支持的字符");
        }
        return _FromBits(bits);
    }

    /// <summary>Code128 值 → 图案。0..105 查表（11 位）；106 = Stop（13 位）。</summary>
    private static int[] _Code128ValuePattern(int v) {
        if (v >= 0 && v <= 105) {
            return _FromBits(_Code128Table().Substring(v * 11, 11));
        }
        if (v == 106) {
            return _FromBits("1100011101011");
        }
        throw new ArgumentException("Code128Pattern: 值超出 0..106");
    }

    /// <summary>Code128 子集自动选择 + 值序列（首元素为 start code）。</summary>
    private static int[] _Code128Values(string text) {
        int n = text.Length;
        bool allDigits = true;
        int i = 0;
        while (i < n) {
            if (text[i] < '0' || text[i] > '9') { allDigits = false; }
            i = i + 1;
        }
        if (allDigits && n >= 2 && n % 2 == 0) {
            // Code C：成对数字 → 0..99
            List<int> vals = new List<int>();
            vals.Add(105);   // Start C
            int j = 0;
            while (j < n) {
                int d1 = _Digit(text[j]);
                int d2 = _Digit(text[j + 1]);
                vals.Add(d1 * 10 + d2);
                j = j + 2;
            }
            return vals.ToArray();
        }
        bool allB = true;
        i = 0;
        while (i < n) {
            if ((int)text[i] < 32 || (int)text[i] > 126) { allB = false; }
            i = i + 1;
        }
        if (allB) {
            // Code B：ASCII 32..126 → 值 = ASCII - 32
            List<int> vals = new List<int>();
            vals.Add(104);   // Start B
            i = 0;
            while (i < n) {
                vals.Add((int)text[i] - 32);
                i = i + 1;
            }
            return vals.ToArray();
        }
        bool allA = true;
        i = 0;
        while (i < n) {
            if ((int)text[i] > 95) { allA = false; }
            i = i + 1;
        }
        if (allA) {
            // Code A：ASCII 0..95 → 值 = ASCII
            List<int> vals = new List<int>();
            vals.Add(103);   // Start A
            i = 0;
            while (i < n) {
                vals.Add((int)text[i]);
                i = i + 1;
            }
            return vals.ToArray();
        }
        throw new ArgumentException("Code128Pattern: 字符超出 A/B/C 子集范围");
    }

    /// <summary>位串 → int[]（'1' = 1 黑条，'0' = 0 白空）。</summary>
    private static int[] _FromBits(string bits) {
        List<int> result = new List<int>();
        int i = 0;
        while (i < bits.Length) {
            if (bits[i] == '1') {
                result.Add(1);
            } else {
                result.Add(0);
            }
            i = i + 1;
        }
        return result.ToArray();
    }

    /// <summary>把 src 追加到 dst 尾部。</summary>
    /// 注意：本方法（及 <see cref="_AddBits"/>）必须**类名限定**调用
    /// （`BarcodeWriter._AddPattern(...)`）：首参为 `List<int>` 时，裸静态调用在
    /// MIR `resolve_static_overload` 中按实参类型解析失败，退化为无类前缀的自由调用
    /// （`@_AddPattern`/`@_AddBits`），LLVM 报 undefined symbol。
    /// 首参为泛型 List 的静态辅助方法一律类名限定。
    private static void _AddPattern(List<int> dst, int[] src) {
        int i = 0;
        while (i < src.Length) {
            dst.Add(src[i]);
            i = i + 1;
        }
    }

    /// <summary>把位串（'0'/'1'）追加到 dst 尾部。</summary>
    private static void _AddBits(List<int> dst, string bits) {
        BarcodeWriter._AddPattern(dst, _FromBits(bits));
    }

    private static int _Digit(char c) {
        if (c >= '0' && c <= '9') { return (int)c - 48; }
        throw new ArgumentException("期望数字字符");
    }

    /// <summary>模块序列 → Bitmap：白底黑条，左右 quiet zone 各 10 模块，模块宽 2px。
    /// std P2 效率批：实心矩形批量填充（白底 1 次 + 每黑条 1 次 FFI），替代逐像素循环。</summary>
    private static Bitmap _Render(int[] modules) {
        int moduleWidth = 2;
        int quietZone = 10;
        int height = 50;
        int width = (quietZone + modules.Length + quietZone) * moduleWidth;
        Bitmap bm = new Bitmap(width, height);
        long argbWhite = (long)255 * (long)16777216 + (long)255 * (long)65536 + (long)255 * (long)256 + (long)255;
        long argbBlack = (long)255 * (long)16777216;
        ImageNative.FillRect(bm.GetPixels(), width, height, 0, 0, width, height, argbWhite);
        int m = 0;
        while (m < modules.Length) {
            if (modules[m] == 1) {
                ImageNative.FillRect(bm.GetPixels(), width, height,
                    (quietZone + m) * moduleWidth, 0, moduleWidth, height, argbBlack);
            }
            m = m + 1;
        }
        return bm;
    }
}
