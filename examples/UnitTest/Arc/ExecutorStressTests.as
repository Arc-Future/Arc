namespace UnitTest.Arc;

using Arc;
using Arc.Diagnostics;
using Arc.QIF;
using Arc.Threading;

/// <summary>
/// 多线程 Executor（RFC 009 M6）压力测试：验证默认多线程策略下协程续体的
/// 并行执行、无丢失唤醒（lost wakeup）、无重入（double resume）。
/// 宿主 async Main 由 codegen 默认创建线程池（worker=硬件并发）并绑定为续体
/// 执行器——EventLoop 驱动线程驱动 IO/定时器，续体由 N worker 并行执行。
///
/// 判别信号：
///   - 确定性：每个并发任务对跨 await 续体累加做自校验（续体丢失/重复/重入
///     都会使 acc 偏离 → 抛异常 → Fact 失败）。
///   - 并行性：每任务记录观察到的工作线程数（Thread.ManagedThreadId）——
///     单线程 executor 下恒为 1，多线程 executor 下（多核）必 ≥2。
/// 多核假设：worker 数 = 硬件并发 ≥ 2（单核机断言并行会失败，非目标环境）。
/// </summary>
public class ExecutorStressTests
{
    private async Task<int> FanoutWorker(int id, int n)
    {
        int[] seen = new int[8];
        int seenCount = 0;
        long acc = 0;
        for (int i = 0; i < n; i++)
        {
            int tid = Thread.ManagedThreadId;
            bool dup = false;
            for (int k = 0; k < seenCount; k++)
            {
                if (seen[k] == tid) { dup = true; break; }
            }
            if (!dup && seenCount < 8)
            {
                seen[seenCount] = tid;
                seenCount++;
            }
            await Task.Delay(3);
            acc += (long)id + i;
        }
        long expected = (long)n * id + (long)n * (n - 1) / 2;
        if (acc != expected)
        {
            throw new Exception("executor_stress_acc_mismatch id=" + id);
        }
        return seenCount;
    }

    // ── 并发 fan-out：N 个 async 任务 × M 次跨 await 续体 ──

    [Fact]
    public async Task Executor_ConcurrentFanout_NoLostWakeup_Deterministic()
    {
        int N = 16;
        int ITERS = 12;
        Task<int>[] tasks = new Task<int>[N];
        for (int i = 0; i < N; i++)
        {
            tasks[i] = FanoutWorker(i, ITERS);
        }
        long total = 0;
        int maxSeen = 0;
        for (int i = 0; i < N; i++)
        {
            int sc = await tasks[i];
            total += sc;
            if (sc > maxSeen) maxSeen = sc;
        }
        // 确定性：每任务已完成（自校验抛异常即失败）。
        Assert.True(total > 0);
        // 并行性：至少一个任务观察到 ≥2 个不同 worker 线程（多线程 executor）。
        Assert.True(maxSeen >= 2, "executor_stress_not_parallel maxSeen=" + maxSeen);
    }

    [Fact]
    public async Task Executor_ConcurrentDelay_WallClock_Parallel()
    {
        // 12 任务 × 15 次 3ms delay：串行下界 = 540ms（细粒度定时）。
        // 并行多线程 executor 下远低于该下界；此处不硬断言墙钟（避免加载抖动），
        // 并行性由 maxSeen≥2 确定性判别，墙钟仅作报告观测值。
        int N = 12;
        int ITERS = 15;
        Stopwatch sw = Stopwatch.StartNew();
        Task<int>[] tasks = new Task<int>[N];
        for (int i = 0; i < N; i++)
        {
            tasks[i] = FanoutWorker(i, ITERS);
        }
        int totalSeen = 0;
        for (int i = 0; i < N; i++)
        {
            totalSeen += await tasks[i];
        }
        sw.Stop();
        Console.WriteLine("executor_stress_wall_ms=" + sw.ElapsedMilliseconds);
        Assert.True(totalSeen > 0);
    }

    // ── Task.Run 默认线程池：CPU 任务并发 + await 收口 ──

    [Fact]
    public async Task Executor_ThreadPool_RunFanout_NoLostWakeup()
    {
        int N = 32;
        Task<int>[] tasks = new Task<int>[N];
        for (int i = 0; i < N; i++)
        {
            int id = i;
            tasks[i] = Task.Run<int>(() =>
            {
                int x = 0;
                for (int j = 0; j < 8000; j++)
                {
                    x += (id * 31 + j) % 17;
                }
                return x;
            });
        }
        long total = 0;
        for (int i = 0; i < N; i++)
        {
            total += await tasks[i];
        }
        Assert.True(total > 0);
    }
}
