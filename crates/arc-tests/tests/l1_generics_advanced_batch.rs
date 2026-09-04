//! L1 批量：高级泛型回归集（8 case）。
//!
//! 从 generics_advanced_batch_e2e.rs 提取，改为 L1 纯编译期测试。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_generics_advanced_batch() {
    assert_compiles_batch(
        "generics_adv",
        &[
            (
                "explicit_generic_vs_overload",
                r#"using Arc;

class EgGenHost {
    public string F<T>(T v) { return "F<T>"; }
    public string F(int v) { return "F-int"; }
    public string F(string v) { return "F-string"; }
    public string G<T1, T2>(T1 a, T2 b) { return "G<T1,T2>"; }
    public string G(int a, int b) { return "G-int"; }
}

void Main() {
    EgGenHost g = new EgGenHost();
    if (g.F<int>(7) != "F<T>") {
        Console.WriteLine("fail:f-int-explicit");
        return;
    }
    if (g.F(7) != "F-int") {
        Console.WriteLine("fail:f-int-implicit");
        return;
    }
    if (g.G<int,string>(7, "x") != "G<T1,T2>") {
        Console.WriteLine("fail:g-explicit");
        return;
    }
    if (g.G(7, 8) != "G-int") {
        Console.WriteLine("fail:g-implicit");
        return;
    }
    Console.WriteLine("explicit_generic_overload_ok");
}
"#,
            ),
            (
                "generic_base_field_mono",
                r#"using Arc;

class GbfDP {
    public long Id;
}

class GbfDPT<T> : GbfDP {
    public T DefaultValue;
    public GbfDPT(T v) { DefaultValue = v; }
}

T GbfRead<T>(GbfDPT<T> p) { return p.DefaultValue; }

void Main() {
    GbfDPT<double> p = new GbfDPT<double>(3.5);
    double v = GbfRead<double>(p);
    if (v > 3.4 && v < 3.6) {
        Console.WriteLine("generic_base_field_ok");
    }
}
"#,
            ),
            (
                "generic_inheritance_chain",
                r#"using Arc;

public class GenRoot<T> {
    public string RootTag { get; set; }
    public string Describe(T value) {
        return "Root<" + value.ToString() + ">:" + this.RootTag;
    }
}

public class GenMid<T> : GenRoot<T> {
    public string MidOf(T value) {
        return "Mid<" + value.ToString() + ">";
    }
    public virtual string Greet(T value) {
        return "MidGreet<" + value.ToString() + ">";
    }
}

public class DerivedInt : GenMid<int> {
    public override string Greet(int value) {
        return "DerivedGreet<" + value.ToString() + ">";
    }
}

public class DerivedGen<T> : GenMid<T> {
}

public class AdvMethodHost {
    public string Pick<U>(U value) {
        return "Pick<" + value.ToString() + ">";
    }
}

void Main() {
    DerivedInt d = new DerivedInt();
    d.RootTag = "tagA";
    if (d.Describe(42) != "Root<42>:tagA") {
        Console.WriteLine("fail:d1");
        return;
    }
    if (d.Greet(100) != "DerivedGreet<100>") {
        Console.WriteLine("fail:d3");
        return;
    }
    DerivedGen<string> dg = new DerivedGen<string>();
    dg.RootTag = "tagB";
    if (dg.Describe("hi") != "Root<hi>:tagB") {
        Console.WriteLine("fail:g1");
        return;
    }
    AdvMethodHost h = new AdvMethodHost();
    if (h.Pick<int>(5) != "Pick<5>") {
        Console.WriteLine("fail:m1");
        return;
    }
    Console.WriteLine("generic_inheritance_ok");
}
"#,
            ),
            (
                "generic_virtual_override",
                r#"using Arc;

public class GvBase<T> {
    public virtual string Tag(T v) {
        return "base<" + v.ToString() + ">";
    }
}

public class GvChild : GvBase<int> {
    public override string Tag(int v) {
        return "child<" + v.ToString() + ">";
    }
}

void Main() {
    GvChild c = new GvChild();
    if (c.Tag(1) != "child<1>") {
        Console.WriteLine("fail:direct");
        return;
    }
    GvBase<int> b = c;
    if (b.Tag(2) != "child<2>") {
        Console.WriteLine("fail:virtual");
        return;
    }
    Console.WriteLine("generic_virtual_ok");
}
"#,
            ),
            (
                "generic_constraint_max",
                r#"using Arc;

T GaMax<T>(T a, T b) where T : IComparable<T> { return a; }

void Main() {
    int m = GaMax<int>(10, 20);
    if (m == 10) {
        Console.WriteLine("generic_constraint_ok");
    }
}
"#,
            ),
            (
                "generic_math_stub",
                r#"using Arc;

void Main() {
    int a = 3;
    int b = 4;
    int c = a + b;
    if (c == 7) {
        Console.WriteLine("generic_math_stub_ok");
    }
}
"#,
            ),
            (
                "generic_algorithms_stub",
                r#"using Arc;

void Main() {
    int[] arr = [3, 1, 4, 1, 5];
    int total = 0;
    for (int i = 0; i < arr.Length; i++) { total = total + arr[i]; }
    if (total == 14) {
        Console.WriteLine("generic_algorithms_stub_ok");
    }
}
"#,
            ),
            (
                "generic_multiple_constraint",
                r#"using Arc;

interface IEquatableDemo<T> {
    bool Equals(T other);
}

class DemoItem : IEquatableDemo<DemoItem> {
    public int Val;
    public DemoItem(int v) { Val = v; }
    public bool Equals(DemoItem other) { return Val == other.Val; }
}

void Main() {
    DemoItem a = new DemoItem(5);
    DemoItem b = new DemoItem(5);
    if (a.Equals(b)) {
        Console.WriteLine("generic_multiple_constraint_ok");
    }
}
"#,
            ),
        ],
    );
}
