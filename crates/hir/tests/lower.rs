use hir::HirBuilder;
use parse::Parser;

#[test]
fn lower_hello() {
    let src = "void Main() { }";
    let program = Parser::parse_program(src).unwrap();
    let mut builder = HirBuilder::new();
    let hir = builder.lower_program(&program).unwrap();
    assert_eq!(hir.items.len(), 1);
}

#[test]
fn lower_using_import() {
    let src = r#"
using Arc;
class Console { public static void WriteLine(string message) { } }
void Main() { }
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut builder = HirBuilder::new();
    let hir = builder.lower_program(&program).unwrap();
    assert_eq!(hir.imports.len(), 1);
    assert_eq!(hir.imports[0].alias.as_str(), "Arc");
    assert!(hir.resolve_name(&"Console".into()).is_some());
}
