//! L1 compile-only batch: Static field, lazy init, lazy new.
//! Extracted from arc-integration e2e tests (static_field, static_lazy_init, static_lazy_new).

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_l1_static_batch() {
    assert_compiles_batch(
        "l1_static_batch",
        &[
            // static_field: read/write in static method
            (
                "static_field_rw_static",
                r#"using Arc;
void Main() {
    int c1 = Counter.GetCount();
    Counter.Increment();
    Counter.Increment();
    int c2 = Counter.GetCount();
    Console.WriteLine("ok");
}

class Counter {
    private static int _count = 0;
    public static int GetCount() { return _count; }
    public static void Increment() { _count = _count + 1; }
}
"#,
            ),
            // static_field: cross-class access
            (
                "static_field_cross_class",
                r#"using Arc;
void Main() {
    int r1 = Consumer.ReadConfig();
    Consumer.WriteConfig(100);
    int r2 = Consumer.ReadConfig();
    Console.WriteLine("ok");
}

class Config { public static int DefaultValue = 42; }
class Consumer {
    public static int ReadConfig() { return Config.DefaultValue; }
    public static void WriteConfig(int v) { Config.DefaultValue = v; }
}
"#,
            ),
            // static_field: initializer (int literal)
            (
                "static_field_initializer",
                r#"using Arc;
void Main() {
    int v = Settings.MaxRetries;
    Console.WriteLine("ok");
}

class Settings { public static int MaxRetries = 3; }
"#,
            ),
            // static_field: const field access
            (
                "const_field_access",
                r#"using Arc;
void Main() {
    int pi = MathLib.Pi;
    int max = MathLib.Max;
    int sum = MathLib.Pi + MathLib.Max;
    Console.WriteLine("ok");
}

class MathLib { public const int Pi = 3; public const int Max = 100; }
"#,
            ),
            // static_field: read/write in instance method
            (
                "static_field_rw_instance",
                r#"using Arc;
void Main() {
    Counter2 c1 = new Counter2();
    Counter2 c2 = new Counter2();
    c1.Bump(); c1.Bump(); c2.Bump();
    int total = Counter2.Total();
    int comb1 = c1.Combined();
    int comb2 = c2.Combined();
    Console.WriteLine("ok");
}

class Counter2 {
    private static int _total = 0;
    private int _local = 0;
    public void Bump() { _total = _total + 1; _local = _local + 1; }
    public int Combined() { return _total + _local; }
    public static int Total() { return _total; }
}
"#,
            ),
            // static_lazy_init: single-thread lazy
            (
                "static_lazy_init_single",
                r#"using Arc;
void Main() {
    bool ok = true;
    if (C.s_constructed != 0) { ok = false; }
    int v = C.Lazy;
    if (v != 42) { ok = false; }
    if (C.s_constructed != 1) { ok = false; }
    int v2 = C.Lazy;
    if (v2 != 42) { ok = false; }
    Console.WriteLine("ok");
}

class C {
    public static int s_constructed = 0;
    public static readonly int Lazy = Construct();
    static int Construct() { s_constructed = 1; return 42; }
}
"#,
            ),
            // static_lazy_init: concurrent first-touch (simplified compile-only)
            (
                "static_lazy_init_concurrent",
                r#"using Arc;
void Main() {
    int c0 = D.s_constructed;
    int v = D.Lazy;
    int c1 = D.s_constructed;
    int v2 = D.Lazy;
    Console.WriteLine("ok");
}

class D {
    public static int s_constructed = 0;
    public static readonly int Lazy = Construct();
    static int Construct() { s_constructed = s_constructed + 1; return 42; }
}
"#,
            ),
            // static_lazy_new: singleton with new expression
            (
                "static_lazy_new_singleton",
                r#"using Arc;
void Main() {
    int created0 = Widget.s_created;
    Widget a = Registry.Single;
    int created1 = Widget.s_created;
    Widget b = Registry.Single;
    int created2 = Widget.s_created;
    string an = (a == null) ? "null" : "nonnull";
    string bn = (b == null) ? "null" : "nonnull";
    bool ok = true;
    if (created0 != 0) { ok = false; }
    if (created1 != 1) { ok = false; }
    if (a == null) { ok = false; }
    if (created2 != 1) { ok = false; }
    if (b == null) { ok = false; }
    Console.WriteLine("ok");
}

class Widget {
    public static int s_created = 0;
    public Widget() { Widget.s_created = Widget.s_created + 1; }
}

class Registry {
    public static readonly Widget Single = new Widget();
}
"#,
            ),
            // static_lazy_new: NullLogger pattern (compile-only, simplified)
            (
                "static_lazy_null_logger",
                r#"using Arc;
void Main() {
    NullLogger a = NullLogger.Instance;
    NullLogger b = NullLogger.Instance;
    if (a == null || b == null) {
        Console.WriteLine("fail");
    } else {
        Console.WriteLine("ok");
    }
}

class NullLogger {
    private static readonly NullLogger _instance = new NullLogger();
    public static NullLogger Instance { get { return _instance; } }
    public NullLogger() {}
}
"#,
            ),
            // static_lazy_new: Brushes pattern (compile-only, simplified)
            (
                "static_lazy_brushes",
                r#"using Arc;
void Main() {
    SolidColorBrush red = Brushes.Red;
    if (red == null) {
        Console.WriteLine("fail");
    } else {
        Console.WriteLine("ok");
    }
}

class SolidColorBrush {
    public SolidColorBrush() {}
}

class Brushes {
    private static readonly SolidColorBrush _red = new SolidColorBrush();
    public static SolidColorBrush Red { get { return _red; } }
}
"#,
            ),
        ],
    );
}
