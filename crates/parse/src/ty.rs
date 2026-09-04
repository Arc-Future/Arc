use crate::lexer::Token;
use ast::*;

use crate::error::ParseError;
use crate::parser::Parser;

impl Parser {
    /// 解析类型基（不含尾随 `[]` 数组后缀）。`new T[n]` 数组分配的元素类型解析用
    /// 此版本，避免 `[` 被当作空数组后缀消费（`[n]` 长度由 `new` 分支单独解析）。
    pub(crate) fn parse_type_base(&mut self) -> Result<Spanned<Type>, ParseError> {
        let start = self.current_span();
        let ty = if self.match_token(&Token::LParen) {
            let mut types = vec![self.parse_type()?];
            while self.match_token(&Token::Comma) {
                types.push(self.parse_type()?);
            }
            self.expect(Token::RParen)?;
            if self.match_token(&Token::Arrow) {
                let ret = self.parse_type()?;
                Type::Func {
                    params: types,
                    ret: Box::new(ret),
                }
            } else {
                return Err(ParseError::Unexpected {
                    span: self.current_span(),
                    expected: "-> (tuple types not supported)".into(),
                    found: self.describe_current(),
                });
            }
        } else if self.match_token(&Token::Void) {
            Type::Named {
                path: vec!["void".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::Float) {
            Type::Named {
                path: vec!["float".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::Double) {
            Type::Named {
                path: vec!["double".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::Long) {
            Type::Named {
                path: vec!["long".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::Short) {
            Type::Named {
                path: vec!["short".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::Byte) {
            Type::Named {
                path: vec!["byte".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::Char) {
            Type::Named {
                path: vec!["char".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::UInt) {
            Type::Named {
                path: vec!["uint".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::ULong) {
            Type::Named {
                path: vec!["ulong".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::UShort) {
            Type::Named {
                path: vec!["ushort".into()],
                generics: vec![],
            }
        } else if self.match_token(&Token::SByte) {
            Type::Named {
                path: vec!["sbyte".into()],
                generics: vec![],
            }
        } else {
            let mut path = vec![self.parse_ident()?];
            while self.match_token(&Token::Dot) {
                path.push(self.parse_ident()?);
            }
            let mut generics = Vec::new();
            if self.match_token(&Token::Lt) {
                loop {
                    // Const generics: integer literal as a generic argument (e.g. `Vector<float, 4>`).
                    if let Token::IntLit(n) = &self.peek().token {
                        let n = *n;
                        self.advance();
                        generics.push(Spanned::new(Type::ConstInt(n), self.prev_span()));
                    } else {
                        generics.push(self.parse_type()?);
                    }
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect_gt_close()?;
            }
            Type::Named { path, generics }
        };
        let mut ty = Spanned::new(ty, start.merge(self.prev_span()));
        if self.match_token(&Token::Question) {
            ty = Spanned::new(
                Type::Nullable {
                    inner: Box::new(ty),
                },
                start.merge(self.prev_span()),
            );
        }
        Ok(ty)
    }

    /// 类型文法（统一后缀链）：`base ('[' ']')* '?'? '?'?`
    ///
    /// 可空后缀 `?` 是**每层复合类型的后缀运算符**，而非基类型专属：
    /// - `string?`        — 可空 string（基级，`parse_type_base` 消费）
    /// - `string?[]`      — 可空 string 的数组（基级 `?` → Array）
    /// - `string[]?`      — 数组本身可空（**本函数数组后缀之后消费**——此前缺失，
    ///   `?` 遗留流中被语句层误吞为三元运算符，产生静默解析错位）
    /// - `string?[]?`     — 两级可空各自成立
    /// 单一后缀链消除「基类型路径 / 复合类型路径」双轨，是 `string[]?`
    /// 缺口的架构级修复（RFC 045 D12 语料暴露）。
    pub(crate) fn parse_type(&mut self) -> Result<Spanned<Type>, ParseError> {
        let mut ty = self.parse_type_base()?;
        let start = ty.span;
        loop {
            if self.match_token(&Token::LBracket) {
                self.expect(Token::RBracket)?;
                let span = start.merge(self.prev_span());
                ty = Spanned::new(
                    Type::Array {
                        inner: Box::new(ty),
                    },
                    span,
                );
                continue;
            }
            if self.match_token(&Token::Question) {
                let span = start.merge(self.prev_span());
                ty = Spanned::new(
                    Type::Nullable {
                        inner: Box::new(ty),
                    },
                    span,
                );
                continue;
            }
            break;
        }
        Ok(ty)
    }

    pub(crate) fn is_block_start_after_lbrace(&self) -> bool {
        matches!(
            self.peek().token,
            Token::Var
                | Token::Return
                | Token::If
                | Token::While
                | Token::For
                | Token::Foreach
                | Token::Switch
                | Token::Break
                | Token::Continue
                | Token::Semi
                | Token::RBrace
        ) || self.looks_like_leading_type_decl()
    }

    pub(crate) fn is_type_start(&self) -> bool {
        matches!(
            self.peek().token,
            Token::Void
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
                | Token::LParen
                | Token::Ident(_)
        )
    }

    pub(crate) fn parse_object_initializer_fields(
        &mut self,
    ) -> Result<Vec<(Ident, Spanned<Expr>)>, ParseError> {
        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let fname = self.parse_ident()?;
            self.expect(Token::Eq)?;
            fields.push((fname, self.parse_expr()?));
            self.match_token(&Token::Comma);
        }
        self.expect(Token::RBrace)?;
        Ok(fields)
    }
}
