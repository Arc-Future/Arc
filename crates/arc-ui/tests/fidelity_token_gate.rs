//! DesignTokenCatalog + bare-value gate (037 fidelity-loop §1 hard gate).
//!
//! Proves: catalog published & in sync; typeck rejects bare #hex / Thickness;
//! theme Color definitions and StaticResource remain legal.

use std::path::PathBuf;

use arc_ui::{
    generate_catalog_json, write_catalog_json, DesignTokenCatalog, Parser, TypeChecker,
    CATALOG_JSON_REL, LIGHT_ARML_REL,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// Golden 文本比对：忽略 `core.autocrlf` 把 LF blob 检出成 CRLF 的平台差（Windows CI）。
fn norm_nl(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

#[test]
fn design_token_catalog_covers_primary_keys() {
    let catalog = DesignTokenCatalog::build_from_repo(&repo_root()).expect("catalog");
    assert!(catalog.tokens.len() >= 40, "expected color+geometry tokens");
    for key in [
        "Color.Primary",
        "Color.Background",
        "Color.Text.Primary",
        "Size.Border.Thickness",
        "Spacing.MD",
        "Radius.Control",
        "Font.Body.Size",
        "Motion.Duration.Fast",
    ] {
        assert!(catalog.contains(key), "missing token {key}");
    }
    assert!(catalog.tokens.iter().all(|t| t.bare_forbidden));
}

#[test]
fn design_token_catalog_json_in_sync() {
    let root = repo_root();
    let expected = generate_catalog_json(&root).expect("generate");
    let path = root.join(CATALOG_JSON_REL);
    if std::env::var("UPDATE_DESIGN_TOKEN_CATALOG").is_ok() {
        write_catalog_json(&root).expect("write catalog");
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {}; regenerate with UPDATE_DESIGN_TOKEN_CATALOG=1 cargo test -p arc-ui --test fidelity_token_gate -- design_token_catalog_json_in_sync ({e})",
            path.display()
        )
    });
    assert_eq!(
        norm_nl(&actual),
        norm_nl(&expected),
        "DesignTokenCatalog.json out of sync; UPDATE_DESIGN_TOKEN_CATALOG=1 to regenerate"
    );
}

#[test]
fn typeck_rejects_bare_hex_on_component() {
    let src = r##"<Window Background="#F0F0F0">
        <Button Background="#0044FF" Content="OK"/>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(!report.is_ok());
    assert!(
        report.errors.iter().any(|e| {
            let msg = e.to_string();
            msg.contains("bare color") && msg.contains("DesignTokenCatalog")
        }),
        "errors: {:?}",
        report.errors
    );
}

#[test]
fn typeck_rejects_bare_thickness_on_margin() {
    let src = r#"<Window>
        <Button Margin="16,12,16,12" Content="OK"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(!report.is_ok());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.to_string().contains("bare Thickness")),
        "errors: {:?}",
        report.errors
    );
}

#[test]
fn typeck_rejects_bare_hex_in_style_setter() {
    let src = r##"<Window>
        <Window.Resources>
            <Style x:Key="PrimaryButton" TargetType="Button">
                <Setter Property="Background" Value="#0044FF"/>
            </Style>
        </Window.Resources>
        <Button Style="{StaticResource PrimaryButton}" Content="Save"/>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(!report.is_ok());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.to_string().contains("bare color")),
        "errors: {:?}",
        report.errors
    );
}

#[test]
fn typeck_allows_static_resource_color_and_theme_defs() {
    let src = r##"<Window Background="{StaticResource Color.Background}">
        <Window.Resources>
            <ResourceDictionary>
                <Color x:Key="Color.Accent" Value="#FF0044FF"/>
                <Style x:Key="PrimaryButton" TargetType="Button">
                    <Setter Property="Background" Value="{StaticResource Color.Primary}"/>
                    <Setter Property="Content" Value="OK"/>
                </Style>
            </ResourceDictionary>
        </Window.Resources>
        <Button Style="{StaticResource PrimaryButton}" Content="Save"/>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
}

#[test]
fn typeck_allows_light_theme_color_dictionary() {
    let root = repo_root();
    let src = std::fs::read_to_string(root.join(LIGHT_ARML_REL)).expect("Light.arml");
    let doc = Parser::parse(&src).expect("parse Light.arml");
    let report = TypeChecker::new().check(&doc);
    assert!(
        report.is_ok(),
        "Light.arml Color definitions must remain legal: {:?}",
        report.errors
    );
}
