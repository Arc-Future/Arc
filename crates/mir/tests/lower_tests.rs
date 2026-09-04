use hir::HirBuilder;
use mir::*;
use parse::Parser;
use typeck::TypeChecker;

fn lower_source(src: &str) -> Vec<(String, MirCfgBody)> {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    lower_module(&fns, tc.registry(), &typeck::ExprTypeTable::default())
}

/// Count CondBr terminators across all CFG blocks ? these are lowered from
/// `MirStatement::If` and `MirStatement::While` in the CFG conversion.
fn count_cond_brs(body: &MirCfgBody) -> usize {
    body.blocks
        .values()
        .filter(|b| matches!(&b.terminator, MirTerminator::CondBr { .. }))
        .count()
}

#[test]
fn lower_linq_query() {
    let src = r#"
struct User { public int Age; public string Name; }
void demo(IEnumerable<User> users) {
    var q = from u in users where u.Age >= 18 select u.Name;
}
"#;
    let mir = lower_source(src);
    assert_eq!(mir.len(), 1);
}

#[test]
fn lower_async_await() {
    let src = r#"
async Task<int> fetch() { return 42; }
async Task<void> Main() {
    var v = await fetch();
    if (v == 42) { Console.WriteLine("42"); }
}
"#;
    let mir = lower_source(src);
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    assert!(main_body.is_async);
    assert!(main_body
        .blocks
        .values()
        .next()
        .unwrap()
        .statements
        .iter()
        .any(|s| { matches!(s, MirStatement::Await { .. }) }));
}

#[test]
fn lower_string_compare_builtin() {
    let src = r#"
void Main() {
    int cmp = string.Compare("a", "b");
}
"#;
    let mir = lower_source(src);
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    assert!(
        main_body
            .blocks
            .values()
            .next()
            .unwrap()
            .statements
            .iter()
            .any(|s| {
                matches!(
                    s,
                    MirStatement::Assign {
                        rvalue: MirRvalue::Call { func, .. },
                        ..
                    } if func == "string.Compare"
                )
            }),
        "expected string.Compare builtin call, got {:?}",
        main_body.blocks.values().next().unwrap().statements
    );
}

#[test]
fn lower_core_semantics() {
    let src = r#"
int add(int a, int b) { return a + b; }
void Main() {
    var x = add(1, 2);
    if (x > 0) {
        Console.WriteLine("yes");
    }
}
"#;
    let mir = lower_source(src);
    assert_eq!(mir.len(), 2);
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    let cond_br = main_body.blocks.values().find_map(|b| {
        if let MirTerminator::CondBr { cond, .. } = &b.terminator {
            Some(cond.clone())
        } else {
            None
        }
    });
    assert!(
        matches!(cond_br, Some(MirOperand::Local(_))),
        "if cond should be a bool local (CondBr terminator), got {cond_br:?}"
    );
}

fn lower_program(program: &ast::Program) -> Vec<(String, MirCfgBody)> {
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    lower_module(&fns, tc.registry(), &typeck::ExprTypeTable::default())
}

fn object_model_merged_program() -> ast::Program {
    // examples/ObjectModel removed; inline flat source for MIR regression.
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
    public int Area() { return Width * Height; }
}
public static class ShapeExtensions {
    public static void Describe(this IShape shape) { }
}
void PrintShapeInfo(IShape shape) {
    shape.Describe();
}
void Main() {
    var rectangle = new Rectangle(10, 20);
    PrintShapeInfo(rectangle);
}
"#;
    Parser::parse_program(src).unwrap()
}

#[test]
fn lower_extension_method_on_interface() {
    let program = object_model_merged_program();
    let mir = lower_program(&program);
    let print_body = mir
        .iter()
        .find(|(n, _)| n == "PrintShapeInfo")
        .expect("PrintShapeInfo")
        .1
        .clone();
    assert!(
        print_body
            .blocks
            .values()
            .next()
            .unwrap()
            .statements
            .iter()
            .any(|s| {
                matches!(
                    s,
                    MirStatement::Assign {
                        rvalue: MirRvalue::Call { func, .. },
                        ..
                    } if func == "ShapeExtensions::Describe"
                )
            }),
        "expected extension call ShapeExtensions::Describe, got {:?}",
        print_body.blocks.values().next().unwrap().statements
    );
}

