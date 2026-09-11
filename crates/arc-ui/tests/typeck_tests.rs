//! RFC 026 M1 类型检查器测试。
//!
//! 验证 ComponentRegistry 注册表、TypeChecker 属性/绑定校验。

use arc_ui::Parser;
use arc_ui::*;
use arc_ui::{ComponentInfo, ComponentRegistry, PropType, TypeChecker};

#[test]
fn registry_builtin_contains_core_components() {
    let reg = ComponentRegistry::builtin();
    for name in [
        "Window",
        "Page",
        "UserControl",
        "Application",
        "StackPanel",
        "VirtualizingStackPanel",
        "Grid",
        "Canvas",
        "ScrollView",
        "WrapPanel",
        "DockPanel",
        "Rectangle",
        "TextBlock",
        "Button",
        "Image",
        "TextBox",
        "PasswordBox",
        "Border",
        "CheckBox",
        "Slider",
        "CodeEditor",
        "ContentPresenter",
        "ContentControl",
        "ItemsControl",
        "ListView",
        "VisualHost",
        "TabControl",
        "TabItem",
        "TreeView",
        "TreeViewItem",
        "DataGrid",
    ] {
        assert!(reg.contains(name), "expected builtin component `{name}`");
    }
}

#[test]
fn registry_window_has_title_property() {
    let reg = ComponentRegistry::builtin();
    let info = reg.get("Window").unwrap();
    assert!(info.has_property("Title"));
    assert!(info.has_property("Width"));
    assert!(info.has_property("Height"));
    assert_eq!(info.property_type("Title"), Some(&PropType::String));
    assert_eq!(info.property_type("Width"), Some(&PropType::Double));
}

#[test]
fn registry_treeview_has_items_source() {
    let reg = ComponentRegistry::builtin();
    let info = reg.get("TreeView").unwrap();
    assert!(info.has_property("ItemsSource"));
    assert_eq!(info.property_type("ItemsSource"), Some(&PropType::Object));
    assert!(info.has_property("SelectedIndex"));
    assert!(info.has_property("VerticalOffset"));
    assert_eq!(
        info.property_type("VerticalOffset"),
        Some(&PropType::Double)
    );
}

#[test]
fn registry_button_has_click_handler() {
    let reg = ComponentRegistry::builtin();
    let info = reg.get("Button").unwrap();
    assert!(info.has_property("Content"));
    assert!(info.has_property("Click"));
    assert_eq!(info.property_type("Click"), Some(&PropType::EventHandler));
}

#[test]
fn registry_custom_register() {
    let mut reg = ComponentRegistry::default();
    reg.register(ComponentInfo::new("MyButton").with_property("Label", PropType::String));
    assert!(reg.contains("MyButton"));
    assert!(reg.get("MyButton").unwrap().has_property("Label"));
}

#[test]
fn typecheck_valid_document() {
    let src = r#"<Window Title="Demo" Width="800" Height="600">
        <StackPanel>
            <TextBlock Text="Hello"/>
            <Button Content="Click"/>
        </StackPanel>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert!(report.component_count >= 4);
}

#[test]
fn typecheck_unknown_component() {
    let src = r#"<UnknownWidget/>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(!report.is_ok());
    assert!(report.errors.iter().any(
        |e| matches!(e, ArmlError::Type { message, .. } if message.contains("unknown component"))
    ));
}

#[test]
fn typecheck_unknown_property_emits_warning() {
    let src = r#"<Window Foo="bar"/>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(
        report.is_ok(),
        "unknown property should be warning, not error"
    );
    assert!(!report.warnings.is_empty());
    assert!(report.warnings.iter().any(
        |w| matches!(w, ArmlError::Type { message, .. } if message.contains("unknown property"))
    ));
}

#[test]
fn typecheck_binding_counted() {
    let src = r#"<Window>
        <TextBlock Text="{Binding Count, Mode=OneWay}"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert_eq!(report.binding_count, 1);
}

