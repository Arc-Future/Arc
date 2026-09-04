//! RFC 009 M4-6 / M5-3: ??????? + span ???
//!
//! ??????????M4-4 / M5-3?????????? AST ???
//! ??? span ??????????? span ?????????
//! ?RFC 009 D10.4 ??????
//!
//! # ????
//!
//! - [`parse_expansion`]?M4-6 ?????????? `Vec<Spanned<Stmt>>`
//!   ?????????? M4 splice ???????
//! - [`rewrite_program_span`]?M5-3 ???????? `Program`?Source
//!   Generator ?????????? span ????? Generate ?????
//!
//! # ???????? span?????????? span + ?????
//!
//! ??????????????????????????????????
//! ??????????????? file_id ????? D10.4?????
//! ?????????????? lambda / Generate ?????????
//! ????????????????
//!
//! # ????
//!
//! ????????? ? AST + span ???????????M4-7???
//! ??? splice ?????M4-9??????????????????

use ast::{
    Block, Expr, InterpHole, InterpPart, Item, Pattern, Program, Span, Spanned, Stmt, SwitchCase,
    SwitchExpr, SwitchExprArm, SwitchExprForm, Type,
};
use parse::{ParseError, Parser};

/// M4-6 splice ?????
///
/// ????? parse ??????????? span ??????????
/// ??????? M4-7 ??? typeck ???????????
///
/// ? derive `Clone`/`PartialEq`????? `ParseError` ?????? trait?
#[derive(Debug)]
pub enum SpliceError {
    /// ????????????????
    ///
    /// `delegate_span` ?? `Func<string>` ?????RFC 009 D10.4 ??????
    /// `source` ??? ParseError???? span ?????????????
    /// ???????????????
    ParseError {
        source: ParseError,
        delegate_span: Span,
    },
}

impl SpliceError {
    /// ??? `TypeError` ?? typeck ????
    ///
    /// RFC 009 M4-9 D12.3??? `arc-macro-003` ????????????????
    /// ?Pass 3 ???????????????????D10.4??
    pub fn to_type_error(&self) -> crate::error::TypeError {
        match self {
            SpliceError::ParseError {
                source,
                delegate_span,
            } => crate::error::TypeError::Macro {
                code: "arc-macro-003",
                message: format!("????????? (???? {:?}): {}", delegate_span, source),
            },
        }
    }
}

/// ?????????? span ???
///
/// ????RFC 009 D10.3 ?? 1+2??
///
/// # ??
///
/// - `expansion_source`????????? Arc ??????
/// - `delegate_span`?`Func<string>` ??????????????????
///   ? span ?????????D10.4 ?????
/// - `file_id`??? file_id???? lex ?? token span??????????
///
/// # ??
///
/// ?????????????? span ?? `delegate_span`?
/// ???? `SpliceError::ParseError`?????????????
///
/// # ??
///
/// ???????? M4-7 ???????? M4-9 ?? splice ???????
pub fn parse_expansion(
    expansion_source: &str,
    delegate_span: Span,
    file_id: ast::FileId,
) -> Result<Vec<Spanned<Stmt>>, SpliceError> {
    let stmts = Parser::parse_stmts_from_str(expansion_source, file_id).map_err(|source| {
        SpliceError::ParseError {
            source,
            delegate_span,
        }
    })?;
    // span ??????? span ??? delegate_span
    Ok(stmts
        .into_iter()
        .map(|s| rewrite_stmt_span(s, delegate_span))
        .collect())
}

// ---------------------------------------------------------------------------
// span ???????? AST???? span ??????? span
// ---------------------------------------------------------------------------

