//! LINQ query desugar: query comprehensions → method chains.

use ast::*;

/// Desugar all query expressions in a program.
/// Returns collected errors (LINQ `let` clause, etc.) instead of panicking.
pub fn desugar_program(program: &mut Program) -> Vec<String> {
    let mut errors = Vec::new();
    for item in &mut program.items {
        desugar_item(&mut item.node, &mut errors);
    }
    errors
}

/// Desugar query comprehension to method chain expression.
/// Returns an error for unsupported clauses (e.g. `let`) instead of panicking.
fn desugar_query_impl(query: &QueryExpr, errors: &mut Vec<String>) -> Option<Spanned<Expr>> {
    let span = query.select.span;
    let mut from_ident: Option<Ident> = None;
    let mut expr = Spanned::new(Expr::IntLit(0), span);

    for clause in &query.clauses {
        match clause {
            QueryClause::From { ident, source } => {
                from_ident = Some(ident.clone());
                expr = source.clone();
            }
            QueryClause::Where(pred) => {
                let param = from_ident.clone().expect("where without from");
                expr = wrap_method_call(expr, "Where", vec![make_lambda(&param, pred)], span);
            }
            QueryClause::OrderBy { key, descending } => {
                let param = from_ident.clone().expect("orderby without from");
                let method = if *descending {
                    "OrderByDescending"
                } else {
                    "OrderBy"
                };
                expr = wrap_method_call(expr, method, vec![make_lambda(&param, key)], span);
            }
            QueryClause::Join {
                ident,
                source,
                on_left,
                on_right,
            } => {
                expr = Spanned::new(
                    Expr::MethodCall {
                        receiver: Box::new(expr),
                        method: "Join".into(),
                        args: vec![source.clone(), make_join_lambda(ident, on_left, on_right)],
                        type_args: vec![],
                        params_span: None,
                    },
                    span,
                );
            }
            QueryClause::GroupBy { key, element, .. } => {
                let param = from_ident.clone().expect("groupby without from");
                let args = if let Some(el) = element {
                    vec![make_lambda(&param, key), el.clone()]
                } else {
                    vec![make_lambda(&param, key)]
                };
                expr = wrap_method_call(expr, "GroupBy", args, span);
            }
            QueryClause::Let { ident, .. } => {
                // 编译器路径已由 `desugar_expr` 的 `query_has_special_clauses`
                // 透传拦截；此处仅防御直接调用 `desugar_query`（测试）时
                // 不静默丢子句——多变量流无法折叠为单值方法链。
                errors.push(format!(
                    "LINQ `let` clause (introduced `{ident}`) must be lowered by the \
                     MIR multi-variable path, not the method-chain desugar."
                ));
                return None;
            }
        }
    }

    let param = from_ident.expect("select without from");
    Some(wrap_method_call(
        expr,
        "Select",
        vec![make_lambda(&param, &query.select)],
        span,
    ))
}

// Re-export for tests that still call `desugar_query` directly.
#[doc(hidden)]
pub fn desugar_query(query: &QueryExpr) -> Spanned<Expr> {
    desugar_query_impl(query, &mut Vec::new())
        .expect("desugar_query should not fail for supported clauses")
}

/// True when the query contains a multi-variable clause (`let` / `join` /
/// `groupby`) that cannot be folded into a single-value method chain.
fn query_has_special_clauses(q: &QueryExpr) -> bool {
    q.clauses.iter().any(|c| {
        matches!(
            c,
            QueryClause::Let { .. } | QueryClause::Join { .. } | QueryClause::GroupBy { .. }
        )
    })
}

fn wrap_method_call(
    receiver: Spanned<Expr>,
    method: &str,
    args: Vec<Spanned<Expr>>,
    span: Span,
) -> Spanned<Expr> {
    Spanned::new(
        Expr::MethodCall {
            receiver: Box::new(receiver),
            method: method.into(),
            args,
            type_args: vec![],
            params_span: None,
        },
        span,
    )
}

fn make_lambda(param: &Ident, body: &Spanned<Expr>) -> Spanned<Expr> {
    Spanned::new(
        Expr::Lambda(LambdaExpr {
            params: vec![LambdaParam {
                name: param.clone(),
                ty: None,
                default: None,
            }],
            body: LambdaBody::Expr(Box::new(body.clone())),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        }),
        body.span,
    )
}

