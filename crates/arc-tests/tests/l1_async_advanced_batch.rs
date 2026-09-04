//! L1 批量：async 高级特性回归集（4 case，#[ignore] 因 GAP #6）。
//!
//! async 状态机 codegen 在非 Windows 目标失败（cleanuppad 需要 EH personality）。
//! 修复 GAP #6 后移除 #[ignore]。

use arc_tests::assert_compiles_batch;

#[test]
#[ignore = "GAP #6: async state machine codegen needs EH personality on non-Windows"]
fn compiles_async_advanced_batch() {
    assert_compiles_batch(
        "async_advanced",
        &[
            (
                "spill_large_struct",
                r#"using Arc;

struct Big {
    public int F0;
    public int F1;
    public int F2;
    public int F3;
    public int F4;
    public int F5;
    public int F6;
    public int F7;
    public int F8;
    public int F9;
}

async Task<void> Main() {
    var big = new Big();
    big.F0 = 100;
    big.F1 = 42;
    big.F9 = 7;

    await Task.Delay(10);

    var s = big.F0 + big.F1 + big.F9;
    Console.WriteLine("" + s);
    if (big.F0 == 100 && big.F1 == 42 && big.F9 == 7) {
        Console.WriteLine("spill across await ok");
    }

    big.F2 = 9;
    await Task.Delay(5);
    var s2 = big.F2;
    Console.WriteLine("" + s2);
    if (s2 == 9) {
        Console.WriteLine("spill write across await ok");
    }
}
"#,
            ),
            (
                "small_locals_no_spill",
                r#"using Arc;

async Task<void> Main() {
    int a = 40;
    string tag = "no-spill";
    await Task.Delay(5);
    int s = a + 2;
    Console.WriteLine("" + s);
    Console.WriteLine(tag);
}
"#,
            ),
            (
                "spill_ir_below_threshold",
                r#"using Arc;

struct Mid {
    public int F0;
    public int F1;
    public int F2;
    public int F3;
    public int F4;
    public int F5;
}

async Task<void> Main() {
    var mid = new Mid();
    mid.F0 = 7;
    mid.F5 = 9;
    await Task.Delay(5);
    var s = mid.F0 + mid.F5;
    Console.WriteLine("" + s);
    if (s == 16) {
        Console.WriteLine("no spill below threshold ok");
    }
}
"#,
            ),
            (
                "lambda_capture",
                r#"using Arc;

class Counter {
    public int Value;
    public void Increment() {
        this.Value = this.Value + 1;
    }
}

async Task<int> FetchValueLam() {
    return 42;
}

async Task<int> FetchWithMultiplier(int b, int m) {
    return b * m;
}

async Task<void> Main() {
    Func<Task<int>> f1 = async () => await FetchValueLam();
    var r1 = await f1();
    if (r1 == 42) {
        Console.WriteLine("no-capture ok");
    }

    var multiplier = 10;
    Func<Task<int>> f2 = async () => {
        var v = await FetchValueLam();
        return v * multiplier;
    };
    var r2 = await f2();
    if (r2 == 420) {
        Console.WriteLine("value-capture ok");
    }

    var counter = new Counter();
    counter.Value = 0;
    Func<Task<int>> f3 = async () => {
        var v = await FetchValueLam();
        counter.Increment();
        return v + counter.Value;
    };
    var r3 = await f3();
    if (r3 == 43 && counter.Value == 1) {
        Console.WriteLine("class-capture ok");
    }

    var baseValue = 5;
    var ctr = new Counter();
    ctr.Value = 100;
    Func<Task<int>> f4 = async () => {
        var v = await FetchWithMultiplier(baseValue, multiplier);
        ctr.Increment();
        return v + ctr.Value;
    };
    var r4 = await f4();
    if (r4 == 151 && ctr.Value == 101) {
        Console.WriteLine("multi-capture ok");
    }

    Console.WriteLine("async_lambda_ok");
}
"#,
            ),
        ],
    );
}
