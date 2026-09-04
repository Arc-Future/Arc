//! RFC 026 M3：Style 属性 typeck 契约。

use arc_ui::{verify_report, Parser, TypeChecker};

#[test]
fn verify_button_style_property_accepted() {
    let src = r#"<Window><Button Style="PrimaryButton" Content="OK"/></Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = verify_report(&doc, &checker);
    assert!(
        report.is_ok(),
        "Style property should type-check: {:?}",
        report.type_check.errors
    );
}

#[test]
fn verify_window_resources_property_accepted() {
    let src = r#"<Window Resources="AppResources"><TextBlock Text="Hi"/></Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = verify_report(&doc, &checker);
    assert!(
        report.is_ok(),
        "Resources property should type-check: {:?}",
        report.type_check.errors
    );
}
