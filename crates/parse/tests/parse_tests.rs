use ast::{
    BinOp, CollectionElement, DeconstructTarget, Expr, IsPattern, Item, LambdaBody, MethodModifier,
    Pattern, PositionalSubpattern, Stmt, Type, Visibility,
};
use parse::Parser;

#[test]
fn parse_hello_fn() {
    let src = r#"
void Main() {
    Console.WriteLine("Hello, World!");
}
"#;
    let program = Parser::parse_program(src).unwrap();
    assert_eq!(program.items.len(), 1);
}

/// RFC 006 D3 / RFC 009 B2：顶层类型无修饰符默认 `internal`（成员仍默认 private）。
#[test]
fn parse_top_level_type_defaults_to_internal() {
    let src = r#"
class Unmarked {}
struct UnmarkedStruct {}
interface IUnmarked {}
enum UnmarkedEnum { A }
variant UnmarkedVariant { | Case }
"#;
    let program = Parser::parse_program(src).unwrap();
    assert_eq!(program.items.len(), 5);
    match &program.items[0].node {
        Item::Class(c) => assert_eq!(c.vis, Visibility::Internal),
        other => panic!("expected class, got {other:?}"),
    }
    match &program.items[1].node {
        Item::Struct(s) => assert_eq!(s.vis, Visibility::Internal),
        other => panic!("expected struct, got {other:?}"),
    }
    match &program.items[2].node {
        Item::Interface(i) => assert_eq!(i.vis, Visibility::Internal),
        other => panic!("expected interface, got {other:?}"),
    }
    match &program.items[3].node {
        Item::Enum(e) => assert_eq!(e.vis, Visibility::Internal),
        other => panic!("expected enum, got {other:?}"),
    }
    match &program.items[4].node {
        Item::Variant(v) => assert_eq!(v.vis, Visibility::Internal),
        other => panic!("expected variant, got {other:?}"),
    }
}