#[test]
fn lower_oop_new_and_method() {
    let program = object_model_merged_program();
    let mir = lower_program(&program);
    assert!(mir.iter().any(|(n, _)| n == "__ctor::Rectangle_2"));
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    assert!(main_body
        .blocks
        .values()
        .next()
        .unwrap()
        .statements
        .iter()
        .any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::New { .. },
                    ..
                }
            )
        }));
}

#[test]
fn lower_var_new_array() {
    let src = r#"
void Main() {
    int[] v = [10, 20];
    int x = v[0];
}
"#;
    let mir = lower_source(src);
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    let v_ty = main_body
        .locals
        .iter()
        .find(|(_, (name, _))| name == "v")
        .map(|(_, (_, ty))| ty.clone())
        .expect("local v");
    assert!(
        matches!(v_ty, typeck::TypeId::Array { .. }),
        "MIR should preserve int[] for var v, got {v_ty:?}"
    );
}

#[test]
fn lower_lambda_to_func() {
    let src = r#"
void Main() {
    Func<int, int> f = x => x + 1;
}
"#;
    let mir = lower_source(src);
    assert!(
        mir.iter().any(|(n, _)| n == "__lambda_0"),
        "expected __lambda_0 in {:?}",
        mir.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    assert!(
        main_body
            .blocks
            .values()
            .next()
            .unwrap()
            .statements
            .iter()
            .any(|s| {
                matches!(
                    s,
                    MirStatement::Assign {
                        rvalue: MirRvalue::FnPtr { .. },
                        ..
                    }
                )
            }),
        "expected FnPtr assignment, got {:?}",
        main_body.blocks.values().next().unwrap().statements
    );
}

#[test]
fn lower_indirect_call() {
    let src = r#"
void Main() {
    Func<int, int> f = x => x + 1;
    int result = f(5);
}
"#;
    let mir = lower_source(src);
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    assert!(
        main_body
            .blocks
            .values()
            .next()
            .unwrap()
            .statements
            .iter()
            .any(|s| {
                matches!(
                    s,
                    MirStatement::Assign {
                        rvalue: MirRvalue::IndirectCall { .. },
                        ..
                    }
                )
            }),
        "expected IndirectCall, got {:?}",
        main_body.blocks.values().next().unwrap().statements
    );
}

#[test]
fn lower_delegate_invoke_method_to_indirect_call() {
    // C#：`f.Invoke(5)` ≡ `f(5)`；须降为 IndirectCall，禁止 MethodCall(unknown)。
    let src = r#"
void Main() {
    Func<int, int> f = x => x + 1;
    int result = f.Invoke(5);
}
"#;
    let mir = lower_source(src);
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    let stmts = &main_body.blocks.values().next().unwrap().statements;
    assert!(
        stmts.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::IndirectCall { .. },
                    ..
                }
            )
        }),
        "expected IndirectCall for f.Invoke, got {:?}",
        stmts
    );
    assert!(
        !stmts.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::MethodCall { method, .. },
                    ..
                } if method == "Invoke"
            )
        }),
        "f.Invoke must not remain MethodCall; got {:?}",
        stmts
    );
}

#[test]
fn lower_delegate_field_invoke_to_indirect_call() {
    // 实例 Func 字段 `_f(x)` 须 IndirectCall，禁止自由函数 Call(`_f`)。
    let src = r#"
class Holder {
    private Func<int, int> _f;
    public void Set(Func<int, int> f) { _f = f; }
    public int Run(int x) { return _f(x); }
}
void Main() {
    Holder h = new Holder();
    h.Set(x => x + 1);
    int r = h.Run(5);
}
"#;
    let mir = lower_source(src);
    let run_body = mir
        .iter()
        .find(|(n, _)| n.contains("Run"))
        .expect("Holder::Run")
        .1
        .clone();
    let stmts: Vec<_> = run_body
        .blocks
        .values()
        .flat_map(|b| b.statements.iter())
        .collect();
    assert!(
        stmts.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::IndirectCall { .. },
                    ..
                }
            )
        }),
        "expected IndirectCall for field _f(x), got {:?}",
        stmts
    );
    assert!(
        !stmts.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::Call { func, .. },
                    ..
                } if func == "_f" || func.ends_with("::_f")
            )
        }),
        "field invoke must not lower to Call(_f), got {:?}",
        stmts
    );
}