fn rewrite_stmt_span(stmt: Spanned<Stmt>, target: Span) -> Spanned<Stmt> {
    let Spanned { node, span: _ } = stmt;
    let new_node = match node {
        Stmt::Let {
            mutable,
            name,
            ty,
            init,
        } => Stmt::Let {
            mutable,
            name,
            ty: ty.map(|t| rewrite_type_span(t, target)),
            init: init.map(|e| rewrite_expr_span(e, target)),
        },
        Stmt::Expr(e) => Stmt::Expr(rewrite_expr_span(e, target)),
        Stmt::Return(opt) => Stmt::Return(opt.map(|e| rewrite_expr_span(e, target))),
        Stmt::While { cond, body } => Stmt::While {
            cond: rewrite_expr_span(cond, target),
            body: rewrite_block_span(body, target),
        },
        Stmt::For { var, iter, body } => Stmt::For {
            var,
            iter: rewrite_expr_span(iter, target),
            body: rewrite_block_span(body, target),
        },
        Stmt::Assign { target: t, value } => Stmt::Assign {
            target: rewrite_expr_span(t, target),
            value: rewrite_expr_span(value, target),
        },
        Stmt::Break => Stmt::Break,
        Stmt::Continue => Stmt::Continue,
        Stmt::Throw { expr } => Stmt::Throw {
            expr: rewrite_expr_span(expr, target),
        },
        Stmt::TryCatch {
            try_body,
            catch_ty,
            catch_name,
            when_cond,
            catch_body,
            finally,
        } => Stmt::TryCatch {
            try_body: rewrite_block_span(try_body, target),
            catch_ty: rewrite_type_span(catch_ty, target),
            catch_name,
            when_cond: when_cond.map(|w| rewrite_expr_span(w, target)),
            catch_body: rewrite_block_span(catch_body, target),
            finally: finally.map(|f| rewrite_block_span(f, target)),
        },
        Stmt::TryFinally { body, finally } => Stmt::TryFinally {
            body: rewrite_block_span(body, target),
            finally: rewrite_block_span(finally, target),
        },
        Stmt::Using {
            name,
            ty,
            init,
            body,
        } => Stmt::Using {
            name,
            ty: ty.map(|t| rewrite_type_span(t, target)),
            init: rewrite_expr_span(init, target),
            body: rewrite_block_span(body, target),
        },
        Stmt::UsingVar { name, ty, init } => Stmt::UsingVar {
            name,
            ty: ty.map(|t| rewrite_type_span(t, target)),
            init: rewrite_expr_span(init, target),
        },
        Stmt::AwaitUsing {
            name,
            ty,
            init,
            body,
        } => Stmt::AwaitUsing {
            name,
            ty: ty.map(|t| rewrite_type_span(t, target)),
            init: rewrite_expr_span(init, target),
            body: rewrite_block_span(body, target),
        },
        Stmt::AwaitUsingVar { name, ty, init } => Stmt::AwaitUsingVar {
            name,
            ty: ty.map(|t| rewrite_type_span(t, target)),
            init: rewrite_expr_span(init, target),
        },
        Stmt::YieldReturn { value } => Stmt::YieldReturn {
            value: rewrite_expr_span(value, target),
        },
        Stmt::YieldBreak => Stmt::YieldBreak,
        Stmt::Lock { expr, body } => Stmt::Lock {
            expr: rewrite_expr_span(expr, target),
            body: rewrite_block_span(body, target),
        },
        Stmt::ForC {
            init,
            cond,
            inc,
            body,
        } => Stmt::ForC {
            init: init.map(|s| {
                Spanned::new(
                    Box::new(
                        rewrite_stmt_span(Spanned::new((*s.node).clone(), s.span), target).node,
                    ),
                    target,
                )
            }),
            cond: cond.map(|e| rewrite_expr_span(e, target)),
            inc: inc.map(|s| {
                Spanned::new(
                    Box::new(
                        rewrite_stmt_span(Spanned::new((*s.node).clone(), s.span), target).node,
                    ),
                    target,
                )
            }),
            body: rewrite_block_span(body, target),
        },
        Stmt::DeconstructAssign {
            declare,
            targets,
            value,
        } => Stmt::DeconstructAssign {
            declare,
            targets,
            value: rewrite_expr_span(value, target),
        },
    };
    Spanned::new(new_node, target)
}

fn rewrite_block_span(block: Block, target: Span) -> Block {
    Block {
        stmts: block
            .stmts
            .into_iter()
            .map(|s| rewrite_stmt_span(s, target))
            .collect(),
        tail: block.tail.map(|t| Box::new(rewrite_expr_span(*t, target))),
    }
}

