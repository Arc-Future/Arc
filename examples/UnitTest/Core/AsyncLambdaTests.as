namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// Async lambda 捕获语义单元测试（RFC 029 M6）。
/// 
/// 验证编译器对 async lambda 的闭包捕获在不同模式下的正确性：
///   - 无捕获：async lambda 不使用外层变量
///   - 值类型捕获：捕获 int 并在 await 后使用（验证 env save/restore）
///   - 引用类型捕获：捕获 class 实例并在 await 后修改其状态
///   - 混合捕获：多种类型同时捕获
/// 
/// 这是 async/await 基础设施的核心验证点——捕获语义错误
/// 会导致难以调试的运行时数据损坏。
/// </summary>

class AsyncCounter {
    public int Value;
    public void Inc() { Value = Value + 1; }
}

public class AsyncLambdaTests
{
    public async Task<int> FetchValue() { return 42; }
    public async Task<int> Multiply(int b, int m) { return b * m; }

    /// <summary>async 测试方法的基础验证。</summary>
    [Fact]
    public async Task NoCapture()
    {
        int r = await this.FetchValue();
        Assert.Equal(42, r);
    }

    /// <summary>捕获值类型 (int) 并在 await 后使用。</summary>
    [Fact]
    public async Task ValueCapture()
    {
        int multiplier = 10;
        Func<Task<int>> f = async () => {
            int v = await this.FetchValue();
            return v * multiplier;
        };
        int r = await f();
        Assert.Equal(420, r);
    }

    /// <summary>捕获引用类型 (class) 并在 await 后修改其状态。</summary>
    [Fact]
    public async Task ClassCapture()
    {
        var counter = new AsyncCounter();
        counter.Value = 0;
        Func<Task<int>> f = async () => {
            int v = await this.FetchValue();
            counter.Inc();
            return v + counter.Value;
        };
        int r = await f();
        Assert.Equal(43, r);
        Assert.Equal(1, counter.Value);
    }

    /// <summary>混合捕获：值类型 + 引用类型 + 跨 await 计算。</summary>
    [Fact]
    public async Task MultiCapture()
    {
        int multiplier = 10;
        int baseVal = 5;
        var ctr = new AsyncCounter();
        ctr.Value = 100;
        Func<Task<int>> f = async () => {
            int v = await this.Multiply(baseVal, multiplier);
            ctr.Inc();
            return v + ctr.Value;
        };
        int r = await f();
        // 5 * 10 + 101 = 151
        Assert.Equal(151, r);
        Assert.Equal(101, ctr.Value);
    }
}