#[test]
fn parse_top_level_explicit_public_and_member_default_private() {
    let src = r#"
public class Box {
    int value;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(class.vis, Visibility::Public);
    assert_eq!(class.fields[0].vis, Visibility::Private);
}

#[test]
fn parse_query_expr() {
    let src = r#"
void demo() {
    var q = from u in users where u.Age >= 18 select u.Name;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let fn_item = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let init = match &fn_item.body.as_ref().unwrap().stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    assert!(matches!(init.node, Expr::Query(_)));
}

#[test]
fn parse_dotted_namespace_block() {
    let src = r#"
namespace myapp.io {
    public class Console { }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let ns = match &program.items[0].node {
        Item::Namespace(n) => n,
        _ => panic!("expected namespace"),
    };
    assert_eq!(ns.path, ["myapp", "io"]);
    assert_eq!(ns.items.len(), 1);
}

#[test]
fn parse_file_scoped_namespace() {
    let src = r#"
namespace myapp.io;

public class Console { }
"#;
    let program = Parser::parse_program(src).unwrap();
    let ns = match &program.items[0].node {
        Item::Namespace(n) => n,
        _ => panic!("expected namespace"),
    };
    assert_eq!(ns.path, ["myapp", "io"]);
    assert_eq!(ns.items.len(), 1);
}

#[test]
fn reject_transitional_namespace() {
    let src = "namespace X; { }";
    assert!(Parser::parse_program(src).is_err());
}

#[test]
fn parse_namespace_capability_single() {
    // RFC 027 M3 §3.4 能力 gating Phase 1+：namespace 单能力声明
    let src = r#"
namespace myapp.io capability io {
    public class File { }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let ns = match &program.items[0].node {
        Item::Namespace(n) => n,
        _ => panic!("expected namespace"),
    };
    assert_eq!(ns.path, ["myapp", "io"]);
    assert_eq!(ns.capabilities, ["io"]);
    assert_eq!(ns.items.len(), 1);
}

#[test]
fn parse_namespace_capability_multiple() {
    // RFC 027 M3 §3.4 能力 gating Phase 1+：namespace 多能力声明（逗号分隔）
    let src = r#"
namespace app capability io, db, net {
    public class Foo { }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let ns = match &program.items[0].node {
        Item::Namespace(n) => n,
        _ => panic!("expected namespace"),
    };
    assert_eq!(ns.path, ["app"]);
    assert_eq!(ns.capabilities, ["io", "db", "net"]);
}

#[test]
fn parse_namespace_capability_file_scoped() {
    // RFC 027 M3 §3.4 能力 gating Phase 1+：file-scoped namespace + capability
    let src = r#"
namespace myapp.io capability io;

public class File { }
"#;
    let program = Parser::parse_program(src).unwrap();
    let ns = match &program.items[0].node {
        Item::Namespace(n) => n,
        _ => panic!("expected namespace"),
    };
    assert_eq!(ns.path, ["myapp", "io"]);
    assert_eq!(ns.capabilities, ["io"]);
    assert_eq!(ns.items.len(), 1);
}

#[test]
fn parse_namespace_no_capability_defaults_empty() {
    // 兼容性：无 capability 子句的 namespace，capabilities 为空 Vec
    let src = r#"
namespace plain {
    public class Foo { }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let ns = match &program.items[0].node {
        Item::Namespace(n) => n,
        _ => panic!("expected namespace"),
    };
    assert!(ns.capabilities.is_empty());
}

#[test]
fn reject_var_bare_brace_array_init() {
    let src = r#"void Main() { var v = { 10, 20 }; }"#;
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("leading-type") || err.to_string().contains("unexpected"),
        "unexpected error: {err}"
    );
}

#[test]
fn parse_var_collection_expr() {
    let src = r#"
void Main() {
    int[] v = [10, 20];
    int x = v[0];
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let fn_item = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let init = match &fn_item.body.as_ref().unwrap().stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    assert!(matches!(init.node, Expr::CollectionExpr { .. }));
}

#[test]
fn parse_leading_type_local() {
    let src = r#"
void Main() {
    string greeting = "hi";
    int[] nums = [1, 2];
    int mid = nums[0];
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let fn_item = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    assert_eq!(fn_item.body.as_ref().unwrap().stmts.len(), 3);
}

#[test]
fn parse_struct_array_leading_type() {
    let src = r#"
struct User {
    public int Age;
    public string Name;
}

void Main() {
    User[] users = [
        new User() { Age = 25, Name = "alice" },
        new User() { Age = 15, Name = "bob" }
    ];
}
"#;
    Parser::parse_program(src).unwrap();
}

#[test]
fn parse_method_call() {
    let src = r#"void demo(IShape s) { s.Name(); }"#;
    let program = Parser::parse_program(src).unwrap();
    let fn_item = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let stmt = match &fn_item.body.as_ref().unwrap().stmts[0].node {
        Stmt::Expr(e) => e,
        _ => panic!("expected expr stmt"),
    };
    assert!(matches!(
        stmt.node,
        Expr::MethodCall { ref method, .. } if method == "Name"
    ));
}

#[test]
fn parse_collection_expr() {
    let src = r#"
void Main() {
    int[] a = [10, 20];
    int[] b = [1, 2];
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let fn_item = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let a_init = match &fn_item.body.as_ref().unwrap().stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    assert!(matches!(a_init.node, Expr::CollectionExpr { .. }));
    let b_init = match &fn_item.body.as_ref().unwrap().stmts[1].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    assert!(matches!(b_init.node, Expr::CollectionExpr { .. }));
}

#[test]
fn parse_nested_array_type_and_collection() {
    let src = r#"
void Main() {
    int[][] nested = [[1, 2], [3, 4]];
    int[][][] deep = [[[10]], [[20], [30]]];
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let fn_item = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let stmts = &fn_item.body.as_ref().unwrap().stmts;
    let (ty0, init0) = match &stmts[0].node {
        Stmt::Let {
            ty: Some(t),
            init: Some(e),
            ..
        } => (t, e),
        _ => panic!("expected let with type"),
    };
    assert!(matches!(
        &ty0.node,
        Type::Array {
            inner
        } if matches!(inner.node, Type::Array { .. })
    ));
    match &init0.node {
        Expr::CollectionExpr { elements } => {
            assert_eq!(elements.len(), 2);
            assert!(matches!(
                &elements[0],
                CollectionElement::Element(e) if matches!(e.node, Expr::CollectionExpr { .. })
            ));
        }
        other => panic!("expected CollectionExpr, got {other:?}"),
    }
    let ty1 = match &stmts[1].node {
        Stmt::Let { ty: Some(t), .. } => t,
        _ => panic!("expected deep let"),
    };
    assert!(matches!(
        &ty1.node,
        Type::Array { inner: a } if matches!(
            &a.node,
            Type::Array { inner: b } if matches!(b.node, Type::Array { .. })
        )
    ));
}

#[test]
fn reject_new_array_syntax() {
    let src = r#"void Main() { int[] x = new int[] { 1, 2 }; }"#;
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("collection expression"),
        "unexpected error: {err}"
    );
}

#[test]
fn reject_struct_literal_without_new() {
    let src = r#"void Main() { var p = Point { X = 1, Y = 2 }; }"#;
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("new Point()"),
        "unexpected error: {err}"
    );
}

#[test]
fn parse_new_object_initializer() {
    let src = r#"
struct Point { public int X; public int Y; }
void Main() {
    var p = new Point() { X = 1, Y = 2 };
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let fn_item = match &program.items[1].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let init = match &fn_item.body.as_ref().unwrap().stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    assert!(matches!(
        init.node,
        Expr::New {
            obj_init: Some(_),
            ..
        }
    ));
}

#[test]
fn parse_target_typed_new() {
    let src = r#"
class Point {
    public int X;
    public Point(int x) { X = x; }
}
void Main() {
    Point p = new(42);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let fn_item = match &program.items[1].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let init = match &fn_item.body.as_ref().unwrap().stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    match &init.node {
        Expr::New { ty, args, obj_init } => {
            assert!(
                matches!(ty.node, Type::Infer),
                "expected Infer ty, got {:?}",
                ty.node
            );
            assert_eq!(args.len(), 1);
            assert!(obj_init.is_none());
        }
        other => panic!("expected New, got {other:?}"),
    }
}

#[test]
fn parse_static_class_extension_method() {
    let src = r#"
static class ShapeExtensions {
    public static void Describe(this IShape shape) { }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert!(class.is_static);
    let method = &class.methods[0].node;
    assert_eq!(method.sig.modifier, ast::MethodModifier::Static);
    assert!(method.sig.params[0].is_extension_receiver);
}

#[test]
fn reject_var_with_type_suffix() {
    let src = r#"void Main() { var x: int = 1; }"#;
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("leading-type"),
        "unexpected error: {err}"
    );
}

#[test]
fn reject_tuple_type_syntax() {
    let src = r#"void f((int, string) x) {}"#;
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("tuple types not supported"),
        "unexpected error: {err}"
    );
}

#[test]
fn parse_class_attribute() {
    let src = r#"
[Obsolete("use v2")]
class QueryProvider { }
"#;
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(class.attributes.len(), 1);
    assert_eq!(class.attributes[0].path, ["Obsolete"]);
    assert_eq!(
        class.attributes[0].args,
        vec![ast::AttributeArg::String("use v2".into())]
    );
}

#[test]
fn parse_method_attribute() {
    let src = r#"
class Provider {
    [Obsolete("use v2")]
    public string Translate<T>(Expression<T> expr) {
        return "";
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    let method = &class.methods[0].node;
    assert_eq!(method.sig.attributes.len(), 1);
    assert_eq!(method.sig.attributes[0].path, ["Obsolete"]);
}

#[test]
fn parse_fn_attribute() {
    let src = r#"
[Obsolete("unused")]
void Main() { }
"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    assert_eq!(f.attributes.len(), 1);
    assert_eq!(f.attributes[0].path, ["Obsolete"]);
}

/// P1 (RFC 017): `///` 文档注释应进入顶层项的 `doc` 字段。
/// 普通单行注释 `//` 不应进入 AST；成员级 `///` 暂不提取（保持 `doc: None`）。
#[test]
fn parse_doc_comment_on_top_level_items() {
    // 单行 doc 注释 + 普通 // 注释混排：doc 提取，// 跳过。
    let src = "\
namespace Arc;
// 普通注释不应进入 AST
/// <summary>测试类</summary>
public class Foo { }
";
    let program = Parser::parse_program(src).unwrap();
    let ns = match &program.items[0].node {
        Item::Namespace(n) => n,
        _ => panic!("expected namespace"),
    };
    let class = match &ns.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(
        class.doc.as_deref(),
        Some("<summary>测试类</summary>"),
        "top-level class doc should be extracted"
    );

    // 多行 /// 注释应以 \n 拼接。
    let src2 = "\
namespace Arc;
/// 第一行。
/// 第二行。
void Bar() { }
";
    let program2 = Parser::parse_program(src2).unwrap();
    let ns2 = match &program2.items[0].node {
        Item::Namespace(n) => n,
        _ => panic!("expected namespace"),
    };
    let f = match &ns2.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    assert_eq!(
        f.doc.as_deref(),
        Some("第一行。\n第二行。"),
        "multi-line doc should be joined with \\n"
    );

    // 顶层项无 doc 注释时 doc == None。
    let src3 = "void Baz() { }";
    let program3 = Parser::parse_program(src3).unwrap();
    let f3 = match &program3.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    assert!(
        f3.doc.is_none(),
        "fn without doc comment should have doc == None"
    );
}

/// P1d (RFC 017)：成员级 `///` 注释应进入成员 `doc` 字段。
/// 覆盖 class 的 field/method/property、interface method、struct field。
#[test]
fn parse_sorted_dict_indexer_with_where() {
    let src = "\
namespace Arc.Collections;
public class SortedDictionary<K, V>
    where K : IComparable<K> {
    public V this[K key] {
        get { return Get(key); }
        set { Set(key, value); }
    }
    public V Get(K key) { return 0; }
    public void Set(K key, V value) { }
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        Item::Namespace(ns) => match &ns.items[0].node {
            Item::Class(c) => c,
            other => panic!("expected class in namespace, got {other:?}"),
        },
        other => panic!("expected class/namespace, got {other:?}"),
    };
    assert!(class.properties[0].is_indexer());
    assert_eq!(class.properties[0].index_params[0].name.as_str(), "key");
}

#[test]
fn parse_indexer_this_brackets() {
    // RFC 060：类/接口 `this[]` 索引器声明。
    let src = "\
class Bag {
    public int this[int i] {
        get { return 0; }
        set { }
    }
}
interface IBag {
    int this[int i] { get; set; }
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(class.properties.len(), 1);
    let p = &class.properties[0];
    assert!(p.is_indexer());
    assert_eq!(p.name.as_str(), "Item");
    assert_eq!(p.index_params.len(), 1);
    assert_eq!(p.index_params[0].name.as_str(), "i");
    assert!(p.has_get && p.has_set);
    assert!(p.get_body.is_some() && p.set_body.is_some());

    let iface = match &program.items[1].node {
        Item::Interface(i) => i,
        _ => panic!("expected interface"),
    };
    assert_eq!(iface.properties.len(), 1);
    let ip = &iface.properties[0];
    assert!(ip.is_indexer());
    assert_eq!(ip.name.as_str(), "Item");
    assert!(ip.has_get && ip.has_set);
    assert!(ip.get_body.is_none() && ip.set_body.is_none());
}

#[test]
fn parse_init_accessor_auto_property() {
    // RFC 069 M1：`{ get; init; }` → has_get && has_init && !has_set
    let src = "\
class Person {
    public string Name { get; init; }
    public int Age { init; get; }
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(class.properties.len(), 2);
    let name = &class.properties[0];
    assert!(name.has_get && name.has_init && !name.has_set);
    assert!(!name.is_required);
    assert!(name.get_body.is_none() && name.set_body.is_none());
    let age = &class.properties[1];
    assert!(age.has_get && age.has_init && !age.has_set);
}

#[test]
fn parse_required_property_m3() {
    let src = "\
class Person {
    public required string Name { get; init; }
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert!(class.properties[0].is_required);
    assert!(class.properties[0].has_init);
}

#[test]
fn parse_property_initializer() {
    // 属性初值（C# `T Prop { get; } = expr;`）：解析 `= expr;` 存入 init，
    // 与表达式体 `=> expr`（get_body）区分。
    let src = "\
class FileResult {
    public string Body { get; } = \"\";
    public int StatusCode { get; } = 200;
    public bool IsBinary { get; } = true;
    public string Name { get; set; } = \"na\";
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(class.properties.len(), 4);
    for p in &class.properties {
        assert!(p.init.is_some(), "property `{}` should carry init", p.name);
        assert!(p.get_body.is_none(), "initializer is not `=>` desugar");
        assert!(p.set_body.is_none(), "auto-property has no accessor body");
    }
    assert_eq!(class.properties[1].name.as_str(), "StatusCode");
}

#[test]
fn parse_property_initializer_rejects_custom_accessor() {
    // 有访问器体（custom）的属性带 `= expr` 属文法误用，应报错。
    let src = "\
class Bad {
    public int X { get { return 1; } } = 1;
}
";
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("initializer"),
        "expected initializer rejection, got {err:?}"
    );
}

#[test]
fn parse_init_accessor_custom_body_m2() {
    // RFC 069 M2：`init { … }` → has_init + set_body
    let src = "\
class Counter {
    int _n;
    public int N {
        get { return _n; }
        init { _n = value; }
    }
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    let n = &class.properties[0];
    assert!(n.has_get && n.has_init && !n.has_set);
    assert!(n.get_body.is_some());
    assert!(n.set_body.is_some());
}

#[test]
fn parse_init_with_set_rejected() {
    let src = "\
class Bad {
    public int X { get; set; init; }
}
";
    let err = Parser::parse_program(src).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("set") && msg.contains("init"),
        "expected set/init conflict, got {msg}"
    );
}

#[test]
fn parse_expression_bodied_property_and_accessors() {
    // C# 表达式体属性 / 访问器：脱糖为 get_body / set_body 块。
    let src = "\
class Box {
    private int _value;
    public int Value => _value;
    public int Wrapped {
        get => _value;
        set => _value = value;
    }
    public int Doubled() => Value * 2;
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(class.properties.len(), 2);
    let value = &class.properties[0];
    assert_eq!(value.name.as_str(), "Value");
    assert!(value.has_get && !value.has_set);
    assert!(value.get_body.is_some());
    assert!(value.set_body.is_none());
    let get_stmts = &value.get_body.as_ref().unwrap().stmts;
    assert!(matches!(get_stmts[0].node, Stmt::Return(Some(_))));

    let wrapped = &class.properties[1];
    assert!(wrapped.has_get && wrapped.has_set);
    assert!(wrapped.get_body.is_some() && wrapped.set_body.is_some());
    assert!(matches!(
        wrapped.get_body.as_ref().unwrap().stmts[0].node,
        Stmt::Return(Some(_))
    ));
    assert!(matches!(
        wrapped.set_body.as_ref().unwrap().stmts[0].node,
        Stmt::Assign { .. }
    ));

    assert_eq!(class.methods.len(), 1);
    let doubled = &class.methods[0].node;
    assert_eq!(doubled.sig.name.as_str(), "Doubled");
    let body = doubled
        .body
        .as_ref()
        .expect("expression-bodied method body");
    assert!(matches!(body.stmts[0].node, Stmt::Return(Some(_))));
}

#[test]
fn parse_expression_bodied_void_method() {
    let src = "\
class Logger {
    public void Ping() => Console.WriteLine(\"ping\");
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    let body = class.methods[0].node.body.as_ref().unwrap();
    assert!(matches!(body.stmts[0].node, Stmt::Expr(_)));
}

#[test]
fn parse_doc_comment_on_members_extracted() {
    // class 成员：field / method / property 均提取 doc。
    let src = "\
class Foo {
    /// <summary>字段 X 文档。</summary>
    public int X;
    /// <summary>方法 M 文档。</summary>
    public void M() { }
    /// <summary>属性 Y 文档。</summary>
    public int Y { get; set; }
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    // 顶层 class 无 doc 注释。
    assert!(class.doc.is_none());
    // field doc
    assert_eq!(
        class.fields[0].doc.as_deref(),
        Some("<summary>字段 X 文档。</summary>"),
        "field doc should be extracted (P1d)"
    );
    // method doc（MethodDef.doc）
    assert_eq!(
        class.methods[0].node.doc.as_deref(),
        Some("<summary>方法 M 文档。</summary>"),
        "method doc should be extracted (P1d)"
    );
    // property doc
    assert_eq!(
        class.properties[0].doc.as_deref(),
        Some("<summary>属性 Y 文档。</summary>"),
        "property doc should be extracted (P1d)"
    );

    // interface 方法 doc 置于 sig.doc。
    let src2 = "\
interface I {
    /// <summary>契约方法文档。</summary>
    void N();
}
";
    let program2 = Parser::parse_program(src2).unwrap();
    let i = match &program2.items[0].node {
        Item::Interface(i) => i,
        _ => panic!("expected interface"),
    };
    assert_eq!(
        i.methods[0].doc.as_deref(),
        Some("<summary>契约方法文档。</summary>"),
        "interface method doc (sig.doc) should be extracted (P1d)"
    );

    // struct field doc。
    let src3 = "\
struct S {
    /// <summary>结构字段文档。</summary>
    public int A;
}
";
    let program3 = Parser::parse_program(src3).unwrap();
    let s = match &program3.items[0].node {
        Item::Struct(s) => s,
        _ => panic!("expected struct"),
    };
    assert_eq!(
        s.fields[0].doc.as_deref(),
        Some("<summary>结构字段文档。</summary>"),
        "struct field doc should be extracted (P1d)"
    );

    // 多行成员 doc 以 \n 拼接。
    let src4 = "\
class C {
    /// 第一行。
    /// 第二行。
    public int Z;
}
";
    let program4 = Parser::parse_program(src4).unwrap();
    let c4 = match &program4.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(
        c4.fields[0].doc.as_deref(),
        Some("第一行。\n第二行。"),
        "multi-line member doc should be joined with \\n"
    );
}

/// struct / interface / enum 顶层项也应提取 doc。
#[test]
fn parse_doc_comment_on_other_top_level_items() {
    let src = "\
/// 结构文档。
struct S { public int A; }
/// 接口文档。
interface I { void M(); }
/// 枚举文档。
enum E { A, B }
";
    let program = Parser::parse_program(src).unwrap();
    let s = match &program.items[0].node {
        Item::Struct(s) => s,
        _ => panic!("expected struct"),
    };
    assert_eq!(s.doc.as_deref(), Some("结构文档。"));

    let i = match &program.items[1].node {
        Item::Interface(i) => i,
        _ => panic!("expected interface"),
    };
    assert_eq!(i.doc.as_deref(), Some("接口文档。"));

    let e = match &program.items[2].node {
        Item::Enum(e) => e,
        _ => panic!("expected enum"),
    };
    assert_eq!(e.doc.as_deref(), Some("枚举文档。"));
}

// ── 三元条件表达式 `cond ? then : else` 解析测试 ──

#[test]
fn parse_ternary_basic_int() {
    let src = r#"void Main() {
    int a = true ? 10 : 20;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let init = match &body.stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let with init"),
    };
    match &init.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(cond.node, Expr::BoolLit(true)));
            assert!(matches!(then_branch.node, Expr::IntLit(10)));
            assert!(matches!(else_branch.node, Expr::IntLit(20)));
        }
        other => panic!("expected Ternary, got {other:?}"),
    }
}