fn rewrite_expr_span(expr: Spanned<Expr>, target: Span) -> Spanned<Expr> {
    let Spanned { node, span: _ } = expr;
    let new_node = match node {
        // ????????????
        Expr::IntLit(i) => Expr::IntLit(i),
        Expr::FloatLit(f) => Expr::FloatLit(f),
        Expr::BoolLit(b) => Expr::BoolLit(b),
        Expr::StringLit(s) => Expr::StringLit(s),
        Expr::CharLit(c) => Expr::CharLit(c),
        Expr::Ident(i) => Expr::Ident(i),
        Expr::Path(p) => Expr::Path(p),
        Expr::This => Expr::This,
        Expr::Base => Expr::Base,
        Expr::Null => Expr::Null,

        // ??/??
        Expr::Unary { op, expr: e } => Expr::Unary {
            op,
            expr: Box::new(rewrite_expr_span(*e, target)),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: Box::new(rewrite_expr_span(*left, target)),
            right: Box::new(rewrite_expr_span(*right, target)),
        },
        Expr::Assign { target: t, value } => Expr::Assign {
            target: Box::new(rewrite_expr_span(*t, target)),
            value: Box::new(rewrite_expr_span(*value, target)),
        },
        Expr::Comptime(inner) => Expr::Comptime(Box::new(rewrite_expr_span(*inner, target))),

        // ???
        Expr::Call {
            func,
            args,
            type_args,
            params_span,
        } => Expr::Call {
            func: Box::new(rewrite_expr_span(*func, target)),
            args: args
                .into_iter()
                .map(|a| rewrite_expr_span(a, target))
                .collect(),
            type_args: type_args
                .into_iter()
                .map(|t| rewrite_type_span(t, target))
                .collect(),
            params_span,
        },
        Expr::MethodCall {
            receiver,
            method,
            args,
            type_args,
            params_span,
        } => Expr::MethodCall {
            receiver: Box::new(rewrite_expr_span(*receiver, target)),
            method,
            args: args
                .into_iter()
                .map(|a| rewrite_expr_span(a, target))
                .collect(),
            type_args: type_args
                .into_iter()
                .map(|t| rewrite_type_span(t, target))
                .collect(),
            params_span,
        },
        Expr::Field { receiver, field } => Expr::Field {
            receiver: Box::new(rewrite_expr_span(*receiver, target)),
            field,
        },
        Expr::Index { receiver, index } => Expr::Index {
            receiver: Box::new(rewrite_expr_span(*receiver, target)),
            index: Box::new(rewrite_expr_span(*index, target)),
        },

        // Lambda / ExpressionLit
        Expr::Lambda(lambda) => Expr::Lambda(rewrite_lambda_span(lambda, target)),
        Expr::ExpressionLit(expr_lit) => Expr::ExpressionLit(ast::ExpressionLit {
            lambda: rewrite_lambda_span(expr_lit.lambda, target),
        }),

        // ???
        Expr::Block(b) => Expr::Block(rewrite_block_span(b, target)),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => Expr::If {
            cond: Box::new(rewrite_expr_span(*cond, target)),
            then_branch: rewrite_block_span(then_branch, target),
            else_branch: else_branch.map(|b| rewrite_block_span(b, target)),
        },
        Expr::Switch(s) => Expr::Switch(rewrite_switch_span(s, target)),
        Expr::SwitchForm(s) => Expr::SwitchForm(rewrite_switch_form_span(s, target)),
        Expr::Await(e) => Expr::Await(Box::new(rewrite_expr_span(*e, target))),

        // ?????
        Expr::CollectionExpr { elements } => Expr::CollectionExpr {
            elements: elements
                .into_iter()
                .map(|el| match el {
                    ast::CollectionElement::Element(e) => {
                        ast::CollectionElement::Element(rewrite_expr_span(e, target))
                    }
                    ast::CollectionElement::Spread(e) => {
                        ast::CollectionElement::Spread(rewrite_expr_span(e, target))
                    }
                })
                .collect(),
        },

        // ????
        Expr::Cast { expr: e, ty } => Expr::Cast {
            expr: Box::new(rewrite_expr_span(*e, target)),
            ty: rewrite_type_span(ty, target),
        },
        Expr::Box { expr: e, value_ty } => Expr::Box {
            expr: Box::new(rewrite_expr_span(*e, target)),
            value_ty: rewrite_type_span(value_ty, target),
        },
        Expr::Unbox { expr: e, value_ty } => Expr::Unbox {
            expr: Box::new(rewrite_expr_span(*e, target)),
            value_ty: rewrite_type_span(value_ty, target),
        },
        Expr::New { ty, args, obj_init } => Expr::New {
            ty: rewrite_type_span(ty, target),
            args: args
                .into_iter()
                .map(|a| rewrite_expr_span(a, target))
                .collect(),
            obj_init: obj_init.map(|init| {
                init.into_iter()
                    .map(|(name, e)| (name, rewrite_expr_span(e, target)))
                    .collect()
            }),
        },
        Expr::Default { ty } => Expr::Default {
            ty: rewrite_type_span(ty, target),
        },
        Expr::TypeOf(ty) => Expr::TypeOf(rewrite_type_span(ty, target)),

        // LINQ ??
        Expr::Query(q) => Expr::Query(rewrite_query_span(q, target)),

        // ????
        Expr::RefArg { is_out, expr: e } => Expr::RefArg {
            is_out,
            expr: Box::new(rewrite_expr_span(*e, target)),
        },
        Expr::NamedArg { name, expr: e } => Expr::NamedArg {
            name,
            expr: Box::new(rewrite_expr_span(*e, target)),
        },
        Expr::StackSpanLit {
            elements,
            mutable,
            elem,
        } => Expr::StackSpanLit {
            elements: elements
                .into_iter()
                .map(|e| rewrite_expr_span(e, target))
                .collect(),
            mutable,
            elem,
        },
        Expr::InterpolatedString { parts } => Expr::InterpolatedString {
            parts: parts
                .into_iter()
                .map(|part| match part {
                    InterpPart::Lit(s) => InterpPart::Lit(s),
                    InterpPart::Expr(hole) => InterpPart::Expr(InterpHole {
                        expr: rewrite_expr_span(hole.expr, target),
                        alignment: hole.alignment,
                        format: hole.format,
                    }),
                })
                .collect(),
        },

        // null ?? / null ?? / ?????
        Expr::Coalesce { left, right } => Expr::Coalesce {
            left: Box::new(rewrite_expr_span(*left, target)),
            right: Box::new(rewrite_expr_span(*right, target)),
        },
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => Expr::Ternary {
            cond: Box::new(rewrite_expr_span(*cond, target)),
            then_branch: Box::new(rewrite_expr_span(*then_branch, target)),
            else_branch: Box::new(rewrite_expr_span(*else_branch, target)),
        },
        Expr::NullCond { access } => Expr::NullCond {
            access: Box::new(rewrite_expr_span(*access, target)),
        },
        Expr::ForceDeref { access } => Expr::ForceDeref {
            access: Box::new(rewrite_expr_span(*access, target)),
        },

        // is ????RFC 036 M1??????????? span?
        Expr::Is { expr, pattern } => Expr::Is {
            expr: Box::new(rewrite_expr_span(*expr, target)),
            pattern,
        },
        Expr::With { receiver, inits } => Expr::With {
            receiver: Box::new(rewrite_expr_span(*receiver, target)),
            inits: inits
                .into_iter()
                .map(|(n, e)| (n, rewrite_expr_span(e, target)))
                .collect(),
        },
        Expr::NewArray { elem_type, length } => Expr::NewArray {
            elem_type: rewrite_type_span(elem_type, target),
            length: Box::new(rewrite_expr_span(*length, target)),
        },
    };
    Spanned::new(new_node, target)
}

