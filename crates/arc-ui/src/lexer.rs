//! `.arml` 词法分析器。
//!
//! 手写字符级游标 + 辅助方法，供 parser 直接调用。
//! 不产生扁平 token 列表，而是提供按需词法分析（QName、字符串字面量、
//! 标记扩展等），便于 parser 实现上下文敏感的递归下降。

use crate::ast::{Ident, Span};
use crate::error::{ArmlError, ArmlResult};
use smol_str::SmolStr;

/// 词法 token（用于 inspect/verify 工具按需输出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Token 种类（仅用于诊断输出与测试断言）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// `<`
    OpenAngle,
    /// `</`
    SlashOpen,
    /// `>`
    CloseAngle,
    /// `/>`
    SlashClose,
    /// `=`
    Equals,
    /// `"..."`（属性值字面量，不含引号）
    StringLit(SmolStr),
    /// 限定名（`Window`/`x:Class`/`xmlns:x`）
    QName(Ident),
    /// `{`（标记扩展开始）
    OpenBrace,
    /// `}`（标记扩展结束）
    CloseBrace,
    /// `,`（标记扩展参数分隔）
    Comma,
    /// 文本内容（标签之间的字符数据）
    Text(SmolStr),
    /// XML 注释 `<!-- ... -->`（内容不含定界符）
    Comment(SmolStr),
    /// XML 声明 `<?xml ... ?>`
    XmlDecl(SmolStr),
    /// 文件结束
    Eof,
}

