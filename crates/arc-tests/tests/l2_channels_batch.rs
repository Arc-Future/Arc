//! L2 批量：Arc.Threading.Channels 通道运行时回归集（8 case，full-rt 门控）。
//!
//! RFC 046：按包引用 std/Arc/Threading/Channels（`using Arc.Threading.Channels;`），
//! 覆盖 MPMC 直付/收纳交接、Wait 背压、三种 Drop 模式、完成信号、终结语义
//!（缓冲余量复查）、协作取消、ReadAllAsync 流式消费与环形扩容。
//! case 按批量协议自打 `ARC_CASE:<name>:PASS/FAIL:<msg>` 标记，消费返回值
//! 逐 case 断言。async case（`async Task<void> Main`）由批量 driver 以
//! `await` 调度。通过 `--features full-rt` 门控。

#[cfg(feature = "full-rt")]
use arc_tests::assert_compiles_and_runs_batch;

#[cfg(feature = "full-rt")]
fn assert_all_passed(batch: &str, results: &[arc_tests::BatchRunResult]) {
    for r in results {
        assert!(
            r.passed,
            "{batch}: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_channels_batch() {
    let results = assert_compiles_and_runs_batch(
        "channels",
        &[
            (
                "channel_unbounded_mpmc",
                r#"using Arc;
using Arc.Threading.Channels;

class ChMpmcHost {
    private Channel<int> _channel;
    public int SumA;
    public int SumB;

    public ChMpmcHost() {
        _channel = Channels.CreateUnbounded<int>();
    }

    public async Task Produce(int seed) {
        for (int i = 1; i <= 500; i++) {
            await _channel.Writer.WriteAsync(seed * 1000000 + i);
        }
    }

    public async Task Consume(int slot) {
        int sum = 0;
        while (true) {
            try {
                int v = await _channel.Reader.ReadAsync();
                sum = sum + v;
            } catch (ChannelClosedException e) {
                break;
            }
        }
        if (slot == 0) {
            this.SumA = sum;
        } else {
            this.SumB = sum;
        }
    }

    public async Task Coordinate() {
        Task p1 = this.Produce(1);
        Task p2 = this.Produce(2);
        await Task.WhenAll(p1, p2);
        _channel.Writer.Complete();
    }
}

async Task<void> Main() {
    ChMpmcHost host = new ChMpmcHost();
    Task coordinator = host.Coordinate();
    Task c1 = host.Consume(0);
    Task c2 = host.Consume(1);
    await Task.WhenAll(coordinator, c1, c2);
    int total = host.SumA + host.SumB;
    if (total == 1500250500) {
        Console.WriteLine("ARC_CASE:channel_unbounded_mpmc:PASS");
    } else {
        Console.WriteLine("ARC_CASE:channel_unbounded_mpmc:FAIL:total=" + total);
    }
}
"#,
            ),
            (
                "channel_bounded_backpressure",
                r#"using Arc;
using Arc.Threading.Channels;

class ChBackpressureHost {
    private Channel<int> _channel;
    public int Received;
    public bool Ordered;

    public ChBackpressureHost() {
        _channel = Channels.CreateBounded<int>(4);
        this.Ordered = true;
    }

    public async Task ProduceAll() {
        for (int i = 0; i < 100; i++) {
            await _channel.Writer.WriteAsync(i);
        }
    }

    public async Task Consume() {
        while (true) {
            try {
                int v = await _channel.Reader.ReadAsync();
                if (v != this.Received) {
                    this.Ordered = false;
                }
                this.Received = this.Received + 1;
            } catch (ChannelClosedException e) {
                break;
            }
        }
    }

    public async Task Coordinate() {
        Task c = this.Consume();
        Task p = this.ProduceAll();
        await p;
        _channel.Writer.Complete();
        await c;
    }
}

async Task<void> Main() {
    ChBackpressureHost host = new ChBackpressureHost();
    await host.Coordinate();
    if (host.Ordered && host.Received == 100) {
        Console.WriteLine("ARC_CASE:channel_bounded_backpressure:PASS");
    } else {
        Console.WriteLine("ARC_CASE:channel_bounded_backpressure:FAIL:received=" + host.Received);
    }
}
"#,
            ),
            (
                "channel_trywrite_count_sync",
                r#"using Arc;
using Arc.Threading.Channels;

void Main() {
    Channel<int> ch = Channels.CreateBounded<int>(2);
    bool ok = ch.Reader.CanCount();
    ok = ok && ch.Writer.TryWrite(1);
    ok = ok && ch.Writer.TryWrite(2);
    ok = ok && ch.Reader.Count() == 2;
    ok = ok && !ch.Writer.TryWrite(3);
    int v = 0;
    ok = ok && ch.Reader.TryRead(out v) && v == 1;
    ok = ok && ch.Reader.Count() == 1;
    ok = ok && ch.Writer.TryWrite(3);
    ok = ok && ch.Reader.TryRead(out v) && v == 2;
    ok = ok && ch.Reader.TryRead(out v) && v == 3;
    ok = ok && !ch.Reader.TryRead(out v);
    if (ok) {
        Console.WriteLine("ARC_CASE:channel_trywrite_count_sync:PASS");
    } else {
        Console.WriteLine("ARC_CASE:channel_trywrite_count_sync:FAIL");
    }
}
"#,
            ),
            (
                "channel_drop_modes_sync",
                r#"using Arc;
using Arc.Threading.Channels;

void Main() {
    bool ok = true;
    int v = 0;

    BoundedChannelOptions oldestOpt = new BoundedChannelOptions(2);
    oldestOpt.FullMode = BoundedChannelFullMode.DropOldest;
    Channel<int> oldest = Channels.CreateBounded<int>(oldestOpt);
    ok = ok && oldest.Writer.TryWrite(1);
    ok = ok && oldest.Writer.TryWrite(2);
    ok = ok && oldest.Writer.TryWrite(3);
    ok = ok && oldest.Reader.TryRead(out v) && v == 2;
    ok = ok && oldest.Reader.TryRead(out v) && v == 3;
    ok = ok && !oldest.Reader.TryRead(out v);

    BoundedChannelOptions newestOpt = new BoundedChannelOptions(2);
    newestOpt.FullMode = BoundedChannelFullMode.DropNewest;
    Channel<int> newest = Channels.CreateBounded<int>(newestOpt);
    ok = ok && newest.Writer.TryWrite(1);
    ok = ok && newest.Writer.TryWrite(2);
    ok = ok && newest.Writer.TryWrite(3);
    ok = ok && newest.Reader.TryRead(out v) && v == 1;
    ok = ok && newest.Reader.TryRead(out v) && v == 2;
    ok = ok && !newest.Reader.TryRead(out v);

    BoundedChannelOptions writeOpt = new BoundedChannelOptions(2);
    writeOpt.FullMode = BoundedChannelFullMode.DropWrite;
    Channel<int> write = Channels.CreateBounded<int>(writeOpt);
    ok = ok && write.Writer.TryWrite(1);
    ok = ok && write.Writer.TryWrite(2);
    ok = ok && write.Writer.TryWrite(3);
    ok = ok && write.Reader.TryRead(out v) && v == 1;
    ok = ok && write.Reader.TryRead(out v) && v == 2;
    ok = ok && !write.Reader.TryRead(out v);

    if (ok) {
        Console.WriteLine("ARC_CASE:channel_drop_modes_sync:PASS");
    } else {
        Console.WriteLine("ARC_CASE:channel_drop_modes_sync:FAIL");
    }
}
"#,
            ),
            (
                "channel_completion_closed",
                r#"using Arc;
using Arc.Threading.Channels;

async Task<void> Main() {
    bool ok = true;

    Channel<int> ch = Channels.CreateBounded<int>(8);
    ch.Writer.TryWrite(1);
    ch.Writer.TryWrite(2);
    ch.Writer.Complete();
    int v = 0;
    ok = ok && ch.Reader.TryRead(out v) && v == 1;
    ok = ok && ch.Reader.TryRead(out v) && v == 2;
    ok = ok && !ch.Reader.TryRead(out v);
    if (!ok) {
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:drain");
        return;
    }
    try {
        await ch.Reader.ReadAsync();
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:read-should-throw");
        return;
    } catch (ChannelClosedException e) {
    }
    bool done = await ch.Reader.Completion();
    if (!done) {
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:completion");
        return;
    }
    if (ch.Writer.TryWrite(3)) {
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:trywrite-after-close");
        return;
    }
    try {
        await ch.Writer.WriteAsync(4);
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:write-after-close");
        return;
    } catch (ChannelClosedException e) {
    }
    try {
        ch.Writer.Complete();
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:double-complete");
        return;
    } catch (ChannelClosedException e) {
    }

    Channel<int> bad = Channels.CreateUnbounded<int>();
    bad.Writer.TryWrite(10);
    bad.Writer.Complete(new Exception("boom"));
    ok = bad.Reader.TryRead(out v) && v == 10;
    if (!ok) {
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:drain-error");
        return;
    }
    try {
        await bad.Reader.Completion();
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:completion-should-fault");
        return;
    } catch (Exception e) {
        if (e.Message != "boom") {
            Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:completion-error=" + e.Message);
            return;
        }
    }
    try {
        await bad.Reader.ReadAsync();
        Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:read-error-expected");
        return;
    } catch (Exception e) {
        if (e.Message != "boom") {
            Console.WriteLine("ARC_CASE:channel_completion_closed:FAIL:read-error=" + e.Message);
            return;
        }
    }
    Console.WriteLine("ARC_CASE:channel_completion_closed:PASS");
}
"#,
            ),
            (
                "channel_readall_growth",
                r#"using Arc;
using Arc.Collections;
using Arc.Threading.Channels;

class ChReadAllSource {
    public IAsyncEnumerable<int> Source() {
        Channel<int> ch = Channels.CreateUnbounded<int>();
        for (int i = 0; i < 40; i++) {
            ch.Writer.TryWrite(i);
        }
        ch.Writer.Complete();
        return ch.Reader.ReadAllAsync();
    }
}

async Task<void> Main() {
    ChReadAllSource source = new ChReadAllSource();
    IAsyncEnumerator<int> e = source.Source().GetAsyncEnumerator(CancellationToken.None);
    int expect = 0;
    bool ok = true;
    while (true) {
        bool more = await e.MoveNextAsync();
        if (!more) {
            break;
        }
        int item = e.Current;
        if (item != expect) {
            ok = false;
            break;
        }
        expect++;
    }
    if (ok && expect == 40) {
        Console.WriteLine("ARC_CASE:channel_readall_growth:PASS");
    } else {
        Console.WriteLine("ARC_CASE:channel_readall_growth:FAIL:count=" + expect);
    }
}
"#,
            ),
            (
                "channel_cancel",
                r#"using Arc;
using Arc.Threading.Channels;

class ChCancelHost {
    private Channel<int> _channel;

    public ChCancelHost() {
        _channel = Channels.CreateBounded<int>(1);
    }

    public async Task<int> ReadWithCancel(CancellationToken ct) {
        return await _channel.Reader.ReadAsync(ct);
    }

    public async Task WriteWithCancel(int item, CancellationToken ct) {
        await _channel.Writer.WriteAsync(item, ct);
    }

    public bool TryPut(int item) {
        return _channel.Writer.TryWrite(item);
    }

    public int DrainOne() {
        int v = 0;
        _channel.Reader.TryRead(out v);
        return v;
    }
}

async Task<void> Main() {
    CancellationTokenSource cts1 = new CancellationTokenSource();
    ChCancelHost host = new ChCancelHost();
    Task<int> read = host.ReadWithCancel(cts1.Token);
    await Task.WhenAny(read, Task.Delay(200));
    cts1.Cancel();
    try {
        int v = await read;
        Console.WriteLine("ARC_CASE:channel_cancel:FAIL:read-returned=" + v);
        return;
    } catch (Exception e) {
    }
    if (!host.TryPut(7) || host.DrainOne() != 7) {
        Console.WriteLine("ARC_CASE:channel_cancel:FAIL:unhealthy");
        return;
    }
    CancellationTokenSource cts2 = new CancellationTokenSource();
    if (!host.TryPut(100)) {
        Console.WriteLine("ARC_CASE:channel_cancel:FAIL:put-100");
        return;
    }
    Task writeTask = host.WriteWithCancel(200, cts2.Token);
    await Task.WhenAny(writeTask, Task.Delay(200));
    if (writeTask.IsCompleted) {
        Console.WriteLine("ARC_CASE:channel_cancel:FAIL:write-not-suspended");
        return;
    }
    cts2.Cancel();
    try {
        await writeTask;
        Console.WriteLine("ARC_CASE:channel_cancel:FAIL:write-completed");
        return;
    } catch (Exception e) {
    }
    if (host.DrainOne() != 100) {
        Console.WriteLine("ARC_CASE:channel_cancel:FAIL:phantom-write");
        return;
    }
    Console.WriteLine("ARC_CASE:channel_cancel:PASS");
}
"#,
            ),
            (
                "channel_bounded_mpmc_stress",
                r#"using Arc;
using Arc.Threading.Channels;

class ChStressHost {
    private Channel<int> _channel;
    public int SumA;
    public int SumB;
    public int SumC;

    public ChStressHost() {
        _channel = Channels.CreateBounded<int>(8);
    }

    public async Task Produce(int seed) {
        for (int i = 1; i <= 200; i++) {
            await _channel.Writer.WriteAsync(seed * 1000000 + i);
        }
    }

    public async Task Consume(int slot) {
        int sum = 0;
        while (true) {
            try {
                int v = await _channel.Reader.ReadAsync();
                sum = sum + v;
            } catch (ChannelClosedException e) {
                break;
            }
        }
        if (slot == 0) {
            this.SumA = sum;
        }
        if (slot == 1) {
            this.SumB = sum;
        }
        if (slot == 2) {
            this.SumC = sum;
        }
    }

    public async Task Coordinate() {
        Task p1 = this.Produce(1);
        Task p2 = this.Produce(2);
        Task p3 = this.Produce(3);
        await Task.WhenAll(p1, p2, p3);
        _channel.Writer.Complete();
    }
}

async Task<void> Main() {
    ChStressHost host = new ChStressHost();
    Task coordinator = host.Coordinate();
    Task c1 = host.Consume(0);
    Task c2 = host.Consume(1);
    Task c3 = host.Consume(2);
    await Task.WhenAll(coordinator, c1, c2, c3);
    int total = host.SumA + host.SumB + host.SumC;
    // 正确期望：sum(seed×1000000) = (1+2+3)×1000000×200 = 1200000000，
    // 加上每写者 1..200 的 i 和 20100 × 3 = 60300。
    if (total == 1200060300) {
        Console.WriteLine("ARC_CASE:channel_bounded_mpmc_stress:PASS");
    } else {
        Console.WriteLine("ARC_CASE:channel_bounded_mpmc_stress:FAIL:total=" + total);
    }
}
"#,
            ),
        ],
    );
    assert_all_passed("channels", &results);
}
