//! L2：ARML `{Binding}` 编译期脱糖（Text OneWay + Command Execute）。
//!
//! 宣称：codegen `this.Path` / `this.Foo.Bar` + Window.Title + Command setter + SyncText。
//! 不宣称 Converter / ElementName / RelativeSource / 运行时路径行走 / `{x:Bind}`。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};
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

fn emit_window(arml: &str) -> String {
    emit_window_with(arml, None)
}

fn emit_window_with(arml: &str, companion: Option<&str>) -> String {
    let doc = Parser::parse(arml).expect("parse ARML");
    let generated = generate(
        &doc,
        &CodegenOptions {
            namespace: "Ns".into(),
            companion_source: companion.map(|s| s.to_string()),
            ..CodegenOptions::default()
        },
    )
    .expect("ARML Binding generate");
    strip_namespace_and_using(&generated)
}

fn splice_codebehind(generated: &str, extra: &str) -> String {
    let generated = generated.replacen("public partial class ", "public class ", 1);
    let idx = generated
        .rfind('}')
        .expect("generated window class closing brace");
    let mut out = String::new();
    out.push_str(&generated[..idx]);
    out.push_str(extra);
    out.push_str(&generated[idx..]);
    out
}

#[test]
fn ui_arml_binding_batch() {
    let generated = emit_window(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.BindWin">
    <StackPanel>
        <TextBlock x:Name="Label" Text="{Binding Message, Mode=OneTime}"/>
        <Button x:Name="GoBtn" Content="Go" Command="{Binding Click}" CommandParameter="p1"/>
    </StackPanel>
</Window>"#,
    );
    assert!(
        generated.contains("child_2.Command = this.Click;"),
        "Command desugar:\n{generated}"
    );
    assert!(
        generated.contains("StringOf(this.Message)"),
        "Text desugar:\n{generated}"
    );
    assert!(
        !generated.contains("ObserveProperty"),
        "OneTime must not ObserveProperty:\n{generated}"
    );

    let window = splice_codebehind(
        &generated,
        r#"
    public static int Hits;
    public static object LastParam;
    public RelayCommand Click;
    [Observable] public string Message { get; set; }

    public static void OnExec(object p)
    {
        Hits = Hits + 1;
        LastParam = p;
    }

    public BindWin()
    {
        this.Message = "hello";
        this.Click = new RelayCommand(BindWin.OnExec);
    }

    public string LabelText()
    {
        return this.Label.Text;
    }

    public void Fire()
    {
        this.GoBtn.RaiseClick();
    }
"#,
    );

    let src = format!(
        r##"using Arc;
using Arc.ComponentModel;
using Arc.UI.Components;

{window}

void Main()
{{
    BindWin.Hits = 0;
    BindWin.LastParam = null;
    BindWin w = new BindWin();
    w.InitializeComponent();
    if (w.Message != "hello")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_text_and_command:FAIL:message");
        return;
    }}
    string label = w.LabelText();
    if (label == null || label != "hello")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_text_and_command:FAIL:label=" + (label == null ? "null" : label));
        return;
    }}
    if (w.Click == null)
    {{
        Console.WriteLine("ARC_CASE:arml_binding_text_and_command:FAIL:noclick");
        return;
    }}
    w.Fire();
    if (BindWin.Hits != 1)
    {{
        Console.WriteLine("ARC_CASE:arml_binding_text_and_command:FAIL:hits=" + BindWin.Hits.ToString());
        return;
    }}
    if (BindWin.LastParam == null || (string)BindWin.LastParam != "p1")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_text_and_command:FAIL:param");
        return;
    }}
    Console.WriteLine("ARC_CASE:arml_binding_text_and_command:PASS");
}}
"##
    );

    let results = build_and_run_batch_with_deps(
        "ui_arml_binding",
        &[BatchCase {
            name: "arml_binding_text_and_command",
            src: &src,
        }],
        UI_DEPS,
    );
    assert!(batch_case_result(&results, "arml_binding_text_and_command").passed);

    let title_gen = emit_window(
        r#"<Window Title="{Binding Caption, Mode=OneTime}" Width="100" Height="100" Class="Ns.TitleWin">
    <TextBlock Text="x"/>
</Window>"#,
    );
    assert!(
        title_gen.contains("this.Title = BindingOperations.StringOf(this.Caption);"),
        "Title desugar:\n{title_gen}"
    );
    let title_window = splice_codebehind(
        &title_gen,
        r#"
    public string Caption;

    public TitleWin()
    {
        this.Caption = "BoundTitle";
    }
"#,
    );
    let title_src = format!(
        r##"using Arc;
using Arc.UI.Components;

{title_window}

void Main()
{{
    TitleWin w = new TitleWin();
    w.InitializeComponent();
    if (w.Title == null || w.Title != "BoundTitle")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_window_title:FAIL:title=" + (w.Title == null ? "null" : w.Title));
        return;
    }}
    Console.WriteLine("ARC_CASE:arml_binding_window_title:PASS");
}}
"##
    );

    let nest_gen = emit_window(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.NestWin">
    <TextBlock x:Name="Label" Text="{Binding User.Name, Mode=OneTime}"/>
</Window>"#,
    );
    assert!(
        nest_gen.contains("this.User.Name"),
        "nested desugar:\n{nest_gen}"
    );
    let nest_window = splice_codebehind(
        &nest_gen,
        r#"
    public NestModel User;

    public NestWin()
    {
        this.User = new NestModel();
        this.User.Name = "nested-ok";
    }

    public string LabelText()
    {
        return this.Label.Text;
    }
"#,
    );
    let nest_src = format!(
        r##"using Arc;
using Arc.UI.Components;

public class NestModel {{
    public string Name;
}}

{nest_window}

void Main()
{{
    NestWin w = new NestWin();
    w.InitializeComponent();
    string label = w.LabelText();
    if (label == null || label != "nested-ok")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_path:FAIL:label=" + (label == null ? "null" : label));
        return;
    }}
    Console.WriteLine("ARC_CASE:arml_binding_nested_path:PASS");
}}
"##
    );

    let null_gen = emit_window(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.NullWin">
    <TextBlock x:Name="Label" Text="{Binding User.Name, Mode=OneTime}"/>
</Window>"#,
    );
    let null_window = splice_codebehind(
        &null_gen,
        r#"
    public NestNullModel User;

    public NullWin()
    {
        this.User = null;
    }

    public string LabelText()
    {
        return this.Label.Text;
    }
"#,
    );
    let null_src = format!(
        r##"using Arc;
using Arc.UI.Components;

public class NestNullModel {{
    public string Name;
}}

{null_window}

void Main()
{{
    NullWin w = new NullWin();
    w.InitializeComponent();
    string label = w.LabelText();
    if (label == null)
    {{
        label = "";
    }}
    if (label != "")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_null:FAIL:label=" + label);
        return;
    }}
    Console.WriteLine("ARC_CASE:arml_binding_nested_null:PASS");
}}
"##
    );

    let more = build_and_run_batch_with_deps(
        "ui_arml_binding_more",
        &[
            BatchCase {
                name: "arml_binding_window_title",
                src: &title_src,
            },
            BatchCase {
                name: "arml_binding_nested_path",
                src: &nest_src,
            },
            BatchCase {
                name: "arml_binding_nested_null",
                src: &null_src,
            },
        ],
        UI_DEPS,
    );
    assert!(batch_case_result(&more, "arml_binding_window_title").passed);
    assert!(batch_case_result(&more, "arml_binding_nested_path").passed);
    assert!(batch_case_result(&more, "arml_binding_nested_null").passed);

    let plain_gen = emit_window(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.PlainWin">
    <TextBlock x:Name="Label" Text="{Binding Caption, Mode=OneWay}"/>
</Window>"#,
    );
    assert!(
        plain_gen.contains("StringOf(this.Caption)") && !plain_gen.contains("ObserveProperty"),
        "plain OneWay must read without ObserveProperty:\n{plain_gen}"
    );
    let plain_window = splice_codebehind(
        &plain_gen,
        r#"
    public string Caption;

    public PlainWin()
    {
        this.Caption = "plain-ok";
    }

    public string LabelText()
    {
        return this.Label.Text;
    }
"#,
    );
    let plain_src = format!(
        r##"using Arc;
using Arc.UI.Components;

{plain_window}

void Main()
{{
    PlainWin w = new PlainWin();
    w.InitializeComponent();
    string label = w.LabelText();
    if (label == null || label != "plain-ok")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_plain_oneway:FAIL:label=" + (label == null ? "null" : label));
        return;
    }}
    Console.WriteLine("ARC_CASE:arml_binding_plain_oneway:PASS");
}}
"##
    );

    let nest_plain_gen = emit_window(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.PlainNestWin">
    <TextBlock x:Name="Label" Text="{Binding User.Name, Mode=OneWay}"/>
</Window>"#,
    );
    assert!(
        nest_plain_gen.contains("StringOf(this.User.Name)")
            && !nest_plain_gen.contains("ObserveProperty"),
        "plain nested must snapshot:\n{nest_plain_gen}"
    );
    let nest_plain_window = splice_codebehind(
        &nest_plain_gen,
        r#"
    public PlainNestModel User;

    public PlainNestWin()
    {
        this.User = new PlainNestModel();
        this.User.Name = "nest-plain";
    }

    public string LabelText()
    {
        return this.Label.Text;
    }
"#,
    );
    let nest_plain_src = format!(
        r##"using Arc;
using Arc.UI.Components;

public class PlainNestModel {{
    public string Name;
}}

{nest_plain_window}

void Main()
{{
    PlainNestWin w = new PlainNestWin();
    w.InitializeComponent();
    string label = w.LabelText();
    if (label == null || label != "nest-plain")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_plain_nested:FAIL:label=" + (label == null ? "null" : label));
        return;
    }}
    Console.WriteLine("ARC_CASE:arml_binding_plain_nested:PASS");
}}
"##
    );

    let plain = build_and_run_batch_with_deps(
        "ui_arml_binding_plain",
        &[
            BatchCase {
                name: "arml_binding_plain_oneway",
                src: &plain_src,
            },
            BatchCase {
                name: "arml_binding_plain_nested",
                src: &nest_plain_src,
            },
        ],
        UI_DEPS,
    );
    assert!(batch_case_result(&plain, "arml_binding_plain_oneway").passed);
    assert!(batch_case_result(&plain, "arml_binding_plain_nested").passed);

    let rebind_extra = r#"
    [Observable] public RebindModel User { get; set; }

    public RebindWin()
    {
        this.User = null;
    }

    public string LabelText()
    {
        return this.Label.Text;
    }
