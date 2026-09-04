mod item_body;
mod item_fn;
mod item_type;

use crate::lexer::Token;
use ast::*;

use crate::error::{ClassBodyMember, FieldOrProperty, InterfaceBodyMember, ParseError};
use crate::parser::Parser;

impl Parser {
    pub(crate) fn parse_program_items(&mut self) -> Result<Program, ParseError> {
        // 跳过前置注释（`//` 行注释和 `///` 文档注释），然后解析前置 `using` 指令。
        // C# 文件作用域命名空间支持 `using` 指令放在 `namespace X;` 之前：
        //   using System;
        //   namespace MyApp;
        // 此处的策略：先跳过注释 + 解析 `using` 作为正常的 item，再判断后续是否
        // 有 file-scoped namespace。若有，则将 `using` items 并入 namespace body。
        while !self.is_at_end() {
            match &self.peek().token {
                Token::LineComment => {
                    self.advance();
                }
                _ => break,
            }
        }

        // 解析前置 `using` / `global using` 指令
        let saved = self.pos;
        let mut leading_uses: Vec<Spanned<Item>> = Vec::new();
        while self.check(&Token::Using)
            || (self.check(&Token::Global) && self.check_at(1, &Token::Using))
        {
            leading_uses.push(self.parse_item()?);
        }

        // 检查是否有 file-scoped namespace
        if self.check(&Token::Namespace) {
            let start = self.current_span();
            match self.try_parse_file_scoped_namespace() {
                Ok(mut ns) => {
                    let end = self.prev_span();
                    // 将前置 `using` 指令插入 namespace body 的最前面
                    let mut all_items = leading_uses;
                    all_items.append(&mut ns.items);
                    ns.items = all_items;
                    return Ok(Program {
                        items: vec![Spanned::new(Item::Namespace(ns), start.merge(end))],
                    });
                }
                Err(_) => {
                    // 回退到 saved 位置，让 parse_item fallback 处理
                    self.pos = saved;
                }
            }
        } else {
            // 无 file-scoped namespace：将已解析的 `using` 指令并回 items
            // （回退到 saved，让后续 while 循环从这些 `using` 开始解析）
            self.pos = saved;
        }

        let mut items = Vec::new();
        while !self.is_at_end() {
            items.push(self.parse_item()?);
        }
        Ok(Program { items })
    }