#[test]
fn lower_delegate_field_call_via_method_call_syntax() {
    // Parser: `this._factory()` → MethodCall；须降为 IndirectCall（Lazy<T>.Value）。
    let src = r#"
class Holder<T> {
    Func<T> _factory;
    public Holder(Func<T> f) { _factory = f; }
    public T Run() { return this._factory(); }
}
void Main() {
    Holder<int> h = new Holder<int>(() => 42);
    int x = h.Run();
}
"#;
    let mir = lower_source(src);
    let run_body = mir
        .iter()
        .find(|(n, _)| n.contains("Run"))
        .unwrap()
        .1
        .clone();
    let stmts = &run_body.blocks.values().next().unwrap().statements;
    assert!(
        stmts.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::IndirectCall { .. },
                    ..
                }
            )
        }),
        "expected IndirectCall for delegate field call, got {:?}",
        stmts
    );
    assert!(
        !stmts.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::MethodCall { method, .. },
                    ..
                } if method == "_factory"
            )
        }),
        "_factory must not remain MethodCall; got {:?}",
        stmts
    );
}

/// RFC 030 M2?object local ??? MIR Drop?v2 ????????
///
/// RFC 030 v2 ?6 ?? object ??? FFI marshal ????????????
/// object local ???????
///   - string ????rodata `@.str.N`????? ArcHeader?
///   - class ?????? ArcHeader?? object local ??? owner?
///   - FFI ??? ArcBox?? FFI ????? `rt_box_destroy` ???
///
/// ? object local ?? Drop ??? UB?rt_arc_dec ??? rodata???
/// `is_class_type(TypeId::Object)` ???? `false`?FFI ArcBox ????
/// ? RFC 027 ?15 ? `rt_box_destroy` ???????? MIR Drop?
#[test]
fn lower_object_local_no_drop() {
    let src = r#"
class Foo { public int Value; public Foo(int v) { this.Value = v; } }
void Main() {
    object o = new Foo(42);
    object n = null;
    object s = "hello";
    Console.WriteLine("ok");
}
"#;
    let mir = lower_source(src);
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    let has_drop = main_body.blocks.values().any(|b| {
        b.statements
            .iter()
            .any(|s| matches!(s, MirStatement::Drop(_)))
    });
    assert!(
        !has_drop,
        "object local should NOT participate in MIR Drop (v2 design: FFI marshal only, \
         rodata string literals would UB on rt_arc_dec), got statements: {:?}",
        main_body.blocks.values().next().unwrap().statements
    );
}

// ---------------------------------------------------------------------------
// ?????else if ????Block.tail ???
// ?? MIR lower_block ???? parser ? `else if` ????
// `Block { stmts: [], tail: Some(Expr::If { ... }) }` ???
// ---------------------------------------------------------------------------

#[test]
fn lower_else_if_chain_via_block_tail() {
    // `if (a) X; else if (b) Y; else Z;`  ? ??????
    // parser ????? if ? else_branch = Block { stmts: [], tail: Some(Expr::If { ... }) }
    // lower_block ??????? tail??? else if ??????
    let src = r#"
void Main(int a, int b) {
    string s = "";
    if (a > 0) { s = "a"; }
    else if (b > 0) { s = "b"; }
    else { s = "none"; }
    Console.WriteLine(s);
}
"#;
    let mir = lower_source(src);
    let main_body = mir
        .iter()
        .find(|(n, _)| n == "Main")
        .expect("Main")
        .1
        .clone();

    // ???main_body ?????? CondBr terminator??? if + else if?????
    let cond_count = count_cond_brs(&main_body);
    assert!(
        cond_count >= 2,
        "else if chain lowering: expected at least 2 CondBr terminators (outer + else-if), \
         got {cond_count}. Block.tail handling may be missing.\nBlocks: {:?}",
        main_body.blocks
    );
}

#[test]
fn lower_else_if_chain_no_braces_typed() {
    // ?? typed ???????????? enum_demo_e2e ??????
    // ???? + ???????? parser + typeck + lower ?????
    let src = r#"
void Main(int x) {
    string result = "";
    if (x < 0) result = "neg";
    else if (x == 0) result = "zero";
    else result = "pos";
    Console.WriteLine(result);
}
"#;
    let mir = lower_source(src);
    let main_body = mir
        .iter()
        .find(|(n, _)| n == "Main")
        .expect("Main")
        .1
        .clone();

    // typed path: ???? CondBr??? if?
    let cond_count = count_cond_brs(&main_body);
    assert!(
        cond_count >= 2,
        "typed else-if chain: expected at least 2 CondBr, got {cond_count}\nBlocks: {:?}",
        main_body.blocks
    );
}

