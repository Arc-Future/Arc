use ast::*;
use parse::Parser;

#[test]
fn optional_param_parse() {
    let src = r#"
class C {
    public int Add(int a, int b = 1) {
        return a + b;
    }
}
"#;
    let prog = Parser::parse_program(src).expect("parse");
    let Item::Class(c) = &prog.items[0].node else {
        panic!("not class")
    };
    let m = &c.methods[0].node;
    assert_eq!(m.sig.params.len(), 2);
    assert!(m.sig.params[0].default.is_none());
    assert!(
        m.sig.params[1].default.is_some(),
        "b default missing: {:?}",
        m.sig.params[1]
    );
}

#[test]
fn optional_ctor_param_parse() {
    let src = r#"
class C {
    public C(int a, int b = 2) {}
}
"#;
    let prog = Parser::parse_program(src).expect("parse");
    let Item::Class(c) = &prog.items[0].node else {
        panic!("not class")
    };
    let ctor = &c.constructors[0].node;
    assert_eq!(ctor.params.len(), 2);
    assert!(ctor.params[0].default.is_none());
    assert!(
        ctor.params[1].default.is_some(),
        "ctor b default missing: {:?}",
        ctor.params[1]
    );
}

#[test]
fn optional_lambda_param_parse() {
    let src = r#"
class C {
    public int Run() {
        return ((a: int, b: int = 10) => a + b)(1);
    }
}
"#;
    let prog = Parser::parse_program(src).expect("parse");
    let Item::Class(c) = &prog.items[0].node else {
        panic!("not class")
    };
    let body = &c.methods[0].node.body.as_ref().expect("body").stmts;
    let Stmt::Return(Some(ret)) = &body[0].node else {
        panic!("expected return, got {:?}", body[0].node);
    };
    let Expr::Call { func, .. } = &ret.node else {
        panic!("expected call");
    };
    let Expr::Lambda(l) = &func.node else {
        panic!("expected lambda callee");
    };
    assert_eq!(l.params.len(), 2);
    assert!(l.params[0].default.is_none());
    assert!(
        l.params[1].default.is_some(),
        "lambda b default missing: {:?}",
        l.params[1]
    );
}
