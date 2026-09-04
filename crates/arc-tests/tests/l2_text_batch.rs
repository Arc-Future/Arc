//! L2 批量运行时测试：Arc.Text 行为验证（JsonReader 转义/数值字面量等）。
//!
//! 通过 `build_and_run_batch` 合并多个 case 为一次编译 + 一次运行。
//! 每个 case 自行输出 `ARC_CASE:{name}:PASS/FAIL` 标记。
//! 需 `--features full-rt` 门控，默认 `cargo test` 不触发。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch, BatchCase};

#[test]
fn text_json_escape_batch() {
    // JsonReader \uXXXX（BMP / 代理对）与 \b\f 控制字符转义的真实运行时行为。
    let results = build_and_run_batch(
        "text_json_escape",
        &[
            BatchCase {
                name: "json_unicode_cjk",
                src: r#"using Arc;
using Arc.Text.Json;

void Main() {
    JsonReader r = new JsonReader("\"\\u4e2d\\u6587\"");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_unicode_cjk:FAIL:read"); return; }
    if (r.GetString() != "中文") { Console.WriteLine("ARC_CASE:json_unicode_cjk:FAIL:value"); return; }
    Console.WriteLine("ARC_CASE:json_unicode_cjk:PASS");
}
"#,
            },
            BatchCase {
                name: "json_unicode_ascii",
                src: r#"using Arc;
using Arc.Text.Json;

void Main() {
    JsonReader r = new JsonReader("\"A\\u0041B\"");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_unicode_ascii:FAIL:read"); return; }
    if (r.GetString() != "AAB") { Console.WriteLine("ARC_CASE:json_unicode_ascii:FAIL:value"); return; }
    Console.WriteLine("ARC_CASE:json_unicode_ascii:PASS");
}
"#,
            },
            BatchCase {
                name: "json_bf_escape",
                src: r#"using Arc;
using Arc.Text;
using Arc.Text.Json;

void Main() {
    JsonReader r = new JsonReader("\"a\\u0008b\\u000Cc\"");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_bf_escape:FAIL:read"); return; }
    string s = r.GetString();
    StringBuilder eb = new StringBuilder();
    eb.Append("a");
    eb.Append((char)8);
    eb.Append("b");
    eb.Append((char)12);
    eb.Append("c");
    if (s != eb.ToString()) { Console.WriteLine("ARC_CASE:json_bf_escape:FAIL:value"); return; }
    Console.WriteLine("ARC_CASE:json_bf_escape:PASS");
}
"#,
            },
            BatchCase {
                name: "json_surrogate_emoji",
                src: r#"using Arc;
using Arc.Text;
using Arc.Text.Json;

void Main() {
    // \uD83D\uDE00（U+1F600）代理对合并 → 4 字节 UTF-8：F0 9F 98 80。
    JsonReader r = new JsonReader("\"\\uD83D\\uDE00\"");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_surrogate_emoji:FAIL:read"); return; }
    string s = r.GetString();
    if (s.Length != 4) { Console.WriteLine("ARC_CASE:json_surrogate_emoji:FAIL:len"); return; }
    StringBuilder eb = new StringBuilder();
    eb.Append((char)240);
    eb.Append((char)159);
    eb.Append((char)152);
    eb.Append((char)128);
    if (s != eb.ToString()) { Console.WriteLine("ARC_CASE:json_surrogate_emoji:FAIL:bytes"); return; }
    Console.WriteLine("ARC_CASE:json_surrogate_emoji:PASS");
}
"#,
            },
            BatchCase {
                name: "json_exponent_int",
                src: r#"using Arc;
using Arc.Text.Json;

void Main() {
    JsonReader r = new JsonReader("[1e5, 2E3]");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_exponent_int:FAIL:open"); return; }
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_exponent_int:FAIL:num1"); return; }
    if (r.GetRawText() != "1e5") { Console.WriteLine("ARC_CASE:json_exponent_int:FAIL:num1_text"); return; }
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_exponent_int:FAIL:num2"); return; }
    if (r.GetRawText() != "2E3") { Console.WriteLine("ARC_CASE:json_exponent_int:FAIL:num2_text"); return; }
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_exponent_int:FAIL:close"); return; }
    Console.WriteLine("ARC_CASE:json_exponent_int:PASS");
}
"#,
            },
            BatchCase {
                name: "json_exponent_frac",
                src: r#"using Arc;
using Arc.Text.Json;

void Main() {
    JsonReader r = new JsonReader("{\"v\":2.5E-3}");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_exponent_frac:FAIL:open"); return; }
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_exponent_frac:FAIL:prop"); return; }
    if (r.GetString() != "v") { Console.WriteLine("ARC_CASE:json_exponent_frac:FAIL:prop_name"); return; }
    if (!r.Read()) { Console.WriteLine("ARC_CASE:json_exponent_frac:FAIL:num"); return; }
    if (r.GetRawText() != "2.5E-3") { Console.WriteLine("ARC_CASE:json_exponent_frac:FAIL:num_text"); return; }
    Console.WriteLine("ARC_CASE:json_exponent_frac:PASS");
}
"#,
            },
        ],
    );

    let r = batch_case_result(&results, "json_unicode_cjk");
    assert!(
        r.passed,
        "json_unicode_cjk failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "json_unicode_ascii");
    assert!(
        r.passed,
        "json_unicode_ascii failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "json_bf_escape");
    assert!(
        r.passed,
        "json_bf_escape failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "json_surrogate_emoji");
    assert!(
        r.passed,
        "json_surrogate_emoji failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "json_exponent_int");
    assert!(
        r.passed,
        "json_exponent_int failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "json_exponent_frac");
    assert!(
        r.passed,
        "json_exponent_frac failed: {:?} stdout: {}",
        r.error, r.stdout
    );
}

#[test]
fn text_xml_attr_batch() {
    // XmlReader 属性值归一化（XML 2.11 + 3.3.3）：字面 \r\n/\r/\n/\t → 空格，
    // 根修 _attrBlob 以 "\n" 作 name/value 分隔符的歧义；实体解码不受影响。
    let results = build_and_run_batch(
        "text_xml_attr",
        &[
            BatchCase {
                name: "xml_attr_multiline_value",
                src: r#"using Arc;
using Arc.Text.Xml;

void Main() {
    XmlReader r = new XmlReader("<a x=\"l1\nl2\" y=\"v\"/>");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:xml_attr_multiline_value:FAIL:read"); return; }
    if (r.GetAttribute("x") != "l1 l2") { Console.WriteLine("ARC_CASE:xml_attr_multiline_value:FAIL:x"); return; }
    if (r.GetAttribute("y") != "v") { Console.WriteLine("ARC_CASE:xml_attr_multiline_value:FAIL:y"); return; }
    Console.WriteLine("ARC_CASE:xml_attr_multiline_value:PASS");
}
"#,
            },
            BatchCase {
                name: "xml_attr_crlf_tab",
                src: r#"using Arc;
using Arc.Text.Xml;

void Main() {
    XmlReader r = new XmlReader("<a x=\"a\r\nb\tc\"/>");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:xml_attr_crlf_tab:FAIL:read"); return; }
    if (r.GetAttribute("x") != "a b c") { Console.WriteLine("ARC_CASE:xml_attr_crlf_tab:FAIL:x"); return; }
    Console.WriteLine("ARC_CASE:xml_attr_crlf_tab:PASS");
}
"#,
            },
            BatchCase {
                name: "xml_attr_entity_mix",
                src: r#"using Arc;
using Arc.Text.Xml;

void Main() {
    XmlReader r = new XmlReader("<a x=\"a&amp;b\nc\"/>");
    if (!r.Read()) { Console.WriteLine("ARC_CASE:xml_attr_entity_mix:FAIL:read"); return; }
    if (r.GetAttribute("x") != "a&b c") { Console.WriteLine("ARC_CASE:xml_attr_entity_mix:FAIL:x"); return; }
    Console.WriteLine("ARC_CASE:xml_attr_entity_mix:PASS");
}
"#,
            },
        ],
    );

    let r = batch_case_result(&results, "xml_attr_multiline_value");
    assert!(
        r.passed,
        "xml_attr_multiline_value failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "xml_attr_crlf_tab");
    assert!(
        r.passed,
        "xml_attr_crlf_tab failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "xml_attr_entity_mix");
    assert!(
        r.passed,
        "xml_attr_entity_mix failed: {:?} stdout: {}",
        r.error, r.stdout
    );
}
