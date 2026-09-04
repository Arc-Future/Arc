use crate::TypeId;
use ast::{Block, Expr, Stmt};

use super::binding::{Binding, Ownership};
use super::checker::BorrowChecker;

impl BorrowChecker {
    pub(crate) fn check_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.check_stmt(&stmt.node);
        }
        if let Some(tail) = &block.tail {
            self.check_expr(&tail.node, false);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, init, .. } => {
                if let Some(init) = init {
                    let ty = self.check_expr(&init.node, true);
                    self.bindings.insert(
                        name.clone(),
                        Binding {
                            ty,
                            ownership: Ownership::Owned,
                        },
                    );
                } else {
                    self.bindings.insert(
                        name.clone(),
                        Binding {
                            ty: TypeId::Infer,
                            ownership: Ownership::Owned,
                        },
                    );
                }
            }
            Stmt::Expr(e) => {
                self.check_expr(&e.node, false);
            }
            Stmt::Return(val) => {
                if let Some(v) = val {
                    self.check_expr(&v.node, false);
                }
            }
            Stmt::While { cond, body } => {
                self.check_expr(&cond.node, false);
                self.check_block(body);
            }
            Stmt::For { iter, body, .. } => {
                self.check_expr(&iter.node, false);
                self.check_block(body);
            }
            Stmt::ForC {
                init,
                cond,
                inc,
                body,
            } => {
                if let Some(s) = init {
                    self.check_stmt(&s.node);
                }
                if let Some(e) = cond {
                    self.check_expr(&e.node, false);
                }
                if let Some(s) = inc {
                    self.check_stmt(&s.node);
                }
                self.check_block(body);
            }
            Stmt::Assign { target, value } => {
                if let (Expr::Ident(dst), Expr::Ident(src)) = (&target.node, &value.node) {
                    self.use_ident(src);
                    if let Some(src_binding) = self.bindings.get(src).cloned() {
                        if self.is_move_type(&src_binding.ty) {
                            self.mark_moved(src);
                        }
                        if let Some(dst_binding) = self.bindings.get_mut(dst) {
                            dst_binding.ty = src_binding.ty;
                        }
                    }
                } else {
                    self.check_expr(&target.node, false);
                    self.check_expr(&value.node, false);
                }
            }
            Stmt::Break | Stmt::Continue => {}
            Stmt::Throw { expr } => {
                self.check_expr(&expr.node, false);
            }
            Stmt::TryCatch {
                try_body,
                catch_name,
                when_cond,
                catch_body,
                finally,
                ..
            } => {
                self.check_block(try_body);
                self.bindings.insert(
                    catch_name.clone(),
                    Binding {
                        ty: TypeId::Infer,
                        ownership: Ownership::Owned,
                    },
                );
                if let Some(w) = when_cond {
                    self.check_expr(&w.node, false);
                }
                self.check_block(catch_body);
                if let Some(f) = finally {
                    self.check_block(f);
                }
            }
            Stmt::TryFinally { body, finally } => {
                self.check_block(body);
                self.check_block(finally);
            }
            Stmt::Using {
                name, init, body, ..
            } => {
                let ty = self.check_expr(&init.node, true);
                self.bindings.insert(
                    name.clone(),
                    Binding {
                        ty,
                        ownership: Ownership::Owned,
                    },
                );
                self.check_block(body);
            }
            Stmt::UsingVar { name, init, .. } => {
                let ty = self.check_expr(&init.node, true);
                self.bindings.insert(
                    name.clone(),
                    Binding {
                        ty,
                        ownership: Ownership::Owned,
                    },
                );
            }
            Stmt::AwaitUsing {
                name, init, body, ..
            } => {
                let ty = self.check_expr(&init.node, true);
                self.bindings.insert(
                    name.clone(),
                    Binding {
                        ty,
                        ownership: Ownership::Owned,
                    },
                );
                self.check_block(body);
            }
            Stmt::AwaitUsingVar { name, init, .. } => {
                let ty = self.check_expr(&init.node, true);
                self.bindings.insert(
                    name.clone(),
                    Binding {
                        ty,
                        ownership: Ownership::Owned,
                    },
                );
            }
            // RFC 044：yield 脱糖后不再到达此处；值表达式按只读使用检查。
            Stmt::YieldReturn { value } => {
                self.check_expr(&value.node, false);
            }
            Stmt::YieldBreak => {}
            Stmt::Lock { expr, body } => {
                let _ = self.check_expr(&expr.node, false);
                self.check_block(body);
            }
            // RFC 004 M2/M7锛氬０鏄庣洰鏍囪繘 bindings锛堝惈宓屽锛夛紱寮冨厓涓嶇粦瀹氾紱鏍￠獙鍙冲€笺€?
            Stmt::DeconstructAssign {
                declare,
                targets,
                value,
            } => {
                self.check_expr(&value.node, false);
                if *declare {
                    let mut binds = Vec::new();
                    for t in targets {
                        t.collect_binds(&mut binds);
                    }
                    for name in binds {
                        self.bindings.insert(
                            name,
                            Binding {
                                ty: TypeId::Infer,
                                ownership: Ownership::Owned,
                            },
                        );
                    }
                }
            }
        }
    }
}