#[test]
fn parse_ternary_comparison_condition() {
    // 验证 `>` 比 `?:` 优先级更高——无需括号
    let src = r#"void Main() {
    int a = 5 > 3 ? 10 : 20;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let init = match &body.stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let with init"),
    };
    match &init.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            // cond 应该是 Binary(5, Gt, 3)，而不是 5
            match &cond.node {
                Expr::Binary { op, left, right } => {
                    assert_eq!(*op, ast::BinOp::Gt);
                    assert!(matches!(left.node, Expr::IntLit(5)));
                    assert!(matches!(right.node, Expr::IntLit(3)));
                }
                other => panic!("expected Binary(Gt) as cond, got {other:?}"),
            }
            assert!(matches!(then_branch.node, Expr::IntLit(10)));
            assert!(matches!(else_branch.node, Expr::IntLit(20)));
        }
        other => panic!("expected Ternary, got {other:?}"),
    }
}

#[test]
fn parse_ternary_nested_right_associative() {
    // 右结合：a ? b : c ? d : e → a ? b : (c ? d : e)
    let src = r#"void Main() {
    int a = 1 ? 2 : 3 ? 4 : 5;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let init = match &body.stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let with init"),
    };
    match &init.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            // cond = 1
            assert!(matches!(cond.node, Expr::IntLit(1)));
            // then_branch = 2
            assert!(matches!(then_branch.node, Expr::IntLit(2)));
            // else_branch = 嵌套的三元 (3 ? 4 : 5) — 右结合
            match &else_branch.node {
                Expr::Ternary {
                    cond: c2,
                    then_branch: t2,
                    else_branch: e2,
                } => {
                    assert!(matches!(c2.node, Expr::IntLit(3)));
                    assert!(matches!(t2.node, Expr::IntLit(4)));
                    assert!(matches!(e2.node, Expr::IntLit(5)));
                }
                other => panic!("expected nested Ternary in else_branch, got {other:?}"),
            }
        }
        other => panic!("expected Ternary, got {other:?}"),
    }
}

#[test]
fn parse_ternary_string_branches() {
    let src = r#"void Main() {
    string s = true ? "yes" : "no";
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let init = match &body.stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let with init"),
    };
    match &init.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(cond.node, Expr::BoolLit(true)));
            assert!(matches!(then_branch.node, Expr::StringLit(ref s) if s == "yes"));
            assert!(matches!(else_branch.node, Expr::StringLit(ref s) if s == "no"));
        }
        other => panic!("expected Ternary, got {other:?}"),
    }
}

#[test]
fn parse_ternary_in_call_argument() {
    // 三元表达式直接作为函数实参
    let src = r#"void Main() {
    int x = 42;
    Console.WriteLine(x > 0 ? "positive" : "negative");
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    // 第二个 Stmt 是 Expr(Call)
    let call = match &body.stmts[1].node {
        Stmt::Expr(e) => e,
        _ => panic!("expected Expr stmt"),
    };
    match &call.node {
        Expr::MethodCall { args, .. } => {
            assert_eq!(args.len(), 1);
            match &args[0].node {
                Expr::Ternary {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    // cond = x > 0
                    assert!(matches!(
                        cond.node,
                        Expr::Binary {
                            op: ast::BinOp::Gt,
                            ..
                        }
                    ));
                    assert!(matches!(then_branch.node, Expr::StringLit(ref s) if s == "positive"));
                    assert!(matches!(else_branch.node, Expr::StringLit(ref s) if s == "negative"));
                }
                other => panic!("expected Ternary in arg, got {other:?}"),
            }
        }
        other => panic!("expected MethodCall, got {other:?}"),
    }
}

#[test]
fn parse_ternary_with_logical_condition() {
    // `&&` 优先级高于 `?:`
    let src = r#"void Main() {
    int a = x > 0 && y < 10 ? 100 : 200;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let init = match &body.stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let with init"),
    };
    match &init.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            // cond 应该是 Binary(And, ...)，即整个 x > 0 && y < 10
            match &cond.node {
                Expr::Binary { op, .. } => {
                    assert_eq!(*op, ast::BinOp::And, "&& should bind tighter than ?:");
                }
                other => panic!("expected Binary(And) as cond, got {other:?}"),
            }
            assert!(matches!(then_branch.node, Expr::IntLit(100)));
            assert!(matches!(else_branch.node, Expr::IntLit(200)));
        }
        other => panic!("expected Ternary, got {other:?}"),
    }
}

// ── 复杂三元场景（真实代码库模式）──

#[test]
fn parse_ternary_in_return_statement() {
    // 来自 std/Arc/Types/Version.as:55: return a > b ? 1 : -1;
    let src = r#"
int compare(int a, int b) {
    return a > b ? 1 : -1;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let ret = match &body.stmts[0].node {
        Stmt::Return(Some(e)) => e,
        _ => panic!("expected return with value"),
    };
    match &ret.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(
                cond.node,
                Expr::Binary {
                    op: ast::BinOp::Gt,
                    ..
                }
            ));
            assert!(matches!(then_branch.node, Expr::IntLit(1)));
            assert!(matches!(
                else_branch.node,
                Expr::Unary {
                    op: ast::UnaryOp::Neg,
                    ..
                }
            ));
        }
        other => panic!("expected Ternary in return, got {other:?}"),
    }
}