    pub(crate) fn parse_item(&mut self) -> Result<Spanned<Item>, ParseError> {
        let doc = self.collect_doc_comments();
        let attrs = self.parse_attributes()?;
        let start = self.current_span();
        let item = if self.check_any(&[
            Token::Public,
            Token::Private,
            Token::Internal,
            Token::Protected,
        ]) {
            match self.peek_at(1) {
                Some(Token::Class) => {
                    let mut c = self.parse_class()?;
                    c.attributes = attrs;
                    c.doc = doc;
                    Item::Class(c)
                }
                Some(Token::Record) => {
                    let mut item = self.parse_record()?;
                    match &mut item {
                        Item::Class(c) => {
                            c.attributes = attrs;
                            c.doc = doc;
                        }
                        Item::Struct(s) => {
                            s.attributes = attrs;
                            s.doc = doc;
                        }
                        _ => unreachable!("parse_record returns Class or Struct"),
                    }
                    item
                }
                // RFC 037：`public partial class ...` / `public partial static class ...`
                // RFC 006：`partial record` 硬拒绝。
                Some(Token::Ident(s)) if s == "partial" => {
                    if self.check_at(2, &Token::Record) {
                        return Err(self.error(
                            "partial class",
                            "`partial record` is not supported (RFC 037 D7.2 / RFC 006)".into(),
                        ));
                    }
                    let mut c = self.parse_class()?;
                    c.attributes = attrs;
                    c.doc = doc;
                    Item::Class(c)
                }
                // RFC 012 M4-1：`public abstract class ...` / `public abstract static class ...`
                // `abstract` 是上下文关键字，由 parse_class 在内部识别并消费。
                Some(Token::Abstract) => {
                    let mut c = self.parse_class()?;
                    c.attributes = attrs;
                    c.doc = doc;
                    Item::Class(c)
                }
                Some(Token::Static) if self.check_at(2, &Token::Class) => {
                    let mut c = self.parse_class()?;
                    c.attributes = attrs;
                    c.doc = doc;
                    Item::Class(c)
                }
                Some(Token::Readonly) if self.check_at(2, &Token::Struct) => {
                    let mut s = self.parse_struct()?;
                    s.attributes = attrs;
                    s.doc = doc;
                    Item::Struct(s)
                }
                Some(Token::Struct) => {
                    let mut s = self.parse_struct()?;
                    s.attributes = attrs;
                    s.doc = doc;
                    Item::Struct(s)
                }
                Some(Token::Interface) => {
                    let mut i = self.parse_interface()?;
                    i.attributes = attrs;
                    i.doc = doc;
                    Item::Interface(i)
                }
                Some(Token::Enum) => {
                    let mut e = self.parse_enum()?;
                    e.attributes = attrs;
                    e.doc = doc;
                    Item::Enum(e)
                }
                // RFC 004 M1：`public variant Name { ... }`
                Some(Token::Variant) => {
                    let mut v = self.parse_variant()?;
                    v.attributes = attrs;
                    v.doc = doc;
                    Item::Variant(v)
                }
                // GAP #5：`public delegate int Converter(int value);`
                Some(Token::Delegate) => {
                    let mut d = self.parse_delegate()?;
                    d.attributes = attrs;
                    d.doc = doc;
                    Item::Delegate(d)
                }
                _ => {
                    let mut f = self.parse_fn()?;
                    f.attributes = attrs;
                    f.doc = doc;
                    Item::Fn(f)
                }
            }
        } else {
            match &self.peek().token {
                Token::Namespace => {
                    if !attrs.is_empty() {
                        return Err(
                            self.error("namespace", "attributes not allowed on namespace".into())
                        );
                    }
                    Item::Namespace(self.parse_namespace()?)
                }
                Token::Using => {
                    if !attrs.is_empty() {
                        return Err(self.error("using", "attributes not allowed on using".into()));
                    }
                    Item::Use(self.parse_use(false)?)
                }
                Token::Global => {
                    if !attrs.is_empty() {
                        return Err(
                            self.error("global using", "attributes not allowed on using".into())
                        );
                    }
                    self.advance();
                    Item::Use(self.parse_use(true)?)
                }
                Token::Readonly if self.check_at(1, &Token::Struct) => {
                    let mut s = self.parse_struct()?;
                    s.attributes = attrs;
                    s.doc = doc;
                    Item::Struct(s)
                }
                Token::Struct => {
                    let mut s = self.parse_struct()?;
                    s.attributes = attrs;
                    s.doc = doc;
                    Item::Struct(s)
                }
                Token::Class => {
                    let mut c = self.parse_class()?;
                    c.attributes = attrs;
                    c.doc = doc;
                    Item::Class(c)
                }
                Token::Record => {
                    let mut item = self.parse_record()?;
                    match &mut item {
                        Item::Class(c) => {
                            c.attributes = attrs;
                            c.doc = doc;
                        }
                        Item::Struct(s) => {
                            s.attributes = attrs;
                            s.doc = doc;
                        }
                        _ => unreachable!("parse_record returns Class or Struct"),
                    }
                    item
                }
                Token::Static if self.check_at(1, &Token::Class) => {
                    let mut c = self.parse_class()?;
                    c.attributes = attrs;
                    c.doc = doc;
                    Item::Class(c)
                }
                // RFC 037：`partial class ...` / `partial static class ...`（无访问修饰符）
                // RFC 006：`partial record` 硬拒绝。
                Token::Ident(s) if s == "partial" => {
                    if self.check_at(1, &Token::Record) {
                        return Err(self.error(
                            "partial class",
                            "`partial record` is not supported (RFC 037 D7.2 / RFC 006)".into(),
                        ));
                    }
                    let mut c = self.parse_class()?;
                    c.attributes = attrs;
                    c.doc = doc;
                    Item::Class(c)
                }
                // RFC 012 M4-1：`abstract class ...` / `abstract static class ...`（无访问修饰符）
                Token::Abstract => {
                    let mut c = self.parse_class()?;
                    c.attributes = attrs;
                    c.doc = doc;
                    Item::Class(c)
                }
                Token::Interface => {
                    let mut i = self.parse_interface()?;
                    i.attributes = attrs;
                    i.doc = doc;
                    Item::Interface(i)
                }
                Token::Enum => {
                    let mut e = self.parse_enum()?;
                    e.attributes = attrs;
                    e.doc = doc;
                    Item::Enum(e)
                }
                // RFC 004 M1：`variant Name { ... }`（无访问修饰符）
                Token::Variant => {
                    let mut v = self.parse_variant()?;
                    v.attributes = attrs;
                    v.doc = doc;
                    Item::Variant(v)
                }
                // GAP #5：`delegate int Converter(int value);`（无访问修饰符）
                Token::Delegate => {
                    let mut d = self.parse_delegate()?;
                    d.attributes = attrs;
                    d.doc = doc;
                    Item::Delegate(d)
                }
                Token::Async
                | Token::Void
                | Token::Float
                | Token::Double
                | Token::Long
                | Token::Short
                | Token::Byte
                | Token::Char
                | Token::UInt
                | Token::ULong
                | Token::UShort
                | Token::SByte
                | Token::LParen => {
                    let mut f = self.parse_fn()?;
                    f.attributes = attrs;
                    f.doc = doc;
                    Item::Fn(f)
                }
                Token::Ident(_)
                    if self.check_at(1, &Token::LParen) || self.check_at(1, &Token::Lt) =>
                {
                    let mut f = self.parse_fn()?;
                    f.attributes = attrs;
                    f.doc = doc;
                    Item::Fn(f)
                }
                Token::Ident(_) if self.starts_with_return_type() => {
                    let mut f = self.parse_fn()?;
                    f.attributes = attrs;
                    f.doc = doc;
                    Item::Fn(f)
                }
                _ => {
                    return Err(self.error("item", self.describe_current()));
                }
            }
        };
        let end = self.prev_span();
        Ok(Spanned::new(item, start.merge(end)))
    }

