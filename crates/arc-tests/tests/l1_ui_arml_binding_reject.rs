//! L1：`{x:Bind}` 与错误路径必须编译失败（指向 `{Binding}` / 未知成员）。

use arc_tests::{assert_compiles_with_deps, assert_rejected_with_deps};
use arc_ui::{generate, CodegenOptions, Parser};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];

fn strip_namespace_and_using(src: &str) -> String {
    src.lines()
        .filter(|line| {
            let t = line.trim_start();
            !(t.starts_with("namespace ") && t.ends_with(';'))
                && !(t.starts_with("using ") && t.ends_with(';'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn arml_xbind_generate_rejected() {
    let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBlock Text="{x:Bind Title}"/>
</Window>"#;
    let err = generate(
        &Parser::parse(src).expect("parse"),
        &CodegenOptions::default(),
    )
    .unwrap_err();
    assert!(err.contains("x:Bind"), "{err}");
    assert!(err.contains("{Binding Path}"), "{err}");
}

#[test]
fn arml_binding_unknown_path_compile_fail() {
    let arml = r#"<Window Title="T" Width="100" Height="100" Class="Ns.MissWin">
    <TextBlock Text="{Binding NoSuchProp, Mode=OneTime}"/>
</Window>"#;
    let generated = generate(
        &Parser::parse(arml).expect("parse"),
        &CodegenOptions {
            namespace: "Ns".into(),
            ..CodegenOptions::default()
        },
    )
    .expect("generate (path checked at Arc typeck)");
    let body = strip_namespace_and_using(&generated).replacen("public partial class ", "public class ", 1);
    let src = format!(
        r#"using Arc;
using Arc.UI.Components;

{body}

void Main()
{{
    MissWin w = new MissWin();
    w.InitializeComponent();
}}
"#
    );
    assert_rejected_with_deps(
        "ui_arml_binding_unknown_path",
        &src,
        "NoSuchProp",
        UI_DEPS,
    );
}

#[test]
fn arml_binding_unknown_nested_path_compile_fail() {
    let arml = r#"<Window Title="T" Width="100" Height="100" Class="Ns.MissNest">
    <TextBlock Text="{Binding Model.NoSuch, Mode=OneTime}"/>
</Window>"#;
    let generated = generate(
        &Parser::parse(arml).expect("parse"),
        &CodegenOptions {
            namespace: "Ns".into(),
            ..CodegenOptions::default()
        },
    )
    .expect("generate (nested path checked at Arc typeck)");
    let body = strip_namespace_and_using(&generated).replacen("public partial class ", "public class ", 1);
    let src = format!(
        r#"using Arc;
using Arc.UI.Components;

{body}

void Main()
{{
    MissNest w = new MissNest();
    w.InitializeComponent();
}}
"#
    );
    assert_rejected_with_deps(
        "ui_arml_binding_unknown_nested_path",
        &src,
        "Model",
        UI_DEPS,
    );
}

#[test]
fn arml_binding_twoway_without_observable_generate_fail() {
    let arml = r#"<Window Title="T" Width="100" Height="100" Class="Ns.TwowayPlain">
    <TextBox Text="{Binding Name, Mode=TwoWay}"/>
</Window>"#;
    let err = generate(
        &Parser::parse(arml).expect("parse"),
        &CodegenOptions {
            namespace: "Ns".into(),
            companion_source: Some("    public string Name { get; set; }\n".into()),
            ..CodegenOptions::default()
        },
    )
    .unwrap_err();
    assert!(err.contains("TwoWay"), "{err}");
    assert!(err.contains("[Observable]"), "{err}");
    assert!(!err.contains("cannot bind"), "{err}");
}

#[test]
fn arml_binding_twoway_without_observable_compile_fail() {
    // `Name` is `Element.Name` — do not hide it. Own a distinct plain property.
    let arml = r#"<Window Title="T" Width="100" Height="100" Class="Ns.TwowayCk">
    <TextBox Text="{Binding FullName, Mode=TwoWay}"/>
</Window>"#;
    let generated = generate(
        &Parser::parse(arml).expect("parse"),
        &CodegenOptions {
            namespace: "Ns".into(),
            ..CodegenOptions::default()
        },
    )
    .expect("TwoWay without metadata still emits ObserveProperty");
    let mut body =
        strip_namespace_and_using(&generated).replacen("public partial class ", "public class ", 1);
    let marker = "class TwowayCk";
    let start = body.find(marker).expect("class decl");
    let open = start + body[start..].find('{').expect("class open");
    body.insert_str(open + 1, "\n    public string FullName { get; set; }\n");
    let src = format!(
        r#"using Arc;
using Arc.UI.Components;

{body}

void Main()
{{
    TwowayCk w = new TwowayCk();
    w.InitializeComponent();
}}
"#
    );
    assert_rejected_with_deps(
        "ui_arml_binding_twoway_plain",
        &src,
        "TwoWay",
        UI_DEPS,
    );
}

#[test]
fn arml_binding_datatype_unknown_path_compile_fail() {
    let arml = r#"<Window Title="T" Width="100" Height="100" Class="Ns.DcMissWin" DataType="DcMiss">
    <TextBlock Text="{Binding NoSuchProp, Mode=OneTime}"/>
</Window>"#;
    let generated = generate(
        &Parser::parse(arml).expect("parse"),
        &CodegenOptions {
            namespace: "Ns".into(),
            ..CodegenOptions::default()
        },
    )
    .expect("generate");
    assert!(
        generated.contains("ContextRoot"),
        "DataType must bind via ContextRoot:\n{generated}"
    );
    let body =
        strip_namespace_and_using(&generated).replacen("public partial class ", "public class ", 1);
    let src = format!(
        r#"using Arc;
using Arc.UI.Components;

public class DcMiss {{
}}

{body}

void Main()
{{
    DcMissWin w = new DcMissWin();
    w.InitializeComponent();
}}
"#
    );
    assert_rejected_with_deps(
        "ui_arml_binding_datatype_unknown",
        &src,
        "NoSuchProp",
        UI_DEPS,
    );
}

#[test]
fn arml_binding_nested_observable_leaf_after_window_compiles() {
    let leaf = r#"
public class NestLeaf {
    [Observable] public string Name { get; set; }
}
"#;
    let arml = r#"<Window Title="T" Width="100" Height="100" Class="Ns.NestLeafWin">
    <TextBlock Text="{Binding Model.Name, Mode=OneWay}"/>
</Window>"#;
    let generated = generate(
        &Parser::parse(arml).expect("parse"),
        &CodegenOptions {
            namespace: "Ns".into(),
            companion_source: Some(format!(
                "{leaf}\n    [Observable] public NestLeaf Model {{ get; set; }}\n"
            )),
            ..CodegenOptions::default()
        },
    )
    .expect("generate");
    assert!(
        generated.contains("ObserveProperty(\"Name\")"),
        "nested Observable leaf must subscribe:\n{generated}"
    );
    let mut body =
        strip_namespace_and_using(&generated).replacen("public partial class ", "public class ", 1);
    let marker = "class NestLeafWin";
    let start = body.find(marker).expect("class decl");
    let open = start + body[start..].find('{').expect("class open");
    body.insert_str(
        open + 1,
        "\n    [Observable] public NestLeaf Model { get; set; }\n",
    );
    let src = format!(
        r#"using Arc;
using Arc.ComponentModel;
using Arc.UI.Components;

{body}

{leaf}

void Main()
{{
    NestLeafWin w = new NestLeafWin();
    w.Model = new NestLeaf();
    w.Model.Name = "n";
    w.InitializeComponent();
}}
"#
    );
    assert_compiles_with_deps("ui_arml_binding_nested_obs_leaf", &src, UI_DEPS);
}

#[test]
fn arml_binding_nested_leaf_unknown_name_fails() {
    let leaf = r#"
public class NestMiss {
    [Observable] public string Name { get; set; }
}
"#;
    let arml = r#"<Window Title="T" Width="100" Height="100" Class="Ns.NestMissWin">
    <TextBlock Text="{Binding Model.NoSuchName, Mode=OneWay}"/>
</Window>"#;
    let generated = generate(
        &Parser::parse(arml).expect("parse"),
        &CodegenOptions {
            namespace: "Ns".into(),
            companion_source: Some(format!(
                "{leaf}\n    [Observable] public NestMiss Model {{ get; set; }}\n"
            )),
            ..CodegenOptions::default()
        },
    )
    .expect("generate (path checked at Arc typeck)");
    let mut body =
        strip_namespace_and_using(&generated).replacen("public partial class ", "public class ", 1);
    let marker = "class NestMissWin";
    let start = body.find(marker).expect("class decl");
    let open = start + body[start..].find('{').expect("class open");
    body.insert_str(
        open + 1,
        "\n    [Observable] public NestMiss Model { get; set; }\n",
    );
    let src = format!(
        r#"using Arc;
using Arc.ComponentModel;
using Arc.UI.Components;

{body}

{leaf}

void Main()
{{
    NestMissWin w = new NestMissWin();
    w.InitializeComponent();
}}
"#
    );
    assert_rejected_with_deps(
        "ui_arml_binding_nested_obs_leaf_miss",
        &src,
        "NoSuchName",
        UI_DEPS,
    );
}
