//! `.arml` 抽象语法树。
//!
//! 对齐 WPF XAML 语法（RFC 037 D1）：元素树 + 属性 + 内容 + 标记扩展。

use smol_str::SmolStr;

/// 源代码位置（字节偏移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn dummy() -> Self {
        Self { start: 0, end: 0 }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// 标识符（元素名、属性名、命名空间前缀）。
pub type Ident = SmolStr;

/// `.arml` 文档根。
#[derive(Debug, Clone)]
pub struct ArmlDocument {
    /// XML 声明（`<?xml version="1.0" ?>`），可选。
    pub xml_decl: Option<XmlDecl>,
    /// 根元素（通常是 `Window`/`Page`/`UserControl`/`Application`）。
    pub root: Element,
    pub span: Span,
}

/// XML 声明 `<?xml version="1.0" encoding="UTF-8" ?>`。
#[derive(Debug, Clone, Default)]
pub struct XmlDecl {
    pub version: SmolStr,
    pub encoding: Option<SmolStr>,
    pub standalone: Option<SmolStr>,
}

/// XML 元素。
#[derive(Debug, Clone)]
pub struct Element {
    /// 元素名（如 `Window`/`StackPanel`/`Text`）。
    pub name: Ident,
    /// 命名空间前缀（如 `x` for `x:Class`）。
    pub prefix: Option<Ident>,
    /// 属性列表（保持源码顺序）。
    pub attributes: Vec<Attribute>,
    /// 子节点（元素、文本、注释）。
    pub children: Vec<ElementChild>,
    pub span: Span,
}

impl Element {
    /// 查找指定名称的属性（不考虑前缀）。
    pub fn attr(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// 查找带前缀的属性（如 `x:Class`）。
    pub fn attr_with_prefix(&self, prefix: &str, name: &str) -> Option<&Attribute> {
        self.attributes
            .iter()
            .find(|a| a.prefix.as_deref() == Some(prefix) && a.name == name)
    }

    /// 是否为自闭合元素（无子节点）。
    pub fn is_self_closing(&self) -> bool {
        self.children.is_empty()
    }

    /// 直接子元素（不含文本/注释）。
    pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            ElementChild::Element(e) => Some(e),
            _ => None,
        })
    }
}

/// 元素子节点。
#[derive(Debug, Clone)]
pub enum ElementChild {
    /// 子元素。
    Element(Element),
    /// 文本内容。
    Text(TextNode),
    /// XML 注释 `<!-- ... -->`。
    Comment(CommentNode),
}

impl ElementChild {
    /// 若为 Element 返回引用。
    pub fn as_element(&self) -> Option<&Element> {
        match self {
            ElementChild::Element(e) => Some(e),
            _ => None,
        }
    }

    /// 若为 Text 返回引用。
    pub fn as_text(&self) -> Option<&TextNode> {
        match self {
            ElementChild::Text(t) => Some(t),
            _ => None,
        }
    }

    /// 若为 Comment 返回引用。
    pub fn as_comment(&self) -> Option<&CommentNode> {
        match self {
            ElementChild::Comment(c) => Some(c),
            _ => None,
        }
    }
}

/// 文本节点。
#[derive(Debug, Clone)]
pub struct TextNode {
    pub text: SmolStr,
    pub span: Span,
}

/// XML 注释 `<!-- ... -->`。
#[derive(Debug, Clone)]
pub struct CommentNode {
    pub text: SmolStr,
    pub span: Span,
}

/// 元素属性。
#[derive(Debug, Clone)]
pub struct Attribute {
    /// 属性名（如 `Title`/`Width`/`Class`）。
    pub name: Ident,
    /// 命名空间前缀（如 `x` for `x:Class`）。
    pub prefix: Option<Ident>,
    /// 属性值（字面量或标记扩展）。
    pub value: AttributeValue,
    pub span: Span,
}

impl Attribute {
    /// 限定名（`x:Class` 形式）。
    pub fn qualified_name(&self) -> String {
        match &self.prefix {
            Some(p) => format!("{}:{}", p, self.name),
            None => self.name.to_string(),
        }
    }
}

/// 属性值。
#[derive(Debug, Clone)]
pub enum AttributeValue {
    /// 字面量字符串（如 `Title="Counter"`）。
    Literal(SmolStr),
    /// 标记扩展（如 `{x:Bind Count, Mode=OneWay}`）。
    MarkupExtension(MarkupExtension),
}