#[test]
fn lower_nested_new_as_call_arg() {
    // Complex arg `new Outer(new Inner())` must materialize Inner (no silent null).
    let src = r#"
class Inner {
    public Inner() {}
}
class Outer {
    public Outer(Inner i) {}
}
void Main() {
    Outer o = new Outer(new Inner());
}
"#;
    let mir = lower_source(src);
    let main = mir.iter().find(|(n, _)| n == "Main").expect("Main");
    let has_new_inner = main.1.blocks.values().any(|b| {
        b.statements.iter().any(|s| matches!(
            s,
            MirStatement::Assign { rvalue: MirRvalue::New { class, .. }, .. } if class == "Inner"
        ))
    });
    assert!(
        has_new_inner,
        "Inner must be materialized as New, not silent null"
    );
}

#[test]
fn lower_generic_class_ctor_mono() {
    let src = r#"
class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
    public T Get() { return Value; }
}
void Main() {
    Box<int> b = new Box<int>(42);
}
"#;
    let mir = lower_source(src);
    let has_ctor = mir
        .iter()
        .any(|(n, _)| n == "__ctor::Box_int_1" || n == "__ctor::Box_int");
    assert!(
        has_ctor,
        "expected monomorphized Box_int ctor; got: {:?}",
        mir.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
}

/// Nested `new Inner<T>` inside `Outer<T>` ctor must mono both ctors (fixpoint).
#[test]
fn lower_nested_generic_ctor_in_ctor_body() {
    let src = r#"
class Inner<T> {
    public T V;
    public Inner(T v) { V = v; }
}
class Outer<T> {
    public Inner<T> Child;
    public Outer(T v) { Child = new Inner<T>(v); }
}
void Main() {
    Outer<int> o = new Outer<int>(7);
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names
            .iter()
            .any(|n| *n == "__ctor::Outer_int_1" || *n == "__ctor::Outer_int"),
        "expected Outer_int ctor; got: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| *n == "__ctor::Inner_int_1" || *n == "__ctor::Inner_int"),
        "expected Inner_int ctor from nested new in Outer ctor; got: {names:?}"
    );
}

/// Generic method body `new Box<T>` after mono must produce Box_int ctor.
#[test]
fn lower_generic_ctor_from_generic_method_body() {
    let src = r#"
class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
class Holder {
    public static Box<T> Make<T>(T v) { return new Box<T>(v); }
}
void Main() {
    Box<int> b = Holder.Make<int>(42);
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.iter().any(|n| n.contains("Make__")),
        "expected monomorphized Make__int; got: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| *n == "__ctor::Box_int_1" || *n == "__ctor::Box_int"),
        "expected Box_int ctor from generic method body; got: {names:?}"
    );
    let main = mir.iter().find(|(n, _)| n == "Main").expect("Main");
    let calls_mono = main.1.blocks.values().any(|b| {
        b.statements.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::Call { func, .. },
                    ..
                } if func.contains("Make__")
            )
        })
    });
    assert!(
        calls_mono,
        "Main must call Make__int, not the generic template; got: {:?}",
        main.1
            .blocks
            .values()
            .flat_map(|b| &b.statements)
            .collect::<Vec<_>>()
    );
    let make_mono = mir
        .iter()
        .find(|(n, _)| n.contains("Make__"))
        .expect("Make__int body");
    let has_new_box_int = make_mono.1.blocks.values().any(|b| {
        b.statements.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::New { class, .. },
                    ..
                } if class == "Box_int"
            )
        })
    });
    assert!(
        has_new_box_int,
        "Make__int body must new Box_int after substitute; got: {:?}",
        make_mono
            .1
            .blocks
            .values()
            .flat_map(|b| &b.statements)
            .collect::<Vec<_>>()
    );
}

/// Generic free function body `new Box<T>` after mono must produce Box_int ctor.
#[test]
fn lower_generic_ctor_from_generic_free_fn_body() {
    let src = r#"
class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
Box<T> MakeBox<T>(T v) { return new Box<T>(v); }
void Main() {
    Box<int> b = MakeBox<int>(42);
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"MakeBox_int"),
        "expected monomorphized MakeBox_int; got: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| *n == "__ctor::Box_int_1" || *n == "__ctor::Box_int"),
        "expected Box_int ctor; got: {names:?}"
    );
}

