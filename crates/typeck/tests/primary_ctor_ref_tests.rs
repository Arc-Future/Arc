//! Primary ctor `ref`/`out`/`in`：不捕获；实例成员引用须硬错误（对齐 C# CS9109）。

use hir::HirBuilder;
use parse::Parser;
use typeck::*;

fn check_module(src: &str) -> Result<Vec<TypedFn>, Vec<TypeError>> {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
}

#[test]
fn primary_ref_field_init_ok() {
    let src = r#"
public class Holder(ref int x) {
    public int Snapshot = x;
}
"#;
    let fns = check_module(src).expect("ref primary + field init should typeck");
    assert!(
        fns.iter()
            .any(|f| f.name.as_str().contains("__ctor::Holder")),
        "expected synthesized ctor"
    );
}

#[test]
fn primary_ref_capture_in_method_rejected() {
    let src = r#"
public class Bad(ref int x) {
    public int Get() { return x; }
}
"#;
    let err = check_module(src).expect_err("capturing ref primary in method must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("Undefined") || msg.contains("undefined") || msg.contains("`x`"),
        "unexpected errors: {msg}"
    );
}

#[test]
fn primary_out_unassigned_rejected() {
    let src = r#"
public class Bad(out int x) {
}
"#;
    let err = check_module(src).expect_err("unassigned out primary must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("out parameter") && msg.contains("must be assigned"),
        "unexpected errors: {msg}"
    );
}

#[test]
fn primary_out_via_base_ok() {
    let src = r#"
public class Base {
    public int N;
    public Base(out int n) { n = 42; N = n; }
}
public class Derived(out int x) : Base(out x) {
}
"#;
    assert!(
        check_module(src).is_ok(),
        "out primary forwarded to base should typeck"
    );
}

#[test]
fn out_forwarded_in_return_expr_ok() {
    let src = r#"
public static class Helper {
    public static bool Assign(out int x) { x = 42; return true; }
    public static bool Forward(out int v) { return Helper.Assign(out v); }
}
"#;
    assert!(
        check_module(src).is_ok(),
        "out param forwarded inside return expression should typeck \
         (return-value evaluation marks it assigned before the definite-assignment check)"
    );
}

#[test]
fn out_unassigned_on_return_rejected() {
    let src = r#"
public static class Helper {
    public static bool NotAssign(out int x) { return true; }
}
"#;
    let err = check_module(src).expect_err("unassigned out on return must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("out parameter") && msg.contains("must be assigned"),
        "unexpected errors: {msg}"
    );
}
