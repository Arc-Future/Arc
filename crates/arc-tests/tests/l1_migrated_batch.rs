//! 从 arc-integration 迁移的 L1 批量测试。
//!
//! 策略：所有 compile-ok case 合并为单次 `assert_compiles_batch`（一次 std 加载），
//! reject case 独立 `assert_rejected`（无需 clang，本身很快）。

use arc_tests::{assert_compiles_batch, assert_rejected};

// ── 编译期拒绝（独立、本身很快） ──

#[test]
fn rejects_cross_enum_bitwise_or() {
    assert_rejected(
        "migrate_enum_bitwise_type",
        r#"using Arc;
[Flags]
public enum A { X = 1, Y = 2 }
[Flags]
public enum B { P = 1, Q = 2 }
void Main() {
    A a = A.X;
    B b = B.P;
    A bad = a | b;
}
"#,
        "mismatch",
    );
}

#[test]
fn rejects_pattern_combinator_not_binding() {
    assert_rejected(
        "migrate_pc_not_binding",
        r#"using Arc;
class Animal { }
void Main() {
    Animal a = new Animal();
    if (a is not Animal x) { }
}
"#,
        "binding",
    );
}

#[test]
fn rejects_pattern_combinator_or_binding() {
    assert_rejected(
        "migrate_pc_or_binding",
        r#"using Arc;
class Animal { }
class Dog : Animal { }
class Cat : Animal { }
void Main() {
    Animal a = new Animal();
    if (a is Dog or Cat y) { }
}
"#,
        "binding",
    );
}

#[test]
fn rejects_bad_enum_flag_cast() {
    assert_rejected(
        "migrate_enum_cast",
        r#"using Arc;
public enum Color { Red, Green, Blue }
void Main() {
    Color c = "Red";
}
"#,
        "type",
    );
}

#[test]
fn rejects_interface_sealed_conflict() {
    assert_rejected(
        "migrate_sealed_iface",
        r#"using Arc;
public interface IFoo { void Bar(); }
sealed class Bad : IFoo { }
void Main() { }
"#,
        "",
    );
}

#[test]
fn rejects_abstract_constructor_call() {
    assert_rejected(
        "migrate_abstract_ctor",
        r#"using Arc;
abstract class Base {
    public abstract void Run();
}
void Main() {
    var b = new Base();
}
"#,
        "",
    );
}

// ── 编译通过（合并为一次编译调用） ──

