//! RFC 068：方法组 → Func/Action；M1–M3（命名空间静态 / 扩展 / 硬拒绝立宪）。

use ast::Expr;
use hir::HirBuilder;
use parse::Parser;
use typeck::{TypeChecker, TypeError, TypedFn, TypedStmt};

fn check_module(src: &str) -> Result<Vec<TypedFn>, Vec<TypeError>> {
    let program = Parser::parse_program(src).expect("parse");
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).expect("hir");
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
}

fn assert_err_contains(err: Vec<TypeError>, needles: &[&str]) {
    let msg = format!("{err:?}");
    assert!(
        needles.iter().any(|n| msg.contains(n)),
        "expected one of {needles:?} in errors: {msg}"
    );
}

#[test]
fn method_group_assign_desugars_to_lambda() {
    let fns = check_module(
        r#"
int Double(int x) { return x * 2; }
int Main() {
    Func<int, int> f = Double;
    return f(5);
}
"#,
    )
    .expect("typeck");
    let main = fns.iter().find(|f| f.name.as_str() == "Main").unwrap();
    let body = main.typed_body.as_ref().unwrap();
    let TypedStmt::Let {
        init: Some(init), ..
    } = &body.stmts[0]
    else {
        panic!("expected Let");
    };
    assert!(
        matches!(init.node, Expr::Lambda(_)),
        "method group must desugar to Lambda, got {:?}",
        init.node
    );
}

#[test]
fn method_group_as_call_arg_desugars() {
    let fns = check_module(
        r#"
int Double(int x) { return x * 2; }
int Apply(Func<int, int> f, int x) { return f(x); }
int Main() {
    return Apply(Double, 5);
}
"#,
    )
    .expect("typeck");
    let main = fns.iter().find(|f| f.name.as_str() == "Main").unwrap();
    let body = main.typed_body.as_ref().unwrap();
    let TypedStmt::Return(Some(ret)) = &body.stmts[0] else {
        panic!("expected Return");
    };
    let Expr::Call { args, .. } = &ret.node else {
        panic!("expected Call");
    };
    assert!(
        matches!(args[0].node, Expr::Lambda(_)),
        "arg method group must desugar to Lambda, got {:?}",
        args[0].node
    );
}

#[test]
fn method_group_undefined_hard_rejected() {
    let err = check_module(
        r#"
int Main() {
    Func<int, int> f = NoSuch;
    return 0;
}
"#,
    )
    .expect_err("must reject undefined");
    assert_err_contains(err, &["Undefined", "NoSuch"]);
}

#[test]
fn method_group_signature_mismatch_hard_rejected() {
    let err = check_module(
        r#"
int Double(int x) { return x * 2; }
int Main() {
    Action<int> a = Double;
    return 0;
}
"#,
    )
    .expect_err("must reject signature mismatch");
    assert_err_contains(err, &["Mismatch", "Func", "Action", "void", "int"]);
}

#[test]
fn method_group_static_desugars() {
    let fns = check_module(
        r#"
class C {
    public static int Double(int x) { return x * 2; }
}
int Main() {
    Func<int, int> f = C.Double;
    return f(5);
}
"#,
    )
    .expect("typeck");
    let main = fns.iter().find(|f| f.name.as_str() == "Main").unwrap();
    let body = main.typed_body.as_ref().unwrap();
    let TypedStmt::Let {
        init: Some(init), ..
    } = &body.stmts[0]
    else {
        panic!("expected Let");
    };
    assert!(
        matches!(init.node, Expr::Lambda(_)),
        "static method group must desugar to Lambda, got {:?}",
        init.node
    );
}

#[test]
fn method_group_instance_desugars() {
    let fns = check_module(
        r#"
class C {
    public int Inc(int x) { return x + 1; }
}
int Main() {
    C c = new C();
    Func<int, int> f = c.Inc;
    return f(5);
}
"#,
    )
    .expect("typeck");
    let main = fns.iter().find(|f| f.name.as_str() == "Main").unwrap();
    let body = main.typed_body.as_ref().unwrap();
    let TypedStmt::Let {
        init: Some(init), ..
    } = &body.stmts[1]
    else {
        panic!("expected Let for method group");
    };
    assert!(
        matches!(init.node, Expr::Lambda(_)),
        "instance method group must desugar to Lambda, got {:?}",
        init.node
    );
}

#[test]
fn method_group_static_signature_mismatch_rejected() {
    let err = check_module(
        r#"
class C {
    public static int Double(int x) { return x * 2; }
}
int Main() {
    Action<int> a = C.Double;
    return 0;
}
"#,
    )
    .expect_err("must reject static signature mismatch");
    assert_err_contains(err, &["Mismatch", "Func", "static"]);
}

#[test]
fn method_group_complex_receiver_hard_rejected() {
    let err = check_module(
        r#"
class C {
    public int Inc(int x) { return x + 1; }
}
int Main() {
    Func<int, int> f = new C().Inc;
    return 0;
}
"#,
    )
    .expect_err("must hard-reject complex receiver");
    assert_err_contains(err, &["RFC 068", "M3", "complex"]);
}

#[test]
fn method_group_ns_qualified_static_hard_rejected() {
    let err = check_module(
        r#"
namespace Util {
    class Math {
        public static int Double(int x) { return x * 2; }
    }
}
int Main() {
    Func<int, int> f = Util.Math.Double;
    return 0;
}
"#,
    )
    .expect_err("Ns.Type.Method must hard-reject");
    assert_err_contains(err, &["RFC 068", "M3", "namespace"]);
}

#[test]
fn method_group_extension_desugars() {
    let fns = check_module(
        r#"
class C { }
static class CExt {
    public static int Double(this C c, int x) { return x * 2; }
}
int Main() {
    C c = new C();
    Func<int, int> f = c.Double;
    return f(5);
}
"#,
    )
    .expect("typeck");
    let main = fns.iter().find(|f| f.name.as_str() == "Main").unwrap();
    let body = main.typed_body.as_ref().unwrap();
    let TypedStmt::Let {
        init: Some(init), ..
    } = &body.stmts[1]
    else {
        panic!("expected Let for extension method group");
    };
    assert!(
        matches!(init.node, Expr::Lambda(_)),
        "extension method group must desugar to Lambda, got {:?}",
        init.node
    );
}

#[test]
fn method_group_nested_field_receiver_hard_rejected() {
    let err = check_module(
        r#"
class Inner {
    public int Inc(int x) { return x + 1; }
}
class Box {
    public Inner inner;
}
int Main() {
    Box b = new Box();
    Func<int, int> f = b.inner.Inc;
    return 0;
}
"#,
    )
    .expect_err("nested field receiver must hard-reject");
    assert_err_contains(err, &["RFC 068", "M3", "complex"]);
}

#[test]
fn method_group_to_expression_hard_rejected() {
    let err = check_module(
        r#"
int Double(int x) { return x * 2; }
int Main() {
    Expression<Func<int, int>> e = Double;
    return 0;
}
"#,
    )
    .expect_err("method group → Expression must hard-reject");
    assert_err_contains(err, &["RFC 068", "M3", "Expression"]);
}
