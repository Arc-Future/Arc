//! L1 快测（批量模式）：N 个合法 case 合并为一次编译调用。
//!
//! 核心提速：避免 N 次 std 库重新加载/解析/typeck。
//! 所有 `assert_compiles` 用例合并为单次 `assert_compiles_batch`。

use arc_tests::{assert_compiles_batch, assert_rejected, compile_in_process};

// ── 类型不匹配 / 编译期拒绝（独立、本身很快） ──

#[test]
fn rejects_type_mismatch_int_string() {
    assert_rejected(
        "l1_type_mismatch_int_string",
        r#"using Arc;
void Main() {
    int x = "hello";
}
"#,
        "type",
    );
}

#[test]
fn rejects_type_mismatch_float_int() {
    assert_rejected(
        "l1_type_mismatch_float_int",
        r#"using Arc;
void Main() {
    int n = 3.14;
}
"#,
        "type",
    );
}

#[test]
fn rejects_abstract_class_instantiation() {
    assert_rejected(
        "l1_abstract_new",
        r#"using Arc;
abstract class Base {
    public abstract int Get();
}
void Main() {
    var b = new Base();
}
"#,
        "",
    );
}

#[test]
fn rejects_assign_to_readonly_field() {
    assert_rejected(
        "l1_readonly_assign",
        r#"using Arc;
class C {
    public readonly int X;
    public C() { X = 1; }
    public void Set() { X = 2; }
}
void Main() { }
"#,
        "",
    );
}

#[test]
fn rejects_unknown_method_call() {
    assert_rejected(
        "l1_unknown_method",
        r#"using Arc;
class Foo { }
void Main() {
    var f = new Foo();
    f.Bar();
}
"#,
        "",
    );
}

#[test]
fn rejects_wrong_argument_count() {
    assert_rejected(
        "l1_wrong_args",
        r#"using Arc;
int Add(int a, int b) { return a + b; }
void Main() {
    int x = Add(1);
}
"#,
        "",
    );
}

#[test]
fn rejects_unrecognized_keyword_as_type() {
    assert_rejected(
        "l1_bad_type",
        r#"using Arc;
void Main() {
    UnknownType x = 1;
}
"#,
        "",
    );
}

/// RFC 029 §7.3：`lock (expr)` 要求 expr 为 Lock 类型（原 arc-integration
/// lock_statement_e2e 编译拒绝路径迁移，typeck 阶段即拒绝，归 L1）。
#[test]
fn rejects_lock_statement_non_lock_type() {
    let err = compile_in_process(
        "l1_lock_non_lock_type",
        r#"using Arc;
using Arc.Threading;

void Main() {
    int x = 0;
    lock (x) {
        Console.WriteLine(1);
    }
}
"#,
        &[],
    )
    .expect_err("lock(int) must fail typeck");
    assert!(
        err.contains("Lock") || err.contains("Mismatch") || err.contains("expected"),
        "expected Lock type diagnostic, got: {err}"
    );
}

// ── 合法程序（合并为一次编译） ──

#[test]
fn compiles_all_features_batch() {
    assert_compiles_batch(
        "l1_all_features",
        &[
            (
                "generic_class",
                r#"using Arc;
public class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
void Main() {
    var b = new Box<int>(99);
}
"#,
            ),
            (
                "lambda_expression",
                r#"using Arc;
void Main() {
    Func<int, int> square = (x) => x * x;
    int r = square(5);
}
"#,
            ),
            (
                "async_hello",
                r#"using Arc;
async Task<int> Fetch() {
    await Task.Delay(1);
    return 42;
}
async Task Main() {
    int val = await Fetch();
    Console.WriteLine(val.ToString());
}
"#,
            ),
            (
                "switch_expression",
                r#"using Arc;
void Main() {
    int n = 2;
    string s = n switch { 1 => "one", 2 => "two", _ => "other" };
    Console.WriteLine(s);
}
"#,
            ),
            (
                "record_type",
                r#"using Arc;
public record Point(int X, int Y);
void Main() {
    var p = new Point(3, 4);
    Console.WriteLine(p.X.ToString());
}
"#,
            ),
            (
                "class_with_method",
                r#"using Arc;
public class Counter {
    public int Count;
    public void Inc() { Count = Count + 1; }
}
void Main() {
    var c = new Counter();
    c.Inc();
    c.Inc();
}
"#,
            ),
            (
                "struct_value_type",
                r#"using Arc;
public struct Vec2 {
    public int X;
    public int Y;
    public Vec2(int x, int y) { X = x; Y = y; }
}
void Main() {
    var p = new Vec2(1, 2);
}
"#,
            ),
            (
                "array_usage",
                r#"using Arc;
void Main() {
    int[] arr = new int[5];
    arr[0] = 10;
    arr[1] = 20;
    int sum = arr[0] + arr[1];
    Console.WriteLine(sum.ToString());
}
"#,
            ),
            (
                "for_loop",
                r#"using Arc;
void Main() {
    int[] arr = new int[3];
    arr[0] = 1; arr[1] = 2; arr[2] = 3;
    int sum = 0;
    for (int i = 0; i < 3; i++) {
        sum += arr[i];
    }
    Console.WriteLine(sum.ToString());
}
"#,
            ),
            (
                "while_loop",
                r#"using Arc;
void Main() {
    int n = 5;
    int fact = 1;
    while (n > 0) {
        fact *= n;
        n -= 1;
    }
    Console.WriteLine(fact.ToString());
}
"#,
            ),
            (
                "basic_hello",
                r#"using Arc;
void Main() {
    Console.WriteLine("hello");
}
"#,
            ),
            (
                "function_call",
                r#"using Arc;
int Add(int a, int b) { return a + b; }
int Factorial(int n) { if (n <= 1) { return 1; } return n * Factorial(n - 1); }
void Main() {
    int x = Add(10, 20);
    if (x == 30) {
        Console.WriteLine("ok-{x}");
    }
}
"#,
            ),
        ],
    );
}
