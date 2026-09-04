//! L1 批量：语言类型系统回归集（整型、数值、可空、默认表达式、const/readonly）。
//!
//! 所有 case 合并为单次 assert_compiles_batch。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_lang_types_batch() {
    assert_compiles_batch(
        "lang_types",
        &[
            (
                "integer_types",
                r#"using Arc;
void Main() {
    long big = 1000000000;
    short s = 100;
    short t = 200;
    int sum = s + t;
    Console.WriteLine("integer_types_ok");
}
"#,
            ),
            (
                "unsigned_types",
                r#"using Arc;
void Main() {
    uint u = 42;
    uint p = uint.Parse("123");
    Console.WriteLine("unsigned_types_ok");
}
"#,
            ),
            (
                "numeric_types",
                r#"using Arc;
void Main() {
    double x = 2.0;
    double y = 3.0;
    double z = x + y;
    float a = 1.5;
    float b = 2.5;
    float c = a + b;
    Console.WriteLine("numeric_types_ok");
}
"#,
            ),
            (
                "char_classify",
                r#"using Arc;
void Main() {
    bool d0 = char.IsDigit('0');
    bool lA = char.IsLetter('A');
    bool w = char.IsWhiteSpace(' ');
    Console.WriteLine("char_classify_ok");
}
"#,
            ),
            (
                "convert_radix",
                r#"using Arc;
void Main() {
    int hex = Convert.ToInt32("FF", 16);
    string s = Convert.ToString(255, 16);
    long bin = Convert.ToInt64("101010", 2);
    Console.WriteLine("convert_radix_ok");
}
"#,
            ),
            (
                "nullable_types",
                r#"using Arc;
void Main() {
    string? s = null;
    string r = s ?? "def";
    string? t = "hi";
    int? len = t?.Length;
    Console.WriteLine("nullable_types_ok");
}
"#,
            ),
            (
                "nullable_narrowing",
                r#"using Arc;
void Main() {
    string? s = "hi";
    if (s != null) {
        int l = s.Length;
    }
    Console.WriteLine("nullable_narrowing_ok");
}
"#,
            ),
            (
                "default_expr",
                r#"using Arc;
void Main() {
    int di = default(int);
    bool db = default(bool);
    Console.WriteLine("default_expr_ok");
}
"#,
            ),
            (
                "const_readonly",
                r#"using Arc;
class Calc {
    public const int Mul = 2;
    public readonly int Off;
    public Calc(int o) { Off = o; }
}
void Main() {
    Calc c = new Calc(5);
    Console.WriteLine("const_readonly_ok");
}
"#,
            ),
            (
                "int_math",
                r#"using Arc;
void Main() {
    int a = Math.Abs(-5);
    double f = Math.Floor(3.7);
    double c = Math.Clamp(20.0, 0.0, 10.0);
    Console.WriteLine("int_math_ok");
}
"#,
            ),
        ],
    );
}
