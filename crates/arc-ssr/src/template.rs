//! SSR 模板解析（RFC 040 §5）：标准 HTML 子集 + 三标记绑定模型。
//!
//! 模板语法（诚实边界：宽松标签/属性/文本/注释/自闭合；绑定路径限属性链 +
//! a-for 循环变量作用域）：
//!
//! - 静态文本：原样输出（生成代码写入字符串字面量）
//! - {{Path}}：文本插值（默认转义）
//! - attr={Path}：属性绑定（默认转义·上下文感知）
//! - a-for={x in Xs}：循环（x 为循环变量，Xs 为集合绑定路径）
//! - a-if={B}：条件（B 为 bool 绑定路径）
//! - a-html={Path}：原始 HTML（显式退出转义）
//!
//! 注释（<!-- ... -->）按静态文本保留。属性值支持 "..." 静态值与 {Path}
//! 绑定两种形态；裸属性（无值）仅输出属性名。

use std::fmt;

/// 模板解析错误（带出错位置）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "template error at offset {}: {}",
            self.offset, self.message
        )
    }
}

impl std::error::Error for TemplateError {}

/// 绑定路径：点分属性链（如 `post.Slug` / `Title`）。
///
/// 顶层绑定相对渲染函数模型参数解析；a-for 循环变量在作用域内覆盖首段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingPath {
    pub parts: Vec<String>,
}

impl BindingPath {
    pub fn display(&self) -> String {
        self.parts.join(".")
    }

    /// 从纯文本解析绑定路径（a-for / 普通绑定统一入口）。
    pub fn parse(raw: &str) -> Result<BindingPath, String> {
        let mut parts = Vec::new();
        let mut cur = String::new();
        let chars = raw.chars().peekable();
        for c in chars {
            if c.is_alphanumeric() || c == '_' {
                cur.push(c);
            } else if c == '.' {
                if cur.is_empty() {
                    return Err("empty binding segment".into());
                }
                parts.push(std::mem::take(&mut cur));
            } else if c.is_whitespace() {
                continue;
            } else {
                return Err(format!("unexpected character '{c}' in binding path"));
            }
        }
        if cur.is_empty() {
            return Err("empty binding path".into());
        }
        parts.push(cur);
        Ok(BindingPath { parts })
    }
}

/// a-for 指令：`{var in collection}`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForLoop {
    pub var: String,
    pub collection: BindingPath,
}

/// 属性值形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrKind {
    /// name="value" —— 静态值
    Static(String),
    /// name={...} —— 绑定原文（元素层按指令语义解析）
    Bound(String),
    /// name —— 裸属性（仅输出属性名，如 `disabled`）
    Bare,
}

/// 元素属性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    pub kind: AttrKind,
}

/// 元素节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<Attr>,
    /// a-for 指令（元素整体循环渲染）
    pub for_loop: Option<ForLoop>,
    /// a-if 指令（条件渲染整个元素）
    pub if_cond: Option<BindingPath>,
    /// a-html 指令（元素内容 = 原始 HTML 绑定）
    pub raw_html: Option<BindingPath>,
    pub children: Vec<Node>,
    pub self_closing: bool,
}

/// 槽点：`<a-slot name="header">fallback</a-slot>`（Web Components `<slot>` 心智）。
///
/// 槽定义在**可复用模板**内作占位（布局单 `body` 注入 / 组件具名槽封装扩展）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRef {
    /// 槽名：`""` 即默认槽（对齐 Web Components 未命名 `<slot>`）。
    pub name: String,
    /// `<a-slot>` 内子内容 = 槽未填充时的 fallback（渲染于本模板作用域）。
    pub fallback: Vec<Node>,
}

/// 模块化组件引用：`<a-component path="card" source={Card}>...slot 内容...</a-component>`。
///
/// 把独立模板文件编译为可复用渲染类 `__SsrComponent_{path}`：以 `source` 绑定为组件
/// payload 传入（组件模板内绑定相对 payload 解析）；子内容按 `slot="name"` 属性分发为
/// 具名槽（无 `slot` 属性进默认槽），槽体在调用方作用域渲染、按名注入组件（对标 Vue /
/// Web Components / kilnx 片段槽，RFC 040 §5）。一次编译、1-N 复用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentRef {
    /// 组件模板名（经 arc 管线解析为独立渲染类类名后缀）。
    pub path: String,
    /// 数据绑定，作为组件 payload 传参。
    /// **可选**：缺省时继承当前上下文数据为 payload（组件模板级 DataContext，对齐 WPF）。
    pub source: Option<BindingPath>,
    /// 调用方按 `slot="name"` 提供的具名槽内容（按首见序去重）。
    /// 槽名与组件模板 `<a-slot name>` 契约对应。
    pub slots: Vec<(String, Vec<Node>)>,
    /// 无 `slot` 属性的子内容 → 默认槽（`name` 为空的 `<a-slot>`）。
    pub default: Vec<Node>,
}

