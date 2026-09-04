use super::*;

use crate::error::ParseError;
use crate::lexer::Token;
use crate::parser::Parser;

impl Parser {
    pub(crate) fn parse_query(&mut self, start: Span) -> Result<Spanned<Expr>, ParseError> {
        let mut clauses = Vec::new();
        // `from` already consumed by parse_prefix
        let ident = self.parse_ident()?;
        self.expect(Token::In)?;
        let source = self.parse_expr()?;
        clauses.push(QueryClause::From { ident, source });

        loop {
            if self.check(&Token::Select) {
                break;
            }
            match &self.peek().token {
                Token::Let => {
                    self.advance();
                    let ident = self.parse_ident()?;
                    self.expect(Token::Eq)?;
                    let value = self.parse_expr()?;
                    clauses.push(QueryClause::Let { ident, value });
                }
                Token::Where => {
                    self.advance();
                    clauses.push(QueryClause::Where(self.parse_expr()?));
                }
                Token::OrderBy => {
                    self.advance();
                    // C# `orderby k1 [descending], k2 [descending]` 多键：
                    // 逗号分隔依次生成 OrderBy 子句，MIR 折叠为单 comparator
                    // （对标 `OrderBy(...).ThenBy(...)`，见 `lower_linq.rs`）。
                    loop {
                        let key = self.parse_expr()?;
                        let descending = self.match_token(&Token::Descending);
                        clauses.push(QueryClause::OrderBy { key, descending });
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                }
                Token::Join => {
                    self.advance();
                    let ident = self.parse_ident()?;
                    self.expect(Token::In)?;
                    let source = self.parse_expr()?;
                    self.expect(Token::On)?;
                    // C# join 相等条件为 `on <left> == <right>`。用 BP 12 解析
                    // on_left/on_right——高于 `==`（BP 11）——避免 parse_expr
                    // 把整个 `p.DeptId == d.Id` 贪婪解析为比较表达式。
                    let on_left = self.parse_expr_bp(12)?;
                    self.expect(Token::EqEq)?;
                    let on_right = self.parse_expr_bp(12)?;
                    clauses.push(QueryClause::Join {
                        ident,
                        source,
                        on_left,
                        on_right,
                    });
                }
                Token::Group => {
                    self.advance();
                    // C# `group <element> by <key> [into <ident>]`。
                    let element = self.parse_expr()?;
                    self.expect(Token::By)?;
                    let key = self.parse_expr()?;
                    let into_ident = if self.match_token(&Token::Into) {
                        Some(self.parse_ident()?)
                    } else {
                        None
                    };
                    clauses.push(QueryClause::GroupBy {
                        key,
                        element: Some(element),
                        into_ident,
                    });
                }
                _ => break,
            }
        }
        self.expect(Token::Select)?;
        let select = self.parse_expr()?;
        let end = select.span;
        Ok(Spanned::new(
            Expr::Query(QueryExpr {
                clauses,
                select: Box::new(select),
            }),
            start.merge(end),
        ))
    }

    pub(crate) fn parse_if_expr(&mut self, start: Span) -> Result<Spanned<Expr>, ParseError> {
        let cond = Box::new(self.parse_expr()?);
        // C#：then 分支接受 block 或单条语句（`if (x) return;` 无需大括号）
        let then_branch = self.parse_block_or_single_stmt()?;
        let else_branch = if self.match_token(&Token::Else) {
            if self.check(&Token::If) {
                // `else if` 嵌套：`else if (cond) ...` 等价于 `else { if (cond) ... }`
                Some(Block {
                    stmts: vec![],
                    tail: Some(Box::new(self.parse_prefix()?)),
                })
            } else {
                // `else` 分支接受 block 或单条语句
                Some(self.parse_block_or_single_stmt()?)
            }
        } else {
            None
        };
        Ok(Spanned::new(
            Expr::If {
                cond,
                then_branch,
                else_branch,
            },
            start.merge(self.prev_span()),
        ))
    }

    pub(crate) fn parse_switch_expr(&mut self, start: Span) -> Result<Spanned<Expr>, ParseError> {
        self.expect(Token::LParen)?;
        let scrutinee = Box::new(self.parse_expr()?);
        self.expect(Token::RParen)?;
        self.expect(Token::LBrace)?;
        let mut cases = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let is_default = self.match_token(&Token::Default);
            let (pattern, when) = if is_default {
                (None, None)
            } else {
                self.expect(Token::Case)?;
                let pattern = Some(self.parse_pattern()?);
                let when = if self.match_token(&Token::When) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                (pattern, when)
            };
            self.expect(Token::Colon)?;
            let body = self.parse_switch_case_body()?;
            cases.push(SwitchCase {
                pattern,
                when,
                body,
            });
        }
        self.expect(Token::RBrace)?;
        Ok(Spanned::new(
            Expr::Switch(SwitchExpr { scrutinee, cases }),
            start.merge(self.prev_span()),
        ))
    }