    /// 收集连续的 `///` 文档注释，拼接为单个字符串（每行原文，以 \n 分隔）。
    /// 遇非 DocComment token 停止。无文档注释返回 None。
    pub(crate) fn collect_doc_comments(&mut self) -> Option<String> {
        let mut lines: Vec<String> = Vec::new();
        while !self.is_at_end() {
            if let Token::DocComment(s) = &self.peek().token {
                lines.push(s.clone());
                self.advance();
            } else {
                break;
            }
        }
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    /// 跳过连续的 `///` 文档注释 token（不提取）。
    /// P1 范围：成员级（Field/Property/Method/EnumVariant）doc 字段暂不提取，
    /// 但需消费 DocComment token 以免阻塞解析。
    pub(crate) fn skip_doc_comments(&mut self) {
        while !self.is_at_end() {
            if matches!(self.peek().token, Token::DocComment(_)) {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset).map(|t| &t.token)
    }

    pub(crate) fn parse_namespace(&mut self) -> Result<NamespaceItem, ParseError> {
        self.expect(Token::Namespace)?;
        let path = self.parse_dotted_path()?;
        let capabilities = self.parse_namespace_capabilities()?;

        if self.match_token(&Token::Semi) {
            if self.check(&Token::LBrace) {
                return Err(self.error(
                    "namespace items",
                    "rejected transitional `namespace X; { }` — use file-scoped \
                     `namespace A.B.C;` or block `namespace A.B.C { }`"
                        .into(),
                ));
            }
            // Fallback 路径：try_parse_file_scoped_namespace 失败后，
            // parse_namespace 被调用并遇到文件作用域 namespace X;。
            // 正确解析剩余 items 作为 namespace body。
            let mut items = Vec::new();
            while !self.is_at_end() {
                items.push(self.parse_item()?);
            }
            return Ok(NamespaceItem {
                path,
                items,
                capabilities,
            });
        }

        self.expect(Token::LBrace)?;
        let mut items = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            items.push(self.parse_item()?);
        }
        self.expect(Token::RBrace)?;
        Ok(NamespaceItem {
            path,
            items,
            capabilities,
        })
    }

    fn try_parse_file_scoped_namespace(&mut self) -> Result<NamespaceItem, ParseError> {
        self.expect(Token::Namespace)?;
        let path = self.parse_dotted_path()?;
        let capabilities = self.parse_namespace_capabilities()?;
        self.expect(Token::Semi)?;
        if self.check(&Token::LBrace) {
            return Err(self.error(
                "namespace items",
                "rejected transitional `namespace X; { }` — use file-scoped \
                 `namespace A.B.C;` or block `namespace A.B.C { }`"
                    .into(),
            ));
        }
        let mut items = Vec::new();
        while !self.is_at_end() {
            items.push(self.parse_item()?);
        }
        Ok(NamespaceItem {
            path,
            items,
            capabilities,
        })
    }

    /// RFC 016 M3 §3.4 能力 gating Phase 1+（[4.4 能力系统]）：
    /// 解析 namespace 上可选的 `capability <ident_list>` 子句。
    ///
    /// 语法：`capability io` 或 `capability io, db, net`（逗号分隔标识符列表）。
    /// `capability` 是上下文关键字（Token::Ident），与 `partial` 一致的处理方式。
    /// 出现在 `namespace X` 与 `{` / `;` 之间。无该子句返回空 Vec。
    fn parse_namespace_capabilities(&mut self) -> Result<Vec<Ident>, ParseError> {
        if !matches!(&self.peek().token, Token::Ident(s) if s == "capability") {
            return Ok(Vec::new());
        }
        self.advance(); // 消费 `capability`
        let mut caps = vec![self.parse_ident()?];
        while self.match_token(&Token::Comma) {
            caps.push(self.parse_ident()?);
        }
        Ok(caps)
    }

    pub(crate) fn parse_dotted_path(&mut self) -> Result<Vec<Ident>, ParseError> {
        let mut path = vec![self.parse_ident()?];
        while self.match_token(&Token::Dot) {
            path.push(self.parse_ident()?);
        }
        Ok(path)
    }

    pub(crate) fn parse_use(&mut self, is_global: bool) -> Result<UseItem, ParseError> {
        self.expect(Token::Using)?;
        let first = self.parse_ident()?;
        if self.match_token(&Token::Eq) {
            let path = self.parse_dotted_path()?;
            self.expect(Token::Semi)?;
            return Ok(UseItem {
                alias: Some(first),
                path,
                is_global,
            });
        }
        let mut path = vec![first];
        while self.match_token(&Token::Dot) {
            path.push(self.parse_ident()?);
        }
        self.expect(Token::Semi)?;
        Ok(UseItem {
            alias: None,
            path,
            is_global,
        })
    }

    pub(crate) fn check_at(&self, offset: usize, token: &Token) -> bool {
        self.tokens
            .get(self.pos + offset)
            .is_some_and(|t| &t.token == token)
    }
}
