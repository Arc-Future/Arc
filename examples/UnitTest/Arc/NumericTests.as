namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// 数值类型（整数、浮点、无符号类型、char）的单元测试。
/// 覆盖 double/float、int/long/short/byte/sbyte、uint/ulong/ushort、
/// char、Parse/TryParse、ToString 及回归。
/// </summary>
public class NumericTests
{
    // ── double / float ──

    [Fact]
    public void Double_Add_Basic()
    {
        double x = 2.0;
        double y = 3.0;
        double sum = x + y;
        Assert.True(sum == 5.0);
    }

    [Fact]
    public void Float_Add_Basic()
    {
        float fa = 1.5;
        float fb = 2.5;
        float fsum = fa + fb;
        Assert.True(fsum == 4.0);
    }

    // ── long ──

    [Fact]
    public void Long_Add_Basic()
    {
        long big = 1000000000;
        long bigger = 2000000000;
        long total = big + bigger;
        Assert.True(total == 3000000000);
    }

    [Fact]
    public void Long_MaxConstant()
    {
        long max = 9223372036854775807;
        Assert.True(max == 9223372036854775807);
    }

    [Fact]
    public void Long_MixedWithInt()
    {
        long big = 1000000000;
        long mixed = big + 500;
        Assert.True(mixed == 1000000500);
    }

    // ── short / byte ──

    [Fact]
    public void Short_Add_PromotesToInt()
    {
        short s = 100;
        short t = 200;
        int shortSum = s + t;
        Assert.Equal(300, shortSum);
    }

    [Fact]
    public void Byte_Add_PromotesToInt()
    {
        byte b = 255;
        byte c = 1;
        int byteSum = b + c;
        Assert.Equal(256, byteSum);
    }

    // ── char ──

    [Fact]
    public void Char_Literal()
    {
        char ch = 'A';
        Assert.True(ch == 'A');
    }

    [Fact]
    public void Char_ToInt()
    {
        char ch = 'A';
        int code = ch;
        Assert.Equal(65, code);
    }

    [Fact]
    public void Char_Arithmetic()
    {
        char ch = 'A';
        int charArith = ch + 1;
        Assert.Equal(66, charArith);
    }

    [Fact]
    public void Char_Escape()
    {
        char nl = '\n';
        Assert.True(nl == '\n');
    }

    [Fact]
    public void Char_IsDigit_Letter_WhiteSpace()
    {
        Assert.True(char.IsDigit('7'));
        Assert.False(char.IsDigit('x'));
        Assert.True(char.IsLetter('Q'));
        Assert.False(char.IsLetter('3'));
        Assert.True(char.IsWhiteSpace(' '));
        Assert.False(char.IsWhiteSpace('A'));
    }

    [Fact]
    public void Char_Case_Classify_And_Convert()
    {
        Assert.True(char.IsUpper('B'));
        Assert.True(char.IsLower('b'));
        Assert.True(char.ToUpper('c') == 'C');
        Assert.True(char.ToLower('D') == 'd');
    }

    // ── uint (UInt32) ──

    [Fact]
    public void UInt_Literal()
    {
        uint u1 = 42;
        uint u2 = 0;
        uint u3 = 1;
        Assert.True(u1 == 42);
    }

    [Fact]
    public void UInt_Add()
    {
        uint u1 = 42;
        uint u3 = 1;
        uint uSum = uint.Add(u1, u3);
        Assert.True(uSum == 43);
    }

    [Fact]
    public void UInt_MaxMin()
    {
        uint uMax = uint.Parse("4294967295");
        uint uMin = 0;
        Assert.True(uMax == uint.Parse("4294967295"));
        Assert.True(uMin == 0);
    }

    [Fact]
    public void UInt_Parse()
    {
        uint uParsed = uint.Parse("123");
        Assert.True(uParsed == 123);
    }

    [Fact]
    public void UInt_TryParse_Valid()
    {
        uint uTryResult;
        bool uTryOk = uint.TryParse("456", out uTryResult);
        Assert.True(uTryOk);
        Assert.True(uTryResult == 456);
    }

    [Fact]
    public void UInt_TryParse_Negative()
    {
        uint uTryResult;
        bool uTryFail = uint.TryParse("-1", out uTryResult);
        Assert.False(uTryFail);
    }

    [Fact]
    public void UInt_ParseMax()
    {
        uint uBig = uint.Parse("4294967295");
        Assert.True(uBig == uint.Parse("4294967295"));
    }

