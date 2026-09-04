use crate::TypeId;
use ast::{Expr, LambdaBody, LambdaExpr, QueryClause, Type};

use super::checker::BorrowChecker;
use super::error::BorrowError;

impl BorrowChecker {
    /// Check expression; when `as_move_operand` is true, struct-typed idents are moved.
    pub(crate) fn check_expr(&mut self, expr: &Expr, as_move_operand: bool) -> TypeId {
        match expr {
            Expr::Ident(name) => {
                self.use_ident(name);
                let ty = self
                    .bindings
                    .get(name)
                    .map(|b| b.ty.clone())
                    .unwrap_or(TypeId::Infer);
                if as_move_operand && self.is_move_type(&ty) {
                    self.mark_moved(name);
                }
                ty
            }
            Expr::CollectionExpr { elements } => {
                for el in elements {
                    self.check_expr(&el.expr().node, false);
                }
                TypeId::Infer
            }
            Expr::New { ty, args, obj_init } => {
                for a in args {
                    self.check_expr(&a.node, true);
                }
                if let Some(fields) = obj_init {
                    for (_, v) in fields {
                        self.check_expr(&v.node, false);
                    }
                }
                self.lower_type_name(&ty.node)
            }
            Expr::Lambda(l) => {
                self.check_lambda(l);
                TypeId::Infer
            }
            Expr::Call { func, args, .. } => {
                self.check_expr(&func.node, false);
                let params = if let Expr::Ident(name) = &func.node {
                    self.fn_sigs.get(name).cloned().unwrap_or_default()
                } else {
                    vec![]
                };
                for (i, a) in args.iter().enumerate() {
                    let move_arg = params.get(i).map(|p| self.is_move_type(p)).unwrap_or(false);
                    self.check_expr(&a.node, move_arg);
                }
                TypeId::Void
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.check_expr(&receiver.node, false);
                for a in args {
                    self.check_expr(&a.node, false);
                }
                TypeId::Void
            }
            Expr::Query(q) => {
                for c in &q.clauses {
                    match c {
                        QueryClause::From { source, .. } => {
                            self.check_expr(&source.node, false);
                        }
                        QueryClause::Where(e) => {
                            self.check_expr(&e.node, false);
                        }
                        QueryClause::Let { value, .. } => {
                            self.check_expr(&value.node, false);
                        }
                        QueryClause::OrderBy { key, .. } => {
                            self.check_expr(&key.node, false);
                        }
                        QueryClause::Join {
                            source,
                            on_left,
                            on_right,
                            ..
                        } => {
                            self.check_expr(&source.node, false);
                            self.check_expr(&on_left.node, false);
                            self.check_expr(&on_right.node, false);
                        }
                        QueryClause::GroupBy { key, element, .. } => {
                            self.check_expr(&key.node, false);
                            if let Some(el) = element {
                                self.check_expr(&el.node, false);
                            }
                        }
                    }
                }
                self.check_expr(&q.select.node, false);
                TypeId::Void
            }
            Expr::Binary { left, right, .. } => {
                self.check_expr(&left.node, false);
                self.check_expr(&right.node, false);
                TypeId::Int
            }
            Expr::Await(inner) => self.check_expr(&inner.node, false),
            Expr::Field { receiver, .. } => {
                self.check_expr(&receiver.node, false);
                TypeId::Infer
            }
            Expr::Block(b) => {
                self.check_block(b);
                TypeId::Void
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(&cond.node, false);
                self.check_block(then_branch);
                if let Some(e) = else_branch {
                    self.check_block(e);
                }
                TypeId::Void
            }
            Expr::Switch(s) => {
                self.check_expr(&s.scrutinee.node, false);
                for case in &s.cases {
                    if let Some(w) = &case.when {
                        self.check_expr(&w.node, false);
                    }
                    self.check_block(&case.body);
                }
                TypeId::Void
            }
            Expr::SwitchForm(s) => {
                self.check_expr(&s.scrutinee.node, false);
                let mut ty = TypeId::Infer;
                for arm in &s.arms {
                    if let Some(w) = &arm.when {
                        self.check_expr(&w.node, false);
                    }
                    ty = self.check_expr(&arm.body.node, false);
                }
                ty
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(&cond.node, false);
                let tt = self.check_expr(&then_branch.node, false);
                let et = self.check_expr(&else_branch.node, false);
                if tt == et {
                    tt
                } else {
                    // 分支类型不一致——本 pass 不做类型诊断（主 checker 负责），如实返回未知
                    TypeId::Infer
                }
            }
            _ => TypeId::Infer,
        }
    }

    fn check_lambda(&mut self, lambda: &LambdaExpr) {
        match &lambda.body {
            LambdaBody::Expr(e) => {
                self.check_expr(&e.node, false);
            }
            LambdaBody::Block(b) => self.check_block(b),
        }
    }

    pub(crate) fn use_ident(&mut self, name: &ast::Ident) {
        if let Some(b) = self.bindings.get(name) {
            if b.ownership == super::binding::Ownership::Moved {
                self.errors
                    .push(BorrowError::UseAfterMove(name.to_string()));
            }
        }
    }

    pub(crate) fn mark_moved(&mut self, name: &ast::Ident) {
        let should_move = self
            .bindings
            .get(name)
            .map(|b| self.is_move_type(&b.ty))
            .unwrap_or(false);
        if should_move {
            if let Some(b) = self.bindings.get_mut(name) {
                b.ownership = super::binding::Ownership::Moved;
            }
        }
    }

    fn lower_type_name(&self, ty: &Type) -> TypeId {
        match ty {
            Type::Named { path, .. } => {
                let name = path.last().cloned().unwrap_or_else(|| "unknown".into());
                TypeId::Named(name)
            }
            _ => TypeId::Infer,
        }
    }
}
