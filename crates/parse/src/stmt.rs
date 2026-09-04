use crate::lexer::Token;
use ast::*;

use crate::error::ParseError;
use crate::parser::Parser;

impl Parser {
    /// RFC 044：当前 token 是否为 `yield` 且下一 token 是 `return`/`break`
    ///（上下文敏感关键字判定的 follow set）。
    pub(crate) fn peek1_is_yield_follow(&self) -> bool {
        matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::Return) | Some(Token::Break)
        )
    }

    pub(crate) fn parse_block(&mut self) -> Result<Spanned<Block>, ParseError> {
        let start = self.current_span();
        self.expect(Token::LBrace)?;
        let block = self.parse_block_inner()?;
        let end = self.prev_span();
        Ok(Spanned::new(block, start.merge(end)))
    }

    /// Parse either a brace-delimited block or a single statement as a block.
    ///
    /// C# supports single-statement bodies for `if`/`while`/`for`/`foreach`:
    ///   `if (cond) stmt;`  →  `if (cond) { stmt; }`
    /// Without this, every control-flow construct would require braces, which is
    /// unnecessarily verbose for simple one-liners like `if (x) return;`.
    pub(crate) fn parse_block_or_single_stmt(&mut self) -> Result<Block, ParseError> {
        if self.check(&Token::LBrace) {
            return Ok(self.parse_block()?.node);
        }
        let stmt = self.parse_stmt()?;
        Ok(Block {
            stmts: vec![stmt],
            tail: None,
        })
    }

    pub(crate) fn parse_block_inner(&mut self) -> Result<Block, ParseError> {
        let mut stmts = Vec::new();
        let tail = None;
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        Ok(Block { stmts, tail })
    }

    pub(crate) fn parse_stmt(&mut self) -> Result<Spanned<Stmt>, ParseError> {
        let start = self.current_span();
        // RFC 004 M1/M2：`(x, y) = expr;` / `(x, _) = expr;`
        if let Some(stmt) = self.try_parse_deconstruct_assign(start)? {
            let end = self.prev_span();
            return Ok(Spanned::new(stmt, start.merge(end)));
        }
        let stmt = match &self.peek().token {
            // RFC 044：上下文敏感关键字 `yield` —— 仅语句位置且后随
            // `return` / `break` 时识别为 yield 语句；其余位置（表达式等）
            // 仍作普通标识符，不破坏既有兼容。
            Token::Ident(name) if name == "yield" && self.peek1_is_yield_follow() => {
                self.advance(); // consume `yield`
                if self.match_token(&Token::Return) {
                    let value = self.parse_expr()?;
                    self.expect(Token::Semi)?;
                    Stmt::YieldReturn { value }
                } else {
                    self.expect(Token::Break)?;
                    self.expect(Token::Semi)?;
                    Stmt::YieldBreak
                }
            }
            Token::Var => {
                self.expect(Token::Var)?;
                // RFC 004 M2：`var (x, y) = expr;` 声明式解构
                if self.check(&Token::LParen) {
                    if let Some(stmt) = self.try_parse_var_deconstruct(start)? {
                        let end = self.prev_span();
                        return Ok(Spanned::new(stmt, start.merge(end)));
                    }
                    return Err(self.error(
                        "`var (x, y) = expr` deconstruction",
                        "expected `var (ident_or_, ident_or_, …) = expr;` with ≥2 targets".into(),
                    ));
                }
                let name = self.parse_ident()?;
                if self.match_token(&Token::Colon) {
                    return Err(self.error(
                        "`var` uses right-side inference",
                        "use a leading-type declaration (`int x = 1`) instead of `var x: Type`"
                            .into(),
                    ));
                }
                self.expect(Token::Eq)?;
                if self.check(&Token::LBrace) && !self.is_block_start_after_lbrace() {
                    return Err(self.bare_brace_initializer_error());
                }
                let init = Some(self.parse_expr()?);
                self.expect(Token::Semi)?;
                Stmt::Let {
                    mutable: false,
                    name,
                    ty: None,
                    init,
                }
            }
            Token::Let => {
                return Err(self.error(
                    "`var` or leading-type declaration",
                    "found `let` — use `var` or a leading-type declaration (`int x = 1`)".into(),
                ));
            }
            // RFC 012：comptime 有限子集——编译期常量求值前缀。
            // 语法：`comptime <leading-type-decl>`（如 `comptime int x = 1 + 2;`）。
            // 仅解析为 Stmt::Let，其 init 表达式包裹为 `Expr::Comptime`，由 typeck
            // 在编译期折叠为常量（int/bool/string 字面量运算）。
            Token::Comptime => {
                self.advance();
                let ty = self.parse_type()?;
                let name = self.parse_ident()?;
                self.expect(Token::Eq)?;
                let inner = self.parse_expr()?;
                let span = inner.span;
                let init = Spanned::new(Expr::Comptime(Box::new(inner)), span);
                self.expect(Token::Semi)?;
                Stmt::Let {
                    mutable: false,
                    name,
                    ty: Some(ty),
                    init: Some(init),
                }
            }
            Token::Ident(name) if name == "match" => {
                return Err(self.error(
                    "`switch`",
                    "found `match` — use C# `switch (expr) { case ...: break; default: break; }`"
                        .into(),
                ));
            }
            Token::Return => {
                self.advance();
                let val = if self.check(&Token::Semi) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(Token::Semi)?;
                Stmt::Return(val)
            }
            Token::While => {
                self.advance();
                let cond = self.parse_expr()?;
                // C#：while body 接受 block 或单条语句
                let body = self.parse_block_or_single_stmt()?;
                Stmt::While { cond, body }
            }
            Token::If => {
                self.advance();
                let if_expr = self.parse_if_expr(start)?;
                Stmt::Expr(if_expr)
            }
            Token::Switch => {
                self.advance();
                let switch_expr = self.parse_switch_expr(start)?;
                Stmt::Expr(switch_expr)
            }
            Token::Break => {
                self.advance();
                self.expect(Token::Semi)?;
                Stmt::Break
            }
            Token::Continue => {
                self.advance();
                self.expect(Token::Semi)?;
                Stmt::Continue
            }
            Token::Throw => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::Semi)?;
                Stmt::Throw { expr }
            }
            Token::Try => {
                self.advance();
                let try_body = self.parse_block()?.node;
                // RFC 009 P1-B2：`try { } catch (...) [when (...)] { } [finally { }]`
                // 或 `try { } finally { }`。
                if self.match_token(&Token::Finally) {
                    let finally = self.parse_block()?.node;
                    Stmt::TryFinally {
                        body: try_body,
                        finally,
                    }
                } else {
                    self.expect(Token::Catch)?;
                    // C# catch 子句支持多种形式：
                    //   `catch { }` — catch-all（无类型/变量）
                    //   `catch when (cond) { }` — catch-all + when 过滤
                    //   `catch (Type) { }` — 类型过滤，无变量
                    //   `catch (Type name) { }` — 类型过滤 + 变量
                    //   `catch (Type name) when (cond) { }` — 完整形式
                    //
                    // 对无类型 catch-all（`catch { }` / `catch when`），
                    // 合成 `Exception` 类型 + `__catch_all` 占位名。
                    // codegen 的零开销 EH（Windows SEH）对 `Exception` 视为
                    // catch-all，不按类型过滤；合成类型用于 typeck 作用域绑定。
                    let (catch_ty, catch_name) = if self.check(&Token::LParen) {
                        self.expect(Token::LParen)?;
                        let catch_ty = self.parse_type()?;
                        // `catch (Type)` 无变量形式：若 `)` 前不是标识符则省略变量名
                        let catch_name = if self.check(&Token::RParen) {
                            Ident::from("__catch_unnamed")
                        } else {
                            self.parse_ident()?
                        };
                        self.expect(Token::RParen)?;
                        (catch_ty, catch_name)
                    } else {
                        // catch-all：合成 Exception 类型
                        let syn_ty = Type::Named {
                            path: vec![Ident::from("Exception")],
                            generics: vec![],
                        };
                        let span = self.current_span();
                        (Spanned::new(syn_ty, span), Ident::from("__catch_all"))
                    };
                    let when_cond = if self.match_token(&Token::When) {
                        // C#：`when (cond)` 括号可选；M1 要求括号以消歧。
                        if self.match_token(&Token::LParen) {
                            let c = self.parse_expr()?;
                            self.expect(Token::RParen)?;
                            Some(c)
                        } else {
                            Some(self.parse_expr()?)
                        }
                    } else {
                        None
                    };
                    self.catch_bindings.push(catch_name.clone());
                    let catch_body = self.parse_block()?.node;
                    self.catch_bindings.pop();
                    let finally = if self.match_token(&Token::Finally) {
                        Some(self.parse_block()?.node)
                    } else {
                        None
                    };
                    Stmt::TryCatch {
                        try_body,
                        catch_ty,
                        catch_name,
                        when_cond,
                        catch_body,
                        finally,
                    }
                }
            }
            Token::Lock => {
                // RFC 009 §7.3：`lock (expr) { body }`
                self.advance();
                self.expect(Token::LParen)?;
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                let body = self.parse_block()?.node;
                Stmt::Lock { expr, body }
            }
            Token::Using => {
                self.advance();
                if self.check(&Token::LParen) {
                    self.expect(Token::LParen)?;
                    // 支持两种形式：`using (Type name = expr)` 和 `using (var name = expr)`
                    let (name, ty) = if self.match_token(&Token::Var) {
                        let n = self.parse_ident()?;
                        (n, None)
                    } else {
                        let t = self.parse_type()?;
                        let n = self.parse_ident()?;
                        (n, Some(t))
                    };
                    self.expect(Token::Eq)?;
                    let init = self.parse_expr()?;
                    self.expect(Token::RParen)?;
                    let body = self.parse_block()?.node;
                    Stmt::Using {
                        name,
                        ty,
                        init,
                        body,
                    }
                } else if self.match_token(&Token::Var) {
                    // RFC 010：`using var name = expr;`
                    let name = self.parse_ident()?;
                    self.expect(Token::Eq)?;
                    let init = self.parse_expr()?;
                    self.expect(Token::Semi)?;
                    Stmt::UsingVar {
                        name,
                        ty: None,
                        init,
                    }
                } else {
                    // RFC 010：`using Type name = expr;`
                    let ty = self.parse_type()?;
                    let name = self.parse_ident()?;
                    self.expect(Token::Eq)?;
                    let init = self.parse_expr()?;
                    self.expect(Token::Semi)?;
                    Stmt::UsingVar {
                        name,
                        ty: Some(ty),
                        init,
                    }
                }
            }
            Token::For => {
                self.advance();
                // C-style `for (init; cond; inc)` vs `for VAR in EXPR`
                if self.check(&Token::LParen) {
                    self.advance(); // consume (
                                    // init clause (optional)
                    let init = if self.check(&Token::Semi) {
                        self.advance(); // consume ;
                        None
                    } else {
                        let s = self.parse_stmt()?;
                        Some(s)
                    };
                    // cond clause (optional)
                    let cond = if self.check(&Token::Semi) {
                        self.advance(); // consume ;
                        None
                    } else {
                        let e = self.parse_expr()?;
                        self.expect(Token::Semi)?;
                        Some(e)
                    };
                    // inc clause (optional) — parsed as expression-or-assignment
                    // because there's no trailing `;` before `)` in C-style for.
                    // `i++` / `i--` 保持语句级脱糖；`=`/复合赋值由 Pratt 层
                    // 一等化后 parse_expr 直接返回 `Expr::Assign`，在此提取。
                    let inc = if self.check(&Token::RParen) {
                        None
                    } else {
                        let inc_expr = self.parse_expr()?;
                        let inc_stmt = if self.match_token(&Token::PlusPlus) {
                            let span = inc_expr.span;
                            let one = Spanned::new(Expr::IntLit(1), span);
                            let value = Spanned::new(
                                Expr::Binary {
                                    op: BinOp::Add,
                                    left: Box::new(inc_expr.clone()),
                                    right: Box::new(one),
                                },
                                span,
                            );
                            Spanned::new(
                                Box::new(Stmt::Assign {
                                    target: inc_expr,
                                    value,
                                }),
                                span,
                            )
                        } else if self.match_token(&Token::MinusMinus) {
                            let span = inc_expr.span;
                            let one = Spanned::new(Expr::IntLit(1), span);
                            let value = Spanned::new(
                                Expr::Binary {
                                    op: BinOp::Sub,
                                    left: Box::new(inc_expr.clone()),
                                    right: Box::new(one),
                                },
                                span,
                            );
                            Spanned::new(
                                Box::new(Stmt::Assign {
                                    target: inc_expr,
                                    value,
                                }),
                                span,
                            )
                        } else {
                            match inc_expr.node {
                                Expr::Assign { target, value } => Spanned::new(
                                    Box::new(Stmt::Assign {
                                        target: *target,
                                        value: *value,
                                    }),
                                    inc_expr.span,
                                ),
                                other => Spanned::new(
                                    Box::new(Stmt::Expr(Spanned::new(other, inc_expr.span))),
                                    inc_expr.span,
                                ),
                            }
                        };
                        Some(inc_stmt)
                    };
                    self.expect(Token::RParen)?;
                    let body = self.parse_block_or_single_stmt()?;
                    Stmt::ForC {
                        init: init.map(|s| s.map(Box::new)),
                        cond,
                        inc,
                        body,
                    }
                } else {
                    let var = self.parse_ident()?;
                    self.expect(Token::In)?;
                    let iter = self.parse_expr()?;
                    let body = self.parse_block_or_single_stmt()?;
                    Stmt::For { var, iter, body }
                }
            }
            Token::Foreach => {
                self.advance();
                self.expect(Token::LParen)?;
                self.match_token(&Token::Var);
                let var = self.parse_ident()?;
                self.expect(Token::In)?;
                let iter = self.parse_foreach_iter()?;
                self.expect(Token::RParen)?;
                let body = self.parse_block_or_single_stmt()?;
                Stmt::For { var, iter, body }
            }
            Token::Await => {
                self.advance();
                // `await using ...` → parse as async resource management statement.
                // Otherwise fall through to expression (`await expr`).
                if self.check(&Token::Using) {
                    self.advance(); // consume `using`
                    if self.check(&Token::LParen) {
                        // ── Block-scoped: `await using (Type name = init) { body }` ──
                        self.expect(Token::LParen)?;
                        let (name, ty) = if self.match_token(&Token::Var) {
                            let n = self.parse_ident()?;
                            (n, None)
                        } else {
                            let t = self.parse_type()?;
                            let n = self.parse_ident()?;
                            (n, Some(t))
                        };
                        self.expect(Token::Eq)?;
                        let init = self.parse_expr()?;
                        self.expect(Token::RParen)?;
                        let body = self.parse_block()?.node;
                        Stmt::AwaitUsing {
                            name,
                            ty,
                            init,
                            body,
                        }
                    } else if self.match_token(&Token::Var) {
                        // ── Declaration-level: `await using var name = init;` ──
                        let name = self.parse_ident()?;
                        self.expect(Token::Eq)?;
                        let init = self.parse_expr()?;
                        self.expect(Token::Semi)?;
                        Stmt::AwaitUsingVar {
                            name,
                            ty: None,
                            init,
                        }
                    } else {
                        // ── Declaration-level: `await using Type name = init;` ──
                        let ty = self.parse_type()?;
                        let name = self.parse_ident()?;
                        self.expect(Token::Eq)?;
                        let init = self.parse_expr()?;
                        self.expect(Token::Semi)?;
                        Stmt::AwaitUsingVar {
                            name,
                            ty: Some(ty),
                            init,
                        }
                    }
                } else {
                    // Not `await using` → parse as `await expr`.
                    let inner = self.parse_expr_bp(0)?;
                    let span = inner.span;
                    let expr = Spanned::new(Expr::Await(Box::new(inner)), start.merge(span));
                    self.expect(Token::Semi)?;
                    Stmt::Expr(expr)
                }
            }
            _ if self.is_type_start() && self.looks_like_leading_type_decl() => {
                let ty = self.parse_type()?;
                let name = self.parse_ident()?;
                let init = if self.match_token(&Token::Eq) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(Token::Semi)?;
                Stmt::Let {
                    mutable: false,
                    name,
                    ty: Some(ty),
                    init,
                }
            }
            _ => {
                let expr = self.parse_expr()?;
                // `++`/`--` 保持语句级脱糖（表达式级 inc/dec 未引入；Pratt 层
                // 已一等化 `=`/复合赋值/`??=`，parse_expr 返回的即是
                // `Expr::Assign`——语句位置提取为 `Stmt::Assign`，下游全链路
                // （typeck/mir）零回归；其余表达式落回 `Stmt::Expr`。
                if self.match_token(&Token::PlusPlus) {
                    self.expect(Token::Semi)?;
                    let one = Spanned::new(Expr::IntLit(1), expr.span);
                    let value = Spanned::new(
                        Expr::Binary {
                            op: BinOp::Add,
                            left: Box::new(expr.clone()),
                            right: Box::new(one),
                        },
                        expr.span,
                    );
                    Stmt::Assign {
                        target: expr,
                        value,
                    }
                } else if self.match_token(&Token::MinusMinus) {
                    self.expect(Token::Semi)?;
                    let one = Spanned::new(Expr::IntLit(1), expr.span);
                    let value = Spanned::new(
                        Expr::Binary {
                            op: BinOp::Sub,
                            left: Box::new(expr.clone()),
                            right: Box::new(one),
                        },
                        expr.span,
                    );
                    Stmt::Assign {
                        target: expr,
                        value,
                    }
                } else {
                    self.expect(Token::Semi)?;
                    match expr.node {
                        Expr::Assign { target, value } => Stmt::Assign {
                            target: *target,
                            value: *value,
                        },
                        other => Stmt::Expr(Spanned::new(other, expr.span)),
                    }
                }
            }
        };
        let end = self.prev_span();
        Ok(Spanned::new(stmt, start.merge(end)))
    }

    /// RFC 004 M1/M2/M7: try parse `(ident|_|nested, ...) = expr;`.
    ///
    /// - all plain idents (no discard/nested) -> MethodCall (M1)
    /// - discard `_` or nested `(...)` -> Stmt::DeconstructAssign
    fn try_parse_deconstruct_assign(&mut self, start: Span) -> Result<Option<Stmt>, ParseError> {
        if !self.check(&Token::LParen) {
            return Ok(None);
        }
        let save = self.pos;
        self.advance(); // (

        let Some(targets) = self.try_parse_deconstruct_targets()? else {
            self.pos = save;
            return Ok(None);
        };

        if targets.len() < 2 || !self.match_token(&Token::RParen) || !self.match_token(&Token::Eq) {
            self.pos = save;
            return Ok(None);
        }

        let value = self.parse_expr()?;
        self.expect(Token::Semi)?;

        let needs_typeck = targets
            .iter()
            .any(|(t, _)| t.has_discard() || t.is_nested());
        if needs_typeck {
            return Ok(Some(Stmt::DeconstructAssign {
                declare: false,
                targets: targets.into_iter().map(|(t, _)| t).collect(),
                value,
            }));
        }

        let args: Vec<Spanned<Expr>> = targets
            .into_iter()
            .map(|(target, span)| {
                let DeconstructTarget::Bind(Some(name)) = target else {
                    unreachable!("flat non-discard path");
                };
                let ident = Spanned::new(Expr::Ident(name), span);
                Spanned::new(
                    Expr::RefArg {
                        is_out: true,
                        expr: Box::new(ident),
                    },
                    span,
                )
            })
            .collect();

        let call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(value),
                method: "Deconstruct".into(),
                args,
                type_args: Vec::new(),
                params_span: None,
            },
            start.merge(self.prev_span()),
        );
        Ok(Some(Stmt::Expr(call)))
    }

    /// RFC 004 M2/M7: after `var`, try `(ident|_|nested, ...) = expr;`.
    fn try_parse_var_deconstruct(&mut self, _start: Span) -> Result<Option<Stmt>, ParseError> {
        let save = self.pos;
        self.advance(); // (

        let Some(targets) = self.try_parse_deconstruct_targets()? else {
            self.pos = save;
            return Ok(None);
        };

        if targets.len() < 2 || !self.match_token(&Token::RParen) || !self.match_token(&Token::Eq) {
            self.pos = save;
            return Ok(None);
        }

        let value = self.parse_expr()?;
        self.expect(Token::Semi)?;

        Ok(Some(Stmt::DeconstructAssign {
            declare: true,
            targets: targets.into_iter().map(|(t, _)| t).collect(),
            value,
        }))
    }

    /// Parse deconstruct targets: `ident` / `_` / `(...)`, without outer parens.
    fn try_parse_deconstruct_targets(
        &mut self,
    ) -> Result<Option<Vec<(DeconstructTarget, Span)>>, ParseError> {
        let mut targets: Vec<(DeconstructTarget, Span)> = Vec::new();
        loop {
            let Some(t) = self.try_parse_deconstruct_target()? else {
                return Ok(None);
            };
            targets.push(t);
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(Some(targets))
    }

    /// One deconstruct target: `ident` / `_` / `(t0, t1, ...)`.
    fn try_parse_deconstruct_target(
        &mut self,
    ) -> Result<Option<(DeconstructTarget, Span)>, ParseError> {
        let start = self.current_span();
        if self.check(&Token::LParen) {
            self.advance(); // (
            let Some(inner) = self.try_parse_deconstruct_targets()? else {
                return Ok(None);
            };
            if inner.len() < 2 || !self.match_token(&Token::RParen) {
                return Ok(None);
            }
            let span = start.merge(self.prev_span());
            let nested: Vec<DeconstructTarget> = inner.into_iter().map(|(t, _)| t).collect();
            return Ok(Some((DeconstructTarget::Nested(nested), span)));
        }
        let Token::Ident(s) = &self.peek().token else {
            return Ok(None);
        };
        let target = if s.as_str() == "_" {
            DeconstructTarget::Bind(None)
        } else {
            DeconstructTarget::Bind(Some(s.clone().into()))
        };
        self.advance();
        Ok(Some((target, start)))
    }

    pub(crate) fn parse_foreach_iter(&mut self) -> Result<Spanned<Expr>, ParseError> {
        if self.check(&Token::LParen) {
            self.advance();
            let expr = self.parse_expr()?;
            self.expect(Token::RParen)?;
            return Ok(expr);
        }
        if self.check(&Token::From) {
            return Err(self.error(
                "parenthesized LINQ query",
                "wrap `from ... select ...` in parentheses in foreach, e.g. `foreach (var x in (from u in xs select u.Name))`".into(),
            ));
        }
        if matches!(self.peek().token, Token::Ident(_)) {
            let expr = self.parse_expr()?;
            if matches!(expr.node, Expr::Query(_)) {
                return Err(self.error(
                    "parenthesized LINQ query",
                    "wrap `from ... select ...` in parentheses in foreach".into(),
                ));
            }
            return Ok(expr);
        }
        self.parse_expr()
    }
}
