namespace UnitTest.Arc;

using Arc;
using Arc.IO;
using Arc.QIF;
using Arc.Text;

/// <summary>
/// MemoryStream 同步面单元测试——对标 C# System.IO.MemoryStream 的读取契约：
/// Position/Seek/SetLength 边界、EOF 读返回 0、越界/寻址前抛对应异常、处置后访问抛
/// ObjectDisposedException。全部为纯内存操作，确定性、无磁盘 I/O、无共享状态（并行安全）。
/// </summary>
public class MemoryStreamTests
{
    private static MemoryStream Make(string content)
    {
        MemoryStream ms = new MemoryStream();
        byte[] bytes = Encoding.GetBytes(content);
        ms.Write(bytes, 0, bytes.Length);
        ms.Position = 0;
        return ms;
    }

    // ── 初始状态 ──

    [Fact]
    public void Empty_InitialState()
    {
        MemoryStream ms = new MemoryStream();
        Assert.True(ms.Length == 0);
        Assert.True(ms.Position == 0);
        Assert.True(ms.CanRead);
        Assert.True(ms.CanWrite);
        Assert.True(ms.CanSeek);
    }

    // ── 读写往返 ──

    [Fact]
    public void WriteThenRead_Roundtrip()
    {
        MemoryStream ms = Make("roundtrip");
        byte[] buf = new byte[9];
        int n = ms.Read(buf, 0, 9);
        Assert.True(n == 9);
        Assert.True(Encoding.GetString(buf) == "roundtrip");
    }

    [Fact]
    public void WriteThenRead_EmptyString_ReturnsZero()
    {
        MemoryStream ms = new MemoryStream();
        byte[] buf = new byte[4];
        int n = ms.Read(buf, 0, 4);
        Assert.True(n == 0);
    }

    // ── EOF 与部分读 ──

    [Fact]
    public void Read_Eof_ReturnsZero()
    {
        MemoryStream ms = Make("abc");
        byte[] buf = new byte[3];
        int first = ms.Read(buf, 0, 3);
        Assert.True(first == 3);
        int second = ms.Read(buf, 0, 3);
        Assert.True(second == 0);
        Assert.True(ms.Position == 3);
    }

    [Fact]
    public void Read_Partial_ReturnsRemainingAndAdvances()
    {
        MemoryStream ms = Make("hello");
        byte[] buf = new byte[10];
        // 请求 count=10，剩余仅 5 → 返回 5，不越界读。
        int n = ms.Read(buf, 0, 10);
        Assert.True(n == 5);
        Assert.True(ms.Position == 5);
    }

    // ── 覆盖写与 Position 推进 ──

    [Fact]
    public void Write_OverwritesExistingBytes()
    {
        MemoryStream ms = Make("abc");
        ms.Position = 0;
        byte[] data = Encoding.GetBytes("xyz");
        ms.Write(data, 0, 1); // 只覆写首字节
        Assert.True(ms.Length == 3);
        ms.Position = 0;
        byte[] buf = new byte[3];
        int n = ms.Read(buf, 0, 3);
        Assert.True(n == 3);
        Assert.True(Encoding.GetString(buf) == "xbc");
    }

    // ── Seek ──

    [Fact]
    public void Seek_BeginCurrentEnd()
    {
        MemoryStream ms = Make("abcdef");
        Assert.True(ms.Seek(2, SeekOrigin.Begin) == 2);
        Assert.True(ms.Position == 2);
        Assert.True(ms.Seek(2, SeekOrigin.Current) == 4);
        Assert.True(ms.Position == 4);
        // End 基址 = Length(6)，offset -1 → 定位到最后一个字节。
        Assert.True(ms.Seek(-1, SeekOrigin.End) == 5);
        Assert.True(ms.Position == 5);
    }

    [Fact]
    public void Seek_BeforeBegin_ThrowsIo()
    {
        MemoryStream ms = Make("abc");
        bool caught = false;
        try {
            ms.Seek(-1, SeekOrigin.Begin);
        } catch (IOException) {
            caught = true;
        }
        Assert.True(caught);
        // 失败后位置保持不变
        Assert.True(ms.Position == 0);
    }

