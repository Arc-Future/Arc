//! 编译器基础面缺漏回归靶标。
//!
//! 每项测试对应 docs/plan.md 中登记的基础面缺漏。
//! 当前版本应编译失败；编译器修复后移除 #[ignore] 即变为通过测试。
//!
//! 关联缺漏编号见各 case 注释。

use arc_tests::assert_compiles_batch;

// ---------------------------------------------------------------
// 缺漏 #1：params int[] 被拒绝
// 状态：编译器要求 Span<T>/ReadOnlySpan<T>，数组写法不可用
// ---------------------------------------------------------------
#[test]
fn gap_01_params_int_array() {
    assert_compiles_batch(
        "gap_params_int_array",
        &[(
            "params_int_array",
            r#"using Arc;

class Gp1 {
    public int Sum(params int[] xs) {
        int total = 0;
        for (int i = 0; i < xs.Length; i++) {
            total = total + xs[i];
        }
        return total;
    }
}

void Main() {
    Gp1 g = new Gp1();
    if (g.Sum(1, 2, 3) != 6) {
        Console.WriteLine("fail");
        return;
    }
    Console.WriteLine("gap_01_ok");
}
"#,
        )],
    );
}

// ---------------------------------------------------------------
// 缺漏 #2：$"字符串插值" 不支持
// 状态：编译器尚未实现插值字符串
// ---------------------------------------------------------------
#[test]
fn gap_02_string_interpolation() {
    assert_compiles_batch(
        "gap_string_interp",
        &[(
            "interp_basic",
            r#"using Arc;
using Arc.Text;

void Main() {
    string name = "Arc";
    string s = $"Hello, {name}!";
    if (s == "Hello, Arc!") {
        Console.WriteLine("gap_02_ok");
    }
}
"#,
        )],
    );
}

// ---------------------------------------------------------------
// 缺漏 #3：float 字面量 f 后缀解析失败
// 状态：词法分析器不识别 C# 风格 float 后缀
// ---------------------------------------------------------------
#[test]
fn gap_03_float_suffix() {
    assert_compiles_batch(
        "gap_float_suffix",
        &[(
            "float_suffix",
            r#"using Arc;

void Main() {
    float a = 1.5f;
    float b = 2.5f;
    float c = a + b;
    Console.WriteLine("gap_03_ok");
}
"#,
        )],
    );
}

// ---------------------------------------------------------------
// 缺漏 #4：多字段声明 int X, Y; 解析失败
// 状态：解析器不支持紧凑多字段声明
// ---------------------------------------------------------------
#[test]
fn gap_04_multi_field_decl() {
    assert_compiles_batch(
        "gap_multi_field",
        &[(
            "multi_field",
            r#"using Arc;

class Gp4 {
    int X, Y;
    public Gp4(int x, int y) { X = x; Y = y; }
    public int Sum() { return X + Y; }
}

void Main() {
    Gp4 g = new Gp4(3, 4);
    if (g.Sum() != 7) {
        Console.WriteLine("fail");
        return;
    }
    Console.WriteLine("gap_04_ok");
}
"#,
        )],
    );
}

// ---------------------------------------------------------------
// 缺漏 #5：delegate 关键字解析失败
// 状态：自定义委托声明不可用，仅支持 Func<T>/Action<T>
// ---------------------------------------------------------------
#[test]
fn gap_05_delegate_keyword() {
    assert_compiles_batch(
        "gap_delegate",
        &[(
            "delegate",
            r#"using Arc;

public delegate int Converter(int value);

class Gp5 {
    public Converter Convert;
    public Gp5(Converter c) { Convert = c; }
}

void Main() {
    Gp5 g = new Gp5(v => v * 2);
    int r = g.Convert(5);
    Console.WriteLine("gap_05_ok");
}
"#,
        )],
    );
}

// ---------------------------------------------------------------
// 缺漏 #9：lambda 显式参数类型 (int x) => 解析失败
// 状态：解析器不支持带类型标注的 lambda 参数
// ---------------------------------------------------------------
#[test]
fn gap_09_lambda_typed_param() {
    assert_compiles_batch(
        "gap_lambda_typed",
        &[(
            "lambda_typed",
            r#"using Arc;

void Main() {
    Func<int, int> f = (int x) => x * 2;
    if (f(5) == 10) {
        Console.WriteLine("gap_09_ok");
    }
}
"#,
        )],
    );
}

// ---------------------------------------------------------------
// 缺漏 #10：switch case 体无花括号被拒
// 状态：解析器要求所有 case/default 分支体必须有 {}
// ---------------------------------------------------------------
#[test]
fn gap_10_switch_no_braces() {
    assert_compiles_batch(
        "gap_switch_braces",
        &[(
            "switch_no_braces",
            r#"using Arc;

void Main() {
    int x = 5;
    string r = "";
    switch (x) {
        case 1: r = "one"; break;
        case 5: r = "five"; break;
        default: r = "other"; break;
    }
    if (r == "five") {
        Console.WriteLine("gap_10_ok");
    }
}
"#,
        )],
    );
}

// ---------------------------------------------------------------
// 缺漏 #8：# 字符被视为预处理指令
// 状态：已解除——编译器无文本预处理层，字符串正则整体吞入含 # 的串，
// 注释正则消费行注释的 #；弧单文件内 `### CASE:` 行标记随测试框架
// 迁移为文件式 `case_*.as` 而废弃。此处回归校验串内 # 不被误判。
// ---------------------------------------------------------------
#[test]
fn gap_08_hash_preprocessor() {
    assert_compiles_batch(
        "gap_hash",
        &[(
            "hash_in_string",
            r#"using Arc;

void Main() {
    // 源文件中的 # 字符不应被当作预处理指令
    string s = "value#tag";
    if (s.Length > 0) {
        Console.WriteLine("gap_08_ok");
    }
}
"#,
        )],
    );
}