/// Nested type argument `Box<Box<int>>` must resolve templates (not silent miss).
#[test]
fn lower_nested_type_arg_generic_ctor() {
    let src = r#"
class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
void Main() {
    Box<Box<int>> b = new Box<Box<int>>(new Box<int>(1));
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names
            .iter()
            .any(|n| *n == "__ctor::Box_int_1" || *n == "__ctor::Box_int"),
        "expected Box_int ctor; got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("__ctor::Box_Box_int")),
        "expected Box_Box_int ctor; got: {names:?}"
    );
}

/// Instance generic method chain: `Wrap<T>` → `Leaf<T>` must mono both,
/// without emitting identity `Leaf__T` from the unbound template body.
#[test]
fn lower_instance_generic_method_chain() {
    let src = r#"
class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
class Chain {
    public Box<T> Leaf<T>(T v) { return new Box<T>(v); }
    public Box<T> Wrap<T>(T v) { return this.Leaf<T>(v); }
}
void Main() {
    Chain c = new Chain();
    Box<int> b = c.Wrap<int>(3);
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"Chain::Wrap__int"),
        "expected Wrap__int; got: {names:?}"
    );
    assert!(
        names.contains(&"Chain::Leaf__int"),
        "expected Leaf__int from nested instance call; got: {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|n| *n == "Chain::Leaf__T" || *n == "Chain::Wrap__T"),
        "must not emit identity mono Leaf__T/Wrap__T; got: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| *n == "__ctor::Box_int_1" || *n == "__ctor::Box_int"),
        "expected Box_int ctor from Leaf__int body; got: {names:?}"
    );
}

/// Instance generic method on a monomorphized generic class: `Mapper<int>.Map<U>`.
#[test]
fn lower_instance_generic_method_on_generic_class() {
    let src = r#"
class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
class Mapper<T> {
    public T Seed;
    public Mapper(T s) { Seed = s; }
    public Box<U> Map<U>(U u) { return new Box<U>(u); }
}
void Main() {
    Mapper<int> m = new Mapper<int>(1);
    Box<string> b = m.Map<string>("hi");
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.iter().any(|n| n.contains("Map__string")),
        "expected Mapper_int::Map__string; got: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| *n == "__ctor::Box_string_1" || *n == "__ctor::Box_string"),
        "expected Box_string ctor from Map__string body; got: {names:?}"
    );
    let main = mir.iter().find(|(n, _)| n == "Main").expect("Main");
    let calls_mono = main.1.blocks.values().any(|b| {
        b.statements.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::MethodCall { target_fn: Some(tfn), .. },
                    ..
                } if tfn.contains("Map__string")
            )
        })
    });
    assert!(
        calls_mono,
        "Main must call Map__string; got: {:?}",
        main.1
            .blocks
            .values()
            .flat_map(|b| &b.statements)
            .collect::<Vec<_>>()
    );
}

/// Nested field chain `b.Value.Value` must infer as the inner payload type so
/// static overload resolve picks `Equal_int` (not MethodCall on `Check`, and
/// not the string overload via wrong Named type / arity-first fallback).
#[test]
fn lower_nested_field_static_overload_by_inferred_type() {
    let src = r#"
class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
public static class Check {
    public static void Equal(int a, int b) { }
    public static void Equal(string a, string b) { }
}
void Main() {
    Box<Box<int>> b = new Box<Box<int>>(new Box<int>(1));
    Check.Equal(1, b.Value.Value);
}
"#;
    let mir = lower_source(src);
    let main = mir.iter().find(|(n, _)| n == "Main").expect("Main");
    let stmts: Vec<_> = main
        .1
        .blocks
        .values()
        .flat_map(|b| b.statements.iter())
        .collect();
    let equal_int = stmts.iter().any(|s| {
        matches!(
            s,
            MirStatement::Assign {
                rvalue: MirRvalue::Call { func, .. },
                ..
            } if func == "Check::Equal_int_int" || func == "Check::Equal_int"
        )
    });
    let as_method_on_check = stmts.iter().any(|s| {
        matches!(
            s,
            MirStatement::Assign {
                rvalue: MirRvalue::MethodCall { method, .. },
                ..
            } if method == "Equal"
        )
    });
    assert!(
        equal_int,
        "expected static Call Check::Equal_int*; got: {stmts:?}"
    );
    assert!(
        !as_method_on_check,
        "nested-field args must not lower Equal as instance MethodCall; got: {stmts:?}"
    );
}