    fn parse_switch_case_body(&mut self) -> Result<Block, ParseError> {
        // AGENTS.md §5：switch 的每个 case/default 分支体可用 `{}` 括起（Allman 风格，
        // 禁止裸语句列表）。此时块内语句由块自身界定，不依赖 break/case/default 终止。
        // 块内 `return`/`break`/`throw` 等控制流由语义层校验，parser 仅负责结构。
        if self.check(&Token::LBrace) {
            return Ok(self.parse_block()?.node);
        }
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace)
            && !self.is_at_end()
            && !self.check(&Token::Case)
            && !self.check(&Token::Default)
        {
            if self.check(&Token::Break) {
                self.advance();
                self.expect(Token::Semi)?;
                break;
            }
            stmts.push(self.parse_stmt()?);
        }
        Ok(Block { stmts, tail: None })
    }

    /// RFC 036 M2 + RFC 004 M3：switch `case` 模式。
    ///
    /// 支持：`_` / `null` / `var name` / 字面量 / `Type.Case(binding)` /
    /// `T` / `T name`（类型模式）/ `(var x, var y)` 位置模式。
    pub(crate) fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        // RFC 004 M7+：属性模式立宪硬拒绝（对齐 RFC 036 §4.2）
        if self.check(&Token::LBrace) {
            return Err(ParseError::Unexpected {
                span: self.current_span(),
                expected: "pattern (property patterns `{ Prop: … }` are rejected; use `when` + property access, RFC 004 M7+)".into(),
                found: self.describe_current(),
            });
        }
        if self.check(&Token::LParen) {
            return Ok(Pattern::Positional(self.parse_positional_subpatterns()?));
        }
        if self.check(&Token::Null) {
            self.advance();
            return Ok(Pattern::Null);
        }
        if self.check(&Token::Var) {
            self.advance();
            let name = self.parse_ident()?;
            return Ok(Pattern::Var(name));
        }
        if matches!(
            &self.peek().token,
            Token::IntLit(_)
                | Token::True
                | Token::False
                | Token::StringLit(_)
                | Token::VerbatimString(_)
                | Token::CharLit(_)
        ) {
            return Ok(Pattern::Literal(self.parse_expr()?));
        }
        if matches!(&self.peek().token, Token::Ident(_)) {
            let name = self.parse_ident()?.to_string();
            if name == "_" {
                return Ok(Pattern::Wildcard);
            }
            // RFC 004：`Type.Case` / `Type.Case(binding)` / `Type.Case(var binding)`
            // RFC 004 M2：`Option<int>.Some(n)` — 复用表达式级 `<…>` 消歧（`>` 后为 `.`）
            if self.is_generic_call_start() {
                self.advance(); // `<`
                let mut type_args = vec![self.parse_type()?];
                while self.match_token(&Token::Comma) {
                    type_args.push(self.parse_type()?);
                }
                self.expect_gt_close()?;
                self.expect(Token::Dot)?;
                let case = self.parse_ident()?;
                let binding = if self.match_token(&Token::LParen) {
                    if self.check(&Token::Var) {
                        self.advance();
                    }
                    let b = self.parse_ident()?;
                    self.expect(Token::RParen)?;
                    Some(b)
                } else {
                    None
                };
                return Ok(Pattern::Variant {
                    path: vec![name.into()],
                    type_args,
                    case,
                    binding,
                });
            }
            if self.check(&Token::Dot) {
                self.advance();
                let case = self.parse_ident()?;
                let binding = if self.match_token(&Token::LParen) {
                    // 支持 `var r` 显式 var 模式与裸 `r` 标识符两种形式
                    if self.check(&Token::Var) {
                        self.advance();
                    }
                    let b = self.parse_ident()?;
                    self.expect(Token::RParen)?;
                    Some(b)
                } else {
                    None
                };
                return Ok(Pattern::Variant {
                    path: vec![name.into()],
                    type_args: vec![],
                    case,
                    binding,
                });
            }
            // `T name` 类型声明模式（绑定名后须为 `when` / `:` / `=>` / `,`）
            if matches!(&self.peek().token, Token::Ident(_)) {
                let binding = self.parse_ident()?;
                return Ok(Pattern::Type {
                    ty: Type::named(name),
                    binding: Some(binding),
                });
            }
            // 裸标识：enum 变体或无绑定类型名（typeck 区分）
            return Ok(Pattern::Ident(name.into()));
        }
        // 泛型等复杂类型：`List<int> xs` / `List<int>`
        let ty = self.parse_type()?;
        let binding = if matches!(&self.peek().token, Token::Ident(_)) {
            Some(self.parse_ident()?)
        } else {
            None
        };
        Ok(Pattern::Type { ty, binding })
    }

    /// RFC 036 M1 + RFC 004 M3 + C# 9 逻辑组合：解析 `is` 表达式的模式部分。
    ///
    /// 支持形式（组合优先级：`or` < `and` < `not` < primary；`(…)` 可显式分组）：
    /// - `is null`                  → IsPattern::Null
    /// - `is var name`              → IsPattern::Var(name)
    /// - `is <literal>`             → IsPattern::Constant(lit)（RFC 004 常量模式）
    /// - `is T`                     → IsPattern::Type { ty: T, binding: None }
    /// - `is T name`                → IsPattern::Type { ty: T, binding: Some(name) }
    /// - `is (var x, var y)` / `is (_, _)` → IsPattern::Positional（RFC 004 M3）
    /// - `is A and B` / `A or B` / `not A` → IsPattern::And/Or/Not（C# 9 逻辑组合）
    /// - `is (A or B) and C`        → 括号分组调整优先级
    ///
    /// `T` 为类型（parse_type 解析）；`name` 为标识符（声明绑定）。
    pub(crate) fn parse_is_pattern(&mut self) -> Result<IsPattern, ParseError> {
        self.parse_is_or()
    }

    /// 最低优先级：`or`。`A or B`（左结合）。
    fn parse_is_or(&mut self) -> Result<IsPattern, ParseError> {
        let mut left = self.parse_is_and()?;
        while self.peek_ident_is("or") {
            let span = self.peek().span;
            self.advance();
            let right = self.parse_is_and()?;
            left = IsPattern::Or {
                left: Box::new(Spanned::new(left, span)),
                right: Box::new(Spanned::new(right, span)),
            };
        }
        Ok(left)
    }

    /// 中优先级：`and`。`A and B`（左结合）。
    fn parse_is_and(&mut self) -> Result<IsPattern, ParseError> {
        let mut left = self.parse_is_not()?;
        while self.peek_ident_is("and") {
            let span = self.peek().span;
            self.advance();
            let right = self.parse_is_not()?;
            left = IsPattern::And {
                left: Box::new(Spanned::new(left, span)),
                right: Box::new(Spanned::new(right, span)),
            };
        }
        Ok(left)
    }

    /// 前缀 `not`：`not A`（可嵌套 `not not A`）。
    fn parse_is_not(&mut self) -> Result<IsPattern, ParseError> {
        if self.peek_ident_is("not") {
            let span = self.peek().span;
            self.advance();
            let inner = self.parse_is_not()?;
            return Ok(IsPattern::Not {
                inner: Box::new(Spanned::new(inner, span)),
            });
        }
        self.parse_is_primary()
    }

    /// primary 模式：括号（分组 / 位置）、null、var、类型模式。
    fn parse_is_primary(&mut self) -> Result<IsPattern, ParseError> {
        // RFC 004 M7+：属性模式立宪硬拒绝（对齐 RFC 036 §4.2）
        if self.check(&Token::LBrace) {
            return Err(ParseError::Unexpected {
                span: self.current_span(),
                expected: "is-pattern (property patterns `{ Prop: … }` are rejected; use `when` + property access, RFC 004 M7+)".into(),
                found: self.describe_current(),
            });
        }
        // 括号：位置模式 `(var x, var y)`（RFC 004 M3）或分组 `(A or B)`（C# 9）。
        if self.check(&Token::LParen) {
            if self.paren_is_positional() {
                return Ok(IsPattern::Positional(self.parse_positional_subpatterns()?));
            }
            // 分组模式：`( pat )` —— 解析完整组合后闭合括号。
            self.advance();
            let inner = self.parse_is_or()?;
            self.expect(Token::RParen)?;
            return Ok(inner);
        }
        // `is null` — null 模式
        if self.check(&Token::Null) {
            self.advance();
            return Ok(IsPattern::Null);
        }
        // `is var name` — var 模式（永远匹配 + 绑定到原类型）
        if self.check(&Token::Var) {
            self.advance();
            let name = self.parse_ident()?;
            return Ok(IsPattern::Var(name));
        }
        // RFC 004：`is <literal>` — 常量模式（int / bool / string / char）。
        // 值相等语义由 typeck + MIR 处理；此处仅按字面量解析为 Constant。
        if matches!(
            &self.peek().token,
            Token::IntLit(_)
                | Token::True
                | Token::False
                | Token::StringLit(_)
                | Token::VerbatimString(_)
                | Token::CharLit(_)
        ) {
            let lit = self.parse_expr()?;
            return Ok(IsPattern::Constant(Box::new(lit)));
        }
        // `is T` / `is T name` — 类型模式 + 可选声明绑定
        let ty = self.parse_type()?;
        // 若类型后紧跟标识符且非逻辑组合关键字，则视为声明绑定（`is string s`）。
        // `and`/`or`/`not` 为模式上下文关键字，此处不当作绑定名。
        if matches!(&self.peek().token, Token::Ident(_))
            && !self.peek_ident_is("and")
            && !self.peek_ident_is("or")
            && !self.peek_ident_is("not")
        {
            let binding = self.parse_ident()?;
            return Ok(IsPattern::Type {
                ty,
                binding: Some(binding),
            });
        }
        Ok(IsPattern::Type { ty, binding: None })
    }

    /// 当前 token 是否为指定名称的标识符（`and`/`or`/`not` 为模式上下文关键字）。
    fn peek_ident_is(&self, name: &str) -> bool {
        matches!(&self.peek().token, Token::Ident(s) if s == name)
    }

    /// 判断 `(` 之后到匹配 `)` 之间是否存在顶层逗号——有则为位置模式，否则为分组。
    fn paren_is_positional(&self) -> bool {
        let mut depth = 0usize;
        for t in &self.tokens[self.pos..] {
            match &t.token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return false; // 到达匹配右括号，其间无顶层逗号 → 分组
                    }
                }
                Token::Comma if depth == 1 => return true,
                _ => {}
            }
        }
        false
    }

    /// RFC 004 M3：解析 `(var x, var y)` / `(_, _)` 位置子模式列表（含外层括号）。
    fn parse_positional_subpatterns(&mut self) -> Result<Vec<PositionalSubpattern>, ParseError> {
        self.expect(Token::LParen)?;
        let mut elems = Vec::new();
        loop {
            elems.push(self.parse_positional_subpattern()?);
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect(Token::RParen)?;
        if elems.len() < 2 {
            return Err(ParseError::Unexpected {
                span: self.current_span(),
                expected: "at least two positional subpatterns".into(),
                found: self.describe_current(),
            });
        }
        Ok(elems)
    }

    fn parse_positional_subpattern(&mut self) -> Result<PositionalSubpattern, ParseError> {
        if self.check(&Token::Var) {
            self.advance();
            let name = self.parse_ident()?;
            return Ok(PositionalSubpattern::Var(name));
        }
        // RFC 004 M6：嵌套位置模式
        if self.check(&Token::LParen) {
            return Ok(PositionalSubpattern::Nested(
                self.parse_positional_subpatterns()?,
            ));
        }
        // RFC 004 M6：常量子模式（字面量）
        if let Some(lit) = self.try_parse_positional_const_literal()? {
            return Ok(PositionalSubpattern::Const(lit));
        }
        if matches!(&self.peek().token, Token::Ident(_)) {
            let first = self.parse_ident()?;
            if first.as_str() == "_" {
                return Ok(PositionalSubpattern::Discard);
            }
            // `T name` 类型子模式（绑定名须为 Ident；其后应为 `,` / `)`）
            if matches!(&self.peek().token, Token::Ident(_)) {
                let name = self.parse_ident()?;
                return Ok(PositionalSubpattern::Typed {
                    ty: Type::named(first.to_string()),
                    name,
                });
            }
            return Err(ParseError::Unexpected {
                span: self.current_span(),
                expected: "`var name`, `_`, `Type name`, constant, or nested `(…)` in positional pattern (RFC 004 M6)".into(),
                found: first.to_string(),
            });
        }
        let ty = self.parse_type()?;
        if matches!(&self.peek().token, Token::Ident(_)) {
            let name = self.parse_ident()?;
            return Ok(PositionalSubpattern::Typed { ty, name });
        }
        Err(ParseError::Unexpected {
            span: self.current_span(),
            expected: "`var name`, `_`, `Type name`, constant, or nested `(…)` in positional pattern (RFC 004 M6)".into(),
            found: self.describe_current(),
        })
    }

    /// RFC 004 M6：位置模式常量子模式字面量（int / bool / string / char / null）。
    fn try_parse_positional_const_literal(&mut self) -> Result<Option<Spanned<Expr>>, ParseError> {
        let span = self.current_span();
        let expr = match &self.peek().token {
            Token::IntLit(n) => {
                let n = *n;
                self.advance();
                Expr::IntLit(n)
            }
            Token::True => {
                self.advance();
                Expr::BoolLit(true)
            }
            Token::False => {
                self.advance();
                Expr::BoolLit(false)
            }
            Token::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Expr::StringLit(s)
            }
            Token::VerbatimString(s) => {
                let s = s.clone();
                self.advance();
                Expr::StringLit(s)
            }
            Token::CharLit(c) => {
                let c = *c;
                self.advance();
                Expr::CharLit(c)
            }
            Token::Null => {
                self.advance();
                Expr::Null
            }
            _ => return Ok(None),
        };
        Ok(Some(Spanned::new(expr, span.merge(self.prev_span()))))
    }
}
