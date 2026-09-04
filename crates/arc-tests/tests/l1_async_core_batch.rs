//! L1 批量：异步核心回归集（6 case）。
//!
//! 从 async_core_batch_e2e.rs 提取，改为 L1 纯编译期测试。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_async_core_batch() {
    assert_compiles_batch(
        "async_core",
        &[
            (
                "tasks_min",
                r#"using Arc;

async Task<int> AcFetch() {
    return 42;
}

void Main() {
    Console.WriteLine("async_min_ok");
}
"#,
            ),
            (
                "typed_string",
                r#"using Arc;

async Task<string> AcFetchGreeting() {
    return "hello async";
}

void Main() {
    Console.WriteLine("async_string_ok");
}
"#,
            ),
            (
                "typed_double",
                r#"using Arc;

async Task<double> AcFetchPi() {
    return 3.14;
}

void Main() {
    Console.WriteLine("async_double_ok");
}
"#,
            ),
            (
                "typed_int",
                r#"using Arc;

async Task<int> AcFetchValue() {
    return 42;
}

void Main() {
    Console.WriteLine("async_int_ok");
}
"#,
            ),
            (
                "typed_mixed",
                r#"using Arc;

async Task<string> AcGetName() {
    return "Arc";
}

async Task<int> AcGetVersion() {
    return 1;
}

void Main() {
    Console.WriteLine("async_mixed_ok");
}
"#,
            ),
            (
                "enum_return",
                r#"using Arc;

public enum AcOutcome {
    Completed,
    Failed,
    Cancelled,
}

public class AcRunner {
    public async Task<int> Pause() {
        return 7;
    }

    public async Task<AcOutcome> Pick(bool ok) {
        int v = await this.Pause();
        if (ok) { return AcOutcome.Completed; }
        return AcOutcome.Failed;
    }
}

void Main() {
    AcRunner r = new AcRunner();
    Console.WriteLine("async_enum_ok");
}
"#,
            ),
        ],
    );
}
