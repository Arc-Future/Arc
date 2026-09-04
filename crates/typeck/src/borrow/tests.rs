use super::*;
use hir::HirBuilder;
use parse::Parser;

use crate::TypeChecker;

fn check_source(src: &str) -> Result<(), Vec<BorrowError>> {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    let mut bc = BorrowChecker::new();
    bc.check_module(&module, &fns)
}

#[test]
fn struct_move_ok() {
    let src = r#"
struct Point { public int X; public int Y; }
void take(Point p) { }
void Main() {
    var a = new Point() { X = 1, Y = 2 };
    var b = a;
    take(b);
}
"#;
    assert!(check_source(src).is_ok());
}

#[test]
fn struct_use_after_move_fails() {
    let src = r#"
class Payload { public int V; }
struct Holder { public Payload Data; }
void Main() {
    var a = new Holder() { Data = new Payload() { V = 1 } };
    var b = a;
    var c = a;
}
"#;
    let err = check_source(src).unwrap_err();
    assert!(err
        .iter()
        .any(|e| matches!(e, BorrowError::UseAfterMove(_))));
}

#[test]
fn pure_value_struct_copy_ok() {
    let src = r#"
struct Point { public int X; public int Y; }
void Main() {
    var a = new Point() { X = 1, Y = 2 };
    var b = a;
    var c = a;
}
"#;
    assert!(check_source(src).is_ok());
}

#[test]
fn string_field_struct_copy_ok() {
    let src = r#"
struct Named { public string Name; public int Id; }
void Main() {
    var a = new Named() { Name = "x", Id = 1 };
    var b = a;
    var c = a;
}
"#;
    assert!(check_source(src).is_ok());
}

#[test]
fn copy_struct_param_then_reuse_ok() {
    // RFC 005：Copy 型 struct 传参不 move——被调方拿到的是私有副本，
    // 调用方局部仍可用（C# 值语义）。
    let src = r#"
struct Point { public int X; public int Y; }
void take(Point p) { }
void Main() {
    var a = new Point() { X = 1, Y = 2 };
    take(a);
    var b = a;
    take(b);
}
"#;
    assert!(check_source(src).is_ok());
}

#[test]
fn null_coalesce_assign_ok() {
    // `??=` 脱糖为 Assign(target, Coalesce(target, rhs))——typeck 走既有
    // Coalesce 检查路径，`string?` 左值 + string 右值应通过。
    let src = r#"
void Main() {
    string? a = null;
    a ??= "fallback";
}
"#;
    assert!(check_source(src).is_ok());
}

#[test]
fn class_share_ok() {
    let src = r#"
class Box { public int Value; public Box(int v) { Value = v; } }
void share(Box b) { }
void Main() {
    var a = new Box(1);
    var b = a;
    share(a);
    share(b);
}
"#;
    assert!(check_source(src).is_ok());
}
