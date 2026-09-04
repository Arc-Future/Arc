//! L1 批量：Content 变体编译期回归集（4 case）。
//!
//! 从 content_variant_implicit_e2e.rs 提取核心语言特性 case。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_content_variant_batch() {
    assert_compiles_batch(
        "content_variant",
        &[
            (
                "implicit_let_init",
                r#"using Arc;

variant ContentV1 {
    | None
    | Text of string
}

void Main() {
    ContentV1 c = "Click";
    string r = c switch {
        ContentV1.Text(s) => s,
        ContentV1.None => "none",
        _ => "other"
    };
    Console.WriteLine(r);
}
"#,
            ),
            (
                "implicit_property_setter",
                r#"using Arc;

variant ContentV2 {
    | None
    | Text of string
}

class Button {
    public ContentV2 Content { get; set; }
}

void Main() {
    Button btn = new Button();
    btn.Content = "Click Me";
    ContentV2 c = btn.Content;
    string r = c switch {
        ContentV2.Text(s) => s,
        _ => "other"
    };
    Console.WriteLine(r);
}
"#,
            ),
            (
                "explicit_construct",
                r#"using Arc;

variant ContentV3 {
    | None
    | Text of string
}

void Main() {
    ContentV3 c = ContentV3.Text("Hello");
    string r = c switch {
        ContentV3.Text(s) => s,
        _ => "other"
    };
    Console.WriteLine(r);
}
"#,
            ),
            (
                "none_default",
                r#"using Arc;

variant ContentV4 {
    | None
    | Text of string
}

class ContentControl {
    public ContentV4 Content { get; set; }
}

void Main() {
    ContentControl cc = new ContentControl();
    ContentV4 c = cc.Content;
    bool isNone = c switch {
        ContentV4.None => true,
        _ => false
    };
    Console.WriteLine("none");
}
"#,
            ),
        ],
    );
}