impl AttributeValue {
    /// 获取字面量值（若为 Literal）。
    pub fn as_literal(&self) -> Option<&str> {
        match self {
            AttributeValue::Literal(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 获取标记扩展（若为 MarkupExtension）。
    pub fn as_markup(&self) -> Option<&MarkupExtension> {
        match self {
            AttributeValue::MarkupExtension(m) => Some(m),
            _ => None,
        }
    }
}

/// 标记扩展（RFC 037 D1.1）。
///
/// 支持：`x:Bind` / `Binding` / `StaticResource` / `Token`。
#[derive(Debug, Clone)]
pub struct MarkupExtension {
    /// 扩展种类（`x:Bind`/`Binding`/`StaticResource`/`Token`）。
    pub kind: MarkupKind,
    /// 位置参数（如 `x:Bind Count` 中的 `Count`）。
    pub args: Vec<SmolStr>,
    /// 命名参数（如 `Mode=OneWay`）。
    pub properties: Vec<(SmolStr, SmolStr)>,
    pub span: Span,
}

/// 标记扩展种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupKind {
    /// `{x:Bind path, Mode=OneWay|TwoWay|OneTime}` 编译期绑定（RFC 037 D4）。
    XBind,
    /// `{Binding path, Mode=...}` 运行时绑定（回退方案）。
    Binding,
    /// `{StaticResource key}` 静态资源引用（应用期按活动主题解析；主题即资源）。
    StaticResource,
    /// `{x:Type Button}` 类型引用（Style.TargetType 指定目标控件类型）。
    XType,
    /// `{Token name}` 设计 Token 引用（RFC 037 D3）。
    Token,
}

impl MarkupKind {
    /// 从标记扩展类型字符串解析。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "x:Bind" => Some(MarkupKind::XBind),
            "Binding" => Some(MarkupKind::Binding),
            "StaticResource" => Some(MarkupKind::StaticResource),
            "x:Type" => Some(MarkupKind::XType),
            "Token" => Some(MarkupKind::Token),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            MarkupKind::XBind => "x:Bind",
            MarkupKind::Binding => "Binding",
            MarkupKind::StaticResource => "StaticResource",
            MarkupKind::XType => "x:Type",
            MarkupKind::Token => "Token",
        }
    }
}

/// 指令元素种类（RFC 026 D1 指令元素 / D2.5 资源字典）。
///
/// 与可视组件树分离：`Style` / `ResourceDictionary` / `Setter` 等不参与布局，
/// 由 typeck 走专用校验路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    Style,
    Setter,
    ResourceDictionary,
    /// 属性元素 `<*.Resources>` 容器。
    Resources,
    /// 属性元素 `<*.Styles>` 作用域样式容器（RFC 026 D3.5）。
    Styles,
    MergedDictionaries,
    ThemeDictionaries,
    ControlTemplate,
    DataTemplate,
    VisualStateManager,
}

impl DirectiveKind {
    pub fn from_element_name(name: &str) -> Option<Self> {
        match name {
            "Style" => Some(Self::Style),
            "Setter" => Some(Self::Setter),
            "ResourceDictionary" => Some(Self::ResourceDictionary),
            "Resources" => Some(Self::Resources),
            "Styles" => Some(Self::Styles),
            "MergedDictionaries" => Some(Self::MergedDictionaries),
            "ThemeDictionaries" => Some(Self::ThemeDictionaries),
            "ControlTemplate" => Some(Self::ControlTemplate),
            "DataTemplate" => Some(Self::DataTemplate),
            "VisualStateManager" => Some(Self::VisualStateManager),
            _ => None,
        }
    }

    pub fn is_directive(name: &str) -> bool {
        Self::from_element_name(name).is_some()
    }
}

/// `<Setter Property="..." Value="..."/>` 结构化视图。
#[derive(Debug, Clone)]
pub struct SetterDef {
    pub property: Ident,
    pub value: AttributeValue,
    pub span: Span,
}

impl SetterDef {
    pub fn from_element(el: &Element) -> Option<Self> {
        if el.name.as_str() != "Setter" {
            return None;
        }
        let property = el.attr("Property")?.value.as_literal()?.into();
        let value = setter_value(el)?;
        Some(Self {
            property,
            value,
            span: el.span,
        })
    }
}

