//! Layout codegen smoke tests — attached properties + StackPanel emission.

use arc_ui::{generate, CodegenOptions, Parser};

#[test]
fn codegen_stackpanel_spacing_orientation_emits_layout_tree() {
    let src = r#"<?xml version="1.0" encoding="UTF-8"?>
<Window xmlns="urn:arc-ui" xmlns:x="urn:arc-ui:x"
        x:Class="Test.MainWindow" Title="T" Width="400" Height="300">
    <StackPanel Orientation="Vertical" Spacing="8">
        <TextBlock Text="Hello" FontSize="16"/>
    </StackPanel>
</Window>"#;
    let doc = Parser::parse(src).unwrap();
    let opts = CodegenOptions {
        namespace: "Test".into(),
        ..Default::default()
    };
    let out = generate(&doc, &opts).expect("codegen");
    assert!(out.contains("Spacing = 8"));
    assert!(out.contains("Orientation = Orientation.Vertical"));
    assert!(out.contains("AddChild("));
    assert!(!out.contains("WindowHost.ElementCreate"));
}

#[test]
fn codegen_canvas_attached_properties() {
    let src = r#"<?xml version="1.0" encoding="UTF-8"?>
<Window xmlns="urn:arc-ui" xmlns:x="urn:arc-ui:x"
        x:Class="Test.MainWindow" Title="T" Width="400" Height="300">
    <Canvas>
        <Rectangle Canvas.Left="10" Canvas.Top="20" Width="100" Height="50"/>
    </Canvas>
</Window>"#;
    let doc = Parser::parse(src).unwrap();
    let opts = CodegenOptions {
        namespace: "Test".into(),
        ..Default::default()
    };
    let out = generate(&doc, &opts).expect("codegen");
    assert!(out.contains("SetAttachedNumber(\"Canvas.Left\""));
    assert!(out.contains("SetAttachedNumber(\"Canvas.Top\""));
}

#[test]
fn codegen_grid_column_definitions() {
    let src = r#"<?xml version="1.0" encoding="UTF-8"?>
<Window xmlns="urn:arc-ui" xmlns:x="urn:arc-ui:x"
        x:Class="Test.MainWindow" Title="T" Width="400" Height="300">
    <Grid>
        <Grid.ColumnDefinitions>
            <ColumnDefinition Width="*"/>
            <ColumnDefinition Width="Auto"/>
            <ColumnDefinition Width="100"/>
        </Grid.ColumnDefinitions>
        <TextBlock Grid.Column="0" Text="A"/>
        <TextBlock Grid.Column="1" Text="Auto"/>
        <TextBlock Grid.Column="2" Text="Px"/>
    </Grid>
</Window>"#;
    let doc = Parser::parse(src).unwrap();
    let opts = CodegenOptions {
        namespace: "Test".into(),
        ..Default::default()
    };
    let out = generate(&doc, &opts).expect("codegen");
    assert!(out.contains("ColumnDefinitions ="));
    assert!(out.contains("new ColumnDefinition()"));
    assert!(out.contains("GridLength.Parse(\"*\")"));
    assert!(out.contains("GridLength.Parse(\"Auto\")"));
    assert!(out.contains("GridLength.Parse(\"100\")"));
    // RFC 040：Grid.Column typed 附加属性 → 宿主静态访问器（替代 SetAttachedNumber）
    assert!(out.contains("Grid.SetColumn(child_5, 0)"));
    assert!(out.contains("Grid.SetColumn(child_6, 1)"));
    assert!(out.contains("Grid.SetColumn(child_7, 2)"));
    assert!(!out.contains("SetAttachedNumber(\"Grid.Column\""));
    assert!(!out.contains("new ColumnDefinitions()"));
}

#[test]
fn codegen_grid_attached_row_column_typed_setters() {
    let src = r#"<?xml version="1.0" encoding="UTF-8"?>
<Window xmlns="urn:arc-ui" xmlns:x="urn:arc-ui:x"
        x:Class="Test.MainWindow" Title="T" Width="400" Height="300">
    <Grid>
        <TextBlock Grid.Row="1" Grid.Column="2" Text="A"/>
        <TextBlock Grid:Row="3" Grid:Column="0" Text="B"/>
    </Grid>
</Window>"#;
    let doc = Parser::parse(src).unwrap();
    let opts = CodegenOptions {
        namespace: "Test".into(),
        ..Default::default()
    };
    let out = generate(&doc, &opts).expect("codegen");
    // 点形式 `Grid.Row` / `Grid.Column` → typed 静态访问器
    assert!(out.contains("Grid.SetRow(child_1, 1)"));
    assert!(out.contains("Grid.SetColumn(child_1, 2)"));
    // 前缀形式 `Grid:Row` / `Grid:Column` 同构
    assert!(out.contains("Grid.SetRow(child_2, 3)"));
    assert!(out.contains("Grid.SetColumn(child_2, 0)"));
    assert!(!out.contains("SetAttachedNumber(\"Grid.Row\""));
    assert!(!out.contains("SetAttachedNumber(\"Grid.Column\""));
}

#[test]
fn codegen_grid_attached_non_integer_falls_back_to_legacy_numeric_path() {
    // RFC 040：非整数字面量由 typeck 层编译期拒绝；codegen 直接调用（无 typeck）
    // 时防御性走既有 SetAttachedNumber 数值路径，不发射 typed setter 伪类型。
    let src = r#"<?xml version="1.0" encoding="UTF-8"?>
<Window xmlns="urn:arc-ui" xmlns:x="urn:arc-ui:x"
        x:Class="Test.MainWindow" Title="T" Width="400" Height="300">
    <Grid>
        <TextBlock Grid.Row="1.5" Text="A"/>
        <TextBlock Grid.Column="abc" Text="B"/>
    </Grid>
</Window>"#;
    let doc = Parser::parse(src).unwrap();
    let opts = CodegenOptions {
        namespace: "Test".into(),
        ..Default::default()
    };
    let out = generate(&doc, &opts).expect("codegen");
    assert!(!out.contains("Grid.SetRow("));
    assert!(!out.contains("Grid.SetColumn("));
    assert!(out.contains("SetAttachedNumber(\"Grid.Row\", 1.5)"));
    assert!(out.contains("SetAttachedString(\"Grid.Column\", \"abc\")"));
}
