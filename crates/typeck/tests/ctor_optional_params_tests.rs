//! RFC 057 M2：`new T(...)` 可选参数与命名实参 typeck 脱糖。

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

fn find_main_new_args(fns: &[TypedFn]) -> Vec<Expr> {
    let main = fns
        .iter()
        .find(|f| f.name.as_str() == "Main" || f.name.as_str().ends_with("Main"))
        .unwrap_or_else(|| {
            panic!(
                "Main missing; have {:?}",
                fns.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()
            )
        });
    let body = main.typed_body.as_ref().expect("typed Main body");
    for stmt in &body.stmts {
        if let TypedStmt::Let {
            init: Some(init), ..
        } = stmt
        {
            if let Expr::New { args, .. } = &init.node {
                return args.iter().map(|a| a.node.clone()).collect();
            }
        }
    }
    panic!("no new in Main; stmt count={}", body.stmts.len());
}

#[test]
fn ctor_optional_omitted_fills_default() {
    let src = r#"
class Box {
    public int V;
    public Box(int v = 42) { V = v; }
}
static class Program {
    public static int Main() {
        Box b = new Box();
        return b.V;
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_main_new_args(&fns);
    assert_eq!(args.len(), 1, "default should fill one positional arg");
    assert!(matches!(&args[0], Expr::IntLit(42)), "got {:?}", args[0]);
}

#[test]
fn ctor_named_args_reorder() {
    let src = r#"
class Pair {
    public int A;
    public int B;
    public Pair(int a, int b = 0) { A = a; B = b; }
}
static class Program {
    public static int Main() {
        Pair p = new Pair(b: 3, a: 1);
        return p.A + p.B;
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_main_new_args(&fns);
    assert_eq!(args.len(), 2);
    assert!(
        matches!(&args[0], Expr::IntLit(1)),
        "a first: {:?}",
        args[0]
    );
    assert!(
        matches!(&args[1], Expr::IntLit(3)),
        "b second: {:?}",
        args[1]
    );
}

#[test]
fn ctor_named_skips_middle_optional() {
    let src = r#"
class Opts {
    public int X;
    public int Y;
    public int Z;
    public Opts(int x, int y = 10, int z = 20) {
        X = x; Y = y; Z = z;
    }
}
static class Program {
    public static int Main() {
        Opts o = new Opts(1, z: 99);
        return o.Y + o.Z;
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_main_new_args(&fns);
    assert_eq!(args.len(), 3);
    assert!(matches!(&args[0], Expr::IntLit(1)));
    assert!(
        matches!(&args[1], Expr::IntLit(10)),
        "y default: {:?}",
        args[1]
    );
    assert!(matches!(&args[2], Expr::IntLit(99)));
}

#[test]
fn ctor_missing_required_errors() {
    let src = r#"
class Need {
    public Need(int x, int y = 1) {}
}
static class Program {
    public static void Main() {
        Need n = new Need();
    }
}
"#;
    let err = check_module(src).expect_err("should fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("no matching constructor")
            || msg.contains("missing")
            || msg.contains("argument"),
        "unexpected: {msg}"
    );
}

#[test]
fn ctor_signatures_preserve_defaults() {
    let src = r#"
class C {
    public C(int x = 7, string s = "hi") {}
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = typeck::TypeRegistry::from_module(&module);
    let ctors = reg.ctor_signatures(&"C".into());
    assert_eq!(ctors.len(), 1);
    assert_eq!(ctors[0].params.len(), 2);
    assert_eq!(ctors[0].params[0].name.as_str(), "x");
    assert_eq!(ctors[0].params[0].default, Some(typeck::ConstValue::Int(7)));
    assert_eq!(
        ctors[0].params[1].default,
        Some(typeck::ConstValue::String("hi".into()))
    );
}

#[test]
fn ctor_default_expr_fills_zero() {
    let src = r#"
class Box {
    public int V;
    public Box(int v = default(int)) { V = v; }
}
static class Program {
    public static int Main() {
        Box b = new Box();
        return b.V;
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_main_new_args(&fns);
    assert_eq!(args.len(), 1);
    assert!(matches!(&args[0], Expr::IntLit(0)), "got {:?}", args[0]);
}

#[test]
fn ctor_const_field_ref_default() {
    let src = r#"
class Opts {
    public const int DefaultY = 7;
}
class Point {
    public int X;
    public int Y;
    public Point(int x, int y = Opts.DefaultY) { X = x; Y = y; }
}
static class Program {
    public static int Main() {
        Point p = new Point(1);
        return p.Y;
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_main_new_args(&fns);
    assert_eq!(args.len(), 2);
    assert!(matches!(&args[0], Expr::IntLit(1)));
    assert!(matches!(&args[1], Expr::IntLit(7)), "got {:?}", args[1]);
}

fn find_derived_base_call_args(fns: &[TypedFn]) -> Vec<Expr> {
    let ctor = fns
        .iter()
        .find(|f| f.name.as_str().contains("__ctor::Derived"))
        .unwrap_or_else(|| {
            panic!(
                "Derived ctor missing; have {:?}",
                fns.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()
            )
        });
    let body = ctor.typed_body.as_ref().expect("typed ctor body");
    for stmt in &body.stmts {
        if let TypedStmt::Expr(e) = stmt {
            if let Expr::Call { func, args, .. } = &e.node {
                if let Expr::Ident(name) = &func.node {
                    if name.as_str().starts_with("__ctor::Base") {
                        // skip synthetic `this`
                        return args.iter().skip(1).map(|a| a.node.clone()).collect();
                    }
                }
            }
        }
    }
    panic!("no base call in Derived ctor");
}

#[test]
fn base_optional_omitted_fills_default() {
    let src = r#"
class Base {
    public int X;
    public int Y;
    public Base(int x, int y = 0) { X = x; Y = y; }
}
class Derived : Base {
    public Derived(int x) : base(x) {}
}
static class Program {
    public static int Main() {
        Derived d = new Derived(3);
        return d.Y;
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_derived_base_call_args(&fns);
    assert_eq!(args.len(), 2, "base should receive filled defaults");
    assert!(
        matches!(&args[0], Expr::Ident(n) if n.as_str() == "x")
            || matches!(&args[0], Expr::IntLit(_)),
        "got {:?}",
        args[0]
    );
    assert!(
        matches!(&args[1], Expr::IntLit(0)),
        "y default: {:?}",
        args[1]
    );
}

#[test]
fn base_named_args_reorder() {
    let src = r#"
class Base {
    public int A;
    public int B;
    public Base(int a, int b = 0) { A = a; B = b; }
}
class Derived : Base {
    public Derived() : base(b: 9, a: 1) {}
}
static class Program {
    public static int Main() {
        Derived d = new Derived();
        return d.A + d.B;
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_derived_base_call_args(&fns);
    assert_eq!(args.len(), 2);
    assert!(
        matches!(&args[0], Expr::IntLit(1)),
        "a first: {:?}",
        args[0]
    );
    assert!(
        matches!(&args[1], Expr::IntLit(9)),
        "b second: {:?}",
        args[1]
    );
}
