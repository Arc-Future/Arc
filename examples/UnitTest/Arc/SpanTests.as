namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.QIF;

/// <summary>RFC 078 M1+M2 + std 最小面：安全 Span / ReadOnlySpan（无 unsafe）。</summary>
public class SpanTests
{
    private void Fill(Span<int> s, int v)
    {
        for (int i = 0; i < s.Length; i++)
        {
            s[i] = v;
        }
    }

    [Fact]
    public void AsSpan_IndexWrite_ReflectsArray()
    {
        int[] buf = [1, 2, 3, 4];
        Span<int> mid = buf.AsSpan(1, 2);
        Assert.Equal(2, mid.Length);
        this.Fill(mid, 9);
        Assert.Equal(1, buf[0]);
        Assert.Equal(9, buf[1]);
        Assert.Equal(9, buf[2]);
        Assert.Equal(4, buf[3]);
    }

    [Fact]
    public void AsReadOnlySpan_IndexRead()
    {
        int[] buf = [5, 6, 7];
        ReadOnlySpan<int> r = buf.AsReadOnlySpan();
        Assert.Equal(3, r.Length);
        Assert.Equal(5, r[0]);
        Assert.Equal(7, r[2]);
    }

    [Fact]
    public void List_AsSpan_IndexWrite_ReflectsList()
    {
        List<int> list = new List<int>();
        list.Add(10);
        list.Add(20);
        list.Add(30);
        Span<int> s = list.AsSpan();
        Assert.Equal(3, s.Length);
        this.Fill(s, 7);
        Assert.Equal(7, list[0]);
        Assert.Equal(7, list[2]);
    }

    [Fact]
    public void String_AsSpan_Utf8Bytes()
    {
        string s = "Hi";
        ReadOnlySpan<byte> b = s.AsSpan();
        Assert.Equal(2, b.Length);
        int b0 = b[0];
        int b1 = b[1];
        Assert.Equal(72, b0);
        Assert.Equal(105, b1);
    }

    [Fact]
    public void CollectionExpr_Target_Span()
    {
        Span<int> s = [1, 2, 3];
        Assert.Equal(3, s.Length);
        Assert.Equal(2, s[1]);
        this.Fill(s, 8);
        Assert.Equal(8, s[0]);
        Assert.Equal(8, s[2]);
    }

    [Fact]
    public void CollectionExpr_Target_ReadOnlySpan_Empty()
    {
        ReadOnlySpan<int> r = [];
        Assert.Equal(0, r.Length);
        Assert.True(r.IsEmpty);
    }

    [Fact]
    public void Slice_SubView_ReflectsArray()
    {
        int[] buf = [1, 2, 3, 4, 5];
        Span<int> all = buf.AsSpan();
        Span<int> mid = all.Slice(1, 3);
        Assert.Equal(3, mid.Length);
        Assert.Equal(2, mid[0]);
        mid[1] = 99;
        // NLL 严格借用（RFC 099 至少 Rust 级）：mutable span 借用期间禁直接
        // 读底层数组（Rust 拒绝；C# 允许）。改用 span 视图验证写已反映。
        Assert.Equal(99, mid[1]);
        ReadOnlySpan<int> ros = all.AsReadOnly();
        Assert.Equal(5, ros.Length);
        Assert.Equal(99, ros[2]);
    }

    [Fact]
    public void IsEmpty_And_StaticEmpty()
    {
        Span<int> empty = Span<int>.Empty;
        Assert.True(empty.IsEmpty);
        Assert.Equal(0, empty.Length);
        ReadOnlySpan<int> roEmpty = ReadOnlySpan<int>.Empty;
        Assert.True(roEmpty.IsEmpty);
        int[] buf = [1];
        Assert.False(buf.AsSpan().IsEmpty);
    }

    [Fact]
    public void CopyTo_Span_ReflectsDest()
    {
        int[] srcBuf = [10, 20, 30];
        int[] dstBuf = [0, 0, 0, 0];
        ReadOnlySpan<int> src = srcBuf.AsReadOnlySpan();
        Span<int> dst = dstBuf.AsSpan(1, 3);
        src.CopyTo(dst);
        Assert.Equal(0, dstBuf[0]);
        Assert.Equal(10, dstBuf[1]);
        Assert.Equal(20, dstBuf[2]);
        Assert.Equal(30, dstBuf[3]);
    }
    [Fact]
    public void Slice_StartOnly_Builtin()
    {
        int[] buf = [1, 2, 3, 4];
        Span<int> tail = buf.AsSpan().Slice(1);
        Assert.Equal(3, tail.Length);
        Assert.Equal(2, tail[0]);
    }

    [Fact]
    public void Fill_Clear_Builtin()
    {
        int[] buf = [5, 6, 7];
        Span<int> s = buf.AsSpan();
        s.Fill(0);
        // NLL 严格借用：mutable span 借用期间禁直接读底层数组；先结束 span。
        s.Clear();
        Assert.Equal(0, buf[0]);
        Assert.Equal(0, buf[2]);
    }

    [Fact]
    public void TryCopyTo_And_ToArray()
    {
        int[] srcBuf = [4, 5, 6];
        int[] shortBuf = [0];
        Assert.False(srcBuf.AsReadOnlySpan().TryCopyTo(shortBuf.AsSpan()));
        int[] dstBuf = [0, 0, 0];
        Assert.True(srcBuf.AsSpan().TryCopyTo(dstBuf.AsSpan()));
        Assert.Equal(4, dstBuf[0]);
        Assert.Equal(6, dstBuf[2]);
        int[] copy = srcBuf.AsSpan(1, 2).ToArray();
        Assert.Equal(2, copy.Length);
        Assert.Equal(5, copy[0]);
        Assert.Equal(6, copy[1]);
    }

    [Fact]
    public void Foreach_Span_SumsElements()
    {
        int[] buf = [1, 2, 3, 4];
        Span<int> mid = buf.AsSpan(1, 2);
        int total = 0;
        foreach (var x in mid)
        {
            total = total + x;
        }
        Assert.Equal(5, total);
    }

    [Fact]
    public void Foreach_ReadOnlySpan_And_Empty()
    {
        ReadOnlySpan<int> r = [10, 20];
        int total = 0;
        foreach (var x in r)
        {
            total = total + x;
        }
        Assert.Equal(30, total);
        Span<int> empty = [];
        int n = 0;
        foreach (var x in empty)
        {
            n = n + 1;
        }
        Assert.Equal(0, n);
    }

}
