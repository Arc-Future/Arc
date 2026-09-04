//! L1 批量：OOP 特性回归集（分派、重载、downcast、委托等）。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_oop_batch() {
    assert_compiles_batch(
        "oop_batch",
        &[
            // === 重载分派 ===
            (
                "overload_resolve",
                r#"using Arc;
public class OrTmp {
    public void Msg(string a) {
        this.Msg(a, "");
    }
    public void Msg(string a, string b) {
        Console.WriteLine("two:" + a + "|" + b);
    }
}
void Main() {
    OrTmp t = new OrTmp();
    t.Msg("hello");
    Console.WriteLine("overload_done");
}
"#,
            ),
            // === 重载虚方法分派槽 ===
            (
                "dispatch_overload_slots",
                r#"using Arc;
class DsCalc {
    public virtual int Describe(int value) { return value * 10; }
    public virtual string Describe(string text) { return "base-string:" + text; }
}
class DsCalcDerived : DsCalc {
    public override int Describe(int value) { return value * 100; }
}
void Main() {
    DsCalcDerived d = new DsCalcDerived();
    int r1 = d.Describe(5);
    string r2 = d.Describe("hi");
    Console.WriteLine(r1.ToString());
    Console.WriteLine(r2);
}
"#,
            ),
            // === 隐式覆写分派 ===
            (
                "dispatch_implicit_override",
                r#"using Arc;
abstract class DgBase {
    public abstract Task<string> Complete(string request);
}
class DgImpl : DgBase {
    public Task<string> Complete(string request) {
        return Task.FromResult("done:" + request);
    }
}
void Main() {
    DgBase b = new DgImpl();
    Task<string> t = b.Complete("hi");
    string result = t.Result;
    Console.WriteLine(result);
}
"#,
            ),
            // === 参数重载优先 ===
            (
                "normal_form_preference",
                r#"using Arc;
class GpCalc {
    public int Sum(int a) { return a; }
    public string Pick(string single) { return "single:" + single; }
}
void Main() {
    GpCalc c = new GpCalc();
    int s1 = c.Sum(5);
    string p1 = c.Pick("x");
    Console.WriteLine("ok");
}
"#,
            ),
            // === 动态 downcast ===
            (
                "dynamic_downcast",
                r#"using Arc;
interface IShape {
    double Area();
}
struct Square : IShape {
    public double Side;
    public Square(double s) { this.Side = s; }
    public double Area() { return this.Side * this.Side; }
}
void Main() {
    Square s = new Square(5.0);
    object o = s;
    IShape i = (IShape)o;
    double a = i.Area();
    if (a == 25.0) {
        Console.WriteLine("struct downcast ok");
    }
    if (o is IShape) {
        Console.WriteLine("struct is-iface ok");
    }
}
"#,
            ),
            // === 枚举比较 ===
            (
                "enum_compare",
                r#"using Arc;
enum Status { Idle, Running, Done }
void Main() {
    Status s = Status.Running;
    string n = "";
    if (s == Status.Idle) { n = "idle"; }
    else if (s == Status.Running) { n = "run"; }
    else { n = "done"; }
    Console.WriteLine(n);
}
"#,
            ),
            // === else-if 链 ===
            (
                "else_if_chain",
                r#"using Arc;
string Classify(int x) {
    string r = "";
    if (x < 0) r = "neg";
    else if (x == 0) r = "zero";
    else if (x < 10) r = "small";
    else r = "large";
    return r;
}
void Main() {
    string r1 = Classify(5);
    string r2 = Classify(-3);
    string r3 = Classify(100);
    string r4 = Classify(0);
    Console.WriteLine("ok");
}
"#,
            ),
            // === Flags 枚举位运算 ===
            (
                "flags_bitwise",
                r#"using Arc;
[Flags]
public enum EfAccess {
    None = 0, Read = 1, Write = 2, Execute = 4,
}
void Main() {
    EfAccess rw = EfAccess.Read | EfAccess.Write;
    EfAccess combined = EfAccess.Read | EfAccess.Write | EfAccess.Execute;
    EfAccess has_rw = combined & (EfAccess.Read | EfAccess.Write);
    EfAccess toggle = EfAccess.Read | EfAccess.Write;
    EfAccess toggled = toggle ^ EfAccess.Write;
    EfAccess not_none = ~EfAccess.None;
    EfAccess flags = EfAccess.None;
    flags |= EfAccess.Read;
    flags |= EfAccess.Write;
    Console.WriteLine("flags_ok");
}
"#,
            ),
            // === Enum 工具方法 ===
            (
                "enum_util",
                r#"using Arc;
[Flags]
public enum EfPerms {
    None = 0, Read = 1, Write = 2, Execute = 4,
}
void Main() {
    EfPerms p = EfPerms.Read | EfPerms.Write;
    bool hasRead = Enum.HasFlag(p, EfPerms.Read);
    bool defRead = Enum.IsDefined(EfPerms.Read);
    List<string> names = Enum.GetNames<EfPerms>();
    Console.WriteLine("enum_util_ok");
}
"#,
            ),
            // === 属性访问器 ===
            (
                "property_get_set",
                r#"using Arc;
public class Person {
    public string Name { get; set; }
    public int Age { get; set; }
}
void Main() {
    var p = new Person();
    p.Name = "Alice";
    p.Age = 30;
    Console.WriteLine(p.Name);
    Console.WriteLine(p.Age.ToString());
}
"#,
            ),
            // === 构造函数链 ===
            (
                "constructor_chain",
                r#"using Arc;
public class Base {
    public int Value;
    public Base() { Value = 1; }
    public Base(int v) { Value = v; }
}
public class Derived : Base {
    public string Label;
    public Derived() : base(42) { Label = "derived"; }
}
void Main() {
    var d = new Derived();
    Console.WriteLine(d.Value.ToString());
    Console.WriteLine(d.Label);
}
"#,
            ),
            // === 静态成员 ===
            (
                "static_member",
                r#"using Arc;
public class Counter {
    public static int Count;
    public static void Increment() { Count++; }
}
void Main() {
    Counter.Increment();
    Counter.Increment();
    Counter.Increment();
    Console.WriteLine(Counter.Count.ToString());
}
"#,
            ),
            // === 委托（用 Func 替代） ===
            (
                "func_delegate",
                r#"using Arc;
void Main() {
    Func<int, string> c = (x) => x.ToString();
    string s = c(42);
    Console.WriteLine(s);
}
"#,
            ),
            // === 结构体作为参数 ===
            (
                "struct_param",
                r#"using Arc;
public struct Point {
    public int X;
    public int Y;
    public Point(int x, int y) { X = x; Y = y; }
}
int Distance(Point a, Point b) {
    int dx = a.X - b.X;
    int dy = a.Y - b.Y;
    return dx * dx + dy * dy;
}
void Main() {
    var p1 = new Point(0, 0);
    var p2 = new Point(3, 4);
    int d = Distance(p1, p2);
    Console.WriteLine(d.ToString());
}
"#,
            ),
            // === 只读属性 ===
            (
                "readonly_prop",
                r#"using Arc;
public class Config {
    public string Name { get; }
    public int Port { get; }
    public Config(string name, int port) { Name = name; Port = port; }
}
void Main() {
    var cfg = new Config("test", 8080);
    Console.WriteLine(cfg.Name);
    Console.WriteLine(cfg.Port.ToString());
}
"#,
            ),
            // === 索引器 ===
            (
                "indexer_basic",
                r#"using Arc;
public class Buffer {
    private int[] _data = new int[10];
    public int this[int index] {
        get { return _data[index]; }
        set { _data[index] = value; }
    }
}
void Main() {
    var buf = new Buffer();
    buf[0] = 100;
    buf[1] = 200;
    int v = buf[0] + buf[1];
    Console.WriteLine(v.ToString());
}
"#,
            ),
        ],
    );
}
