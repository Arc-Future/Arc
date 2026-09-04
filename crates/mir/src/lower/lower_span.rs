//! RFC 005：`foreach` over `Span` / `ReadOnlySpan` → 索引 while（热路径零堆）。

use super::lower_call::lower_arg_operand;
use super::lower_type::*;
use super::*;

impl MirBuilder {
    /// Desugar `foreach (var x in span) { body }` into an index while loop.
    ///
    /// 热路径零分配：`Length` + `IndexGet`（GEP），**不**构造堆上
    /// `IEnumerator` / `GetEnumerator` 对象（与 List 索引快速路径同阶；显式
    /// `GetEnumerator` API 仍后置）。
    pub(super) fn lower_span_foreach(
        &mut self,
        var: &Ident,
        elem_ty: &TypeId,
        iter: &Spanned<Expr>,
        body: &TypedBlock,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        // foreach 变量自成一作用域（与 typeck `Stmt::For` 的 scope 对齐）。
        ctx.push_scope();
        let (mut iter_prep, span_op0) = lower_arg_operand(self, &iter.node, ctx);
        stmts.append(&mut iter_prep);

        let span_ty = infer_type_from_spanned(iter, ctx);
        let class = match &span_ty {
            TypeId::Span { mutable: false, .. } => "ReadOnlySpan",
            _ => "Span",
        };

        // IndexGet codegen 对 Local 的 `TypeId::Span` 分流；强制物化到局部。
        let span_local = match &span_op0 {
            MirOperand::Local(id) => *id,
            other => {
                let id = self.fresh_local(&"_span".into(), span_ty.clone(), ctx.locals);
                stmts.push(MirStatement::Assign {
                    place: id,
                    rvalue: MirRvalue::Use(other.clone()),
                });
                id
            }
        };
        let span_op = MirOperand::Local(span_local);

        let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
        let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
        let elem_local = self.fresh_local(var, elem_ty.clone(), ctx.locals);
        ctx.enter_loop_body();
        ctx.bind(var, elem_local);

        stmts.push(MirStatement::Assign {
            place: count_local,
            rvalue: MirRvalue::Use(MirOperand::Field {
                object: Box::new(span_op.clone()),
                class: class.into(),
                field: "Length".into(),
            }),
        });
        stmts.push(MirStatement::Assign {
            place: idx_local,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });

        let mut while_body = Vec::new();
        while_body.push(MirStatement::Assign {
            place: elem_local,
            rvalue: MirRvalue::IndexGet {
                array: span_op.clone(),
                index: MirOperand::Local(idx_local),
                elem_type: elem_ty.clone(),
            },
        });
        while_body.extend(self.lower_typed_block(body, ctx));
        ctx.exit_loop_body();
        while_body.push(MirStatement::Assign {
            place: idx_local,
            rvalue: MirRvalue::Binary {
                op: BinOp::Add,
                left: MirOperand::Local(idx_local),
                right: MirOperand::ConstInt(1),
            },
        });

        stmts.push(MirStatement::While {
            cond: MirRvalue::Binary {
                op: BinOp::Lt,
                left: MirOperand::Local(idx_local),
                right: MirOperand::Local(count_local),
            },
            body: while_body,
            foreach_source: Some(span_op.clone()),
        });
        ctx.pop_scope();
    }

    /// Untyped-path variant of [`lower_span_foreach`] for lambda bodies.
    pub(super) fn lower_span_foreach_untyped(
        &mut self,
        var: &Ident,
        iter: &Spanned<Expr>,
        body: &Block,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        // foreach 变量自成一作用域（与 typed 路径 / typeck `Stmt::For` 对齐）。
        ctx.push_scope();
        let (mut iter_prep, span_op0) = lower_arg_operand(self, &iter.node, ctx);
        stmts.append(&mut iter_prep);

        let span_ty = infer_type_from_spanned(iter, ctx);
        let elem_ty = span_ty.enumerable_elem().unwrap_or(TypeId::Infer);
        let class = match &span_ty {
            TypeId::Span { mutable: false, .. } => "ReadOnlySpan",
            _ => "Span",
        };

        let span_local = match &span_op0 {
            MirOperand::Local(id) => *id,
            other => {
                let id = self.fresh_local(&"_span".into(), span_ty.clone(), ctx.locals);
                stmts.push(MirStatement::Assign {
                    place: id,
                    rvalue: MirRvalue::Use(other.clone()),
                });
                id
            }
        };
        let span_op = MirOperand::Local(span_local);

        let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
        let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
        let elem_local = self.fresh_local(var, elem_ty.clone(), ctx.locals);
        ctx.enter_loop_body();
        ctx.bind(var, elem_local);

        stmts.push(MirStatement::Assign {
            place: count_local,
            rvalue: MirRvalue::Use(MirOperand::Field {
                object: Box::new(span_op.clone()),
                class: class.into(),
                field: "Length".into(),
            }),
        });
        stmts.push(MirStatement::Assign {
            place: idx_local,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });

        let mut while_body = Vec::new();
        while_body.push(MirStatement::Assign {
            place: elem_local,
            rvalue: MirRvalue::IndexGet {
                array: span_op.clone(),
                index: MirOperand::Local(idx_local),
                elem_type: elem_ty,
            },
        });
        while_body.extend(self.lower_block(body, ctx));
        ctx.exit_loop_body();
        while_body.push(MirStatement::Assign {
            place: idx_local,
            rvalue: MirRvalue::Binary {
                op: BinOp::Add,
                left: MirOperand::Local(idx_local),
                right: MirOperand::ConstInt(1),
            },
        });

        stmts.push(MirStatement::While {
            cond: MirRvalue::Binary {
                op: BinOp::Lt,
                left: MirOperand::Local(idx_local),
                right: MirOperand::Local(count_local),
            },
            body: while_body,
            foreach_source: Some(span_op.clone()),
        });
        ctx.pop_scope();
    }
}
