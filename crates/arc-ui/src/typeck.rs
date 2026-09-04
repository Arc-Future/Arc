//! `.arml` 类型检查器。
//!
//! 验证组件属性、`x:Bind` 表达式、Style Selector（RFC 037 M1）。
//! 组件注册表对齐 WPF XAML 正统命名（RFC 037 D1.1）。

use crate::ast::*;
use crate::error::ArmlError;
use indexmap::IndexMap;
use smol_str::SmolStr;

/// 组件注册表。
///
/// 注册已知组件及其属性、内容模型，供类型检查器查询。
#[derive(Default)]
pub struct ComponentRegistry {
    components: IndexMap<Ident, ComponentInfo>,
}

impl ComponentRegistry {
    /// 创建默认注册表（包含 RFC 037 D1.1 列出的所有元素）。
    pub fn builtin() -> Self {
        let mut reg = Self::default();
        // 根元素
        reg.register(
            ComponentInfo::new("Window")
                .with_content_control_props()
                .with_property("Title", PropType::String)
                .with_property("Left", PropType::Double)
                .with_property("Top", PropType::Double),
        );
        reg.register(ComponentInfo::new("Page").with_content_control_props());
        reg.register(ComponentInfo::new("UserControl").with_content_control_props());
        reg.register(
            ComponentInfo::new("Application").with_property("StartupUri", PropType::String),
        );
        // 布局容器
        reg.register(
            ComponentInfo::new("StackPanel")
                .with_panel_props()
                .with_property("Orientation", PropType::Enum("Orientation"))
                .with_property("Spacing", PropType::Double),
        );
        reg.register(
            ComponentInfo::new("VirtualizingStackPanel")
                .with_panel_props()
                .with_property("Orientation", PropType::Enum("Orientation"))
                .with_property("VerticalOffset", PropType::Double)
                .with_property("ItemHeight", PropType::Double)
                .with_property("CacheLengthBefore", PropType::Double)
                .with_property("CacheLengthAfter", PropType::Double),
        );
        reg.register(ComponentInfo::new("Grid").with_panel_props());
        reg.register(ComponentInfo::new("Canvas").with_panel_props());
        reg.register(
            ComponentInfo::new("ScrollView")
                .with_panel_props()
                .with_property("Content", PropType::Object)
                .with_property(
                    "HorizontalScrollBarVisibility",
                    PropType::Enum("ScrollBarVisibility"),
                )
                .with_property(
                    "VerticalScrollBarVisibility",
                    PropType::Enum("ScrollBarVisibility"),
                )
                .with_property("HorizontalOffset", PropType::Double)
                .with_property("VerticalOffset", PropType::Double),
        );
        reg.register(
            ComponentInfo::new("VisualHost")
                .with_content_control_props()
                .with_property("Child", PropType::Object),
        );
        reg.register(ComponentInfo::new("WrapPanel").with_panel_props());
        reg.register(ComponentInfo::new("DockPanel").with_panel_props());
        // 条件子树 `<Adaptive>`（RFC 016 §11.4）：与 `Match` 同一套结构化条件属性
        reg.register(
            ComponentInfo::new("Adaptive")
                .with_framework_element_props()
                .with_property("Content", PropType::Object)
                .with_property("Tier", PropType::String)
                .with_property("Idiom", PropType::String)
                .with_property("Media", PropType::String)
                .with_property("MediaValue", PropType::String)
                .with_property("Density", PropType::String),
        );
        // 基础控件
        reg.register(
            ComponentInfo::new("Rectangle")
                .with_shape_props()
                .with_property("RadiusX", PropType::Double)
                .with_property("RadiusY", PropType::Double),
        );
        reg.register(
            ComponentInfo::new("TextBlock")
                .with_control_props()
                .with_property("Text", PropType::String)
                .with_property("HorizontalAlignment", PropType::Enum("HorizontalAlignment")),
        );
        reg.register(
            ComponentInfo::new("Button")
                .with_content_control_props()
                .with_property("Click", PropType::EventHandler)
                .with_property("Command", PropType::Object)
                .with_property("CommandParameter", PropType::Object)
                .with_property("IsDefault", PropType::Bool)
                .with_property("IsCancel", PropType::Bool),
        );
        reg.register(
            ComponentInfo::new("Image")
                .with_control_props()
                .with_property("Source", PropType::String)
                .with_property("Stretch", PropType::Enum("Stretch")),
        );
        reg.register(
            ComponentInfo::new("TextBox")
                .with_control_props()
                .with_property("Text", PropType::String)
                .with_property("Placeholder", PropType::String)
                .with_property("IsReadOnly", PropType::Bool)
                .with_property("MaxLength", PropType::Int),
        );
        reg.register(
            ComponentInfo::new("CodeEditor")
                .with_control_props()
                .with_property("VerticalOffset", PropType::Double)
                .with_property("DocumentPath", PropType::String),
        );
        reg.register(
            ComponentInfo::new("CheckBox")
                .with_content_control_props()
                .with_property("IsChecked", PropType::Bool)
                .with_property("IsThreeState", PropType::Bool)
                .with_property("Checked", PropType::EventHandler)
                .with_property("Unchecked", PropType::EventHandler)
                .with_property("Indeterminate", PropType::EventHandler),
        );
        reg.register(
            ComponentInfo::new("Slider")
                .with_control_props()
                .with_property("Minimum", PropType::Double)
                .with_property("Maximum", PropType::Double)
                .with_property("Value", PropType::Double),
        );
        // 内容控件
        reg.register(
            ComponentInfo::new("ContentPresenter")
                .with_framework_element_props()
                .with_property("Content", PropType::Object),
        );
        reg.register(ComponentInfo::new("ContentControl").with_content_control_props());
        reg.register(
            ComponentInfo::new("ItemsControl")
                .with_content_control_props()
                .with_property("ItemsSource", PropType::Object)
                .with_property("ItemTemplate", PropType::Object)
                .with_property("ItemsPanel", PropType::Object)
                .with_property("DisplayMemberPath", PropType::String),
        );
        reg.register(
            ComponentInfo::new("ListView")
                .with_content_control_props()
                .with_property("ItemsSource", PropType::Object)
                .with_property("ItemTemplate", PropType::Object)
                .with_property("ItemsPanel", PropType::Object)
                .with_property("DisplayMemberPath", PropType::String)
                .with_property("SelectedIndex", PropType::Int)
                .with_property("SelectedItem", PropType::Object)
                .with_property("SelectedValue", PropType::Object)
                .with_property("SelectedValuePath", PropType::String)
                .with_property("SelectionMode", PropType::String),
        );
        // RFC 037 §4 · M-VZ4：行虚拟化表格（编程式 AddColumn/AddRow；ARML 属性面
        // 暴露选中/几何 + SelectionChanged 事件名）
        reg.register(
            ComponentInfo::new("DataGrid")
                .with_control_props()
                .with_property("SelectedIndex", PropType::Int)
                .with_property("RowHeight", PropType::Double)
                .with_property("HeaderHeight", PropType::Double)
                .with_property("VerticalOffset", PropType::Double)
                .with_property("SelectionChanged", PropType::EventHandler),
        );
        reg
    }