fn rewrite_lambda_span(lambda: ast::LambdaExpr, target: Span) -> ast::LambdaExpr {
    ast::LambdaExpr {
        params: lambda
            .params
            .into_iter()
            .map(|p| ast::LambdaParam {
                name: p.name,
                ty: p.ty.map(|t| rewrite_type_span(t, target)),
                default: p.default.map(|e| rewrite_expr_span(e, target)),
            })
            .collect(),
        body: match lambda.body {
            ast::LambdaBody::Expr(e) => {
                ast::LambdaBody::Expr(Box::new(rewrite_expr_span(*e, target)))
            }
            ast::LambdaBody::Block(b) => ast::LambdaBody::Block(rewrite_block_span(b, target)),
        },
        is_expression_tree: lambda.is_expression_tree,
        is_async: lambda.is_async,
        captures: lambda.captures,
    }
}

fn rewrite_switch_span(s: SwitchExpr, target: Span) -> SwitchExpr {
    SwitchExpr {
        scrutinee: Box::new(rewrite_expr_span(*s.scrutinee, target)),
        cases: s
            .cases
            .into_iter()
            .map(|c| rewrite_switch_case_span(c, target))
            .collect(),
    }
}

fn rewrite_switch_form_span(s: SwitchExprForm, target: Span) -> SwitchExprForm {
    SwitchExprForm {
        scrutinee: Box::new(rewrite_expr_span(*s.scrutinee, target)),
        arms: s
            .arms
            .into_iter()
            .map(|a| SwitchExprArm {
                pattern: rewrite_pattern_span(a.pattern, target),
                when: a.when.map(|w| rewrite_expr_span(w, target)),
                body: rewrite_expr_span(a.body, target),
            })
            .collect(),
    }
}

fn rewrite_switch_case_span(c: SwitchCase, target: Span) -> SwitchCase {
    SwitchCase {
        pattern: c.pattern.map(|p| rewrite_pattern_span(p, target)),
        when: c.when.map(|w| rewrite_expr_span(w, target)),
        body: rewrite_block_span(c.body, target),
    }
}

fn rewrite_pattern_span(p: Pattern, target: Span) -> Pattern {
    match p {
        Pattern::Wildcard => Pattern::Wildcard,
        Pattern::Ident(i) => Pattern::Ident(i),
        Pattern::Literal(e) => Pattern::Literal(rewrite_expr_span(e, target)),
        Pattern::Type { ty, binding } => Pattern::Type {
            ty: Spanned::new(ty.node, target),
            binding,
        },
        Pattern::Null => Pattern::Null,
        Pattern::Var(n) => Pattern::Var(n),
        Pattern::Variant {
            path,
            type_args,
            case,
            binding,
        } => Pattern::Variant {
            path,
            type_args: type_args
                .into_iter()
                .map(|t| rewrite_type_span(t, target))
                .collect(),
            case,
            binding,
        },
        Pattern::Positional(elems) => Pattern::Positional(elems),
    }
}

fn rewrite_query_span(q: ast::QueryExpr, target: Span) -> ast::QueryExpr {
    let clauses = q
        .clauses
        .into_iter()
        .map(|c| rewrite_query_clause_span(c, target))
        .collect();
    ast::QueryExpr {
        clauses,
        select: Box::new(rewrite_expr_span(*q.select, target)),
    }
}

fn rewrite_query_clause_span(c: ast::QueryClause, target: Span) -> ast::QueryClause {
    match c {
        ast::QueryClause::From { ident, source } => ast::QueryClause::From {
            ident,
            source: rewrite_expr_span(source, target),
        },
        ast::QueryClause::Let { ident, value } => ast::QueryClause::Let {
            ident,
            value: rewrite_expr_span(value, target),
        },
        ast::QueryClause::Where(e) => ast::QueryClause::Where(rewrite_expr_span(e, target)),
        ast::QueryClause::OrderBy { key, descending } => ast::QueryClause::OrderBy {
            key: rewrite_expr_span(key, target),
            descending,
        },
        ast::QueryClause::Join {
            ident,
            source,
            on_left,
            on_right,
        } => ast::QueryClause::Join {
            ident,
            source: rewrite_expr_span(source, target),
            on_left: rewrite_expr_span(on_left, target),
            on_right: rewrite_expr_span(on_right, target),
        },
        ast::QueryClause::GroupBy {
            key,
            element,
            into_ident,
        } => ast::QueryClause::GroupBy {
            key: rewrite_expr_span(key, target),
            element: element.map(|e| rewrite_expr_span(e, target)),
            into_ident: into_ident.clone(),
        },
    }
}