#[test]
fn parse_ternary_multiple_assignments() {
    // 来自 std/Arc/Types/Version.as:40-43: 连续 4 个三元赋值
    let src = r#"
void parse(string[] parts, int count) {
    int ma = count >= 1 ? _parse(parts[0]) : 0;
    int mi = count >= 2 ? _parse(parts[1]) : 0;
    int bu = count >= 3 ? _parse(parts[2]) : 0;
    int re = count >= 4 ? _parse(parts[3]) : 0;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    for i in 0..4 {
        match &body.stmts[i].node {
            Stmt::Let { init: Some(e), .. } => {
                assert!(
                    matches!(e.node, Expr::Ternary { .. }),
                    "stmt {} should be Ternary",
                    i
                );
            }
            _ => panic!("expected let at stmt {}", i),
        }
    }
}

#[test]
fn parse_ternary_with_method_calls() {
    // 来自 std/Net/Core/Uri.as:173: slashPos >= 0 ? remaining.Substring(...) : ""
    let src = r#"
void parse(int slashPos, string remaining) {
    string result = slashPos >= 0 ? remaining.Substring(slashPos) : "";
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let init = match &body.stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    match &init.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(
                cond.node,
                Expr::Binary {
                    op: ast::BinOp::Ge,
                    ..
                }
            ));
            assert!(matches!(then_branch.node, Expr::MethodCall { .. }));
            assert!(matches!(else_branch.node, Expr::StringLit(ref s) if s.is_empty()));
        }
        other => panic!("expected Ternary, got {other:?}"),
    }
}

#[test]
fn parse_ternary_in_arithmetic() {
    // 来自 std/Arc/Types/DateTime.as:133: days += IsLeapYear(i) ? 366 : 365
    let src = r#"
void compute() {
    int days = 0;
    int i = 2020;
    days += IsLeapYear(i) ? 366 : 365;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let assign = match &body.stmts[2].node {
        Stmt::Assign {
            target: _,
            value: e,
        } => e,
        _ => panic!("expected assign"),
    };
    match &assign.node {
        Expr::Binary {
            op: ast::BinOp::Add,
            right,
            ..
        } => match &right.node {
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                assert!(matches!(cond.node, Expr::Call { .. }));
                assert!(matches!(then_branch.node, Expr::IntLit(366)));
                assert!(matches!(else_branch.node, Expr::IntLit(365)));
            }
            other => panic!("expected Ternary, got {other:?}"),
        },
        other => panic!("expected Binary(Add), got {other:?}"),
    }
}

#[test]
fn parse_compound_assign_desugars_to_binary() {
    // RFC 076：`a += e` → Assign(a, Binary(Add, a, e))
    let src = r#"
void compute() {
    int a = 1;
    a += 2;
    a -= 1;
    a *= 3;
    a /= 2;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let ops = [
        ast::BinOp::Add,
        ast::BinOp::Sub,
        ast::BinOp::Mul,
        ast::BinOp::Div,
    ];
    for (i, expected_op) in ops.into_iter().enumerate() {
        match &body.stmts[i + 1].node {
            Stmt::Assign { target, value } => {
                assert!(matches!(target.node, Expr::Ident(ref n) if n.as_str() == "a"));
                match &value.node {
                    Expr::Binary { op, left, right } => {
                        assert_eq!(*op, expected_op);
                        assert!(matches!(left.node, Expr::Ident(ref n) if n.as_str() == "a"));
                        assert!(matches!(right.node, Expr::IntLit(_)));
                    }
                    other => panic!("stmt {}: expected Binary, got {other:?}", i + 1),
                }
            }
            other => panic!("stmt {}: expected Assign, got {other:?}", i + 1),
        }
    }
}

#[test]
fn parse_null_coalesce_assign_desugars_to_coalesce() {
    // RFC 005 配套：`a ??= e` → Assign(a, Coalesce(a, e))——与 `??` 同一
    // AST 变体，typeck/codegen 零新增路径；C# 对应 `??=` null 合并赋值。
    let src = r#"
void compute() {
    string? a = null;
    a ??= "fallback";
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    match &body.stmts[1].node {
        Stmt::Assign { target, value } => {
            assert!(matches!(target.node, Expr::Ident(ref n) if n.as_str() == "a"));
            match &value.node {
                Expr::Coalesce { left, right } => {
                    assert!(matches!(left.node, Expr::Ident(ref n) if n.as_str() == "a"));
                    assert!(matches!(right.node, Expr::StringLit(_)));
                }
                other => panic!("expected Coalesce, got {other:?}"),
            }
        }
        other => panic!("expected Assign, got {other:?}"),
    }
}

#[test]
fn parse_assign_expression_lambda_body() {
    // 赋值表达式一等化（⑥）：lambda 表达式体内的赋值——`() => done = true`
    // 的 body 是 `Expr::Assign`（此前 parse_expr 停在 `=`，孤儿 `=` 报错）。
    let src = r#"
Func<bool> make() {
    bool done = false;
    return () => done = true;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    match &f.body.as_ref().unwrap().stmts[1].node {
        Stmt::Return(Some(ret)) => match &ret.node {
            Expr::Lambda(l) => match &l.body {
                LambdaBody::Expr(body) => match &body.node {
                    Expr::Assign { target, value } => {
                        assert!(matches!(target.node, Expr::Ident(ref n) if n.as_str() == "done"));
                        assert!(matches!(value.node, Expr::BoolLit(true)));
                    }
                    other => panic!("expected Assign body, got {other:?}"),
                },
                other => panic!("expected Expr body, got {other:?}"),
            },
            other => panic!("expected Lambda, got {other:?}"),
        },
        other => panic!("expected Return, got {other:?}"),
    }
}

#[test]
fn parse_assign_expression_right_associative_and_ternary_rhs() {
    // 赋值右结合：`a = b = 7` 右折叠；三元 rhs：`g = c ? p : q`（赋值 rhs
    // 内三元须消费——Question guard 放宽至 min_bp <= 1）。
    let src = r#"
void compute() {
    int a;
    int b;
    int g;
    a = b = 7;
    g = a > 0 ? 1 : 0;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    // 右结合：a = (b = 7)——value 侧是嵌套 Assign。
    match &body.stmts[3].node {
        Stmt::Assign { target, value } => {
            assert!(matches!(target.node, Expr::Ident(ref n) if n.as_str() == "a"));
            match &value.node {
                Expr::Assign { target: t2, .. } => {
                    assert!(matches!(t2.node, Expr::Ident(ref n) if n.as_str() == "b"));
                }
                other => panic!("expected nested Assign (right-assoc), got {other:?}"),
            }
        }
        other => panic!("expected Assign, got {other:?}"),
    }
    // 三元 rhs：g = (a > 0 ? 1 : 0)——value 是 Ternary。
    match &body.stmts[4].node {
        Stmt::Assign { target, value } => {
            assert!(matches!(target.node, Expr::Ident(ref n) if n.as_str() == "g"));
            assert!(matches!(value.node, Expr::Ternary { .. }));
        }
        other => panic!("expected Assign, got {other:?}"),
    }
}

#[test]
fn parse_assign_expression_as_operand() {
    // 赋值表达式作操作数/实参（C# 惯用法）：`(v = -3) + 10`——括号内赋值
    // 与 lambda 形参默认值消歧（check_lambda 的平衡 `)` 前瞻）。
    let src = r#"
int compute() {
    int v;
    return (v = -3) + 10;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    match &f.body.as_ref().unwrap().stmts[1].node {
        Stmt::Return(Some(ret)) => match &ret.node {
            Expr::Binary { left, .. } => match &left.node {
                Expr::Assign { target, .. } => {
                    assert!(matches!(target.node, Expr::Ident(ref n) if n.as_str() == "v"));
                }
                other => panic!("expected Assign operand, got {other:?}"),
            },
            other => panic!("expected Binary, got {other:?}"),
        },
        other => panic!("expected Return, got {other:?}"),
    }
}

#[test]
fn parse_ternary_deep_nesting_3_levels() {
    // a ? b : c ? d : e ? f : g  →  a ? b : (c ? d : (e ? f : g))
    let src = r#"
void demo() {
    int result = 1 ? 2 : 3 ? 4 : 5 ? 6 : 7;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().unwrap();
    let init = match &body.stmts[0].node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    let l1 = match &init.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(cond.node, Expr::IntLit(1)));
            assert!(matches!(then_branch.node, Expr::IntLit(2)));
            else_branch
        }
        other => panic!("expected Ternary L1, got {other:?}"),
    };
    let l2 = match &l1.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(cond.node, Expr::IntLit(3)));
            assert!(matches!(then_branch.node, Expr::IntLit(4)));
            else_branch
        }
        other => panic!("expected Ternary L2, got {other:?}"),
    };
    match &l2.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(cond.node, Expr::IntLit(5)));
            assert!(matches!(then_branch.node, Expr::IntLit(6)));
            assert!(matches!(else_branch.node, Expr::IntLit(7)));
        }
        other => panic!("expected Ternary L3, got {other:?}"),
    }
}

#[test]
fn parse_ternary_with_new_expression() {
    // 来自 std/Arc/Types/TimeSpan.as:68: ticks < 0 ? new TimeSpan(-ticks) : new TimeSpan(ticks)
    let src = r#"
class TimeSpan {
    private int _ticks;
    public TimeSpan(int t) { _ticks = t; }
    public TimeSpan Duration() {
        return this._ticks < 0 ? new TimeSpan(-this._ticks) : new TimeSpan(this._ticks);
    }
}"#;
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    let m = &class.methods[0].node;
    let mbody = m.body.as_ref().unwrap();
    let ret = match &mbody.stmts[0].node {
        Stmt::Return(Some(e)) => e,
        _ => panic!("expected return"),
    };
    match &ret.node {
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            assert!(matches!(
                cond.node,
                Expr::Binary {
                    op: ast::BinOp::Lt,
                    ..
                }
            ));
            assert!(matches!(then_branch.node, Expr::New { .. }));
            assert!(matches!(else_branch.node, Expr::New { .. }));
        }
        other => panic!("expected Ternary with New branches, got {other:?}"),
    }
}

