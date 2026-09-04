//! L1 快测（批量模式）：基础语言特性合并为一次编译。
//!
//! 核心提速：所有 `assert_compiles` 用例合并为单次 `assert_compiles_batch`。

use arc_tests::{assert_compiles_batch, assert_rejected};

#[test]
fn rejects_undefined_symbol() {
    assert_rejected(
        "l1_undefined_symbol",
        r#"using Arc;
void Main() {
    blah_blah_blah(1);
}
"#,
        "undefined",
    );
}

#[test]
fn rejects_multiple_main() {
    assert_rejected(
        "l1_two_main",
        r#"using Arc;
void Main() { }
void Main(int x) { }
"#,
        "Main",
    );
}

#[test]
fn compiles_basic_batch() {
    assert_compiles_batch(
        "l1_basic",
        &[
            (
                "hello",
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