/// `<Style ...>` 结构化视图。
#[derive(Debug, Clone)]
pub struct StyleDef {
    pub key: Option<Ident>,
    pub target_type: Option<Ident>,
    pub based_on: Option<AttributeValue>,
    pub ancestor_type: Option<Ident>,
    pub setters: Vec<SetterDef>,
    pub span: Span,
}

impl StyleDef {
    pub fn from_element(el: &Element) -> Option<Self> {
        if el.name.as_str() != "Style" {
            return None;
        }
        let key = el
            .attr_with_prefix("x", "Key")
            .and_then(|a| a.value.as_literal().map(SmolStr::from));
        let target_type = el.attr("TargetType").and_then(|a| match &a.value {
            // 双形态：字面量 `TargetType="Button"` 或 `{x:Type Button}`（WPF 惯例）。
            AttributeValue::Literal(s) => Some(SmolStr::from(s.as_str())),
            AttributeValue::MarkupExtension(ext) if ext.kind == MarkupKind::XType => {
                ext.args.first().cloned()
            }
            _ => None,
        });
        let based_on = el.attr("BasedOn").map(|a| a.value.clone());
        let ancestor_type = el
            .attr("AncestorType")
            .and_then(|a| a.value.as_literal().map(SmolStr::from));
        let setters = el
            .child_elements()
            .filter_map(SetterDef::from_element)
            .collect();
        Some(Self {
            key,
            target_type,
            based_on,
            ancestor_type,
            setters,
            span: el.span,
        })
    }
}

/// `<ResourceDictionary>` 及其属性元素容器的结构化视图。
#[derive(Debug, Clone)]
pub struct ResourceDictionaryDef {
    pub key: Option<Ident>,
    pub source: Option<Ident>,
    pub styles: Vec<StyleDef>,
    /// 类型化键值条目（`<Color>`/`<Double>`/`<String>` 等，对照 `ResourceValue`）。
    pub entries: Vec<ResourceEntryDef>,
    pub merged: Vec<ResourceDictionaryDef>,
    pub theme_entries: Vec<(Ident, ResourceDictionaryDef)>,
    pub span: Span,
}

impl ResourceDictionaryDef {
    pub fn from_element(el: &Element) -> Option<Self> {
        match el.name.as_str() {
            "ResourceDictionary" => Some(parse_resource_dictionary(el)),
            "Resources" | "Styles" => Some(parse_scope_container(el)),
            _ => None,
        }
    }

    /// 递归收集本字典及合并字典中的全部 Style。
    pub fn all_styles(&self) -> Vec<&StyleDef> {
        let mut out = Vec::new();
        self.collect_styles(&mut out);
        out
    }

    fn collect_styles<'a>(&'a self, out: &mut Vec<&'a StyleDef>) {
        out.extend(self.styles.iter());
        for m in &self.merged {
            m.collect_styles(out);
        }
        for (_, d) in &self.theme_entries {
            d.collect_styles(out);
        }
    }
}

/// `<Theme>` 声明（`<Application.Themes>/<Theme x:Key="Name" BasedOn="...">`）。
///
/// 用户以声明式方式覆盖/新增主题：`<Theme x:Key="Light">` 覆盖内置 Light 中的指定键，
/// `<Theme x:Key="HighContrast" BasedOn="Light">` 继承另一主题再覆盖。主题可**聚合多个
/// ResourceDictionary**（各源为 `Color`/`Double`/`String` 等类型化键值；后声明者覆盖
/// 同名键），直接子条目并入首个隐式字典。
#[derive(Debug, Clone)]
pub struct ThemeDef {
    /// 主题名（`x:Key`）。
    pub key: Ident,
    /// 继承的主题名（`BasedOn`，可选）。
    pub based_on: Option<Ident>,
    /// 聚合的资源源（有序；后声明者覆盖同名键）。
    pub dictionaries: Vec<ResourceDictionaryDef>,
    pub span: Span,
}

