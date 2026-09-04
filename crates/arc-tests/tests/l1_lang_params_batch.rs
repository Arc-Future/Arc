//! L1 批量：语言参数修饰回归集（6 case）。
//!
//! 从 lang_params_batch_e2e.rs 提取，改为 L1 纯编译期测试。
//! 排除 out_forward/params_span/params_compound（依赖 Dictionary/ReadOnlySpan 等运行时类型）。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_lang_params_batch() {
    assert_compiles_batch(
        "lang_params",
        &[
            (
                "optional_params",
                r#"using Arc;

class Adder {
    public int Add(int a, int b = 10) {
        return a + b;
    }
}

void Main() {
    Adder c = new Adder();
    if (c.Add(5) != 15) { Console.WriteLine("fail:omitted"); return; }
    if (c.Add(2, 5) != 7) { Console.WriteLine("fail:explicit"); return; }
    Console.WriteLine("optional_params_ok");
}
"#,
            ),
            (
                "optional_ctor",
                r#"using Arc;

class Point {
    public int X;
    public int Y;
    public Point(int x, int y = 0) {
        X = x;
        Y = y;
    }
}

class Label {
    public string Text;
    public int Level;
    public bool Flush;
    public Label(string text, int level = 0, bool flush = false) {
        Text = text;
        Level = level;
        Flush = flush;
    }
}

void Main() {
    Point p = new Point(3);
    if (p.X != 3 || p.Y != 0) { Console.WriteLine("fail:omit"); return; }
    Point q = new Point(y: 9, x: 1);
    if (q.X != 1 || q.Y != 9) { Console.WriteLine("fail:named_reorder"); return; }
    Label l = new Label("hi", flush: true);
    if (!(l.Text == "hi") || l.Level != 0 || !l.Flush) { Console.WriteLine("fail:skip_middle"); return; }
    Console.WriteLine("optional_ctor_ok");
}
"#,
            ),
            (
                "optional_m2b_defaults",
                r#"using Arc;

class Opts {
    public const int DefaultY = 7;
}

class LpPoint {
    public int X;
    public int Y;
    public LpPoint(int x, int y = Opts.DefaultY) {
        X = x;
        Y = y;
    }
}

class LpCounter {
    public int N;
    public LpCounter(int n = default(int)) {
        N = n;
    }
}

class LpBase {
    public int A;
    public int B;
    public LpBase(int a, int b = 0) {
        A = a;
        B = b;
    }
}

class LpDerived : LpBase {
    public LpDerived(int a) : base(a) {}
    public LpDerived(int a, int b) : base(b: b, a: a) {}
}

void Main() {
    LpCounter c = new LpCounter();
    if (c.N != 0) { Console.WriteLine("fail:default_expr"); return; }
    LpPoint p = new LpPoint(1);
    if (p.X != 1 || p.Y != 7) { Console.WriteLine("fail:const_ref"); return; }
    LpDerived d = new LpDerived(3);
    if (d.A != 3 || d.B != 0) { Console.WriteLine("fail:base_omit"); return; }
    LpDerived e = new LpDerived(1, 9);
    if (e.A != 1 || e.B != 9) { Console.WriteLine("fail:base_named"); return; }
    Console.WriteLine("optional_m2b_defaults_ok");
}
"#,
            ),
            (
                "in_param",
                r#"using Arc;

class InOps {
    public int Magnitude(in int v) { return v < 0 ? -v : v; }
    public int ReadField(in int v) { return v; }
}

void Main() {
    int x = -42;
    int m = new InOps().Magnitude(in x);
    if (m != 42) { Console.WriteLine("fail:magnitude"); return; }
    if (x != -42) { Console.WriteLine("fail:caller_intact"); return; }
    int v = 7;
    int r = new InOps().ReadField(in v);
    if (r != 7) { Console.WriteLine("fail:read_field"); return; }
    Console.WriteLine("in_param_ok");
}
"#,
            ),
            (
                "ref_out",
                r#"using Arc;

class Swapper {
    public void Swap(ref int a, ref int b) { int t = a; a = b; b = t; }
    public void Init(out int x) { x = 42; }
}

void Main() {
    int x = 1;
    int y = 2;
    new Swapper().Swap(ref x, ref y);
    if (x != 2 || y != 1) { Console.WriteLine("fail:swap"); return; }
    int v;
    new Swapper().Init(out v);
    if (v != 42) { Console.WriteLine("fail:init"); return; }
    Console.WriteLine("ref_out_ok");
}
"#,
            ),
            (
                "named_param_bool",
                r#"using Arc;

class NpbConfig {
    public bool Enabled;
    public bool Verbose;
    public int Count;
    public NpbConfig(bool enabled = false, bool verbose = false, int count = 1) {
        Enabled = enabled;
        Verbose = verbose;
        Count = count;
    }
}

void Main() {
    NpbConfig c1 = new NpbConfig();
    if (c1.Enabled || c1.Verbose || c1.Count != 1) { Console.WriteLine("fail:defaults"); return; }
    NpbConfig c2 = new NpbConfig(enabled: true);
    if (!c2.Enabled || c2.Verbose) { Console.WriteLine("fail:named_bool"); return; }
    Console.WriteLine("named_param_bool_ok");
}
"#,
            ),
        ],
    );
}
