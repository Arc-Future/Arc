//! RFC 037 M3 切片：`<Style>` / `ResourceDictionary` 解析与 typeck/verify。

use arc_ui::{verify_report, AttributeValue, CodegenOptions, MarkupKind, Parser, TypeChecker};

fn generate_code(src: &str) -> String {
    let doc = Parser::parse(src).unwrap();
    arc_ui::generate(
        &doc,
        &CodegenOptions {
            namespace: "Ns".into(),
            ..CodegenOptions::default()
        },
    )
    .expect("generate")
}

#[test]
fn parse_window_resources_with_style() {
    let src = r##"<Window>
        <Window.Resources>
            <ResourceDictionary>
                <Style x:Key="PrimaryButton" TargetType="Button">
                    <Setter Property="Background" Value="#0044FF"/>
                    <Setter Property="Foreground" Value="#FFFFFF"/>
                </Style>
            </ResourceDictionary>
        </Window.Resources>
        <Button Style="{StaticResource PrimaryButton}" Content="Save"/>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let styles = doc.collect_styles();
    assert_eq!(styles.len(), 1);
    let style = &styles[0];
    assert_eq!(style.key.as_deref(), Some("PrimaryButton"));
    assert_eq!(style.target_type.as_deref(), Some("Button"));
    assert_eq!(style.setters.len(), 2);
    assert_eq!(style.setters[0].property.as_str(), "Background");
    assert_eq!(style.setters[0].value.as_literal().unwrap(), "#0044FF");
}

#[test]
fn parse_implicit_type_selector_style() {
    let src = r#"<Window>
        <Window.Resources>
            <Style TargetType="TextBlock">
                <Setter Property="FontSize" Value="16"/>
            </Style>
        </Window.Resources>
        <TextBlock Text="Hi"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let style = doc.collect_styles().into_iter().next().unwrap();
    assert!(style.key.is_none());
    assert_eq!(style.target_type.as_deref(), Some("TextBlock"));
}

#[test]
fn parse_grid_scoped_styles() {
    let src = r#"<Window>
        <Grid>
            <Grid.Styles>
                <Style TargetType="Button">
                    <Setter Property="Background" Value="Transparent"/>
                </Style>
            </Grid.Styles>
            <Button Content="OK"/>
        </Grid>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let style = doc.collect_styles().into_iter().next().unwrap();
    assert!(style.key.is_none());
    assert_eq!(style.target_type.as_deref(), Some("Button"));
}

#[test]
fn parse_merged_resource_dictionaries() {
    let src = r##"<Window>
        <Window.Resources>
            <ResourceDictionary>
                <ResourceDictionary.MergedDictionaries>
                    <ResourceDictionary Source="themes/light.arml.theme"/>
                </ResourceDictionary.MergedDictionaries>
                <Style x:Key="Accent" TargetType="Button">
                    <Setter Property="Background" Value="#0044FF"/>
                </Style>
            </ResourceDictionary>
        </Window.Resources>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let dicts = doc.collect_resource_dictionaries();
    assert_eq!(dicts.len(), 1);
    let resources = &dicts[0];
    let inner = &resources.merged[0];
    assert_eq!(inner.merged.len(), 1);
    assert_eq!(
        inner.merged[0].source.as_deref(),
        Some("themes/light.arml.theme")
    );
    assert_eq!(inner.styles.len(), 1);
}

#[test]
fn parse_style_based_on_static_resource() {
    let src = r##"<Window>
        <Window.Resources>
            <Style x:Key="Base" TargetType="Button">
                <Setter Property="Background" Value="#0044FF"/>
            </Style>
            <Style x:Key="Derived" TargetType="Button" BasedOn="{StaticResource Base}">
                <Setter Property="FontSize" Value="18"/>
            </Style>
        </Window.Resources>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let styles = doc.collect_styles();
    assert_eq!(styles.len(), 2);
    let derived = styles
        .iter()
        .find(|s| s.key.as_deref() == Some("Derived"))
        .unwrap();
    match &derived.based_on {
        Some(AttributeValue::MarkupExtension(ext)) => {
            assert_eq!(ext.kind, MarkupKind::StaticResource);
            assert_eq!(ext.args[0].as_str(), "Base");
        }
        other => panic!("expected StaticResource BasedOn, got {other:?}"),
    }
}

