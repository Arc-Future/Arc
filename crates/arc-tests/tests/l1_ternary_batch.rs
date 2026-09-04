//! L1 批量：三元表达式回归集（10 case）。
//!
//! 从 ternary_batch_e2e.rs 提取，改为 L1 纯编译期测试。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_ternary_batch() {
    assert_compiles_batch(
        "ternary",
        &[
            (
                "basic_int",
                r#"using Arc;

void Main() {
    int a = true ? 10 : 20;
    Console.Write("a=");
    Console.WriteLine(a.ToString());
}
"#,
            ),
            (
                "basic_string",
                r#"using Arc;

void Main() {
    bool cond = false;
    string s = cond ? "yes" : "no";
    Console.WriteLine(s);
}
"#,
            ),
            (
                "comparison",
                r#"using Arc;

void Main() {
    int a = 5 > 3 ? 10 : 20;
    Console.Write("gt=");
    Console.WriteLine(a.ToString());

    int b = 3 > 5 ? 10 : 20;
    Console.Write("lt=");
    Console.WriteLine(b.ToString());

    int c = 5 == 5 ? 100 : 200;
    Console.Write("eq=");
    Console.WriteLine(c.ToString());

    int d = 5 != 5 ? 100 : 200;
    Console.Write("ne=");
    Console.WriteLine(d.ToString());
}
"#,
            ),
            (
                "nested_right",
                r#"using Arc;

void Main() {
    int a = 10;
    int b = a < 5 ? 1 : a < 15 ? 2 : 3;
    Console.Write("nested1=");
    Console.WriteLine(b.ToString());

    int c = a < 5 ? 1 : a < 20 ? 4 : 5;
    Console.Write("nested2=");
    Console.WriteLine(c.ToString());
}
"#,
            ),
            (
                "in_function_call",
                r#"using Arc;

void Main() {
    int x = 42;
    Console.WriteLine(x > 0 ? "positive" : "non-positive");
    Console.WriteLine(x < 0 ? "negative" : "non-negative");
}
"#,
            ),
            (
                "return_stmt",
                r#"using Arc;

int TnCompare(int a, int b) {
    return a > b ? 1 : -1;
}

void Main() {
    Console.Write("r1="); Console.WriteLine(TnCompare(5, 3).ToString());
    Console.Write("r2="); Console.WriteLine(TnCompare(3, 5).ToString());
}
"#,
            ),
            (
                "multi_assign",
                r#"using Arc;

void Main() {
    int count = 3;
    int ma = count >= 1 ? 10 : 0;
    int mi = count >= 2 ? 20 : 0;
    int bu = count >= 3 ? 30 : 0;
    int re = count >= 4 ? 40 : 0;
    Console.Write("ma="); Console.WriteLine(ma.ToString());
    Console.Write("mi="); Console.WriteLine(mi.ToString());
    Console.Write("bu="); Console.WriteLine(bu.ToString());
    Console.Write("re="); Console.WriteLine(re.ToString());
}
"#,
            ),
            (
                "new_expr",
                r#"using Arc;

class TnNum {
    private int _val;
    public TnNum(int v) { _val = v; }
    public int Value { get { return this._val; } }
}

void Main() {
    int ticks = -5;
    TnNum n = ticks < 0 ? new TnNum(-ticks) : new TnNum(ticks);
    Console.Write("v="); Console.WriteLine(n.Value.ToString());
}
"#,
            ),
            (
                "deep_nesting",
                r#"using Arc;

void Main() {
    int a = true  ? 2 : true  ? 4 : true  ? 6 : 7;
    Console.Write("a="); Console.WriteLine(a.ToString());

    int b = false ? 2 : false ? 4 : false ? 6 : 7;
    Console.Write("b="); Console.WriteLine(b.ToString());

    int c = false ? 2 : true  ? 4 : false ? 6 : 7;
    Console.Write("c="); Console.WriteLine(c.ToString());
}
"#,
            ),
            (
                "bool_var",
                r#"using Arc;

string TnEval(int score) {
    bool pass = score >= 60;
    return pass ? "pass" : "fail";
}

void Main() {
    Console.Write("r1="); Console.WriteLine(TnEval(80));
    Console.Write("r2="); Console.WriteLine(TnEval(30));
}
"#,
            ),
        ],
    );
}
