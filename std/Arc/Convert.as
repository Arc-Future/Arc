// Convert — C# System.Convert 诚实子集（类型转换门面）。
//
// 单一惯用法（RFC 002）：
//   - 数值/布尔/字符串互转 → Convert.To*
//   - 进制 2/8/10/16 → ToInt32(string, fromBase) / ToString(int, toBase)
//   - Base64 → Arc.Text.Base64（**不**在 Convert 上再暴露 ToBase64String，禁止双轨）
//
// 已落地：ToInt32 / ToInt64 / ToDouble / ToBoolean / ToString（string 与常用标量）；
//         ToByte / ToUInt32 / ToUInt64 / ToChar（string→窄/无符号/字符）；
//         进制 ToInt32/ToInt64(string, fromBase) / ToString(int, toBase)（2/8/10/16）。
// 后置：ChangeType / IConvertible / ToDateTime / ToDecimal / banker's ToInt32(double) /
//       文化感知 / DBNull / OverflowException（溢出暂以 FormatException 诚实抛出）/
//       非 2·8·10·16 进制。
namespace Arc;

/// <summary>类型转换门面，对齐 C# <c>System.Convert</c> 常用子集。</summary>
public static class Convert {
    // ── string → 数值 / 布尔 ──

    public static int ToInt32(string value) {
        if (value == null) { throw new FormatException("Convert.ToInt32: null"); }
        return int.Parse(value);
    }

    /// <summary>
    /// 按进制解析（仅 <c>2</c>/<c>8</c>/<c>10</c>/<c>16</c>）。
    /// 非 10 进制无符号位时，32 位无符号值按补码重释为 <c>int</c>（如 <c>"FFFFFFFF", 16</c> → <c>-1</c>）。
    /// 进制 16 允许可选 <c>0x</c>/<c>0X</c> 前缀。溢出以 <c>FormatException</c> 抛出（无 <c>OverflowException</c>）。
    /// </summary>
    public static int ToInt32(string value, int fromBase) {
        if (fromBase != 2 && fromBase != 8 && fromBase != 10 && fromBase != 16) {
            throw new ArgumentOutOfRangeException("fromBase");
        }
        if (value == null) { throw new FormatException("Convert.ToInt32: null"); }
        if (value == "") { throw new FormatException("Convert.ToInt32: empty"); }

        int i = 0;
        bool neg = false;
        string c0 = value.Substring(0, 1);
        if (c0 == "-") {
            neg = true;
            i = 1;
        } else if (c0 == "+") {
            i = 1;
        }
        if (i >= value.Length) { throw new FormatException("Convert.ToInt32: no digits"); }

        if (fromBase == 16 && i + 1 < value.Length) {
            string p0 = value.Substring(i, 1);
            string p1 = value.Substring(i + 1, 1);
            if (p0 == "0" && (p1 == "x" || p1 == "X")) {
                i = i + 2;
                if (i >= value.Length) { throw new FormatException("Convert.ToInt32: no digits after 0x"); }
            }
        }

        long mag = 0;
        // 避免 (long)uint.Max 的强转瑕疵：用 long.Parse 得到 0xFFFFFFFF
        long maxU32 = long.Parse("4294967295");
        long maxI32 = 2147483647;
        long minAbs = 2147483647;
        minAbs = minAbs + 1; // 2147483648，避免字面量越界
        bool any = false;
        bool go = true;
        while (go) {
            if (i >= value.Length) { go = false; }
            else {
                int d = Convert._digitValue(value.Substring(i, 1));
                if (d < 0 || d >= fromBase) {
                    throw new FormatException("Convert.ToInt32: invalid digit");
                }
                if (mag > (maxU32 / fromBase)) {
                    throw new FormatException("Convert.ToInt32: overflow");
                }
                long next = mag * fromBase + d;
                if (fromBase != 10 && next > maxU32) {
                    throw new FormatException("Convert.ToInt32: overflow");
                }
                mag = next;
                any = true;
                i = i + 1;
            }
        }
        if (!any) { throw new FormatException("Convert.ToInt32: no digits"); }

        if (neg) {
            if (mag > minAbs) {
                throw new FormatException("Convert.ToInt32: overflow");
            }
            if (mag == minAbs) { return 0 - 2147483647 - 1; }
            return 0 - (int)mag;
        }

        if (fromBase == 10) {
            if (mag > maxI32) {
                throw new FormatException("Convert.ToInt32: overflow");
            }
            return (int)mag;
        }

        // 非 10：0..0xFFFFFFFF 按 32 位补码重释
        if (mag > maxI32) {
            return (int)(mag - (maxU32 + 1));
        }
        return (int)mag;
    }