#[test]
fn lower_array_index_set() {
    let src = r#"
void Main() {
    int[] md = [31, 28, 31];
    md[1] = 29;
    int v = md[1];
}
"#;
    let mir = lower_source(src);
    let main_body = mir.iter().find(|(n, _)| n == "Main").unwrap().1.clone();
    let stmts: Vec<_> = main_body
        .blocks
        .values()
        .flat_map(|b| b.statements.iter())
        .collect();
    let has_index_set = stmts.iter().any(|s| {
        matches!(
            s,
            MirStatement::IndexSet {
                elem_type: typeck::TypeId::Int,
                ..
            }
        )
    });
    assert!(
        has_index_set,
        "expected MirStatement::IndexSet for md[1]=29; got: {stmts:?}"
    );
}

/// Generic `Make<double>` → `new Signal<double>` must mono `__ctor::Signal_double_1`.
#[test]
fn lower_signal_double_ctor_from_generic_method() {
    let src = r#"
class Signal<T> {
    public T Value;
    public Signal(T v) { Value = v; }
}
class Holder {
    public static Signal<T> Make<T>(T v) { return new Signal<T>(v); }
}
void Main() {
    Signal<double> s = Holder.Make<double>(1.0);
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"__ctor::Signal_double_1"),
        "expected Signal_double ctor; got: {names:?}"
    );
}

#[test]
fn lower_element_setvalue_double_ctor() {
    let src = r#"
class Signal<T> {
    public T Value;
    public Signal(T v) { Value = v; }
}
class Element {
    public void SetValue<T>(T defaultValue, T value) {
        Signal<T> newSignal = new Signal<T>(defaultValue);
    }
}
void Main() {
    Element e = new Element();
    e.SetValue<double>(0.0, 16.0);
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"__ctor::Signal_double_1"),
        "expected Signal_double ctor from Element.SetValue<double>; got: {names:?}"
    );
}

#[test]
fn lower_element_setvalue_content_ctor() {
    let src = r#"
variant Content {
    | None
    | Text of string
}
class Signal<T> {
    public T Value;
    public Signal(T v) { Value = v; }
}
class Element {
    public void SetValue<T>(T defaultValue, T value) {
        Signal<T> newSignal = new Signal<T>(defaultValue);
    }
}
void Main() {
    Element e = new Element();
    e.SetValue<Content>(Content.None, Content.Text("Click"));
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"__ctor::Signal_Content_1"),
        "expected Signal_Content ctor from Element.SetValue<Content>; got: {names:?}"
    );
}

#[test]
fn lower_linq_orderby_sorts() {
    // 方法链形式（= 真实管线 desugar 后产物）：`OrderBy(n => n)` 触发
    // 排序物化：缓冲 List + `Sort(cmp)` + 升序 comparator。
    let src = r#"
void demo() {
    int[] nums = [5, 8, 3];
    int i = 0;
    int a = 0;
    foreach (var x in nums.OrderBy(n => n)) {
        if (i == 0) { a = x; }
        i = i + 1;
    }
}
"#;
    let mir = lower_source(src);
    let names: Vec<&str> = mir.iter().map(|(n, _)| n.as_str()).collect();
    let cmp = mir
        .iter()
        .find(|(n, _)| n.starts_with("__lambda_linq_cmp_"))
        .unwrap_or_else(|| panic!("expected lifted comparator; got: {names:?}"));
    assert_eq!(cmp.1.ret, typeck::TypeId::Int);
    assert_eq!(cmp.1.param_count, 2);

    let demo = mir
        .iter()
        .find(|(n, _)| n == "demo")
        .expect("demo")
        .1
        .clone();
    let stmts: Vec<_> = demo
        .blocks
        .values()
        .flat_map(|b| b.statements.iter())
        .collect();
    assert!(
        stmts.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::MethodCall { method, .. },
                    ..
                } if method == "Sort"
            )
        }),
        "expected List Sort method call; got: {stmts:?}"
    );
    assert!(
        stmts.iter().any(|s| {
            matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::MethodCall {
                        receiver: MirOperand::Local(_),
                        method,
                        args,
                        ..
                    },
                    ..
                } if method == "Sort"
                    && args.iter().any(|a| matches!(
                        a,
                        MirOperand::FnPtr { name }
                            if name.starts_with("__lambda_linq_cmp_")
                    ))
            )
        }),
        "Sort must pass the lifted comparator as FnPtr arg; got: {stmts:?}"
    );
}

// ---------------------------------------------------------------------------
// RFC 062 M3：按需 spill —— MirBody.spill_set 填充
// ---------------------------------------------------------------------------