/// RFC 061 M1：静态字段解析验证。
/// 覆盖 `static` 单独、`static readonly` 组合、`readonly static` 顺序灵活性、
/// 静态方法识别、实例字段不受影响。
#[test]
fn parse_static_field_modifiers() {
    let src = "\
class Counter {
    private static int _count = 0;
    private static readonly int _max = 100;
    private static readonly int _min = 0;
    public static int Increment() { return 0; }
    private int _instance;
    public const int K = 42;
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    // 字段顺序：_count, _max, _min, _instance, K
    let fields = &class.fields;
    assert_eq!(fields.len(), 5, "expected 5 fields");

    // _count: static, 非 const, 非 readonly
    assert_eq!(fields[0].name.as_str(), "_count");
    assert!(fields[0].is_static, "_count should be static");
    assert!(!fields[0].is_const, "_count should not be const");
    assert!(!fields[0].is_readonly, "_count should not be readonly");

    // _max: static + readonly
    assert_eq!(fields[1].name.as_str(), "_max");
    assert!(fields[1].is_static, "_max should be static");
    assert!(!fields[1].is_const, "_max should not be const");
    assert!(fields[1].is_readonly, "_max should be readonly");

    // _min: readonly static (顺序灵活性验证)
    assert_eq!(fields[2].name.as_str(), "_min");
    assert!(
        fields[2].is_static,
        "_min should be static (readonly static order)"
    );
    assert!(fields[2].is_readonly, "_min should be readonly");

    // _instance: 非 static 实例字段
    assert_eq!(fields[3].name.as_str(), "_instance");
    assert!(!fields[3].is_static, "_instance should NOT be static");
    assert!(!fields[3].is_const);
    assert!(!fields[3].is_readonly);

    // K: const（隐含 static，但 is_static 应为 false——const 不消费 static token）
    assert_eq!(fields[4].name.as_str(), "K");
    assert!(fields[4].is_const, "K should be const");
    assert!(
        !fields[4].is_static,
        "const field should not set is_static (const implies static)"
    );

    // Increment 方法应为 static（modifier == Static）
    let inc = &class.methods[0].node;
    assert_eq!(inc.sig.name.as_str(), "Increment");
    assert_eq!(
        inc.sig.modifier,
        ast::MethodModifier::Static,
        "Increment should be static method"
    );
}

/// RFC 061 M1：`static const` 互斥修饰符应报错。
#[test]
fn parse_static_const_rejects_redundant_modifier() {
    let src = "\
class Bad {
    private static const int _x = 0;
}
";
    let result = Parser::parse_program(src);
    assert!(result.is_err(), "static const should be rejected");
    let err = result.unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("const") && msg.contains("static"),
        "error should mention both static and const, got: {msg}"
    );
}

/// RFC 031 M2：泛型 variant 表达式级调用 `Option<int>.Some(42)`。
/// 验证 parser 消歧 `Option<int>.Some` 为泛型类型限定 + 成员访问，
/// 而非 `Option < int > .Some` 比较链。
#[test]
fn parse_generic_variant_expr() {
    let src = "void main() { var r = Option<int>.Some(42); }";
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let stmt = &f.body.as_ref().unwrap().stmts[0];
    let init = match &stmt.node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    match &init.node {
        Expr::MethodCall {
            receiver,
            method,
            args,
            type_args,
            params_span: _,
        } => {
            // receiver 应为 `Call { func: Ident("Option"), args: [], type_args: [int] }`
            match &receiver.node {
                Expr::Call {
                    func: _,
                    args: call_args,
                    type_args: call_type_args,
                    params_span: _,
                } => {
                    assert!(call_args.is_empty());
                    assert_eq!(call_type_args.len(), 1);
                    match &call_type_args[0].node {
                        ast::Type::Named { path, .. } => {
                            assert_eq!(path[0].as_str(), "int");
                        }
                        _ => panic!("expected Type::Named"),
                    }
                }
                other => panic!("expected Call receiver, got {other:?}"),
            }
            assert_eq!(method.as_str(), "Some");
            assert_eq!(args.len(), 1);
            assert!(matches!(args[0].node, Expr::IntLit(42)));
            assert!(type_args.is_empty());
        }
        other => panic!("expected MethodCall, got {other:?}"),
    }
}

/// RFC 031 M2：泛型 variant switch 模式 `Option<int>.Some(n)`。
#[test]
fn parse_generic_variant_pattern() {
    let src =
        "void main() { int v = r switch { Option<int>.Some(n) => n, Option<int>.None => 0 }; }";
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let stmt = &f.body.as_ref().unwrap().stmts[0];
    let init = match &stmt.node {
        Stmt::Let { init: Some(e), .. } => e,
        _ => panic!("expected let"),
    };
    let Expr::SwitchForm(sw) = &init.node else {
        panic!("expected SwitchForm");
    };
    assert_eq!(sw.arms.len(), 2);
    match &sw.arms[0].pattern {
        Pattern::Variant {
            path,
            type_args,
            case,
            binding,
        } => {
            assert_eq!(path[0].as_str(), "Option");
            assert_eq!(type_args.len(), 1);
            assert_eq!(case.as_str(), "Some");
            assert_eq!(binding.as_ref().map(|b| b.as_str()), Some("n"));
        }
        other => panic!("expected Variant pattern, got {other:?}"),
    }
    match &sw.arms[1].pattern {
        Pattern::Variant {
            path,
            type_args,
            case,
            binding,
        } => {
            assert_eq!(path[0].as_str(), "Option");
            assert_eq!(type_args.len(), 1);
            assert_eq!(case.as_str(), "None");
            assert!(binding.is_none());
        }
        other => panic!("expected Variant pattern, got {other:?}"),
    }
}

/// RFC 009 L1：primary constructor 最小子集 — 声明脱糖为字段捕获 + 合成构造。
#[test]
fn parse_primary_constructor_desugars_fields_and_ctor() {
    let src = "\
public class Point(int x, int y) {
    public int X() { return x; }
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert_eq!(class.fields.len(), 2);
    assert_eq!(class.fields[0].name.as_str(), "x");
    assert_eq!(class.fields[1].name.as_str(), "y");
    assert_eq!(class.fields[0].vis, Visibility::Private);
    assert_eq!(class.fields[1].vis, Visibility::Private);
    assert_eq!(class.constructors.len(), 1);
    let ctor = &class.constructors[0].node;
    assert_eq!(ctor.vis, Visibility::Public);
    assert_eq!(ctor.params.len(), 2);
    assert_eq!(ctor.params[0].name.as_str(), "x");
    assert_eq!(ctor.params[1].name.as_str(), "y");
    assert_eq!(ctor.body.stmts.len(), 2);
    match &ctor.body.stmts[0].node {
        Stmt::Assign { target, value } => {
            match &target.node {
                Expr::Field { receiver, field } => {
                    assert!(matches!(receiver.node, Expr::This));
                    assert_eq!(field.as_str(), "x");
                }
                other => panic!("expected Field target, got {other:?}"),
            }
            assert!(matches!(value.node, Expr::Ident(ref n) if n.as_str() == "x"));
        }
        other => panic!("expected Assign, got {other:?}"),
    }
}

#[test]
fn parse_primary_constructor_rejects_field_name_conflict() {
    let src = "\
class C(int x) {
    private int x;
}
";
    let err = Parser::parse_program(src).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("conflicts with a field"),
        "unexpected error: {msg}"
    );
}

/// Primary：`ref`/`out`/`in` 不捕获为字段，但仍保留在合成 ctor 形参上。
#[test]
fn parse_primary_constructor_ref_out_in_no_capture() {
    let src = "\
public class Holder(ref int x, out int y, in int z) {
    public int Snapshot = x;
}
";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    // 仅用户声明的 Snapshot 字段；无 x/y/z 捕获字段
    assert_eq!(class.fields.len(), 1);
    assert_eq!(class.fields[0].name.as_str(), "Snapshot");
    assert_eq!(class.constructors.len(), 1);
    let ctor = &class.constructors[0].node;
    assert_eq!(ctor.params.len(), 3);
    assert!(ctor.params[0].is_ref);
    assert!(ctor.params[1].is_out);
    assert!(ctor.params[2].is_in);
    assert!(
        ctor.body.stmts.is_empty(),
        "by-ref params must not emit this.x = x"
    );
}

/// Primary：`class D(int x) : Base(x)` 脱糖为合成 ctor 的 `base_args`。
#[test]
fn parse_primary_constructor_base_args() {
    let src = "\
public class Base {
    public Base(int n) { }
}
public class Derived(int x) : Base(x) {
    public int X() { return x; }
}
";
    let program = Parser::parse_program(src).unwrap();
    let derived = match &program.items[1].node {
        Item::Class(c) => c,
        _ => panic!("expected Derived class"),
    };
    assert_eq!(derived.name.as_str(), "Derived");
    assert_eq!(derived.bases.len(), 1);
    assert_eq!(derived.constructors.len(), 1);
    let ctor = &derived.constructors[0].node;
    let args = ctor.base_args.as_ref().expect("expected base_args");
    assert_eq!(args.len(), 1);
    assert!(matches!(args[0].node, Expr::Ident(ref n) if n.as_str() == "x"));
}

#[test]
fn parse_primary_constructor_base_args_require_primary() {
    let src = "\
class Base { public Base(int n) { } }
class Bad : Base(1) { }
";
    let err = Parser::parse_program(src).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("primary constructor"),
        "unexpected error: {msg}"
    );
}