    public static long ToInt64(string value) {
        if (value == null) { throw new FormatException("Convert.ToInt64: null"); }
        return long.Parse(value);
    }

    /// <summary>
    /// 按进制解析为 <c>long</c>（仅 <c>2</c>/<c>8</c>/<c>10</c>/<c>16</c>；RFC 023 进制转换面）。
    /// 非 10 进制无符号位时，64 位无符号值按补码重释为 <c>long</c>
    /// （如 <c>"FFFFFFFFFFFFFFFF", 16</c> → <c>-1</c>）。
    /// 进制 16 允许可选 <c>0x</c>/<c>0X</c> 前缀；负号前缀对非 10 进制仅接受可安全取反的幅值。
    /// 溢出以 <c>FormatException</c> 抛出（无 <c>OverflowException</c>）。
    /// </summary>
    public static long ToInt64(string value, int fromBase)
    {
        if (fromBase != 2 && fromBase != 8 && fromBase != 10 && fromBase != 16)
        {
            throw new ArgumentOutOfRangeException("fromBase");
        }
        if (value == null) { throw new FormatException("Convert.ToInt64: null"); }
        if (value == "") { throw new FormatException("Convert.ToInt64: empty"); }
        if (fromBase == 10)
        {
            return long.Parse(value);
        }

        int i = 0;
        bool neg = false;
        string c0 = value.Substring(0, 1);
        if (c0 == "-")
        {
            neg = true;
            i = 1;
        }
        else if (c0 == "+")
        {
            i = 1;
        }
        if (i >= value.Length) { throw new FormatException("Convert.ToInt64: no digits"); }

        if (fromBase == 16 && i + 1 < value.Length)
        {
            string p0 = value.Substring(i, 1);
            string p1 = value.Substring(i + 1, 1);
            if (p0 == "0" && (p1 == "x" || p1 == "X"))
            {
                i = i + 2;
                if (i >= value.Length) { throw new FormatException("Convert.ToInt64: no digits after 0x"); }
            }
        }

        // 非 10 进制：按 64 位无符号累积（long 两补码回绕即无符号重释）。
        // 位宽上限 = 该进制可容纳 64 位无符号值的最大位数；八进制 22 位时最高位须 ≤ 1。
        int maxDigits = 16;
        if (fromBase == 2) { maxDigits = 64; }
        else if (fromBase == 8) { maxDigits = 22; }

        long acc = 0;
        int digits = 0;
        int leadingDigit = -1;
        bool any = false;
        bool go = true;
        while (go)
        {
            if (i >= value.Length) { go = false; }
            else
            {
                int d = Convert._digitValue(value.Substring(i, 1));
                if (d < 0 || d >= fromBase)
                {
                    throw new FormatException("Convert.ToInt64: invalid digit");
                }
                any = true;
                digits = digits + 1;
                if (digits == 1) { leadingDigit = d; }
                if (digits > maxDigits)
                {
                    throw new FormatException("Convert.ToInt64: overflow");
                }
                acc = acc * fromBase + d;
                i = i + 1;
            }
        }
        if (!any) { throw new FormatException("Convert.ToInt64: no digits"); }
        if (fromBase == 8 && digits == maxDigits && leadingDigit > 1)
        {
            throw new FormatException("Convert.ToInt64: overflow");
        }

        if (neg)
        {
            // 回绕后为负 → 幅值 ≥ 2^63，负号溢出。
            if (acc < 0) { throw new FormatException("Convert.ToInt64: overflow"); }
            return 0 - acc;
        }
        return acc;
    }

    public static double ToDouble(string value) {
        if (value == null) { throw new FormatException("Convert.ToDouble: null"); }
        return double.Parse(value);
    }

