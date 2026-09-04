//! L1 批量：语言特性回归集（枚举、泛型、模式匹配、三元、字段关键字等）。
//!
//! 所有 case 合并为单次 assert_compiles_batch（一次 std 加载 + 一次 clang 链接）。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_lang_features_batch() {
    assert_compiles_batch(
        "lang_features",
        &[
            // === 枚举基础 ===
            (
                "enum_basic",
                r#"using Arc;
enum Color { Red, Green, Blue }
void Main() {
    Color c = Color.Green;
    int v = (int)c;
    Console.WriteLine(v.ToString());
}
"#,
            ),
            (
                "enum_flags_basic",
                r#"using Arc;
[Flags]
public enum Perm { None = 0, Read = 1, Write = 2, Exec = 4 }
void Main() {
    Perm p = Perm.Read | Perm.Write;
    int v = (int)p;
    Console.WriteLine("enum_flags_ok");
}
"#,
            ),
            (
                "enum_switch",
                r#"using Arc;
enum Status { Idle, Running, Done }
void Main() {
    Status s = Status.Running;
    string n = s switch { Status.Idle => "idle", Status.Running => "run", _ => "done" };
    Console.WriteLine(n);
}
"#,
            ),
            // === switch 语句（case 体花括号块与 break 消解回归） ===
            (
                "switch_stmt_braced",
                r#"using Arc;
string Classify(int n) {
    string r = "many";
    switch (n) {
        case 0: {
            r = "zero";
            break;
        }
        case 1:
        case 2: {
            r = "small";
            break;
        }
        default: {
            break;
        }
    }
    return r;
}
void Main() {
    Console.WriteLine(Classify(0) + Classify(1) + Classify(9));
}
"#,
            ),
            (
                "switch_stmt_nested_if_break",
                r#"using Arc;
string Pick(int n) {
    string r = "other";
    switch (n) {
        case 1: {
            if (n > 0) {
                break;
            }
            r = "unreachable";
            break;
        }
        default: {
            break;
        }
    }
    return r;
}
void Main() {
    Console.WriteLine(Pick(1) + Pick(9));
}
"#,
            ),
            (
                "switch_stmt_loop_break",
                r#"using Arc;
int FindTwo(int mode) {
    int found = -1;
    switch (mode) {
        case 3: {
            int i = 0;
            while (i < 10) {
                if (i == 2) {
                    found = i;
                    break;
                }
                i += 1;
            }
            break;
        }
        default: {
            break;
        }
    }
    return found;
}
void Main() {
    Console.WriteLine(FindTwo(3).ToString());
}
"#,
            ),
            // === 泛型 ===
            (
                "generic_class",
                r#"using Arc;
public class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
void Main() {
    var b = new Box<int>(99);
    Console.WriteLine("generic_ok");
}
"#,
            ),
            (
                "generic_constraint",
                r#"using Arc;
public class Stack<T> where T : class {
    private T[] _items;
    public void Push(T item) { }
}
void Main() {
    var s = new Stack<string>();
    Console.WriteLine("constraint_ok");
}
"#,
            ),
            // === 三元表达式 ===
            (
                "ternary_int",
                r#"using Arc;
void Main() {
    int a = true ? 10 : 20;
    Console.WriteLine(a.ToString());
}
"#,
            ),
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
            (
                "ternary_compare",
                r#"using Arc;
void Main() {
    int a = 5 > 3 ? 10 : 20;
    int c = 5 == 5 ? 100 : 200;
    Console.WriteLine(a.ToString());
    Console.WriteLine(c.ToString());
}
"#,
            ),
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
            (
                "ternary_return",
                r#"using Arc;
int Compare(int a, int b) { return a > b ? 1 : -1; }
void Main() {
    int r = Compare(5, 3);
    Console.WriteLine(r.ToString());
}
"#,
            ),
            // === 字段关键字 ===
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
            (
                "fk_class_validate",
                r#"using Arc;
class FkWallet {
    public int Balance { get { return field; } set { if (value >= 0) { field = value; } } }
}
void Main() {
    FkWallet w = new FkWallet();
    w.Balance = 100;
    Console.WriteLine("field class validate ok");
}
"#,
            ),
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
            (
                "fk_auto",
                r#"using Arc;
class FkBox {
    public int Value { get; set; }
}
void Main() {
    FkBox b = new FkBox();
    b.Value = 7;
    Console.WriteLine("field auto ok");
}
"#,
            ),
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
    Console.WriteLine("field no-field ok");
}
"#,
            ),
            // === 结构体 ===
            (
                "struct_point",
                r#"using Arc;
public struct Vec2 {
    public int X;
    public int Y;
    public Vec2(int x, int y) { X = x; Y = y; }
}
void Main() {
    var p = new Vec2(3, 4);
    Console.WriteLine("struct_ok");
}
"#,
            ),
            (
                "struct_valuetype",
                r#"using Arc;
public struct ColorRgb {
    public byte R;
    public byte G;
    public byte B;
}
void Main() {
    ColorRgb c;
    c.R = 255;
    c.G = 128;
    Console.WriteLine("valuetype_ok");
}
"#,
            ),
            // === 记录类型 ===
            (
                "record_basic",
                r#"using Arc;
public record Point {
    public int X { get; set; }
    public int Y { get; set; }
}
void Main() {
    var p = new Point();
    p.X = 3;
    p.Y = 4;
    Console.WriteLine(p.X.ToString());
}
"#,
            ),
            // === 类与继承 ===
            (
                "class_inheritance",
                r#"using Arc;
public class Animal {
    public string Name;
    public virtual string Speak() { return "..."; }
}
public class Dog : Animal {
    public override string Speak() { return "Woof"; }
}
void Main() {
    var d = new Dog();
    Console.WriteLine("inheritance_ok");
}
"#,
            ),
            // === 接口 ===
            (
                "interface_basic",
                r#"using Arc;
public interface IShape {
    double Area();
}
public class Circle : IShape {
    public double Radius;
    public double Area() { return 3.14 * Radius * Radius; }
}
void Main() {
    IShape s = new Circle();
    Console.WriteLine("interface_ok");
}
"#,
            ),
            // === 数组 ===
            (
                "array_basic",
                r#"using Arc;
void Main() {
    int[] arr = new int[5];
    arr[0] = 10; arr[1] = 20; arr[2] = 30;
    int sum = arr[0] + arr[1] + arr[2];
    Console.WriteLine(sum.ToString());
}
"#,
            ),
            // === for 循环 ===
            (
                "for_loop",
                r#"using Arc;
void Main() {
    int[] arr = new int[3];
    arr[0] = 1; arr[1] = 2; arr[2] = 3;
    int sum = 0;
    for (int i = 0; i < 3; i++) {
        sum += arr[i];
    }
    Console.WriteLine(sum.ToString());
}
"#,
            ),
            // === while 循环 ===
            (
                "while_loop",
                r#"using Arc;
void Main() {
    int n = 5;
    int fact = 1;
    while (n > 0) {
        fact *= n;
        n -= 1;
    }
    Console.WriteLine(fact.ToString());
}
"#,
            ),
            // === 异步 ===
            (
                "async_hello",
                r#"using Arc;
async Task<int> LangFetch() {
    await Task.Delay(1);
    return 42;
}
async Task Main() {
    int val = await LangFetch();
    Console.WriteLine(val.ToString());
}
"#,
            ),
            (
                "async_lambda",
                r#"using Arc;
async Task<int> AsyncFetch() { return 7; }
async Task Main() {
    Func<Task<int>> f = async () => await AsyncFetch();
    int r = await f();
    Console.WriteLine(r.ToString());
}
"#,
            ),
            // === Lambda ===
            (
                "lambda_basic",
                r#"using Arc;
void Main() {
    Func<int, int> square = (x) => x * x;
    int r = square(5);
    Console.WriteLine(r.ToString());
}
"#,
            ),
            (
                "lambda_block",
                r#"using Arc;
void Main() {
    Func<int> f = () => { int x = 1; x += 2; return x; };
    int r = f();
    Console.WriteLine(r.ToString());
}
"#,
            ),
            // === 模式匹配 ===
            (
                "pattern_is",
                r#"using Arc;
void Main() {
    object o = "hello";
    if (o is string s) {
        Console.WriteLine(s);
    }
}
"#,
            ),
            // === Nullable ===
            (
                "nullable_basic",
                r#"using Arc;
void Main() {
    int? x = null;
    int y = x ?? 42;
    Console.WriteLine(y.ToString());
}
"#,
            ),
            // === 可空引用类型 ===
            (
                "nullable_ref",
                r#"using Arc;
string? GetName() { return null; }
void Main() {
    string? name = GetName();
    if (name != null) {
        Console.WriteLine(name);
    } else {
        Console.WriteLine("null");
    }
}
"#,
            ),
        ],
    );
}
