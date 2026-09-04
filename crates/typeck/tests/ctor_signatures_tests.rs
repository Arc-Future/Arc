//! RFC 035 M0：TypeRegistry::ctor_signatures API 单元测试。
//!
//! 验证构造函数签名查询的正确性，为 M1 codegen 工厂生成铺路。

use hir::HirBuilder;
use parse::Parser;
use typeck::TypeRegistry;

#[test]
fn ctor_signatures_empty_for_unknown_type() {
    let src = r#"
class Foo {
    public Foo() {}
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    assert!(reg.ctor_signatures(&"UnknownType".into()).is_empty());
}

#[test]
fn ctor_signatures_for_class_with_no_args_ctor() {
    let src = r#"
class Foo {
    public Foo() {}
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let ctors = reg.ctor_signatures(&"Foo".into());
    assert_eq!(ctors.len(), 1, "class Foo should have 1 constructor");
    assert!(
        ctors[0].param_types.is_empty(),
        "no-arg constructor should have empty param_types"
    );
}

#[test]
fn ctor_signatures_for_class_with_params_ctor() {
    let src = r#"
class Bar {
    public Bar(int x, string y) {}
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let ctors = reg.ctor_signatures(&"Bar".into());
    assert_eq!(ctors.len(), 1, "class Bar should have 1 constructor");
    assert_eq!(
        ctors[0].param_types.len(),
        2,
        "Bar(int, string) should have 2 params"
    );
    assert_eq!(ctors[0].param_types[0], "int");
    assert_eq!(ctors[0].param_types[1], "string");
}

#[test]
fn ctor_signatures_for_class_with_multiple_ctors() {
    let src = r#"
class Widget {
    public int X { get; }
    public Widget() {
        X = 0;
    }
    public Widget(int x) {
        X = x;
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let ctors = reg.ctor_signatures(&"Widget".into());
    assert_eq!(ctors.len(), 2, "class Widget should have 2 constructors");
    // 无参构造
    assert!(ctors[0].param_types.is_empty());
    // 单参构造
    assert_eq!(ctors[1].param_types.len(), 1);
    assert_eq!(ctors[1].param_types[0], "int");
}

#[test]
fn ctor_signatures_empty_for_class_without_explicit_ctor() {
    let src = r#"
class Empty {
    public int x;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    // class 无显式构造函数时，constructors 为空（Arc 不自动合成默认构造函数）
    assert!(
        reg.ctor_signatures(&"Empty".into()).is_empty(),
        "class without explicit ctor should have empty constructors"
    );
}