/// 模板节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// 静态文本（含 HTML 注释原文）
    Text(String),
    /// {{Path}} 文本插值（默认转义）
    Interpolation(BindingPath),
    /// 元素（含其指令）
    Element(Element),
    /// `<a-slot name="body" />`：内容注入槽。
    Slot(SlotRef),
    /// `<a-component path source>children</a-component>`：模块化片段引用（独立渲染类复用）。
    Component(ComponentRef),
}

/// 解析完成的模板（根节点列表 + 布局声明）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub root: Vec<Node>,
    /// `<a-layout name="..." />`（页面模板根声明）：关联的共享布局名。
    /// 布局编译一次，多个页面引用（1-N 复用）。未声明则为 None。
    pub layout: Option<String>,
}

/// 解析 HTML 模板源码。
pub fn parse_template(source: &str) -> Result<Template, TemplateError> {
    let mut p = Parser {
        src: source,
        pos: 0,
        layout: None,
    };
    let root = p.parse_nodes()?;
    Ok(Template {
        root,
        layout: p.layout,
    })
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    /// 页面模板根声明的布局名（`<a-layout name="..." />`），一次性写入。
    layout: Option<String>,
}

impl<'a> Parser<'a> {
    fn err(&self, msg: impl Into<String>) -> TemplateError {
        TemplateError {
            offset: self.pos,
            message: msg.into(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// 解析节点序列直到输入结束。
    fn parse_nodes(&mut self) -> Result<Vec<Node>, TemplateError> {
        let mut nodes = Vec::new();
        while self.pos < self.src.len() {
            if self.starts_with("<!--") {
                nodes.push(Node::Text(self.parse_comment()?));
            } else if self.peek() == Some('<') {
                let el = self.parse_element()?;
                if let Some(n) = self.element_to_node(el)? {
                    nodes.push(n);
                }
            } else {
                nodes.extend(self.parse_text_nodes()?);
            }
        }
        Ok(nodes)
    }

    /// 解析注释原文（含标记）。
    fn parse_comment(&mut self) -> Result<String, TemplateError> {
        let start = self.pos;
        let end = self.src[start..]
            .find("-->")
            .ok_or_else(|| self.err("unterminated comment"))?;
        self.pos = start + end + 3;
        Ok(self.src[start..self.pos].to_string())
    }

    /// 解析文本节点序列：静态文本与 {{Path}} 插值交错，直到下一个 '<'。
    fn parse_text_nodes(&mut self) -> Result<Vec<Node>, TemplateError> {
        let mut nodes = Vec::new();
        let mut buf = String::new();
        loop {
            if self.pos >= self.src.len() || self.peek() == Some('<') {
                break;
            }
            if self.starts_with("{{") {
                if !buf.is_empty() {
                    nodes.push(Node::Text(std::mem::take(&mut buf)));
                }
                nodes.push(Node::Interpolation(self.parse_interpolation()?));
            } else {
                buf.push(self.bump().unwrap());
            }
        }
        if !buf.is_empty() {
            nodes.push(Node::Text(buf));
        }
        Ok(nodes)
    }

    /// 解析 {{Path}}：消费 '{{' 与 '}}'，返回绑定路径。
    fn parse_interpolation(&mut self) -> Result<BindingPath, TemplateError> {
        debug_assert!(self.starts_with("{{"));
        self.pos += 2;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == '}' {
                break;
            }
            self.bump();
        }
        let raw = self.src[start..self.pos].trim().to_string();
        if !self.starts_with("}}") {
            return Err(self.err("unterminated '{{...}}' interpolation"));
        }
        self.pos += 2;
        BindingPath::parse(&raw).map_err(|m| self.err(format!("interpolation {m}")))
    }

    /// 解析元素：`<tag attr...>` ... `</tag>` 或 `<tag .../>`。
    fn parse_element(&mut self) -> Result<Element, TemplateError> {
        // 消费 '<'
        self.bump();
        self.skip_ws();
        let tag = self.parse_name()?;
        let mut attrs = Vec::new();
        let mut for_loop = None;
        let mut if_cond = None;
        let mut raw_html = None;
        let mut self_closing = false;

        loop {
            self.skip_ws();
            match self.peek() {
                Some('/') => {
                    self.bump();
                    if self.peek() == Some('>') {
                        self.bump();
                        self_closing = true;
                        break;
                    }
                    return Err(self.err("expected '>' after '/'"));
                }
                Some('>') => {
                    self.bump();
                    break;
                }
                None => return Err(self.err("unterminated tag")),
                _ => {
                    let (name, kind) = self.parse_attr()?;
                    match name.as_str() {
                        "a-for" => {
                            let raw = match &kind {
                                AttrKind::Bound(r) => r.clone(),
                                _ => return Err(self.err("a-for requires {x in Xs}")),
                            };
                            for_loop = Some(parse_for_loop(&raw)?);
                        }
                        "a-if" => {
                            let raw = match &kind {
                                AttrKind::Bound(r) => r.clone(),
                                _ => return Err(self.err("a-if requires {B}")),
                            };
                            if_cond = Some(
                                BindingPath::parse(&raw)
                                    .map_err(|m| self.err(format!("a-if {m}")))?,
                            );
                        }
                        "a-html" => {
                            let raw = match &kind {
                                AttrKind::Bound(r) => r.clone(),
                                _ => return Err(self.err("a-html requires {Path}")),
                            };
                            raw_html = Some(
                                BindingPath::parse(&raw)
                                    .map_err(|m| self.err(format!("a-html {m}")))?,
                            );
                        }
                        _ => attrs.push(Attr { name, kind }),
                    }
                }
            }
        }

        // a-component 为片段引用：支持子内容（按 `slot` 属性分发为组件插槽），否则跳过子解析。
        let has_children = !self_closing && !is_void_tag(&tag);
        let children = if has_children {
            self.parse_children(&tag)?
        } else {
            Vec::new()
        };

        Ok(Element {
            tag,
            attrs,
            for_loop,
            if_cond,
            raw_html,
            children,
            self_closing,
        })
    }

    /// 解析子节点直到匹配的闭合标签。
    fn parse_children(&mut self, tag: &str) -> Result<Vec<Node>, TemplateError> {
        let mut nodes = Vec::new();
        loop {
            if self.pos >= self.src.len() {
                return Err(self.err(format!("unclosed element <{tag}>")));
            }
            if self.starts_with("<!--") {
                nodes.push(Node::Text(self.parse_comment()?));
                continue;
            }
            if self.starts_with("</") {
                self.pos += 2;
                let close = self.parse_name()?;
                self.skip_ws();
                if self.peek() != Some('>') {
                    return Err(self.err("expected '>' in closing tag"));
                }
                self.bump();
                if close != tag {
                    return Err(self.err(format!(
                        "mismatched closing tag </{close}> (expected </{tag}>)"
                    )));
                }
                return Ok(nodes);
            }
            if self.peek() == Some('<') {
                let el = self.parse_element()?;
                if let Some(n) = self.element_to_node(el)? {
                    nodes.push(n);
                }
            } else {
                nodes.extend(self.parse_text_nodes()?);
            }
        }
    }

    /// 元素归纳：把可能为伪元素的 Element 归位为 Slot / Component / 布局声明（返回 None 则不产出节点）。
    fn element_to_node(&mut self, el: Element) -> Result<Option<Node>, TemplateError> {
        match el.tag.as_str() {
            "a-slot" => {
                let name = el
                    .attrs
                    .iter()
                    .find(|a| a.name == "name")
                    .map(Parser::attr_value)
                    .unwrap_or_default();
                Ok(Some(Node::Slot(SlotRef {
                    name,
                    fallback: el.children,
                })))
            }
            "a-layout" => {
                // 布局声明 `<a-layout name="..." />`：仅元数据，不产出渲染节点。
                self.layout = el
                    .attrs
                    .iter()
                    .find(|a| a.name == "name")
                    .map(Parser::attr_value);
                Ok(None)
            }
            "a-component" => {
                let mut path = String::new();
                let mut source = None;
                for a in &el.attrs {
                    match a.name.as_str() {
                        "path" => path = Parser::attr_value(a),
                        "source" => source = BindingPath::parse(&Parser::attr_value(a)).ok(),
                        _ => {}
                    }
                }
                let (slots, default) = distribute_slots(el.children);
                Ok(Some(Node::Component(ComponentRef {
                    path,
                    source,
                    slots,
                    default,
                })))
            }
            _ => Ok(Some(Node::Element(el))),
        }
    }

    /// 读取属性值（Static/Bound 原文；Bare 为空）。
    fn attr_value(a: &Attr) -> String {
        match &a.kind {
            AttrKind::Static(v) => v.clone(),
            AttrKind::Bound(v) => v.clone(),
            AttrKind::Bare => String::new(),
        }
    }

    /// 解析属性名 + 值。
    fn parse_attr(&mut self) -> Result<(String, AttrKind), TemplateError> {
        let name = self.parse_name()?;
        self.skip_ws();
        if self.peek() != Some('=') {
            // 裸属性
            return Ok((name, AttrKind::Bare));
        }
        self.bump();
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.bump();
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == '}' {
                        break;
                    }
                    self.bump();
                }
                if self.peek() != Some('}') {
                    return Err(self.err("unterminated '{...}' attribute value"));
                }
                let raw = self.src[start..self.pos].to_string();
                self.bump(); // '}'
                Ok((name, AttrKind::Bound(raw)))
            }
            Some('"') => {
                self.bump();
                let mut value = String::new();
                loop {
                    match self.peek() {
                        Some('"') => {
                            self.bump();
                            break;
                        }
                        Some(c) => {
                            self.bump();
                            value.push(c);
                        }
                        None => return Err(self.err("unterminated attribute value")),
                    }
                }
                Ok((name, AttrKind::Static(value)))
            }
            _ => Err(self.err("expected '{' or \"'\" after '='")),
        }
    }

    /// 解析标识符（标签名 / 属性名）。
    fn parse_name(&mut self) -> Result<String, TemplateError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == ':' {
                self.bump();
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(self.err("expected identifier"));
        }
        Ok(self.src[start..self.pos].to_string())
    }
}

