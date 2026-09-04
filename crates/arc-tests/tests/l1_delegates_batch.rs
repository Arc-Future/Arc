//! L1 批量：委托与 Lambda 回归集（11 case）。
//!
//! 从 delegates_lambda_batch_e2e.rs 提取，改为 L1 纯编译期测试。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_delegates_batch() {
    assert_compiles_batch(
        "delegates",
        &[
            (
                "delegates_func",
                r#"using Arc;

void Main() {
    Func<int, int> f = x => x * 2;
    if (f(5) == 10) {
        Console.WriteLine("delegates_ok");
    }
}
"#,
            ),
            (
                "func_field_nocapture",
                r#"using Arc;

public class FfnHolder {
    private Func<int, int> _f;
    public void Set(Func<int, int> f) { this._f = f; }
    public int Run(int x) { return this._f(x); }
}

void Main() {
    FfnHolder h = new FfnHolder();
    h.Set(x => x + 1);
    if (h.Run(5) != 6) {
        Console.WriteLine("fail:inc");
        return;
    }
    h.Set(x => x * 3);
    if (h.Run(5) != 15) {
        Console.WriteLine("fail:mul");
        return;
    }
    Console.WriteLine("func_field_ok");
}
"#,
            ),
            (
                "lambda_capture",
                r#"using Arc;

class LcCounter { public int Value; public LcCounter(int v) { Value = v; } }

void Main() {
    LcCounter c = new LcCounter(10);
    Func<int> f = () => c.Value;
    c.Value = 20;
    if (f() != 20) {
        Console.WriteLine("fail:byref");
        return;
    }
    int x = 10;
    Func<int> g = () => x;
    x = 100;
    if (g() != 10) {
        Console.WriteLine("fail:byval");
        return;
    }
    Console.WriteLine("lambda_capture_ok");
}
"#,
            ),
            (
                "lambda_return_escape",
                r#"using Arc;

Func<int> DlMakeConst()
{
    return () => 42;
}

Func<int> DlMakeCaptured()
{
    int x = 7;
    return () => x + 1;
}

class DlBox
{
    public int V;
    public DlBox(int v) { V = v; }
}

class DlHolder
{
    private Func<int, int> _factory;
    public void Set(Func<int, int> f) { this._factory = f; }
    public int Run(int x) { return this._factory(x); }
}

DlHolder DlMakeHolder()
{
    DlBox box = new DlBox(100);
    DlHolder holder = new DlHolder();
    holder.Set(n => box.V + n);
    return holder;
}

void Main()
{
    Func<int> f1 = DlMakeConst();
    if (f1() != 42)
    {
        Console.WriteLine("fail:const");
        return;
    }
    Func<int> f2 = DlMakeCaptured();
    if (f2() != 8)
    {
        Console.WriteLine("fail:captured");
        return;
    }
    DlHolder holder = DlMakeHolder();
    if (holder.Run(5) != 105)
    {
        Console.WriteLine("fail:field");
        return;
    }
    Console.WriteLine("lambda_return_escape_ok");
}
"#,
            ),
            (
                "closure_escape",
                r#"using Arc;

class CeBox {
    public int V;
    public CeBox(int v) { V = v; }
}

Func<CeBox> DlMakeBoxClosure() {
    CeBox b = new CeBox(42);
    Func<CeBox> f = () => b;
    return f;
}

Func<CeBox> DlMakeSharedClosure() {
    CeBox b = new CeBox(1);
    Func<CeBox> get = () => b;
    return get;
}

void Main() {
    Func<CeBox> f1 = DlMakeBoxClosure();
    CeBox b1 = f1();
    if (b1 == null || b1.V != 42) {
        Console.WriteLine("fail:escape-read");
        return;
    }
    Func<CeBox> f2 = DlMakeSharedClosure();
    CeBox b2 = f2();
    if (b2 == null || b2.V != 1) {
        Console.WriteLine("fail:shared");
        return;
    }
    Console.WriteLine("closure_escape_ok");
}
"#,
            ),
            (
                "method_group_assign_arg",
                r#"using Arc;

int DlDouble(int x) { return x * 2; }
int DlApply(Func<int, int> f, int x) { return f(x); }
void DlPing() { }

void Main() {
    Func<int, int> f = DlDouble;
    int a = f(5);
    int b = DlApply(DlDouble, 5);
    Action act = DlPing;
    act();
    Console.WriteLine("" + (a + b));
}
"#,
            ),
            (
                "method_group_static",
                r#"using Arc;

class MgsC {
    public static int Double(int x) { return x * 2; }
}

int DlApply2(Func<int, int> f, int x) { return f(x); }

void Main() {
    Func<int, int> f = MgsC.Double;
    int a = f(5);
    int b = DlApply2(MgsC.Double, 5);
    Console.WriteLine("" + (a + b));
}
"#,
            ),
            (
                "method_group_instance",
                r#"using Arc;

class DlMgiC {
    public int Inc(int x) { return x + 1; }
}

void Main() {
    DlMgiC c = new DlMgiC();
    Func<int, int> g = c.Inc;
    Console.WriteLine("" + g(5));
}
"#,
            ),
            (
                "method_group_cross_fn_arg",
                r#"using Arc;

class DlMgxC {
    public int Inc(int x) { return x + 1; }
}

int DlApply3(Func<int, int> f, int x) { return f(x); }

void Main() {
    DlMgxC c = new DlMgxC();
    int viaGroup = DlApply3(c.Inc, 5);
    int viaLambda = DlApply3(x => c.Inc(x), 5);
    Console.WriteLine("" + (viaGroup * 10 + viaLambda));
}
"#,
            ),
            (
                "func_delegate_compose",
                r#"using Arc;

void Main() {
    Func<int, int> inc = x => x + 1;
    Func<int, int> dbl = x => x * 2;
    int r1 = inc(5);
    int r2 = dbl(5);
    Func<int, string> toStr = x => x.ToString();
    string s = toStr(42);
    Console.WriteLine("func_compose_ok");
}
"#,
            ),
            (
                "generic_delegate_alias",
                r#"using Arc;

delegate R Map<T, R>(T x);

void Main() {
    Map<int, int> doubleIt = x => x * 2;
    int doubled = doubleIt(21);
    Map<string, int> lengthOf = s => s.Length;
    int len = lengthOf("hello");
    if (doubled != 42 || len != 5) {
        Console.WriteLine("fail:generic_delegate");
        return;
    }
    Console.WriteLine("generic_delegate_ok");
}
"#,
            ),
        ],
    );
}