/// RFC 066：位置参数 record 脱糖为公共字段 + 构造器。
#[test]
fn parse_record_positional() {
    let src = "record Point(int X, int Y);";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert!(class.is_record);
    assert_eq!(class.name.as_str(), "Point");
    assert!(
        class.fields.is_empty(),
        "positional -> init properties, not fields"
    );
    assert_eq!(class.properties.len(), 2);
    assert_eq!(class.properties[0].name.as_str(), "X");
    assert_eq!(class.properties[1].name.as_str(), "Y");
    assert!(
        class.properties[0].has_get && class.properties[0].has_init && !class.properties[0].has_set
    );
    assert!(
        class.properties[1].has_get && class.properties[1].has_init && !class.properties[1].has_set
    );
    assert_eq!(class.constructors.len(), 1);
    assert_eq!(class.constructors[0].node.params.len(), 2);
    assert_eq!(class.constructors[0].node.params[0].name.as_str(), "x");
    assert_eq!(class.constructors[0].node.params[1].name.as_str(), "y");
    assert_eq!(class.constructors[0].node.body.stmts.len(), 2);
    // RFC 066 M2：合成 Equals
    assert!(
        class.methods.iter().any(|m| {
            m.node.sig.name.as_str() == "Equals"
                && !matches!(m.node.sig.modifier, ast::MethodModifier::Static)
        }),
        "expected synthesized instance Equals"
    );
    // RFC 066：合成 GetHashCode（与 Equals 同字段）
    assert!(
        class.methods.iter().any(|m| {
            m.node.sig.name.as_str() == "GetHashCode"
                && !matches!(m.node.sig.modifier, ast::MethodModifier::Static)
        }),
        "expected synthesized instance GetHashCode"
    );
    // RFC 066：合成 static Equals / GetHashCode（IEquatable / IHashable）
    assert!(
        class.methods.iter().any(|m| {
            m.node.sig.name.as_str() == "Equals"
                && matches!(m.node.sig.modifier, ast::MethodModifier::Static)
                && m.node.sig.params.len() == 2
        }),
        "expected synthesized static Equals"
    );
    assert!(
        class.methods.iter().any(|m| {
            m.node.sig.name.as_str() == "GetHashCode"
                && matches!(m.node.sig.modifier, ast::MethodModifier::Static)
                && m.node.sig.params.len() == 1
        }),
        "expected synthesized static GetHashCode"
    );
    assert!(
        class.bases.iter().any(|b| matches!(
            b,
            ast::Type::Named { path, .. } if path.last().map(|n| n.as_str()) == Some("IEquatable")
        )),
        "expected IEquatable base"
    );
    assert!(
        class.bases.iter().any(|b| matches!(
            b,
            ast::Type::Named { path, .. } if path.last().map(|n| n.as_str()) == Some("IHashable")
        )),
        "expected IHashable base"
    );
    // RFC 066：合成 Deconstruct（位置参数 out）
    let deconstruct = class
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Deconstruct")
        .expect("expected synthesized Deconstruct");
    assert_eq!(deconstruct.node.sig.params.len(), 2);
    assert!(deconstruct.node.sig.params[0].is_out);
    assert!(deconstruct.node.sig.params[1].is_out);
    assert_eq!(deconstruct.node.sig.params[0].name.as_str(), "x");
    assert_eq!(deconstruct.node.sig.params[1].name.as_str(), "y");
}

#[test]
fn reject_record_class_keyword_synonym() {
    // RFC 075：`record class` 是 C# 兼容同义拼写，Arc 收口单一惯用法 → 硬拒。
    let src = "record class Person { public string Name; }";
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("record class"),
        "unexpected error: {err}"
    );
}

#[test]
fn parse_record_body_form_is_teaching_path() {
    // RFC 075：体形式 `record` 是教学正道（勿用 `record class`）。
    let src = "record Person { public string Name; }";
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert!(class.is_record);
    assert_eq!(class.name.as_str(), "Person");
    assert_eq!(class.fields.len(), 1);
    // 无位置参数 → 无 Deconstruct
    assert!(
        !class
            .methods
            .iter()
            .any(|m| m.node.sig.name.as_str() == "Deconstruct"),
        "body-only record must not synthesize Deconstruct"
    );
}

