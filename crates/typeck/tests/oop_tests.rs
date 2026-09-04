use hir::HirBuilder;
use parse::Parser;
use typeck::*;

#[test]
fn interface_impl_ok() {
    let src = r#"
interface IGreet {
    string Greet();
}
class Greeter : IGreet {
    public string Greet() {
        return "hi";
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    assert!(reg
        .check_interface_impl(&"Greeter".into(), &"IGreet".into())
        .is_ok());
}

#[test]
fn interface_impl_missing_method() {
    let src = r#"
interface IGreet {
    string Greet();
}
class Bad : IGreet {
    string name;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    assert!(reg
        .check_interface_impl(&"Bad".into(), &"IGreet".into())
        .is_err());
}

#[test]
fn extension_method_registered() {
    let src = r#"
interface IShape { string Name(); }
static class ShapeExtensions {
    public static void Describe(this IShape shape) { }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let scope = AccessContext {
        current_type: None,
        extension_scope: ExtensionScope {
            imported: vec![],
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    let ext = reg
        .resolve_extension(&"IShape".into(), &"Describe".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .expect("extension method");
    assert_eq!(ext.container, "ShapeExtensions");
    assert_eq!(ext.sig.name, "Describe");
    assert!(ext.sig.params.is_empty());
}

#[test]
fn extension_hidden_without_using() {
    let src = r#"
namespace App.Extensions;
interface IShape { string Name(); }
static class ShapeExtensions {
    public static void Describe(this IShape shape) { }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let scope = AccessContext {
        current_type: None,
        extension_scope: ExtensionScope {
            imported: vec![],
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    assert!(reg
        .resolve_extension(&"IShape".into(), &"Describe".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .is_none());
}

#[test]
fn extension_visible_with_namespace_using() {
    let src = r#"
namespace App.Extensions;
interface IShape { string Name(); }
static class ShapeExtensions {
    public static void Describe(this IShape shape) { }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let imported = reg.resolve_extension_imports(&[hir::ImportBinding {
        path: vec!["App".into(), "Extensions".into()],
        alias: "Extensions".into(),
        kind: hir::ImportKind::Namespace,
    }]);
    let scope = AccessContext {
        current_type: None,
        extension_scope: ExtensionScope {
            imported,
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    assert!(reg
        .resolve_extension(&"IShape".into(), &"Describe".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .is_some());
}

#[test]
fn extension_visible_in_same_namespace() {
    let src = r#"
namespace App.Extensions;
interface IShape { string Name(); }
static class ShapeExtensions {
    public static void Describe(this IShape shape) { }
}
void Demo(IShape s) { s.Describe(); }
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let scope = AccessContext {
        current_type: None,
        extension_scope: ExtensionScope {
            imported: vec![],
            enclosing: vec!["App".into(), "Extensions".into()],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    assert!(reg
        .resolve_extension(&"IShape".into(), &"Describe".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .is_some());
}

#[test]
fn namespace_matches_import_short_and_prefix() {
    use typeck::namespace_matches_import;
    let ext_ns = vec!["ObjectModel".into(), "Extensions".into()];
    assert!(namespace_matches_import(&["Extensions".into()], &ext_ns));
    assert!(namespace_matches_import(
        &["ObjectModel".into(), "Extensions".into()],
        &ext_ns
    ));
    assert!(!namespace_matches_import(&["Shapes".into()], &ext_ns));
}

#[test]
fn lsp_override_return_mismatch() {
    let src = r#"
class Base {
    virtual int Value() { return 0; }
}
class Derived : Base {
    override string Value() { return "x"; }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let errs = reg.validate_all().unwrap_err();
    assert!(errs
        .iter()
        .any(|e| matches!(e, OopError::LspViolation { .. })));
}

fn object_model_merged_program() -> ast::Program {
    // examples/ObjectModel 已清理；内联最小扁平源码保持布局 / 虚函数槽回归。
    let src = r#"
using Arc;
public interface IShape {
    int Area();
    string Name { get; }
}
public class Rectangle : IShape {
    public int Width;
    private int Height;
    public string Name;
    public Rectangle(int width, int height) {
        Width = width;
        Height = height;
        Name = "rectangle";
    }
    public virtual int Area() { return Width * Height; }
}
public class Square : Rectangle {
    public Square(int side) : base(side, side) { }
    public override int Area() { return Width * Width; }
}
void Main() {
    var rectangle = new Rectangle(10, 20);
}
"#;
    Parser::parse_program(src).unwrap()
}

#[test]
fn square_overrides_area_slot() {
    let program = object_model_merged_program();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let layouts = layouts_from_registry(&reg);
    let square = layouts.get("Square").unwrap();
    assert_eq!(
        square
            .method_impl
            .get(&("Area".into(), vec![]))
            .map(|s| s.as_str()),
        Some("Square")
    );
    let ishape = layouts.interfaces.get("IShape").unwrap();
    assert!(
        ishape
            .properties
            .iter()
            .any(|(n, t)| n == "Name" && t == "string"),
        "IShape should declare Name property, got {:?}",
        ishape.properties
    );
    let rect = layouts.get("Rectangle").unwrap();
    assert!(
        rect.fields.iter().any(|f| f.name == "Name"),
        "Rectangle should have Name field implementing IShape.Name"
    );
}

// CD-18/G2：具体类未实现继承链上抽象方法 → `validate_all` 报错。
#[test]
fn abstract_required_missing_inherited_impl_errors() {
    let src = r#"
abstract class AbsBase {
    public abstract int Compute();
}
class BadImpl : AbsBase {
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let errs = reg.validate_all().unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(
            e,
            OopError::AbstractInConcreteClass {
                class,
                method
            } if class == "BadImpl" && method == "Compute"
        )),
        "expected AbstractInConcreteClass for BadImpl.Compute, got: {errs:?}"
    );
}

// CD-18/G2：抽象类可不实现继承的抽象方法（继续抽象）；具体类在多级链末端实现 → 合法。
#[test]
fn abstract_required_abstract_class_may_skip_and_concrete_impl_ok() {
    let src = r#"
abstract class Root {
    public abstract int Value();
}
abstract class Mid : Root {
    public abstract string Label();
}
class Leaf : Mid {
    public override int Value() {
        return 1;
    }
    public override string Label() {
        return "leaf";
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    assert!(reg.validate_all().is_ok(), "expected no errors");
}

// RFC 024 M7（P12）：`new BlockingCollection<T>(collection, capacity)` 的第一实参
// 必须是 ConcurrentQueue/Bag/Stack 三种具体集合——违规在 typeck 报 TypeError::Oop
// 诊断（原为 codegen ICE panic，用户不可读）。
#[test]
fn blocking_collection_ctor_rejects_non_concurrent_first_arg() {
    let src = r#"
class C {
    public void Bad() {
        List<int> l = new List<int>();
        BlockingCollection<int> bc = new BlockingCollection<int>(l, 0);
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let errs = tc.check_module(&module).unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(
            e,
            TypeError::Oop(msg)
                if msg.contains("BlockingCollection")
                    && msg.contains("ConcurrentQueue")
                    && msg.contains("`List`")
        )),
        "expected BlockingCollection first-arg diagnostic, got: {errs:?}"
    );
}

#[test]
fn blocking_collection_ctor_accepts_concurrent_queue_first_arg() {
    let src = r#"
class C {
    public void Ok() {
        ConcurrentQueue<int> q = new ConcurrentQueue<int>();
        BlockingCollection<int> bc = new BlockingCollection<int>(q, 0);
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_ok(),
        "legal ConcurrentQueue first argument must typecheck, got: {:?}",
        result.err()
    );
}

// CD-18/G2：`override abstract` 再声明由更下层具体类实现 → 不重复报错、合法。
#[test]
fn abstract_required_override_abstract_chain_ok() {
    let src = r#"
abstract class A {
    public abstract int Compute();
}
abstract class B : A {
    public override abstract int Compute();
}
class C : B {
    public override int Compute() {
        return 7;
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    assert!(reg.validate_all().is_ok(), "expected no errors");
}

// CD-18/G2：具体类漏实现多级 `override abstract` → 报错（`BadImpl : B` 未实现）。
#[test]
fn abstract_required_override_abstract_missing_errors() {
    let src = r#"
abstract class A {
    public abstract int Compute();
}
abstract class B : A {
    public override abstract int Compute();
}
class BadImpl : B {
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    let errs = reg.validate_all().unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(
            e,
            OopError::AbstractInConcreteClass { class, .. } if class == "BadImpl"
        )),
        "expected AbstractInConcreteClass for BadImpl, got: {errs:?}"
    );
}

// CD-18/G2：抽象属性可由自动属性 override（public 字段）满足——与接口
// `is_satisfied_by_public_field` 语义一致，不误报。
#[test]
fn abstract_required_auto_property_override_satisfies_abstract_property() {
    let src = r#"
abstract class PropBase {
    public abstract string Tag { get; }
    public abstract int Count();
}
class PropImpl : PropBase {
    public override string Tag { get; }
    public override int Count() {
        return 3;
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let reg = TypeRegistry::from_module(&module);
    assert!(reg.validate_all().is_ok(), "expected no errors");
}