/// 生成含 `n` 个 int 字段的 struct 声明（n×4B > 256B 触发 spill）。
fn big_struct_decl(n: usize) -> String {
    let fields: String = (0..n).map(|i| format!("    public int F{i};\n")).collect();
    format!("struct Big {{\n{fields}}}\n")
}

#[test]
fn lower_async_spill_large_struct_local() {
    let src = format!(
        r#"
{big}async Task<int> helper() {{ return 7; }}

async Task<void> F() {{
    var big = new Big();
    big.F0 = 100;
    big.F69 = 42;
    int a = await helper();
    int x = big.F0 + big.F69 + a;
}}
"#,
        big = big_struct_decl(70), // 70 × 4B = 280B > SPILL_THRESHOLD (256)
    );
    let mir = lower_source(&src);
    let f_body = mir
        .iter()
        .find(|(n, _)| n == "F")
        .expect("async F")
        .1
        .clone();
    assert!(f_body.is_async, "F must be async");
    let big_id = f_body
        .locals
        .iter()
        .find(|(_, (n, _))| n == "big")
        .map(|(id, _)| *id)
        .expect("local big");
    assert!(
        f_body.spill_set.contains(&(big_id.0 as usize)),
        "280B Big local crossing await must be spilled, spill_set={:?}",
        f_body.spill_set.spilled
    );
    // helper 无 >256B local → 不 spill。
    let helper_body = mir
        .iter()
        .find(|(n, _)| n == "helper")
        .expect("helper")
        .1
        .clone();
    assert!(
        helper_body.spill_set.spilled.is_empty(),
        "helper has no large local; must not spill, got {:?}",
        helper_body.spill_set.spilled
    );
}

#[test]
fn lower_async_no_spill_for_small_locals() {
    let src = r#"
async Task<int> helper() { return 7; }

async Task<void> F() {
    int a = 1;
    string s = "x";
    int b = await helper();
    int x = a + b;
}
"#;
    let mir = lower_source(src);
    let f_body = mir
        .iter()
        .find(|(n, _)| n == "F")
        .expect("async F")
        .1
        .clone();
    assert!(f_body.is_async, "F must be async");
    assert!(
        f_body.spill_set.spilled.is_empty(),
        "small locals (int/string) must not spill, got {:?}",
        f_body.spill_set.spilled
    );
}

#[test]
fn lower_sync_fn_never_spills() {
    let src = format!(
        r#"
{big}void F() {{
    var big = new Big();
    big.F0 = 1;
}}
"#,
        big = big_struct_decl(70),
    );
    let mir = lower_source(&src);
    let f_body = mir
        .iter()
        .find(|(n, _)| n == "F")
        .expect("sync F")
        .1
        .clone();
    assert!(!f_body.is_async, "F must be sync");
    assert!(
        f_body.spill_set.spilled.is_empty(),
        "sync functions never spill, got {:?}",
        f_body.spill_set.spilled
    );
}

// ---------------------------------------------------------------------------
// `a ?? b` 操作数物化：Coalesce 两侧须走 lower_arg_operand（builder-aware）
// ---------------------------------------------------------------------------

/// 回归：`culture ?? CultureInfo.Current`（Coalesce 右侧为**静态自定义属性**，
/// registry 中注册为 `get_Current` 方法而非字段）。
///
/// 修复前 Coalesce 落入 `lower_expr_to_rvalue_simple` 的 `operand_from_expr`
/// 叶子路径：静态自定义属性既非 const 也非静态字段 → 回退实例字段物化 →
/// receiver `Ident("CultureInfo")` 是类名非实例 → "unresolved ident" ICE
/// （typeck 已放行的合法表达式；std/Arc/Resources 下静态属性实测触发）。
#[test]
fn lower_coalesce_with_static_custom_property() {
    let src = r#"
class CultureInfo {
    private static CultureInfo? _current;

    public static CultureInfo Current {
        get {
            if (_current == null) {
                _current = new CultureInfo();
            }
            return _current;
        }
    }

    public CultureInfo() {
    }
}

class App {
    public static CultureInfo? Fallback() {
        return null;
    }

    public static void Main() {
        CultureInfo? culture = Fallback();
        CultureInfo c = culture ?? CultureInfo.Current;
    }
}
"#;
    let mir = lower_source(src);
    let main_body = mir
        .iter()
        .find(|(n, _)| n == "App::Main")
        .or_else(|| mir.iter().find(|(n, _)| n.ends_with("Main")))
        .expect("Main")
        .1
        .clone();
    let stmts: Vec<&MirStatement> = main_body
        .blocks
        .values()
        .flat_map(|b| b.statements.iter())
        .collect();
    // 静态 getter 须物化为 Call（右侧不再被误降为实例字段访问）。
    assert!(
        stmts.iter().any(|s| matches!(
            s,
            MirStatement::Assign {
                rvalue: MirRvalue::Call { func, .. },
                ..
            } if func.contains("get_Current")
        )),
        "static getter must be lowered to a Call, got: {stmts:?}"
    );
    // Coalesce 保留为显式 rvalue，两侧均为物化后的 operand。
    assert!(
        stmts.iter().any(|s| matches!(
            s,
            MirStatement::Assign {
                rvalue: MirRvalue::Coalesce { .. },
                ..
            }
        )),
        "coalesce rvalue must survive lowering, got: {stmts:?}"
    );
}