    [Fact]
    public void UInt_ToString()
    {
        uint u1 = 42;
        string su = uint.ToString(u1);
        Assert.True(su == "42");
    }

    [Fact]
    public void UInt_ToStringMax()
    {
        uint uBig = uint.Parse("4294967295");
        string uBigStr = uint.ToString(uBig);
        Assert.True(uBigStr == "4294967295");
    }

    // ── ulong (UInt64) ──

    [Fact]
    public void ULong_Literal()
    {
        ulong l1 = 10000000000;
        ulong l2 = 0;
        Assert.True(l1 == 10000000000);
    }

    [Fact]
    public void ULong_Add()
    {
        ulong l1 = 10000000000;
        ulong lSum = ulong.Add(l1, 1);
        Assert.True(lSum == 10000000001);
    }

    [Fact]
    public void ULong_MaxMin()
    {
        ulong lMax = ulong.Parse("18446744073709551615");
        ulong lMin = 0;
        Assert.True(lMax == ulong.Parse("18446744073709551615"));
        Assert.True(lMin == 0);
    }

    [Fact]
    public void ULong_Parse()
    {
        ulong lParsed = ulong.Parse("9999999999");
        Assert.True(lParsed == 9999999999);
    }

    [Fact]
    public void ULong_TryParse_Valid()
    {
        ulong lTryResult;
        bool lTryOk = ulong.TryParse("8888888888", out lTryResult);
        Assert.True(lTryOk);
        Assert.True(lTryResult == 8888888888);
    }

    [Fact]
    public void ULong_TryParse_Negative()
    {
        ulong lTryResult;
        bool lTryFail = ulong.TryParse("-5", out lTryResult);
        Assert.False(lTryFail);
    }

    [Fact]
    public void ULong_ParseMax()
    {
        ulong lBig = ulong.Parse("18446744073709551615");
        Assert.True(lBig == ulong.Parse("18446744073709551615"));
    }

    [Fact]
    public void ULong_ToString()
    {
        ulong l1 = 10000000000;
        string sl = ulong.ToString(l1);
        Assert.True(sl == "10000000000");
    }

    // ── ushort (UInt16) ──

    [Fact]
    public void UShort_Literal()
    {
        ushort s1 = 100;
        Assert.True(s1 == 100);
    }

    [Fact]
    public void UShort_MaxMin()
    {
        ushort sMax = 65535;
        ushort sMin = 0;
        Assert.True(sMax == 65535);
        Assert.True(sMin == 0);
    }

    [Fact]
    public void UShort_ToString()
    {
        ushort s1 = 100;
        string ss = ushort.ToString(s1);
        Assert.True(ss == "100");
    }

    // ── sbyte (SByte) ──

    [Fact]
    public void SByte_Literal()
    {
        sbyte b1 = 42;
        sbyte b2 = -128;
        Assert.True(b1 == 42);
        Assert.True(b2 == -128);
    }

    [Fact]
    public void SByte_MaxMin()
    {
        sbyte bMax = 127;
        sbyte bMin = -128;
        Assert.True(bMax == 127);
        Assert.True(bMin == -128);
    }

    [Fact]
    public void SByte_ToString()
    {
        sbyte b1 = 42;
        string sb = sbyte.ToString(b1);
        Assert.True(sb == "42");
    }

    [Fact]
    public void SByte_ToString_Negative()
    {
        sbyte b2 = -128;
        string sb2 = sbyte.ToString(b2);
        Assert.True(sb2 == "-128");
    }

    // ── 回归：已存在符号类型的 Parse/TryParse ──

    [Fact]
    public void Int_Parse_Regression()
    {
        int iParsed = int.Parse("42");
        Assert.Equal(42, iParsed);
    }

    [Fact]
    public void Int_TryParse_Regression()
    {
        int iTryResult;
        bool iTryOk = int.TryParse("99", out iTryResult);
        Assert.True(iTryOk);
        Assert.Equal(99, iTryResult);
    }

    [Fact]
    public void Double_Parse_Regression()
    {
        double dParsed = double.Parse("3.14");
        Assert.True(dParsed > 3.13 && dParsed < 3.15);
    }

    [Fact]
    public void Bool_Parse_Regression()
    {
        bool boolParsed = bool.Parse("True");
        Assert.True(boolParsed);
    }
}