    pub fn register(&mut self, info: ComponentInfo) {
        self.components.insert(info.name.clone(), info);
    }

    pub fn get(&self, name: &str) -> Option<&ComponentInfo> {
        self.components.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.components.contains_key(name)
    }
}

/// 单个组件的元信息。
#[derive(Default, Debug, Clone)]
pub struct ComponentInfo {
    pub name: Ident,
    pub properties: IndexMap<Ident, PropType>,
}

impl ComponentInfo {
    pub fn new(name: &str) -> Self {
        Self {
            name: SmolStr::new(name),
            properties: IndexMap::new(),
        }
    }

    pub fn with_property(mut self, name: &str, ty: PropType) -> Self {
        self.properties.insert(SmolStr::new(name), ty);
        self
    }

    /// FrameworkElement 继承属性（RFC 037 / RFC 037 D2.1）。
    pub fn with_framework_element_props(mut self) -> Self {
        self = self
            .with_property("Width", PropType::Double)
            .with_property("Height", PropType::Double)
            .with_property("MinWidth", PropType::Double)
            .with_property("MaxWidth", PropType::Double)
            .with_property("MinHeight", PropType::Double)
            .with_property("MaxHeight", PropType::Double)
            .with_property("Margin", PropType::Thickness)
            .with_property("HorizontalAlignment", PropType::Enum("HorizontalAlignment"))
            .with_property("VerticalAlignment", PropType::Enum("VerticalAlignment"))
            .with_property("Style", PropType::Object)
            .with_property("Resources", PropType::Object)
            .with_property("Tag", PropType::Object);
        self
    }

