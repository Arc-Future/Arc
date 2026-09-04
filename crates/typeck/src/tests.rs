use super::*;
use hir::HirBuilder;
use parse::Parser;

#[test]
fn check_async_return() {
    let src = r#"
async Task<int> fetch() {
    return 42;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

/// CD-30 批处理扩容（阶段 B·typeck 侧）：跨命名空间同名类的引用沿 FQN 路由。
///
/// `BatchX.Case1.Shape` 与 `BatchX.Case2.Shape` 同名碰撞：后注册者（Case2）占
/// 短名主索引 `types["Shape"]`（胜者，短名返回），先注册者（Case1）按 FQN 存于
/// `shadowed_types`（输家，FQN 返回）——使 MIR/codegen 最终沿 `classes[FQN]`
/// 分区解析，而非误落到短名胜者。
#[test]
fn check_cross_namespace_same_class_fqn_routing() {
    let src = r#"
namespace BatchX.Case1 {
    public class Shape { public int Side() { return 1; } }
    public Shape Make1() { return new Shape(); }
}
namespace BatchX.Case2 {
    public class Shape { public int Side() { return 2; } }
    public Shape Make2() { return new Shape(); }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    let ret_of = |name: &str| -> TypeId {
        fns.iter()
            .find(|f| f.name.as_str() == name)
            .unwrap_or_else(|| panic!("fn {name} missing"))
            .ret
            .clone()
    };
    // 碰撞输家（Case1）的 `Shape` 引用路由到 FQN。
    assert!(matches!(
        ret_of("Make1"),
        TypeId::Named(n) if n == "BatchX.Case1.Shape"
    ));
    // 碰撞胜者（Case2）维持短名。
    assert!(matches!(
        ret_of("Make2"),
        TypeId::Named(n) if n == "Shape"
    ));
}

#[test]
fn check_interface_oriented_subtyping() {
    let src = r#"
interface IShape {
    int Area();
}
class Rectangle : IShape {
    public int Width;
    public int Height;
    public int Area() { return 0; }
}
void useShape(IShape s) {
    var a = s.Area();
}
void Main() {
    useShape(new Rectangle());
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn check_lsp_rejects_bad_override() {
    let src = r#"
class Base {
    virtual int Value() { return 0; }
}
class Bad : Base {
    override string Value() { return "x"; }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn private_field_inaccessible_from_outside() {
    let src = r#"
class Box {
    private int Secret;
    public int GetSecret() { return this.Secret; }
}
void peek(Box b) {
    var x = b.Secret;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn void_fn_rejects_return_value() {
    let src = r#"
void Main() {
    return 1;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn async_defaults_to_task_void() {
    let src = r#"
class Console { public static void WriteLine(string message) { } }
async Task<void> work() {
    Console.WriteLine("done");
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    // RFC 004 M2：static 方法（如 `Console.WriteLine`）也被推入 `typed_fns`
    //（用于 `@int_Add` 等单态化符号路由）。不能假定 `fns[0]` 是 top-level 函数，
    // 需按 name 查找 `work`。
    let work = fns
        .iter()
        .find(|f| f.name == "work")
        .expect("work fn should be in typed_fns");
    assert!(matches!(work.ret, TypeId::Task { .. }));
}

#[test]
fn await_outside_async_fails() {
    let src = r#"
void Main() {
    await fetchValue();
}
async Task<int> fetchValue() { return 42; }
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn var_new_array_inference() {
    let src = r#"
void Main() {
    int[] v = [10, 20];
    int x = v[0];
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    let main = fns.iter().find(|f| f.name == "Main").unwrap();
    let let_stmt = &main.typed_body.as_ref().unwrap().stmts[0];
    match let_stmt {
        crate::TypedStmt::Let { ty, .. } => {
            assert!(matches!(ty, TypeId::Array { .. }));
        }
        _ => panic!("expected let"),
    }
}

#[test]
fn var_new_inferred_array() {
    let src = r#"
void Main() {
    var v = [10, 20];
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    let main = fns.iter().find(|f| f.name == "Main").unwrap();
    let let_stmt = &main.typed_body.as_ref().unwrap().stmts[0];
    match let_stmt {
        crate::TypedStmt::Let { ty, .. } => match ty {
            TypeId::Array { elem } => {
                assert_eq!(**elem, TypeId::Int);
            }
            _ => panic!("expected int[]"),
        },
        _ => panic!("expected let"),
    }
}

#[test]
fn reject_var_bare_brace_array() {
    let src = r#"
void Main() {
    var v = { 10, 20 };
}
"#;
    assert!(Parser::parse_program(src).is_err());
}

#[test]
fn var_struct_array_inference() {
    let src = r#"
struct User { public int Age; }
void Main() {
    User[] users = [new User() { Age = 1 }];
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    let main = fns.iter().find(|f| f.name == "Main").unwrap();
    let let_stmt = &main.typed_body.as_ref().unwrap().stmts[0];
    match let_stmt {
        crate::TypedStmt::Let { ty, .. } => match ty {
            TypeId::Array { elem } => {
                assert!(matches!(elem.as_ref(), TypeId::Named(n) if n == "User"));
            }
            _ => panic!("expected User[]"),
        },
        _ => panic!("expected let"),
    }
}

#[test]
fn check_generic_class_and_fn_monomorphization() {
    let src = r#"
class Box<T> {
    public T Value;
    public Box(T value) { Value = value; }
    public T Get() { return Value; }
}
T Identity<T>(T x) { return x; }
void Main() {
    Box<int> b = new Box<int>(42);
    int n = b.Get();
    int m = Identity<int>(n);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    assert!(fns.iter().any(|f| f.name == "Box_int::Get"));
    assert!(fns.iter().any(|f| f.name == "Identity_int"));
    assert!(tc.registry().get(&"Box_int".into()).is_some());
}

#[test]
fn generic_template_class_emits_ctor_fns_when_emit_fns_false() {
    // Pass 2 对泛型模板 `check_class_inner(..., emit_fns=false)` 仍须把
    // ctor 推入 typed_fns，供 MIR generate_generic_class_ctors 克隆 mono。
    let src = r#"
class Signal<T> {
    public T Value;
    public Signal() { Value = default(T); }
    public Signal(T initial) { Value = initial; }
}
class SignalFactory {
    public static Signal<T> Make<T>(T initial) { return new Signal<T>(initial); }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc.check_module(&module).unwrap();
    let ctors: Vec<String> = fns
        .iter()
        .filter(|f| f.name.as_str().starts_with("__ctor::Signal"))
        .map(|f| f.name.to_string())
        .collect();
    assert!(
        ctors.iter().any(|n| n == "__ctor::Signal"),
        "missing template __ctor::Signal; got {ctors:?}"
    );
    assert!(
        ctors.iter().any(|n| n == "__ctor::Signal_1"),
        "missing template __ctor::Signal_1; got {ctors:?}"
    );
    for name in ["__ctor::Signal", "__ctor::Signal_1"] {
        let tf = fns.iter().find(|f| f.name.as_str() == name).unwrap();
        assert!(
            tf.typed_body.is_some(),
            "{name} must have typed_body for MIR lower"
        );
    }
}

#[test]
fn nested_generic_instantiation_from_static_method_ctor_reads_instance_field() {
    // `instantiate_generic_class` 从 static 方法触发时，mono ctor 体须能读实例字段。
    let src = r#"
class Box<T> {
    T value;
    public Box(T v) {
        value = v;
        T copy = value;
    }
}
class Factory {
    public static Box<T> Make<T>(T v) { return new Box<T>(v); }
}
void Main() {
    Box<int> b = Factory.Make<int>(42);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
        .expect("ctor field read from static factory");
}

#[test]
fn delegate_field_call_via_method_call_syntax() {
    // Parser: `this._factory()` → MethodCall；须改写为 IndirectCall（Lazy<T>.Value）。
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
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
        .expect("delegate field call via MethodCall syntax");
}

#[test]
fn static_method_group_to_mangled_func_param() {
    // S0 根因 A：std OOP 签名把 `Action<...>`/`Func<...>` 参数以 mangled 名存储
    // （实例化后如 `Func_int_void` / `Func_int_int_bool`），期望类型为 mangled
    // Named 时静态方法组也须脱糖为 lambda（而非退回字段查找报「no field」）。
    let src = r#"
class Recorder {
    public static int Fired = 0;
    public static void OnChanged(int value) {
        Fired = Fired + 1;
    }
    public static bool OnChanging(int oldValue, int newValue) {
        return oldValue != newValue;
    }
}
class Box<T> {
    public int Subscribe(Action<T> handler) {
        return 0;
    }
    public int SubscribeChanging(Func<T, T, bool> handler) {
        return 0;
    }
}
void Main() {
    Box<int> b = new Box<int>();
    int a = b.Subscribe(Recorder.OnChanged);
    int c = b.SubscribeChanging(Recorder.OnChanging);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
        .expect("static method group to mangled Func/Action param");
}

#[test]
fn reject_mismatched_method_group_to_mangled_func_param() {
    // 方法与 mangled 委托签名不兼容（`OnChanged(int)` vs `Action<string>`）须拒绝。
    let src = r#"
class Recorder {
    public static void OnChanged(int value) { }
}
class Box<T> {
    public int Subscribe(Action<T> handler) {
        return 0;
    }
}
void Main() {
    Box<string> b = new Box<string>();
    int a = b.Subscribe(Recorder.OnChanged);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn composite_generic_delegate_arg_demangles_by_registry() {
    // S0 根因 A 补充：`Func<T,T,bool>` / `Action<T,T>` 在 T 为复合泛型（单态化名
    // 含 `_`，如 `ObservableCollection_int`）时，mangled 名
    // `Func_ObservableCollection_int_ObservableCollection_int_bool` 必须按注册表
    // 识别切分为 2 参 + bool 返回；否则委托调用 `handler(oldValue, newValue)` 退化
    // 为 void，`!handler(...)` 报「expected bool, found void」（Signal.TrySet 场景）。
    let src = r#"
class ObservableCollection<T> {
}
class Recorder {
    public static bool Changing(ObservableCollection<int> oldValue, ObservableCollection<int> newValue) {
        return true;
    }
}
class Box<T> {
    private Func<T, T, bool> _handler;
    public int OnChanging(Func<T, T, bool> handler) {
        _handler = handler;
        return 0;
    }
    public bool CanChange(T oldValue, T newValue) {
        Func<T, T, bool> handler = _handler;
        return handler(oldValue, newValue);
    }
}
void Main() {
    Box<ObservableCollection<int>> box = new Box<ObservableCollection<int>>();
    box.OnChanging(Recorder.Changing);
    bool ok = box.CanChange(new ObservableCollection<int>(), new ObservableCollection<int>());
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
        .expect("composite generic Func/Action arg demangles by registry");
}

#[test]
fn reject_generic_arity_mismatch() {
    let src = r#"
class Pair<T> {
    public T First;
    public Pair(T first) { First = first; }
}
void Main() {
    Pair<int, string> p = new Pair<int, string>(1);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn check_extension_method_call() {
    let src = r#"
interface IShape {
    string Name();
}
static class ShapeExtensions {
    public static void Describe(this IShape shape) { }
}
void demo(IShape s) {
    s.Describe();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
    let reg = tc.registry();
    let scope = crate::AccessContext {
        current_type: None,
        extension_scope: crate::ExtensionScope {
            imported: vec![],
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    assert!(reg
        .resolve_extension(&"IShape".into(), &"Describe".into(), 0, &[], &scope)
        .unwrap()
        .is_some());
}

#[test]
fn extension_method_requires_using() {
    let src = r#"
namespace App.Extensions {
    interface IShape {
        string Name();
    }
    static class ShapeExtensions {
        public static void Describe(this IShape shape) { }
    }
}
void demo(IShape s) {
    s.Describe();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn extension_method_with_using_ok() {
    let src = r#"
using App.Extensions;
namespace App.Extensions {
    interface IShape {
        string Name();
    }
    static class ShapeExtensions {
        public static void Describe(this IShape shape) { }
    }
}
void demo(IShape s) {
    s.Describe();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn extension_visible_in_same_namespace() {
    // 决策 #9：同命名空间内扩展方法始终可见，无需 using 导入。
    let src = r#"
namespace App {
    public static class FooExt {
        public static void Bar(this int x) { }
    }
    public class Consumer {
        public void Use() { 42.Bar(); }
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
    // 验证 42.Bar() 脱糖目标为 FooExt::Bar(42)：同命名空间 enclosing 可见。
    let reg = tc.registry();
    let scope = crate::AccessContext {
        current_type: None,
        extension_scope: crate::ExtensionScope {
            imported: vec![],
            enclosing: vec!["App".into()],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    let resolved = reg
        .resolve_extension(&"int".into(), &"Bar".into(), 0, &[], &scope)
        .unwrap();
    assert!(resolved.is_some());
    let resolution = resolved.unwrap();
    assert_eq!(resolution.container.as_str(), "FooExt");
}

#[test]
fn extension_visible_with_prefix_match() {
    // 决策 #9：using App; 前缀匹配 App.Extensions 命名空间，扩展方法可见。
    let src = r#"
using App;
namespace App.Extensions {
    public static class FooExt {
        public static void Bar(this int x) { }
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
    // 验证前缀匹配：using App 命中 App.Extensions 内的扩展容器 FooExt。
    let reg = tc.registry();
    let scope = crate::AccessContext {
        current_type: None,
        extension_scope: crate::ExtensionScope {
            imported: vec![vec!["App".into()]],
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    let resolved = reg
        .resolve_extension(&"int".into(), &"Bar".into(), 0, &[], &scope)
        .unwrap();
    assert!(resolved.is_some());
    let resolution = resolved.unwrap();
    assert_eq!(resolution.container.as_str(), "FooExt");
}

#[test]
fn extension_more_specific_receiver_wins() {
    // 决策 #8 规则 2：更具体的接收者类型优先。
    // CircleExt.Describe(this Circle) 与 ShapeExt.Describe(this IShape) 同时可见，
    // 调用 circle.Describe() 应选 CircleExt（Circle 是 IShape 的子类型，更具体）。
    let src = r#"
interface IShape {
    string Name();
}
class Circle : IShape {
    public string Name() { return "circle"; }
}
static class ShapeExt {
    public static string Describe(this IShape s) { return "shape"; }
}
static class CircleExt {
    public static string Describe(this Circle c) { return "circle"; }
}
void demo(Circle c) {
    var x = c.Describe();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
    let reg = tc.registry();
    let scope = crate::AccessContext {
        current_type: None,
        extension_scope: crate::ExtensionScope {
            imported: vec![],
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    let resolved = reg
        .resolve_extension(&"Circle".into(), &"Describe".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .expect("extension method should resolve");
    assert_eq!(resolved.container.as_str(), "CircleExt");
}

#[test]
fn extension_same_namespace_wins() {
    // 决策 #8 规则 1：同命名空间优先。
    // 两个扩展方法均作用于 int（接收者类型并列），一个在命名空间 App，一个在 Other。
    // 调用点 enclosing = App 时，应选 App 内的扩展，而非 Other 内的扩展。
    let src = r#"
namespace App {
    public static class SameNsExt {
        public static string Hello(this int x) { return "same"; }
    }
}
namespace Other {
    public static class OtherNsExt {
        public static string Hello(this int x) { return "other"; }
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
    let reg = tc.registry();
    // 同时导入 Other 命名空间，使两个候选均可见，触发歧义消解。
    let scope = crate::AccessContext {
        current_type: None,
        extension_scope: crate::ExtensionScope {
            imported: vec![vec!["Other".into()]],
            enclosing: vec!["App".into()],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    let resolved = reg
        .resolve_extension(&"int".into(), &"Hello".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .expect("extension method should resolve");
    assert_eq!(resolved.container.as_str(), "SameNsExt");
}

#[test]
fn extension_ambiguous_call() {
    // 决策 #8 规则 3：接收者类型并列且无同命名空间优势时，报 AmbiguousExtensionCall。
    // ReadWriter 同时实现 IReader 与 IWriter；两个扩展分别作用于 IReader/IWriter，
    // 互不为子类型，应报歧义错误。
    let src = r#"
interface IReader { }
interface IWriter { }
class ReadWriter : IReader, IWriter { }
static class ReaderExt {
    public static string Tag(this IReader r) { return "reader"; }
}
static class WriterExt {
    public static string Tag(this IWriter w) { return "writer"; }
}
void demo(ReadWriter rw) {
    var x = rw.Tag();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected AmbiguousExtensionCall for rw.Tag() with parallel receiver types"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(
            e,
            TypeError::Oop(ref s) if s.contains("ambiguous extension method call")
        )),
        "expected AmbiguousExtensionCall, got: {:?}",
        errs
    );
}

#[test]
fn generic_extension_identity() {
    // 决策 #7（RFC 010）：泛型扩展方法 `static T Id<T>(this T x)`。
    // 调用 `b.Id()`（b: Box）应解析到 BoxExt::Id_Box（mangled call_name），
    // 并触发单态化生成 BoxExt::Id_Box 方法体。
    // 注：基元类型（int/string）的扩展方法在 check_expr 路径暂不支持
    //（type_name_of 不覆盖 TypeId::Int），故使用 class 接收者验证完整链路。
    let src = r#"
class Box { public int Value; }
static class BoxExt {
    public static T Id<T>(this T x) { return x; }
}
void demo(Box b) {
    var y = b.Id();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
    let reg = tc.registry();
    let scope = crate::AccessContext {
        current_type: None,
        extension_scope: crate::ExtensionScope {
            imported: vec![],
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    let resolved = reg
        .resolve_extension(&"Box".into(), &"Id".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .expect("generic extension method should resolve");
    assert_eq!(resolved.container.as_str(), "BoxExt");
    assert_eq!(resolved.call_name, "BoxExt::Id_Box");
    assert_eq!(resolved.inferred_arg.as_deref(), Some("Box"));
    // 验证单态化方法体已 emit（typed_fns 包含 mangled 名）
    let _ = reg;
    assert!(
        tc.typed_fns
            .iter()
            .any(|f| f.name.as_str() == "BoxExt::Id_Box"),
        "expected monomorphized typed_fn `BoxExt::Id_Box` in typed_fns; got: {:?}",
        tc.typed_fns
            .iter()
            .map(|f| f.name.as_str().to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_extension_with_constraint() {
    // 决策 #7（RFC 010）：带 `where T : class` 约束的泛型扩展方法。
    // 引用类型 Widget 满足 class 约束，调用 `w.Tag()` 应解析到 WidgetExt::Tag_Widget。
    let src = r#"
class Widget { }
static class WidgetExt {
    public static string Tag<T>(this T x) where T : class { return "tag"; }
}
void demo(Widget w) {
    var y = w.Tag();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
    let reg = tc.registry();
    let scope = crate::AccessContext {
        current_type: None,
        extension_scope: crate::ExtensionScope {
            imported: vec![],
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    let resolved = reg
        .resolve_extension(&"Widget".into(), &"Tag".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .expect("generic extension method should resolve");
    assert_eq!(resolved.container.as_str(), "WidgetExt");
    assert_eq!(resolved.call_name, "WidgetExt::Tag_Widget");
    assert_eq!(resolved.inferred_arg.as_deref(), Some("Widget"));
}

#[test]
fn generic_extension_inference() {
    // 决策 #7（RFC 010）：同一泛型扩展方法对不同接收者类型产生不同的 mangled call_name。
    // int → IdExt::Id_int；string → IdExt::Id_string。
    let src = r#"
static class IdExt {
    public static T Id<T>(this T x) { return x; }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
    let reg = tc.registry();
    let scope = crate::AccessContext {
        current_type: None,
        extension_scope: crate::ExtensionScope {
            imported: vec![],
            enclosing: vec![],
        },
        current_package: None,
        enclosing_namespace: vec![],
        skip_type_visibility: false,
    };
    let int_res = reg
        .resolve_extension(&"int".into(), &"Id".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .expect("int extension should resolve");
    assert_eq!(int_res.call_name, "IdExt::Id_int");
    assert_eq!(int_res.inferred_arg.as_deref(), Some("int"));

    let str_res = reg
        .resolve_extension(&"string".into(), &"Id".into(), 0, &[], &scope)
        .expect("extension resolution should not error")
        .expect("string extension should resolve");
    assert_eq!(str_res.call_name, "IdExt::Id_string");
    assert_eq!(str_res.inferred_arg.as_deref(), Some("string"));
}

#[test]
fn string_length_and_equality() {
    let src = r#"
void demo() {
    string s = "hi";
    int n = s.Length;
    bool same = s == "hi";
    bool diff = s != "bye";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn byte_array_collection_target_ok() {
    let src = r#"
void demo() {
    byte[] src = [1, 2, 3, 4];
    int n = src.Length;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let r = tc.check_module(&module);
    assert!(r.is_ok(), "byte[] = [ints] should typeck: {:?}", r.err());
}

#[test]
fn string_char_index_ok() {
    let src = r#"
void demo() {
    string s = "hi";
    char c = s[0];
    char d = s[1];
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn string_char_index_assign_rejected() {
    let src = r#"
void demo() {
    string s = "hi";
    s[0] = 'x';
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn string_equality_rejects_mixed_types() {
    let src = r#"
void demo() {
    string s = "hi";
    bool bad = s == 1;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn method_overload_resolves_by_argument_types() {
    let src = r#"
class Printer {
    public void Print(int x) { }
    public void Print(string s) { }
    public int Show(int x) { return x; }
    public string Show(string s) { return s; }
    public Printer() { }
}
void Main() {
    Printer p = new Printer();
    p.Print(1);
    p.Print("a");
    int n = p.Show(2);
    string t = p.Show("b");
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn method_overload_rejects_no_match() {
    let src = r#"
class Printer {
    public void Print(int x) { }
    public void Print(string s) { }
}
void Main() {
    Printer p = new Printer();
    p.Print(true);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn check_lambda_target_inference() {
    let src = r#"
void Main() {
    Func<int, int> f = x => x + 1;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn check_func_value_call() {
    let src = r#"
void Main() {
    Func<int, int> f = x => x + 1;
    int result = f(5);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn class_constraint_violation() {
    // `where T : class` 要求 T 为引用类型；int 是值类型，
    // 实例化 Foo<int>（此处作为 Bar 字段类型触发 lower_type）
    // 应报 ConstraintNotSatisfied。
    let src = r#"
class Foo<T> where T : class {
    public T Value;
}
class Bar {
    public Foo<int> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Foo<int> under `where T : class`"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

#[test]
fn struct_constraint_violation() {
    // `where T : struct` 要求 T 为值类型；string 是引用类型，
    // 实例化 Foo<string>（此处作为 Bar 字段类型触发 lower_type）
    // 应报 ConstraintNotSatisfied。
    let src = r#"
class Foo<T> where T : struct {
    public T Value;
}
class Bar {
    public Foo<string> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Foo<string> under `where T : struct`"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

// ===== P1: new() 构造约束 =====

#[test]
fn new_constraint_satisfied() {
    // `where T : new()` 要求 T 有 public 无参构造；
    // Bar 显式声明 `public Bar()`，实例化 Foo<Bar> 应通过。
    // 作为 Baz 字段类型触发 lower_type → check_constraints。
    let src = r#"
class Foo<T> where T : new() {
    public T Value;
}
class Bar {
    public Bar() {}
}
class Baz {
    public Foo<Bar> Good;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected Foo<Bar> to satisfy `where T : new()`"
    );
}

#[test]
fn new_constraint_violated_no_ctor() {
    // Bar 仅有带参构造 `public Bar(int x)`，无 public 无参构造；
    // 实例化 Foo<Bar> 应报 ConstraintNotSatisfied。
    let src = r#"
class Foo<T> where T : new() {
    public T Value;
}
class Bar {
    public Bar(int x) {}
}
class Baz {
    public Foo<Bar> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Foo<Bar> under `where T : new()` (no parameterless ctor)"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

#[test]
fn new_constraint_violated_private_ctor() {
    // Bar 的无参构造为 private，不满足 new() 的 public 要求；
    // 实例化 Foo<Bar> 应报 ConstraintNotSatisfied。
    let src = r#"
class Foo<T> where T : new() {
    public T Value;
}
class Bar {
    private Bar() {}
}
class Baz {
    public Foo<Bar> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Foo<Bar> under `where T : new()` (private ctor)"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

// ===== P2: 多约束组合 `where T : A, B` =====

#[test]
fn multi_constraint_type_only() {
    // `where T : IComparable<T>, IEquatable<T>` 用 int 实例化；
    // int 天然满足两个内置接口（基元满足规则），应通过。
    let src = r#"
class Foo<T> where T : IComparable<T>, IEquatable<T> {
    public T Value;
}
class Baz {
    public Foo<int> Good;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected Foo<int> to satisfy `where T : IComparable<T>, IEquatable<T>`"
    );
}

#[test]
fn multi_constraint_with_new() {
    // `where T : class, new()` —— T 须同时为引用类型且有 public 无参构造；
    // Bar 是 class 且有 `public Bar()`，实例化 Foo<Bar> 应通过。
    let src = r#"
class Foo<T> where T : class, new() {
    public T Value;
}
class Bar {
    public Bar() {}
}
class Baz {
    public Foo<Bar> Good;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected Foo<Bar> to satisfy `where T : class, new()`"
    );
}

#[test]
fn multi_constraint_violation() {
    // `where T : class, new()` —— Bar 是 class 但构造为 private；
    // new() 不满足，实例化 Foo<Bar> 应报 ConstraintNotSatisfied。
    let src = r#"
class Foo<T> where T : class, new() {
    public T Value;
}
class Bar {
    private Bar() {}
}
class Baz {
    public Foo<Bar> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Foo<Bar> under `where T : class, new()` (private ctor)"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

#[test]
fn constraint_batch_reports_all_violations() {
    // DiagnosticBag 错误恢复语义：双参数同时违约时，一次报告全部违约
    // （A、B 各自的 ConstraintNotSatisfied 均入池），而非仅首个——
    // fail-fast 会让用户按「修一个报下一个」逐次循环修复。
    let src = r#"
class Pair<A, B> where A : class where B : class {
    public A First;
    public B Second;
}
class Holder {
    public Pair<int, int> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected constraint violations for Pair<int, int> under `where A : class where B : class`"
    );
    let errs = result.unwrap_err();
    // 内容级 + 计数级断言：A 与 B 的违约各自呈现且恰好一次。层 1 负缓存
    // （violated）与层 2 收敛去重（take_errors_deduped）落地后，同一条
    // 违约不再因同声明点的多次 lower 而重复入池——原「用 any 而非计数」
    // 的妥协断言随诊断去重管道交付升级为计数断言。
    let count_a = errs
        .iter()
        .filter(|e| match e {
            TypeError::ConstraintNotSatisfied { param, bound, .. } => {
                param.as_str() == "A" && bound.as_str() == "class"
            }
            _ => false,
        })
        .count();
    let count_b = errs
        .iter()
        .filter(|e| match e {
            TypeError::ConstraintNotSatisfied { param, bound, .. } => {
                param.as_str() == "B" && bound.as_str() == "class"
            }
            _ => false,
        })
        .count();
    assert_eq!(
        count_a, 1,
        "expected exactly one ConstraintNotSatisfied for param A (class), got: {:?}",
        errs
    );
    assert_eq!(
        count_b, 1,
        "expected exactly one ConstraintNotSatisfied for param B (class), got: {:?}",
        errs
    );
}

#[test]
fn negative_cache_constraint_detail_reported_once() {
    // 层 1 负缓存语义验证：违约实例化点不进正缓存（instantiated），同
    // mangled 类型再次引用时 registry 查不到会重触达单态化入口。入口负
    // 缓存命中即短路返回缓存哨兵——违约明细（ConstraintNotSatisfied）只
    // 随首次 check_constraints 入池，重触达零重复。无负缓存时 Bad2 的
    // 重触达会重跑检查，本断言计数为 2。
    let src = r#"
class Foo<T> where T : class {
    public T Value;
}
class Holder {
    public Foo<int> Bad1;
    public Foo<int> Bad2;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected constraint violation for Foo<int> under `where T : class`"
    );
    let errs = result.unwrap_err();
    let detail_count = errs
        .iter()
        .filter(|e| match e {
            TypeError::ConstraintNotSatisfied { param, bound, .. } => {
                param.as_str() == "T" && bound.as_str() == "class"
            }
            _ => false,
        })
        .count();
    assert_eq!(
        detail_count, 1,
        "constraint detail must be reported exactly once (negative cache), got: {:?}",
        errs
    );
}

#[test]
fn take_errors_deduped_preserves_first_occurrence_order() {
    // 层 2 收敛出口语义验证：内容级去重（全等重复保留首次出现）+ 保序
    // （不排序，非全等项相对次序不变）。TypeError 已 derive
    // Clone/PartialEq/Eq/Hash，支撑负缓存存取与 IndexSet 去重。
    let mut tc = TypeChecker::new();
    tc.errors.push(TypeError::Undefined("a".into()));
    tc.errors.push(TypeError::Undefined("b".into()));
    tc.errors.push(TypeError::Undefined("a".into()));
    tc.errors
        .push(TypeError::ConstraintBatchViolated { count: 2 });
    tc.errors.push(TypeError::Undefined("b".into()));
    let deduped = tc.take_errors_deduped();
    assert_eq!(
        deduped,
        vec![
            TypeError::Undefined("a".into()),
            TypeError::Undefined("b".into()),
            TypeError::ConstraintBatchViolated { count: 2 },
        ],
        "dedupe must keep first-occurrence order and drop exact duplicates"
    );
    // 收敛出口消费后错误池清空（与裸 mem::take 出口语义一致）。
    assert!(tc.errors.is_empty());
}

#[test]
fn constraint_batch_type_bounds_all_reported() {
    // 同 param 双 Type bound（`where T : Req1 where T : Req2`）：int 对两个
    // bound 均违约，两条违约均应入池——批量收集覆盖同 param 多 bound 形态。
    let src = r#"
class Req1 {
}
class Req2 {
}
class Cell<T> where T : Req1 where T : Req2 {
    public T Value;
}
class Holder {
    public Cell<int> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected constraint violations for Cell<int> under `where T : Req1 where T : Req2`"
    );
    let errs = result.unwrap_err();
    let reports_req1 = errs.iter().any(|e| match e {
        TypeError::ConstraintNotSatisfied { bound, .. } => bound.as_str() == "Req1",
        _ => false,
    });
    let reports_req2 = errs.iter().any(|e| match e {
        TypeError::ConstraintNotSatisfied { bound, .. } => bound.as_str() == "Req2",
        _ => false,
    });
    assert!(
        reports_req1,
        "expected ConstraintNotSatisfied bound Req1, got: {:?}",
        errs
    );
    assert!(
        reports_req2,
        "expected ConstraintNotSatisfied bound Req2, got: {:?}",
        errs
    );
}

#[test]
fn delegate_constraint_satisfied() {
    // `where T : class` 于泛型委托：string 满足引用类型约束，
    // 实例化点 lambda 赋值应通过。
    let src = r#"
delegate R Map<T, R>(T x) where T : class;

void Main() {
    Map<string, int> lengthOf = s => s.Length;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected Map<string, int> to satisfy `where T : class`"
    );
}

#[test]
fn delegate_constraint_violation() {
    // `where T : class` 于泛型委托：int 是值类型，字段实例化点
    // Map<int, int> 应报 ConstraintNotSatisfied。
    let src = r#"
delegate R Map<T, R>(T x) where T : class;

class Holder {
    public Map<int, int> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Map<int, int> under `where T : class`"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

#[test]
fn delegate_multi_where_satisfied() {
    // 多 where 段于泛型委托：T=string（class）+ U=int（struct）均满足。
    let src = r#"
delegate U Pick<T, U>(T x) where T : class where U : struct;

void Main() {
    Pick<string, int> p = s => s.Length;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected Pick<string, int> to satisfy `where T : class where U : struct`"
    );
}

#[test]
fn delegate_multi_where_violation() {
    // 多 where 段于泛型委托：U=string 违反 `where U : struct`。
    let src = r#"
delegate U Pick<T, U>(T x) where T : class where U : struct;

class Holder {
    public Pick<int, string> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Pick<int, string> under `where U : struct`"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

#[test]
fn delegate_new_constraint_satisfied() {
    // `where T : new()` 于泛型委托：Widget 有 public 无参构造，应通过。
    let src = r#"
delegate T Create<T>() where T : new();

class Widget {
    public Widget() {}
}

class Holder {
    public Create<Widget> Good;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected Create<Widget> to satisfy `where T : new()`"
    );
}

#[test]
fn delegate_new_constraint_violation() {
    // `where T : new()` 于泛型委托：Sealed 构造为 private，应报
    // ConstraintNotSatisfied。
    let src = r#"
delegate T Create<T>() where T : new();

class Sealed {
    private Sealed() {}
}

class Holder {
    public Create<Sealed> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Create<Sealed> under `where T : new()` (private ctor)"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

#[test]
fn delegate_where_undefined_param() {
    // 非泛型委托携带 where 子句：约束 param 未在泛型参数表中声明，
    // 声明期应报 UndefinedTypeParameter（C# CS0081 语义）。
    let src = r#"
delegate int F(int x) where T : class;
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected UndefinedTypeParameter for non-generic delegate with where clause"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::UndefinedTypeParameter(_))),
        "expected UndefinedTypeParameter, got: {:?}",
        errs
    );
}

#[test]
fn class_constraint_rejects_enum() {
    // is_reference_type 语义边界：enum 是值类型，`where T : class`
    // 必须拒绝 enum 实参（C# 语义；修复前 enum 会漏过 class 约束）。
    let src = r#"
enum Color { Red = 0, Green = 1 }

class Foo<T> where T : class {
    public T Value;
}
class Baz {
    public Foo<Color> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Foo<Color> under `where T : class` (enum is value type)"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

#[test]
fn class_constraint_rejects_variant() {
    // is_reference_type 语义边界：variant 是栈上标签联合（值语义），
    // `where T : class` 必须拒绝 variant 实参。
    let src = r#"
variant Shape { | Circle of int | Null }

class Foo<T> where T : class {
    public T Value;
}
class Baz {
    public Foo<Shape> Bad;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected ConstraintNotSatisfied for Foo<Shape> under `where T : class` (variant is value type)"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, TypeError::ConstraintNotSatisfied { .. })),
        "expected ConstraintNotSatisfied, got: {:?}",
        errs
    );
}

// ===== RFC 009 M4-1：abstract class 语义检查 =====

#[test]
fn abstract_class_rejects_direct_instantiation() {
    // `abstract class Base` 不可直接 `new Base()`——typeck 在 Expr::New 检查
    // NominalType.is_abstract 字段，禁止直接实例化。
    let src = r#"
abstract class Base {
    public Base() {}
}
class C {
    void M() {
        var b = new Base();
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected error for `new Base()` where Base is abstract class"
    );
    let errs = result.unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(
            e,
            TypeError::Oop(ref msg) if msg.contains("abstract class")
        )),
        "expected Oop error mentioning abstract class, got: {:?}",
        errs
    );
}

#[test]
fn abstract_class_allows_derived_instantiation() {
    // 派生类 `new Derived()` 合法——派生类不是 abstract，可直接实例化。
    let src = r#"
abstract class Base {
    public Base() {}
}
class Derived : Base {
    public Derived() {}
}
class C {
    void M() {
        var d = new Derived();
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected `new Derived()` to be valid where Derived extends abstract Base"
    );
}

#[test]
fn abstract_class_parser_recognizes_keyword() {
    // 验证 parser 正确识别 `abstract class` 上下文关键字——
    // `abstract` 不是专用 Token，而是上下文关键字（match_ident_keyword 模式）。
    // 解析后 ClassDef.is_abstract 应为 true；typeck 注册时 NominalType.is_abstract
    // 应传播该字段。通过仅声明 abstract class 不实例化验证字段传播无错误。
    let src = r#"
abstract class AbstractMarker {
    public AbstractMarker() {}
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    // 仅声明 abstract class 不实例化——应通过（无错误）。
    assert!(
        tc.check_module(&module).is_ok(),
        "expected abstract class declaration to typeck successfully"
    );
}

#[test]
fn covariance_out_param_assignment_ok() {
    let src = r#"
class Animal {}
class Dog : Animal {}
interface IGetter<out T> {
    T Get();
}
class DogBox : IGetter<Dog> {
    public Dog Get() { return new Dog(); }
}
void Main() {
    IGetter<Dog> dogs = new DogBox();
    IGetter<Animal> animals = dogs;
    Animal a = animals.Get();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let fns = tc
        .check_module(&module)
        .expect("expected covariant assignment");
    let names: Vec<_> = fns.iter().map(|f| f.name.as_str()).collect();
    assert!(
        names
            .iter()
            .any(|n| n.contains("DogBox") && n.contains("Get")),
        "expected DogBox::Get in typed_fns, got {names:?}"
    );
}

#[test]
fn covariance_out_in_input_position_rejected() {
    let src = r#"
interface IBad<out T> {
    void Set(T value);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let err = tc.check_module(&module).unwrap_err();
    assert!(
        err.iter()
            .any(|e| matches!(e, TypeError::InvalidVariance { .. })),
        "expected InvalidVariance, got {err:?}"
    );
}

#[test]
fn contravariance_in_on_output_rejected() {
    let src = r#"
interface IBad<in T> {
    T Get();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let err = tc.check_module(&module).unwrap_err();
    assert!(
        err.iter()
            .any(|e| matches!(e, TypeError::InvalidVariance { .. })),
        "expected InvalidVariance, got {err:?}"
    );
}

#[test]
fn contravariance_in_param_assignment_ok() {
    let src = r#"
interface IAnimal {}
class Dog : IAnimal {}
interface IConsumer<in T> {
    void Take(T value);
}
class AnimalConsumer : IConsumer<IAnimal> {
    public void Take(IAnimal a) {}
}
void Main() {
    IConsumer<IAnimal> wide = new AnimalConsumer();
    IConsumer<Dog> narrow = wide;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected contravariant assignment"
    );
}

#[test]
fn nested_where_ok() {
    let src = r#"
interface IBag<U> {}
class Repo<T, U> where T : IBag<U> where U : class {
    public Repo() {}
}
class Item {}
class Bag : IBag<Item> {}
void Main() {
    Repo<Bag, Item> r = new Repo<Bag, Item>();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected nested where to typeck"
    );
}

#[test]
fn nested_where_undefined_param_rejected() {
    let src = r#"
interface IBag<U> {}
class Bad<T> where T : IBag<U> {
    public Bad() {}
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let err = tc.check_module(&module).unwrap_err();
    assert!(
        err.iter()
            .any(|e| matches!(e, TypeError::UndefinedTypeParameter(_))),
        "expected UndefinedTypeParameter, got {err:?}"
    );
}

#[test]
fn variance_not_allowed_on_class() {
    let src = r#"
class Box<out T> {
    public T Value;
}
"#;
    assert!(
        Parser::parse_program(src).is_err(),
        "expected parse error for out on class type parameter"
    );
}

#[test]
fn covariance_nested_out_ienumerable_ok() {
    let src = r#"
interface IEnumerator<out T> {
    bool MoveNext();
    T Current { get; }
}
interface IEnumerable<out T> {
    IEnumerator<T> GetEnumerator();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    match tc.check_module(&module) {
        Ok(_) => {}
        Err(e) => panic!("expected IEnumerable<out T> nested covariance, got {e:?}"),
    }
}

#[test]
fn covariance_ienumerable_impl_and_assign() {
    let src = r#"
interface IEnumerator<out T> {
    bool MoveNext();
    T Current { get; }
}
interface IEnumerable<out T> {
    IEnumerator<T> GetEnumerator();
}
interface IAnimal { int Id(); }
class Dog : IAnimal { public int Id() { return 42; } }
class DogEnumerator : IEnumerator<Dog> {
    private int _state;
    public DogEnumerator() { _state = 0; }
    public bool MoveNext() {
        if (_state == 0) { _state = 1; return true; }
        return false;
    }
    public Dog Current { get { return new Dog(); } }
}
class DogSeq : IEnumerable<Dog> {
    public IEnumerator<Dog> GetEnumerator() { return new DogEnumerator(); }
}
void Main() {
    IEnumerator<Dog> e = new DogEnumerator();
    IEnumerable<Dog> dogs = new DogSeq();
    IEnumerable<IAnimal> animals = dogs;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    match tc.check_module(&module) {
        Ok(_) => {}
        Err(e) => panic!("expected IEnumerable impl+assign, got {e:?}"),
    }
}

#[test]
fn covariance_nested_invariant_rejected() {
    let src = r#"
interface IBox<T> { T Get(); void Set(T value); }
interface IBad<out T> { IBox<T> GetBox(); }
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let err = tc.check_module(&module).unwrap_err();
    assert!(
        err.iter()
            .any(|e| matches!(e, TypeError::InvalidVariance { .. })),
        "expected InvalidVariance for out T inside invariant nested generic, got {err:?}"
    );
}

/// RFC 005 项 3：数组元素 invariant——拒 C# `Dog[] → Animal[]` 危险协变。
#[test]
fn array_elem_invariant_rejects_dog_to_animal() {
    let src = r#"
class Animal { public Animal() {} }
class Dog : Animal { public Dog() {} }
void Main() {
    Dog[] dogs = [new Dog()];
    Animal[] animals = dogs;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_err(),
        "Dog[] must not assign to Animal[] (array element invariant)"
    );
}

#[test]
fn array_elem_invariant_same_type_ok() {
    let src = r#"
class Dog { public Dog() {} }
void Main() {
    Dog[] a = [new Dog()];
    Dog[] b = a;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "Dog[] assign to Dog[] must succeed"
    );
}

// ===== 三元条件表达式 `cond ? then : else` 类型检查测试 =====

#[test]
fn ternary_basic_typeck() {
    let src = r#"
void Main() {
    int a = true ? 10 : 20;
    string s = false ? "yes" : "no";
    int b = 5 > 3 ? 100 : 200;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn ternary_non_bool_cond_rejected() {
    let src = r#"
void Main() {
    int a = 5 ? 10 : 20;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn ternary_mismatched_branches_rejected() {
    let src = r#"
void Main() {
    int a = true ? 10 : "hello";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_err());
}

#[test]
fn ternary_nested_typeck() {
    let src = r#"
void Main() {
    int a = 10;
    int b = a < 5 ? 1 : a < 15 ? 2 : 3;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn ternary_in_call_arg_typeck() {
    let src = r#"
void Main() {
    int x = 42;
    Console.WriteLine(x > 0 ? "positive" : "non-positive");
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

// ── 复杂三元类型检查场景 ──

#[test]
fn ternary_return_statement_typeck() {
    // 来自 Version.as: return a > b ? 1 : -1;
    let src = r#"
int compare(int a, int b) {
    return a > b ? 1 : -1;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn ternary_multiple_assignments_typeck() {
    // 来自 Version.as: 连续多个三元赋值
    let src = r#"
int parseInt(string s) { return 0; }
void parse(string[] parts, int count) {
    int ma = count >= 1 ? parseInt(parts[0]) : 0;
    int mi = count >= 2 ? parseInt(parts[1]) : 0;
    int bu = count >= 3 ? parseInt(parts[2]) : 0;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn ternary_in_arithmetic_typeck() {
    // 来自 DateTime.as: days += IsLeapYear(i) ? 366 : 365
    let src = r#"
bool IsLeapYear(int y) { return false; }
void compute() {
    int days = 0;
    int i = 2000;
    days = days + (IsLeapYear(i) ? 366 : 365);
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn ternary_deep_nesting_typeck() {
    // 3 层嵌套类型检查（使用 bool 条件）
    let src = r#"
void demo() {
    int x = true ? 2 : false ? 4 : true ? 6 : 7;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn ternary_generic_condition_typeck() {
    // 验证 bool 变量作为条件
    let src = r#"
void demo(bool flag) {
    int val = flag ? 100 : 200;
    string msg = flag ? "ok" : "fail";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

// ============================================================
// RFC 004 §D9 / RFC 037 M2：隐式 variant 构造测试
//
// 验证 typeck 在 types_compatible 失败时自动将值包装为
// `Variant.Case(value)` 的 AST 重写。覆盖：
//   - let 初始化（string / int / class 类型）
//   - property setter 赋值
//   - 方法参数传递
//   - return 语句
//   - 歧义拒绝（多 case 同 payload 类型）
// ============================================================

#[test]
fn variant_implicit_let_init_string() {
    // `Variant v = "Click";` → 自动重写为 `Variant.Text("Click")`
    let src = r#"
variant Content {
    | Text of string
    | Element of int
}
void Main() {
    Content c = "Click";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected implicit variant coercion for string"
    );
}

#[test]
fn variant_implicit_let_init_int() {
    // `Variant v = 42;` → 自动重写为 `Variant.Int(42)`
    let src = r#"
variant Value {
    | Int of int
    | Str of string
}
void Main() {
    Value v = 42;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected implicit variant coercion for int"
    );
}

#[test]
fn variant_implicit_let_init_class() {
    // 类类型 payload 隐式构造（模拟 ContentVariant.Element(button)）
    let src = r#"
class Button {}
variant Content {
    | Text of string
    | Element of Button
}
void Main() {
    Content c = new Button();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected implicit variant coercion for class type"
    );
}

#[test]
fn variant_implicit_property_setter() {
    // `obj.Prop = "Click";` → setter 形参为 Variant 时自动包装
    let src = r#"
variant Content {
    | Text of string
    | Element of int
}
class Button {
    private Content _content;
    public Content Content {
        get { return _content; }
        set { _content = value; }
    }
}
void Main() {
    Button b = new Button();
    b.Content = "Click";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected implicit variant coercion in property setter"
    );
}

#[test]
fn variant_implicit_method_param() {
    // 方法形参为 Variant，传入 string 自动包装
    let src = r#"
variant Content {
    | Text of string
    | Element of int
}
void consume(Content c) {}
void Main() {
    consume("Click");
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected implicit variant coercion for method argument"
    );
}

#[test]
fn variant_implicit_return() {
    // 函数返回 Variant，return "Click" 自动包装
    let src = r#"
variant Content {
    | Text of string
    | Element of int
}
Content make() {
    return "Click";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected implicit variant coercion in return statement"
    );
}

#[test]
fn variant_implicit_ambiguity_rejected() {
    // 两个 case 都接受 int payload → 歧义，拒绝隐式构造
    let src = r#"
variant Ambiguous {
    | A of int
    | B of int
}
void Main() {
    Ambiguous v = 42;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let err = tc.check_module(&module).unwrap_err();
    assert!(
        err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })),
        "expected Mismatch error for ambiguous variant coercion, got {err:?}"
    );
}

#[test]
fn variant_implicit_content_text_over_resource() {
    // Arc.UI.Content：`Text of string` 与 `Resource of string` 歧义时，
    // 裸 string 字面量优先映射为 Text（RFC 037 D2 / RFC 004 §D9 框架消解）。
    let src = r#"
variant Content {
    | None
    | Text of string
    | Element of int
    | Binding of int
    | Resource of string
}
class Button {
    private Content _content;
    public Content Content {
        get { return _content; }
        set { _content = value; }
    }
}
void Main() {
    Button b = new Button();
    b.Content = "Click";
    Content c = "Hello";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "expected Content string implicit coercion preferring Text over Resource"
    );
}

#[test]
fn variant_implicit_no_match_rejected() {
    // 无 case 匹配 string → 类型不匹配错误
    let src = r#"
variant IntOnly {
    | Value of int
}
void Main() {
    IntOnly v = "not an int";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let err = tc.check_module(&module).unwrap_err();
    assert!(
        err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })),
        "expected Mismatch error for no matching variant case, got {err:?}"
    );
}

#[test]
fn interp_format_m2a_ok() {
    let src = r#"
string Main() {
    int n = 42;
    return $"{n:D5}";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn interp_format_m2b_ncep_ok() {
    let src = r#"
string Main() {
    double x = 1234.5;
    string a = $"{x:N}";
    string b = $"{x:C2}";
    string c = $"{x:E}";
    string d = $"{0.25:P0}";
    return a + b + c + d;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn interp_format_m2c_custom_ok() {
    let src = r#"
string Main() {
    int n = 42;
    double d = 3.14159;
    string a = $"{n:000}";
    string b = $"{d:0.00}";
    string c = $"{n:0.00}";
    return a + b + c;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn interp_format_m2d_custom_hash_ok() {
    let src = r#"
string Main() {
    int n = 1234;
    double d = 0.1234;
    string a = $"{n:#.##}";
    string b = $"{n:#,##0}";
    string c = $"{d:0.00%}";
    string e = $"{42:#00}";
    return a + b + c + e;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn interp_custom_scale_comma_ok() {
    let src = r#"
string Main() {
    int n = 1234567;
    string a = $"{n:0,}";
    string b = $"{n:0,,.00}";
    return a + b;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn interp_formattable_date_format_rejected() {
    let src = r#"
string Main() {
    int n = 42;
    return $"{n:yyyy}";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let err = tc.check_module(&module).unwrap_err();
    assert!(
        err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })),
        "expected Mismatch for date-like yyyy on int, got {err:?}"
    );
}

#[test]
fn interp_datetime_rich_tokens_ok() {
    let src = r#"
struct DateTime {
    public DateTime() {}
    public string ToString(string format) { return format; }
}
string Main() {
    DateTime dt = new DateTime();
    string a = $"{dt:yyyy-MM-ddTHH:mm:ss.fff}";
    string b = $"{dt:hh:mm:ss tt}";
    string c = $"{dt:dddd}";
    string d = $"{dt:M/MMM/MMMM}";
    string e = $"{dt:ddd}";
    string f = $"{dt:yy}";
    string g = $"{dt:yyyy-MM-dd zzz}";
    return a + b + c + d + e + f + g;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "rich date tokens should typeck"
    );
}

#[test]
fn interp_datetime_unsupported_token_rejected() {
    let src = r#"
struct DateTime {
    public DateTime() {}
    public string ToString(string format) { return format; }
}
string Main() {
    DateTime dt = new DateTime();
    return $"{dt:yyyy-MM-dd K}";
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let err = tc.check_module(&module).unwrap_err();
    assert!(
        err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })),
        "expected Mismatch for unsupported date token K, got {err:?}"
    );
}

#[test]
fn interp_custom_section_ok() {
    let src = r#"
string Main() {
    int n = 42;
    string a = $"{n:0;0}";
    string b = $"{(-42):0;00}";
    string c = $"{0:0;0;#}";
    return a + b + c;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(tc.check_module(&module).is_ok());
}

#[test]
fn interp_custom_quotes_ok() {
    let src = r#"
string Main() {
    int n = 42;
    string a = $"{n:0'x'}";
    string b = $"{(-3):0;'('#')'}";
    return a + b;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "prefix/suffix quotes literals should typeck"
    );
}

#[test]
fn interp_custom_mid_quotes_ok() {
    let src = r#"
string Main() {
    int n = 42;
    string a = $"{n:0'x'0}";
    string b = $"{5:0'x'0}";
    string c = $"{123:0'x'0}";
    return a + b + c;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "integer mid-placeholder quoted literals should typeck (RFC 007 M2g)"
    );
}

#[test]
fn interp_custom_frac_mid_quotes_ok() {
    let src = r#"
string Main() {
    double n = 1.23;
    string a = $"{n:0.0'x'0}";
    string b = $"{1.2:0.0'x'0}";
    string c = $"{1.23:0.'p'0'x'0}";
    return a + b + c;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    assert!(
        tc.check_module(&module).is_ok(),
        "fractional mid-placeholder quoted literals should typeck (RFC 007 M2i)"
    );
}

#[test]
fn bare_struct_instance_field_access() {
    // Bare `_field` (without `this.`) must resolve as instance field access
    // in struct methods, constructors, and property bodies.
    let src = r#"
struct Counter {
    private int _count;
    public Counter(int initial) {
        _count = initial;
    }
    public int Value {
        get { return _count; }
        set { _count = value; }
    }
    public void Increment() {
        _count = _count + 1;
    }
    public int Read() {
        return _count;
    }
}
void Main() {
    Counter c = new Counter(0);
    c.Increment();
    int v = c.Read();
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    assert!(
        result.is_ok(),
        "bare struct field access should typecheck: {:?}",
        result.err()
    );
}

#[test]
fn rfc080_operator_overload_typeck() {
    let src = r#"
struct Vec2 {
    public int X;
    public int Y;
    public Vec2(int x, int y) {
        this.X = x;
        this.Y = y;
    }
    public static Vec2 operator +(Vec2 a, Vec2 b) {
        return new Vec2(a.X + b.X, a.Y + b.Y);
    }
    public static Vec2 operator -(Vec2 a) {
        return new Vec2(-a.X, -a.Y);
    }
    public static bool operator ==(Vec2 a, Vec2 b) {
        return a.X == b.X && a.Y == b.Y;
    }
}
void Main() {
    Vec2 a = new Vec2(1, 2);
    Vec2 b = new Vec2(3, 4);
    Vec2 s = a + b;
    Vec2 n = -a;
    bool eq = a == b;
    a += b;
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    match tc.check_module(&module) {
        Ok(_) => {}
        Err(e) => panic!("typeck failed: {e:?}"),
    }
}

const NULL_SHORT_CIRCUIT_CLASS: &str = r#"
class Node {
    public int Revision;
    public bool Valid;
}
"#;

fn typeck_src(src: &str) -> Result<(), String> {
    let program = Parser::parse_program(src).map_err(|e| format!("parse: {e:?}"))?;
    let mut hir = HirBuilder::new();
    let module = hir
        .lower_program(&program)
        .map_err(|e| format!("hir: {e:?}"))?;
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
        .map(|_| ())
        .map_err(|e| format!("typeck: {e:?}"))
}

#[test]
fn null_short_circuit_or_right_narrows_receiver() {
    // CD-8：`x == null || x.Member` —— 右操作数仅在左为假（x 非空）时求值，
    // 与 C#/Roslyn 语义对齐，条件表达式内部即可收窄。
    let src = format!(
        "{NULL_SHORT_CIRCUIT_CLASS}\n\
         void Main() {{\n\
         \x20   Node? v = new Node();\n\
         \x20   if (v == null || v.Revision != 2) {{ return; }}\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn null_short_circuit_and_right_narrows_receiver() {
    // `&&` 对称方向：`x != null && x.Member` 右操作数仅在左为真（x 非空）时求值。
    let src = format!(
        "{NULL_SHORT_CIRCUIT_CLASS}\n\
         void Main() {{\n\
         \x20   Node? v = new Node();\n\
         \x20   if (v != null && v.Valid) {{ return; }}\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn null_short_circuit_or_chain_narrows_receiver() {
    // `||` 链式（左结合）：`x == null || y == null || x.Member` 右侧须同时收窄 x、y。
    let src = format!(
        "{NULL_SHORT_CIRCUIT_CLASS}\n\
         void Main() {{\n\
         \x20   Node? x = new Node();\n\
         \x20   Node? y = new Node();\n\
         \x20   if (x == null || y == null || x.Revision != 2 || y.Valid) {{ return; }}\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn null_short_circuit_or_does_not_pass_unrelated_receiver() {
    // 负向：左为 `x == null` 只收窄 x；`y.Member` 仍是真可空访问 → 必须报错。
    let src = format!(
        "{NULL_SHORT_CIRCUIT_CLASS}\n\
         void Main() {{\n\
         \x20   Node? x = new Node();\n\
         \x20   Node? y = new Node();\n\
         \x20   if (x == null || y.Revision != 2) {{ return; }}\n\
         }}"
    );
    assert!(
        typeck_src(&src).is_err(),
        "unrelated nullable receiver must still error"
    );
}

#[test]
fn null_short_circuit_or_does_not_pass_null_path_receiver() {
    // 负向：`x != null || x.Member` —— 左为真短路（x 非空），左为假则 x 为空，
    // 右操作数 `x.Member` 恒落在空路径上 → 必须报错（收窄不能变成无脑放行）。
    let src = format!(
        "{NULL_SHORT_CIRCUIT_CLASS}\n\
         void Main() {{\n\
         \x20   Node? x = new Node();\n\
         \x20   if (x != null || x.Revision != 2) {{ return; }}\n\
         }}"
    );
    assert!(
        typeck_src(&src).is_err(),
        "right operand reached with null receiver must still error"
    );
}

#[test]
fn null_short_circuit_and_does_not_pass_null_path_receiver() {
    // 负向：`x == null && x.Member` —— 右操作数仅在 x 为空时求值 → 必须报错。
    let src = format!(
        "{NULL_SHORT_CIRCUIT_CLASS}\n\
         void Main() {{\n\
         \x20   Node? x = new Node();\n\
         \x20   if (x == null && x.Revision != 2) {{ return; }}\n\
         }}"
    );
    assert!(
        typeck_src(&src).is_err(),
        "right operand reached with null receiver must still error"
    );
}

#[test]
fn nullable_string_vs_string_comparison_ok() {
    // `string?` 与 `string` 直接比较合法：可空引用类型与基础类型是同一运行时
    // 类型，可空性仅是编译期注解。曾报 type mismatch——string 比较特判只认
    // 裸 null（Nullable{Infer}）与 is_reference_type 放行，而后者不解包
    // Nullable → `string?` 两路皆落空。修复：is_reference_type 递归解包。
    let src = r#"
void Main() {
    string? a = null;
    a ??= "fallback";
    if (a != "fallback") { return; }
    if (a == "other") { return; }
    if (a == null) { return; }
    string? b = "x";
    if (b == a) { return; }
}
"#;
    assert!(
        typeck_src(src).is_ok(),
        "string? vs string comparison must typecheck"
    );
}

#[test]
fn int_vs_string_comparison_still_rejected() {
    // 反向锚：修复只解包 Nullable 的引用性，不放宽 string 比较特判本身——
    // 值类型与 string 比较仍须报 type mismatch。
    let src = r#"
void Main() {
    int n = 1;
    if (n == "x") { return; }
}
"#;
    assert!(
        typeck_src(src).is_err(),
        "int == string must still be a type mismatch"
    );
}

// ============================================================
// C# 优雅语法特性 typeck 覆盖补全
//
// 背景：switch 表达式（RFC 036 M4）、`?.`（RFC 009 L2）、`??`/`??=`、
// is 组合模式（RFC 004 M6 + C# 9）、target-typed new（RFC 006）、
// record/with（RFC 006 M2/M5+）、lambda 捕获（RFC 008）均已实现，
// 但 typeck 层缺乏专项测试。本批按特性分组补齐正/负例。
// ============================================================

// ---------- switch 表达式（RFC 036 M4） ----------

#[test]
fn switch_expr_enum_exhaustive_ok() {
    // enum scrutinee 全变体覆盖：`Color.Red` 变体模式 + 穷尽性检查通过。
    let src = r#"
enum Color { Red = 0, Green = 1, Blue = 2 }
void Main() {
    Color c = Color.Red;
    int r = c switch {
        Color.Red => 1,
        Color.Green => 2,
        Color.Blue => 3,
    };
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn switch_expr_enum_missing_variant_rejected() {
    // 缺变体且无默认臂 → NonExhaustiveMatch（missing 列表）。
    let src = r#"
enum Color { Red = 0, Green = 1, Blue = 2 }
void Main() {
    Color c = Color.Red;
    int r = c switch {
        Color.Red => 1,
        Color.Green => 2,
    };
}
"#;
    assert!(
        typeck_src(src).is_err(),
        "missing Blue variant without default arm must be non-exhaustive"
    );
}

#[test]
fn switch_expr_wildcard_default_arm_ok() {
    // `_` 通配默认臂兜底：缺变体也算穷尽。
    let src = r#"
enum Color { Red = 0, Green = 1, Blue = 2 }
void Main() {
    Color c = Color.Green;
    int r = c switch {
        Color.Red => 1,
        _ => 0,
    };
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn switch_expr_variant_payload_binding_ok() {
    // variant case 模式绑定（RFC 004 M1/M2）：payload 绑定入作用域，
    // 臂体直接引用 `n`（int payload）——若绑定未入 scope 则报未定义。
    let src = r#"
variant Content {
    | Text of string
    | Element of int
}
void Main() {
    Content c = 42;
    int r = c switch {
        Content.Text(t) => 0,
        Content.Element(n) => n,
    };
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn switch_expr_when_guard_ok() {
    // when 守卫：bool 表达式合法；`_` 兜底保证穷尽。
    // guard 写成裸 `flag =>` 会被解析吞为 lambda（Ident+FatArrow），
    // 故用 `flag == true` 收尾于字面量规避歧义。
    let src = r#"
enum Color { Red = 0, Green = 1, Blue = 2 }
void Main() {
    Color c = Color.Red;
    bool flag = true;
    int r = c switch {
        Color.Red => 1,
        _ when flag == true => 2,
    };
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn switch_expr_when_guard_non_bool_rejected() {
    // when 守卫非 bool → Mismatch。
    let src = r#"
enum Color { Red = 0, Green = 1, Blue = 2 }
void Main() {
    Color c = Color.Red;
    int r = c switch {
        Color.Red => 1,
        _ when 1 => 2,
    };
}
"#;
    assert!(typeck_src(src).is_err(), "when guard must be bool");
}

#[test]
fn switch_expr_arm_types_unified() {
    // 臂体类型不一致 → Mismatch。
    let src = r#"
enum Color { Red = 0, Green = 1, Blue = 2 }
void Main() {
    Color c = Color.Red;
    int r = c switch {
        Color.Red => 1,
        _ => "x",
    };
}
"#;
    assert!(typeck_src(src).is_err(), "arm body types must unify");
}

#[test]
fn switch_expr_non_enum_requires_default_arm() {
    // 非 enum scrutinee 无默认臂 → Oop 提示加 `_ => ...`。
    // 字面量模式分类为 Wildcard 恒穷尽，无法构造非穷尽负例；
    // `null` 模式（MatchPat::Null）不置位 has_default，可用作非穷尽臂。
    let src = r#"
void Main() {
    string? s = "x";
    int r = s switch {
        null => 0,
    };
}
"#;
    assert!(
        typeck_src(src).is_err(),
        "non-enum scrutinee without `_` arm must be non-exhaustive"
    );
}

#[test]
fn switch_expr_literal_patterns_ok() {
    // 非 enum + 默认臂：字面量模式经 types_compatible 检查。
    let src = r#"
void Main() {
    int x = 5;
    int r = x switch {
        1 => 10,
        _ => 0,
    };
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

// ---------- `?.` null 条件（RFC 009 L2） ----------

const NULL_COND_NODE: &str = r#"
class Node {
    public int Depth;
    public string Name;
    public Node? Child;
    public int GetDepth() { return Depth; }
}
"#;

#[test]
fn null_conditional_value_field_wraps_nullable() {
    // RFC 009 L2：`n?.Depth`（int 字段）结果恒为 `int?`——值类型也包 Nullable。
    let src = format!(
        "{NULL_COND_NODE}\n\
         void Main() {{\n\
         \x20   Node? n = new Node();\n\
         \x20   int? d = n?.Depth;\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn null_conditional_value_field_to_non_nullable_rejected() {
    // 结果是 int?，直接赋给 int 须报错（恒包装规则不可绕过）。
    let src = format!(
        "{NULL_COND_NODE}\n\
         void Main() {{\n\
         \x20   Node? n = new Node();\n\
         \x20   int d = n?.Depth;\n\
         }}"
    );
    assert!(
        typeck_src(&src).is_err(),
        "`?.` result must stay nullable, assigning to bare int must fail"
    );
}

#[test]
fn null_conditional_chain_ok() {
    // 链式 `n?.Child?.Name`：Child 为 Node?，最终结果仍为 `string?`（不二次包装）。
    let src = format!(
        "{NULL_COND_NODE}\n\
         void Main() {{\n\
         \x20   Node? n = new Node();\n\
         \x20   string? name = n?.Child?.Name;\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn null_conditional_method_call_wraps_nullable() {
    // `?.` 方法调用形式：`n?.GetDepth()` → `int?`。
    let src = format!(
        "{NULL_COND_NODE}\n\
         void Main() {{\n\
         \x20   Node? n = new Node();\n\
         \x20   int? d = n?.GetDepth();\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn null_conditional_non_nullable_receiver_rejected() {
    // 非 nullable 接收方 → Oop「requires nullable receiver」。
    let src = format!(
        "{NULL_COND_NODE}\n\
         void Main() {{\n\
         \x20   Node n = new Node();\n\
         \x20   int? d = n?.Depth;\n\
         }}"
    );
    assert!(
        typeck_src(&src).is_err(),
        "`?.` on non-nullable receiver must error"
    );
}

// ---------- `??` / `??=` 空合并（Coalesce / RFC 005） ----------

#[test]
fn coalesce_nullable_left_ok_string() {
    // `string? ?? string` → 结果为 string。
    let src = r#"
void Main() {
    string? a = null;
    string b = a ?? "fallback";
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn coalesce_nullable_left_ok_int() {
    // `int? ?? int` → 结果为 int（canonical(inner)）。
    let src = r#"
void Main() {
    int? n = 3;
    int m = n ?? 0;
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn coalesce_non_nullable_left_rejected() {
    // 左侧非 nullable → Oop「`??` requires nullable left side」。
    let src = r#"
void Main() {
    int x = 1;
    int y = x ?? 0;
}
"#;
    assert!(typeck_src(src).is_err(), "`??` requires nullable left side");
}

#[test]
fn coalesce_right_type_mismatch_rejected() {
    // 右侧类型与 inner 不兼容 → Mismatch。
    let src = r#"
void Main() {
    string? a = null;
    string b = a ?? 5;
}
"#;
    assert!(
        typeck_src(src).is_err(),
        "`??` right side must match inner type"
    );
}

#[test]
fn coalesce_assignment_ok() {
    // RFC 005：`??=` 空合并赋值（string? 与 int? 双形态）。
    let src = r#"
void Main() {
    string? s = null;
    s ??= "fallback";
    int? n = null;
    n ??= 5;
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn coalesce_assignment_type_mismatch_rejected() {
    // `??=` 右侧与 inner 不兼容须报错。
    let src = r#"
void Main() {
    string? s = null;
    s ??= 5;
}
"#;
    assert!(
        typeck_src(src).is_err(),
        "`??=` right side must match inner type"
    );
}

// ---------- is 模式（RFC 004 M6 + C# 9 组合） ----------

#[test]
fn is_null_pattern_ok() {
    let src = r#"
void Main() {
    string? s = null;
    if (s is null) { return; }
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn is_var_pattern_ok() {
    // `is var y`（C# var 模式）：作为 bool 表达式合法。
    let src = r#"
void Main() {
    int x = 5;
    if (x is var y) { return; }
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn is_constant_or_pattern_ok() {
    // C# 9 常量组合：`x is 0 or 1`。
    let src = r#"
void Main() {
    int x = 1;
    if (x is 0 or 1) { return; }
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn is_not_null_pattern_ok() {
    // `is not null`：not 前缀嵌套 null 模式。
    let src = r#"
void Main() {
    string? s = null;
    if (s is not null) { return; }
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn is_type_and_not_null_ok() {
    // `s is string and not null`：类型 + not null 的 and 组合。
    let src = r#"
void Main() {
    string s = "x";
    if (s is string and not null) { return; }
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn is_or_forbids_binding_rejected() {
    // C# 9 规则：`or` 内禁止一切绑定（`var y` 在 or 左侧须被拒）。
    let src = r#"
void Main() {
    int x = 1;
    if (x is var y or 1) { return; }
}
"#;
    assert!(
        typeck_src(src).is_err(),
        "binding inside `or` must be rejected (C# 9 rule)"
    );
}

// ---------- target-typed new（RFC 006） ----------

const TYPED_NEW_POINT: &str = r#"
class Point {
    public int X;
    public int Y;
    public Point(int x, int y) { X = x; Y = y; }
}
"#;

#[test]
fn target_typed_new_assignment_ok() {
    // `Point p = new(1, 2);`：目标类型自赋值左侧传播。
    let src = format!(
        "{TYPED_NEW_POINT}\n\
         void Main() {{\n\
         \x20   Point p = new(1, 2);\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn target_typed_new_ternary_ok() {
    // 三元两分支各自传播目标类型（apply_target_typed_new 递归 Ternary）。
    let src = format!(
        "{TYPED_NEW_POINT}\n\
         void Main() {{\n\
         \x20   bool f = true;\n\
         \x20   Point p = f ? new(1, 2) : new(3, 4);\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn target_typed_new_collection_element_ok() {
    // 集合字面量元素：expected 类型（Point[]）经 apply_target_typed_new
    // 递归 CollectionExpr 元素传播（collection_expr_list.rs）。单测环境
    // 不加载 std，泛型 List<T> 不可用，故用 typed array 表达集合目标。
    let src = format!(
        "{TYPED_NEW_POINT}\n\
         void Main() {{\n\
         \x20   Point[] ps = [new(1, 2), new(3, 4)];\n\
         }}"
    );
    assert_eq!(typeck_src(&src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn target_typed_new_var_target_rejected() {
    // var 目标不可构造（Infer 非 constructible target）→ 报错。
    let src = format!(
        "{TYPED_NEW_POINT}\n\
         void Main() {{\n\
         \x20   var p = new(1, 2);\n\
         }}"
    );
    assert!(
        typeck_src(&src).is_err(),
        "target-typed `new()` without a target type must be rejected"
    );
}

// ---------- record + with（RFC 006 M2/M5+） ----------

#[test]
fn record_positional_decl_and_with_ok() {
    // record 位置参数声明（脱糖 props+ctor）+ `with { X = 5 }` 非破坏性修改。
    let src = r#"
record Point(int X, int Y);
void Main() {
    Point p = new Point(1, 2);
    Point q = p with { X = 5 };
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn record_with_rejects_unknown_member() {
    // with 修改不存在的成员 → 报错。
    let src = r#"
record Point(int X, int Y);
void Main() {
    Point p = new Point(1, 2);
    Point q = p with { Z = 5 };
}
"#;
    assert!(
        typeck_src(src).is_err(),
        "`with` must reject unknown member"
    );
}

// ---------- lambda 捕获（RFC 008） ----------

#[test]
fn lambda_captures_outer_local_ok() {
    // RFC 008：lambda 体可引用外层局部变量（捕获语义的 typeck 可见性前提）。
    let src = r#"
void Main() {
    int outer = 5;
    Func<int, int> f = x => x + outer;
}
"#;
    assert_eq!(typeck_src(src).map_err(|e| e.clone()), Ok(()), "{}", src);
}

#[test]
fn lambda_body_undefined_name_rejected() {
    // 反向锚：lambda 体引用未定义名字须报错（捕获检查不是无脑放行）。
    let src = r#"
void Main() {
    Func<int, int> f = x => x + undefined_v;
}
"#;
    assert!(
        typeck_src(src).is_err(),
        "lambda body referencing undefined name must error"
    );
}
