// async IO 对比 harness：与 Arc `adv_async_io_throughput_e2e` 同构（该 bench 已随 arc-integration 退场，a2627a0f；同 K/ROUNDS/offset 公式）。
//
// 镜像 Arc 工作负载：
//   - 64 MiB 文件，单 fd
//   - 每轮批量提交 K=4096 个 4KB offset 异步读 → 全部完成
//   - ROUNDS=256，总 ops = 1,048,576
//   - offset(j) = (j*4096) % (64MiB - 4096)，复用 Arc 公式
//   - 4 轮 untimed warmup，再计时 ROUNDS 轮
//
// 惯用路径：`RandomAccess.ReadAsync` + `FileOptions.Asynchronous` = 真 IOCP
// （与 Arc 的 IOCP reactor 后端同构），非阻塞线程池。
// 输出 `OK:` 行供 `run-async-io-cmp.ps1` 解析成对比。仅锚点，不作业界领先宣称。

using System.Diagnostics;
using Microsoft.Win32.SafeHandles;

const long FileSize = 64L * 1024 * 1024;
const int Buf = 4096;
const int K = 4096;
const int Rounds = 256;

string path = Path.Combine(Path.GetTempPath(), "adv_async_io_dotnet.tmp");

// 创建 64 MiB 测试文件（普通句柄写入，再以异步句柄重开）。
{
    var chunk = new byte[1024 * 1024];
    Array.Fill(chunk, (byte)'x');
    using var setup = new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.ReadWrite);
    for (int i = 0; i < 64; i++) setup.Write(chunk, 0, chunk.Length);
}

// 异步句柄：FileOptions.Asynchronous → 真 IOCP。
using SafeFileHandle handle = File.OpenHandle(
    path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite, FileOptions.Asynchronous);

// 预分配 K 个 buffer（各并发读独立，跨轮复用，与 Arc 的 OVERLAPPED/buffer 复用池同构）。
var bufs = new byte[K][];
for (int i = 0; i < K; i++) bufs[i] = new byte[Buf];

long Offset(int j) => ((long)j * Buf) % (FileSize - Buf);

Task RunRound()
{
    var tasks = new Task<int>[K];
    for (int j = 0; j < K; j++)
    {
        tasks[j] = RandomAccess.ReadAsync(handle, bufs[j], Offset(j)).AsTask();
    }
    return Task.WhenAll(tasks);
}

// warmup（不计时）
for (int w = 0; w < 4; w++) await RunRound();

var sw = Stopwatch.StartNew();
for (int r = 0; r < Rounds; r++) await RunRound();
sw.Stop();

double ops = (double)K * Rounds;
double nsTotal = sw.Elapsed.TotalNanoseconds;
double nsOp = nsTotal / ops;
double opsS = ops * 1e9 / nsTotal;
Console.WriteLine($"OK: async_io_dotnet ops={ops} ns_total={nsTotal} ns_per_op={nsOp:0.00} ops_per_s={opsS:0}");