/// 把组件子内容按 `slot` 属性分发给具名槽 / 默认槽（对齐 Web Components named slot assignment）。
///
/// 具名槽按首见序去重保序作契约顺序；`slot` 属性在分发后被剥离（不进入最终 HTML）。
pub fn distribute_slots(children: Vec<Node>) -> (Vec<(String, Vec<Node>)>, Vec<Node>) {
    let mut slots: Vec<(String, Vec<Node>)> = Vec::new();
    let mut default: Vec<Node> = Vec::new();
    for n in children {
        match n {
            Node::Element(mut el) => {
                let mut slot_name = None;
                let mut i = 0;
                while i < el.attrs.len() {
                    if el.attrs[i].name == "slot" {
                        slot_name = Some(Parser::attr_value(&el.attrs[i]));
                        el.attrs.remove(i);
                    } else {
                        i += 1;
                    }
                }
                match slot_name {
                    Some(name) => match slots.iter_mut().find(|(n, _)| *n == name) {
                        Some(existing) => existing.1.push(Node::Element(el)),
                        None => slots.push((name, vec![Node::Element(el)])),
                    },
                    None => default.push(Node::Element(el)),
                }
            }
            other => default.push(other),
        }
    }
    (slots, default)
}

/// 解析 a-for 值原文：`x in Xs`（'{\}' 已剥离）。
fn parse_for_loop(raw: &str) -> Result<ForLoop, TemplateError> {
    let mut parts = raw.splitn(2, "in");
    let var = parts.next().unwrap_or("").trim().to_string();
    let coll = parts
        .next()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if var.is_empty() || coll.is_empty() || !is_identifier(&var) {
        return Err(TemplateError {
            offset: 0,
            message: format!("a-for requires {{var in collection}}, got '{{{raw}}}'"),
        });
    }
    let collection = BindingPath::parse(&coll).map_err(|m| TemplateError {
        offset: 0,
        message: format!("a-for {m}"),
    })?;
    Ok(ForLoop { var, collection })
}

/// HTML void 元素（无闭合标签，隐式自闭合）。
fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}
