//! L1 批量：属性初始化器回归集（10 case）。
//!
//! 从 props_init_batch_e2e.rs 提取，改为 L1 纯编译期测试。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_props_init_batch() {
    assert_compiles_batch(
        "props_init",
        &[
            (
                "init_m1",
                r#"using Arc;

class Ia1Person {
    public string Name { get; init; }
    public int Age { get; init; }

    public Ia1Person(string name) {
        Name = name;
    }
}

void Main() {
    Ia1Person p = new Ia1Person("a") { Age = 10 };
    if (p.Name != "a") {
        Console.WriteLine("fail name");
        return;
    }
    if (p.Age != 10) {
        Console.WriteLine("fail age");
        return;
    }
    Console.WriteLine("init accessor m1 ok");
}
"#,
            ),
            (
                "init_m2_custom",
                r#"using Arc;

class Ia2Counter {
    int _n;
    public int N {
        get { return _n; }
        init { _n = value; }
    }
    public Ia2Counter(int n) {
        this.N = n;
    }
}

void Main() {
    Ia2Counter c = new Ia2Counter(7);
    if (c.N != 7) {
        Console.WriteLine("fail custom init");
        return;
    }
    Console.WriteLine("init accessor m2 ok");
}
"#,
            ),
            (
                "required_m3",
                r#"using Arc;

class Ia3Person {
    public required string Name { get; init; }
    public int Age { get; init; }
}

void Main() {
    Ia3Person p = new Ia3Person() { Name = "a", Age = 10 };
    if (p.Name != "a") {
        Console.WriteLine("fail name");
        return;
    }
    if (p.Age != 10) {
        Console.WriteLine("fail age");
        return;
    }
    Console.WriteLine("required member m3 ok");
}
"#,
            ),
            (
                "required_ctor_m4",
                r#"using Arc;

class Ia4Person {
    public required string Name { get; init; }
    public int Age { get; init; }

    public Ia4Person(string name) {
        Name = name;
    }
}

void Main() {
    Ia4Person p = new Ia4Person("a") { Age = 10 };
    if (p.Name != "a") {
        Console.WriteLine("fail name");
        return;
    }
    if (p.Age != 10) {
        Console.WriteLine("fail age");
        return;
    }
    Ia4Person q = new Ia4Person("b");
    if (q.Name != "b") {
        Console.WriteLine("fail q name");
        return;
    }
    Console.WriteLine("required ctor sets m4 ok");
}
"#,
            ),
            (
                "init_m5_obj_init",
                r#"using Arc;

class Ia5Counter {
    int _n;
    public int N {
        get { return _n; }
        init { _n = value; }
    }
    public Ia5Counter() {}
}

void Main() {
    Ia5Counter c = new Ia5Counter() { N = 7 };
    if (c.N != 7) {
        Console.WriteLine("fail custom obj init");
        return;
    }
    Console.WriteLine("init accessor m5 obj init ok");
}
"#,
            ),
            (
                "init_m5plus_with",
                r#"using Arc;

record Ia5pCounter {
    int _n;
    public int N {
        get { return _n; }
        init { _n = value; }
    }
    public Ia5pCounter(int n) { _n = n; }
}

void Main() {
    Ia5pCounter c = new Ia5pCounter(1);
    Ia5pCounter d = c with { N = 7 };
    if (d.N != 7) {
        Console.WriteLine("fail with custom init");
        return;
    }
    if (c.N != 1) {
        Console.WriteLine("fail source mutated");
        return;
    }
    Console.WriteLine("init accessor m5plus with ok");
}
"#,
            ),
            (
                "private_set",
                r#"using Arc;

class Pav1Person {
    public string Name { get; private set; }
    public Pav1Person(string n) { Name = n; }
    public void Rename(string n) { Name = n; }
}

void Main() {
    Pav1Person p = new Pav1Person("a");
    p.Rename("b");
    if (p.Name != "b") {
        Console.WriteLine("fail read");
        return;
    }
    Console.WriteLine("private set ok");
}
"#,
            ),
            (
                "protected_set",
                r#"using Arc;

class Pav2Base {
    public int X { get; protected set; }
    public Pav2Base() {}
}

class Pav2Derived : Pav2Base {
    public void SetX(int v) { X = v; }
}

void Main() {
    Pav2Derived d = new Pav2Derived();
    d.SetX(7);
    if (d.X != 7) {
        Console.WriteLine("fail read");
        return;
    }
    Console.WriteLine("protected set ok");
}
"#,
            ),
            (
                "no_acc_vis",
                r#"using Arc;

class Pav3Counter {
    public int N { get; set; }
    public Pav3Counter() {}
}

void Main() {
    Pav3Counter c = new Pav3Counter();
    c.N = 5;
    if (c.N != 5) {
        Console.WriteLine("fail");
        return;
    }
    Console.WriteLine("no accessor vis ok");
}
"#,
            ),
            (
                "private_init",
                r#"using Arc;

class Pav4Person {
    public string Name { get; private init; }
    public Pav4Person(string n) { Name = n; }
}

void Main() {
    Pav4Person p = new Pav4Person("a");
    if (p.Name != "a") {
        Console.WriteLine("fail read");
        return;
    }
    Console.WriteLine("private init ok");
}
"#,
            ),
        ],
    );
}
