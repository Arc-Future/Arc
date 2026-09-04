//! RFC 006 A2：C# 13 `field` 关键字——属性访问器体内引用该属性的合成 backing field。
//!
//! 设计（纯新增、零新增语言机制）：在 typeck **检查访问器体之前**，把体内所有
//! `Ident("field")` 在 AST 层重写为 `this.<backing>` 字段访问（backing field 名 =
//! 属性名，已由 registry/checker 注册进类型布局）。这样 `field` 自然复用既有字段
//! 访问 lower（可读可写、落到实例槽），无需改动作用域/左值机制。
//!
//! 仅属性访问器体（get_body/set_body）调用本模块；普通方法体等其它上下文不调用，
//! `field` 保持普通标识符语义（作为变量名合法）。
//!
//! 说明：`uses_field` 与 `rewrite_field_block` 共用同一递归遍历
//! （`rewrite_block`），用 `seen` 回传是否命中 `field`，避免维护两份遍历。

use ast::*;

/// 合成 backing field 的命名。
///
/// 必须与属性名**区分**：MIR `is_custom_accessor_property` 判定
/// `has_get_X && !has_field(X)`——若 backing field 与属性同名，会被判为
/// auto-property，导致 `obj.X` 直接走字段访问、绕过 get/set 访问器
/// （`field` 体内的校验逻辑将失效）。故采用 C# 式合成名 `{Prop}__backing`。
pub(crate) fn backing_field_name(prop: &Ident) -> Ident {
    format!("{prop}__backing").into()
}

/// 判断属性是否在其访问器体（get/set）中引用了 `field` 关键字。
pub(crate) fn uses_field(get_body: &Option<Block>, set_body: &Option<Block>) -> bool {
    let mut seen = false;
    // backing 名仅在真正命中 `Ident("field")` 时才会被用到；此处传占位名，
    // 仅利用 `seen` 回传命中与否。
    if let Some(b) = get_body {
        rewrite_block(b, &"field".into(), &mut seen);
    }
    if let Some(b) = set_body {
        rewrite_block(b, &"field".into(), &mut seen);
    }
    seen
}

/// 把访问器体中的 `Ident("field")` 重写为 `this.<backing>` 字段访问。
pub(crate) fn rewrite_field_block(block: &Block, backing: &Ident) -> Block {
    let mut _seen = false;
    rewrite_block(block, backing, &mut _seen)
}

fn rewrite_block(block: &Block, backing: &Ident, seen: &mut bool) -> Block {
    Block {
        stmts: block
            .stmts
            .iter()
            .map(|s| Spanned::new(rewrite_stmt(&s.node, backing, seen), s.span))
            .collect(),
        tail: block
            .tail
            .as_ref()
            .map(|e| Box::new(rw_span_expr(e, backing, seen))),
    }
}

fn rw_span_expr(e: &Spanned<Expr>, backing: &Ident, seen: &mut bool) -> Spanned<Expr> {
    Spanned::new(rewrite_expr(&e.node, backing, seen), e.span)
}