    /// Control 继承属性（Background/Foreground/字体/IsEnabled 等）。
    pub fn with_control_props(mut self) -> Self {
        self = self
            .with_framework_element_props()
            .with_property("Background", PropType::String)
            .with_property("Foreground", PropType::String)
            .with_property("FontFamily", PropType::String)
            .with_property("FontSize", PropType::Double)
            .with_property("FontWeight", PropType::String)
            .with_property("IsEnabled", PropType::Bool)
            .with_property("Template", PropType::Object)
            .with_property("Focusable", PropType::Bool)
            .with_property("IsTabStop", PropType::Bool);
        self
    }

    /// Panel 继承属性（Background + FrameworkElement）。
    pub fn with_panel_props(mut self) -> Self {
        self = self
            .with_framework_element_props()
            .with_property("Background", PropType::String);
        self
    }

    /// Shape 继承属性（Fill/Stroke + FrameworkElement）。
    pub fn with_shape_props(mut self) -> Self {
        self = self
            .with_framework_element_props()
            .with_property("Fill", PropType::String)
            .with_property("Stroke", PropType::String)
            .with_property("StrokeThickness", PropType::Double);
        self
    }

    /// ContentControl 继承属性。
    pub fn with_content_control_props(mut self) -> Self {
        self = self
            .with_control_props()
            .with_property("Content", PropType::String)
            .with_property("ContentTemplate", PropType::Object)
            .with_property("ContentStringFormat", PropType::String)
            .with_property("ContentDirection", PropType::Enum("FlowDirection"))
            .with_property(
                "HorizontalContentAlignment",
                PropType::Enum("HorizontalAlignment"),
            )
            .with_property(
                "VerticalContentAlignment",
                PropType::Enum("VerticalAlignment"),
            )
            .with_property("Padding", PropType::Thickness);
        self
    }

    pub fn has_property(&self, name: &str) -> bool {
        self.properties.contains_key(name)
    }

    pub fn property_type(&self, name: &str) -> Option<&PropType> {
        self.properties.get(name)
    }
}

/// 属性类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropType {
    String,
    Int,
    Double,
    Bool,
    Thickness,
    Enum(&'static str),
    EventHandler,
    Object,
}

/// 自适应数据元素（RFC 027 §11）：不参与可视组件树，由自适应检查器校验。
fn is_adaptive_data_element(name: &str) -> bool {
    matches!(
        name,
        "Double"
            | "Color"
            | "TrackList"
            | "Thickness"
            | "Boolean"
            | "String"
            | "Match"
            | "Tiers"
            | "Media"
    )
}

/// Grid 附加属性白名单（RFC 037 布局 §7 / RFC 040）。
///
/// Grid 真实支持的附加属性仅 `Grid.Row` / `Grid.Column`——RFC 040 typed 化后为
/// `DependencyProperty<int>` + 静态访问器 `Grid.GetRow/SetRow/GetColumn/SetColumn`
/// （对齐 std/UI/Core/Components/Layout/Grid.as）；RowSpan/ColumnSpan 尚未实现
/// （RFC 027 §7 差距项），不列入。
const GRID_ATTACHED_PROPERTIES: &[&str] = &["Row", "Column"];

/// 解析附加属性宿主与本地名（点形式 `Grid.Row="1"` 或前缀形式 `Grid:Row="1"`）。
///
/// parser 对点不拆分，`Grid.Row` 整体留在 `attr.name`（prefix=None）；
/// `Grid:Row` 则拆为 prefix=Some("Grid")、name="Row"。两种形式 codegen 均按附加属性发射。
/// 仅已知宿主 `Grid` 返回 `Some`；未知宿主返回 `None`，走普通属性检查（维持现行为）。
fn attached_property_parts(attr: &Attribute) -> Option<(&str, &str)> {
    match attr.prefix.as_deref() {
        Some("Grid") => Some(("Grid", attr.name.as_str())),
        Some(_) => None,
        None => {
            let (host, local) = attr.name.split_once('.')?;
            if host == "Grid" {
                Some((host, local))
            } else {
                None
            }
        }
    }
}

/// 类型检查报告。
#[derive(Default, Debug, Clone)]
pub struct TypeCheckReport {
    pub errors: Vec<ArmlError>,
    pub warnings: Vec<ArmlError>,
    pub component_count: usize,
    pub binding_count: usize,
    pub style_count: usize,
    /// M5 事件签名匹配：声明的 `EventHandler` 属性（如 `<Button Click="OnRefresh"/>`）
    /// 数量。codegen 将每个事件挂接为 `On*(_ => this.Method())`，其 handler 方法签名
    /// 由 Arc 编译器 typeck 在构建时校验（RFC 006 M5 事件签名匹配）。
    pub event_count: usize,
}