impl ThemeDef {
    pub fn from_element(el: &Element) -> Option<Self> {
        if el.name.as_str() != "Theme" {
            return None;
        }
        let key = el
            .attr_with_prefix("x", "Key")
            .and_then(|a| a.value.as_literal())
            .map(SmolStr::from)?;
        let based_on = el
            .attr("BasedOn")
            .and_then(|a| a.value.as_literal())
            .map(SmolStr::from);

        let mut direct = Vec::new();
        let mut dictionaries = Vec::new();
        for child in el.child_elements() {
            match child.name.as_str() {
                "ResourceDictionary" => {
                    if let Some(d) = ResourceDictionaryDef::from_element(child) {
                        dictionaries.push(d);
                    }
                }
                "MergedDictionaries" => {
                    for m in child.child_elements() {
                        if let Some(d) = ResourceDictionaryDef::from_element(m) {
                            dictionaries.push(d);
                        }
                    }
                }
                _ => {
                    if let Some(re) = ResourceEntryDef::from_element(child) {
                        direct.push(re);
                    }
                }
            }
        }
        // 直接条目并入首个隐式字典（保证统一「有序聚合」语义）。
        if !direct.is_empty() {
            let implicit = ResourceDictionaryDef {
                key: None,
                source: None,
                styles: Vec::new(),
                entries: direct,
                merged: Vec::new(),
                theme_entries: Vec::new(),
                span: el.span,
            };
            dictionaries.insert(0, implicit);
        }

        Some(ThemeDef {
            key,
            based_on,
            dictionaries,
            span: el.span,
        })
    }
}

/// 主题/资源中的类型化键值条目（`<Color x:Key="..." Value="..."/>` 等）。
#[derive(Debug, Clone)]
pub struct ResourceEntryDef {
    /// 元素类型名（`Color`/`Double`/`String`/`Boolean` 等）。
    pub type_name: Ident,
    /// 资源键（`x:Key`）。
    pub key: Ident,
    /// 字面量值（`Value="..."` 或嵌套 `<Value>`；标记扩展不计入）。
    pub value: Option<String>,
    pub span: Span,
}

impl ResourceEntryDef {
    pub fn from_element(el: &Element) -> Option<Self> {
        let key = el
            .attr_with_prefix("x", "Key")
            .and_then(|a| a.value.as_literal())
            .map(SmolStr::from)?;
        let value = setter_value(el).and_then(|v| match v {
            AttributeValue::Literal(s) => Some(s.to_string()),
            _ => None,
        });
        Some(ResourceEntryDef {
            type_name: el.name.clone(),
            key,
            value,
            span: el.span,
        })
    }
}

impl Element {
    pub fn directive_kind(&self) -> Option<DirectiveKind> {
        DirectiveKind::from_element_name(self.name.as_str())
    }

    pub fn as_style(&self) -> Option<StyleDef> {
        StyleDef::from_element(self)
    }

    pub fn as_setter(&self) -> Option<SetterDef> {
        SetterDef::from_element(self)
    }

    pub fn as_resource_dictionary(&self) -> Option<ResourceDictionaryDef> {
        ResourceDictionaryDef::from_element(self)
    }
}

impl ArmlDocument {
    /// 收集文档中全部 Style 定义（含 Resources / Styles / ResourceDictionary 作用域）。
    pub fn collect_styles(&self) -> Vec<StyleDef> {
        let mut out = Vec::new();
        collect_styles_in_element(&self.root, &mut out);
        out
    }

    /// 收集文档中全部 ResourceDictionary 视图（含 `Window.Resources` 等属性元素容器）。
    pub fn collect_resource_dictionaries(&self) -> Vec<ResourceDictionaryDef> {
        let mut out = Vec::new();
        collect_dictionaries_in_element(&self.root, &mut out);
        out
    }

    /// 收集 `<Application.Themes>/<Theme ...>` 声明（主题定义，codegen 消费）。
    pub fn collect_themes(&self) -> Vec<ThemeDef> {
        let mut out = Vec::new();
        for child in self.root.child_elements() {
            if child.name.as_str() == "Themes" {
                for t in child.child_elements() {
                    if let Some(td) = ThemeDef::from_element(t) {
                        out.push(td);
                    }
                }
            }
        }
        out
    }
}

