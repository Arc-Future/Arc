//! ARML `{Binding Path}` 唯一作者惯用法（编译期脱糖）。
//!
//! 宣称：Text / Command / ItemsSource → `this.Path`；`{x:Bind}` 硬拒绝。
//! 不宣称 Converter / ElementName / RelativeSource / 运行时路径行走。

use arc_ui::{generate, CodegenOptions, Parser, TypeChecker};

fn window(inner: &str) -> String {
    format!(
        r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    {inner}
</Window>"#
    )
}

fn codegen(src: &str) -> Result<String, String> {
    codegen_obs(src, &[])
}

fn codegen_obs(src: &str, observables: &[&str]) -> Result<String, String> {
    let doc = Parser::parse(src).expect("parse");
    generate(
        &doc,
        &CodegenOptions {
            namespace: "Ns".into(),
            observable_members: observables.iter().map(|s| (*s).to_string()).collect(),
            ..CodegenOptions::default()
        },
    )
}

#[test]
fn typecheck_button_command_binding_ok() {
    let src = window(r#"<Button Content="Go" Command="{Binding Click}"/>"#);
    let report = TypeChecker::new().check(&Parser::parse(&src).expect("parse"));
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert_eq!(report.binding_count, 1);
}

#[test]
fn codegen_command_binding_assigns_this_path() {
    let src = window(r#"<Button x:Name="GoBtn" Content="Go" Command="{Binding Click}"/>"#);
    let code = codegen(&src).expect("generate");
    assert!(code.contains("child_0.Command = this.Click;"), "{code}");
}

#[test]
fn codegen_command_xbind_rejected() {
    let err = codegen(&window(r#"<Button Command="{x:Bind Click}"/>"#)).unwrap_err();
    assert!(err.contains("x:Bind"), "{err}");
    assert!(err.contains("{Binding Path}"), "{err}");
}

#[test]
fn codegen_text_xbind_rejected() {
    let err = codegen(&window(r#"<TextBlock Text="{x:Bind Title}"/>"#)).unwrap_err();
    assert!(err.contains("x:Bind"), "{err}");
    assert!(err.contains("{Binding Path}"), "{err}");
}

#[test]
fn codegen_binding_converter_rejected() {
    let err = codegen(&window(
        r#"<TextBlock Text="{Binding Title, Converter=Invert}"/>"#,
    ))
    .unwrap_err();
    assert!(err.contains("Converter"), "{err}");
    assert!(err.contains("not supported"), "{err}");
}

#[test]
fn codegen_binding_window_title_assigns_and_subscribes() {
    let src = r#"<Window Title="{Binding Caption}" Width="100" Height="100" Class="Ns.W"/>"#;
    let code = codegen_obs(src, &["Caption"]).expect("generate");
    assert!(
        code.contains("this.Title = BindingOperations.StringOf(this.Caption);"),
        "{code}"
    );
    assert!(
        code.contains("BindingOperations.BindWindowTitle(this, this.ObserveProperty(\"Caption\"))"),
        "{code}"
    );
}

#[test]
fn codegen_binding_nested_path_emits_guards() {
    let plain = codegen(&window(r#"<TextBlock Text="{Binding User.Name, Mode=OneWay}"/>"#))
        .expect("generate");
    assert!(plain.contains("StringOf(this.User.Name)"), "{plain}");
    assert!(!plain.contains("ObserveProperty"), "{plain}");

    let code = codegen_obs(
        &window(r#"<TextBlock Text="{Binding User.Name, Mode=OneWay}"/>"#),
        &["User", "Name"],
    )
    .expect("generate");
    assert!(code.contains("BeginNestedText"), "{code}");
    assert!(code.contains("OnNestedBindingRefresh"), "{code}");
    assert!(code.contains("this.ObserveProperty(\"User\")"), "{code}");
    assert!(
        code.contains("this.User.ObserveProperty(\"Name\")"),
        "{code}"
    );
    assert!(code.contains("RunNestedRefresh"), "{code}");
}
