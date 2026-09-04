// 基本语法性能对比 harness：与 Arc `syntax_perf_bench_e2e.rs` 同构（同 N/ops/负载）。
//
// 场景：
//   1. loop_sum            纯算术循环求和 N=5e7（同 Arc/rust 公式 s += i ^ (i>>3)）
//   2. string_replace_long 1MB 文本（'a' 为主 + 16 处 "xyz"）替换 "xyz"→"XYZ" 20 次
//   3. file_concurrency    8 线程各自 write+read 64KB 文件 50 次（Task.Run）
//
// 输出 `OK:` 行供 `run-syntax-perf-cmp.ps1` 解析。仅锚点，不作业界领先宣称。

using System.Diagnostics;
using System.Text;

static void Report(string name, double ops, double nsTotal)
{
    double nsOp = nsTotal / (ops > 0.0 ? ops : 1.0);
    double opsS = ops * 1e9 / (nsTotal > 1.0 ? nsTotal : 1.0);
    Console.WriteLine($"OK: {name} ops={ops:0} ns_total={nsTotal:0} ns_per_op={nsOp:0.00} ops_per_s={opsS:0}");
}

// 1. 纯算术循环（N=5e7；sink 用后防 elision）
{
    const long N = 50_000_000;
    long s = 0;
    var sw = Stopwatch.StartNew();
    for (long i = 0; i < N; i++) s += i ^ (i >> 3);
    sw.Stop();
    long sink = s;
    if (sink == 12345) throw new Exception("unreachable");
    Report("loop_sum", N, sw.Elapsed.TotalNanoseconds);
}

// 2. 长文本 replace（1MB + 16 处 "xyz" → "XYZ"，20 次）
{
    const int Len = 1_048_576;
    const int Occ = 16;
    const int Step = 65536;
    const int N = 20;
    var sb = new StringBuilder(Len);
    sb.Append('a', Len);
    for (int i = 0; i < Occ; i++)
    {
        sb[i * Step] = 'x'; sb[i * Step + 1] = 'y'; sb[i * Step + 2] = 'z';
    }
    string s = sb.ToString();
    var sw = Stopwatch.StartNew();
    for (int i = 0; i < N; i++) s = s.Replace("xyz", "XYZ");
    sw.Stop();
    if (s.Length != Len) throw new Exception("replace length");
    Report("string_replace_long", N, sw.Elapsed.TotalNanoseconds);
}

// 3. 文件操作并发（8 线程 × 50 次 write+read 64KB）
{
    const int T = 8, M = 50;
    const int Payload = 64 * 1024;
    byte[] payload = new byte[Payload];
    Array.Fill(payload, (byte)'x');
    string dir = Path.GetTempPath();
    var paths = new string[T];
    for (int t = 0; t < T; t++) paths[t] = Path.Combine(dir, $"synfc_{t}.tmp");
    var tasks = new Task[T];
    var sw = Stopwatch.StartNew();
    for (int t = 0; t < T; t++)
    {
        int ti = t;
        tasks[ti] = Task.Run(() =>
        {
            for (int i = 0; i < M; i++)
            {
                File.WriteAllBytes(paths[ti], payload);
                byte[] got = File.ReadAllBytes(paths[ti]);
                if (got.Length != Payload) throw new Exception("fc length");
            }
        });
    }
    Task.WaitAll(tasks);
    sw.Stop();
    foreach (var p in paths) File.Delete(p);
    Report("file_concurrency", T * M * 2, sw.Elapsed.TotalNanoseconds);
}
