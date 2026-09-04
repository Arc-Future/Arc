// Guid — C# System.Guid 诚实子集（RFC 037 M4 加深）。
//
// 存储：规范 D 格式小写字符串（8-4-4-4-12）。
// 已落地：NewGuid / Empty / Parse / TryParse / ParseExact / TryParseExact /
//         Compare / Equals / ToString / ToString("D"|"N"|"B"|"P") /
//         ToByteArray / FromByteArray（.NET 混合端序；纯 Arc）。
// 后置：自定义 format provider。
// 故意不提供 Guid(byte[]) ctor：Arc 中 string 与 byte[] 同为 UTF-8 载荷，字节 ctor 会吞掉字符串 ctor。
namespace Arc;

public struct Guid
{
    private string _str;

    public Guid(string s)
    {
        _str = s;
    }

    /// <summary>.NET 混合端序 16 字节工厂（不用 <c>Guid(byte[])</c> ctor——Arc 中 string 与 byte[]
    /// 同为 UTF-8 载荷，字节 ctor 会吞掉字符串 ctor）。长度非 16 → FormatException。</summary>
    public static Guid FromByteArray(byte[] b)
    {
        if (b == null || b.Length != 16)
        {
            throw new FormatException("Guid.FromByteArray: byte array must be Length 16");
        }
        // Inverse of .NET ToByteArray mixed-endian → canonical D.
        byte[] raw = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        raw[0] = b[3]; raw[1] = b[2]; raw[2] = b[1]; raw[3] = b[0];
        raw[4] = b[5]; raw[5] = b[4];
        raw[6] = b[7]; raw[7] = b[6];
        int i = 8;
        while (i < 16)
        {
            raw[i] = b[i];
            i = i + 1;
        }
        return new Guid(Guid.BytesToD(raw));
    }

    public static Guid NewGuid()
    {
        string s = rt_resources.rt_guid_new_string();
        return new Guid(s);
    }

    public static readonly Guid Empty = new Guid("00000000-0000-0000-0000-000000000000");

    /// <summary>解析 D/N/B/P 格式；失败抛 <see cref="FormatException"/>。</summary>
    public static Guid Parse(string s)
    {
        Guid g;
        if (!Guid.TryParse(s, out g))
        {
            throw new FormatException("Guid.Parse: invalid GUID string");
        }
        return g;
    }

    /// <summary>尝试解析 D/N/B/P；成功写出规范 D 小写。</summary>
    public static bool TryParse(string s, out Guid result)
    {
        result = Guid.Empty;
        if (s == null || s == "")
        {
            return false;
        }
        string t = s;
        // B: {D}  /  P: (D)
        if (t.Length >= 2)
        {
            string a = t.Substring(0, 1);
            string b = t.Substring(t.Length - 1, 1);
            if (a == "{" && b == "}")
            {
                t = t.Substring(1, t.Length - 2);
            }
            else if (a == "(" && b == ")")
            {
                t = t.Substring(1, t.Length - 2);
            }
        }
        string d = "";
        if (t.Length == 36)
        {
            if (!Guid.IsDashed(t))
            {
                return false;
            }
            d = t.ToLower();
        }
        else if (t.Length == 32)
        {
            if (!Guid.IsHexRun(t, 0, 32))
            {
                return false;
            }
            string n = t.ToLower();
            d = n.Substring(0, 8) + "-" + n.Substring(8, 4) + "-" + n.Substring(12, 4)
              + "-" + n.Substring(16, 4) + "-" + n.Substring(20, 12);
        }
        else
        {
            return false;
        }
        result = new Guid(d);
        return true;
    }

    /// <summary>按指定格式精确解析（<c>D</c>/<c>N</c>/<c>B</c>/<c>P</c>）；内容与格式不匹配抛 <see cref="FormatException"/>。</summary>
    public static Guid ParseExact(string s, string format)
    {
        Guid g;
        if (!Guid.TryParseExact(s, format, out g))
        {
            throw new FormatException("Guid.ParseExact: invalid GUID string for format");
        }
        return g;
    }