"#;
    let rebind_model = r#"
public class RebindModel {
    [Observable] public string Name { get; set; }
}
"#;
    let rebind_gen = emit_window_with(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.RebindWin">
    <TextBlock x:Name="Label" Text="{Binding User.Name, Mode=OneWay}"/>
</Window>"#,
        Some(&format!("{rebind_model}\n{rebind_extra}")),
    );
    assert!(
        rebind_gen.contains("BeginNestedText") && rebind_gen.contains("OnNestedBindingRefresh"),
        "nested rebind desugar:\n{rebind_gen}"
    );
    let rebind_window = splice_codebehind(
        &rebind_gen,
        r#"
    [Observable] public RebindModel User { get; set; }

    public RebindWin()
    {
        this.User = null;
    }

    public string LabelText()
    {
        return this.Label.Text;
    }
"#,
    );
    let rebind_src = format!(
        r##"using Arc;
using Arc.ComponentModel;
using Arc.UI.Components;

public class RebindModel {{
    [Observable] public string Name {{ get; set; }}
}}

{rebind_window}

void Main()
{{
    RebindWin w = new RebindWin();
    w.InitializeComponent();
    string label = w.LabelText();
    if (label == null)
    {{
        label = "";
    }}
    if (label != "")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_rebind:FAIL:init=" + label);
        return;
    }}
    RebindModel first = new RebindModel();
    first.Name = "alpha";
    w.User = first;
    label = w.LabelText();
    if (label == null || label != "alpha")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_rebind:FAIL:assign=" + (label == null ? "null" : label));
        return;
    }}
    RebindModel second = new RebindModel();
    second.Name = "beta";
    w.User = second;
    label = w.LabelText();
    if (label == null || label != "beta")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_rebind:FAIL:replace=" + (label == null ? "null" : label));
        return;
    }}
    first.Name = "stale";
    label = w.LabelText();
    if (label == null || label != "beta")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_rebind:FAIL:stale=" + (label == null ? "null" : label));
        return;
    }}
    w.User = null;
    label = w.LabelText();
    if (label == null)
    {{
        label = "";
    }}
    if (label != "")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_rebind:FAIL:null=" + label);
        return;
    }}
    second.Name = "ghost";
    label = w.LabelText();
    if (label == null)
    {{
        label = "";
    }}
    if (label != "")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_rebind:FAIL:ghost=" + label);
        return;
    }}
    Console.WriteLine("ARC_CASE:arml_binding_nested_rebind:PASS");
}}
"##
    );

    let rebind = build_and_run_batch_with_deps(
        "ui_arml_binding_rebind",
        &[BatchCase {
            name: "arml_binding_nested_rebind",
            src: &rebind_src,
        }],
        UI_DEPS,
    );
    assert!(batch_case_result(&rebind, "arml_binding_nested_rebind").passed);

    let dc_plain_gen = emit_window(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.DcPlainWin" DataType="DcPlainVm">
    <TextBlock x:Name="Label" Text="{Binding Caption, Mode=OneWay}"/>
</Window>"#,
    );
    assert!(
        dc_plain_gen.contains("ContextRoot") && dc_plain_gen.contains("DataContextProperty"),
        "DataType OneWay must follow DataContext:\n{dc_plain_gen}"
    );

    let merge_leaf = r#"
