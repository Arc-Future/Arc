use arc_tests::assert_compiles_batch;

#[test]
fn compiles_field_keyword_batch() {
    assert_compiles_batch(
        "field_keyword",
        &[
            (
                "fk_struct_rw",
                r#"using Arc;

struct FkCounter {
    public int X { get { return field; } set { field = value; } }
}

void Main() {
    FkCounter c = new FkCounter();
    c.X = 42;
    if (c.X != 42) {
        Console.WriteLine("fail rw");
        return;
    }
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
    w.Balance = -5;
    if (w.Balance != 100) {
        Console.WriteLine("fail balance");
        return;
    }
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
    if (p.X != 3) {
        Console.WriteLine("fail x");
        return;
    }
    if (p.Y != 4) {
        Console.WriteLine("fail y");
        return;
    }
    Console.WriteLine("field class init ok");
}
"#,
            ),
            (
                "fk_auto_regression",
                r#"using Arc;

class FkBox {
    public int Value { get; set; }
}

void Main() {
    FkBox b = new FkBox();
    b.Value = 7;
    if (b.Value != 7) {
        Console.WriteLine("fail auto");
        return;
    }
    Console.WriteLine("field auto regression ok");
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
    if (c.Compute() != 7) {
        Console.WriteLine("fail var");
        return;
    }
    Console.WriteLine("field ordinary var ok");
}
"#,
            ),
            (
                "fk_no_field_regression",
                r#"using Arc;

class FkKeeper {
    private int _v;
    public int V { get { return _v; } set { _v = value; } }
}

void Main() {
    FkKeeper k = new FkKeeper();
    k.V = 9;
    if (k.V != 9) {
        Console.WriteLine("fail");
        return;
    }
    Console.WriteLine("field no-field regression ok");
}
"#,
            ),
            (
                "fk_generic",
                r#"using Arc;

class FkHolder<T> {
    public T Value { get { return field; } set { field = value; } }
}

void Main() {
    FkHolder<int> h = new FkHolder<int>();
    h.Value = 42;
    if (h.Value != 42) {
        Console.WriteLine("fail generic int");
        return;
    }
    FkHolder<string> s = new FkHolder<string>();
    s.Value = "hi";
    if (s.Value != "hi") {
        Console.WriteLine("fail generic str");
        return;
    }
    Console.WriteLine("field generic ok");
}
"#,
            ),
        ],
    );
}
