//! L2 批量运行时回归：语句级 `!.` / `?.` 调用与 λ 内裸静态字段赋值。
//!
//! 两宗 MIR lower 语句静默丢弃回归的运行时锚点（chord corpus 触发面）：
//! 一是 `ChordContext.Bubble` 祖先链 `up!.EmitSelf(...)`——`lower_stmt_expr`
//! 兜底 `_ => {}` 吞掉 ForceDeref/NullCond 语句，Bubble 只发自身不发祖先；
//! 二是 λ 内裸静态字段赋值（`() => { _cleaned = _cleaned + 1; }`）——无 `this`
//! 捕获的 λ lowering 上下文 class_fields 为空，`is_class_field` 门控恒 false，
//! 赋值整体消失（DisposableAction 撤销回调计数恒 0）。
//! 通过 `--features full-rt` 门控。

#[cfg(feature = "full-rt")]
use arc_tests::assert_compiles_and_runs_batch;

#[cfg(feature = "full-rt")]
#[test]
fn statement_forms_regression_batch() {
    let results = assert_compiles_and_runs_batch(
        "stmt_regression",
        &[
            (
                "force_deref_stmt_call",
                r#"using Arc;

class Node {
    public Node? Parent;
    public int Hits;
    public int LineHits;
    public void Fire() {
        Hits = Hits + 1;
    }
    public void LineFire() {
        LineHits = LineHits + 1;
    }
    public void Bubble() {
        this.Fire();
        Node? up = Parent;
        while (up != null) {
            up!.Fire();
            up = up!.Parent;
        }
    }
    public void GuardedLine() {
        Node? up = Parent;
        if (up != null) {
            up!.LineFire();
        }
    }
}

void Main() {
    Node app = new Node();
    Node child = new Node();
    child.Parent = app;
    // while 体内 `up!.Fire();` 语句不得被丢弃（祖先冒泡）
    child.Bubble();
    if (app.Hits != 1) { Console.WriteLine("ARC_CASE:force_deref_stmt_call:FAIL:bubble_app"); return; }
    if (child.Hits != 1) { Console.WriteLine("ARC_CASE:force_deref_stmt_call:FAIL:bubble_child"); return; }
    // if 体内 `up!.LineFire();` 语句不得被丢弃
    child.GuardedLine();
    if (app.LineHits != 1) { Console.WriteLine("ARC_CASE:force_deref_stmt_call:FAIL:guarded_line"); return; }
    Console.WriteLine("ARC_CASE:force_deref_stmt_call:PASS");
}
"#,
            ),
            (
                "lambda_bare_static_write",
                r#"using Arc;

class Counter {
    public static int A;
    public static int B;
    public static int C;
    public static int D;

    public static void BumpA() {
        A = A + 1;
    }

    public void BumpB() {
        B = B + 1;
    }

    public static void BumpC() {
        Action fn = () => {
            C = C + 1;
        };
        fn();
    }

    public void BumpD() {
        Action fn = () => {
            D = D + 1;
        };
        fn();
    }
}

void Main() {
    Counter c = new Counter();
    Counter.BumpA();
    c.BumpB();
    Counter.BumpC();
    c.BumpD();
    if (Counter.A != 1) { Console.WriteLine("ARC_CASE:lambda_bare_static_write:FAIL:a"); return; }
    if (Counter.B != 1) { Console.WriteLine("ARC_CASE:lambda_bare_static_write:FAIL:b"); return; }
    if (Counter.C != 1) { Console.WriteLine("ARC_CASE:lambda_bare_static_write:FAIL:c"); return; }
    if (Counter.D != 1) { Console.WriteLine("ARC_CASE:lambda_bare_static_write:FAIL:d"); return; }
    Console.WriteLine("ARC_CASE:lambda_bare_static_write:PASS");
}
"#,
            ),
        ],
    );

    for r in &results {
        assert!(
            r.passed,
            "case `{}` failed: {:?} stdout: {}",
            r.name, r.error, r.stdout
        );
    }
}
