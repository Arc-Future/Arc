//! L1 批量：枚举与基础类型编译期回归集（2 case）。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_finally_exceptions_batch() {
    assert_compiles_batch(
        "finally_exceptions",
        &[
            (
                "exception_types",
                r#"using Arc;

void Main() {
        Exception constructed = new Exception("x");
        string s = constructed.ToString();

        Exception inner = new Exception("inner");
        Exception outer = new Exception("outer", inner);
        string s2 = outer.ToString();
}
"#,
            ),
            (
                "enum_bitops",
                r#"using Arc;

public enum Perms {
    PNone = 0,
    Read = 1,
    Write = 2,
    Execute = 4,
}

void Main() {
    Perms p = Perms.Read | Perms.Write;
    Perms has = p & Perms.Read;
    Perms x = p ^ Perms.Write;
    Perms n = ~Perms.PNone;
}
"#,
            ),
        ],
    );
}
