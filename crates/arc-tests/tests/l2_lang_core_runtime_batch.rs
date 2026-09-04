//! L2 批量：语言核心运行时回归集（7 case）。
//!
//! 从 lang_core_batch_e2e.rs 提取。case 按批量协议自打
//! `ARC_CASE:<name>:PASS / FAIL:<msg>` 标记，Rust 侧消费返回值逐 case
//! 断言（修复早期版本 case 未打标记且丢弃返回值的假绿）。
//! 通过 `--features full-rt` 门控。

#[cfg(feature = "full-rt")]
use arc_tests::assert_compiles_and_runs_batch;

#[cfg(feature = "full-rt")]
#[test]
fn runs_lang_core_batch() {
    let results = assert_compiles_and_runs_batch(
        "lang_core_runtime",
        &[
            (
                "bitwise_ops",
                r#"using Arc;

void Main() {
    int a = 0b1100;
    int b = 0b1010;
    if ((a & b) != 0b1000) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:and"); return; }
    if ((a | b) != 0b1110) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:or"); return; }
    if ((a ^ b) != 0b0110) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:xor"); return; }
    if ((1 << 4) != 16) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:shl"); return; }
    if ((0b1010 >> 1) != 0b0101) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:shr"); return; }
    if ((-8) >> 2 != -2) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:ashr"); return; }
    if (~5 != -6) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:not"); return; }
    if ((0b11 | 0b10 ^ 0b01 & 0b10) != 0b11) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:prec"); return; }
    int flags = 0x01 | 0x02;
    if (flags != 0x03) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:flags"); return; }
    int masked = flags & 0x02;
    if (masked != 0x02) { Console.WriteLine("ARC_CASE:bitwise_ops:FAIL:mask"); return; }
    Console.WriteLine("ARC_CASE:bitwise_ops:PASS");
}
"#,
            ),
            (
                "math_statics",
                r#"using Arc;

void Main() {
    int absNeg = Math.Abs(-5);
    if (absNeg != 5) { Console.WriteLine("ARC_CASE:math_statics:FAIL:abs_neg"); return; }
    int absPos = Math.Abs(3);
    if (absPos != 3) { Console.WriteLine("ARC_CASE:math_statics:FAIL:abs_pos"); return; }
    double floor = Math.Floor(3.7);
    if (!(floor > 2.999 && floor < 3.001)) { Console.WriteLine("ARC_CASE:math_statics:FAIL:floor"); return; }
    double clamp = Math.Clamp(20.0, 0.0, 10.0);
    if (!(clamp > 9.999 && clamp < 10.001)) { Console.WriteLine("ARC_CASE:math_statics:FAIL:clamp"); return; }
    int lo = 0;
    int hi = 10;
    int clampInt = Math.Clamp(20, lo, hi);
    if (clampInt != 10) { Console.WriteLine("ARC_CASE:math_statics:FAIL:clamp_int"); return; }
    int signNeg = Math.Sign(-5);
    int signPos = Math.Sign(5);
    if (signNeg != -1) { Console.WriteLine("ARC_CASE:math_statics:FAIL:sign_neg"); return; }
    if (signPos != 1) { Console.WriteLine("ARC_CASE:math_statics:FAIL:sign_pos"); return; }
    long la = -9;
    long lb = 4;
    long mn = Math.Min(la, lb);
    long mx = Math.Max(la, lb);
    if (!(mn == la)) { Console.WriteLine("ARC_CASE:math_statics:FAIL:min"); return; }
    if (!(mx == lb)) { Console.WriteLine("ARC_CASE:math_statics:FAIL:max"); return; }
    Console.WriteLine("ARC_CASE:math_statics:PASS");
}
"#,
            ),
            (
                "compound_assign",
                r#"using Arc;

class Counter {
    private int _n;
    public int N {
        get { return _n; }
        set { _n = value; }
    }
}

void Main() {
    int a = 10;
    a += 3;
    if (a != 13) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:add"); return; }
    a -= 5;
    if (a != 8) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:sub"); return; }
    a *= 2;
    if (a != 16) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:mul"); return; }
    a /= 4;
    if (a != 4) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:div"); return; }

    Counter c = new Counter();
    c.N = 10;
    c.N += 5;
    if (c.N != 15) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:prop_add"); return; }
    c.N *= 2;
    if (c.N != 30) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:prop_mul"); return; }

    int s = 0;
    for (int i = 0; i < 3; i += 1) {
        s += i;
    }
    if (s != 3) { Console.WriteLine("ARC_CASE:compound_assign:FAIL:for_inc"); return; }

    Console.WriteLine("ARC_CASE:compound_assign:PASS");
}
"#,
            ),
            (
                "switch_expr",
                r#"using Arc;

void Main() {
    int n = 2;
    string s = n switch { 1 => "one", 2 => "two", _ => "other" };
    if (!(s == "two")) { Console.WriteLine("ARC_CASE:switch_expr:FAIL:match"); return; }
    Console.WriteLine("ARC_CASE:switch_expr:PASS");
}
"#,
            ),
            (
                "switch_expr_when",
                r#"using Arc;

void Main() {
    int n = 5;
    string s = n switch { int x when x < 0 => "neg", int x when x > 0 => "pos", _ => "zero" };
    if (!(s == "pos")) { Console.WriteLine("ARC_CASE:switch_expr_when:FAIL:match"); return; }
    Console.WriteLine("ARC_CASE:switch_expr_when:PASS");
}
"#,
            ),
            (
                "when_clause",
                r#"using Arc;

void Main() {
    int x = 5;
    bool hit = false;
    switch (x) {
        case int n when n > 3: { hit = true; break; }
        default: { break; }
    }
    if (!hit) { Console.WriteLine("ARC_CASE:when_clause:FAIL:match"); return; }
    Console.WriteLine("ARC_CASE:when_clause:PASS");
}
"#,
            ),
            (
                "loop_break_continue",
                r#"using Arc;

void Main() {
    int i = 0;
    while (true) {
        if (i >= 3) { break; }
        i = i + 1;
    }
    if (i != 3) { Console.WriteLine("ARC_CASE:loop_break_continue:FAIL:break"); return; }

    int outer = 0;
    int hits = 0;
    while (outer < 2) {
        int inner = 0;
        while (true) {
            if (inner >= 2) { break; }
            hits = hits + 1;
            inner = inner + 1;
        }
        outer = outer + 1;
    }
    if (outer != 2) { Console.WriteLine("ARC_CASE:loop_break_continue:FAIL:nested_outer"); return; }
    if (hits != 4) { Console.WriteLine("ARC_CASE:loop_break_continue:FAIL:nested_hits"); return; }

    int j = 0;
    int sum = 0;
    while (j < 5) {
        j = j + 1;
        if (j == 3) { continue; }
        sum = sum + j;
    }
    if (sum != 12) { Console.WriteLine("ARC_CASE:loop_break_continue:FAIL:continue_sum"); return; }
    if (j != 5) { Console.WriteLine("ARC_CASE:loop_break_continue:FAIL:continue_i"); return; }

    int n = 0;
    for (int k = 0; k < 10; k = k + 1) {
        if (k == 4) { break; }
        n = n + 1;
    }
    if (n != 4) { Console.WriteLine("ARC_CASE:loop_break_continue:FAIL:for_break"); return; }

    Console.WriteLine("ARC_CASE:loop_break_continue:PASS");
}
"#,
            ),
        ],
    );
    for r in &results {
        assert!(
            r.passed,
            "lang_core_runtime: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}

#[cfg(not(feature = "full-rt"))]
#[test]
fn runs_lang_core_batch() {
    // L2 runtime tests require --features full-rt
}