#[test]
fn typecheck_style_document_ok() {
    let src = r##"<Window>
        <Window.Resources>
            <Style x:Key="PrimaryButton" TargetType="Button">
                <Setter Property="Background" Value="#0044FF"/>
                <Setter Property="Content" Value="OK"/>
            </Style>
        </Window.Resources>
        <Button Style="{StaticResource PrimaryButton}" Content="Save"/>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
    assert_eq!(report.style_count, 1);
    assert_eq!(report.component_count, 2); // Window + Button
}

#[test]
fn typecheck_style_unknown_target_type() {
    let src = r#"<Window>
        <Window.Resources>
            <Style TargetType="UnknownWidget">
                <Setter Property="Width" Value="10"/>
            </Style>
        </Window.Resources>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(!report.is_ok());
    assert!(report.errors.iter().any(|e| matches!(
        e,
        arc_ui::ArmlError::Type { message, .. } if message.contains("TargetType")
    )));
}

#[test]
fn typecheck_style_missing_target_and_key() {
    let src = r##"<Window>
        <Window.Resources>
            <Style>
                <Setter Property="Background" Value="#000"/>
            </Style>
        </Window.Resources>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(!report.is_ok());
    assert!(report.errors.iter().any(|e| matches!(
        e,
        arc_ui::ArmlError::Type { message, .. } if message.contains("TargetType")
    )));
}

#[test]
fn typecheck_setter_unknown_property_warning() {
    let src = r#"<Window>
        <Window.Resources>
            <Style TargetType="Button">
                <Setter Property="NotARealProperty" Value="x"/>
            </Style>
        </Window.Resources>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let report = TypeChecker::new().check(&doc);
    assert!(report.is_ok());
    assert!(report.warnings.iter().any(|w| matches!(
        w,
        arc_ui::ArmlError::Type { message, .. } if message.contains("NotARealProperty")
    )));
}

#[test]
fn verify_duplicate_style_key() {
    let src = r##"<Window>
        <Window.Resources>
            <Style x:Key="Dup" TargetType="Button">
                <Setter Property="Background" Value="#111"/>
            </Style>
            <Style x:Key="Dup" TargetType="TextBlock">
                <Setter Property="FontSize" Value="12"/>
            </Style>
        </Window.Resources>
    </Window>"##;
    let doc = Parser::parse(src).unwrap();
    let report = verify_report(&doc, &TypeChecker::new());
    assert!(!report.is_ok());
    assert!(report.style_issues.iter().any(|e| matches!(
        e,
        arc_ui::ArmlError::Type { message, .. } if message.contains("duplicate Style x:Key")
    )));
}

#[test]
fn codegen_window_style_registration_and_binding() {
    let src = r##"<Window Class="StyleDemoWindow">
        <Window.Resources>
            <Style x:Key="BaseButton" TargetType="Button">
                <Setter Property="FontSize" Value="14"/>
            </Style>
            <Style x:Key="PrimaryButton" TargetType="{x:Type Button}" BasedOn="{StaticResource BaseButton}">
                <Setter Property="Background" Value="#0044FF"/>
            </Style>
        </Window.Resources>
        <Button Style="{StaticResource PrimaryButton}" Content="Save"/>
    </Window>"##;
    let code = generate_code(src);

    // 窗口资源字典：强类型局部变量承载（object DP 静态解析）
    assert!(code.contains("var _resources = new ResourceDictionary();"));
    assert!(code.contains("this.Resources = _resources;"));
    // 需求 4：BasedOn 显式发射（派生样式 1 → 基础样式 0）
    assert!(code.contains("_style_0.Key = \"BaseButton\";"));
    assert!(code.contains("_style_1.BasedOn = \"BaseButton\";"));
    // 需求 2：TargetType 双形态归一（字面量与 {x:Type Button} 同射）
    assert!(code.contains("_style_0.TargetType = \"Button\";"));
    assert!(code.contains("_style_1.TargetType = \"Button\";"));
    // Setter 发射 + 注册收口
    assert!(code.contains("_setter_0_0.Value = SetterValue.Number(14.0);"));
    assert!(code.contains("_style_0.Setters.Add(_setter_0_0);"));
    assert!(code.contains("_setter_1_0.Value = SetterValue.String(\"#0044FF\");"));
    assert!(code.contains("_style_1.Setters.Add(_setter_1_0);"));
    assert!(code.contains("_resources.AddStyle(_style_0);"));
    assert!(code.contains("_resources.AddStyle(_style_1);"));
    // 需求 1：元素端显式 Style 编译定型——键命中窗口字典 → 直接引用注册
    // 的 Style 对象（运行时零字符串查找）
    assert!(code.contains("child_0.Style = _style_1;"));
    // 回归：属性元素容器不得发射为控件实例
    assert!(!code.contains("new Resources();"));
}

