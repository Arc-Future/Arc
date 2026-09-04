namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Convert / Guid / TimeSpan / DateTime L2 加深（非 Fact-Skip）。
/// </summary>
public class ConvertGuidTimeTests
{
    // ── Convert ──

    [Fact]
    public void Convert_ToInt32_String()
    {
        Assert.Equal(42, Convert.ToInt32("42"));
        Assert.Equal(-7, Convert.ToInt32("-7"));
    }

    [Fact]
    public void Convert_ToInt64_And_Double()
    {
        Assert.True(Convert.ToInt64("10000000000") == 10000000000);
        double d = Convert.ToDouble("3.5");
        Assert.True(d > 3.49 && d < 3.51);
    }

    [Fact]
    public void Convert_ToBoolean_HonestSubset()
    {
        Assert.True(Convert.ToBoolean("True"));
        Assert.True(Convert.ToBoolean("1"));
        Assert.False(Convert.ToBoolean("false"));
        Assert.False(Convert.ToBoolean("0"));
        Assert.Equal(1, Convert.ToInt32(true));
        Assert.Equal(0, Convert.ToInt32(false));
    }

    [Fact]
    public void Convert_ToString_Scalars()
    {
        Assert.Equal("42", Convert.ToString(42));
        Assert.Equal("True", Convert.ToString(true));
    }

    [Fact]
    public void Convert_ToInt32_Double_Truncates()
    {
        // 诚实：向零截断，非 banker's rounding
        Assert.Equal(3, Convert.ToInt32(3.9));
        Assert.Equal(-3, Convert.ToInt32(-3.9));
    }

    [Fact]
    public void Convert_ToInt32_Radix()
    {
        Assert.Equal(255, Convert.ToInt32("FF", 16));
        Assert.Equal(255, Convert.ToInt32("0xff", 16));
        Assert.Equal(42, Convert.ToInt32("101010", 2));
        Assert.Equal(63, Convert.ToInt32("77", 8));
        Assert.Equal(-16, Convert.ToInt32("-10", 16));
        // 非 10：32 位补码重释
        Assert.Equal(-1, Convert.ToInt32("FFFFFFFF", 16));
    }

    [Fact]
    public void Convert_ToString_Radix()
    {
        Assert.Equal("ff", Convert.ToString(255, 16));
        Assert.Equal("101010", Convert.ToString(42, 2));
        Assert.Equal("77", Convert.ToString(63, 8));
        Assert.Equal("42", Convert.ToString(42, 10));
        // 非 10：负数按补码无符号位模式
        Assert.Equal("ffffffff", Convert.ToString(-1, 16));
    }

    [Fact]
    public void Convert_ToInt64_Radix()
    {
        Assert.True(Convert.ToInt64("FF", 16) == 255);
        Assert.True(Convert.ToInt64("0x1FF", 16) == 511);
        Assert.True(Convert.ToInt64("101010", 2) == 42);
        Assert.True(Convert.ToInt64("77", 8) == 63);
        Assert.True(Convert.ToInt64("-10", 16) == -16);
        Assert.True(Convert.ToInt64("1000000000000", 16) == 281474976710656);
        // 非 10：64 位补码重释
        Assert.True(Convert.ToInt64("FFFFFFFFFFFFFFFF", 16) == -1);
        Assert.True(Convert.ToInt64("1777777777777777777777", 8) == -1);
        Assert.True(Convert.ToInt64("1111111111111111111111111111111111111111111111111111111111111111", 2) == -1);
        long max = Convert.ToInt64("7FFFFFFFFFFFFFFF", 16);
        Assert.True(max == 9223372036854775807);
        long min = Convert.ToInt64("8000000000000000", 16);
        Assert.True(min < 0);
    }