fn rewrite_type_span(ty: Spanned<Type>, target: Span) -> Spanned<Type> {
    let Spanned { node, span: _ } = ty;
    let new_node = match node {
        Type::Named { path, generics } => Type::Named {
            path,
            generics: generics
                .into_iter()
                .map(|g| rewrite_type_span(g, target))
                .collect(),
        },
        Type::Ref { inner, mutable } => Type::Ref {
            inner: Box::new(rewrite_type_span(*inner, target)),
            mutable,
        },
        Type::Func { params, ret } => Type::Func {
            params: params
                .into_iter()
                .map(|p| rewrite_type_span(p, target))
                .collect(),
            ret: Box::new(rewrite_type_span(*ret, target)),
        },
        Type::Array { inner } => Type::Array {
            inner: Box::new(rewrite_type_span(*inner, target)),
        },
        Type::Nullable { inner } => Type::Nullable {
            inner: Box::new(rewrite_type_span(*inner, target)),
        },
        Type::ConstInt(i) => Type::ConstInt(i),
        Type::Infer => Type::Infer,
    };
    Spanned::new(new_node, target)
}

// ---------------------------------------------------------------------------
// M5-3: Program ? span ????Source Generator ????????
// ---------------------------------------------------------------------------

/// RFC 009 M5-3: ? Source Generator ??? `Program` ??? span
/// ????? Generate ???????
///
/// ????RFC 009 D10.3 ?? 2 ? M5 ????? [`parse_expansion`]
/// ???Source Generator ??????? Arc ????? namespace / class
/// ???????????? `Item` ?????? M4-6 ? `rewrite_block_span`
/// ??????????
///
/// ???????????? span ??? `Generate` ??????
/// ?RFC 009 D10.4??
pub fn rewrite_program_span(program: Program, target: Span) -> Program {
    Program {
        items: program
            .items
            .into_iter()
            .map(|item| rewrite_item_span(item, target))
            .collect(),
    }
}

fn rewrite_item_span(item: Spanned<Item>, target: Span) -> Spanned<Item> {
    let Spanned { node, span: _ } = item;
    let new_node = match node {
        Item::Namespace(ns) => Item::Namespace(rewrite_namespace_span(ns, target)),
        Item::Use(u) => Item::Use(u), // Use ?? Ident ???? span
        Item::Struct(s) => Item::Struct(rewrite_struct_span(s, target)),
        Item::Class(c) => Item::Class(rewrite_class_span(c, target)),
        Item::Interface(i) => Item::Interface(rewrite_interface_span(i, target)),
        Item::Enum(e) => Item::Enum(rewrite_enum_span(e, target)),
        Item::Fn(f) => Item::Fn(rewrite_fn_span(f, target)),
        Item::Native(n) => Item::Native(n), // NativeModule ?? .ani????
        Item::Variant(v) => Item::Variant(v), // VariantDef ??? Span?????
        Item::Delegate(d) => Item::Delegate(d), // DelegateDef ?? Span?????
    };
    Spanned::new(new_node, target)
}

fn rewrite_namespace_span(ns: ast::NamespaceItem, target: Span) -> ast::NamespaceItem {
    ast::NamespaceItem {
        path: ns.path,
        items: ns
            .items
            .into_iter()
            .map(|i| rewrite_item_span(i, target))
            .collect(),
        capabilities: ns.capabilities,
    }
}

fn rewrite_struct_span(s: ast::StructDef, target: Span) -> ast::StructDef {
    ast::StructDef {
        vis: s.vis,
        is_readonly: s.is_readonly,
        is_record: s.is_record,
        name: s.name,
        generics: s.generics,
        where_clause: s
            .where_clause
            .into_iter()
            .map(|c| rewrite_type_constraint_span(c, target))
            .collect(),
        fields: s
            .fields
            .into_iter()
            .map(|f| rewrite_field_span(f, target))
            .collect(),
        bases: s.bases,
        properties: s.properties,
        methods: s.methods,
        constructors: s.constructors,
        attributes: s.attributes,
        doc: s.doc,
    }
}

fn rewrite_class_span(c: ast::ClassDef, target: Span) -> ast::ClassDef {
    ast::ClassDef {
        vis: c.vis,
        is_static: c.is_static,
        is_abstract: c.is_abstract,
        is_partial: c.is_partial,
        is_record: c.is_record,
        name: c.name,
        generics: c.generics,
        where_clause: c
            .where_clause
            .into_iter()
            .map(|c| rewrite_type_constraint_span(c, target))
            .collect(),
        bases: c
            .bases
            .into_iter()
            .map(|b| rewrite_type_node(b, target))
            .collect(),
        fields: c
            .fields
            .into_iter()
            .map(|f| rewrite_field_span(f, target))
            .collect(),
        properties: c
            .properties
            .into_iter()
            .map(|p| rewrite_property_span(p, target))
            .collect(),
        methods: c
            .methods
            .into_iter()
            .map(|m| rewrite_method_def_span(m, target))
            .collect(),
        constructors: c
            .constructors
            .into_iter()
            .map(|ctor| rewrite_ctor_def_span(ctor, target))
            .collect(),
        attributes: c.attributes,
        doc: c.doc,
        synthesized_host: c.synthesized_host,
    }
}

