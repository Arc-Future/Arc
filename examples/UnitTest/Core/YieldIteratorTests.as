namespace UnitTest.Core;

using Arc;
using Arc.Collections;
using Arc.Linq;
using Arc.QIF;

class YieldProbe {
    public int Ticks;

    public YieldProbe() {
        Ticks = 0;
    }

    public IEnumerable<int> Nums() {
        yield return 1;
        yield return 2;
        yield return 3;
    }

    /// <summary>纯 yield break 的空迭代器。</summary>
    public IEnumerable<int> Empty() {
        yield break;
    }

    /// <summary>this 捕获（__host 字段）：每个挂起段前递增宿主 Ticks。</summary>
    public IEnumerable<int> Ticked() {
        this.Ticks = this.Ticks + 1;
        yield return 1;
        this.Ticks = this.Ticks + 1;
        yield return 2;
    }

    /// <summary>参数跨挂起点捕获（__prm_limit）+ while/if + yield break。</summary>
    public IEnumerable<int> EarlyBreak(int limit) {
        int n = 0;
        while (true) {
            if (n >= limit) {
                yield break;
            }
            yield return n;
            n = n + 1;
        }
    }

    /// <summary>局部变量状态跨挂起点存活（__loc_acc/__loc_i）+ for 循环。</summary>
    public IEnumerable<int> RunningSum() {
        int acc = 0;
        for (int i = 1; i <= 4; i = i + 1) {
            acc = acc + i;
            yield return acc;
        }
    }

    /// <summary>迭代器方法体内 foreach（List 源，枚举器协议展开 __enum_*）。</summary>
    public IEnumerable<int> DoubleListBody() {
        List<int> src = new List<int>();
        src.Add(10);
        src.Add(20);
        foreach (var v in src) {
            yield return v * 2;
        }
    }

    /// <summary>返回 IEnumerator&lt;T&gt; 的迭代器方法（无 GetEnumerator 合成）。</summary>
    public IEnumerator<int> DirectEnumerator() {
        yield return 7;
        yield return 8;
    }

    public IEnumerable<string> Words() {
        yield return "a";
        yield return "b";
    }

    /// <summary>异步迭代器方法（async IAsyncEnumerable&lt;T&gt;）。</summary>
    public async IAsyncEnumerable<int> AsyncNums() {
        yield return 5;
        yield return 6;
    }
}

/// <summary>
/// yield 迭代器语义单元测试：覆盖 RFC 044 全部同步惯用法与异步消费协议。
/// - 多值 yield return + foreach 消费 / 空迭代器 / yield break 提前退出
/// - 延迟求值（消费前生成端零执行：宿主 Ticks 计数器断言）
/// - 局部变量 / 方法参数状态跨挂起点存活；this 捕获（__host）
/// - IEnumerator&lt;T&gt; 手写 MoveNext 协议；string 序列
/// - 迭代器方法体内 foreach（List 源）；再枚举安全（新鲜状态机实例）
/// - 嵌套 foreach；yield 序列物化为 List 后接入 LINQ（Where/Select）
/// - 异步迭代器：GetAsyncEnumerator + await MoveNextAsync 手写循环
/// （await foreach 语句编译器不支持，消费协议对齐 AISessionStreamCollector）
/// </summary>
public class YieldIteratorTests
{
    // ── 基础序列 ──

    [Fact]
    public void Yield_Foreach_ConsumesMultiValues()
    {
        YieldProbe p = new YieldProbe();
        int sum = 0;
        int count = 0;
        foreach (var x in p.Nums()) {
            sum = sum + x;
            count = count + 1;
        }
        Assert.Equal(6, sum);
        Assert.Equal(3, count);
    }

    [Fact]
    public void Yield_Empty_YieldsNothing()
    {
        YieldProbe p = new YieldProbe();
        int count = 0;
        foreach (var x in p.Empty()) {
            count = count + 1;
        }
        Assert.Equal(0, count);
    }

    [Fact]
    public void Yield_EarlyBreak_StopsBeforeLimit()
    {
        YieldProbe p = new YieldProbe();
        int seen = 0;
        foreach (var x in p.EarlyBreak(3)) {
            seen = seen * 10 + x;
        }
        Assert.Equal(12, seen);

        int lateCount = 0;
        foreach (var y in p.EarlyBreak(0)) {
            lateCount = lateCount + 1;
        }
        Assert.Equal(0, lateCount);
    }

    // ── 延迟求值 ──

    [Fact]
    public void Yield_LazyEvaluation_GeneratorIdleUntilPulled()
    {
        YieldProbe p = new YieldProbe();
        IEnumerable<int> seq = p.Ticked();
        Assert.Equal(0, p.Ticks);

        IEnumerator<int> e = seq.GetEnumerator();
        Assert.Equal(0, p.Ticks);

        Assert.True(e.MoveNext());
        Assert.Equal(1, e.Current);
        Assert.Equal(1, p.Ticks);

        Assert.True(e.MoveNext());
        Assert.Equal(2, e.Current);
        Assert.Equal(2, p.Ticks);

        Assert.False(e.MoveNext());
        Assert.Equal(2, p.Ticks);
    }

    // ── 状态机状态存活 ──

    [Fact]
    public void Yield_LocalVariableState_AcrossSuspensions()
    {
        YieldProbe p = new YieldProbe();
        int seen = 0;
        foreach (var x in p.RunningSum()) {
            seen = seen * 100 + x;
        }
        Assert.Equal(1030610, seen);
    }