    [Fact]
    public void Convert_ToInt64_Radix_Overflow_Throws()
    {
        bool threw16 = false;
        try {
            Convert.ToInt64("10000000000000000", 16);
        } catch (FormatException ex) {
            threw16 = true;
            Assert.True(ex.Message.Length > 0);
        }
        Assert.True(threw16);

        bool threw8 = false;
        try {
            Convert.ToInt64("2000000000000000000000", 8);
        } catch (FormatException ex) {
            threw8 = true;
            Assert.True(ex.Message.Length > 0);
        }
        Assert.True(threw8);

        bool threwNeg = false;
        try {
            Convert.ToInt64("-FFFFFFFFFFFFFFFF", 16);
        } catch (FormatException ex) {
            threwNeg = true;
            Assert.True(ex.Message.Length > 0);
        }
        Assert.True(threwNeg);
    }

    [Fact]
    public void Convert_ToByte_UInt_Char()
    {
        Assert.Equal(200, (int)Convert.ToByte("200"));
        uint u = Convert.ToUInt32("4294967295");
        Assert.True(u == uint.Parse("4294967295"));
        ulong ul = Convert.ToUInt64("18446744073709551615");
        Assert.True(ul == ulong.Parse("18446744073709551615"));
        char a = Convert.ToChar("A");
        Assert.True(a == 'A');
    }

    [Fact]
    public void Convert_ToInt32_BadBase_Throws()
    {
        bool threw = false;
        try {
            Convert.ToInt32("10", 3);
        } catch (ArgumentOutOfRangeException ex) {
            threw = true;
            Assert.True(ex.Message.Length > 0);
        }
        Assert.True(threw);
    }

    // ── Guid ──

    [Fact]
    public void Guid_Empty_And_Parse_D()
    {
        Guid e = Guid.Empty;
        Assert.Equal("00000000-0000-0000-0000-000000000000", e.ToString());
        Guid g = Guid.Parse("A1B2C3D4-E5F6-7890-ABCD-EF1234567890");
        Assert.Equal("a1b2c3d4-e5f6-7890-abcd-ef1234567890", g.ToString());
        Assert.Equal("a1b2c3d4e5f67890abcdef1234567890", g.ToString("N"));
        Assert.Equal("{a1b2c3d4-e5f6-7890-abcd-ef1234567890}", g.ToString("B"));
    }

    [Fact]
    public void Guid_Parse_N_And_B()
    {
        Guid g = Guid.Parse("a1b2c3d4e5f67890abcdef1234567890");
        Assert.Equal("a1b2c3d4-e5f6-7890-abcd-ef1234567890", g.ToString("D"));
        Guid b = Guid.Parse("{a1b2c3d4-e5f6-7890-abcd-ef1234567890}");
        Assert.True(Guid.Equals(g, b));
    }

    [Fact]
    public void Guid_TryParse_Invalid()
    {
        Guid g;
        Assert.False(Guid.TryParse("not-a-guid", out g));
        Assert.False(Guid.TryParse("", out g));
    }

    [Fact]
    public void Guid_NewGuid_Format()
    {
        Guid g = Guid.NewGuid();
        string s = g.ToString();
        Assert.Equal(36, s.Length);
        Assert.Equal("-", s.Substring(8, 1));
        Assert.Equal("4", s.Substring(14, 1)); // UUID v4
    }

    [Fact]
    public void Guid_Compare()
    {
        Guid a = Guid.Parse("00000000-0000-0000-0000-000000000001");
        Guid b = Guid.Parse("00000000-0000-0000-0000-000000000002");
        Assert.True(Guid.Compare(a, b) < 0);
        Assert.Equal(0, Guid.Compare(a, a));
    }