    public static byte ToByte(string value) {
        if (value == null) { throw new FormatException("Convert.ToByte: null"); }
        return byte.Parse(value);
    }

    public static uint ToUInt32(string value) {
        if (value == null) { throw new FormatException("Convert.ToUInt32: null"); }
        return uint.Parse(value);
    }

    public static ulong ToUInt64(string value) {
        if (value == null) { throw new FormatException("Convert.ToUInt64: null"); }
        return ulong.Parse(value);
    }

    public static char ToChar(string value) {
        if (value == null) { throw new FormatException("Convert.ToChar: null"); }
        return char.Parse(value);
    }

    /// <summary>接受 <c>True</c>/<c>False</c>/<c>true</c>/<c>false</c>/<c>1</c>/<c>0</c>。</summary>
    public static bool ToBoolean(string value) {
        if (value == null) { throw new FormatException("Convert.ToBoolean: null"); }
        if (value == "True" || value == "true" || value == "1") { return true; }
        if (value == "False" || value == "false" || value == "0") { return false; }
        throw new FormatException("Convert.ToBoolean: invalid boolean string");
    }

    // ── 标量互转 ──

    public static int ToInt32(bool value) {
        if (value) { return 1; }
        return 0;
    }

    public static int ToInt32(long value) {
        return (int)value;
    }

    /// <summary>向零截断（诚实：非 C# Convert banker's rounding）。</summary>
    public static int ToInt32(double value) {
        return (int)value;
    }

    public static long ToInt64(int value) {
        return (long)value;
    }

    public static long ToInt64(double value) {
        return (long)value;
    }

    public static double ToDouble(int value) {
        return (double)value;
    }

    public static double ToDouble(long value) {
        return (double)value;
    }

    public static bool ToBoolean(int value) {
        return value != 0;
    }

    // ── ToString ──

    public static string ToString(int value) {
        return value.ToString();
    }

    /// <summary>
    /// 按进制格式化（仅 <c>2</c>/<c>8</c>/<c>10</c>/<c>16</c>）。
    /// 非 10 进制对负数按 32 位补码无符号位模式输出（如 <c>ToString(-1, 16)</c> → <c>"ffffffff"</c>）；十六进制小写。
    /// </summary>
    public static string ToString(int value, int toBase) {
        if (toBase != 2 && toBase != 8 && toBase != 10 && toBase != 16) {
            throw new ArgumentOutOfRangeException("toBase");
        }
        if (toBase == 10) { return value.ToString(); }

        long n = (long)value;
        if (value < 0) {
            long u32mod = long.Parse("4294967295");
            n = n + u32mod + 1;
        }
        if (n == 0) { return "0"; }

        string hex = "0123456789abcdef";
        string digits = "";
        bool go = true;
        while (go) {
            if (n == 0) { go = false; }
            else {
                int rem = (int)(n - (n / toBase) * toBase);
                digits = hex.Substring(rem, 1) + digits;
                n = n / toBase;
            }
        }
        return digits;
    }

    public static string ToString(long value) {
        return value.ToString();
    }

    public static string ToString(double value) {
        return value.ToString();
    }

    public static string ToString(bool value) {
        return value.ToString();
    }

    public static string ToString(string value) {
        if (value == null) { return ""; }
        return value;
    }

    // ── 私有 ──

    private static int _digitValue(string ch) {
        if (ch == "0") { return 0; }
        if (ch == "1") { return 1; }
        if (ch == "2") { return 2; }
        if (ch == "3") { return 3; }
        if (ch == "4") { return 4; }
        if (ch == "5") { return 5; }
        if (ch == "6") { return 6; }
        if (ch == "7") { return 7; }
        if (ch == "8") { return 8; }
        if (ch == "9") { return 9; }
        if (ch == "a" || ch == "A") { return 10; }
        if (ch == "b" || ch == "B") { return 11; }
        if (ch == "c" || ch == "C") { return 12; }
        if (ch == "d" || ch == "D") { return 13; }
        if (ch == "e" || ch == "E") { return 14; }
        if (ch == "f" || ch == "F") { return 15; }
        return -1;
    }
}
