use arc_tests::assert_compiles_batch;

#[test]
fn compiles_interface_is_batch() {
    assert_compiles_batch(
        "interface_is",
        &[
            (
                "iface_is_direct",
                r#"using Arc;

interface IMarker {
    string Tag();
}

interface IOther {
    string Other();
}

class Widget1 {
    public virtual string Kind() { return "widget"; }
}

class Impl1 : Widget1, IMarker {
    public override string Kind() { return "impl"; }
    public string Tag() { return "ok"; }
}

class Plain1 : Widget1 {
    public override string Kind() { return "plain"; }
}

void Main() {
    Widget1 w = new Impl1();
    if (!(w is IMarker)) { Console.WriteLine("fail"); }
    if (w is IOther) { Console.WriteLine("fail"); }
    Widget1 plain = new Plain1();
    if (plain is IMarker) { Console.WriteLine("fail"); }
    Widget1 n = null;
    if (n is IMarker) { Console.WriteLine("fail"); }
}
"#,
            ),
            (
                "iface_is_inherited",
                r#"using Arc;

interface IBase2 {
    string BaseTag();
}

interface IChild2 : IBase2 {
    string ChildTag();
}

class Widget2 {
    public virtual string Kind() { return "widget"; }
}

class Impl2 : Widget2, IChild2 {
    public override string Kind() { return "impl"; }
    public string BaseTag() { return "base"; }
    public string ChildTag() { return "child"; }
}

class Plain2 : Widget2 {
    public override string Kind() { return "plain"; }
}

void Main() {
    Widget2 w = new Impl2();
    if (!(w is IChild2)) { Console.WriteLine("fail"); }
    if (!(w is IBase2)) { Console.WriteLine("fail"); }
    Widget2 plain = new Plain2();
    if (plain is IBase2) { Console.WriteLine("fail"); }
}
"#,
            ),
            (
                "iface_is_generic_inherited",
                r#"using Arc;

interface IBase3<T> {
    T Get();
}

interface IChild3<T> : IBase3<T> {
    void Set(T value);
}

class Widget3 {
    public virtual string Kind() { return "widget"; }
}

class Box3 : Widget3, IChild3<int> {
    int _v;
    public Box3() { this._v = 0; }
    public override string Kind() { return "box"; }
    public int Get() { return this._v; }
    public void Set(int value) { this._v = value; }
}

class Plain3 : Widget3 {
    public override string Kind() { return "plain"; }
}

void Main() {
    Widget3 w = new Box3();
    if (!(w is IChild3<int>)) { Console.WriteLine("fail"); }
    if (!(w is IBase3<int>)) { Console.WriteLine("fail"); }
    Widget3 plain = new Plain3();
    if (plain is IBase3<int>) { Console.WriteLine("fail"); }
}
"#,
            ),
            (
                "iface_is_bind_direct",
                r#"using Arc;

interface IMarker4 {
    string Tag();
}

class Widget4 {
    public virtual string Kind() { return "widget"; }
}

class Impl4 : Widget4, IMarker4 {
    public override string Kind() { return "impl"; }
    public string Tag() { return "ok"; }
}

void Main() {
    Widget4 w = new Impl4();
    if (w is IMarker4 m) {
        if (m.Tag().Length != 2) { Console.WriteLine("fail"); }
    } else {
        Console.WriteLine("fail");
    }
}
"#,
            ),
            (
                "iface_is_bind_inherited",
                r#"using Arc;

interface IBase5 {
    string BaseTag();
}

interface IChild5 : IBase5 {
    string ChildTag();
}

class Widget5 {
    public virtual string Kind() { return "widget"; }
}

class Impl5 : Widget5, IChild5 {
    public override string Kind() { return "impl"; }
    public string BaseTag() { return "base"; }
    public string ChildTag() { return "child"; }
}

void Main() {
    Widget5 w = new Impl5();
    if (w is IBase5 b) {
        if (b.BaseTag().Length != 4) { Console.WriteLine("fail"); }
    } else {
        Console.WriteLine("fail");
    }
}
"#,
            ),
            (
                "iface_is_bind_generic_inherited",
                r#"using Arc;

interface IBase6<T> {
    T Get();
}

interface IChild6<T> : IBase6<T> {
    void Set(T value);
}

class Widget6 {
    public virtual string Kind() { return "widget"; }
}

class Box6 : Widget6, IChild6<int> {
    int _v;
    public Box6() { this._v = 7; }
    public override string Kind() { return "box"; }
    public int Get() { return this._v; }
    public void Set(int value) { this._v = value; }
}

void Main() {
    Widget6 w = new Box6();
    if (w is IBase6<int> b) {
        if (b.Get() != 7) { Console.WriteLine("fail"); }
    } else {
        Console.WriteLine("fail");
    }
}
"#,
            ),
            (
                "cd11_interface_override",
                r#"using Arc;

interface ITalk7 {
    string Talk();
}

class TalkBase7 : ITalk7 {
    public virtual string Talk() { return "base-talk"; }
}

class TalkDerived7 : TalkBase7 {
    public override string Talk() { return "derived-talk"; }
}

void Main() {
    TalkDerived7 td = new TalkDerived7();
    ITalk7 it = td;
    if (it.Talk() != "derived-talk") { Console.WriteLine("fail"); }
    if (!(td is ITalk7)) { Console.WriteLine("fail"); }
}
"#,
            ),
            (
                "cd12_interface_inheritance",
                r#"using Arc;

interface IBase8 {
    string BaseTag();
}

interface IChild8 : IBase8 {
    string ChildTag();
}

class Widget8 {
    public virtual string Kind() { return "widget"; }
}

class Impl8 : Widget8, IChild8 {
    public override string Kind() { return "impl"; }
    public string BaseTag() { return "base"; }
    public string ChildTag() { return "child"; }
}

void Main() {
    IChild8 c = new Impl8();
    if (c.BaseTag() != "base") { Console.WriteLine("fail"); }
    if (c.ChildTag() != "child") { Console.WriteLine("fail"); }

    Widget8 w = new Impl8();
    if (w is IChild8 cc) {
        if (cc.BaseTag() != "base") { Console.WriteLine("fail"); }
    } else {
        Console.WriteLine("fail");
    }
}
"#,
            ),
        ],
    );
}