fn rewrite_interface_span(i: ast::InterfaceDef, target: Span) -> ast::InterfaceDef {
    ast::InterfaceDef {
        vis: i.vis,
        name: i.name,
        generics: i.generics,
        where_clause: i
            .where_clause
            .into_iter()
            .map(|c| rewrite_type_constraint_span(c, target))
            .collect(),
        bases: i
            .bases
            .into_iter()
            .map(|b| rewrite_type_node(b, target))
            .collect(),
        methods: i
            .methods
            .into_iter()
            .map(|m| rewrite_method_sig_span(m, target))
            .collect(),
        properties: i
            .properties
            .into_iter()
            .map(|p| rewrite_property_span(p, target))
            .collect(),
        attributes: i.attributes,
        doc: i.doc,
    }
}

fn rewrite_enum_span(e: ast::EnumDef, target: Span) -> ast::EnumDef {
    ast::EnumDef {
        vis: e.vis,
        name: e.name,
        variants: e
            .variants
            .into_iter()
            .map(|v| rewrite_enum_variant_span(v, target))
            .collect(),
        attributes: e.attributes,
        doc: e.doc,
    }
}

fn rewrite_enum_variant_span(v: ast::EnumVariant, target: Span) -> ast::EnumVariant {
    ast::EnumVariant {
        name: v.name,
        discriminant: v.discriminant,
        fields: v
            .fields
            .into_iter()
            .map(|f| rewrite_field_span(f, target))
            .collect(),
        attributes: v.attributes,
        doc: v.doc,
    }
}

fn rewrite_fn_span(f: ast::FnDef, target: Span) -> ast::FnDef {
    ast::FnDef {
        vis: f.vis,
        name: f.name,
        generics: f.generics,
        where_clause: f
            .where_clause
            .into_iter()
            .map(|c| rewrite_type_constraint_span(c, target))
            .collect(),
        params: f
            .params
            .into_iter()
            .map(|p| rewrite_param_span(p, target))
            .collect(),
        ret: f.ret.map(|t| rewrite_type_span(t, target)),
        body: f.body.map(|b| rewrite_block_span(b, target)),
        is_async: f.is_async,
        attributes: f.attributes,
        doc: f.doc,
    }
}

fn rewrite_method_def_span(m: Spanned<ast::MethodDef>, target: Span) -> Spanned<ast::MethodDef> {
    let Spanned { node, span: _ } = m;
    let new_node = ast::MethodDef {
        sig: rewrite_method_sig_inner(node.sig, target),
        body: node.body.map(|b| rewrite_block_span(b, target)),
        doc: node.doc,
    };
    Spanned::new(new_node, target)
}

fn rewrite_method_sig_span(m: ast::MethodSig, target: Span) -> ast::MethodSig {
    rewrite_method_sig_inner(m, target)
}

fn rewrite_method_sig_inner(m: ast::MethodSig, target: Span) -> ast::MethodSig {
    ast::MethodSig {
        vis: m.vis,
        name: m.name,
        generics: m.generics,
        where_clause: m
            .where_clause
            .into_iter()
            .map(|c| rewrite_type_constraint_span(c, target))
            .collect(),
        params: m
            .params
            .into_iter()
            .map(|p| rewrite_param_span(p, target))
            .collect(),
        ret: m.ret.map(|t| rewrite_type_span(t, target)),
        is_async: m.is_async,
        modifier: m.modifier,
        is_static_abstract: m.is_static_abstract,
        attributes: m.attributes,
        doc: m.doc,
    }
}

fn rewrite_ctor_def_span(
    ctor: Spanned<ast::ConstructorDef>,
    target: Span,
) -> Spanned<ast::ConstructorDef> {
    let Spanned { node, span: _ } = ctor;
    let new_node = ast::ConstructorDef {
        vis: node.vis,
        params: node
            .params
            .into_iter()
            .map(|p| rewrite_param_span(p, target))
            .collect(),
        body: rewrite_block_span(node.body, target),
        base_args: node.base_args,
        doc: node.doc,
    };
    Spanned::new(new_node, target)
}

fn rewrite_field_span(f: ast::FieldDef, target: Span) -> ast::FieldDef {
    ast::FieldDef {
        vis: f.vis,
        name: f.name,
        ty: rewrite_type_span(f.ty, target),
        is_readonly: f.is_readonly,
        is_const: f.is_const,
        is_static: f.is_static,
        init: f.init.map(|e| rewrite_expr_span(e, target)),
        attributes: f.attributes,
        doc: f.doc,
    }
}

fn rewrite_property_span(p: ast::PropertyDef, target: Span) -> ast::PropertyDef {
    ast::PropertyDef {
        vis: p.vis,
        name: p.name,
        ty: rewrite_type_span(p.ty, target),
        has_get: p.has_get,
        has_set: p.has_set,
        has_init: p.has_init,
        is_required: p.is_required,
        get_body: p.get_body.map(|b| rewrite_block_span(b, target)),
        set_body: p.set_body.map(|b| rewrite_block_span(b, target)),
        get_vis: p.get_vis,
        set_vis: p.set_vis,
        modifier: p.modifier,
        is_static_abstract: p.is_static_abstract,
        attributes: p.attributes,
        index_params: p
            .index_params
            .into_iter()
            .map(|ip| rewrite_param_span(ip, target))
            .collect(),
        init: p.init.map(|e| rewrite_expr_span(e, target)),
        doc: p.doc,
    }
}