fn make_join_lambda(
    ident: &Ident,
    on_left: &Spanned<Expr>,
    on_right: &Spanned<Expr>,
) -> Spanned<Expr> {
    Spanned::new(
        Expr::Lambda(LambdaExpr {
            params: vec![
                LambdaParam {
                    name: "outer".into(),
                    ty: None,
                    default: None,
                },
                LambdaParam {
                    name: ident.clone(),
                    ty: None,
                    default: None,
                },
            ],
            body: LambdaBody::Expr(Box::new(Spanned::new(
                Expr::Binary {
                    op: BinOp::Eq,
                    left: Box::new(on_left.clone()),
                    right: Box::new(on_right.clone()),
                },
                Span::DUMMY,
            ))),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        }),
        Span::DUMMY,
    )
}

fn desugar_item(item: &mut Item, errors: &mut Vec<String>) {
    match item {
        Item::Namespace(ns) => {
            for inner in &mut ns.items {
                desugar_item(&mut inner.node, errors);
            }
        }
        Item::Fn(f) => {
            if let Some(body) = &mut f.body {
                desugar_block(body, errors);
            }
        }
        Item::Class(c) => {
            for m in &mut c.methods {
                if let Some(body) = &mut m.node.body {
                    desugar_block(body, errors);
                }
            }
            for ctor in &mut c.constructors {
                desugar_block(&mut ctor.node.body, errors);
            }
        }
        _ => {}
    }
}

fn desugar_block(block: &mut Block, errors: &mut Vec<String>) {
    for stmt in &mut block.stmts {
        desugar_stmt(&mut stmt.node, errors);
    }
    if let Some(tail) = &mut block.tail {
        desugar_expr(&mut tail.node, errors);
    }
}

fn desugar_stmt(stmt: &mut Stmt, errors: &mut Vec<String>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                desugar_expr(&mut e.node, errors);
            }
        }
        Stmt::Return(e) => {
            if let Some(ex) = e {
                desugar_expr(&mut ex.node, errors);
            }
        }
        Stmt::While { cond, body } => {
            desugar_expr(&mut cond.node, errors);
            desugar_block(body, errors);
        }
        Stmt::For { iter, body, .. } => {
            desugar_expr(&mut iter.node, errors);
            desugar_block(body, errors);
        }
        Stmt::Assign { target, value } => {
            desugar_expr(&mut target.node, errors);
            desugar_expr(&mut value.node, errors);
        }
        Stmt::Break | Stmt::Continue => {}
        Stmt::Expr(e) => desugar_expr(&mut e.node, errors),
        Stmt::Throw { expr } => desugar_expr(&mut expr.node, errors),
        Stmt::TryCatch {
            try_body,
            when_cond,
            catch_body,
            finally,
            ..
        } => {
            desugar_block(try_body, errors);
            if let Some(w) = when_cond {
                desugar_expr(&mut w.node, errors);
            }
            desugar_block(catch_body, errors);
            if let Some(f) = finally {
                desugar_block(f, errors);
            }
        }
        Stmt::TryFinally { body, finally } => {
            desugar_block(body, errors);
            desugar_block(finally, errors);
        }
        Stmt::Using { init, body, .. } => {
            desugar_expr(&mut init.node, errors);
            desugar_block(body, errors);
        }
        Stmt::UsingVar { init, .. } => {
            desugar_expr(&mut init.node, errors);
        }
        Stmt::Lock { expr, body } => {
            desugar_expr(&mut expr.node, errors);
            desugar_block(body, errors);
        }
        Stmt::ForC { .. } => {}
        Stmt::DeconstructAssign { value, .. } => {
            desugar_expr(&mut value.node, errors);
        }
        Stmt::AwaitUsing { init, body, .. } => {
            desugar_expr(&mut init.node, errors);
            desugar_block(body, errors);
        }
        Stmt::AwaitUsingVar { init, .. } => {
            desugar_expr(&mut init.node, errors);
        }
        // RFC 044：yield 由专门的脱糖 pass 重写为状态机；此处仅递归其值表达式。
        Stmt::YieldReturn { value } => {
            desugar_expr(&mut value.node, errors);
        }
        Stmt::YieldBreak => {}
    }
}