    [Fact]
    public void Yield_ParamCaptured_AcrossSuspensions()
    {
        YieldProbe p = new YieldProbe();
        int a = 0;
        int b = 0;
        foreach (var x in p.EarlyBreak(5)) {
            if (x >= 3) {
                b = b + 1;
            } else {
                a = a + 1;
            }
        }
        Assert.Equal(3, a);
        Assert.Equal(2, b);
    }

    // ── 手写枚举协议 / 类型形态 ──

    [Fact]
    public void Yield_IEnumerator_MoveNextProtocol()
    {
        YieldProbe p = new YieldProbe();
        IEnumerator<int> e = p.DirectEnumerator();
        int seen = 0;
        while (e.MoveNext()) {
            seen = seen * 100 + e.Current;
        }
        Assert.Equal(708, seen);
    }

    [Fact]
    public void Yield_StringSequence_Foreach()
    {
        YieldProbe p = new YieldProbe();
        string cat = "";
        foreach (var w in p.Words()) {
            cat = cat + w;
        }
        Assert.True(cat == "ab");
    }

    // ── 迭代器方法体内的控制流 ──

    [Fact]
    public void Yield_IteratorBody_ForeachOverList()
    {
        YieldProbe p = new YieldProbe();
        int sum = 0;
        foreach (var x in p.DoubleListBody()) {
            sum = sum + x;
        }
        Assert.Equal(60, sum);
    }

    // ── 再枚举安全 ──

    [Fact]
    public void Yield_Reenumeration_FreshSequenceEachCall()
    {
        YieldProbe p = new YieldProbe();
        int first = 0;
        foreach (var x in p.Nums()) {
            first = first + 1;
        }
        int second = 0;
        foreach (var y in p.Nums()) {
            second = second + 1;
        }
        Assert.Equal(3, first);
        Assert.Equal(3, second);
    }

    [Fact]
    public void Yield_HeldEnumerable_Reenumerable()
    {
        YieldProbe p = new YieldProbe();
        IEnumerable<int> seq = p.Nums();
        int first = 0;
        foreach (var x in seq) {
            first = first + 1;
        }
        int second = 0;
        foreach (var y in seq) {
            second = second + 1;
        }
        Assert.Equal(3, first);
        Assert.Equal(3, second);
    }

    // ── 嵌套消费 ──

    [Fact]
    public void Yield_NestedForeach_ConsumesTwoSequences()
    {
        YieldProbe p = new YieldProbe();
        int pairs = 0;
        int diag = 0;
        int i = 0;
        foreach (var a in p.Nums()) {
            int j = 0;
            foreach (var b in p.Nums()) {
                if (i == j) {
                    diag = diag + a * b;
                }
                pairs = pairs + 1;
                j = j + 1;
            }
            i = i + 1;
        }
        Assert.Equal(9, pairs);
        Assert.Equal(14, diag);
    }

    // ── LINQ 衔接（物化源）──

    /// <summary>
    /// yield 序列物化为 List 后接入 LINQ Where/Select 方法链。
    /// 注：Where/Select 为 MIR 编译期展开，仅支持数组/List 源——
    /// 直接挂在 yield 迭代器返回的 IEnumerable&lt;T&gt; 上不被支持
    /// （错误指纹：`OOP: unknown method `Where` on type `IEnumerable_int``）。
    /// </summary>
    [Fact]
    public void Yield_Linq_OnMaterializedSource()
    {
        YieldProbe p = new YieldProbe();
        List<int> buffer = new List<int>();
        foreach (var n in p.Nums()) {
            buffer.Add(n);
        }
        Assert.Equal(3, buffer.Count);

        int sum = 0;
        int count = 0;
        foreach (var m in buffer.Where(n => n > 1).Select(n => n * 2)) {
            sum = sum + m;
            count = count + 1;
        }
        Assert.Equal(2, count);
        Assert.Equal(10, sum);
    }

    // ── 异步迭代器 ──

    /// <summary>
    /// 异步迭代器消费协议：GetAsyncEnumerator + await MoveNextAsync 手写循环
    /// （对齐 RFC 044 消费协议与 AISessionStreamCollector 用法；
    /// await foreach 语句暂无编译器支持）。
    /// </summary>
    [Fact]
    public async Task Yield_Async_ConsumeViaMoveNextAsync()
    {
        YieldProbe p = new YieldProbe();
        IAsyncEnumerator<int> e = p.AsyncNums().GetAsyncEnumerator(CancellationToken.None);
        int sum = 0;
        while (true) {
            bool moved = await e.MoveNextAsync();
            if (!moved) {
                break;
            }
            sum = sum + e.Current;
        }
        Assert.Equal(11, sum);
    }
}

// ── 已知边界（缺陷指纹，探针实测；待编译器修复后启用）──
// 1. LINQ 方法链直接接在 yield 迭代器返回的 IEnumerable<T> 上：
//    `foreach (var x in p.Nums().Where(n => n > 1)) { ... }`
//    → error: OOP: unknown method `Where` on type `IEnumerable_int`
//    （Where/Select 为 MIR 编译期展开，仅接受数组/List 源；见 Yield_Linq_OnMaterializedSource 物化路径）
// 2. 迭代器方法体内 foreach 数组（元素类型为值类型的 T[]）：
//    `int[] src = [1, 2]; foreach (var v in src) { yield return v; }`
//    → error: OOP: undefined type `int_arr`
//    （数组声明与索引访问正常，仅数组枚举展开在状态机 OOP 检查阶段失败）