    /// <summary>按指定格式精确解析（<c>D</c>/<c>N</c>/<c>B</c>/<c>P</c>）；格式非法或内容不匹配返回 false。</summary>
    public static bool TryParseExact(string s, string format, out Guid result)
    {
        result = Guid.Empty;
        if (s == null || s == "")
        {
            return false;
        }
        if (format == null || format == "")
        {
            return false;
        }
        string f = format.ToLower();
        if (f == "d")
        {
            if (s.Length != 36)
            {
                return false;
            }
            if (!Guid.IsDashed(s))
            {
                return false;
            }
            result = new Guid(s.ToLower());
            return true;
        }
        if (f == "n")
        {
            if (s.Length != 32)
            {
                return false;
            }
            if (!Guid.IsHexRun(s, 0, 32))
            {
                return false;
            }
            string n = s.ToLower();
            result = new Guid(n.Substring(0, 8) + "-" + n.Substring(8, 4) + "-" + n.Substring(12, 4)
                + "-" + n.Substring(16, 4) + "-" + n.Substring(20, 12));
            return true;
        }
        if (f == "b" || f == "p")
        {
            string open = "{";
            string close = "}";
            if (f == "p")
            {
                open = "(";
                close = ")";
            }
            if (s.Length != 38)
            {
                return false;
            }
            if (s.Substring(0, 1) != open)
            {
                return false;
            }
            if (s.Substring(s.Length - 1, 1) != close)
            {
                return false;
            }
            string inner = s.Substring(1, s.Length - 2);
            if (!Guid.IsDashed(inner))
            {
                return false;
            }
            result = new Guid(inner.ToLower());
            return true;
        }
        return false;
    }

    public string ToString()
    {
        return _str;
    }

    /// <summary>格式：<c>D</c>（默认虚线）、<c>N</c>（无虚线）、<c>B</c>、<c>P</c>。其它 → FormatException。</summary>
    public string ToString(string format)
    {
        if (format == null || format == "")
        {
            return _str;
        }
        string f = format.ToLower();
        if (f == "d")
        {
            return _str;
        }
        if (f == "n")
        {
            return _str.Substring(0, 8) + _str.Substring(9, 4) + _str.Substring(14, 4)
                 + _str.Substring(19, 4) + _str.Substring(24, 12);
        }
        if (f == "b")
        {
            return "{" + _str + "}";
        }
        if (f == "p")
        {
            return "(" + _str + ")";
        }
        throw new FormatException("Guid.ToString: unsupported format (honest subset: D/N/B/P)");
    }

    /// <summary>.NET 混合端序 16 字节（Data1/2/3 小端，Data4 原序）。</summary>
    public byte[] ToByteArray()
    {
        string n = this.ToString("N");
        byte[] raw = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        int i = 0;
        while (i < 16)
        {
            raw[i] = Guid.ParseHexByte(n, i * 2);
            i = i + 1;
        }
        byte[] b = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        b[0] = raw[3]; b[1] = raw[2]; b[2] = raw[1]; b[3] = raw[0];
        b[4] = raw[5]; b[5] = raw[4];
        b[6] = raw[7]; b[7] = raw[6];
        i = 8;
        while (i < 16)
        {
            b[i] = raw[i];
            i = i + 1;
        }
        return b;
    }

    private static int HexValue(string ch)
    {
        if (ch == "0")
        {
            return 0;
        }
        if (ch == "1")
        {
            return 1;
        }
        if (ch == "2")
        {
            return 2;
        }
        if (ch == "3")
        {
            return 3;
        }
        if (ch == "4")
        {
            return 4;
        }
        if (ch == "5")
        {
            return 5;
        }
        if (ch == "6")
        {
            return 6;
        }
        if (ch == "7")
        {
            return 7;
        }
        if (ch == "8")
        {
            return 8;
        }
        if (ch == "9")
        {
            return 9;
        }
        if (ch == "a" || ch == "A")
        {
            return 10;
        }
        if (ch == "b" || ch == "B")
        {
            return 11;
        }
        if (ch == "c" || ch == "C")
        {
            return 12;
        }
        if (ch == "d" || ch == "D")
        {
            return 13;
        }
        if (ch == "e" || ch == "E")
        {
            return 14;
        }
        if (ch == "f" || ch == "F")
        {
            return 15;
        }
        return 0;
    }