fn rewrite_expr(expr: &Expr, backing: &Ident, seen: &mut bool) -> Expr {
    use Expr as E;
    match expr {
        E::Ident(name) => {
            if name.as_str() == "field" {
                *seen = true;
                E::Field {
                    receiver: Box::new(Spanned::new(E::This, Span::DUMMY)),
                    field: backing.clone(),
                }
            } else {
                E::Ident(name.clone())
            }
        }
        E::IntLit(_)
        | E::FloatLit(_)
        | E::BoolLit(_)
        | E::StringLit(_)
        | E::CharLit(_)
        | E::Path(_)
        | E::This
        | E::Base
        | E::Null => expr.clone(),
        E::InterpolatedString { parts } => E::InterpolatedString {
            parts: parts
                .iter()
                .map(|p| match p {
                    InterpPart::Lit(s) => InterpPart::Lit(s.clone()),
                    InterpPart::Expr(hole) => InterpPart::Expr(InterpHole {
                        expr: rw_span_expr(&hole.expr, backing, seen),
                        alignment: hole.alignment,
                        format: hole.format.clone(),
                    }),
                })
                .collect(),
        },
        E::Binary { op, left, right } => E::Binary {
            op: *op,
            left: Box::new(rw_span_expr(left, backing, seen)),
            right: Box::new(rw_span_expr(right, backing, seen)),
        },
        // 赋值表达式：目标与值均递归重写（`field` 关键字可出现在两侧）。
        E::Assign { target, value } => E::Assign {
            target: Box::new(rw_span_expr(target, backing, seen)),
            value: Box::new(rw_span_expr(value, backing, seen)),
        },
        E::Unary { op, expr } => E::Unary {
            op: *op,
            expr: Box::new(rw_span_expr(expr, backing, seen)),
        },
        E::Comptime(inner) => E::Comptime(Box::new(rw_span_expr(inner, backing, seen))),
        // `new T[n]`：仅长度表达式可能含 `field`，元素类型无。
        E::NewArray { elem_type, length } => E::NewArray {
            elem_type: elem_type.clone(),
            length: Box::new(rw_span_expr(length, backing, seen)),
        },
        E::Call {
            func,
            args,
            type_args,
            params_span,
        } => E::Call {
            func: Box::new(rw_span_expr(func, backing, seen)),
            args: args
                .iter()
                .map(|a| rw_span_expr(a, backing, seen))
                .collect(),
            type_args: type_args.clone(),
            params_span: params_span.clone(),
        },
        E::MethodCall {
            receiver,
            method,
            args,
            type_args,
            params_span,
        } => E::MethodCall {
            receiver: Box::new(rw_span_expr(receiver, backing, seen)),
            method: method.clone(),
            args: args
                .iter()
                .map(|a| rw_span_expr(a, backing, seen))
                .collect(),
            type_args: type_args.clone(),
            params_span: params_span.clone(),
        },
        E::Field { receiver, field } => E::Field {
            receiver: Box::new(rw_span_expr(receiver, backing, seen)),
            field: field.clone(),
        },
        E::Index { receiver, index } => E::Index {
            receiver: Box::new(rw_span_expr(receiver, backing, seen)),
            index: Box::new(rw_span_expr(index, backing, seen)),
        },
        E::Lambda(l) => E::Lambda(LambdaExpr {
            params: l.params.clone(),
            body: match &l.body {
                LambdaBody::Expr(e) => LambdaBody::Expr(Box::new(rw_span_expr(e, backing, seen))),
                LambdaBody::Block(b) => LambdaBody::Block(rewrite_block(b, backing, seen)),
            },
            is_expression_tree: l.is_expression_tree,
            is_async: l.is_async,
            captures: l.captures.clone(),
        }),
        E::ExpressionLit(el) => E::ExpressionLit(ExpressionLit {
            lambda: LambdaExpr {
                params: el.lambda.params.clone(),
                body: match &el.lambda.body {
                    LambdaBody::Expr(e) => {
                        LambdaBody::Expr(Box::new(rw_span_expr(e, backing, seen)))
                    }
                    LambdaBody::Block(b) => LambdaBody::Block(rewrite_block(b, backing, seen)),
                },
                is_expression_tree: el.lambda.is_expression_tree,
                is_async: el.lambda.is_async,
                captures: el.lambda.captures.clone(),
            },
        }),
        E::Await(e) => E::Await(Box::new(rw_span_expr(e, backing, seen))),
        E::Block(b) => E::Block(rewrite_block(b, backing, seen)),
        E::If {
            cond,
            then_branch,
            else_branch,
        } => E::If {
            cond: Box::new(rw_span_expr(cond, backing, seen)),
            then_branch: rewrite_block(then_branch, backing, seen),
            else_branch: else_branch
                .as_ref()
                .map(|b| rewrite_block(b, backing, seen)),
        },
        E::Switch(s) => E::Switch(SwitchExpr {
            scrutinee: Box::new(rw_span_expr(&s.scrutinee, backing, seen)),
            cases: s
                .cases
                .iter()
                .map(|c| SwitchCase {
                    pattern: c.pattern.clone(),
                    when: c.when.as_ref().map(|w| rw_span_expr(w, backing, seen)),
                    body: rewrite_block(&c.body, backing, seen),
                })
                .collect(),
        }),
        E::SwitchForm(s) => E::SwitchForm(SwitchExprForm {
            scrutinee: Box::new(rw_span_expr(&s.scrutinee, backing, seen)),
            arms: s
                .arms
                .iter()
                .map(|a| SwitchExprArm {
                    pattern: a.pattern.clone(),
                    when: a.when.as_ref().map(|w| rw_span_expr(w, backing, seen)),
                    body: rw_span_expr(&a.body, backing, seen),
                })
                .collect(),
        }),
        E::CollectionExpr { elements } => E::CollectionExpr {
            elements: elements
                .iter()
                .map(|el| match el {
                    CollectionElement::Element(e) => {
                        CollectionElement::Element(rw_span_expr(e, backing, seen))
                    }
                    CollectionElement::Spread(e) => {
                        CollectionElement::Spread(rw_span_expr(e, backing, seen))
                    }
                })
                .collect(),
        },
        E::Cast { expr, ty } => E::Cast {
            expr: Box::new(rw_span_expr(expr, backing, seen)),
            ty: ty.clone(),
        },
        E::Box { expr, value_ty } => E::Box {
            expr: Box::new(rw_span_expr(expr, backing, seen)),
            value_ty: value_ty.clone(),
        },
        E::Unbox { expr, value_ty } => E::Unbox {
            expr: Box::new(rw_span_expr(expr, backing, seen)),
            value_ty: value_ty.clone(),
        },
        E::New { ty, args, obj_init } => E::New {
            ty: ty.clone(),
            args: args
                .iter()
                .map(|a| rw_span_expr(a, backing, seen))
                .collect(),
            obj_init: obj_init.as_ref().map(|inits| {
                inits
                    .iter()
                    .map(|(name, e)| (name.clone(), rw_span_expr(e, backing, seen)))
                    .collect()
            }),
        },
        E::Query(q) => E::Query(QueryExpr {
            clauses: q
                .clauses
                .iter()
                .map(|cl| match cl {
                    QueryClause::From { ident, source } => QueryClause::From {
                        ident: ident.clone(),
                        source: rw_span_expr(source, backing, seen),
                    },
                    QueryClause::Let { ident, value } => QueryClause::Let {
                        ident: ident.clone(),
                        value: rw_span_expr(value, backing, seen),
                    },
                    QueryClause::Where(e) => QueryClause::Where(rw_span_expr(e, backing, seen)),
                    QueryClause::OrderBy { key, descending } => QueryClause::OrderBy {
                        key: rw_span_expr(key, backing, seen),
                        descending: *descending,
                    },
                    QueryClause::Join {
                        ident,
                        source,
                        on_left,
                        on_right,
                    } => QueryClause::Join {
                        ident: ident.clone(),
                        source: rw_span_expr(source, backing, seen),
                        on_left: rw_span_expr(on_left, backing, seen),
                        on_right: rw_span_expr(on_right, backing, seen),
                    },
                    QueryClause::GroupBy {
                        key,
                        element,
                        into_ident,
                    } => QueryClause::GroupBy {
                        key: rw_span_expr(key, backing, seen),
                        element: element.as_ref().map(|e| rw_span_expr(e, backing, seen)),
                        into_ident: into_ident.clone(),
                    },
                })
                .collect(),
            select: Box::new(rw_span_expr(&q.select, backing, seen)),
        }),
        E::RefArg { is_out, expr } => E::RefArg {
            is_out: *is_out,
            expr: Box::new(rw_span_expr(expr, backing, seen)),
        },
        E::NamedArg { name, expr } => E::NamedArg {
            name: name.clone(),
            expr: Box::new(rw_span_expr(expr, backing, seen)),
        },
        E::StackSpanLit {
            elements,
            mutable,
            elem,
        } => E::StackSpanLit {
            elements: elements
                .iter()
                .map(|e| rw_span_expr(e, backing, seen))
                .collect(),
            mutable: *mutable,
            elem: elem.clone(),
        },
        E::Ternary {
            cond,
            then_branch,
            else_branch,
        } => E::Ternary {
            cond: Box::new(rw_span_expr(cond, backing, seen)),
            then_branch: Box::new(rw_span_expr(then_branch, backing, seen)),
            else_branch: Box::new(rw_span_expr(else_branch, backing, seen)),
        },
        E::Coalesce { left, right } => E::Coalesce {
            left: Box::new(rw_span_expr(left, backing, seen)),
            right: Box::new(rw_span_expr(right, backing, seen)),
        },
        E::NullCond { access } => E::NullCond {
            access: Box::new(rw_span_expr(access, backing, seen)),
        },
        E::ForceDeref { access } => E::ForceDeref {
            access: Box::new(rw_span_expr(access, backing, seen)),
        },
        E::Default { ty } => E::Default { ty: ty.clone() },
        E::TypeOf(ty) => E::TypeOf(ty.clone()),
        E::Is { expr, pattern } => E::Is {
            expr: Box::new(rw_span_expr(expr, backing, seen)),
            pattern: pattern.clone(),
        },
        E::With { receiver, inits } => E::With {
            receiver: Box::new(rw_span_expr(receiver, backing, seen)),
            inits: inits
                .iter()
                .map(|(n, e)| (n.clone(), rw_span_expr(e, backing, seen)))
                .collect(),
        },
    }
}