fn desugar_expr(expr: &mut Expr, errors: &mut Vec<String>) {
    match expr {
        Expr::Query(q) => {
            if query_has_special_clauses(q) {
                // `let` / `join` / `groupby` 是多变量流（范围变量 + 引入变量同时
                // 前流），无法折叠为单值 Select 方法链。保留 `Expr::Query` 透传
                // typeck → MIR 特化物化（RFC 003 编译期展开红线不变，仅物化层级
                // 不同）。仍递归子表达式，保证嵌套的普通查询照常脱糖。
                for clause in &mut q.clauses {
                    match clause {
                        QueryClause::From { source, .. } => desugar_expr(&mut source.node, errors),
                        QueryClause::Let { value, .. } => desugar_expr(&mut value.node, errors),
                        QueryClause::Where(e) => desugar_expr(&mut e.node, errors),
                        QueryClause::OrderBy { key, .. } => desugar_expr(&mut key.node, errors),
                        QueryClause::Join {
                            source,
                            on_left,
                            on_right,
                            ..
                        } => {
                            desugar_expr(&mut source.node, errors);
                            desugar_expr(&mut on_left.node, errors);
                            desugar_expr(&mut on_right.node, errors);
                        }
                        QueryClause::GroupBy { key, element, .. } => {
                            desugar_expr(&mut key.node, errors);
                            if let Some(el) = element {
                                desugar_expr(&mut el.node, errors);
                            }
                        }
                    }
                }
                desugar_expr(&mut q.select.node, errors);
                return;
            }
            match desugar_query_impl(q, errors) {
                Some(desugared) => {
                    *expr = desugared.node;
                    desugar_expr(expr, errors);
                }
                None => {
                    // Error already collected; leave expr unchanged
                    // so downstream phases can still produce partial results.
                }
            }
        }
        Expr::Binary { left, right, .. } => {
            desugar_expr(&mut left.node, errors);
            desugar_expr(&mut right.node, errors);
        }
        Expr::Unary { expr: inner, .. } => desugar_expr(&mut inner.node, errors),
        Expr::Call { func, args, .. } => {
            desugar_expr(&mut func.node, errors);
            for a in args {
                desugar_expr(&mut a.node, errors);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            desugar_expr(&mut receiver.node, errors);
            for a in args {
                desugar_expr(&mut a.node, errors);
            }
        }
        Expr::Field { receiver, .. } => desugar_expr(&mut receiver.node, errors),
        Expr::Index {
            receiver, index, ..
        } => {
            desugar_expr(&mut receiver.node, errors);
            desugar_expr(&mut index.node, errors);
        }
        Expr::Lambda(l) => desugar_lambda(l, errors),
        Expr::Await(inner) => desugar_expr(&mut inner.node, errors),
        Expr::Block(b) => desugar_block(b, errors),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            desugar_expr(&mut cond.node, errors);
            desugar_block(then_branch, errors);
            if let Some(e) = else_branch {
                desugar_block(e, errors);
            }
        }
        Expr::Switch(s) => desugar_switch(s, errors),
        Expr::SwitchForm(s) => {
            desugar_expr(&mut s.scrutinee.node, errors);
            for arm in &mut s.arms {
                if let Some(w) = &mut arm.when {
                    desugar_expr(&mut w.node, errors);
                }
                desugar_expr(&mut arm.body.node, errors);
            }
        }
        Expr::CollectionExpr { elements } => {
            for el in elements {
                desugar_expr(&mut el.expr_mut().node, errors);
            }
        }
        _ => {}
    }
}

fn desugar_lambda(l: &mut LambdaExpr, errors: &mut Vec<String>) {
    match &mut l.body {
        LambdaBody::Expr(e) => desugar_expr(&mut e.node, errors),
        LambdaBody::Block(b) => desugar_block(b, errors),
    }
}

fn desugar_switch(s: &mut SwitchExpr, errors: &mut Vec<String>) {
    desugar_expr(&mut s.scrutinee.node, errors);
    for case in &mut s.cases {
        desugar_block(&mut case.body, errors);
    }
}