/// 词法分析器（字符游标 + 辅助方法）。
pub struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    pub fn input(&self) -> &'a str {
        self.input
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn is_at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    pub fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    pub fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    pub fn advance(&mut self) {
        self.pos += 1;
    }

    /// 向前推进 `n` 个字符。
    pub fn bump(&mut self, n: usize) {
        self.pos += n;
    }

    pub fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    pub fn starts_with(&self, s: &str) -> bool {
        // 用 bytes 比较避免 UTF-8 多字节字符边界 panic：
        // `self.input[self.pos..]` 要求 `pos` 在字符边界，但 `advance()`
        // 按字节递增，遇到多字节字符（如中文注释）会让 `pos` 落在字符中间。
        // `bytes[pos..].starts_with(s.as_bytes())` 等价但不要求字符边界。
        self.bytes[self.pos..].starts_with(s.as_bytes())
    }

    pub fn span_from(&self, start: usize) -> Span {
        Span {
            start,
            end: self.pos,
        }
    }

    pub fn current_span(&self) -> Span {
        Span {
            start: self.pos,
            end: self.pos,
        }
    }

    /// 词法分析限定名（标签名/属性名，可含 `:`/`-`/`.`）。
    pub fn lex_qname(&mut self) -> ArmlResult<Ident> {
        self.skip_whitespace();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b':' || c == b'.' {
                self.advance();
            } else {
                break;
            }
        }
        if self.pos == start {
            let ch = self.peek().map(|b| b as char).unwrap_or(' ');
            return Err(ArmlError::lex(
                self.span_from(start),
                format!("expected qualified name, found `{ch}`"),
            ));
        }
        Ok(SmolStr::new(&self.input[start..self.pos]))
    }

    /// 词法分析字符串字面量 `"..."`（返回内容不含引号）。
    pub fn lex_string_lit(&mut self) -> ArmlResult<SmolStr> {
        // 调用方保证当前字符是 `"`
        self.advance();
        let start = self.pos;
        loop {
            match self.peek() {
                Some(b'"') => {
                    let raw = &self.input[start..self.pos];
                    let decoded = Self::decode_entities(raw);
                    let content = SmolStr::new(&decoded);
                    self.advance();
                    return Ok(content);
                }
                Some(_) => self.advance(),
                None => {
                    return Err(ArmlError::lex(
                        self.span_from(start),
                        "unterminated string literal, missing `\"`",
                    ));
                }
            }
        }
    }

    /// 词法分析标记扩展 `{...}`（已消费 `{`）。
    ///
    /// 语法：`{Kind arg0, arg1, prop0=val0, prop1=val1}`
    pub fn lex_markup_extension(&mut self) -> ArmlResult<(crate::ast::MarkupExtension, usize)> {
        let start = self.pos;
        // 读取扩展类型（`x:Bind`/`Binding`/`StaticResource`/...）
        let kind_str = self.lex_qname()?;
        let kind = crate::ast::MarkupKind::parse(&kind_str).ok_or_else(|| {
            ArmlError::lex(
                self.span_from(start),
                format!("unknown markup extension `{kind_str}`"),
            )
        })?;
        let mut args = Vec::new();
        let mut properties = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => {
                    return Err(ArmlError::lex(
                        self.span_from(start),
                        "unterminated markup extension, missing `}`",
                    ));
                }
                Some(b'}') => {
                    self.advance();
                    let end = self.pos;
                    return Ok((
                        crate::ast::MarkupExtension {
                            kind,
                            args,
                            properties,
                            span: self.span_from(start),
                        },
                        end,
                    ));
                }
                Some(b',') => {
                    self.advance();
                    // 下一个参数
                }
                Some(b'=') => {
                    // 命名参数：先回退，等下面的 QName 读取
                    // 实际上 `=` 前应该有 QName，这里处理 `=value`
                    self.advance();
                    self.skip_whitespace();
                    // 读取值
                    let val = if self.peek() == Some(b'"') {
                        self.lex_string_lit()?
                    } else {
                        self.lex_qname()?
                    };
                    // 前一个 QName 是 key（在调用方处理）
                    // 这里返回特殊标记
                    // 实际上，更好的方式是返回值，让调用方记录 key
                    properties.push((SmolStr::new(""), val));
                }
                Some(c) if c.is_ascii_alphanumeric() || c == b'_' || c == b':' || c == b'.' => {
                    let name = self.lex_qname()?;
                    self.skip_whitespace();
                    if self.peek() == Some(b'=') {
                        // 命名参数
                        self.advance();
                        self.skip_whitespace();
                        let val = if self.peek() == Some(b'"') {
                            self.lex_string_lit()?
                        } else {
                            self.lex_qname()?
                        };
                        // 修正最后一个 property 的 key
                        if !properties.is_empty() && properties.last().unwrap().0.is_empty() {
                            properties.last_mut().unwrap().0 = name;
                        } else {
                            properties.push((name, val));
                        }
                    } else {
                        // 位置参数
                        args.push(name);
                    }
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }

    /// 词法分析注释 `<!-- ... -->`（已消费 `<!--`）。
    pub fn lex_comment(&mut self) -> ArmlResult<SmolStr> {
        // 消费 `<!--`
        self.pos += 4;
        let start = self.pos;
        loop {
            if self.pos + 3 > self.bytes.len() {
                return Err(ArmlError::lex(
                    self.span_from(start),
                    "unterminated comment, missing `-->`",
                ));
            }
            if self.starts_with("-->") {
                // 用 bytes 切片避免 UTF-8 边界 panic，再从 str 重建。
                // `start..self.pos` 可能跨越多字节字符的中间字节，但只有当
                // 注释内容含未消费的多字节字符时才会发生——这里 start 是
                // 注释开始后的字节偏移，self.pos 是当前游标，二者都是合法
                // 边界（如果 advance 一直按字节推进且未在多字节字符中间停止）。
                let content = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or("");
                let result = SmolStr::new(content.trim());
                self.pos += 3;
                return Ok(result);
            }
            self.advance();
        }
    }

    /// 词法分析处理指令 `<?target ... ?>`（已消费 `<?`）。
    pub fn lex_processing_instruction(&mut self) -> ArmlResult<(SmolStr, SmolStr)> {
        // 消费 `<?`
        self.pos += 2;
        let target_start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                break;
            }
            self.advance();
        }
        let target = SmolStr::new(&self.input[target_start..self.pos]);
        let content_start = self.pos;
        loop {
            if self.pos + 2 > self.bytes.len() {
                return Err(ArmlError::lex(
                    self.span_from(content_start),
                    "unterminated processing instruction, missing `?>`",
                ));
            }
            if self.starts_with("?>") {
                let content = SmolStr::new(self.input[content_start..self.pos].trim());
                self.pos += 2;
                return Ok((target, content));
            }
            self.advance();
        }
    }

    /// 词法分析文本内容（标签外字符数据，直到下一个 `<`）。
    pub fn lex_text(&mut self) -> SmolStr {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'<' {
                break;
            }
            self.advance();
        }
        SmolStr::new(self.input[start..self.pos].trim())
    }

    /// 解析 XML 实体引用（`&amp;`/`&lt;`/`&gt;`/`&quot;`/`&apos;`/`&#NN;`/`&#xNN;`）。
    pub fn decode_entities(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(idx) = rest.find('&') {
            out.push_str(&rest[..idx]);
            let after = &rest[idx..];
            let semi = after.find(';');
            let (entity, decoded, consumed) = match semi {
                Some(end) => {
                    let ent = &after[..=end]; // 含 `;`
                    let inner = &after[1..end]; // 不含 `&` 和 `;`
                    if let Some(hex) = inner
                        .strip_prefix("#x")
                        .or_else(|| inner.strip_prefix("#X"))
                    {
                        let consumed = ent.len();
                        match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                            Some(c) => (ent, c.to_string(), consumed),
                            None => (ent, String::new(), consumed), // 非法 hex：丢弃实体
                        }
                    } else if let Some(dec) = inner.strip_prefix('#') {
                        let consumed = ent.len();
                        match dec.parse::<u32>().ok().and_then(char::from_u32) {
                            Some(c) => (ent, c.to_string(), consumed),
                            None => (ent, String::new(), consumed),
                        }
                    } else {
                        let consumed = ent.len();
                        let decoded = match ent {
                            "&amp;" => "&".into(),
                            "&lt;" => "<".into(),
                            "&gt;" => ">".into(),
                            "&quot;" => "\"".into(),
                            "&apos;" => "'".into(),
                            _ => ent.into(), // 未知命名实体：原样保留
                        };
                        (ent, decoded, consumed)
                    }
                }
                None => (after, String::new(), after.len()), // 无 `;`：丢弃剩余
            };
            let _ = entity;
            out.push_str(&decoded);
            rest = &rest[idx + consumed..];
        }
        out.push_str(rest);
        out
    }
}