fn rewrite_param_span(p: ast::Param, target: Span) -> ast::Param {
    ast::Param {
        name: p.name,
        ty: rewrite_type_span(p.ty, target),
        attributes: p.attributes,
        is_extension_receiver: p.is_extension_receiver,
        is_ref: p.is_ref,
        is_out: p.is_out,
        is_in: p.is_in,
        is_params: p.is_params,
        default: p.default.map(|e| rewrite_expr_span(e, target)),
    }
}

fn rewrite_type_constraint_span(c: ast::TypeConstraint, target: Span) -> ast::TypeConstraint {
    ast::TypeConstraint {
        param: c.param,
        kind: match c.kind {
            ast::ConstraintKind::Type(t) => ast::ConstraintKind::Type(rewrite_type_span(t, target)),
            ast::ConstraintKind::Class => ast::ConstraintKind::Class,
            ast::ConstraintKind::Struct => ast::ConstraintKind::Struct,
            ast::ConstraintKind::New => ast::ConstraintKind::New,
        },
    }
}

/// ??? `Type`?? `Spanned<Type>`????? `ClassDef.bases` ?
/// `InterfaceDef.bases` ????
fn rewrite_type_node(ty: Type, target: Span) -> Type {
    match ty {
        Type::Named { path, generics } => Type::Named {
            path,
            generics: generics
                .into_iter()
                .map(|g| rewrite_type_span(g, target))
                .collect(),
        },
        Type::Ref { inner, mutable } => Type::Ref {
            inner: Box::new(rewrite_type_span(*inner, target)),
            mutable,
        },
        Type::Func { params, ret } => Type::Func {
            params: params
                .into_iter()
                .map(|p| rewrite_type_span(p, target))
                .collect(),
            ret: Box::new(rewrite_type_span(*ret, target)),
        },
        Type::Array { inner } => Type::Array {
            inner: Box::new(rewrite_type_span(*inner, target)),
        },
        Type::Nullable { inner } => Type::Nullable {
            inner: Box::new(rewrite_type_span(*inner, target)),
        },
        Type::ConstInt(i) => Type::ConstInt(i),
        Type::Infer => Type::Infer,
    }
}