#[test]
fn compiles_migrated_batch() {
    assert_compiles_batch(
        "migrate_batch",
        &[
            // 三元表达式：基本 int
            (
                "ternary_int",
                r#"using Arc;
void Main() {
    int a = true ? 10 : 20;
    Console.WriteLine(a.ToString());
}
"#,
            ),
            // 三元表达式：基本 string
            (
                "ternary_string",
                r#"using Arc;
void Main() {
    bool cond = false;
    string s = cond ? "yes" : "no";
    Console.WriteLine(s);
}
"#,
            ),
            // 三元表达式：比较条件
            (
                "ternary_compare",
                r#"using Arc;
void Main() {
    int a = 5 > 3 ? 10 : 20;
    int c = 5 == 5 ? 100 : 200;
    int d = 5 != 5 ? 100 : 200;
    Console.WriteLine(a.ToString());
    Console.WriteLine(c.ToString());
    Console.WriteLine(d.ToString());
}
"#,
            ),
            // 三元表达式：嵌套右结合
            (
                "ternary_nested",
                r#"using Arc;
void Main() {
    int a = 10;
    int b = a < 5 ? 1 : a < 15 ? 2 : 3;
    Console.WriteLine(b.ToString());
}
"#,
            ),
            // 三元表达式：函数调用
            (
                "ternary_func_call",
                r#"using Arc;
void Main() {
    int x = 42;
    Console.WriteLine(x > 0 ? "positive" : "non-positive");
}
"#,
            ),
            // 三元表达式：return 语句
            (
                "ternary_return",
                r#"using Arc;
int Compare(int a, int b) {
    return a > b ? 1 : -1;
}
void Main() {
    int r = Compare(5, 3);
    Console.WriteLine(r.ToString());
}
"#,
            ),
            // 三元表达式：new 表达式
            (
                "ternary_new_expr",
                r#"using Arc;
class MtNum {
    private int _val;
    public MtNum(int v) { _val = v; }
    public int Value { get { return this._val; } }
}
void Main() {
    int ticks = -5;
    MtNum n = ticks < 0 ? new MtNum(-ticks) : new MtNum(ticks);
    Console.WriteLine(n.Value.ToString());
}
"#,
            ),
            // 三元表达式：深嵌套
            (
                "ternary_deep",
                r#"using Arc;
void Main() {
    int a = true ? 2 : true ? 4 : true ? 6 : 7;
    int b = false ? 2 : false ? 4 : false ? 6 : 7;
    int c = false ? 2 : true ? 4 : false ? 6 : 7;
    Console.WriteLine(a.ToString());
    Console.WriteLine(b.ToString());
    Console.WriteLine(c.ToString());
}
"#,
            ),
            // 三元表达式：bool 变量
            (
                "ternary_bool_var",
                r#"using Arc;
string Eval(int score) {
    bool pass = score >= 60;
    return pass ? "pass" : "fail";
}
void Main() {
    Console.WriteLine(Eval(80));
    Console.WriteLine(Eval(30));
}
"#,
            ),
            // 字段关键字：struct 读写
            (
                "fk_struct_rw",
                r#"using Arc;
struct FkCounter {
    public int X { get { return field; } set { field = value; } }
}
void Main() {
    FkCounter c = new FkCounter();
    c.X = 42;
    Console.WriteLine("field struct rw ok");
}
"#,
            ),
            // 字段关键字：class setter 校验
            (
                "fk_class_validate",
                r#"using Arc;
class FkWallet {
    public int Balance { get { return field; } set { if (value >= 0) { field = value; } } }
}
void Main() {
    FkWallet w = new FkWallet();
    w.Balance = 100;
    w.Balance = -5;
    Console.WriteLine("field class validate ok");
}
"#,
            ),
            // 字段关键字：getter-only + init
            (
                "fk_class_init",
                r#"using Arc;
class FkPoint {
    public int X { get { return field; } init { field = value; } }
    public int Y { get { return field; } init { field = value; } }
}
void Main() {
    FkPoint p = new FkPoint() { X = 3, Y = 4 };
    Console.WriteLine("field class init ok");
}
"#,
            ),
            // 字段关键字：自动属性
            (
                "fk_auto_regression",
                r#"using Arc;
class FkBox {
    public int Value { get; set; }
}
void Main() {
    FkBox b = new FkBox();
    b.Value = 7;
    Console.WriteLine("field auto regression ok");
}
"#,
            ),
            // 字段关键字：普通方法内 field 作标识符
            (
                "fk_ordinary_var",
                r#"using Arc;
class FkCalc {
    public int Compute() {
        int field = 5;
        field = field + 2;
        return field;
    }
}
void Main() {
    FkCalc c = new FkCalc();
    int r = c.Compute();
    Console.WriteLine("field ordinary var ok");
}
"#,
            ),
            // 字段关键字：无 field 自定义访问器
            (
                "fk_no_field",
                r#"using Arc;
class FkKeeper {
    private int _v;
    public int V { get { return _v; } set { _v = value; } }
}
void Main() {
    FkKeeper k = new FkKeeper();
    k.V = 9;
    Console.WriteLine("field no-field regression ok");
}
"#,
            ),
            // 字段关键字：泛型类
            (
                "fk_generic",
                r#"using Arc;
class FkHolder<T> {
    public T Value { get { return field; } set { field = value; } }
}
void Main() {
    FkHolder<int> h = new FkHolder<int>();
    h.Value = 42;
    FkHolder<string> s = new FkHolder<string>();
    s.Value = "hi";
    Console.WriteLine("field generic ok");
}
"#,
            ),
            // 枚举 flags 基础
            (
                "enum_flags_basic",
                r#"using Arc;
[Flags]
public enum Perm { None = 0, Read = 1, Write = 2, Exec = 4 }
void Main() {
    Perm p = Perm.Read | Perm.Write;
    int v = (int)p;
    Console.WriteLine(v.ToString());
}
"#,
            ),
            // 枚举 switch 表达式
            (
                "enum_switch_expr",
                r#"using Arc;
public enum Color { Red, Green, Blue }
void Main() {
    Color c = Color.Green;
    string s = c switch { Color.Red => "red", Color.Green => "green", _ => "other" };
    Console.WriteLine(s);
}
"#,
            ),
        ],
    );
}