    // ── Position 边界 ──

    [Fact]
    public void Position_GetSetRoundtrip()
    {
        MemoryStream ms = Make("abcd");
        ms.Position = 3;
        Assert.True(ms.Position == 3);
        byte[] buf = new byte[1];
        int n = ms.Read(buf, 0, 1);
        Assert.True(n == 1);
        Assert.True(ms.Position == 4);
    }

    [Fact]
    public void Position_Negative_Throws()
    {
        MemoryStream ms = Make("abc");
        bool caught = false;
        try {
            ms.Position = -1;
        } catch (ArgumentOutOfRangeException) {
            caught = true;
        }
        Assert.True(caught);
        Assert.True(ms.Position == 0);
    }

    // ── SetLength ──

    [Fact]
    public void SetLength_ExtendThenTruncate()
    {
        MemoryStream ms = Make("abcd");
        ms.SetLength(6);
        Assert.True(ms.Length == 6);
        Assert.True(ms.Position == 0); // 初始 Position=0（Make 已归零）
        ms.SetLength(2);
        Assert.True(ms.Length == 2);
        ms.Position = 0;
        byte[] buf = new byte[2];
        int n = ms.Read(buf, 0, 2);
        Assert.True(n == 2);
        Assert.True(Encoding.GetString(buf) == "ab");
    }

    // ── ToArray ──

    [Fact]
    public void ToArray_Copies_WithoutAdvancingPosition()
    {
        MemoryStream ms = Make("hello");
        ms.Position = 2;
        byte[] arr = ms.ToArray();
        Assert.True(arr.Length == 5);
        Assert.True(ms.Position == 2);
        Assert.True(Encoding.GetString(arr) == "hello");
    }

    // ── 处置后访问 ──

    [Fact]
    public void Dispose_ThenRead_Throws()
    {
        MemoryStream ms = new MemoryStream();
        ms.Dispose();
        bool caughtRead = false;
        byte[] buf = new byte[4];
        try {
            ms.Read(buf, 0, 4);
        } catch (ObjectDisposedException) {
            caughtRead = true;
        }
        Assert.True(caughtRead);
    }

    [Fact]
    public void Dispose_ThenWrite_Throws()
    {
        MemoryStream ms = new MemoryStream();
        ms.Dispose();
        bool caughtWrite = false;
        byte[] data = new byte[4];
        try {
            ms.Write(data, 0, 4);
        } catch (ObjectDisposedException) {
            caughtWrite = true;
        }
        Assert.True(caughtWrite);
    }

    // ── 非法参数前提 ──

    [Fact]
    public void Read_BadOffsetCount_Throws()
    {
        MemoryStream ms = Make("abc");
        byte[] buf = new byte[4];
        bool caught = false;
        try {
            ms.Read(buf, 3, 2); // offset+count=5 > len=4
        } catch (ArgumentOutOfRangeException) {
            caught = true;
        }
        Assert.True(caught);
    }

    [Fact]
    public void Write_NegativeCount_Throws()
    {
        MemoryStream ms = Make("abc");
        byte[] data = new byte[2];
        bool caught = false;
        try {
            ms.Write(data, 0, -1);
        } catch (ArgumentOutOfRangeException) {
            caught = true;
        }
        Assert.True(caught);
    }

    // ── 字节数组构造 ──

    [Fact]
    public void Ctor_WithBuffer_CopiesContent()
    {
        byte[] seed = Encoding.GetBytes("seed");
        MemoryStream ms = new MemoryStream(seed);
        Assert.True(ms.Length == 4);
        Assert.True(ms.CanRead);
        byte[] buf = new byte[4];
        int n = ms.Read(buf, 0, 4);
        Assert.True(n == 4);
        Assert.True(Encoding.GetString(buf) == "seed");
    }
}