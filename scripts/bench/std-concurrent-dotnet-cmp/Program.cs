// RFC 015 并发性能协议对照（.NET TPL / System.Collections.Concurrent）。
//
// 与 Arc 侧同构场景：
//   Arc 侧 concurrent_bench_e2e bench（已随 arc-integration 退场，a2627a0f）
//     - bench_dict_try_add_32_threads  → concurrent_dict_add
//     - bench_queue_mpmc_32            → concurrent_queue_mpmc
//   Arc 侧 roofline_bench bench（已随 arc-integration 退场，a2627a0f）
//     - parallel_for_amdahl_bench      → parallel_for_scale（串行基线 + 1/2/4/8/16）
//     - slab_alloc_free_bench          → task_create_1m（10^6 slab alloc/free，RFC 013 M5 预算）
//
// 输出 OK: 行供协议对照脚本解析。正确性断言硬门禁，性能仅观测（同 RFC 015 验收口径）。

using System.Collections.Concurrent;
using System.Diagnostics;

static void Report(string name, double ops, double ms)
{
    Console.WriteLine($"OK: {name} ops={ops:0} ms={ms:0.00} ops_per_s={ops * 1000.0 / (ms > 0.001 ? ms : 0.001):0}");
}

static void ReportMs(string name, double ms)
{
    Console.WriteLine($"OK: {name} ms={ms:0.00}");
}

// 32 线程 × 10k 不同键 TryAdd（与 Arc bench_dict_try_add_32_threads 同构）。
static void BenchDictTryAdd()
{
    const int NThreads = 32;
    const int NPer = 10_000;
    int total = NThreads * NPer;
    var d = new ConcurrentDictionary<int, int>(Environment.ProcessorCount, 127);
    var sw = Stopwatch.StartNew();
    var tasks = new Task[NThreads];
    for (int t = 0; t < NThreads; t++)
    {
        int id = t;
        tasks[t] = Task.Run(() =>
        {
            int baseV = id * NPer;
            for (int i = 1; i <= NPer; i++)
            {
                int k = baseV + i;
                if (!d.TryAdd(k, k * 3)) throw new Exception($"cd add failed at {k}");
            }
        });
    }
    Task.WaitAll(tasks);
    sw.Stop();
    if (d.Count != total) throw new Exception($"cd count {d.Count} != {total}");
    Report("concurrent_dict_add", total, sw.Elapsed.TotalMilliseconds);
}

// 16 生产 + 16 消费 MPMC（与 Arc bench_queue_mpmc_32 同构：10k/生产者）。
static void BenchQueueMpmc()
{
    const int NProd = 16;
    const int NCons = 16;
    const int NPer = 10_000;
    int total = NProd * NPer;
    var q = new ConcurrentQueue<int>();
    var seen = new int[total + 1];
    var taken = 0;

    var sw = Stopwatch.StartNew();
    var producers = new Task[NProd];
    for (int p = 0; p < NProd; p++)
    {
        int id = p;
        producers[p] = Task.Run(() =>
        {
            int baseV = id * NPer;
            for (int i = 1; i <= NPer; i++) q.Enqueue(baseV + i);
        });
    }
    var consumers = new Task[NCons];
    for (int c = 0; c < NCons; c++)
    {
        consumers[c] = Task.Run(() =>
        {
            while (true)
            {
                if (Volatile.Read(ref taken) >= total) return;
                if (q.TryDequeue(out int v))
                {
                    if (v < 1 || v > total) throw new Exception("queue value out of range");
                    int prev = Interlocked.Increment(ref seen[v]) - 1;
                    if (prev != 0) throw new Exception("duplicate");
                    Interlocked.Increment(ref taken);
                }
            }
        });
    }
    Task.WaitAll(producers);
    Task.WaitAll(consumers);
    sw.Stop();

    for (int i = 1; i <= total; i++)
        if (seen[i] != 1) throw new Exception($"seen[{i}] = {seen[i]}");
    if (!q.IsEmpty) throw new Exception("queue not drained");
    Report("concurrent_queue_mpmc", total * 2.0, sw.Elapsed.TotalMilliseconds);
}