// ---------------------------------------------------------------------------
// ????
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DELEGATE_SPAN: Span = Span {
        file_id: 42,
        start: 100,
        end: 200,
    };

    fn parse_and_rewrite(src: &str) -> Vec<Spanned<Stmt>> {
        parse_expansion(src, TEST_DELEGATE_SPAN, 0).expect("parse should succeed")
    }

    #[test]
    fn parse_empty_string_yields_no_stmts() {
        let stmts = parse_and_rewrite("");
        assert!(stmts.is_empty());
    }

    #[test]
    fn parse_single_let_stmt_span_rewritten() {
        let stmts = parse_and_rewrite("var x = 1;");
        assert_eq!(stmts.len(), 1);
        // ?? span ?????????
        assert_eq!(stmts[0].span, TEST_DELEGATE_SPAN);
        // ?? init ??? span ????
        match &stmts[0].node {
            Stmt::Let { init: Some(e), .. } => {
                assert_eq!(e.span, TEST_DELEGATE_SPAN);
                match &e.node {
                    Expr::IntLit(i) => assert_eq!(*i, 1),
                    other => panic!("expected IntLit, got {other:?}"),
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn parse_string_concat_span_rewritten() {
        // 解析期将相邻字符串字面量 `+` 折叠为单个字面量（parse::expr 常量折叠），
        // 故 `"a" + "b"` 产出 `StringLit("ab")` 而非 Binary——span 仍须被重写。
        let stmts = parse_and_rewrite("var s = \"a\" + \"b\";");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].node {
            Stmt::Let { init: Some(e), .. } => {
                assert_eq!(e.span, TEST_DELEGATE_SPAN);
                match &e.node {
                    Expr::StringLit(s) => assert_eq!(s, "ab"),
                    other => panic!("expected StringLit(\"ab\"), got {other:?}"),
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn parse_method_call_chain_span_rewritten() {
        // ????????`sb.Append("a").Append("b");`
        let stmts = parse_and_rewrite("sb.Append(\"a\").Append(\"b\").ToString();");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].node {
            Stmt::Expr(e) => {
                assert_eq!(e.span, TEST_DELEGATE_SPAN);
                // ??????? receiver span
                match &e.node {
                    Expr::MethodCall { receiver, .. } => match &receiver.node {
                        Expr::MethodCall {
                            receiver: inner, ..
                        } => {
                            assert_eq!(inner.span, TEST_DELEGATE_SPAN);
                            match &inner.node {
                                Expr::MethodCall {
                                    receiver: innermost,
                                    ..
                                } => {
                                    assert_eq!(innermost.span, TEST_DELEGATE_SPAN);
                                    assert!(matches!(innermost.node, Expr::Ident(_)));
                                }
                                other => panic!("expected MethodCall, got {other:?}"),
                            }
                        }
                        other => panic!("expected MethodCall, got {other:?}"),
                    },
                    other => panic!("expected MethodCall, got {other:?}"),
                }
            }
            other => panic!("expected Expr stmt, got {other:?}"),
        }
    }

    #[test]
    fn parse_if_else_span_rewritten() {
        let stmts = parse_and_rewrite("if (true) { return \"a\"; } else { return \"b\"; }");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].node {
            Stmt::Expr(e) => match &e.node {
                Expr::If {
                    cond,
                    then_branch,
                    else_branch: Some(else_branch),
                } => {
                    assert_eq!(cond.span, TEST_DELEGATE_SPAN);
                    // then_branch ? return ?? span
                    let return_stmt = &then_branch.stmts[0];
                    assert_eq!(return_stmt.span, TEST_DELEGATE_SPAN);
                    // else_branch ? return ?? span
                    let else_return = &else_branch.stmts[0];
                    assert_eq!(else_return.span, TEST_DELEGATE_SPAN);
                }
                other => panic!("expected If, got {other:?}"),
            },
            other => panic!("expected Expr stmt, got {other:?}"),
        }
    }

    #[test]
    fn parse_new_expr_span_rewritten() {
        let stmts = parse_and_rewrite("var sb = new StringBuilder();");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].node {
            Stmt::Let { init: Some(e), .. } => match &e.node {
                Expr::New { ty, args, .. } => {
                    assert_eq!(ty.span, TEST_DELEGATE_SPAN);
                    assert!(args.is_empty());
                }
                other => panic!("expected New, got {other:?}"),
            },
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn parse_multiple_stmts_all_spans_rewritten() {
        let stmts = parse_and_rewrite("var a = 1; var b = 2; var c = a + b;");
        assert_eq!(stmts.len(), 3);
        for s in &stmts {
            assert_eq!(s.span, TEST_DELEGATE_SPAN);
        }
    }

    #[test]
    fn parse_foreach_loop_span_rewritten() {
        // ?????????????? splice ????????? Arc ??
        let stmts = parse_and_rewrite("foreach (var x in items) { Console.WriteLine(x); }");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].node {
            Stmt::For { var, iter, body } => {
                assert_eq!(iter.span, TEST_DELEGATE_SPAN);
                assert_eq!(var.as_str(), "x");
                assert_eq!(body.stmts[0].span, TEST_DELEGATE_SPAN);
            }
            other => panic!("expected For, got {other:?}"),
        }
    }

    #[test]
    fn parse_error_propagates_with_delegate_span() {
        let r = parse_expansion("var x = ;", TEST_DELEGATE_SPAN, 0);
        match r {
            Err(SpliceError::ParseError { delegate_span, .. }) => {
                assert_eq!(delegate_span, TEST_DELEGATE_SPAN);
            }
            other => panic!("expected ParseError, got {other:?}"),
        }
    }

    #[test]
    fn parse_assign_stmt_span_rewritten() {
        let stmts = parse_and_rewrite("x = 42;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].node {
            Stmt::Assign { target, value } => {
                assert_eq!(target.span, TEST_DELEGATE_SPAN);
                assert_eq!(value.span, TEST_DELEGATE_SPAN);
            }
            other => panic!("expected Assign, got {other:?}"),
        }
    }

    #[test]
    fn parse_return_stmt_span_rewritten() {
        let stmts = parse_and_rewrite("return \"hello\";");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].node {
            Stmt::Return(Some(e)) => {
                assert_eq!(e.span, TEST_DELEGATE_SPAN);
            }
            other => panic!("expected Return, got {other:?}"),
        }
    }

    #[test]
    fn parse_type_annotated_let_span_rewritten() {
        let stmts = parse_and_rewrite("string s = \"hello\";");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].node {
            Stmt::Let { ty: Some(t), .. } => {
                assert_eq!(t.span, TEST_DELEGATE_SPAN);
            }
            other => panic!("expected Let with ty, got {other:?}"),
        }
    }

    #[test]
    fn parse_call_with_type_args_span_rewritten() {
        // `new List<int>()` ????????????? type_args span ??
        let stmts = parse_and_rewrite("var xs = new List<int>();");
        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0].span, TEST_DELEGATE_SPAN);
        match &stmts[0].node {
            Stmt::Let { init: Some(e), .. } => match &e.node {
                Expr::New { ty, .. } => {
                    assert_eq!(ty.span, TEST_DELEGATE_SPAN);
                    // ?????? span ????
                    match &ty.node {
                        Type::Named { generics, .. } => {
                            assert_eq!(generics.len(), 1);
                            assert_eq!(generics[0].span, TEST_DELEGATE_SPAN);
                        }
                        other => panic!("expected Named type, got {other:?}"),
                    }
                }
                other => panic!("expected New, got {other:?}"),
            },
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn parse_complex_expansion_realistic() {
        // ?????????????? SQL ???
        let src = r#"var sb = new StringBuilder();
sb.Append("SELECT * FROM users");
if (true) {
    sb.Append(" WHERE age > 18");
}
sb.AppendLine();
return sb.ToString();"#;
        let stmts = parse_and_rewrite(src);
        assert_eq!(stmts.len(), 5);
        for s in &stmts {
            assert_eq!(s.span, TEST_DELEGATE_SPAN);
        }
    }
}
