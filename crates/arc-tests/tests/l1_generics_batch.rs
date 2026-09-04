//! L1 批量：泛型核心回归集（9 case）。
//!
//! 从 generics_core_batch_e2e.rs 提取，改为 L1 纯编译期测试。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_generics_batch() {
    assert_compiles_batch(
        "generics_core",
        &[
            (
                "generic_class_basic",
                r#"using Arc;

class GcBox<T> {
    public T Value;
    public GcBox(T v) { Value = v; }
    public T Get() { return Value; }
}

void Main() {
    GcBox<int> b = new GcBox<int>(42);
    if (b.Get() == 42) {
        Console.WriteLine("generic_class_ok");
    }
}
"#,
            ),
            (
                "generic_fluent_new",
                r#"using Arc;

class GfnBox<T> {
    public T Value;
    public GfnBox(T v) { Value = v; }
    public T Get() { return Value; }
}

void Main() {
    if (new GfnBox<int>(42).Get() == 42) {
        Console.WriteLine("generic_fluent_ok");
    }
}
"#,
            ),
            (
                "nested_generic_ctor",
                r#"using Arc;

class NgcInner<T> {
    public T V;
    public NgcInner(T v) { V = v; }
}

class NgcOuter<T> {
    public NgcInner<T> Child;
    public NgcOuter(T v) { Child = new NgcInner<T>(v); }
}

void Main() {
    NgcOuter<int> o = new NgcOuter<int>(7);
    if (o.Child.V == 7) {
        Console.WriteLine("nested_generic_ctor_ok");
    }
}
"#,
            ),
            (
                "static_generic_method_new",
                r#"using Arc;

class SgnBox<T> {
    public T Value;
    public SgnBox(T v) { Value = v; }
    public T Get() { return Value; }
}

class SgnHolder {
    public static SgnBox<T> Make<T>(T v) { return new SgnBox<T>(v); }
}

void Main() {
    SgnBox<int> b = SgnHolder.Make<int>(42);
    if (b.Get() == 42) {
        Console.WriteLine("static_generic_new_ok");
    }
}
"#,
            ),
            (
                "nested_type_arg",
                r#"using Arc;

class NtaBox<T> {
    public T Value;
    public NtaBox(T v) { Value = v; }
}

void Main() {
    NtaBox<NtaBox<int>> b = new NtaBox<NtaBox<int>>(new NtaBox<int>(1));
    if (b.Value.Value == 1) {
        Console.WriteLine("nested_type_arg_ok");
    }
}
"#,
            ),
            (
                "inst_generic_chain",
                r#"using Arc;

class IgcBox<T> {
    public T Value;
    public IgcBox(T v) { Value = v; }
    public T Get() { return Value; }
}

class IgcChain {
    public IgcBox<T> Leaf<T>(T v) { return new IgcBox<T>(v); }
    public IgcBox<T> Wrap<T>(T v) { return this.Leaf<T>(v); }
}

void Main() {
    IgcChain c = new IgcChain();
    IgcBox<int> b = c.Wrap<int>(7);
    if (b.Get() == 7) {
        Console.WriteLine("inst_generic_chain_ok");
    }
}
"#,
            ),
            (
                "inst_generic_on_generic_class",
                r#"using Arc;

class IogBox<T> {
    public T Value;
    public IogBox(T v) { Value = v; }
    public T Get() { return Value; }
}

class IogMapper<T> {
    public T Seed;
    public IogMapper(T s) { Seed = s; }
    public IogBox<U> Map<U>(U u) { return new IogBox<U>(u); }
}

void Main() {
    IogMapper<int> m = new IogMapper<int>(1);
    IogBox<string> b = m.Map<string>("hi");
    if (b.Get() != "hi") {
        Console.WriteLine("fail:map-string");
        return;
    }
    IogBox<int> c = m.Map<int>(42);
    if (c.Get() != 42) {
        Console.WriteLine("fail:map-int");
        return;
    }
    Console.WriteLine("inst_generic_on_class_ok");
}
"#,
            ),
            (
                "inst_generic_nested_ctor",
                r#"using Arc;

class IngInner<T> {
    public T V;
    public IngInner(T v) { V = v; }
}

class IngOuter<T> {
    public IngInner<T> Child;
    public IngOuter(T v) { Child = new IngInner<T>(v); }
}

class IngFactory {
    public IngOuter<T> Build<T>(T v) { return new IngOuter<T>(v); }
}

void Main() {
    IngFactory f = new IngFactory();
    IngOuter<int> o = f.Build<int>(7);
    if (o.Child.V == 7) {
        Console.WriteLine("inst_generic_nested_ok");
    }
}
"#,
            ),
            (
                "iface_generic_dispatch",
                r#"using Arc;

interface ISeed {
    int Value();
}

class IgmSeed : ISeed {
    public int _v;
    public IgmSeed(int v) { _v = v; }
    public int Value() { return _v; }
}

interface IGetter {
    T Get<T>(T seed) where T : ISeed;
}

class IgmFoo : IGetter {
    public T Get<T>(T seed) where T : ISeed {
        int v = seed.Value();
        return seed;
    }
}

class IgmBar : IGetter {
    public T Get<T>(T seed) where T : ISeed {
        return seed;
    }
}

void Main() {
    IGetter a = new IgmFoo();
    IGetter b = new IgmBar();
    IgmSeed s = new IgmSeed(42);
    if (a.Get<IgmSeed>(s).Value() != 42) {
        Console.WriteLine("fail:iface-generic-foo");
        return;
    }
    if (b.Get<IgmSeed>(s).Value() != 42) {
        Console.WriteLine("fail:iface-generic-bar");
        return;
    }
    Console.WriteLine("iface_generic_ok");
}
"#,
            ),
        ],
    );
}
