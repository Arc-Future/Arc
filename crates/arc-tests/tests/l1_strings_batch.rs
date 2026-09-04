//! L1 批量：字符串操作回归集（4 case）。
//!
//! 从 string_batch_e2e.rs 提取，改为 L1 纯编译期测试。
//! 排除字符串插值（编译器尚未支持）。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_string_batch() {
    assert_compiles_batch(
        "strings",
        &[
            (
                "string_split_join",
                r#"using Arc;

void Main() {
    string[] parts = "a,b,c".Split(',');
    if (parts.Length != 3 || parts[0] != "a") {
        Console.WriteLine("fail:char-sep");
        return;
    }
    if (string.Join("-", parts) != "a-b-c") {
        Console.WriteLine("fail:join");
        return;
    }
    Console.WriteLine("string_split_join_ok");
}
"#,
            ),
            (
                "string_split_options",
                r#"using Arc;

void Main() {
    string[] parts = ",a,,b,".Split(',', StringSplitOptions.RemoveEmptyEntries);
    if (parts.Length != 2 || parts[0] != "a") {
        Console.WriteLine("fail:remove-empty");
        return;
    }
    if (string.Join("|", parts) != "a|b") {
        Console.WriteLine("fail:remove-empty-join");
        return;
    }
    Console.WriteLine("string_split_options_ok");
}
"#,
            ),
            (
                "string_trim_replace",
                r#"using Arc;

void Main() {
    if ("--hi--".Trim('-') != "hi") {
        Console.WriteLine("fail:trim");
        return;
    }
    if ("hello".Replace("l", "x") != "hexxo") {
        Console.WriteLine("fail:multi-hit");
        return;
    }
    if ("ab".Replace("b", "") != "a") {
        Console.WriteLine("fail:remove-end");
        return;
    }
    Console.WriteLine("string_trim_replace_ok");
}
"#,
            ),
            (
                "string_char_index",
                r#"using Arc;

void Main() {
    string s = "hi";
    Console.WriteLine("" + (int)s[0]);
    Console.WriteLine("" + (int)s[1]);
    Console.WriteLine("" + s.Length);
    Console.WriteLine("string_char_index_ok");
}
"#,
            ),
        ],
    );
}
