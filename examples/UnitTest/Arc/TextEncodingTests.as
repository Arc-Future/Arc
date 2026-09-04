namespace UnitTest.Arc;

using Arc;
using Arc.QIF;
using Arc.Text;

/// <summary>
/// Base64 / Hex / Encoding（UTF-8）编解码单元测试（RFC 035 M1 Stable 面）。
/// 覆盖开发者对文本进行 Base64 / 十六进制互转与 UTF-8 字节转换的真实场景。
/// </summary>
public class TextEncodingTests
{
    // ── Base64 ──

    [Fact]
    public void Base64_Encode_KnownValue()
    {
        Assert.Equal("SGVsbG8=", Base64.Encode("Hello"));
        Assert.Equal("aGVsbG8gd29ybGQ=", Base64.Encode("hello world"));
    }

    [Fact]
    public void Base64_Decode_KnownValue()
    {
        Assert.Equal("Hello", Base64.Decode("SGVsbG8="));
    }

    [Fact]
    public void Base64_RoundTrip()
    {
        string original = "dlang-arc base64 round trip 123";
        Assert.Equal(original, Base64.Decode(Base64.Encode(original)));
    }

    // ── Hex（字符串路径）──

    [Fact]
    public void Hex_Encode_KnownValue()
    {
        Assert.Equal("48656c6c6f", Hex.Encode("Hello"));
    }

    [Fact]
    public void Hex_Decode_KnownValue()
    {
        Assert.Equal("Hello", Hex.Decode("48656c6c6f"));
    }

    [Fact]
    public void Hex_RoundTrip()
    {
        string original = "hex payload 0x7f";
        Assert.Equal(original, Hex.Decode(Hex.Encode(original)));
    }

    // ── Hex（字节数组路径）──

    [Fact]
    public void Hex_ToHexString_KnownBytes()
    {
        byte[] data = [0x48, 0x65, 0x6c, 0x6c, 0x6f];
        Assert.Equal("48656c6c6f", Hex.ToHexString(data));
    }

    [Fact]
    public void Hex_FromHexString_KnownValue()
    {
        byte[] data = Hex.FromHexString("48656c6c6f");
        Assert.Equal(5, data.Length);
        Assert.Equal(0x48, (int)data[0]);
        Assert.Equal(0x6c, (int)data[2]);
        Assert.Equal(0x6f, (int)data[4]);
    }

    [Fact]
    public void Hex_Bytes_RoundTrip()
    {
        byte[] original = [0xde, 0xad, 0xbe, 0xef];
        byte[] restored = Hex.FromHexString(Hex.ToHexString(original));
        Assert.Equal(original.Length, restored.Length);
        Assert.Equal(0xde, (int)restored[0]);
        Assert.Equal(0xef, (int)restored[3]);
    }

    // ── Encoding（UTF-8）──

    [Fact]
    public void Encoding_GetBytes_KnownBytes()
    {
        byte[] data = Encoding.GetBytes("ABC");
        Assert.Equal(3, data.Length);
        Assert.Equal(0x41, (int)data[0]);
        Assert.Equal(0x43, (int)data[2]);
    }

    [Fact]
    public void Encoding_GetString_RoundTrip()
    {
        string original = "UTF-8 text";
        Assert.Equal(original, Encoding.GetString(Encoding.GetBytes(original)));
    }

    [Fact]
    public void Encoding_GetByteCount_AlignsWithLength()
    {
        Assert.Equal(5, Encoding.GetByteCount("hello"));
        Assert.Equal(0, Encoding.GetByteCount(""));
        // GetByteCount 与 GetBytes().Length 对齐。
        Assert.Equal(Encoding.GetBytes("arc").Length, Encoding.GetByteCount("arc"));
    }
}