#[test]
fn typecheck_binding_missing_path() {
    let src = r#"<Window>
        <TextBlock Text="{Binding}"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(!report.is_ok());
    assert!(report
        .errors
        .iter()
        .any(|e| matches!(e, ArmlError::Type { message, .. } if message.contains("binding path"))));
}

#[test]
fn typecheck_binding_invalid_mode() {
    let src = r#"<Window>
        <TextBlock Text="{Binding Count, Mode=Invalid}"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(!report.is_ok());
    assert!(report
        .errors
        .iter()
        .any(|e| matches!(e, ArmlError::Type { message, .. } if message.contains("Mode"))));
}

#[test]
fn typecheck_xbind_rejected_points_to_binding() {
    let src = r#"<Window>
        <TextBlock Text="{x:Bind Count}"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(!report.is_ok());
    assert!(report.errors.iter().any(|e| {
        matches!(e, ArmlError::Type { message, .. } if message.contains("x:Bind") && message.contains("{Binding Path}"))
    }));
}

#[test]
fn typecheck_binding_nested_path_ok() {
    let src = r#"<Window>
        <TextBlock Text="{Binding User.Name}"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert_eq!(report.binding_count, 1);
}

#[test]
fn typecheck_binding_invalid_path_segment() {
    let src = r#"<Window>
        <TextBlock Text="{Binding User..Name}"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(!report.is_ok());
    assert!(report
        .errors
        .iter()
        .any(|e| matches!(e, ArmlError::Type { message, .. } if message.contains("invalid"))));
}

#[test]
fn typecheck_binding_window_title_ok() {
    let src = r#"<Window Title="{Binding Caption}">
        <TextBlock Text="hi"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert_eq!(report.binding_count, 1);
}

#[test]
fn typecheck_binding_converter_rejected() {
    let src = r#"<Window>
        <TextBlock Text="{Binding Count, Converter=Invert}"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(!report.is_ok());
    assert!(report.errors.iter().any(|e| {
        matches!(e, ArmlError::Type { message, .. } if message.contains("Converter") && message.contains("not supported"))
    }));
}

#[test]
fn typecheck_window_with_visual_tree() {
    let src = r#"<Window Title="ok">
        <StackPanel/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
}

#[test]
fn typecheck_skips_xmlns_attributes() {
    let src = r#"<Window xmlns="http://schemas.arc.dev/winfx" xmlns:x="http://schemas.arc.dev/xaml" Title="ok"/>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(
        report.is_ok(),
        "xmlns declarations should not trigger warnings: {:?}",
        report.warnings
    );
}

#[test]
fn typecheck_skips_x_directives() {
    let src = r#"<Window x:Class="MyApp.Main"/>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(
        report.is_ok(),
        "x: directives should not trigger warnings: {:?}",
        report.warnings
    );
}

#[test]
fn typecheck_rectangle_fill_and_size() {
    let src = r#"<Window>
        <Rectangle Width="50" Height="50" Fill="{StaticResource Color.Danger}" Stroke="{StaticResource Color.Border}" StrokeThickness="2"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert!(
        report.warnings.is_empty(),
        "warnings: {:?}",
        report.warnings
    );
}

#[test]
fn typecheck_listview_items_source_no_warnings() {
    let src = r#"<Window>
        <ListView ItemsSource="{Binding Items}" DisplayMemberPath="Name"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert!(
        report.warnings.is_empty(),
        "warnings: {:?}",
        report.warnings
    );
    assert_eq!(report.binding_count, 1);
}

