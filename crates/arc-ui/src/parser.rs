//! `.arml` 递归下降语法分析器。
//!
//! 复用 Lexer 的字符游标，按需词法分析 QName/字符串/标记扩展。
//! 支持：XML 声明、元素树、属性、内容（元素/文本/注释）、
//! 属性元素语法（`<Button.Background>` / `<Window.Resources>`）、
//! 指令元素（`<Style>` / `<ResourceDictionary>` / `<Setter>`）、
//! 标记扩展（`{x:Bind ...}` / `{StaticResource ...}`）。

use crate::ast::*;
use crate::error::{ArmlError, ArmlResult};
use crate::lexer::Lexer;
use smol_str::SmolStr;

/// `.arml` 语法分析器。
pub struct Parser<'a> {
    lexer: Lexer<'a>,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            lexer: Lexer::new(input),
        }
    }

    /// 解析完整 `.arml` 文档。
    pub fn parse(input: &'a str) -> ArmlResult<ArmlDocument> {
        let mut parser = Parser::new(input);
        parser.parse_document()
    }

    /// 解析文档：可选 XML 声明 + 根元素。
    fn parse_document(&mut self) -> ArmlResult<ArmlDocument> {
        let start = self.lexer.pos();
        self.skip_prolog();
        let mut xml_decl = None;
        // 处理 XML 声明与处理指令
        loop {
            self.lexer.skip_whitespace();
            if self.lexer.starts_with("<?") {
                let (target, content) = self.lexer.lex_processing_instruction()?;
                if target == "xml" {
                    xml_decl = Some(self.parse_xml_decl(&content)?);
                }
                // 其他处理指令忽略
            } else {
                break;
            }
        }
        // 跳过注释与空白
        loop {
            self.lexer.skip_whitespace();
            if self.lexer.starts_with("<!--") {
                let _ = self.lexer.lex_comment()?;
            } else {
                break;
            }
        }
        // 根元素
        if self.lexer.peek() != Some(b'<') {
            return Err(ArmlError::parse(
                self.lexer.current_span(),
                format!(
                    "expected root element `<...>`, found `{}`",
                    self.lexer.peek().map(|b| b as char).unwrap_or(' ')
                ),
            ));
        }
        let root = self.parse_element()?;
        let end = self.lexer.pos();
        // 尾随注释与空白允许
        Ok(ArmlDocument {
            xml_decl,
            root,
            span: Span { start, end },
        })
    }

    /// 解析 XML 声明内容 `version="1.0" encoding="UTF-8" standalone="yes"`。
    fn parse_xml_decl(&self, content: &str) -> ArmlResult<XmlDecl> {
        let mut version = SmolStr::default();
        let mut encoding = None;
        let mut standalone = None;
        for part in content.split_whitespace() {
            if let Some(eq_pos) = part.find('=') {
                let key = &part[..eq_pos];
                let val = part[eq_pos + 1..].trim_matches('"');
                match key {
                    "version" => version = SmolStr::new(val),
                    "encoding" => encoding = Some(SmolStr::new(val)),
                    "standalone" => standalone = Some(SmolStr::new(val)),
                    _ => {}
                }
            }
        }
        Ok(XmlDecl {
            version,
            encoding,
            standalone,
        })
    }

    /// 跳过文档 prolog（BOM 等）。
    fn skip_prolog(&mut self) {
        // UTF-8 BOM
        if self.lexer.starts_with("\u{FEFF}") {
            self.lexer.bump(3);
        }
    }

    /// 解析元素 `<name attrs>content</name>` 或 `<name attrs/>`。
    fn parse_element(&mut self) -> ArmlResult<Element> {
        let start = self.lexer.pos();
        // 消费 `<`
        if self.lexer.peek() != Some(b'<') {
            return Err(ArmlError::parse(
                self.lexer.current_span(),
                "expected `<` to start element",
            ));
        }
        self.lexer.advance();
        let (name, prefix) = self.parse_qualified_name()?;
        let attributes = self.parse_attributes()?;
        // 自闭合或开始标签结束
        self.lexer.skip_whitespace();
        let self_closing = if self.lexer.starts_with("/>") {
            self.lexer.bump(2);
            true
        } else if self.lexer.peek() == Some(b'>') {
            self.lexer.advance();
            false
        } else {
            return Err(ArmlError::parse(
                self.lexer.current_span(),
                format!(
                    "expected `>` or `/>`, found `{}`",
                    self.lexer.peek().map(|b| b as char).unwrap_or(' ')
                ),
            ));
        };
        if self_closing {
            return Ok(Element {
                name,
                prefix,
                attributes,
                children: Vec::new(),
                span: self.lexer.span_from(start),
            });
        }
        // 内容
        let children = self.parse_element_content(&name)?;
        let end = self.lexer.pos();
        Ok(Element {
            name,
            prefix,
            attributes,
            children,
            span: Span { start, end },
        })
    }

    /// 解析限定名 `prefix:local` 或 `local`。
    fn parse_qualified_name(&mut self) -> ArmlResult<(Ident, Option<Ident>)> {
        let qname = self.lexer.lex_qname()?;
        if let Some(colon_pos) = qname.find(':') {
            let prefix = SmolStr::new(&qname[..colon_pos]);
            let local = SmolStr::new(&qname[colon_pos + 1..]);
            Ok((local, Some(prefix)))
        } else {
            Ok((qname, None))
        }
    }

    /// 解析属性列表（直到 `>` 或 `/>`）。
    fn parse_attributes(&mut self) -> ArmlResult<Vec<Attribute>> {
        let mut attrs = Vec::new();
        loop {
            self.lexer.skip_whitespace();
            match self.lexer.peek() {
                Some(b'>') => break,
                None => {
                    return Err(ArmlError::parse(
                        self.lexer.current_span(),
                        "unterminated attribute list, missing `>`",
                    ))
                }
                Some(b'/') if self.lexer.peek_at(1) == Some(b'>') => break,
                _ => {}
            }
            let attr = self.parse_attribute()?;
            attrs.push(attr);
        }
        Ok(attrs)
    }

    /// 解析单个属性 `name="value"`。
    fn parse_attribute(&mut self) -> ArmlResult<Attribute> {
        let start = self.lexer.pos();
        let (name, prefix) = self.parse_qualified_name()?;
        self.lexer.skip_whitespace();
        if self.lexer.peek() != Some(b'=') {
            return Err(ArmlError::parse(
                self.lexer.current_span(),
                format!("expected `=` after attribute name `{}`", name),
            ));
        }
        self.lexer.advance();
        self.lexer.skip_whitespace();
        let value = self.parse_attribute_value()?;
        Ok(Attribute {
            name,
            prefix,
            value,
            span: self.lexer.span_from(start),
        })
    }

    /// 解析属性值（字面量或标记扩展）。
    fn parse_attribute_value(&mut self) -> ArmlResult<AttributeValue> {
        if self.lexer.peek() != Some(b'"') {
            return Err(ArmlError::parse(
                self.lexer.current_span(),
                "expected `\"` to start attribute value",
            ));
        }
        // 检查是否为标记扩展 `{...}`
        // 向前看一位：`"{...}"` 形式
        let val_start = self.lexer.pos() + 1;
        if self.lexer.peek_at(1) == Some(b'{') {
            // 消费 `"` 与 `{`
            self.lexer.advance(); // `"`
            self.lexer.advance(); // `{`
            let (ext, _end) = self.lexer.lex_markup_extension()?;
            // 消费闭合 `"`
            self.lexer.skip_whitespace();
            if self.lexer.peek() == Some(b'"') {
                self.lexer.advance();
            }
            return Ok(AttributeValue::MarkupExtension(ext));
        }
        // 普通字面量
        let lit = self.lexer.lex_string_lit()?;
        let _ = val_start; // 仅用于诊断
        Ok(AttributeValue::Literal(lit))
    }

    /// 解析元素内容（直到 `</name>`）。
    fn parse_element_content(&mut self, expected_close: &Ident) -> ArmlResult<Vec<ElementChild>> {
        let mut children = Vec::new();
        loop {
            self.lexer.skip_whitespace();
            if self.lexer.is_at_end() {
                return Err(ArmlError::parse(
                    self.lexer.current_span(),
                    format!(
                        "unterminated element `{expected_close}`, missing `</{expected_close}>`"
                    ),
                ));
            }
            if self.lexer.starts_with("</") {
                // 闭合标签
                self.lexer.bump(2);
                let (close_name, _) = self.parse_qualified_name()?;
                self.lexer.skip_whitespace();
                if self.lexer.peek() != Some(b'>') {
                    return Err(ArmlError::parse(
                        self.lexer.current_span(),
                        format!("expected `>` after closing tag `</{close_name}`"),
                    ));
                }
                self.lexer.advance();
                if close_name != *expected_close {
                    return Err(ArmlError::parse(
                        self.lexer.current_span(),
                        format!("mismatched closing tag: expected `</{expected_close}>`, found `</{close_name}>`"),
                    ));
                }
                return Ok(children);
            }
            if self.lexer.starts_with("<!--") {
                let start = self.lexer.pos();
                let comment = self.lexer.lex_comment()?;
                children.push(ElementChild::Comment(CommentNode {
                    text: comment,
                    span: self.lexer.span_from(start),
                }));
                continue;
            }
            if self.lexer.starts_with("<?") {
                // 处理指令（忽略内容，但需消费）
                let _ = self.lexer.lex_processing_instruction()?;
                continue;
            }
            if self.lexer.peek() == Some(b'<') {
                // 子元素或属性元素语法
                let child = self.parse_element_or_property_element(expected_close)?;
                children.push(child);
                continue;
            }
            // 文本内容
            let text = self.lexer.lex_text();
            if !text.is_empty() {
                children.push(ElementChild::Text(TextNode {
                    text,
                    span: self.lexer.current_span(),
                }));
            }
        }
    }

    /// 解析子元素或属性元素语法 `<Parent.Property>...</Parent.Property>`。
    fn parse_element_or_property_element(
        &mut self,
        parent_name: &Ident,
    ) -> ArmlResult<ElementChild> {
        let start = self.lexer.pos();
        self.lexer.advance(); // `<`
        let (name, prefix) = self.parse_qualified_name()?;
        // 判断是否为属性元素语法 `<Parent.Property>`
        if let Some(dot_pos) = name.find('.') {
            let owner = &name[..dot_pos];
            let prop = &name[dot_pos + 1..];
            if owner == parent_name.as_str() {
                // 属性元素语法
                let prop_name = SmolStr::new(prop);
                // 解析为元素，name 为属性名；保留属性（如 `<*.Tiers Default="...">`）
                let attributes = self.parse_attributes()?;
                self.lexer.skip_whitespace();
                let self_closing = if self.lexer.starts_with("/>") {
                    self.lexer.bump(2);
                    true
                } else if self.lexer.peek() == Some(b'>') {
                    self.lexer.advance();
                    false
                } else {
                    return Err(ArmlError::parse(
                        self.lexer.current_span(),
                        "expected `>` or `/>` after property element attributes",
                    ));
                };
                let children = if self_closing {
                    Vec::new()
                } else {
                    let mut kids = Vec::new();
                    loop {
                        self.lexer.skip_whitespace();
                        if self.lexer.starts_with("</") {
                            self.lexer.bump(2);
                            let (close_name, _) = self.parse_qualified_name()?;
                            self.lexer.skip_whitespace();
                            if self.lexer.peek() == Some(b'>') {
                                self.lexer.advance();
                            }
                            let _ = close_name;
                            break;
                        }
                        if self.lexer.peek() == Some(b'<') {
                            let child = self.parse_element_or_property_element(&prop_name)?;
                            kids.push(child);
                        }
                    }
                    kids
                };
                return Ok(ElementChild::Element(Element {
                    name: prop_name,
                    prefix: None,
                    attributes,
                    children,
                    span: self.lexer.span_from(start),
                }));
            }
        }
        // 普通子元素，回退到 parse_element 逻辑
        // 由于已消费 `<` 与 QName，需手动解析剩余部分
        let attributes = self.parse_attributes()?;
        self.lexer.skip_whitespace();
        let self_closing = if self.lexer.starts_with("/>") {
            self.lexer.bump(2);
            true
        } else if self.lexer.peek() == Some(b'>') {
            self.lexer.advance();
            false
        } else {
            return Err(ArmlError::parse(
                self.lexer.current_span(),
                format!(
                    "expected `>` or `/>`, found `{}`",
                    self.lexer.peek().map(|b| b as char).unwrap_or(' ')
                ),
            ));
        };
        let children = if self_closing {
            Vec::new()
        } else {
            self.parse_element_content(&name)?
        };
        let end = self.lexer.pos();
        Ok(ElementChild::Element(Element {
            name,
            prefix,
            attributes,
            children,
            span: Span { start, end },
        }))
    }
}
