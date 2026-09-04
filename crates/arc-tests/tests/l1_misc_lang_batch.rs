//! L1 compile-only batch: Misc language features.

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_l1_misc_lang_batch() {
    assert_compiles_batch(
        "l1_misc_lang_batch",
        &[
            (
                "hello_world",
                r#"using Arc;
void Main() {
    bool eq = "Hello, World!" == "Hello, World!";
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "using_var_disposable",
                r#"using Arc;
class Res : IDisposable {
    public string Name;
    public Res(string n) { Name = n; }
    public void Dispose() {}
}
void Main() {
    using var r = new Res("t");
    string name = r.Name;
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "console_color",
                r#"using Arc;
void Main() {
    Console.SetForegroundColor(12);
    int fg = Console.GetForegroundColor();
    Console.ResetColor();
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "box_nested",
                r#"using Arc;
class Box<T> {
    public T Value;
    public Box(T v) { Value = v; }
}
void Main() {
    Box<Box<string>> b = new Box<Box<string>>(new Box<string>("hi"));
    string v = b.Value.Value;
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "struct_init",
                r#"using Arc;
struct User { public int Age; public string Name; }
void Main() {
    User u = new User();
    u.Age = 30;
    u.Name = "Ada";
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "array_index",
                r#"using Arc;
void Main() {
    int[] arr = new int[3];
    arr[0] = 10; arr[1] = 20; arr[2] = 30;
    int total = arr[0] + arr[1] + arr[2];
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "string_ops",
                r#"using Arc;
void Main() {
    string s = "hello";
    int len = s.Length;
    char c = s[0];
    string upper = s.ToUpper();
    bool contains = s.Contains("ell");
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "datetime_now",
                r#"using Arc;
void Main() {
    DateTime now = DateTime.Now;
    int year = now.Year;
    int month = now.Month;
    Console.WriteLine("ok");
}
"#,
            ),
        ],
    );
}
