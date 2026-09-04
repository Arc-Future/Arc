namespace UnitTest.Arc;

using Arc;
using Arc.IO;
using Arc.QIF;
using Arc.Text;

/// <summary>
/// Stream 异步虚面（RFC 038 M2 / P0-b）：ReadAsync / WriteAsync / FlushAsync
/// 默认同步完成语义（对齐 C# MemoryStream）。MemoryStream 继承 Stream 默认真体，
/// 验证异步方法可经基类虚面调用且结果正确；CancellationToken 提交前预检（已取消
/// 直接返回已取消 Task，不执行底层 I/O）。
/// </summary>
public class StreamAsyncTests
{
    private static MemoryStream MakeStream(string content)
    {
        MemoryStream ms = new MemoryStream();
        byte[] bytes = Encoding.GetBytes(content);
        ms.Write(bytes, 0, bytes.Length);
        ms.Position = 0;
        return ms;
    }

    [Fact]
    public async Task Stream_ReadAsync_ReadsAllBytes()
    {
        MemoryStream ms = MakeStream("hello-async");
        byte[] buffer = new byte[11];
        int n = await ms.ReadAsync(buffer, 0, 11);
        Assert.True(n == 11);
        Assert.Equal("hello-async", Encoding.GetString(buffer));
    }

    [Fact]
    public async Task Stream_WriteAsync_WritesBytes()
    {
        MemoryStream ms = new MemoryStream();
        byte[] data = Encoding.GetBytes("write-async");
        await ms.WriteAsync(data, 0, data.Length);
        Assert.True(ms.Length == 11);
        ms.Position = 0;
        byte[] buf = new byte[11];
        int n = ms.Read(buf, 0, 11);
        Assert.True(n == 11);
        Assert.Equal("write-async", Encoding.GetString(buf));
    }

    [Fact]
    public async Task Stream_FlushAsync_Completes()
    {
        MemoryStream ms = new MemoryStream();
        await ms.FlushAsync();
        Assert.True(ms.CanRead);
    }

    [Fact]
    public async Task Stream_ReadAsync_PreCanceled_ReturnsCanceled()
    {
        MemoryStream ms = MakeStream("cancel");
        CancellationTokenSource cts = new CancellationTokenSource();
        cts.Cancel();
        Task<int> t = ms.ReadAsync(new byte[16], 0, 16, cts.Token);
        Assert.True(t.IsCanceled);
        Assert.True(!t.IsCompleted);
    }

    [Fact]
    public async Task Stream_WriteAsync_PreCanceled_ReturnsCanceled()
    {
        MemoryStream ms = new MemoryStream();
        CancellationTokenSource cts = new CancellationTokenSource();
        cts.Cancel();
        Task t = ms.WriteAsync(new byte[4], 0, 4, cts.Token);
        Assert.True(t.IsCanceled);
    }
}
