namespace UnitTest.Arc;

using Arc;
using Arc.QIF;
using Arc.Threading;

/// <summary>
/// Lazy&lt;T&gt; / LazyInitializer Stable 最小面（非 Fact-Skip）。
/// 主线程缓存、Lazy&lt;string&gt;、worker 首次求值、并发首次求值（ExecutionAndPublication 精华）。
/// </summary>
public class LazyTests
{
    [Fact]
    public void Lazy_IsValueCreated_FalseUntilAccess()
    {
        LazyCallCounter c = new LazyCallCounter();
        Lazy<int> lazy = new Lazy<int>(() => c.Bump());
        Assert.False(lazy.IsValueCreated);
        Assert.Equal(1, lazy.Value);
        Assert.True(lazy.IsValueCreated);
    }

    [Fact]
    public void Lazy_Value_Cached()
    {
        LazyCallCounter c = new LazyCallCounter();
        Lazy<int> lazy = new Lazy<int>(() => c.Bump());
        Assert.Equal(1, lazy.Value);
        Assert.Equal(1, lazy.Value);
        Assert.Equal(1, c.Count);
    }

    [Fact]
    public void Lazy_Factory_RunsOnce_WithCapture()
    {
        LazyCallCounter c = new LazyCallCounter();
        Lazy<int> lazy = new Lazy<int>(() => c.Bump());
        Assert.Equal(1, lazy.Value);
        Assert.Equal(1, lazy.Value);
        Assert.Equal(1, c.Count);
    }

    [Fact]
    public void Lazy_String_Value()
    {
        Lazy<string> lazy = new Lazy<string>(() => "hello");
        Assert.Equal("hello", lazy.Value);
        Assert.Equal("hello", lazy.Value);
        Assert.True(lazy.IsValueCreated);
    }

    [Fact]
    public void Lazy_Worker_FirstEval()
    {
        LazyCallCounter c = new LazyCallCounter();
        Lazy<int> lazy = new Lazy<int>(() => c.Bump());
        Thread t = new Thread(() => { int v = lazy.Value; });
        t.Start();
        t.Join();
        Assert.Equal(1, lazy.Value);
        Assert.Equal(1, c.Count);
        Assert.True(lazy.IsValueCreated);
    }

    [Fact]
    public void Lazy_Concurrent_FirstEval_Once()
    {
        LazyCallCounter c = new LazyCallCounter();
        Lazy<int> lazy = new Lazy<int>(() => c.BumpSlow());
        Thread t0 = new Thread(() => { int v = lazy.Value; });
        Thread t1 = new Thread(() => { int v = lazy.Value; });
        Thread t2 = new Thread(() => { int v = lazy.Value; });
        Thread t3 = new Thread(() => { int v = lazy.Value; });
        t0.Start();
        t1.Start();
        t2.Start();
        t3.Start();
        t0.Join();
        t1.Join();
        t2.Join();
        t3.Join();
        Assert.Equal(1, c.Count);
        Assert.Equal(1, lazy.Value);
        Assert.True(lazy.IsValueCreated);
    }

    [Fact]
    public void LazyInitializer_EnsureInitialized_Once()
    {
        int target = 0;
        bool initialized = false;
        Lock sync = new Lock();
        LazyCallCounter c = new LazyCallCounter();
        int v1 = LazyInitializer.EnsureInitialized(ref target, ref initialized, sync, () => c.Bump());
        int v2 = LazyInitializer.EnsureInitialized(ref target, ref initialized, sync, () => c.Bump());
        Assert.Equal(1, v1);
        Assert.Equal(1, v2);
        Assert.Equal(1, target);
        Assert.True(initialized);
        Assert.Equal(1, c.Count);
    }
}

/// <summary>
/// Lazy 工厂捕获目标。须 <c>public</c>：lambda 间接调用 mangled
/// <c>LazyCallCounter_Bump</c>，非 public 文件局部类在 UnitTest 过滤/合并宿主下可能不发射方法体（未定义符号）。
/// </summary>
public class LazyCallCounter {
    public int Count;

    public int Bump() {
        this.Count = this.Count + 1;
        return this.Count;
    }

    public int BumpSlow() {
        this.Count = this.Count + 1;
        Thread.Sleep(20);
        return this.Count;
    }
}
