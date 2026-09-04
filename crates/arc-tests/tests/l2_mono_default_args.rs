//! L2 批量：单态化类方法默认参数回归集（编译器缺口收口配套，full-rt 门控）。
//!
//! 缺口：`register_monomorphized_class` 的 OopSig 构造将 `default` 硬编码为
//! None——单态化泛型类上省略默认实参的调用（`Get(1)` 对
//! `Get(T, ct = default)`）报 no matching overload；非泛型路径
//!（`method_sig_from_ast`）不受影响。修复后本批为守门回归。

#[cfg(feature = "full-rt")]
use arc_tests::assert_compiles_and_runs_batch;

#[cfg(feature = "full-rt")]
#[test]
fn runs_mono_default_args_batch() {
    let results = assert_compiles_and_runs_batch(
        "mono_default_args",
        &[
            (
                "mono_default_args_basic",
                r#"using Arc;
using Arc.Collections;
using Arc.Threading;

public abstract class DReader<T> {
    public abstract Task<T> Get(T seed, CancellationToken ct = default);

    public void Seal(Exception? e = null) {
    }
}

public class DReaderImpl<T> : DReader<T> {
    public override async Task<T> Get(T seed, CancellationToken ct = default) {
        ct.ThrowIfCancellationRequested();
        return seed;
    }
}

public class DHold<T> {
    private DReader<T> _r;

    public DHold(DReader<T> r) {
        _r = r;
    }

    public DReader<T> R { get { return _r; } }
}

public static class DFact {
    public static DHold<T> Make<T>(T seed) {
        return new DHold<T>(new DReaderImpl<T>());
    }
}

async Task<void> Main() {
    DHold<int> h = DFact.Make<int>(5);
    int v = await h.R.Get(1);
    h.R.Seal();
    if (v == 1) {
        Console.WriteLine("ARC_CASE:mono_default_args_basic:PASS");
    } else {
        Console.WriteLine("ARC_CASE:mono_default_args_basic:FAIL");
    }
}
"#,
            ),
        ],
    );
    for r in results {
        assert!(
            r.passed,
            "mono_default_args: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}