// Parallel.For 扩展性（串行基线 + MaxDegreeOfParallelism 1/2/4/8/16）。
// 与 Arc parallel_for_amdahl_bench（roofline_bench）同构：10^7 迭代原子自增。
static void BenchParallelForScale()
{
    const long Total = 10_000_000;
    long SerialSum()
    {
        long sum = 0;
        for (long i = 0; i < Total; i++) sum += i;
        return sum;
    }

    long serial = 0;
    var sw = Stopwatch.StartNew();
    serial = SerialSum();
    sw.Stop();
    double serialMs = sw.Elapsed.TotalMilliseconds;
    Console.WriteLine($"OK: parallel_for_scale serial_ms={serialMs:0.00} sum={serial}");

    foreach (int w in new[] { 1, 2, 4, 8, 16 })
    {
        long sum = 0;
        var opts = new ParallelOptions { MaxDegreeOfParallelism = w };
        sw.Restart();
        Parallel.For(0, Total, opts, i => Interlocked.Add(ref sum, i));
        sw.Stop();
        double ms = sw.Elapsed.TotalMilliseconds;
        Console.WriteLine($"OK: parallel_for_scale w={w} ms={ms:0.00} speedup={serialMs / ms:0.00}");
    }
}

// 10^6 Task 创建（RFC 013 M5 预算对照：Arc 目标 <3ms；.NET 实测供 ≥17× 宣称基线）。
static void BenchTaskCreate1M()
{
    const int N = 1_000_000;
    var sw = Stopwatch.StartNew();
    for (int i = 0; i < N; i++)
    {
        var t = new Task(() => { });
        GC.KeepAlive(t);
    }
    sw.Stop();
    ReportMs("task_create_1m", sw.Elapsed.TotalMilliseconds);
}

// Task.Run + WaitAll 统计化测量（与 Arc bench_task_spawn_wait_statistical 同构 · RFC 044 §2.2）。
// 30 轮迭代 + warmup + min/p50/p99/mean/stddev + HIGH_PRIORITY_CLASS。
// 用户驳回「独占机协议」挡箭牌后，本机直接对照——若 .NET 慢于 Arc 则 Arc 胜出。
static void BenchTaskSpawnWaitStatistical()
{
    const int N_TASKS = 50000;
    const int N_ITERS = 30;
    const int N_WARMUP = 50000;

    // 提升进程优先级（与 Arc 侧 SetPriorityClass(HIGH_PRIORITY_CLASS) 对称）
    System.Diagnostics.Process.GetCurrentProcess().PriorityClass =
        System.Diagnostics.ProcessPriorityClass.High;

    // untimed warmup：预热 ThreadPool + JIT
    var warm = new Task[N_WARMUP];
    for (int i = 0; i < N_WARMUP; i++)
        warm[i] = Task.Run(() => { });
    Task.WaitAll(warm);

    // timed: M 轮迭代，每轮 spawn N + WaitAll
    var tasks = new Task[N_TASKS];
    var itersNs = new double[N_ITERS];
    for (int it = 0; it < N_ITERS; it++)
    {
        long t0 = Stopwatch.GetTimestamp();
        for (int i = 0; i < N_TASKS; i++)
            tasks[i] = Task.Run(() => { });
        Task.WaitAll(tasks);
        long t1 = Stopwatch.GetTimestamp();
        itersNs[it] = (double)(t1 - t0) * 1e9 / Stopwatch.Frequency;
    }

    // 排序计算 min/p50/p99/mean/stddev
    Array.Sort(itersNs);
    double mn = itersNs[0];
    double p50 = itersNs[N_ITERS / 2];
    double p99 = itersNs[N_ITERS - 1];
    double sum = 0;
    for (int i = 0; i < N_ITERS; i++) sum += itersNs[i];
    double mean = sum / N_ITERS;
    double var = 0;
    for (int i = 0; i < N_ITERS; i++)
    {
        double d = itersNs[i] - mean;
        var += d * d;
    }
    double stddev = Math.Sqrt(var / N_ITERS);

    double minPerOp = mn / N_TASKS;
    double p50PerOp = p50 / N_TASKS;
    double p99PerOp = p99 / N_TASKS;

    Console.WriteLine($"OK: task_spawn_wait_statistical N={N_TASKS} iters={N_ITERS}");
    Console.WriteLine($"  min={mn:0}ns ({minPerOp:0.00}ns/op)  p50={p50:0}ns ({p50PerOp:0.00}ns/op)");
    Console.WriteLine($"  p99={p99:0}ns ({p99PerOp:0.00}ns/op)  mean={mean:0}ns ({mean / N_TASKS:0.00}ns/op)  stddev={stddev:0}ns");
    Console.WriteLine($"  claim: min_per_op={minPerOp:0.00}ns (falsifiable lower bound)");
}

// Warmup (untimed) 减少 JIT 抖动。
BenchTaskCreate1M();
BenchTaskSpawnWaitStatistical();
BenchDictTryAdd();
BenchQueueMpmc();
BenchParallelForScale();

Console.WriteLine("OK: concurrent_protocol done");