public class MergeLeaf {
    [Observable] public string Name { get; set; }
}
"#;
    let merge_gen = emit_window_with(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.MergeLeafWin">
    <TextBlock x:Name="Label" Text="{Binding Model.Name, Mode=OneWay}"/>
</Window>"#,
        Some(&format!(
            "{merge_leaf}\n    [Observable] public MergeLeaf Model {{ get; set; }}\n"
        )),
    );
    assert!(
        merge_gen.contains("ObserveProperty(\"Name\")"),
        "merge-unit nested Observable leaf must ObserveProperty:\n{merge_gen}"
    );
    let merge_window = splice_codebehind(
        &merge_gen,
        r#"
    [Observable] public MergeLeaf Model { get; set; }

    public string LabelText()
    {
        return this.Label.Text;
    }
"#,
    );
    let merge_src = format!(
        r##"using Arc;
using Arc.ComponentModel;
using Arc.UI.Components;

{merge_window}

public class MergeLeaf {{
    [Observable] public string Name {{ get; set; }}
}}

void Main()
{{
    MergeLeafWin w = new MergeLeafWin();
    w.Model = new MergeLeaf();
    w.Model.Name = "leaf-1";
    w.InitializeComponent();
    string label = w.LabelText();
    if (label == null || label != "leaf-1")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_obs_leaf_merge:FAIL:init=" + (label == null ? "null" : label));
        return;
    }}
    w.Model.Name = "leaf-2";
    label = w.LabelText();
    if (label == null || label != "leaf-2")
    {{
        Console.WriteLine("ARC_CASE:arml_binding_nested_obs_leaf_merge:FAIL:live=" + (label == null ? "null" : label));
        return;
    }}
    Console.WriteLine("ARC_CASE:arml_binding_nested_obs_leaf_merge:PASS");
}}
"##
    );

    let merge = build_and_run_batch_with_deps(
        "ui_arml_binding_nested_obs_leaf",
        &[BatchCase {
            name: "arml_binding_nested_obs_leaf_merge",
            src: &merge_src,
        }],
        UI_DEPS,
    );
    assert!(batch_case_result(&merge, "arml_binding_nested_obs_leaf_merge").passed);
}
