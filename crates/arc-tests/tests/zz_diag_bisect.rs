//! 临时取证批：case8 残余挂死 bisect（根因定位后删除）。
//! V1 = case8 精确规模（3产3消 bounded(8) WhenAll）；
//! V2 = 1产1消 + WhenAll；V3 = 1产1消 顺序 await（无 WhenAll）。
#![cfg(feature = "full-rt")]

use arc_tests::assert_compiles_and_runs_batch;

#[cfg(feature = "full-rt")]
fn assert_all_passed(batch: &str, results: &[arc_tests::BatchRunResult]) {
    for r in results {
        let tail: Vec<&str> = r.stdout.lines().rev().take(3).collect();
        let mut shown: Vec<&str> = tail.into_iter().rev().collect();
        shown.reverse();
        eprintln!("[{batch}:{}] stdout-tail: {:?}", r.name, shown);
        assert!(
            r.passed,
            "{batch}: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}

/// V1：case8 精确规模。
#[test]
#[ignore = "临时取证批（case8 残余挂死 bisect + 心跳计数器）：按需 --ignored 运行，定位后删除"]
fn bisect_v1_case8_scale() {
    let results = assert_compiles_and_runs_batch(
        "bisect_v1",
        &[(
            "v1_case8",
            r#"using Arc;
using Arc.Threading.Channels;

class Host {
    private Channel<int> _ch;
    public int SumA;
    public int SumB;
    public int SumC;

    public Host() {
        _ch = Channels.CreateBounded<int>(8);
    }

    public async Task Produce(int seed) {
        for (int i = 1; i <= 200; i++) {
            await _ch.Writer.WriteAsync(seed * 1000000 + i);
        }
    }

    public async Task Consume(int slot) {
        int sum = 0;
        while (true) {
            try {
                int v = await _ch.Reader.ReadAsync();
                sum = sum + v;
            } catch (ChannelClosedException e) {
                break;
            }
        }
        if (slot == 0) { this.SumA = sum; }
        if (slot == 1) { this.SumB = sum; }
        if (slot == 2) { this.SumC = sum; }
    }

    public async Task Coordinate() {
        Task p1 = this.Produce(1);
        Task p2 = this.Produce(2);
        Task p3 = this.Produce(3);
        await Task.WhenAll(p1, p2, p3);
        _ch.Writer.Complete();
    }

    public async Task Heartbeat() {
        while (true) {
            await Task.Delay(300);
            Console.WriteLine("DIAG " + ChannelDiag.Snapshot());
        }
    }
}

async Task<void> Main() {
    Host host = new Host();
    Task hb = host.Heartbeat();
    Task coordinator = host.Coordinate();
    Task c1 = host.Consume(0);
    Task c2 = host.Consume(1);
    Task c3 = host.Consume(2);
    await Task.WhenAll(coordinator, c1, c2, c3);
    int total = host.SumA + host.SumB + host.SumC;
    if (total == 1200060300) {
        Console.WriteLine("ARC_CASE:v1_case8:PASS");
    } else {
        Console.WriteLine("ARC_CASE:v1_case8:FAIL:" + total);
    }
}
"#,
        )],
    );
    assert_all_passed("bisect_v1", &results);
}

/// V2：1产1消 + WhenAll（容量 2，50 项）。
#[test]
#[ignore = "临时取证批（case8 残余挂死 bisect + 心跳计数器）：按需 --ignored 运行，定位后删除"]
fn bisect_v2_pair_whenall() {
    let results = assert_compiles_and_runs_batch(
        "bisect_v2",
        &[(
            "v2_pair_whenall",
            r#"using Arc;
using Arc.Threading.Channels;

class Host {
    private Channel<int> _ch;
    public int Sum;

    public Host() {
        _ch = Channels.CreateBounded<int>(2);
    }

    public async Task Produce() {
        for (int i = 1; i <= 50; i++) {
            await _ch.Writer.WriteAsync(i);
        }
    }

    public async Task Consume() {
        while (true) {
            try {
                int v = await _ch.Reader.ReadAsync();
                Sum = Sum + v;
            } catch (ChannelClosedException e) {
                break;
            }
        }
    }

    public void CompleteChannel() {
        _ch.Writer.Complete();
    }
}

async Task<void> Main() {
    Host host = new Host();
    Task p = host.Produce();
    Task c = host.Consume();
    await p;
    host.CompleteChannel();
    await c;
    if (host.Sum != 1275) { Console.WriteLine("ARC_CASE:v2_pair_whenall:FAIL:" + host.Sum); return; }
    Console.WriteLine("ARC_CASE:v2_pair_whenall:PASS");
}
"#,
        )],
    );
    assert_all_passed("bisect_v2", &results);
}

/// V5：最小心跳隔离——纯 Task.Delay+打印（无通道），验证心跳通路本身。
#[test]
#[ignore = "临时取证批（case8 残余挂死 bisect + 心跳计数器）：按需 --ignored 运行，定位后删除"]
fn bisect_v5_heartbeat_only() {
    let results = assert_compiles_and_runs_batch(
        "bisect_v5",
        &[(
            "v5_hb",
            r#"using Arc;

async Task<void> Main() {
    int n = 0;
    while (n < 3) {
        await Task.Delay(200);
        Console.WriteLine("DIAG tick " + n);
        n = n + 1;
    }
    Console.WriteLine("ARC_CASE:v5_hb:PASS");
}
"#,
        )],
    );
    assert_all_passed("bisect_v5", &results);
}

/// V4：case8 结构 + 通道诊断计数器心跳（冻结时定位未发生的通道操作）。
#[test]
#[ignore = "临时取证批（case8 残余挂死 bisect + 心跳计数器）：按需 --ignored 运行，定位后删除"]
fn bisect_v4_diag_heartbeat() {
    let results = assert_compiles_and_runs_batch(
        "bisect_v4",
        &[(
            "v4_diag",
            r#"using Arc;
using Arc.Threading.Channels;

class Host {
    private Channel<int> _ch;
    public int SumA;
    public int SumB;
    public int SumC;

    public Host() {
        _ch = Channels.CreateBounded<int>(8);
    }

    public async Task Produce(int seed) {
        for (int i = 1; i <= 200; i++) {
            await _ch.Writer.WriteAsync(seed * 1000000 + i);
        }
    }

    public async Task Consume(int slot) {
        int sum = 0;
        while (true) {
            try {
                int v = await _ch.Reader.ReadAsync();
                sum = sum + v;
            } catch (ChannelClosedException e) {
                break;
            }
        }
        if (slot == 0) { this.SumA = sum; }
        if (slot == 1) { this.SumB = sum; }
        if (slot == 2) { this.SumC = sum; }
    }

    public async Task Coordinate() {
        Task p1 = this.Produce(1);
        Task p2 = this.Produce(2);
        Task p3 = this.Produce(3);
        await Task.WhenAll(p1, p2, p3);
        _ch.Writer.Complete();
    }

    public async Task Heartbeat() {
        while (true) {
            await Task.Delay(300);
            Console.WriteLine("DIAG " + ChannelDiag.Snapshot());
        }
    }
}

async Task<void> Main() {
    Host host = new Host();
    Task hb = host.Heartbeat();
    Task coordinator = host.Coordinate();
    Task c1 = host.Consume(0);
    Task c2 = host.Consume(1);
    Task c3 = host.Consume(2);
    await Task.WhenAll(coordinator, c1, c2, c3);
    int total = host.SumA + host.SumB + host.SumC;
    if (total == 1200060300) {
        Console.WriteLine("ARC_CASE:v4_diag:PASS");
    } else {
        Console.WriteLine("ARC_CASE:v4_diag:FAIL:" + total);
    }
}
"#,
        )],
    );
    assert_all_passed("bisect_v4", &results);
}