    [Fact]
    public void Guid_ToByteArray_NetEndian_RoundTrip()
    {
        // D: a1b2c3d4-e5f6-7890-abcd-ef1234567890
        // .NET ToByteArray: d4 c3 b2 a1 | f6 e5 | 90 78 | ab cd ef 12 34 56 78 90
        // 不用 Guid(byte[]) ctor（Arc string/byte[] 同载荷会吞字符串 ctor）→ FromByteArray
        Guid g = Guid.Parse("a1b2c3d4-e5f6-7890-abcd-ef1234567890");
        byte[] b = g.ToByteArray();
        Assert.Equal(16, b.Length);
        Assert.Equal(0xd4, (int)b[0]);
        Assert.Equal(0xc3, (int)b[1]);
        Assert.Equal(0xb2, (int)b[2]);
        Assert.Equal(0xa1, (int)b[3]);
        Assert.Equal(0xf6, (int)b[4]);
        Assert.Equal(0xe5, (int)b[5]);
        Assert.Equal(0x90, (int)b[6]);
        Assert.Equal(0x78, (int)b[7]);
        Assert.Equal(0xab, (int)b[8]);
        Assert.Equal(0x90, (int)b[15]);
        Guid g2 = Guid.FromByteArray(b);
        Assert.True(Guid.Equals(g, g2));
        Assert.Equal("a1b2c3d4-e5f6-7890-abcd-ef1234567890", g2.ToString());
    }

    [Fact]
    public void Guid_FromByteArray_BadLength_Throws()
    {
        bool threw = false;
        try {
            byte[] bad = [1, 2, 3];
            Guid g = Guid.FromByteArray(bad);
            Assert.True(g.ToString().Length > 0);
        } catch (FormatException ex) {
            threw = true;
            Assert.True(ex.Message.Length > 0);
        }
        Assert.True(threw);
    }

    [Fact]
    public void Guid_Parse_Throws()
    {
        bool threw = false;
        try {
            Guid.Parse("bad");
        } catch (FormatException ex) {
            threw = true;
            Assert.True(ex.Message.Length > 0);
        }
        Assert.True(threw);
    }

    [Fact]
    public void Guid_ParseExact_Formats()
    {
        Guid g = Guid.ParseExact("A1B2C3D4-E5F6-7890-ABCD-EF1234567890", "D");
        Assert.Equal("a1b2c3d4-e5f6-7890-abcd-ef1234567890", g.ToString());
        Guid n = Guid.ParseExact("a1b2c3d4e5f67890abcdef1234567890", "N");
        Assert.True(Guid.Equals(g, n));
        Guid b = Guid.ParseExact("{a1b2c3d4-e5f6-7890-abcd-ef1234567890}", "B");
        Assert.True(Guid.Equals(g, b));
        Guid p = Guid.ParseExact("(a1b2c3d4-e5f6-7890-abcd-ef1234567890)", "P");
        Assert.True(Guid.Equals(g, p));
    }

    [Fact]
    public void Guid_ParseExact_Mismatch_Throws()
    {
        bool threw = false;
        try {
            Guid.ParseExact("a1b2c3d4e5f67890abcdef1234567890", "D");
        } catch (FormatException ex) {
            threw = true;
            Assert.True(ex.Message.Length > 0);
        }
        Assert.True(threw);
    }

    [Fact]
    public void Guid_TryParseExact_FormatEnforced()
    {
        Guid g;
        Assert.True(Guid.TryParseExact("a1b2c3d4-e5f6-7890-abcd-ef1234567890", "D", out g));
        Assert.False(Guid.TryParseExact("a1b2c3d4-e5f6-7890-abcd-ef1234567890", "N", out g));
        Assert.False(Guid.TryParseExact("{a1b2c3d4-e5f6-7890-abcd-ef1234567890}", "D", out g));
        Assert.True(Guid.TryParseExact("{a1b2c3d4-e5f6-7890-abcd-ef1234567890}", "B", out g));
        Assert.False(Guid.TryParseExact("(a1b2c3d4-e5f6-7890-abcd-ef1234567890)", "B", out g));
        Assert.False(Guid.TryParseExact("x", "D", out g));
        Assert.False(Guid.TryParseExact("a1b2c3d4-e5f6-7890-abcd-ef1234567890", "X", out g));
        Assert.False(Guid.TryParseExact(null, "D", out g));
        Assert.False(Guid.TryParseExact("a1b2c3d4-e5f6-7890-abcd-ef1234567890", "", out g));
    }

