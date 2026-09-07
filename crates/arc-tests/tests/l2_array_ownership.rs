//! L2：RFC 052 数组所有权（ArcHeader + 局部 drop + Keys/Values 快照）。
//! full-rt 门控。

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
fn runs_array_ownership_batch() {
    let results = assert_compiles_and_runs_batch(
        "array_ownership",
        &[
            (
                "arr_scalar_drop",
                r#"using Arc;

void Main() {
    for (int i = 0; i < 2000; i++) {
        byte[] buf = new byte[4096];
        buf[0] = 1;
        buf[4095] = 2;
    }
    Console.WriteLine("ARC_CASE:arr_scalar_drop:PASS");
}
"#,
            ),
            (
                "arr_class_elems",
                r#"using Arc;

class Payload {
    public int N;
    public Payload(int n) { this.N = n; }
}

void Main() {
    for (int round = 0; round < 500; round++) {
        Payload[] xs = new Payload[8];
        for (int i = 0; i < 8; i++) {
            xs[i] = new Payload(i);
        }
        if (xs[3].N != 3) {
            Console.WriteLine("ARC_CASE:arr_class_elems:FAIL:slot");
            return;
        }
    }
    Console.WriteLine("ARC_CASE:arr_class_elems:PASS");
}
"#,
            ),
            (
                "arr_alias_share",
                r#"using Arc;

void Main() {
    int[] a = [1, 2, 3];
    int[] b = a;
    b[1] = 99;
    if (a[1] != 99) {
        Console.WriteLine("ARC_CASE:arr_alias_share:FAIL");
        return;
    }
    Console.WriteLine("ARC_CASE:arr_alias_share:PASS");
}
"#,
            ),
            (
                "dict_values_snapshot",
                r#"using Arc;
using Arc.Collections;

class Box {
    public int V;
    public Box(int v) { this.V = v; }
}

void Main() {
    Dictionary<int, Box> d = new Dictionary<int, Box>();
    d.Add(1, new Box(10));
    d.Add(2, new Box(20));
    Box[] snap = d.Values;
    d.Clear();
    if (snap.Length != 2) {
        Console.WriteLine("ARC_CASE:dict_values_snapshot:FAIL:len");
        return;
    }
    if (snap[0].V + snap[1].V != 30) {
        Console.WriteLine("ARC_CASE:dict_values_snapshot:FAIL:sum");
        return;
    }
    Console.WriteLine("ARC_CASE:dict_values_snapshot:PASS");
}
"#,
            ),
        ],
    );
    assert_all_passed("array_ownership", &results);
}