    private static byte ParseHexByte(string n, int offset)
    {
        int hi = Guid.HexValue(n.Substring(offset, 1));
        int lo = Guid.HexValue(n.Substring(offset + 1, 1));
        return (byte)(hi * 16 + lo);
    }

    private static string ByteHex(byte v)
    {
        string hex = "0123456789abcdef";
        int x = (int)v;
        if (x < 0)
        {
            x = x + 256;
        }
        int hi = x / 16;
        int lo = x - hi * 16;
        return hex.Substring(hi, 1) + hex.Substring(lo, 1);
    }

    /// <summary>16 raw UUID-order bytes → D lowercase。</summary>
    private static string BytesToD(byte[] raw)
    {
        return Guid.ByteHex(raw[0]) + Guid.ByteHex(raw[1]) + Guid.ByteHex(raw[2]) + Guid.ByteHex(raw[3])
             + "-" + Guid.ByteHex(raw[4]) + Guid.ByteHex(raw[5])
             + "-" + Guid.ByteHex(raw[6]) + Guid.ByteHex(raw[7])
             + "-" + Guid.ByteHex(raw[8]) + Guid.ByteHex(raw[9])
             + "-" + Guid.ByteHex(raw[10]) + Guid.ByteHex(raw[11]) + Guid.ByteHex(raw[12])
             + Guid.ByteHex(raw[13]) + Guid.ByteHex(raw[14]) + Guid.ByteHex(raw[15]);
    }

    public static int Compare(Guid a, Guid b)
    {
        return a._str.Compare(b._str);
    }

    public static bool Equals(Guid a, Guid b)
    {
        return a._str == b._str;
    }

    private static bool IsDashed(string s)
    {
        if (s.Substring(8, 1) != "-")
        {
            return false;
        }
        if (s.Substring(13, 1) != "-")
        {
            return false;
        }
        if (s.Substring(18, 1) != "-")
        {
            return false;
        }
        if (s.Substring(23, 1) != "-")
        {
            return false;
        }
        if (!Guid.IsHexRun(s, 0, 8))
        {
            return false;
        }
        if (!Guid.IsHexRun(s, 9, 4))
        {
            return false;
        }
        if (!Guid.IsHexRun(s, 14, 4))
        {
            return false;
        }
        if (!Guid.IsHexRun(s, 19, 4))
        {
            return false;
        }
        if (!Guid.IsHexRun(s, 24, 12))
        {
            return false;
        }
        return true;
    }

    private static bool IsHexRun(string s, int start, int len)
    {
        int i = start;
        int end = start + len;
        bool ok = true;
        while (ok)
        {
            if (i >= end)
            {
                ok = false;
            }
            else
            {
                if (i >= s.Length)
                {
                    return false;
                }
                string ch = s.Substring(i, 1);
                bool hex = false;
                if (ch == "0" || ch == "1" || ch == "2" || ch == "3" || ch == "4"
                 || ch == "5" || ch == "6" || ch == "7" || ch == "8" || ch == "9")
                {
                    hex = true;
                }
                if (ch == "a" || ch == "b" || ch == "c" || ch == "d" || ch == "e" || ch == "f")
                {
                    hex = true;
                }
                if (ch == "A" || ch == "B" || ch == "C" || ch == "D" || ch == "E" || ch == "F")
                {
                    hex = true;
                }
                if (!hex)
                {
                    return false;
                }
                i = i + 1;
            }
        }
        return true;
    }
}