#[test]
fn parse_record_positional_with_body_method() {
    let src = r#"
record Point(int X, int Y) {
    public int Sum() { return X + Y; }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let class = match &program.items[0].node {
        Item::Class(c) => c,
        _ => panic!("expected class"),
    };
    assert!(class.is_record);
    assert!(class.fields.is_empty());
    assert_eq!(class.properties.len(), 2);
    assert_eq!(class.methods.len(), 6); // Sum + Equals + GetHashCode + Deconstruct + static Equals + static GetHashCode
    assert!(class
        .methods
        .iter()
        .any(|m| m.node.sig.name.as_str() == "Sum"));
    assert!(class.methods.iter().any(|m| {
        m.node.sig.name.as_str() == "Equals"
            && !matches!(m.node.sig.modifier, ast::MethodModifier::Static)
    }));
    assert!(class.methods.iter().any(|m| {
        m.node.sig.name.as_str() == "Equals"
            && matches!(m.node.sig.modifier, ast::MethodModifier::Static)
    }));
    assert!(class.methods.iter().any(|m| {
        m.node.sig.name.as_str() == "GetHashCode"
            && !matches!(m.node.sig.modifier, ast::MethodModifier::Static)
    }));
    assert!(class.methods.iter().any(|m| {
        m.node.sig.name.as_str() == "GetHashCode"
            && matches!(m.node.sig.modifier, ast::MethodModifier::Static)
    }));
    assert!(class
        .methods
        .iter()
        .any(|m| m.node.sig.name.as_str() == "Deconstruct"));
    // RFC 066：自动实现 IEquatable / IHashable
    assert!(
        class.bases.iter().any(|b| matches!(
            b,
            ast::Type::Named { path, generics }
                if path.last().map(|n| n.as_str()) == Some("IEquatable") && generics.len() == 1
        )),
        "expected IEquatable<Point> base"
    );
    assert!(
        class.bases.iter().any(|b| matches!(
            b,
            ast::Type::Named { path, generics }
                if path.last().map(|n| n.as_str()) == Some("IHashable") && generics.len() == 1
        )),
        "expected IHashable<Point> base"
    );
    assert_eq!(class.constructors.len(), 1);
}

#[test]
fn parse_with_expression() {
    let src = r#"
void Main() {
    Point q = p with { X = 10 };
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let f = match &program.items[0].node {
        Item::Fn(f) => f,
        _ => panic!("expected fn"),
    };
    let body = f.body.as_ref().expect("body");
    let Stmt::Let {
        init: Some(init), ..
    } = &body.stmts[0].node
    else {
        panic!("expected let");
    };
    match &init.node {
        Expr::With { inits, .. } => {
            assert_eq!(inits.len(), 1);
            assert_eq!(inits[0].0.as_str(), "X");
        }
        other => panic!("expected With, got {other:?}"),
    }
}

#[test]
fn parse_record_struct_positional() {
    let src = "record struct Point(int X, int Y);";
    let program = Parser::parse_program(src).unwrap();
    let s = match &program.items[0].node {
        Item::Struct(s) => s,
        other => panic!("expected struct, got {other:?}"),
    };
    assert!(s.is_record);
    assert_eq!(s.name.as_str(), "Point");
    assert_eq!(s.properties.len(), 2);
    assert_eq!(s.constructors.len(), 1);
    assert!(s
        .methods
        .iter()
        .any(|m| m.node.sig.name.as_str() == "Equals"));
    assert!(s
        .methods
        .iter()
        .any(|m| m.node.sig.name.as_str() == "GetHashCode"));
    // RFC 066：record struct 自动实现 IEquatable / IHashable
    assert!(
        s.bases.iter().any(|b| matches!(
            b,
            ast::Type::Named { path, generics }
                if path.last().map(|n| n.as_str()) == Some("IEquatable") && generics.len() == 1
        )),
        "expected IEquatable<Point> base"
    );
    assert!(
        s.bases.iter().any(|b| matches!(
            b,
            ast::Type::Named { path, generics }
                if path.last().map(|n| n.as_str()) == Some("IHashable") && generics.len() == 1
        )),
        "expected IHashable<Point> base"
    );
    assert!(
        s.methods.iter().any(|m| {
            matches!(m.node.sig.modifier, ast::MethodModifier::Static)
                && m.node.sig.name.as_str() == "Equals"
                && m.node.sig.params.len() == 2
        }),
        "expected synthesized static Equals"
    );
    assert!(
        s.methods.iter().any(|m| {
            matches!(m.node.sig.modifier, ast::MethodModifier::Static)
                && m.node.sig.name.as_str() == "GetHashCode"
                && m.node.sig.params.len() == 1
        }),
        "expected synthesized static GetHashCode"
    );
    // record struct：struct 实例方法 out 管线关账后合成 Deconstruct
    let deco = s
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Deconstruct")
        .expect("expected synthesized Deconstruct on record struct");
    assert_eq!(deco.node.sig.params.len(), 2);
    assert!(deco.node.sig.params.iter().all(|p| p.is_out));
}

#[test]
fn reject_partial_record() {
    let src = "partial record Point(int X);";
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("partial record"),
        "unexpected error: {err}"
    );
}

#[test]
fn reject_record_base_ctor_args() {
    let src = "record Child(int X) : Parent(X);";
    let err = Parser::parse_program(src).unwrap_err();
    assert!(
        err.to_string().contains("base constructor"),
        "unexpected error: {err}"
    );
}

#[test]
fn parse_deconstruct_assign_desugars_to_method_call() {
    // RFC 067 M1：`(x, y) = p;` → `p.Deconstruct(out x, out y);`
    let src = r#"
void Main() {
    int x;
    int y;
    Point p = new Point(1, 2);
    (x, y) = p;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &program.items[0].node else {
        panic!("expected fn");
    };
    let body = f.body.as_ref().expect("body");
    let last = body.stmts.last().expect("stmt");
    let Stmt::Expr(call) = &last.node else {
        panic!("expected Expr stmt, got {:?}", last.node);
    };
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = &call.node
    else {
        panic!("expected MethodCall, got {:?}", call.node);
    };
    assert_eq!(method.as_str(), "Deconstruct");
    assert!(matches!(receiver.node, Expr::Ident(ref n) if n.as_str() == "p"));
    assert_eq!(args.len(), 2);
    for (i, name) in ["x", "y"].iter().enumerate() {
        match &args[i].node {
            Expr::RefArg { is_out: true, expr } => {
                assert!(matches!(expr.node, Expr::Ident(ref n) if n.as_str() == *name));
            }
            other => panic!("arg {i}: expected out RefArg, got {other:?}"),
        }
    }
}

#[test]
fn deconstruct_assign_requires_two_idents() {
    // 单元素 `(x) = p` 不走 RFC 067 M1，仍为括号赋值
    let src = r#"
void Main() {
    int x;
    (x) = 1;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &program.items[0].node else {
        panic!("expected fn");
    };
    let body = f.body.as_ref().expect("body");
    let last = body.stmts.last().expect("stmt");
    assert!(
        matches!(last.node, Stmt::Assign { .. }),
        "single paren assign must stay Assign, got {:?}",
        last.node
    );
}

#[test]
fn parse_deconstruct_assign_with_discard() {
    // RFC 067 M2：`(x, _) = p;` → DeconstructAssign（含弃元）
    let src = r#"
void Main() {
    int x;
    Point p = new Point(1, 2);
    (x, _) = p;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &program.items[0].node else {
        panic!("expected fn");
    };
    let body = f.body.as_ref().expect("body");
    let last = body.stmts.last().expect("stmt");
    let Stmt::DeconstructAssign {
        declare,
        targets,
        value,
    } = &last.node
    else {
        panic!("expected DeconstructAssign, got {:?}", last.node);
    };
    assert!(!declare);
    assert_eq!(targets.len(), 2);
    assert!(matches!(&targets[0], DeconstructTarget::Bind(Some(n)) if n.as_str() == "x"));
    assert!(matches!(&targets[1], DeconstructTarget::Bind(None)));
    assert!(matches!(value.node, Expr::Ident(ref n) if n.as_str() == "p"));
}

#[test]
fn parse_var_deconstruct_declare() {
    // RFC 067 M2：`var (x, y) = p;` → DeconstructAssign(declare=true)
    let src = r#"
void Main() {
    Point p = new Point(1, 2);
    var (x, y) = p;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &program.items[0].node else {
        panic!("expected fn");
    };
    let body = f.body.as_ref().expect("body");
    let last = body.stmts.last().expect("stmt");
    let Stmt::DeconstructAssign {
        declare,
        targets,
        value,
    } = &last.node
    else {
        panic!("expected DeconstructAssign, got {:?}", last.node);
    };
    assert!(*declare);
    assert_eq!(targets.len(), 2);
    assert!(matches!(&targets[0], DeconstructTarget::Bind(Some(n)) if n.as_str() == "x"));
    assert!(matches!(&targets[1], DeconstructTarget::Bind(Some(n)) if n.as_str() == "y"));
    assert!(matches!(value.node, Expr::Ident(ref n) if n.as_str() == "p"));
}

#[test]
fn parse_var_deconstruct_with_discard() {
    let src = r#"
void Main() {
    Point p = new Point(1, 2);
    var (x, _) = p;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &program.items[0].node else {
        panic!("expected fn");
    };
    let body = f.body.as_ref().expect("body");
    let last = body.stmts.last().expect("stmt");
    let Stmt::DeconstructAssign {
        declare, targets, ..
    } = &last.node
    else {
        panic!("expected DeconstructAssign, got {:?}", last.node);
    };
    assert!(*declare);
    assert!(matches!(&targets[0], DeconstructTarget::Bind(Some(n)) if n.as_str() == "x"));
    assert!(matches!(&targets[1], DeconstructTarget::Bind(None)));
}

#[test]
fn parse_positional_is_pattern_m3() {
    // RFC 067 M3：`p is (var x, var y)` → IsPattern::Positional
    let src = r#"
void Main() {
    if (p is (var x, var y)) { }
}
"#;
    let module = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &module.items[0].node else {
        panic!("expected fn");
    };
    let Stmt::Expr(e) = &f.body.as_ref().unwrap().stmts[0].node else {
        panic!("expected expr stmt");
    };
    let Expr::If { cond, .. } = &e.node else {
        panic!("expected if");
    };
    let Expr::Is { pattern, .. } = &cond.node else {
        panic!("expected is");
    };
    let IsPattern::Positional(elems) = pattern else {
        panic!("expected Positional, got {pattern:?}");
    };
    assert_eq!(elems.len(), 2);
    assert!(matches!(&elems[0], PositionalSubpattern::Var(n) if n.as_str() == "x"));
    assert!(matches!(&elems[1], PositionalSubpattern::Var(n) if n.as_str() == "y"));
}

#[test]
fn parse_is_constant_pattern() {
    // RFC 004 常量模式：`n is 5` / `s is "a"` / `b is true/false` / `c is 'x'`
    // → IsPattern::Constant（字面量）。
    let src = r#"
void Main() {
    int n = 5;
    bool a = n is 5;
    bool b = n is "x";
    bool c = n is true;
    bool d = n is false;
    bool e = n is 'c';
}
"#;
    let module = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &module.items[0].node else {
        panic!("expected fn");
    };
    let stmts = &f.body.as_ref().unwrap().stmts;
    let pattern_of = |idx: usize| -> &IsPattern {
        let Stmt::Let { init, .. } = &stmts[idx].node else {
            panic!("expected let");
        };
        let Some(init) = init else {
            panic!("expected init");
        };
        let Expr::Is { pattern, .. } = &init.node else {
            panic!("expected is");
        };
        pattern
    };
    assert!(matches!(
        pattern_of(1),
        IsPattern::Constant(lit) if matches!(&lit.node, Expr::IntLit(_))
    ));
    assert!(matches!(
        pattern_of(2),
        IsPattern::Constant(lit) if matches!(&lit.node, Expr::StringLit(s) if s == "x")
    ));
    assert!(matches!(
        pattern_of(3),
        IsPattern::Constant(lit) if matches!(&lit.node, Expr::BoolLit(true))
    ));
    assert!(matches!(
        pattern_of(4),
        IsPattern::Constant(lit) if matches!(&lit.node, Expr::BoolLit(false))
    ));
    assert!(matches!(
        pattern_of(5),
        IsPattern::Constant(lit) if matches!(&lit.node, Expr::CharLit(_))
    ));
}

#[test]
fn parse_is_precedence_tighter_than_logical() {
    // is+&& 误标缺陷回归：`is` 优先级须高于 `&&`/`||`/`==`（C# relational &
    // type-testing 同级）。若错置为低于 `&&`，`n == 5 && o is int` 会被解析为
    // `(n == 5 && o) is int`——scrutinee 误收为整个布尔表达式。
    let src = r#"
void Main() {
    bool r1 = n == 5 && o is int;
    bool r2 = n == 6 || o is int;
    bool r3 = a == b is C;
}
"#;
    let module = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &module.items[0].node else {
        panic!("expected fn");
    };
    let stmts = &f.body.as_ref().unwrap().stmts;

    // `n == 5 && o is int` → `(n == 5) && (o is int)`
    let Stmt::Let {
        init: Some(init1), ..
    } = &stmts[0].node
    else {
        panic!("expected let");
    };
    let Expr::Binary {
        op: BinOp::And,
        left,
        right,
    } = &init1.node
    else {
        panic!("expected &&, got {:?}", init1.node);
    };
    assert!(matches!(&left.node, Expr::Binary { op: BinOp::Eq, .. }));
    assert!(
        matches!(&right.node, Expr::Is { .. }),
        "right of && must be Is, got {:?}",
        right.node
    );

    // `n == 6 || o is int` → `(n == 6) || (o is int)`
    let Stmt::Let {
        init: Some(init2), ..
    } = &stmts[1].node
    else {
        panic!("expected let");
    };
    let Expr::Binary {
        op: BinOp::Or,
        right,
        ..
    } = &init2.node
    else {
        panic!("expected ||, got {:?}", init2.node);
    };
    assert!(
        matches!(&right.node, Expr::Is { .. }),
        "right of || must be Is, got {:?}",
        right.node
    );

    // `a == b is C` → `a == (b is C)`（is 高于 ==）
    let Stmt::Let {
        init: Some(init3), ..
    } = &stmts[2].node
    else {
        panic!("expected let");
    };
    let Expr::Binary {
        op: BinOp::Eq,
        right,
        ..
    } = &init3.node
    else {
        panic!("expected ==, got {:?}", init3.node);
    };
    assert!(
        matches!(&right.node, Expr::Is { .. }),
        "right of == must be Is, got {:?}",
        right.node
    );
}

#[test]
fn parse_positional_switch_pattern_m3() {
    // RFC 067 M3：`case (var a, _):` → Pattern::Positional
    let src = r#"
void Main() {
    switch (p) {
        case (var a, _):
            break;
        default:
            break;
    }
}
"#;
    let module = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &module.items[0].node else {
        panic!("expected fn");
    };
    let Stmt::Expr(e) = &f.body.as_ref().unwrap().stmts[0].node else {
        panic!("expected expr stmt");
    };
    let Expr::Switch(sw) = &e.node else {
        panic!("expected switch");
    };
    let Some(Pattern::Positional(elems)) = &sw.cases[0].pattern else {
        panic!("expected Positional case, got {:?}", sw.cases[0].pattern);
    };
    assert_eq!(elems.len(), 2);
    assert!(matches!(&elems[0], PositionalSubpattern::Var(n) if n.as_str() == "a"));
    assert!(matches!(&elems[1], PositionalSubpattern::Discard));
}

#[test]
fn parse_switch_case_block_body() {
    // AGENTS.md §5：switch 的每个 case/default 分支体可用 `{}` 括起（Allman 风格）。
    // 块内语句由块自身界定，不依赖 break/case/default 终止。
    let src = r#"
int Run(string s) {
    switch (s) {
        case "a": {
            return 1;
        }
        case "b":
        {
            return 2;
        }
        default: {
            return 0;
        }
    }
}
"#;
    let module = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &module.items[0].node else {
        panic!("expected fn");
    };
    let Stmt::Expr(e) = &f.body.as_ref().unwrap().stmts[0].node else {
        panic!("expected expr stmt");
    };
    let Expr::Switch(sw) = &e.node else {
        panic!("expected switch");
    };
    assert_eq!(sw.cases.len(), 3, "expected 3 cases (a/b/default)");

    // case "a": { return 1; } — 块体应包含 1 条 return 语句
    let case_a_body = &sw.cases[0].body;
    assert_eq!(
        case_a_body.stmts.len(),
        1,
        "case \"a\" block should have 1 stmt"
    );
    assert!(matches!(&case_a_body.stmts[0].node, Stmt::Return(Some(_))));

    // case "b": { return 2; } — Allman 风格（左花括号独立成行）同样支持
    let case_b_body = &sw.cases[1].body;
    assert_eq!(
        case_b_body.stmts.len(),
        1,
        "case \"b\" block should have 1 stmt"
    );
    assert!(matches!(&case_b_body.stmts[0].node, Stmt::Return(Some(_))));

    // default: { return 0; } — default 分支体同样支持块语法
    let default_body = &sw.cases[2].body;
    assert_eq!(
        default_body.stmts.len(),
        1,
        "default block should have 1 stmt"
    );
    assert!(matches!(&default_body.stmts[0].node, Stmt::Return(Some(_))));
}

#[test]
fn positional_typed_subpattern_m5() {
    // RFC 067 M5：`p is (int x, int y)` → Typed 子模式
    let program = Parser::parse_program(
        r#"
record Point(int X, int Y);
void F(Point p) {
    if (p is (int x, int y)) { }
}
"#,
    )
    .unwrap();
    let Item::Fn(f) = &program.items[1].node else {
        panic!("expected fn");
    };
    let Stmt::Expr(e) = &f.body.as_ref().unwrap().stmts[0].node else {
        panic!("expected expr stmt");
    };
    let Expr::If { cond, .. } = &e.node else {
        panic!("expected if");
    };
    let Expr::Is { pattern, .. } = &cond.node else {
        panic!("expected is");
    };
    let IsPattern::Positional(elems) = pattern else {
        panic!("expected Positional, got {pattern:?}");
    };
    assert!(matches!(
        &elems[0],
        PositionalSubpattern::Typed { name, .. } if name.as_str() == "x"
    ));
    assert!(matches!(
        &elems[1],
        PositionalSubpattern::Typed { name, .. } if name.as_str() == "y"
    ));
}

#[test]
fn parse_positional_const_and_nested_m6() {
    // RFC 067 M6：常量子模式 + 嵌套
    let src = "\
void Main() {
    if (p is (1, 2)) { }
    if (s is ((var x, var y), _)) { }
}
";
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(func) = &program.items[0].node else {
        panic!("expected fn");
    };
    let body = func.body.as_ref().unwrap();
    let Stmt::Expr(e0) = &body.stmts[0].node else {
        panic!("expected if");
    };
    let Expr::If { cond, .. } = &e0.node else {
        panic!("expected if expr");
    };
    let Expr::Is { pattern, .. } = &cond.node else {
        panic!("expected is");
    };
    let IsPattern::Positional(elems) = pattern else {
        panic!("expected positional");
    };
    assert!(matches!(&elems[0], PositionalSubpattern::Const(_)));
    assert!(matches!(&elems[1], PositionalSubpattern::Const(_)));

    let Stmt::Expr(e1) = &body.stmts[1].node else {
        panic!("expected second if");
    };
    let Expr::If { cond, .. } = &e1.node else {
        panic!("expected if expr");
    };
    let Expr::Is { pattern, .. } = &cond.node else {
        panic!("expected is");
    };
    let IsPattern::Positional(elems) = pattern else {
        panic!("expected positional");
    };
    assert!(matches!(&elems[0], PositionalSubpattern::Nested(_)));
    assert!(matches!(&elems[1], PositionalSubpattern::Discard));
}

#[test]
fn parse_nested_deconstruct_assign_m7() {
    // RFC 067 M7: `(a, (b, c)) = e;` -> DeconstructAssign with Nested
    let src = r#"
void Main() {
    int a;
    int b;
    int c;
    Pair p = new Pair(1, new Point(2, 3));
    (a, (b, c)) = p;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &program.items[0].node else {
        panic!("expected fn");
    };
    let body = f.body.as_ref().expect("body");
    let last = body.stmts.last().expect("stmt");
    let Stmt::DeconstructAssign {
        declare, targets, ..
    } = &last.node
    else {
        panic!("expected DeconstructAssign, got {:?}", last.node);
    };
    assert!(!declare);
    assert_eq!(targets.len(), 2);
    assert!(matches!(&targets[0], DeconstructTarget::Bind(Some(n)) if n.as_str() == "a"));
    match &targets[1] {
        DeconstructTarget::Nested(inner) => {
            assert_eq!(inner.len(), 2);
            assert!(matches!(&inner[0], DeconstructTarget::Bind(Some(n)) if n.as_str() == "b"));
            assert!(matches!(&inner[1], DeconstructTarget::Bind(Some(n)) if n.as_str() == "c"));
        }
        other => panic!("expected Nested, got {other:?}"),
    }
}

#[test]
fn property_pattern_rejected_m7_plus() {
    // RFC 067 M7+：属性模式立宪硬拒绝
    let src = r#"
void Main() {
    Point p = new Point(1, 2);
    if (p is { X: var x }) { }
}
"#;
    let err = Parser::parse_program(src).expect_err("property pattern must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("property pattern") || msg.contains("RFC 067"),
        "expected property-pattern rejection, got: {msg}"
    );
}

#[test]
fn parse_lock_statement() {
    // RFC 029 section 7.3: lock (expr) { body }
    let src = r#"
void Main() {
    lock (l) {
        x = 1;
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &program.items[0].node else {
        panic!("expected function item");
    };
    let body = f.body.as_ref().expect("body");
    match &body.stmts[0].node {
        Stmt::Lock { expr, body } => {
            assert!(matches!(&expr.node, Expr::Ident(n) if n.as_str() == "l"));
            assert_eq!(body.stmts.len(), 1);
        }
        other => panic!("expected Stmt::Lock, got {other:?}"),
    }
}

#[test]
fn lex_lock_keyword() {
    let tokens = parse::lex("lock (x) { }", 0).unwrap();
    assert_eq!(tokens[0].token, parse::Token::Lock);
}

#[test]
fn parse_operator_overload_maps_to_op_methods() {
    // RFC 003：operator + / unary - → op_Addition / op_UnaryNegation
    let src = r#"
struct Vec2 {
    public int X;
    public static Vec2 operator +(Vec2 a, Vec2 b) {
        return a;
    }
    public static Vec2 operator -(Vec2 a) {
        return a;
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Struct(s) = &program.items[0].node else {
        panic!("expected struct");
    };
    let names: Vec<&str> = s.methods.iter().map(|m| m.node.sig.name.as_str()).collect();
    assert!(
        names.contains(&"op_Addition"),
        "expected op_Addition, got {names:?}"
    );
    assert!(
        names.contains(&"op_UnaryNegation"),
        "expected op_UnaryNegation, got {names:?}"
    );
    assert!(s
        .methods
        .iter()
        .all(|m| m.node.sig.modifier == MethodModifier::Static));
}

#[test]
fn parse_operator_plus_eq_rejected() {
    let src = r#"
struct V {
    public static V operator +=(V a, V b) { return a; }
}
"#;
    let err = Parser::parse_program(src).expect_err("operator+= must fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("operator") || msg.contains("hard-rejected") || msg.contains("compound"),
        "got: {msg}"
    );
}

/// `(long)a * 16777216` 必须是 `((long)a) * 16777216`（Cast 只作用于 `a`），
/// 而非 `(long)(a * 16777216)`。修复前 Cast 操作数 min_bp=13 低于乘法
/// left_bp=19，乘法被 Cast 吞入——乘法在窄域（int）执行后高位符号扩展
/// 失真（barcode argb 打包错位根因）。
#[test]
fn parse_cast_precedence_binds_tighter_than_multiply() {
    let src = r#"void Main() {
    long v = (long)a * 16777216;
}"#;
    let program = Parser::parse_program(src).unwrap();
    let Item::Fn(f) = &program.items[0].node else {
        panic!("expected fn");
    };
    let body = f.body.as_ref().expect("fn body");
    let Stmt::Let { init, .. } = &body.stmts[0].node else {
        panic!("expected let stmt");
    };
    let init = init.as_ref().expect("init");
    let Expr::Binary {
        op: BinOp::Mul,
        left,
        right,
    } = &init.node
    else {
        panic!(
            "init must be Mul, got {:?}",
            std::mem::discriminant(&init.node)
        );
    };
    let _ = right; // right = 16777216 常量；左侧结构是断言重点。
    let Expr::Cast { ty, expr } = &left.node else {
        panic!(
            "left must be Cast, got {:?}",
            std::mem::discriminant(&left.node)
        );
    };
    let Type::Named { path, .. } = &ty.node else {
        panic!("cast target must be named type");
    };
    assert_eq!(path.last().unwrap().as_str(), "long");
    let Expr::Ident(name) = &expr.node else {
        panic!("cast operand must be ident");
    };
    assert_eq!(name.as_str(), "a");
}