impl TypeCheckReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// 类型检查器。
pub struct TypeChecker {
    registry: ComponentRegistry,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            registry: ComponentRegistry::builtin(),
        }
    }

    pub fn with_registry(registry: ComponentRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &ComponentRegistry {
        &self.registry
    }

    /// 检查文档：遍历元素树，验证组件名、属性名、`x:Bind` 绑定路径与 Style 块。
    pub fn check(&self, doc: &ArmlDocument) -> TypeCheckReport {
        let mut report = TypeCheckReport::default();
        self.check_element(&doc.root, &mut report);
        report
    }

    fn check_element(&self, element: &Element, report: &mut TypeCheckReport) {
        if let Some(kind) = element.directive_kind() {
            self.check_directive(kind, element, report);
            return;
        }
        // 自适应数据元素（类型化值元素 / `<Match>` / `<*.Tiers>` / `<*.Media>`）：
        // 不参与可视组件树，由 `arc ui verify` 的自适应检查器（RFC 037 §11）校验。
        if is_adaptive_data_element(&element.name) {
            return;
        }
        report.component_count += 1;
        // 组件名检查
        if let Some(info) = self.registry.get(&element.name) {
            // 属性检查
            for attr in &element.attributes {
                // 跳过 xmlns 声明（`xmlns` 与 `xmlns:prefix`）与 x: 指令
                let qname = attr.qualified_name();
                if qname == "xmlns" || qname.starts_with("xmlns:") {
                    continue;
                }
                if attr.prefix.as_deref() == Some("x") {
                    continue;
                }
                // 附加属性白名单（RFC 037 布局 §7）：`Grid.Row="1"` 由 Grid 宿主提供，
                // 不参与普通属性名检查；已知宿主但名字不在白名单 → unknown attached property。
                match attached_property_parts(attr) {
                    Some((host, local)) => {
                        if !GRID_ATTACHED_PROPERTIES.contains(&local) {
                            report.warnings.push(ArmlError::type_error(
                                attr.span,
                                format!(
                                    "unknown attached property `{host}.{local}` on `<{}>`",
                                    element.name
                                ),
                            ));
                        } else if let Some(lit) = attr.value.as_literal() {
                            // RFC 040：Grid.Row/Column 为 typed DependencyProperty<int>，
                            // 仅接受整数字面量——非整数（如 "1.5"/"abc"）编译期报错
                            // （原运行期 (int) 截断已收紧）。标记扩展（x:Bind 等）不经
                            // 字面量分支，维持现行为。
                            if lit.parse::<i64>().is_err() {
                                report.errors.push(ArmlError::type_error(
                                    attr.span,
                                    format!(
                                        "attached property `{host}.{local}` requires an integer literal, got `{lit}`"
                                    ),
                                ));
                            }
                        }
                    }
                    None => {
                        if !info.has_property(&attr.name) {
                            report.warnings.push(ArmlError::type_error(
                                attr.span,
                                format!("unknown property `{}` on `<{}>`", attr.name, element.name),
                            ));
                        } else if info.property_type(&attr.name) == Some(&PropType::EventHandler) {
                            // M5 事件签名匹配（RFC 006）：`<Button Click="OnRefresh"/>` 声明式
                            // 事件挂接的值必须是 code-behind 上的方法名。此处校验值形如
                            // 合法标识符（非空、首字符字母/下划线，后续字母/数字/下划线），
                            // 并计入 event_count 供 `arc ui verify` 标注；handler 方法本身的
                            // 签名匹配由 codegen 生成的 `On*(_ => this.Method())` 交给 Arc
                            // 编译器 typeck 在构建时完成（`event_signature_match_e2e` 验收）。
                            if let Some(lit) = attr.value.as_literal() {
                                let valid = !lit.is_empty()
                                    && (lit
                                        .chars()
                                        .next()
                                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_'))
                                    && lit.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                                if !valid {
                                    report.errors.push(ArmlError::type_error(
                                        attr.span,
                                        format!(
                                            "event handler `{}` on `<{}>` must be a method name (identifier), got `{lit}`",
                                            attr.name, element.name
                                        ),
                                    ));
                                }
                            }
                            report.event_count += 1;
                        }
                    }
                }
                // 标记扩展检查
                if let AttributeValue::MarkupExtension(ext) = &attr.value {
                    report.binding_count += 1;
                    self.check_markup_extension(ext, attr.span, report);
                }
            }
        } else {
            report.errors.push(ArmlError::type_error(
                element.span,
                format!("unknown component `<{}>`", element.name),
            ));
        }
        // 递归子元素
        for child in &element.children {
            if let ElementChild::Element(e) = child {
                self.check_element(e, report);
            }
        }
    }

    fn check_directive(
        &self,
        kind: DirectiveKind,
        element: &Element,
        report: &mut TypeCheckReport,
    ) {
        match kind {
            DirectiveKind::Style => self.check_style(element, report),
            DirectiveKind::Setter => self.check_setter(element, None, report),
            DirectiveKind::ResourceDictionary
            | DirectiveKind::Resources
            | DirectiveKind::Styles => self.check_resource_dictionary(element, report),
            DirectiveKind::MergedDictionaries | DirectiveKind::ThemeDictionaries => {
                for child in element.child_elements() {
                    if let Some(k) = child.directive_kind() {
                        self.check_directive(k, child, report);
                    } else if child.name.as_str() == "ResourceDictionary" {
                        self.check_resource_dictionary(child, report);
                    }
                }
            }
            DirectiveKind::ControlTemplate
            | DirectiveKind::DataTemplate
            | DirectiveKind::VisualStateManager => {
                for child in element.child_elements() {
                    if let Some(k) = child.directive_kind() {
                        self.check_directive(k, child, report);
                    }
                }
            }
        }
    }

    fn check_resource_dictionary(&self, element: &Element, report: &mut TypeCheckReport) {
        if let Some(dict) = ResourceDictionaryDef::from_element(element) {
            for style in &dict.styles {
                self.check_style_def(style, report);
            }
            for merged in &dict.merged {
                self.check_merged_dictionary(merged, report);
            }
            for (_, theme) in &dict.theme_entries {
                self.check_merged_dictionary(theme, report);
            }
        }
        for child in element.child_elements() {
            if child.directive_kind().is_some() {
                continue;
            }
            if child.name.as_str() == "MergedDictionaries"
                || child.name.as_str() == "ThemeDictionaries"
            {
                self.check_directive(
                    DirectiveKind::from_element_name(child.name.as_str()).unwrap(),
                    child,
                    report,
                );
            }
        }
    }

    fn check_merged_dictionary(&self, dict: &ResourceDictionaryDef, report: &mut TypeCheckReport) {
        if dict.source.is_none() && dict.key.is_none() && dict.styles.is_empty() {
            report.warnings.push(ArmlError::type_error(
                dict.span,
                "empty merged ResourceDictionary entry",
            ));
        }
        for style in &dict.styles {
            self.check_style_def(style, report);
        }
        for merged in &dict.merged {
            self.check_merged_dictionary(merged, report);
        }
        for (_, theme) in &dict.theme_entries {
            self.check_merged_dictionary(theme, report);
        }
    }

    fn check_style(&self, element: &Element, report: &mut TypeCheckReport) {
        let Some(style) = StyleDef::from_element(element) else {
            report.errors.push(ArmlError::type_error(
                element.span,
                "invalid `<Style>` element",
            ));
            return;
        };
        self.check_style_def(&style, report);
    }

    fn check_style_def(&self, style: &StyleDef, report: &mut TypeCheckReport) {
        report.style_count += 1;
        let has_key = style.key.is_some();
        let has_target = style.target_type.is_some();
        if !has_key && !has_target {
            report.errors.push(ArmlError::type_error(
                style.span,
                "`<Style>` requires `TargetType` and/or `x:Key`",
            ));
        }
        if let Some(target) = &style.target_type {
            if target.as_str() != "*" && !self.registry.contains(target.as_str()) {
                report.errors.push(ArmlError::type_error(
                    style.span,
                    format!("`<Style>` unknown TargetType `{target}`"),
                ));
            }
        }
        if let Some(based_on) = &style.based_on {
            self.check_style_based_on(based_on, style.span, report);
        }
        if style.setters.is_empty() {
            report.warnings.push(ArmlError::type_error(
                style.span,
                "`<Style>` has no `<Setter>` children",
            ));
        }
        let target = style.target_type.as_deref();
        for setter in &style.setters {
            self.check_setter_def(setter, target, report);
        }
    }

    fn check_style_based_on(
        &self,
        value: &AttributeValue,
        span: Span,
        report: &mut TypeCheckReport,
    ) {
        match value {
            AttributeValue::MarkupExtension(ext) if ext.kind == MarkupKind::StaticResource => {
                if ext.args.is_empty() {
                    report.errors.push(ArmlError::type_error(
                        span,
                        "`<Style BasedOn>` requires `{StaticResource key}` argument",
                    ));
                }
            }
            AttributeValue::Literal(_) => {
                report.warnings.push(ArmlError::type_error(
                    span,
                    "`<Style BasedOn>` should use `{StaticResource key}` markup extension",
                ));
            }
            _ => {
                report.warnings.push(ArmlError::type_error(
                    span,
                    "`<Style BasedOn>` should use `{StaticResource key}` markup extension",
                ));
            }
        }
    }

    fn check_setter(
        &self,
        element: &Element,
        target_type: Option<&str>,
        report: &mut TypeCheckReport,
    ) {
        let Some(setter) = SetterDef::from_element(element) else {
            report.errors.push(ArmlError::type_error(
                element.span,
                "`<Setter>` requires `Property` and `Value`",
            ));
            return;
        };
        self.check_setter_def(&setter, target_type, report);
    }

    fn check_setter_def(
        &self,
        setter: &SetterDef,
        target_type: Option<&str>,
        report: &mut TypeCheckReport,
    ) {
        if setter.property.is_empty() {
            report.errors.push(ArmlError::type_error(
                setter.span,
                "`<Setter>` requires non-empty `Property`",
            ));
            return;
        }
        if let AttributeValue::MarkupExtension(ext) = &setter.value {
            self.check_markup_extension(ext, setter.span, report);
        }
        if let Some(target) = target_type {
            if target != "*" {
                if let Some(info) = self.registry.get(target) {
                    if !info.has_property(setter.property.as_str()) {
                        report.warnings.push(ArmlError::type_error(
                            setter.span,
                            format!(
                                "`<Setter Property=\"{}\">` unknown on `<{target}>`",
                                setter.property
                            ),
                        ));
                    }
                }
            }
        }
    }

    fn check_markup_extension(
        &self,
        ext: &MarkupExtension,
        span: Span,
        report: &mut TypeCheckReport,
    ) {
        match ext.kind {
            MarkupKind::XBind => {
                // x:Bind 至少需要一个位置参数（绑定路径）
                if ext.args.is_empty() {
                    report.errors.push(ArmlError::type_error(
                        span,
                        "`x:Bind` requires a binding path (e.g., `{x:Bind Count}`)",
                    ));
                }
                // Mode 参数校验
                for (key, val) in &ext.properties {
                    if key == "Mode" {
                        match val.as_str() {
                            "OneWay" | "TwoWay" | "OneTime" => {}
                            _ => report.errors.push(ArmlError::type_error(
                                span,
                                format!(
                                    "invalid x:Bind Mode `{val}`, expected OneWay/TwoWay/OneTime"
                                ),
                            )),
                        }
                    }
                }
            }
            MarkupKind::XType => {
                if ext.args.is_empty() {
                    report.errors.push(ArmlError::type_error(
                        span,
                        "`{x:Type}` requires a type name (e.g., `{x:Type Button}`)",
                    ));
                } else if !self.registry.contains(ext.args[0].as_str()) {
                    report.errors.push(ArmlError::type_error(
                        span,
                        format!("`{{x:Type}}` unknown type `{}`", ext.args[0]),
                    ));
                }
            }
            MarkupKind::Binding | MarkupKind::StaticResource | MarkupKind::Token => {
                if ext.args.is_empty() {
                    report.warnings.push(ArmlError::type_error(
                        span,
                        format!(
                            "`{}` markup extension requires a key argument",
                            ext.kind.as_str()
                        ),
                    ));
                }
                // RFC 037：多资源绑定（`{StaticResource K1, K2}`）逐键非空——
                // 空段在 codegen 键解析中会产生悬空查找，编译期拦截。
                for arg in &ext.args {
                    if arg.trim().is_empty() {
                        report.errors.push(ArmlError::type_error(
                            span,
                            format!(
                                "`{}` markup extension keys must be non-empty, got empty segment",
                                ext.kind.as_str()
                            ),
                        ));
                    }
                }
            }
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}