/// 回归：`类名.静态属性 = v`（自定义访问器 setter 赋值，如
/// `CultureInfo.CurrentUICulture = x`）。与 getter（见上）对称：
/// 修复前 Assign 的 `Expr::Field` 分支只有跨类静态字段（StaticFieldSet）与
/// 实例属性 set_* 派发，receiver 类名被误当表达式物化 →
/// "unresolved ident `CultureInfo`" ICE（typeck 已放行的合法语句）。
#[test]
fn lower_assign_to_static_custom_property() {
    let src = r#"
class CultureInfo {
    private static CultureInfo? _current;

    public static CultureInfo Current {
        get {
            if (_current == null) {
                _current = new CultureInfo();
            }
            return _current;
        }
        set {
            _current = value;
        }
    }

    public CultureInfo() {
    }
}

class App {
    public static void Main() {
        CultureInfo.Current = new CultureInfo();
    }
}
"#;
    let mir = lower_source(src);
    let main_body = mir
        .iter()
        .find(|(n, _)| n == "App::Main")
        .or_else(|| mir.iter().find(|(n, _)| n.ends_with("Main")))
        .expect("Main")
        .1
        .clone();
    let stmts: Vec<&MirStatement> = main_body
        .blocks
        .values()
        .flat_map(|b| b.statements.iter())
        .collect();
    // 静态 setter 须物化为无 this 的 Call（不得走 receiver 物化路径）。
    assert!(
        stmts.iter().any(|s| matches!(
            s,
            MirStatement::Assign {
                rvalue: MirRvalue::Call { func, args, .. },
                ..
            } if func.contains("set_Current") && args.len() == 1
        )),
        "static setter must be lowered to a receiverless Call, got: {stmts:?}"
    );
}

/// 回归：Coalesce 两侧均为方法调用（非叶子表达式）也须物化为临时 local，
/// 不回退 `operand_from_expr`（修复前同型 panic 路径）。
#[test]
fn lower_coalesce_with_call_operands() {
    let src = r#"
class App {
    public static string? Fallback() {
        return null;
    }

    public static string Make() {
        return "x";
    }

    public static void Main() {
        string? lhs = Fallback();
        string s = lhs ?? Make();
    }
}
"#;
    let mir = lower_source(src);
    let main_body = mir
        .iter()
        .find(|(n, _)| n.ends_with("Main"))
        .expect("Main")
        .1
        .clone();
    assert!(
        main_body
            .blocks
            .values()
            .flat_map(|b| b.statements.iter())
            .any(|s| matches!(
                s,
                MirStatement::Assign {
                    rvalue: MirRvalue::Coalesce { .. },
                    ..
                }
            )),
        "coalesce over call operands must lower without panic"
    );
}

/// 回归（ctor 重载表收集噪音）：含参 ctor 的类 + 泛型方法共存时，
/// `lower_module` 内部对 `__ctor::Class_N` 模板名不得再做方法泛型解析
/// （修复前以 owner=`__ctor` 查 registry → UndefinedType 噪音日志）。
#[test]
fn lower_ctor_template_names_skip_method_generic_lookup() {
    let src = r#"
class Widget {
    private int _v;

    public Widget(int v) {
        _v = v;
    }

    public Widget() {
        _v = 0;
    }

    public static T Pick<T>(T a) {
        return a;
    }
}

class App {
    public static void Main() {
        var w = new Widget(3);
        int x = Widget.Pick(7);
    }
}
"#;
    let mir = lower_source(src);
    assert!(
        mir.iter().any(|(n, _)| n.contains("__ctor::Widget")),
        "ctor bodies must be lowered, got: {:?}",
        mir.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()
    );
}