    // ── TimeSpan ──

    [Fact]
    public void TimeSpan_Parse_RoundTrip()
    {
        TimeSpan ts = new TimeSpan(1, 2, 3, 4, 5);
        TimeSpan p = TimeSpan.Parse(ts.ToString());
        Assert.True(TimeSpan.Equals(ts, p));
    }

    [Fact]
    public void TimeSpan_Parse_Hms()
    {
        TimeSpan ts = TimeSpan.Parse("01:02:03");
        Assert.Equal(1, ts.Hours);
        Assert.Equal(2, ts.Minutes);
        Assert.Equal(3, ts.Seconds);
    }

    [Fact]
    public void TimeSpan_Parse_Negative()
    {
        TimeSpan ts = TimeSpan.Parse("-00:00:05");
        Assert.True(ts.Ticks < 0);
        Assert.Equal(-5, ts.Seconds);
    }

    [Fact]
    public void TimeSpan_Parse_PureDays()
    {
        TimeSpan ts = TimeSpan.Parse("3");
        Assert.Equal(3, ts.Days);
        Assert.Equal(0, ts.Hours);
        Assert.True(TimeSpan.Equals(ts, TimeSpan.FromDays(3.0)));
        TimeSpan neg = TimeSpan.Parse("-2");
        Assert.Equal(-2, neg.Days);
        Assert.True(neg.Ticks < 0);
    }

    [Fact]
    public void TimeSpan_TryParse_Invalid()
    {
        TimeSpan ts;
        Assert.False(TimeSpan.TryParse("nope", out ts));
    }

    // ── DateTime ──

    [Fact]
    public void DateTime_MinMax()
    {
        DateTime min = DateTime.MinValue;
        Assert.True(min.Ticks == 0);
        DateTime max = DateTime.MaxValue;
        Assert.True(max.Ticks > min.Ticks);
        Assert.Equal(9999, max.Year);
        Assert.Equal(12, max.Month);
        Assert.Equal(31, max.Day);
    }

    [Fact]
    public void DateTime_SpecifyKind()
    {
        DateTime dt = DateTime.FromYMD(2024, 3, 15);
        DateTime utc = DateTime.SpecifyKind(dt, 1);
        Assert.Equal(1, utc.Kind);
        Assert.True(dt.Ticks == utc.Ticks);
    }

    [Fact]
    public void DateTime_Parse_And_TryParse()
    {
        DateTime dt = DateTime.Parse("2024-03-15");
        Assert.Equal(2024, dt.Year);
        Assert.Equal(3, dt.Month);
        Assert.Equal(15, dt.Day);
        DateTime full = DateTime.Parse("2024-03-15T14:30:45");
        Assert.Equal(14, full.Hour);
        Assert.Equal(30, full.Minute);
        Assert.Equal(45, full.Second);
        DateTime bad;
        Assert.False(DateTime.TryParse("not-a-date", out bad));
    }

    [Fact]
    public void DateTime_Parse_Throws()
    {
        bool threw = false;
        try {
            DateTime.Parse("xx");
        } catch (FormatException ex) {
            threw = true;
            Assert.True(ex.Message.Length > 0);
        }
        Assert.True(threw);
    }

    [Fact]
    public void DateTime_Subtract_TimeSpan_And_AddTicks()
    {
        DateTime dt = DateTime.FromYMDHMS(2024, 3, 15, 12, 0, 0);
        DateTime earlier = dt.Subtract(TimeSpan.FromHours(2.0));
        Assert.Equal(10, earlier.Hour);
        long oneSec = 10000000;
        DateTime later = dt.AddTicks(oneSec);
        Assert.Equal(1, later.Second);
    }

    [Fact]
    public void DateTime_Subtract_DateTime()
    {
        DateTime a = DateTime.FromYMD(2024, 3, 16);
        DateTime b = DateTime.FromYMD(2024, 3, 15);
        TimeSpan diff = a.Subtract(b);
        Assert.Equal(1, diff.Days);
    }
}
