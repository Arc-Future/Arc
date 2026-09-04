//! RFC 026 M1 D11 verify 工具测试。
//!
//! 验证 `arc ui verify` 报告：类型检查 + A11y + 布局。

use arc_ui::{verify_report, Parser, TypeChecker};

#[test]
fn verify_clean_document() {
    let src = r#"<Window Title="Demo" Width="800" Height="600">
        <StackPanel>
            <TextBlock Text="Hello"/>
            <Button Content="Click"/>
        </StackPanel>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = verify_report(&doc, &checker);
    assert!(report.is_ok(), "errors: {:?}", report.type_check.errors);
    assert!(report.a11y_issues.is_empty());
    assert!(report.layout_issues.is_empty());
}

#[test]
fn verify_a11y_missing_label_for_button() {
    let src = r#"<Window>
        <Button Click="OnClick"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = verify_report(&doc, &checker);
    assert!(!report.a11y_issues.is_empty());
    assert!(report.a11y_issues.iter().any(|e| matches!(e, arc_ui::ArmlError::Type { message, .. } if message.contains("accessible label"))));
}

#[test]
fn verify_a11y_button_with_content_passes() {
    let src = r#"<Window><Button Content="OK"/></Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = verify_report(&doc, &checker);
    assert!(report.a11y_issues.is_empty());
}

#[test]
fn verify_a11y_input_with_text_passes() {
    let src = r#"<Window><TextBox Text="Name"/></Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = verify_report(&doc, &checker);
    assert!(report.a11y_issues.is_empty());
}

#[test]
fn verify_layout_stackpanel_too_many_children() {
    let mut src = String::from("<Window><StackPanel>");
    for i in 0..15 {
        src.push_str(&format!("<TextBlock Text=\"item{i}\"/>"));
    }
    src.push_str("</StackPanel></Window>");
    let doc = Parser::parse(&src).unwrap();
    let checker = TypeChecker::new();
    let report = verify_report(&doc, &checker);
    assert!(!report.layout_issues.is_empty());
    assert!(report.layout_issues.iter().any(
        |e| matches!(e, arc_ui::ArmlError::Type { message, .. } if message.contains("StackPanel"))
    ));
}

#[test]
fn verify_report_error_count_aggregates() {
    let src = r#"<Window>
        <Button Click="x"/>
        <UnknownComp/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = verify_report(&doc, &checker);
    // 1 个 type error（UnknownComp）+ 1 个 a11y issue（Button 无 label）
    assert!(report.error_count() >= 2);
}