#[test]
fn codegen_style_multi_resource_binding_and_fallback() {
    let src = r##"<Window Class="MultiStyleDemoWindow">
        <Window.Resources>
            <Style x:Key="BaseButton" TargetType="Button">
                <Setter Property="FontSize" Value="14"/>
            </Style>
            <Style x:Key="PrimaryButton" TargetType="Button" BasedOn="{StaticResource BaseButton}">
                <Setter Property="Background" Value="#0044FF"/>
            </Style>
        </Window.Resources>
        <Button Style="{StaticResource BaseButton, PrimaryButton}" Content="Save"/>
        <Button Style="{StaticResource AppDanger}" Content="Delete"/>
    </Window>"##;
    let code = generate_code(src);

    // 多键全命中窗口字典 → 定型对象列表依次引用（运行时零字符串查找）
    assert!(code.contains("var _style_refs_0 = new List<Style>();"));
    assert!(code.contains("_style_refs_0.Add(_style_0);"));
    assert!(code.contains("_style_refs_0.Add(_style_1);"));
    assert!(code.contains("child_0.Style = _style_refs_0;"));
    // App 域键（窗口字典不可解析）→ 逗号键字符串，应用期解析链逐键兜底
    assert!(code.contains("child_1.Style = \"AppDanger\";"));
}

#[test]
fn codegen_window_resource_entries_and_merged() {
    let src = r##"<Window Class="ResourceDemoWindow">
        <Window.Resources>
            <Double x:Key="CardRadius" Value="12"/>
            <ResourceDictionary>
                <Color x:Key="Accent" Value="#0044FF"/>
            </ResourceDictionary>
        </Window.Resources>
        <Button Content="OK"/>
    </Window>"##;
    let code = generate_code(src);

    // 扁平条目直发窗口字典；嵌套 <ResourceDictionary> 为 merged 子字典
    assert!(code.contains("_resources.Add(\"CardRadius\", ResourceValue.Number(12.0));"));
    assert!(code
        .contains("_merged_0.Add(\"Accent\", ResourceValue.Brush(Brushes.Parse(\"#0044FF\")));"));
    assert!(code.contains("_resources.MergedDictionaries.Add(_merged_0);"));
    assert!(!code.contains("new Resources();"));
}

#[test]
fn codegen_grid_scoped_styles_float_to_window() {
    let src = r#"<Window Class="ScopedDemoWindow">
        <Grid>
            <Grid.Styles>
                <Style TargetType="Button">
                    <Setter Property="Background" Value="Transparent"/>
                </Style>
            </Grid.Styles>
            <Button Content="OK"/>
        </Grid>
    </Window>"#;
    let code = generate_code(src);

    // 嵌套属性元素容器上浮注册进窗口字典（运行时以 MainWindow.Resources
    // 为唯一 primary 解析域，无子树样式域——静默降级会丢失样式定义）
    assert!(code.contains("this.Resources = _resources;"));
    assert!(code.contains("_style_0.TargetType = \"Button\";"));
    assert!(code.contains("_setter_0_0.Value = SetterValue.String(\"Transparent\");"));
    assert!(code.contains("_resources.AddStyle(_style_0);"));
    // 回归：属性元素容器不得发射为控件实例（垃圾 `new Resources()`）
    assert!(!code.contains("new Resources();"));
    assert!(!code.contains("new Styles();"));
}
