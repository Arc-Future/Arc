// Same-shape microbenches as Arc std_hotpath_bench_e2e (retired with arc-integration, a2627a0f)
// (List Add+get / Dict set+get / HashSet Add+Contains / StringBuilder append /
//  Task.Run spawn+wait / ConcurrentDictionary TryAdd+TryGet / File 64KiB Write+Read).
// Hard gate = correctness; prints ns/op for G8 same-machine compare. Not a product claim.
// One untimed warmup pass per scenario (JIT / tiering), then timed pass — Arc e2e has no
// warmup; document that difference in 3.3.

using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;

static void Report(string name, double ops, double nsTotal)
{
    double nsOp = nsTotal / (ops > 0.0 ? ops : 1.0);
    double opsS = ops * 1e9 / (nsTotal > 1.0 ? nsTotal : 1.0);
    Console.WriteLine($"OK: {name} ops={ops:0} ns_total={nsTotal:0} ns_per_op={nsOp:0.00} ops_per_s={opsS:0}");
}

static double ElapsedNs(Stopwatch sw) => sw.Elapsed.TotalNanoseconds;

static void BenchList(bool timed)
{
    const int N = 200_000;
    // 稳态测量：计时期外预分配容量，计时区内纯热路径（无扩容抖动）。
    var list = new List<int>(N);
    var sw = Stopwatch.StartNew();
    for (int i = 0; i < N; i++)
        list.Add(i);
    for (int i = 0; i < N; i++)
    {
        int got = list[i];
        if (got != i) throw new Exception($"list get mismatch at {i}");
    }
    sw.Stop();
    if (list.Count != N) throw new Exception("list count");
    if (timed) Report("list_add_get", N * 2.0, ElapsedNs(sw));
}

static void BenchDict(bool timed)
{
    const int N = 150_000;
    // 稳态测量：计时期外预分配容量，计时区内纯热路径（无扩容抖动）。
    var d = new Dictionary<int, int>(N);
    var sw = Stopwatch.StartNew();
    for (int i = 1; i <= N; i++)
        d[i] = i * 2;
    for (int i = 1; i <= N; i++)
    {
        if (d[i] != i * 2) throw new Exception($"dict get mismatch at {i}");
    }
    sw.Stop();
    if (d.Count != N) throw new Exception("dict count");
    if (timed) Report("dict_set_get", N * 2.0, ElapsedNs(sw));
}

static void BenchHashSet(bool timed)
{
    const int N = 150_000;
    // 稳态测量：计时期外预分配容量，计时区内纯热路径（无扩容抖动）。
    var s = new HashSet<int>();
    s.EnsureCapacity(N);
    var sw = Stopwatch.StartNew();
    for (int i = 1; i <= N; i++)
    {
        if (!s.Add(i)) throw new Exception($"hashset add failed at {i}");
    }
    for (int i = 1; i <= N; i++)
    {
        if (!s.Contains(i)) throw new Exception($"hashset contains miss at {i}");
        if (s.Add(i)) throw new Exception($"hashset duplicate add should fail at {i}");
    }
    sw.Stop();
    if (s.Count != N) throw new Exception("hashset count");
    if (timed) Report("hashset_add_contains", N * 3.0, ElapsedNs(sw));
}

static void BenchStringBuilder(bool timed)
{
    const int N = 100_000;
    var sb = new StringBuilder(N * 8);
    var sw = Stopwatch.StartNew();
    for (int i = 0; i < N; i++)
        sb.Append('x');
    string str = sb.ToString();
    sw.Stop();
    if (str.Length != N) throw new Exception("sb length");
    if (sb.Length != N) throw new Exception("sb Length prop");
    if (timed) Report("stringbuilder_append", (double)N, ElapsedNs(sw));
}

// async 任务：N 次 Task.Run 派发 + 批量 WaitAll（与 Arc bench_task_spawn_wait 同构）。
static void BenchTaskSpawn(bool timed)
{
    const int N = 50_000;
    var tasks = new Task[N];
    var sw = Stopwatch.StartNew();
    for (int i = 0; i < N; i++)
        tasks[i] = Task.Run(() => { });
    Task.WaitAll(tasks);
    sw.Stop();
    if (timed) Report("task_spawn_wait", (double)N, ElapsedNs(sw));
}

// 并发集合（单线程同构）：N 次 ConcurrentDictionary.TryAdd + TryGetValue
// （与 Arc bench_concurrent_dict_1t 同构；多线程吞吐见 std-concurrent-dotnet-cmp）。
static void BenchConcurrentDict1T(bool timed)
{
    const int N = 100_000;
    var d = new ConcurrentDictionary<int, int>(8, 127);
    var sw = Stopwatch.StartNew();
    for (int i = 1; i <= N; i++)
    {
        if (!d.TryAdd(i, i * 2)) throw new Exception($"cd add failed at {i}");
    }
    for (int i = 1; i <= N; i++)
    {
        if (!d.TryGetValue(i, out int v) || v != i * 2) throw new Exception($"cd get failed at {i}");
    }
    sw.Stop();
    if (d.Count != N) throw new Exception("cd count");
    if (timed) Report("concurrent_dict_1t", N * 2.0, ElapsedNs(sw));
}

// IO 吞吐：64 KiB 载荷 WriteAllText + ReadAllText 往返（与 Arc bench_file_io_throughput 同构）。
static void BenchFileIoThroughput(bool timed)
{
    const int N = 64;
    const int PayloadSize = 64 * 1024;
    string path = Path.Combine(Path.GetTempPath(), "std_hotpath_bench_io_big.tmp");
    string payload = new string('x', PayloadSize);
    var sw = Stopwatch.StartNew();
    for (int i = 0; i < N; i++)
    {
        File.WriteAllText(path, payload);
        string got = File.ReadAllText(path);
        if (got.Length != PayloadSize) throw new Exception("io length");
    }
    sw.Stop();
    File.Delete(path);
    if (timed) Report("file_io_throughput", N * 2.0, ElapsedNs(sw));
}

// Warmup (untimed) then timed — reduces first-method JIT skew on .NET.
BenchList(false);
BenchDict(false);
BenchHashSet(false);
BenchStringBuilder(false);
BenchTaskSpawn(false);
BenchConcurrentDict1T(false);
BenchFileIoThroughput(false);

BenchList(true);
BenchDict(true);
BenchHashSet(true);
BenchStringBuilder(true);
BenchTaskSpawn(true);
BenchConcurrentDict1T(true);
BenchFileIoThroughput(true);
