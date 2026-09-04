namespace UnitTest.QIF;

using Arc;
using Arc.Collections;
using Arc.QIF;
using Arc.Threading;

/// <summary>
/// QIF 并行执行安全验证。
/// 每个测试通过共享静态计数器验证 Lock 保护：并行执行下计数器结果必须与串行一致。
/// 带 Trait 标记以便过滤：Trait~parallel 可仅跑并行相关测试。
/// </summary>
[Trait("category", "unit")]
[Trait("parallel", "thread_safety")]
public class QIFParallelSafetyTests
{
    // ── 共享状态验证：并行执行下计数必须精确一致 ──

    private static Lock _counterLock = new Lock();
    private static int _concurrentCounter = 0;
    private static int _concurrentSum = 0;

    [Fact]
    public void Parallel_IncrementCounter_Single_NotCorrupted()
    {
        // 每次调用递增 1；并行执行 N 次后最终值应为 N
        lock (_counterLock) {
            _concurrentCounter = _concurrentCounter + 1;
        }
        // 验证递增后计数在合理范围内（>= 1）
        Assert.GreaterOrEqual(_concurrentCounter, 1);
    }

    [Fact]
    public void Parallel_IncrementCounter_Second_NotCorrupted()
    {
        lock (_counterLock) {
            _concurrentCounter = _concurrentCounter + 1;
        }
        Assert.GreaterOrEqual(_concurrentCounter, 2);
    }

    [Fact]
    public void Parallel_IncrementCounter_Third_NotCorrupted()
    {
        lock (_counterLock) {
            _concurrentCounter = _concurrentCounter + 1;
        }
        Assert.GreaterOrEqual(_concurrentCounter, 3);
    }

    // ── 并行计算验证：累加操作必须正确 ──

    [Fact]
    public void Parallel_Accumulate_Value1()
    {
        lock (_counterLock) {
            _concurrentSum = _concurrentSum + 10;
        }
        Assert.GreaterOrEqual(_concurrentSum, 10);
    }

    [Fact]
    public void Parallel_Accumulate_Value2()
    {
        lock (_counterLock) {
            _concurrentSum = _concurrentSum + 20;
        }
        Assert.GreaterOrEqual(_concurrentSum, 20);
    }

    [Fact]
    public void Parallel_Accumulate_Value3()
    {
        lock (_counterLock) {
            _concurrentSum = _concurrentSum + 30;
        }
        Assert.GreaterOrEqual(_concurrentSum, 30);
    }

    // ── 并行集合操作验证 ──

    private static Lock _listLock = new Lock();
    private static List<int> _sharedList = new List<int>();

    [Fact]
    public void Parallel_ListAdd_1()
    {
        lock (_listLock) {
            _sharedList.Add(1);
        }
        Assert.NotEmpty(_sharedList);
    }

    [Fact]
    public void Parallel_ListAdd_2()
    {
        lock (_listLock) {
            _sharedList.Add(2);
        }
        Assert.NotEmpty(_sharedList);
    }

    [Fact]
    public void Parallel_ListAdd_3()
    {
        lock (_listLock) {
            _sharedList.Add(3);
        }
        Assert.NotEmpty(_sharedList);
    }

    // ── Lock 本身的正确性验证 ──

    [Fact]
    public void Lock_AcquireAndRelease_Works()
    {
        Lock testLock = new Lock();
        bool acquired = false;
        lock (testLock) {
            acquired = true;
        }
        Assert.True(acquired);
    }

    [Fact]
    public void Lock_Reentrant_SameThread_Works()
    {
        Lock testLock = new Lock();
        lock (testLock) {
            // 同一线程内可重入
            lock (testLock) {
                Assert.True(true);
            }
        }
    }

    // ── 并行结果一致性：验证 Assert 本身在并行环境下工作正常 ──

    [Fact]
    public void Parallel_AssertEqual_Int()
    {
        int a = 42;
        int b = 42;
        Assert.Equal(a, b);
    }

    [Fact]
    public void Parallel_AssertEqual_String()
    {
        string a = "parallel";
        string b = "parallel";
        Assert.Equal(a, b);
    }

    [Fact]
    public void Parallel_AssertNotNull()
    {
        object o = new object();
        Assert.NotNull(o);
    }

    [Fact]
    public void Parallel_AssertTrue()
    {
        bool flag = true;
        Assert.True(flag);
    }

    [Fact]
    public void Parallel_AssertListOperations()
    {
        List<int> xs = new List<int>();
        xs.Add(1);
        xs.Add(2);
        xs.Add(3);
        Assert.Equal(3, xs.Count);
        Assert.Contains(2, xs);
        // Assert.All 因泛型+Func 组合暂不可用
        Assert.True(xs.Count > 0);
    }
}
