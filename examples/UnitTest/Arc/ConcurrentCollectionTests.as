namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.Collections.Concurrent;
using Arc.QIF;

/// <summary>
/// 并发集合单元测试：覆盖 ConcurrentDictionary / ConcurrentQueue /
/// ConcurrentStack / ConcurrentBag / BlockingCollection 基本 API（非 Skip）。
/// M6 压力面由 `concurrent_bench_e2e` 承担（非本文件）。
/// </summary>
public class ConcurrentCollectionTests
{
    // ── ConcurrentDictionary ──

    [Fact]
    public void ConcurrentDictionary_TryAdd()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        Assert.True(dict.TryAdd("alice", 42));
        Assert.False(dict.TryAdd("alice", 7));
        Assert.Equal(1, dict.Count);
        Assert.True(dict.ContainsKey("alice"));
        Assert.False(dict.IsEmpty);
    }

    [Fact]
    public void ConcurrentDictionary_TryGetValue()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        dict.TryAdd("alice", 42);
        int v;
        Assert.True(dict.TryGetValue("alice", out v));
        Assert.Equal(42, v);
        Assert.False(dict.TryGetValue("bob", out v));
        Assert.Equal(0, v);
    }

    [Fact]
    public void ConcurrentDictionary_GetValueOrDefault()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        dict.TryAdd("alice", 42);
        Assert.Equal(42, dict.GetValueOrDefault("alice"));
        Assert.Equal(0, dict.GetValueOrDefault("bob"));
    }

    [Fact]
    public void ConcurrentDictionary_Indexer()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        dict.TryAdd("alice", 42);
        dict["alice"] = 100;
        Assert.Equal(100, dict.GetValueOrDefault("alice"));
    }

    [Fact]
    public void ConcurrentDictionary_Clear()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        dict.TryAdd("alice", 42);
        dict.Clear();
        Assert.Equal(0, dict.Count);
    }

    [Fact]
    public void ConcurrentDictionary_TryUpdate()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        dict.TryAdd("alice", 42);
        Assert.True(dict.TryUpdate("alice", 100, 42));
        Assert.Equal(100, dict.GetValueOrDefault("alice"));
        Assert.False(dict.TryUpdate("alice", 7, 42));
        Assert.Equal(100, dict.GetValueOrDefault("alice"));
    }

    [Fact]
    public void ConcurrentDictionary_GetOrAdd_Value()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        Assert.Equal(10, dict.GetOrAdd("x", 10));
        Assert.Equal(10, dict.GetOrAdd("x", 99));
        Assert.Equal(1, dict.Count);
    }

    [Fact]
    public void ConcurrentDictionary_TryRemove()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        dict.TryAdd("alice", 42);
        int v;
        Assert.True(dict.TryRemove("alice", out v));
        Assert.Equal(42, v);
        Assert.False(dict.ContainsKey("alice"));
        Assert.True(dict.IsEmpty);
    }

    [Fact]
    public void ConcurrentDictionary_Keys_Length()
    {
        ConcurrentDictionary<string, int> dict = new ConcurrentDictionary<string, int>();
        dict.TryAdd("a", 1);
        dict.TryAdd("b", 2);
        string[] keys = dict.Keys;
        Assert.Equal(2, keys.Length);
    }

    // ── ConcurrentQueue ──

    [Fact]
    public void ConcurrentQueue_Enqueue()
    {
        ConcurrentQueue<int> q = new ConcurrentQueue<int>();
        q.Enqueue(10);
        q.Enqueue(20);
        q.Enqueue(30);

        Assert.Equal(3, q.Count);
        Assert.False(q.IsEmpty);
    }

    [Fact]
    public void ConcurrentQueue_IsEmpty()
    {
        ConcurrentQueue<int> q = new ConcurrentQueue<int>();
        Assert.Equal(0, q.Count);
        Assert.True(q.IsEmpty);
    }

    [Fact]
    public void ConcurrentQueue_TryDequeue_Fifo()
    {
        ConcurrentQueue<int> q = new ConcurrentQueue<int>();
        q.Enqueue(10);
        q.Enqueue(20);
        int a;
        int b;
        Assert.True(q.TryDequeue(out a));
        Assert.Equal(10, a);
        Assert.True(q.TryDequeue(out b));
        Assert.Equal(20, b);
        Assert.True(q.IsEmpty);
    }

    [Fact]
    public void ConcurrentQueue_TryPeek_ToArray()
    {
        ConcurrentQueue<int> q = new ConcurrentQueue<int>();
        q.Enqueue(10);
        q.Enqueue(20);
        int peek;
        Assert.True(q.TryPeek(out peek));
        Assert.Equal(10, peek);
        Assert.Equal(2, q.Count);
        // ToArray 返回 void* 槽快照；Length 可信，标量元素索引布局后置。
        int[] arr = q.ToArray();
        Assert.Equal(2, arr.Length);
    }

    [Fact]
    public void ConcurrentQueue_Pcc_TryAdd_TryTake()
    {
        ConcurrentQueue<int> q = new ConcurrentQueue<int>();
        Assert.True(q.TryAdd(1));
        Assert.True(q.TryAdd(2));
        Assert.Equal(2, q.Count);
        int a;
        int b;
        Assert.True(q.TryTake(out a));
        Assert.Equal(1, a);
        Assert.True(q.TryTake(out b));
        Assert.Equal(2, b);
        Assert.True(q.IsEmpty);
    }

    // ── ConcurrentStack ──

    [Fact]
    public void ConcurrentStack_Push()
    {
        ConcurrentStack<int> s = new ConcurrentStack<int>();
        s.Push(1);
        s.Push(2);
        s.Push(3);

        Assert.Equal(3, s.Count);
        Assert.False(s.IsEmpty);
    }

    [Fact]
    public void ConcurrentStack_IsEmpty()
    {
        ConcurrentStack<int> s = new ConcurrentStack<int>();
        Assert.Equal(0, s.Count);
        Assert.True(s.IsEmpty);
    }

    [Fact]
    public void ConcurrentStack_TryPop_Lifo()
    {
        ConcurrentStack<int> s = new ConcurrentStack<int>();
        s.Push(1);
        s.Push(2);
        s.Push(3);
        int a;
        int b;
        Assert.True(s.TryPop(out a));
        Assert.Equal(3, a);
        Assert.True(s.TryPop(out b));
        Assert.Equal(2, b);
        Assert.Equal(1, s.Count);
    }

    [Fact]
    public void ConcurrentStack_TryPeek_Lifo()
    {
        ConcurrentStack<int> s = new ConcurrentStack<int>();
        s.Push(10);
        s.Push(20);
        s.Push(30);
        int peek;
        Assert.True(s.TryPeek(out peek));
        Assert.Equal(30, peek);
        Assert.Equal(3, s.Count);
        int a;
        Assert.True(s.TryPop(out a));
        Assert.Equal(30, a);
        Assert.Equal(2, s.Count);
    }

    [Fact]
    public void ConcurrentStack_Pcc_TryAdd_TryTake()
    {
        ConcurrentStack<int> s = new ConcurrentStack<int>();
        Assert.True(s.TryAdd(1));
        Assert.True(s.TryAdd(2));
        int v;
        Assert.True(s.TryTake(out v));
        Assert.Equal(2, v);
    }

    // ── ConcurrentBag ──

    [Fact]
    public void ConcurrentBag_Add()
    {
        ConcurrentBag<int> b = new ConcurrentBag<int>();
        b.Add(100);
        b.Add(200);
        b.Add(300);

        Assert.Equal(3, b.Count);
        Assert.False(b.IsEmpty);
    }

    [Fact]
    public void ConcurrentBag_IsEmpty()
    {
        ConcurrentBag<int> b = new ConcurrentBag<int>();
        Assert.Equal(0, b.Count);
        Assert.True(b.IsEmpty);
    }

    [Fact]
    public void ConcurrentBag_TryTake()
    {
        ConcurrentBag<int> b = new ConcurrentBag<int>();
        b.Add(42);
        int v;
        Assert.True(b.TryTake(out v));
        Assert.Equal(42, v);
        Assert.True(b.IsEmpty);
    }

    [Fact]
    public void ConcurrentBag_TryPeek()
    {
        ConcurrentBag<int> b = new ConcurrentBag<int>();
        b.Add(7);
        int v;
        Assert.True(b.TryPeek(out v));
        Assert.Equal(7, v);
        Assert.Equal(1, b.Count);
    }

    // ── BlockingCollection ──

    [Fact]
    public void BlockingCollection_TryAdd_TryTake()
    {
        BlockingCollection<int> bc = new BlockingCollection<int>(0);
        Assert.True(bc.TryAdd(7));
        Assert.True(bc.TryAdd(8));
        Assert.Equal(2, bc.Count);
        int a;
        int b;
        Assert.True(bc.TryTake(out a));
        Assert.Equal(7, a);
        Assert.True(bc.TryTake(out b));
        Assert.Equal(8, b);
        Assert.Equal(0, bc.Count);
    }

    [Fact]
    public void BlockingCollection_CompleteAdding_RejectsTryAdd()
    {
        BlockingCollection<int> bc = new BlockingCollection<int>(0);
        Assert.True(bc.TryAdd(1));
        bc.CompleteAdding();
        Assert.True(bc.IsAddingCompleted);
        Assert.False(bc.TryAdd(2));
        int v;
        Assert.True(bc.TryTake(out v));
        Assert.Equal(1, v);
    }

    [Fact]
    public void BlockingCollection_IsCompleted_AfterDrain()
    {
        BlockingCollection<int> bc = new BlockingCollection<int>(0);
        Assert.True(bc.TryAdd(1));
        bc.CompleteAdding();
        Assert.False(bc.IsCompleted);
        int v;
        Assert.True(bc.TryTake(out v));
        Assert.Equal(1, v);
        Assert.True(bc.IsCompleted);
    }

    [Fact]
    public void BlockingCollection_Bounded_TryAddFailsWhenFull()
    {
        BlockingCollection<int> bc = new BlockingCollection<int>(1);
        Assert.True(bc.TryAdd(1));
        Assert.False(bc.TryAdd(2));
        int v;
        Assert.True(bc.TryTake(out v));
        Assert.Equal(1, v);
        Assert.True(bc.TryAdd(2));
    }

    [Fact]
    public void BlockingCollection_PccCtor_Queue_Fifo()
    {
        ConcurrentQueue<int> q = new ConcurrentQueue<int>();
        q.Enqueue(10);
        q.Enqueue(20);
        BlockingCollection<int> bc = new BlockingCollection<int>(q, 0);
        int a;
        int b;
        Assert.True(bc.TryTake(out a));
        Assert.Equal(10, a);
        Assert.True(bc.TryTake(out b));
        Assert.Equal(20, b);
    }

    [Fact]
    public void BlockingCollection_PccCtor_Stack_Lifo()
    {
        ConcurrentStack<int> st = new ConcurrentStack<int>();
        st.Push(1);
        st.Push(2);
        BlockingCollection<int> bc = new BlockingCollection<int>(st, 4);
        int a;
        int b;
        Assert.True(bc.TryTake(out a));
        Assert.Equal(2, a);
        Assert.True(bc.TryTake(out b));
        Assert.Equal(1, b);
    }
}
