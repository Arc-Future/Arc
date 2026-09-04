//! RFC 057 M2c / D7：lambda IIFE 可选/命名实参 typeck 脱糖；
//! 非 IIFE 流入 Func/Action 硬拒绝（委托不携带默认槽）。

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

fn assert_defaults_hard_reject(err: Vec<TypeError>) {
    let msg = format!("{err:?}");
    assert!(
        msg.contains("immediate calls") || msg.contains("Func/Action") || msg.contains("M2c"),
        "unexpected errors: {msg}"
    );
}

fn find_main_call_args(fns: &[TypedFn]) -> Vec<Expr> {
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
            if let Expr::Call { args, func, .. } = &init.node {
                assert!(
                    matches!(func.node, Expr::Lambda(_)),
                    "expected lambda IIFE callee"
                );
                return args.iter().map(|a| a.node.clone()).collect();
            }
        }
        if let TypedStmt::Return(Some(ret)) = stmt {
            if let Expr::Call { args, func, .. } = &ret.node {
                assert!(
                    matches!(func.node, Expr::Lambda(_)),
                    "expected lambda IIFE callee"
                );
                return args.iter().map(|a| a.node.clone()).collect();
            }
        }
    }
    panic!(
        "no lambda IIFE call in Main; stmt count={}",
        body.stmts.len()
    );
}

#[test]
fn lambda_iife_optional_omitted_fills_default() {
    let src = r#"
static class Program {
    public static int Main() {
        int r = ((a: int, b: int = 10) => a + b)(1);
        return r;
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_main_call_args(&fns);
    assert_eq!(args.len(), 2, "default should fill second arg");
    assert!(matches!(&args[0], Expr::IntLit(1)), "got {:?}", args[0]);
    assert!(matches!(&args[1], Expr::IntLit(10)), "got {:?}", args[1]);
}

#[test]
fn lambda_iife_named_reorder() {
    let src = r#"
static class Program {
    public static int Main() {
        return ((a: int, b: int = 0) => a + b)(b: 3, a: 1);
    }
}
"#;
    let fns = check_module(src).expect("typeck");
    let args = find_main_call_args(&fns);
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
fn lambda_defaults_assigned_to_func_hard_rejected() {
    let src = r#"
static class Program {
    public static int Main() {
        Func<int, int, int> f = (a: int, b: int = 10) => a + b;
        return f(1, 2);
    }
}
"#;
    let err = check_module(src).expect_err("must reject Func assignment");
    assert_defaults_hard_reject(err);
}

#[test]
fn lambda_defaults_assigned_to_action_hard_rejected() {
    let src = r#"
static class Program {
    public static int Main() {
        Action<int, int> a = (x: int, y: int = 0) => { };
        a(1, 2);
        return 0;
    }
}
"#;
    let err = check_module(src).expect_err("must reject Action assignment");
    assert_defaults_hard_reject(err);
}

#[test]
fn lambda_defaults_as_call_arg_hard_rejected() {
    let src = r#"
static class Program {
    public static int Apply(Func<int, int, int> f) {
        return f(1, 2);
    }
    public static int Main() {
        return Apply((a: int, b: int = 10) => a + b);
    }
}
"#;
    let err = check_module(src).expect_err("must reject lambda-with-defaults as arg");
    assert_defaults_hard_reject(err);
}

#[test]
fn lambda_defaults_as_return_hard_rejected() {
    let src = r#"
static class Program {
    public static Func<int, int, int> Make() {
        return (a: int, b: int = 10) => a + b;
    }
    public static int Main() {
        Func<int, int, int> f = Make();
        return f(1, 2);
    }
}
"#;
    let err = check_module(src).expect_err("must reject lambda-with-defaults as return");
    assert_defaults_hard_reject(err);
}