fn rewrite_stmt(stmt: &Stmt, backing: &Ident, seen: &mut bool) -> Stmt {
    use Stmt as S;
    match stmt {
        S::Let {
            mutable,
            name,
            ty,
            init,
        } => S::Let {
            mutable: *mutable,
            name: name.clone(),
            ty: ty.clone(),
            init: init.as_ref().map(|e| rw_span_expr(e, backing, seen)),
        },
        S::Expr(e) => S::Expr(rw_span_expr(e, backing, seen)),
        S::Return(e) => S::Return(e.as_ref().map(|e| rw_span_expr(e, backing, seen))),
        S::While { cond, body } => S::While {
            cond: rw_span_expr(cond, backing, seen),
            body: rewrite_block(body, backing, seen),
        },
        S::For { var, iter, body } => S::For {
            var: var.clone(),
            iter: rw_span_expr(iter, backing, seen),
            body: rewrite_block(body, backing, seen),
        },
        S::ForC {
            init,
            cond,
            inc,
            body,
        } => S::ForC {
            init: init
                .as_ref()
                .map(|s| Spanned::new(Box::new(rewrite_stmt(&s.node, backing, seen)), s.span)),
            cond: cond.as_ref().map(|e| rw_span_expr(e, backing, seen)),
            inc: inc
                .as_ref()
                .map(|s| Spanned::new(Box::new(rewrite_stmt(&s.node, backing, seen)), s.span)),
            body: rewrite_block(body, backing, seen),
        },
        S::Assign { target, value } => S::Assign {
            target: rw_span_expr(target, backing, seen),
            value: rw_span_expr(value, backing, seen),
        },
        S::Break => S::Break,
        S::Continue => S::Continue,
        S::Throw { expr } => S::Throw {
            expr: rw_span_expr(expr, backing, seen),
        },
        S::TryCatch {
            try_body,
            catch_ty,
            catch_name,
            when_cond,
            catch_body,
            finally,
        } => S::TryCatch {
            try_body: rewrite_block(try_body, backing, seen),
            catch_ty: catch_ty.clone(),
            catch_name: catch_name.clone(),
            when_cond: when_cond.as_ref().map(|e| rw_span_expr(e, backing, seen)),
            catch_body: rewrite_block(catch_body, backing, seen),
            finally: finally.as_ref().map(|b| rewrite_block(b, backing, seen)),
        },
        S::TryFinally { body, finally } => S::TryFinally {
            body: rewrite_block(body, backing, seen),
            finally: rewrite_block(finally, backing, seen),
        },
        S::Using {
            name,
            ty,
            init,
            body,
        } => S::Using {
            name: name.clone(),
            ty: ty.clone(),
            init: rw_span_expr(init, backing, seen),
            body: rewrite_block(body, backing, seen),
        },
        S::UsingVar { name, ty, init } => S::UsingVar {
            name: name.clone(),
            ty: ty.clone(),
            init: rw_span_expr(init, backing, seen),
        },
        S::AwaitUsing {
            name,
            ty,
            init,
            body,
        } => S::AwaitUsing {
            name: name.clone(),
            ty: ty.clone(),
            init: rw_span_expr(init, backing, seen),
            body: rewrite_block(body, backing, seen),
        },
        S::AwaitUsingVar { name, ty, init } => S::AwaitUsingVar {
            name: name.clone(),
            ty: ty.clone(),
            init: rw_span_expr(init, backing, seen),
        },
        S::YieldReturn { value } => S::YieldReturn {
            value: rw_span_expr(value, backing, seen),
        },
        S::YieldBreak => S::YieldBreak,
        S::Lock { expr, body } => S::Lock {
            expr: rw_span_expr(expr, backing, seen),
            body: rewrite_block(body, backing, seen),
        },
        S::DeconstructAssign {
            declare,
            targets,
            value,
        } => S::DeconstructAssign {
            declare: *declare,
            targets: targets.clone(),
            value: rw_span_expr(value, backing, seen),
        },
    }
}
