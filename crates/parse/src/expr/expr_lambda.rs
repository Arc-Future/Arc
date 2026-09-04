use super::*;

use crate::error::ParseError;
use crate::lexer::Token;
use crate::parser::Parser;

impl Parser {
    pub(crate) fn check_lambda(&self) -> bool {
        // 单参数 lambda：`x => ...` / `x, ...` / `x: Type, ...`（RFC 007 M2c）。
        if matches!(self.peek().token, Token::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
                .map(|t| matches!(t.token, Token::FatArrow | Token::Comma | Token::Colon))
                .unwrap_or(false)
        {
            return true;
        }
        // `x = const, ...`（形参默认值，RFC 007 M2c）：赋值表达式一等化后
        // `Ident =` 不再独占 lambda 语义——`(v = -3) + 10`、`f(x = 5)` 的
        // `v =` 是赋值表达式开头。消歧规则：形参列表的平衡 `)` 之后紧跟
        // `=>` 才判定为带默认值 lambda（与 check_async_lambda 的扫描同构）；
        // 否则回落普通表达式路径。
        if matches!(self.peek().token, Token::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
                .map(|t| matches!(t.token, Token::Eq))
                .unwrap_or(false)
            && self.parens_close_then_fatarrow(1, self.pos + 2)
        {
            return true;
        }
        // RFC 009 M4-3: 零参数 lambda `() => ...`（`Func<string>` / `Action` 等）。
        // peek 是 `)`，下一个 token 是 `FatArrow`。
        if matches!(self.peek().token, Token::RParen)
            && self
                .tokens
                .get(self.pos + 1)
                .map(|t| matches!(t.token, Token::FatArrow))
                .unwrap_or(false)
        {
            return true;
        }
        // RFC 045（closure_nested 崩溃根因）：单参带括号无类型 `(x) => ...`——
        // 括号内单 ident 且括号后跟 FatArrow（如嵌套块体 lambda 内
        // `Func<int,int> f = (x) => { ... };`）。旧实现仅识别裸 `x =>` 与
        // `() =>`，`(x) =>` 落 cast/分组解析 → `expected Semi, found FatArrow`。
        // 仅当后随 FatArrow 才识别（`(x) + 1` 等分组表达式无歧义）。
        if matches!(self.peek().token, Token::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
                .map(|t| matches!(t.token, Token::RParen))
                .unwrap_or(false)
            && self
                .tokens
                .get(self.pos + 2)
                .map(|t| matches!(t.token, Token::FatArrow))
                .unwrap_or(false)
        {
            return true;
        }
        // C# 风格带类型参数 lambda：`(int x) => ...` —— 括号内 type+ident 且后跟 FatArrow
        if matches!(self.peek().token, Token::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
                .map(|t| matches!(t.token, Token::Ident(_)))
                .unwrap_or(false)
            && self
                .tokens
                .get(self.pos + 2)
                .map(|t| matches!(t.token, Token::RParen | Token::Comma))
                .unwrap_or(false)
            && self
                .tokens
                .get(self.pos + 3)
                .map(|t| matches!(t.token, Token::FatArrow))
                .unwrap_or(false)
        {
            return true;
        }
        false
    }

    /// 自 `start` 起以给定嵌套深度扫描，找到闭合 `)` 后判定是否紧跟 `=>`。
    /// 用于 `(` 上下文内「形参默认值 lambda」与「赋值表达式」的消歧
    /// （`check_lambda` 的 `Ident =` 分支与 `check_async_lambda` 同构）。
    /// 扫描越界（无闭合）返回 false——回落普通表达式路径。
    fn parens_close_then_fatarrow(&self, mut depth: i32, start: usize) -> bool {
        let mut i = start;
        while i < self.tokens.len() {
            match &self.tokens[i].token {
                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return self
                            .tokens
                            .get(i + 1)
                            .map(|t| matches!(t.token, Token::FatArrow))
                            .unwrap_or(false);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// RFC 009 M6: 检测 `async` 前缀的异步 lambda。
    ///
    /// 形式：
    ///   - `async () => ...`       （零参数）
    ///   - `async (x) => ...`      （单/多参数）
    ///   - `async (x: T) => ...`   （带类型参数）
    ///   - `async x => ...`        （单参数无括号）
    ///
    /// 当前位置应已指向 `async` 关键字。返回 `true` 表示这是一个 async lambda。
    pub(crate) fn check_async_lambda(&self) -> bool {
        if !matches!(self.peek().token, Token::Async) {
            return false;
        }
        let next = match self.tokens.get(self.pos + 1) {
            Some(t) => t,
            None => return false,
        };
        // `async ( ...` → 可能是带括号的 async lambda（需进一步检查括号后内容）
        // `async ident =>` → 单参数 async lambda
        match &next.token {
            Token::LParen => {
                // 跳过 `async (`，扫描到 `)` 后是否紧跟 `=>`
                let mut depth: i32 = 1;
                let mut i = self.pos + 2;
                while i < self.tokens.len() {
                    match &self.tokens[i].token {
                        Token::LParen => depth += 1,
                        Token::RParen => {
                            depth -= 1;
                            if depth == 0 {
                                // 检查 `)` 之后是否是 `=>`
                                return self
                                    .tokens
                                    .get(i + 1)
                                    .map(|t| matches!(t.token, Token::FatArrow))
                                    .unwrap_or(false);
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                false
            }
            Token::Ident(name) => {
                // `async ident =>` / `async ident, ...` / `async ident: Type` / `async ident = const`
                self.tokens
                    .get(self.pos + 2)
                    .map(|t| {
                        matches!(
                            t.token,
                            Token::FatArrow | Token::Comma | Token::Colon | Token::Eq
                        )
                    })
                    .unwrap_or(false)
                    && name != "fn"
                    && name != "if"
                    && name != "while"
                    && name != "for"
                    && name != "return"
            }
            _ => false,
        }
    }

    pub(crate) fn parse_lambda(
        &mut self,
        start: Span,
        is_expression_tree: bool,
        is_async: bool,
    ) -> Result<Spanned<Expr>, ParseError> {
        // RFC 009 M4-3: 零参数 lambda `() => body`
        if matches!(self.peek().token, Token::RParen) {
            self.expect(Token::RParen)?;
            self.expect(Token::FatArrow)?;
            let body = if self.match_token(&Token::LBrace) {
                LambdaBody::Block(self.parse_block_inner()?)
            } else {
                LambdaBody::Expr(Box::new(self.parse_expr_bp(0)?))
            };
            let lambda = LambdaExpr {
                params: vec![],
                body,
                is_expression_tree,
                is_async,
                captures: vec![],
            };
            return Ok(Spanned::new(
                Expr::Lambda(lambda),
                start.merge(self.prev_span()),
            ));
        }

        let mut params = Vec::new();
        loop {
            let is_csharp_style = matches!(self.peek().token, Token::Ident(_))
                && self
                    .tokens
                    .get(self.pos + 1)
                    .map(|t| {
                        matches!(t.token, Token::Ident(_))
                            && !matches!(
                                t.token,
                                Token::Colon
                                    | Token::Comma
                                    | Token::RParen
                                    | Token::Eq
                                    | Token::Lt
                                    | Token::LBracket
                                    | Token::Question
                            )
                    })
                    .unwrap_or(false);

            if is_csharp_style {
                let ty = self.parse_type()?;
                let name = self.parse_ident()?;
                let default = if self.match_token(&Token::Eq) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                params.push(LambdaParam {
                    name,
                    ty: Some(ty),
                    default,
                });
            } else {
                let name = self.parse_ident()?;
                let ty = if self.match_token(&Token::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                let default = if self.match_token(&Token::Eq) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                params.push(LambdaParam { name, ty, default });
            }
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect(Token::RParen)?;
        self.expect(Token::FatArrow)?;
        let body = if self.match_token(&Token::LBrace) {
            LambdaBody::Block(self.parse_block_inner()?)
        } else {
            LambdaBody::Expr(Box::new(self.parse_expr_bp(0)?))
        };
        let lambda = LambdaExpr {
            params,
            body,
            is_expression_tree,
            is_async,
            captures: vec![],
        };
        if is_expression_tree {
            Ok(Spanned::new(
                Expr::ExpressionLit(ExpressionLit { lambda }),
                start.merge(self.prev_span()),
            ))
        } else {
            Ok(Spanned::new(
                Expr::Lambda(lambda),
                start.merge(self.prev_span()),
            ))
        }
    }

    pub(crate) fn parse_lambda_from_param(
        &mut self,
        start: Span,
        name: String,
        is_expression_tree: bool,
        is_async: bool,
    ) -> Result<Spanned<Expr>, ParseError> {
        // 单参数无括号 lambda `x => body` 的 body 与 `(x) => body` 同规则：
        // `{ ... }` 是语句块体（LambdaBody::Block），否则是表达式体。此前恒按
        // LambdaBody::Expr 解析，`x => { ...; return ...; }` 被误作「块表达式」
        // （值为 void）→ typeck 报 expected int, found void（block lambda P0 缺陷）。
        let body = if self.match_token(&Token::LBrace) {
            LambdaBody::Block(self.parse_block_inner()?)
        } else {
            LambdaBody::Expr(Box::new(self.parse_expr_bp(0)?))
        };
        let lambda = LambdaExpr {
            params: vec![LambdaParam {
                name: name.into(),
                ty: None,
                default: None,
            }],
            body,
            is_expression_tree,
            is_async,
            captures: vec![],
        };
        if is_expression_tree {
            Ok(Spanned::new(
                Expr::ExpressionLit(ExpressionLit { lambda }),
                start.merge(self.prev_span()),
            ))
        } else {
            Ok(Spanned::new(
                Expr::Lambda(lambda),
                start.merge(self.prev_span()),
            ))
        }
    }

    pub(crate) fn is_deprecated_expression_lambda(&self) -> bool {
        if !matches!(&self.peek().token, Token::Ident(name) if name == "expression") {
            return false;
        }
        let saved = self.pos;
        let mut p = Parser {
            tokens: self.tokens.clone(),
            pos: saved + 1,
            catch_bindings: self.catch_bindings.clone(),
        };
        if p.match_token(&Token::LParen) {
            return p.check_lambda();
        }
        matches!(p.peek().token, Token::Ident(_)) && {
            let saved2 = p.pos;
            p.advance();
            p.check(&Token::FatArrow) || {
                p.pos = saved2;
                false
            }
        }
    }

    pub(crate) fn deprecated_expression_keyword_error(&self, span: Span) -> ParseError {
        ParseError::Unexpected {
            span,
            expected:
                "`Expression<Func<...>> x = param => body` (C# expression-tree type + lambda)"
                    .into(),
            found: "deprecated `expression` keyword before lambda".into(),
        }
    }
}