fn setter_value(el: &Element) -> Option<AttributeValue> {
    if let Some(attr) = el.attr("Value") {
        return Some(attr.value.clone());
    }
    for child in &el.children {
        if let ElementChild::Element(c) = child {
            if c.name.as_str() == "Value" {
                if let Some(text) = c.children.iter().find_map(|n| n.as_text()) {
                    return Some(AttributeValue::Literal(text.text.clone()));
                }
                if let Some(nested) = c.child_elements().next() {
                    return Some(AttributeValue::Literal(nested.name.clone()));
                }
            }
        }
    }
    None
}

fn parse_resource_dictionary(el: &Element) -> ResourceDictionaryDef {
    let key = el
        .attr_with_prefix("x", "Key")
        .and_then(|a| a.value.as_literal().map(SmolStr::from));
    let source = el
        .attr("Source")
        .and_then(|a| a.value.as_literal().map(SmolStr::from));
    let mut styles = Vec::new();
    let mut entries = Vec::new();
    let mut merged = Vec::new();
    let mut theme_entries = Vec::new();
    for child in el.child_elements() {
        match child.name.as_str() {
            "Style" => {
                if let Some(s) = StyleDef::from_element(child) {
                    styles.push(s);
                }
            }
            "ResourceDictionary" => {
                if let Some(d) = ResourceDictionaryDef::from_element(child) {
                    if d.source.is_some() {
                        merged.push(d);
                    } else if d.key.is_some() {
                        theme_entries.push((d.key.clone().unwrap(), d));
                    } else {
                        merged.push(d);
                    }
                }
            }
            "MergedDictionaries" => {
                for m in child.child_elements() {
                    if let Some(d) = ResourceDictionaryDef::from_element(m) {
                        merged.push(d);
                    }
                }
            }
            "ThemeDictionaries" => {
                for t in child.child_elements() {
                    if let Some(d) = ResourceDictionaryDef::from_element(t) {
                        if let Some(k) = d.key.clone() {
                            theme_entries.push((k, d));
                        }
                    }
                }
            }
            _ => {
                if let Some(re) = ResourceEntryDef::from_element(child) {
                    entries.push(re);
                }
            }
        }
    }
    ResourceDictionaryDef {
        key,
        source,
        styles,
        entries,
        merged,
        theme_entries,
        span: el.span,
    }
}

fn parse_scope_container(el: &Element) -> ResourceDictionaryDef {
    let mut styles = Vec::new();
    let mut entries = Vec::new();
    let mut merged = Vec::new();
    for child in el.child_elements() {
        match child.name.as_str() {
            "Style" => {
                if let Some(s) = StyleDef::from_element(child) {
                    styles.push(s);
                }
            }
            "ResourceDictionary" => {
                if let Some(d) = ResourceDictionaryDef::from_element(child) {
                    merged.push(d);
                }
            }
            _ => {
                if let Some(re) = ResourceEntryDef::from_element(child) {
                    entries.push(re);
                }
            }
        }
    }
    ResourceDictionaryDef {
        key: None,
        source: None,
        styles,
        entries,
        merged,
        theme_entries: Vec::new(),
        span: el.span,
    }
}

fn collect_styles_in_element(el: &Element, out: &mut Vec<StyleDef>) {
    if let Some(style) = StyleDef::from_element(el) {
        out.push(style);
        return;
    }
    if let Some(dict) = ResourceDictionaryDef::from_element(el) {
        out.extend(dict.styles.clone());
        for m in &dict.merged {
            out.extend(m.all_styles().into_iter().cloned());
        }
        for (_, t) in &dict.theme_entries {
            out.extend(t.all_styles().into_iter().cloned());
        }
        return;
    }
    if el.name.as_str() == "MergedDictionaries" || el.name.as_str() == "ThemeDictionaries" {
        for child in el.child_elements() {
            collect_styles_in_element(child, out);
        }
        return;
    }
    for child in el.child_elements() {
        collect_styles_in_element(child, out);
    }
}

fn collect_dictionaries_in_element(el: &Element, out: &mut Vec<ResourceDictionaryDef>) {
    if let Some(dict) = ResourceDictionaryDef::from_element(el) {
        out.push(dict);
        return;
    }
    if el.name.as_str() == "MergedDictionaries" || el.name.as_str() == "ThemeDictionaries" {
        for child in el.child_elements() {
            if let Some(d) = ResourceDictionaryDef::from_element(child) {
                out.push(d);
            }
        }
        return;
    }
    for child in el.child_elements() {
        collect_dictionaries_in_element(child, out);
    }
}