#[test]
fn typecheck_p0_visual_properties_no_warnings() {
    // fidelity-loop §1.2：色值须 StaticResource；FontSize/Spacing 本切片仍允许字面量。
    let src = r#"<Window Background="{StaticResource Color.Background}" Title="Demo" Width="400" Height="300">
        <StackPanel Orientation="Vertical" Spacing="8" Background="{StaticResource Color.Surface}">
            <TextBlock Text="Hello" FontSize="16" Foreground="{StaticResource Color.Primary}" Background="{StaticResource Color.Text.Highlight}"/>
            <Button Content="OK" Background="{StaticResource Color.Disabled.Fill}" Foreground="{StaticResource Color.Text.Secondary}" FontSize="14"/>
            <Rectangle Width="80" Height="24" Fill="{StaticResource Color.Success}"/>
        </StackPanel>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert!(
        report.warnings.is_empty(),
        "warnings: {:?}",
        report.warnings
    );
}

#[test]
fn typecheck_visual_host_document() {
    let src = r#"<Window Title="Demo" Width="480" Height="320">
        <StackPanel>
            <Button Content="Host"/>
            <VisualHost Height="120">
                <StackPanel>
                    <Button Content="Preview"/>
                </StackPanel>
            </VisualHost>
        </StackPanel>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
}

#[test]
fn typecheck_content_presenter_content_binding() {
    let src = r#"<Window>
        <ContentPresenter Content="{Binding Selected}"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert!(
        report.warnings.is_empty(),
        "warnings: {:?}",
        report.warnings
    );
    assert_eq!(report.binding_count, 1);
}

#[test]
fn typecheck_grid_attached_properties_no_warnings() {
    let src = r#"<Window>
        <Grid>
            <TextBlock Grid.Row="1" Grid.Column="0" Text="A"/>
        </Grid>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert!(
        report.warnings.is_empty(),
        "warnings: {:?}",
        report.warnings
    );
}

#[test]
fn typecheck_unknown_attached_host_emits_warning() {
    let src = r#"<Window>
        <TextBlock Foo.Bar="1"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(
        report.is_ok(),
        "unknown property should be warning, not error"
    );
    assert!(report.warnings.iter().any(
        |w| matches!(w, ArmlError::Type { message, .. } if message.contains("unknown property"))
    ));
}

#[test]
fn typecheck_unknown_grid_attached_property_emits_warning() {
    let src = r#"<Window>
        <Grid>
            <TextBlock Grid.Bogus="1"/>
        </Grid>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(
        report.is_ok(),
        "unknown attached property should be warning, not error"
    );
    assert!(report.warnings.iter().any(|w| matches!(w, ArmlError::Type { message, .. } if message.contains("unknown attached property"))));
}

#[test]
fn typecheck_grid_attached_non_integer_literal_errors() {
    // RFC 040：Grid.Row/Grid.Column 为 typed DependencyProperty<int>，仅接受整数字面量。
    let src = r#"<Window>
        <Grid>
            <TextBlock Grid.Row="1.5" Grid.Column="0" Text="A"/>
            <TextBlock Grid.Column="abc" Text="B"/>
        </Grid>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(
        !report.is_ok(),
        "non-integer Grid.Row/Column should be errors"
    );
    let messages: Vec<&str> = report
        .errors
        .iter()
        .filter_map(|e| match e {
            ArmlError::Type { message, .. } => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("requires an integer literal") && m.contains("Grid.Row")),
        "missing Grid.Row non-integer error: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("requires an integer literal") && m.contains("Grid.Column")),
        "missing Grid.Column non-integer error: {messages:?}"
    );
}

#[test]
fn typecheck_grid_attached_integer_literals_ok() {
    // RFC 040：整数字面量（含负值）保留合法；整数 + 非法组合不互斥报错。
    let src = r#"<Window>
        <Grid>
            <TextBlock Grid.Row="1" Grid.Column="0" Text="A"/>
            <TextBlock Grid:Row="-2" Text="B"/>
        </Grid>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(
        report.is_ok(),
        "integer Grid.Row/Column should be valid: {:?}",
        report.errors
    );
    assert!(
        report.warnings.is_empty(),
        "warnings: {:?}",
        report.warnings
    );
}
