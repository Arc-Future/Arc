use super::lower_call::*;
use super::lower_expr::*;
use super::lower_type::*;
use super::*;

/// 物化流的叶子目标：把当前元素追加进列表，或把 groupby 的 key/item
/// 求值后写入 pair 缓冲（在流式作用域内求值，`let`/`join` 引入的局部仍存活）。
enum LinqTarget<'a> {
    Append {
        result_list: LocalId,
        result_class: &'a str,
    },
    GroupByPair {
        key_list: LocalId,
        item_list: LocalId,
        key_ty: &'a TypeId,
        item_ty: &'a TypeId,
        key_lambda: &'a LambdaExpr,
        item_lambda: Option<&'a LambdaExpr>,
    },
}

/// 首个需要缓冲的算子类别（OrderBy 排序 / GroupBy 分组）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum BufKind {
    OrderBy,
    GroupBy,
}

/// 需缓冲物化的 LINQ 算子（OrderBy 排序 / GroupBy 分组 / Join 内层迭代）。
/// foreach 与终端两条路径共用此判定：这些算子无法在流式 foreach 中原位
/// 展开（Join 需对每个外层元素重扫内层源），必须经
/// `materialize_linq_chain_to_list` 物化后再续流——单一事实来源，禁止
/// 各路径自维护活动清单（漏判一处即静默跳过、结果错误）。
fn linq_op_requires_materialization(op: &LinqOp) -> bool {
    matches!(
        op,
        LinqOp::OrderBy { .. } | LinqOp::GroupBy { .. } | LinqOp::Join { .. }
    )
}

/// GroupBy 物化计划：pair 缓冲 local + 分组产物类型信息。
struct LinqGroupByPlan {
    pair_keys: LocalId,
    pair_items: LocalId,
    key_ty: TypeId,
    item_ty: TypeId,
    /// 分组元素类型 `Grouping_<K,T>`（resume 续流的元素类型；`groups_class`
    /// 是承载它的 `List_<Grouping_<K,T>>`，二者不可混淆）。
    group_ty: TypeId,
    groups_class: String,
    key_lambda: LambdaExpr,
    item_lambda: Option<LambdaExpr>,
}

impl MirBuilder {
    /// Desugar `foreach (var x in list) { body }` into an index-based while loop.
    /// Used when the iterable is a `List<T>` (non-LINQ path). RFC 007 §9.
    pub(super) fn lower_list_foreach(
        &mut self,
        var: &Ident,
        elem_ty: &TypeId,
        iter: &Spanned<Expr>,
        body: &TypedBlock,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        // foreach 变量自成一作用域（与 typeck `Stmt::For` 的 scope 对齐）：防止
        // 循环体内嵌套同名变量劫持本层 `var` 的后续解析（for 循环同款根因）。
        ctx.push_scope();
        let (mut iter_prep, list_op) = lower_arg_operand(self, &iter.node, ctx);
        stmts.append(&mut iter_prep);

        let recv_type = class_from_expr(&iter.node, ctx);

        let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
        let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
        let elem_local = self.fresh_local(var, elem_ty.clone(), ctx.locals);
        ctx.enter_loop_body();
        ctx.bind(var, elem_local);

        stmts.push(MirStatement::Assign {
            place: count_local,
            rvalue: MirRvalue::MethodCall {
                receiver: list_op.clone(),
                method: "get_Count".to_string(),
                args: vec![],
                receiver_type: recv_type.clone(),
                impl_class: Some(recv_type.clone()),
                target_fn: Some(format!("{}::get_Count", recv_type)),
                is_virtual: false,
                params: vec![],
            },
        });
        stmts.push(MirStatement::Assign {
            place: idx_local,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });

        let mut while_body = Vec::new();
        while_body.push(MirStatement::Assign {
            place: elem_local,
            rvalue: MirRvalue::MethodCall {
                receiver: list_op.clone(),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(idx_local)],
                receiver_type: recv_type.clone(),
                impl_class: Some(recv_type.clone()),
                target_fn: Some(format!("{}::get_Item", recv_type)),
                is_virtual: false,
                params: vec![],
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
            foreach_source: Some(list_op.clone()),
        });
        ctx.pop_scope();
    }

    /// Expand `foreach (var x in <linq chain>) { body }` into an indexed
    /// while-loop, inlining Where/Select lambdas.
    ///
    /// Two source kinds are supported:
    /// - **Compile-time array** (`source_len = Some(n)`): uses `IndexGet` and
    ///   a constant count.
    /// - **Runtime `List<T>`** (`source_len = None` but source type is
    ///   `List_<T>`): uses `get_Count` / `Get(i)` method calls so Where/Select
    ///   operators are applied per element. Without this path the fallback
    ///   `LinqForeach` statement would emit a plain indexed loop that ignores
    ///   all LINQ operators.
    pub(super) fn lower_linq_foreach(
        &mut self,
        var: &Ident,
        chain: LinqChain,
        body: &TypedBlock,
        elem_ty: &TypeId,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        // foreach 变量自成一作用域（与 typeck `Stmt::For` 的 scope 对齐）。
        ctx.push_scope();
        // OrderBy / GroupBy → materialize the chain into a temp List, then
        // re-enumerate it with the Case 2 List loop over `operators = []`.
        // The source must be a known array / List; otherwise the materializer
        // would leave the temp null and the resume loop would crash.
        if (chain.source_len.is_some() || list_source_info(&chain.source, ctx).is_some())
            && chain.operators.iter().any(linq_op_requires_materialization)
        {
            let temp_list_ty =
                TypeId::Named(mangle_generic("List", std::slice::from_ref(elem_ty)).into());
            let temp_list =
                self.fresh_local(&"_linq_sorted".into(), temp_list_ty.clone(), ctx.locals);
            let var_local = self.fresh_local(var, elem_ty.clone(), ctx.locals);
            ctx.enter_loop_body();
            ctx.bind(var, var_local);
            let body_stmts = self.lower_typed_block(body, ctx);
            ctx.exit_loop_body();

            self.materialize_linq_chain_to_list(chain, temp_list, &temp_list_ty, ctx, stmts);

            let resume = LinqChain {
                source: MirOperand::Local(temp_list),
                source_len: None,
                operators: vec![],
            };
            self.emit_linq_source_loop(
                &resume, elem_ty, var_local, elem_ty, body_stmts, ctx, stmts,
            );
            ctx.pop_scope();
            return;
        }

        // Case 1: compile-time array — use IndexGet with constant count.
        if let Some(count) = chain.source_len {
            let src_elem_ty = source_elem_ty(&chain.source, ctx).unwrap_or(TypeId::Infer);

            let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
            let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
            let elem_local = self.fresh_local(&"_elem".into(), src_elem_ty.clone(), ctx.locals);

            stmts.push(MirStatement::Assign {
                place: count_local,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(count as i64)),
            });
            stmts.push(MirStatement::Assign {
                place: idx_local,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
            });

            let var_local = self.fresh_local(var, elem_ty.clone(), ctx.locals);
            ctx.enter_loop_body();
            ctx.bind(var, var_local);

            let body_stmts = self.lower_typed_block(body, ctx);
            ctx.exit_loop_body();

            let mut while_body = Vec::new();
            while_body.push(MirStatement::Assign {
                place: elem_local,
                rvalue: MirRvalue::IndexGet {
                    array: chain.source.clone(),
                    index: MirOperand::Local(idx_local),
                    elem_type: src_elem_ty.clone(),
                },
            });

            self.apply_linq_ops(
                &chain.operators,
                0,
                elem_local,
                &src_elem_ty,
                var_local,
                elem_ty,
                body_stmts,
                ctx,
                &mut while_body,
            );

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
                foreach_source: Some(chain.source.clone()),
            });
            ctx.pop_scope();
            return;
        }

        // Case 2: runtime List<T> — expand to while loop with Get/Count.
        let list_info = list_source_info(&chain.source, ctx);
        if let Some((recv_type, src_elem_ty)) = list_info {
            let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
            let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
            let elem_local = self.fresh_local(&"_elem".into(), src_elem_ty.clone(), ctx.locals);

            // count = source.Count
            stmts.push(MirStatement::Assign {
                place: count_local,
                rvalue: MirRvalue::MethodCall {
                    receiver: chain.source.clone(),
                    method: "get_Count".to_string(),
                    args: vec![],
                    receiver_type: recv_type.clone(),
                    impl_class: Some(recv_type.clone()),
                    target_fn: Some(format!("{}::get_Count", recv_type)),
                    is_virtual: false,
                    params: vec![],
                },
            });
            // idx = 0
            stmts.push(MirStatement::Assign {
                place: idx_local,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
            });

            let var_local = self.fresh_local(var, elem_ty.clone(), ctx.locals);
            ctx.enter_loop_body();
            ctx.bind(var, var_local);

            let body_stmts = self.lower_typed_block(body, ctx);
            ctx.exit_loop_body();

            let mut while_body = Vec::new();
            // elem = source.Get(idx)
            while_body.push(MirStatement::Assign {
                place: elem_local,
                rvalue: MirRvalue::MethodCall {
                    receiver: chain.source.clone(),
                    method: "get_Item".to_string(),
                    args: vec![MirOperand::Local(idx_local)],
                    receiver_type: recv_type.clone(),
                    impl_class: Some(recv_type.clone()),
                    target_fn: Some(format!("{}::get_Item", recv_type)),
                    is_virtual: false,
                    params: vec![],
                },
            });

            self.apply_linq_ops(
                &chain.operators,
                0,
                elem_local,
                &src_elem_ty,
                var_local,
                elem_ty,
                body_stmts,
                ctx,
                &mut while_body,
            );

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
                foreach_source: Some(chain.source.clone()),
            });
            ctx.pop_scope();
            return;
        }

        // Case 3: fallback — emit LinqForeach for codegen to handle.
        stmts.push(MirStatement::LinqForeach {
            var: var.clone(),
            chain,
            body: self.lower_loop_typed_block(body, ctx),
        });
        ctx.pop_scope();
    }

    /// Recursively apply LINQ operators to the current element, building
    /// nested `If` blocks for `Where` filters and chaining `Select` projections.
    fn apply_linq_ops(
        &mut self,
        ops: &[LinqOp],
        idx: usize,
        current: LocalId,
        current_ty: &TypeId,
        var_local: LocalId,
        var_ty: &TypeId,
        body_stmts: Vec<MirStatement>,
        ctx: &mut LowerCtx,
        out: &mut Vec<MirStatement>,
    ) {
        if idx >= ops.len() {
            out.push(MirStatement::Assign {
                place: var_local,
                rvalue: MirRvalue::Use(MirOperand::Local(current)),
            });
            out.extend(body_stmts);
            return;
        }

        match &ops[idx] {
            LinqOp::Where(lambda) => {
                ctx.push_scope();
                let param_name = lambda.params[0].name.clone();
                let param_local = self.fresh_local(&param_name, current_ty.clone(), ctx.locals);
                ctx.bind(&param_name, param_local);

                out.push(MirStatement::Assign {
                    place: param_local,
                    rvalue: MirRvalue::Use(MirOperand::Local(current)),
                });

                let mut cond_prep = Vec::new();
                let cond_rv = match &lambda.body {
                    LambdaBody::Expr(e) => {
                        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        cond_prep.append(&mut prep);
                        rv
                    }
                    LambdaBody::Block(_) => MirRvalue::Use(MirOperand::ConstBool(true)),
                };
                let cond_local = self.fresh_local(&"_where".into(), TypeId::Bool, ctx.locals);
                out.append(&mut cond_prep);
                out.push(MirStatement::Assign {
                    place: cond_local,
                    rvalue: cond_rv,
                });

                let mut then_body = Vec::new();
                self.apply_linq_ops(
                    ops,
                    idx + 1,
                    current,
                    current_ty,
                    var_local,
                    var_ty,
                    body_stmts,
                    ctx,
                    &mut then_body,
                );

                out.push(MirStatement::If {
                    cond: MirOperand::Local(cond_local),
                    then_body,
                    else_body: vec![],
                });

                ctx.pop_scope();
            }
            LinqOp::Select(lambda) => {
                ctx.push_scope();
                let param_name = lambda.params[0].name.clone();
                let param_local = self.fresh_local(&param_name, current_ty.clone(), ctx.locals);
                ctx.bind(&param_name, param_local);

                out.push(MirStatement::Assign {
                    place: param_local,
                    rvalue: MirRvalue::Use(MirOperand::Local(current)),
                });

                let mut new_prep = Vec::new();
                let new_rv = match &lambda.body {
                    LambdaBody::Expr(e) => {
                        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        new_prep.append(&mut prep);
                        rv
                    }
                    LambdaBody::Block(_) => MirRvalue::Use(MirOperand::Local(current)),
                };
                let new_local = self.fresh_local(&"_sel".into(), var_ty.clone(), ctx.locals);
                out.append(&mut new_prep);
                out.push(MirStatement::Assign {
                    place: new_local,
                    rvalue: new_rv,
                });

                self.apply_linq_ops(
                    ops,
                    idx + 1,
                    new_local,
                    var_ty,
                    var_local,
                    var_ty,
                    body_stmts,
                    ctx,
                    out,
                );

                ctx.pop_scope();
            }
            LinqOp::OrderBy { .. } => {
                // OrderBy requires materialization; skip in streaming foreach.
                self.apply_linq_ops(
                    ops,
                    idx + 1,
                    current,
                    current_ty,
                    var_local,
                    var_ty,
                    body_stmts,
                    ctx,
                    out,
                );
            }
            LinqOp::Let { ident, value } => {
                // `let` 只引入绑定、元素本身继续前流——与物化路径语义一致，
                // foreach 下同样可原位展开。
                ctx.push_scope();
                let param_name = value.params[0].name.clone();
                let param_local = self.fresh_local(&param_name, current_ty.clone(), ctx.locals);
                ctx.bind(&param_name, param_local);
                out.push(MirStatement::Assign {
                    place: param_local,
                    rvalue: MirRvalue::Use(MirOperand::Local(current)),
                });
                let value_ty = match &value.body {
                    LambdaBody::Expr(e) => infer_type_from_spanned(e, ctx),
                    LambdaBody::Block(_) => current_ty.clone(),
                };
                let v_local = self.fresh_local(ident, value_ty, ctx.locals);
                let mut v_prep = Vec::new();
                let v_rv = match &value.body {
                    LambdaBody::Expr(e) => {
                        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        v_prep.append(&mut prep);
                        rv
                    }
                    LambdaBody::Block(_) => MirRvalue::Use(MirOperand::Local(current)),
                };
                out.append(&mut v_prep);
                out.push(MirStatement::Assign {
                    place: v_local,
                    rvalue: v_rv,
                });
                ctx.bind(ident, v_local);
                self.apply_linq_ops(
                    ops,
                    idx + 1,
                    current,
                    current_ty,
                    var_local,
                    var_ty,
                    body_stmts,
                    ctx,
                    out,
                );
                ctx.pop_scope();
            }
            LinqOp::Join { .. } | LinqOp::GroupBy { .. } => {
                // 不应到达：Join / GroupBy 已在 `lower_linq_foreach` / 终端
                // 路径经 `linq_op_requires_materialization` 统一路由到物化
                // 机制，流式 foreach 只承载 Where/Select/Let。若仍到达说明
                // 上游路由漏判——必须大声失败，禁止静默跳过产生错误结果。
                unreachable!(
                    "Join/GroupBy 必须在流式展开前路由到物化路径（linq_op_requires_materialization）"
                );
            }
        }
    }

    /// Materialize a `LinqChain` into a fresh `List<T>` local.
    ///
    /// Used by `lower_let` when the right-hand side is a LINQ method chain
    /// (e.g. `let r = list.Where(p).Select(s);`) or a query expression. The
    /// chain is expanded into:
    ///
    /// ```text
    /// %result = new List_<T>()
    /// %count = source.Count
    /// %idx = 0
    /// while %idx < %count {
    ///     %elem = source.Get(%idx)   // or IndexGet for compile-time arrays
    ///     // apply Where/Select lambdas inline
    ///     call %result.Add(transformed_elem)
    ///     %idx = %idx + 1
    /// }
    /// ```
    ///
    /// Without this, `MirRvalue::LinqChain` falls through to a codegen stub
    /// (`alloca i8`) that returns an invalid pointer and crashes at runtime.
    /// `OrderBy`/`GroupBy` are buffered (see `materialize_chain_inner`); when a
    /// key/element captures or is unsupported they are honestly skipped and
    /// elements keep their source order (documented subset, same as the
    /// `List.Sort(cmp)` no-capture limitation).
    pub(super) fn materialize_linq_chain_to_list(
        &mut self,
        chain: LinqChain,
        result_list: LocalId,
        result_list_ty: &TypeId,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        let result_class = match result_list_ty {
            TypeId::Named(n) => n.to_string(),
            _ => return,
        };

        // result = new List_<T>()
        stmts.push(MirStatement::Assign {
            place: result_list,
            rvalue: MirRvalue::New {
                class: result_class.clone(),
                args: vec![],
                ctor_params: vec![],
            },
        });

        let src_elem_ty = source_elem_ty(&chain.source, ctx)
            .or_else(|| list_source_info(&chain.source, ctx).map(|(_, e)| e));
        let Some(src_elem_ty) = src_elem_ty else {
            // 无源信息时结果列表保持空（best-effort；同原有 Case 3 行为）。
            return;
        };
        self.materialize_chain_inner(chain, src_elem_ty, result_list, &result_class, ctx, stmts);
    }

    /// 递归物化核心：处理第一个缓冲算子（OrderBy / GroupBy）后对 resume 链
    /// 递归，终止于纯流式 `emit_materialize_ops_loop`。
    ///
    /// - **多键 OrderBy**：连续 OrderBy run 折叠为单 comparator（ThenBy 语义，
    ///   无需依赖 qsort 稳定性）；非连续 OrderBy 由递归各自独立排序。
    /// - **GroupBy**：Phase A 在流式叶子求值 key/item（`let`/`join` 作用域
    ///   存活）灌入 pair 缓冲；Phase B 顺序扫描构建分组（首次出现序）；
    ///   Phase C 递归 resume 后续算子。
    /// - prepare 失败（不可排序 key / 捕获 key）→ 流式回退，OrderBy/GroupBy
    ///   在 `materialize_linq_ops` 中被跳过（诚实子集，同既有 OrderBy 限制）。
    fn materialize_chain_inner(
        &mut self,
        chain: LinqChain,
        src_elem_ty: TypeId,
        result_list: LocalId,
        result_class: &str,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        let first_buf = chain
            .operators
            .iter()
            .enumerate()
            .find_map(|(i, op)| match op {
                LinqOp::OrderBy { .. } => Some((i, BufKind::OrderBy)),
                LinqOp::GroupBy { .. } => Some((i, BufKind::GroupBy)),
                _ => None,
            });
        let Some((k, kind)) = first_buf else {
            self.emit_materialize_ops_loop(
                &chain.source,
                chain.source_len,
                &src_elem_ty,
                &chain.operators,
                &LinqTarget::Append {
                    result_list,
                    result_class,
                },
                ctx,
                stmts,
            );
            return;
        };

        match kind {
            BufKind::OrderBy => {
                let mut run_end = k;
                while run_end + 1 < chain.operators.len()
                    && matches!(chain.operators[run_end + 1], LinqOp::OrderBy { .. })
                {
                    run_end += 1;
                }
                if let Some((buffer, buffer_class, buffer_elem_ty, cmp)) =
                    self.prepare_linq_sort(&chain, k, run_end, &src_elem_ty, ctx, stmts)
                {
                    self.emit_materialize_ops_loop(
                        &chain.source,
                        chain.source_len,
                        &src_elem_ty,
                        &chain.operators[..k],
                        &LinqTarget::Append {
                            result_list: buffer,
                            result_class: &buffer_class,
                        },
                        ctx,
                        stmts,
                    );
                    // buffer.Sort(cmp) — rt_list_sort 原位排序（qsort）。
                    let sort_place = self.fresh_local(&"_sort".into(), TypeId::Void, ctx.locals);
                    stmts.push(MirStatement::Assign {
                        place: sort_place,
                        rvalue: MirRvalue::MethodCall {
                            receiver: MirOperand::Local(buffer),
                            method: "Sort".to_string(),
                            args: vec![cmp],
                            receiver_type: buffer_class,
                            impl_class: None,
                            target_fn: None,
                            is_virtual: false,
                            params: vec![],
                        },
                    });
                    // 排序后 resume：若缓冲前缀含 join，inner 绑定在缓冲中被丢弃，
                    // 需在 resume 阶段重放前缀 join（重新绑定 inner 供尾算子引用），
                    // 再续流尾算子。join 对每个排序后元素重扫内层源，语义等价。
                    let mut resume_ops: Vec<LinqOp> = chain.operators[..k]
                        .iter()
                        .filter(|op| matches!(op, LinqOp::Join { .. }))
                        .cloned()
                        .collect();
                    resume_ops.extend_from_slice(&chain.operators[run_end + 1..]);
                    let resume = LinqChain {
                        source: MirOperand::Local(buffer),
                        source_len: None,
                        operators: resume_ops,
                    };
                    self.materialize_chain_inner(
                        resume,
                        buffer_elem_ty,
                        result_list,
                        result_class,
                        ctx,
                        stmts,
                    );
                    return;
                }
            }
            BufKind::GroupBy => {
                if let Some(plan) = self.prepare_linq_groupby(&chain, k, &src_elem_ty, ctx, stmts) {
                    self.emit_materialize_ops_loop(
                        &chain.source,
                        chain.source_len,
                        &src_elem_ty,
                        &chain.operators[..k],
                        &LinqTarget::GroupByPair {
                            key_list: plan.pair_keys,
                            item_list: plan.pair_items,
                            key_ty: &plan.key_ty,
                            item_ty: &plan.item_ty,
                            key_lambda: &plan.key_lambda,
                            item_lambda: plan.item_lambda.as_ref(),
                        },
                        ctx,
                        stmts,
                    );
                    let groups = self.fresh_local(
                        &"_linq_groups".into(),
                        TypeId::Named(plan.groups_class.clone().into()),
                        ctx.locals,
                    );
                    stmts.push(MirStatement::Assign {
                        place: groups,
                        rvalue: MirRvalue::New {
                            class: plan.groups_class.clone(),
                            args: vec![],
                            ctor_params: vec![],
                        },
                    });
                    self.emit_groupby_groups(&plan, groups, ctx, stmts);
                    let resume = LinqChain {
                        source: MirOperand::Local(groups),
                        source_len: None,
                        operators: chain.operators[k + 1..].to_vec(),
                    };
                    self.materialize_chain_inner(
                        resume,
                        plan.group_ty.clone(),
                        result_list,
                        result_class,
                        ctx,
                        stmts,
                    );
                    return;
                }
            }
        }
        // prepare 失败 → 流式回退（OrderBy/GroupBy 被 `materialize_linq_ops` 跳过）。
        self.emit_materialize_ops_loop(
            &chain.source,
            chain.source_len,
            &src_elem_ty,
            &chain.operators,
            &LinqTarget::Append {
                result_list,
                result_class,
            },
            ctx,
            stmts,
        );
    }

    /// Emit an indexed loop that streams `source` through `ops` (Where/Select/
    /// Let/Join inlined via `materialize_linq_ops`, OrderBy/GroupBy buffered by
    /// the caller) applying the leaf `target` per element. Shared by the plain
    /// materialization path, the OrderBy buffer/resume flow and the GroupBy pair
    /// buffer; array (`IndexGet`) and `List_<T>` (`get_Count`/`get_Item`)
    /// sources both supported.
    fn emit_materialize_ops_loop(
        &mut self,
        source: &MirOperand,
        source_len: Option<usize>,
        src_elem_ty: &TypeId,
        ops: &[LinqOp],
        target: &LinqTarget,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        if let Some(count) = source_len {
            let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
            let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
            let elem_local = self.fresh_local(&"_elem".into(), src_elem_ty.clone(), ctx.locals);

            stmts.push(MirStatement::Assign {
                place: count_local,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(count as i64)),
            });
            stmts.push(MirStatement::Assign {
                place: idx_local,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
            });

            let mut while_body = Vec::new();
            while_body.push(MirStatement::Assign {
                place: elem_local,
                rvalue: MirRvalue::IndexGet {
                    array: source.clone(),
                    index: MirOperand::Local(idx_local),
                    elem_type: src_elem_ty.clone(),
                },
            });

            self.materialize_linq_ops(
                ops,
                0,
                elem_local,
                src_elem_ty,
                target,
                ctx,
                &mut while_body,
            );

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
                foreach_source: Some(source.clone()),
            });
            return;
        }

        let Some((recv_type, _)) = list_source_info(source, ctx) else {
            return;
        };
        let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
        let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
        let elem_local = self.fresh_local(&"_elem".into(), src_elem_ty.clone(), ctx.locals);

        stmts.push(MirStatement::Assign {
            place: count_local,
            rvalue: MirRvalue::MethodCall {
                receiver: source.clone(),
                method: "get_Count".to_string(),
                args: vec![],
                receiver_type: recv_type.clone(),
                impl_class: Some(recv_type.clone()),
                target_fn: Some(format!("{}::get_Count", recv_type)),
                is_virtual: false,
                params: vec![],
            },
        });
        stmts.push(MirStatement::Assign {
            place: idx_local,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });

        let mut while_body = Vec::new();
        while_body.push(MirStatement::Assign {
            place: elem_local,
            rvalue: MirRvalue::MethodCall {
                receiver: source.clone(),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(idx_local)],
                receiver_type: recv_type.clone(),
                impl_class: Some(recv_type.clone()),
                target_fn: Some(format!("{}::get_Item", recv_type)),
                is_virtual: false,
                params: vec![],
            },
        });

        self.materialize_linq_ops(
            ops,
            0,
            elem_local,
            src_elem_ty,
            target,
            ctx,
            &mut while_body,
        );

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
            foreach_source: Some(source.clone()),
        });
    }

    /// Create the sort buffer `List_<Tk>` local (with `new`) and lift the
    /// OrderBy run `operators[k..=run_end]` (consecutive keys) to a single
    /// comparator — C# `OrderBy(...).ThenBy(...)` 语义。Returns `None` when any
    /// key captures or has no supported comparison — keep the documented skip.
    fn prepare_linq_sort(
        &mut self,
        chain: &LinqChain,
        k: usize,
        run_end: usize,
        src_elem_ty: &TypeId,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) -> Option<(LocalId, String, TypeId, MirOperand)> {
        let buffer_elem_ty = projected_elem_ty(&chain.operators[..k], src_elem_ty, self, ctx);
        let keys: Vec<(LambdaExpr, bool)> = chain.operators[k..=run_end]
            .iter()
            .map(|op| match op {
                LinqOp::OrderBy { key, descending } => (key.clone(), *descending),
                _ => unreachable!("prepare_linq_sort called with non-OrderBy run"),
            })
            .collect();
        let cmp = self.lift_linq_cmp_multi(&keys, &buffer_elem_ty, ctx)?;
        let buffer_class = mangle_generic("List", std::slice::from_ref(&buffer_elem_ty));
        let buffer = self.fresh_local(
            &"_linq_buffer".into(),
            TypeId::Named(buffer_class.clone().into()),
            ctx.locals,
        );
        stmts.push(MirStatement::Assign {
            place: buffer,
            rvalue: MirRvalue::New {
                class: buffer_class.clone(),
                args: vec![],
                ctor_params: vec![],
            },
        });
        Some((buffer, buffer_class, buffer_elem_ty, cmp))
    }

    /// Lift the OrderBy key run into a no-capture comparator function
    /// `i32 (a: Tk, b: Tk)` (lambda ABI `ret i32(ptr, ptr)`, matching qsort's
    /// callback), suitable for `List<T>.Sort(cmp)` / `rt_list_sort`.
    ///
    /// Body shape per key `(ka = key(a); kb = key(b); cmp_i = ka.Compare(kb))`:
    /// `if cmp_i != 0 { return desc ? -cmp_i : cmp_i }` — 多个 key 依次生效
    /// （ThenBy 语义，无需依赖 qsort 稳定性）。任一带捕获的 key 返回 `None` —
    /// qsort 无 env slot，保持文档化的诚实跳过（同 `List.Sort(cmp)` 限制）。
    pub(super) fn lift_linq_cmp_multi(
        &mut self,
        keys: &[(LambdaExpr, bool)],
        elem_ty: &TypeId,
        ctx: &mut LowerCtx,
    ) -> Option<MirOperand> {
        if keys.is_empty() {
            return None;
        }
        for (key, _) in keys {
            if !Self::compute_captures(key, ctx).captures.is_empty() {
                return None;
            }
            if !matches!(key.body, LambdaBody::Expr(_)) {
                return None;
            }
        }
        let lambda_name = format!("__lambda_linq_cmp_{}", self.next_lambda);
        self.next_lambda += 1;

        let saved = self.next_local;
        self.next_local = 0;
        let mut locals = IndexMap::new();
        let mut scopes = vec![IndexMap::new()];

        let a_local = self.fresh_local(&"a".into(), elem_ty.clone(), &mut locals);
        scopes[0].insert("a".into(), a_local);
        let b_local = self.fresh_local(&"b".into(), elem_ty.clone(), &mut locals);
        scopes[0].insert("b".into(), b_local);

        let mut lambda_ctx = LowerCtx {
            scopes,
            loop_scopes: Vec::new(),
            locals: &mut locals,
            array_lengths: IndexMap::new(),
            owner: None,
            class_fields: &[],
            fn_sigs: ctx.fn_sigs,
            registry: ctx.registry,
            layouts: ctx.layouts,
            host_linkage: ctx.host_linkage,
            type_sizes: ctx.type_sizes,
            fn_ret: TypeId::Int,
            expr_types: ctx.expr_types,
        };

        let mut stmts = Vec::new();

        for (key, descending) in keys {
            let LambdaBody::Expr(key_expr) = &key.body else {
                return None;
            };
            let param_name = key.params[0].name.clone();

            // ka = key(a)
            lambda_ctx.push_scope();
            let ka_bind = self.fresh_local(&param_name, elem_ty.clone(), lambda_ctx.locals);
            lambda_ctx.bind(&param_name, ka_bind);
            stmts.push(MirStatement::Assign {
                place: ka_bind,
                rvalue: MirRvalue::Use(MirOperand::Local(a_local)),
            });
            let key_ty = infer_type_from_spanned(key_expr, &lambda_ctx);
            if matches!(key_ty, TypeId::Infer | TypeId::Error) {
                return None;
            }
            let (mut prep_a, ka_rv) =
                lower_expr_to_rvalue_with_binary(&key_expr.node, self, &mut lambda_ctx);
            stmts.append(&mut prep_a);
            let ka_slot = self.fresh_local(&"_ka".into(), key_ty.clone(), lambda_ctx.locals);
            stmts.push(MirStatement::Assign {
                place: ka_slot,
                rvalue: ka_rv,
            });
            lambda_ctx.pop_scope();

            // kb = key(b)
            lambda_ctx.push_scope();
            let kb_bind = self.fresh_local(&param_name, elem_ty.clone(), lambda_ctx.locals);
            lambda_ctx.bind(&param_name, kb_bind);
            stmts.push(MirStatement::Assign {
                place: kb_bind,
                rvalue: MirRvalue::Use(MirOperand::Local(b_local)),
            });
            let (mut prep_b, kb_rv) =
                lower_expr_to_rvalue_with_binary(&key_expr.node, self, &mut lambda_ctx);
            stmts.append(&mut prep_b);
            let kb_slot = self.fresh_local(&"_kb".into(), key_ty.clone(), lambda_ctx.locals);
            stmts.push(MirStatement::Assign {
                place: kb_slot,
                rvalue: kb_rv,
            });
            lambda_ctx.pop_scope();

            let cmp_rv = linq_key_compare_rvalue(&key_ty, ka_slot, kb_slot, ctx)?;
            let cmp_local = self.fresh_local(&"_cmp".into(), TypeId::Int, lambda_ctx.locals);
            stmts.push(MirStatement::Assign {
                place: cmp_local,
                rvalue: cmp_rv,
            });
            let ret_rv = if *descending {
                MirRvalue::Binary {
                    op: BinOp::Sub,
                    left: MirOperand::ConstInt(0),
                    right: MirOperand::Local(cmp_local),
                }
            } else {
                MirRvalue::Use(MirOperand::Local(cmp_local))
            };
            // if cmp != 0 { return (desc ? -cmp : cmp) }——ThenBy 级联。
            let nz_local = self.fresh_local(&"_cmpnz".into(), TypeId::Bool, lambda_ctx.locals);
            stmts.push(MirStatement::Assign {
                place: nz_local,
                rvalue: MirRvalue::Binary {
                    op: BinOp::NotEq,
                    left: MirOperand::Local(cmp_local),
                    right: MirOperand::ConstInt(0),
                },
            });
            stmts.push(MirStatement::If {
                cond: MirOperand::Local(nz_local),
                then_body: vec![MirStatement::Return(Some(ret_rv))],
                else_body: vec![],
            });
        }
        // 所有 key 相等 → 0。
        stmts.push(MirStatement::Return(Some(MirRvalue::Use(
            MirOperand::ConstInt(0),
        ))));

        self.next_local = saved;
        let body = MirBody {
            params: vec![("a".into(), elem_ty.clone()), ("b".into(), elem_ty.clone())],
            ret: TypeId::Int,
            param_count: 2,
            locals,
            blocks: vec![MirBasicBlock { statements: stmts }],
            is_async: false,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            is_static: false,
            captures: vec![],
            linkage: ctx.host_linkage,
            parallelize: false,
            spill_set: typeck::SpillSet::empty(),
        };
        self.lifted.push((lambda_name.clone(), body.to_cfg()));
        Some(MirOperand::FnPtr { name: lambda_name })
    }

    /// Recursive counterpart of `apply_linq_ops` for materialization: instead
    /// of executing foreach body statements at the leaf, applies the `target`
    /// — either `result.Add(current)` (plain / sort buffer) or the groupby
    /// pair-buffer leaf (key/item evaluated in the live `let`/`join` scopes).
    fn materialize_linq_ops(
        &mut self,
        ops: &[LinqOp],
        idx: usize,
        current: LocalId,
        current_ty: &TypeId,
        target: &LinqTarget,
        ctx: &mut LowerCtx,
        out: &mut Vec<MirStatement>,
    ) {
        if idx >= ops.len() {
            match target {
                LinqTarget::Append {
                    result_list,
                    result_class,
                } => {
                    let (impl_class, target_fn) = resolve_method_target(
                        ctx.registry,
                        &result_class.to_string().into(),
                        &"Add".into(),
                        ctx.owner.clone(),
                    );
                    let is_virtual = is_virtual_member(ctx.layouts, result_class, "Add", &[]);
                    let add_place = self.fresh_local(&"_add".into(), TypeId::Void, ctx.locals);
                    out.push(MirStatement::Assign {
                        place: add_place,
                        rvalue: MirRvalue::MethodCall {
                            receiver: MirOperand::Local(*result_list),
                            method: "Add".to_string(),
                            args: vec![MirOperand::Local(current)],
                            receiver_type: result_class.to_string(),
                            impl_class,
                            target_fn,
                            is_virtual,
                            params: vec![],
                        },
                    });
                }
                LinqTarget::GroupByPair {
                    key_list,
                    item_list,
                    key_ty,
                    item_ty,
                    key_lambda,
                    item_lambda,
                } => {
                    // 在流式叶子（`let`/`join` 作用域仍存活）求值 key / item，
                    // 使 `group x by y` 的 y 可引用之前引入的变量。
                    ctx.push_scope();
                    let k_param = key_lambda.params[0].name.clone();
                    let k_bind = self.fresh_local(&k_param, current_ty.clone(), ctx.locals);
                    ctx.bind(&k_param, k_bind);
                    out.push(MirStatement::Assign {
                        place: k_bind,
                        rvalue: MirRvalue::Use(MirOperand::Local(current)),
                    });
                    let mut key_prep = Vec::new();
                    let key_rv = match &key_lambda.body {
                        LambdaBody::Expr(e) => {
                            let (mut prep, rv) =
                                lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                            key_prep.append(&mut prep);
                            rv
                        }
                        LambdaBody::Block(_) => MirRvalue::Use(MirOperand::Local(current)),
                    };
                    let key_local =
                        self.fresh_local(&"_gkey".into(), (*key_ty).clone(), ctx.locals);
                    out.append(&mut key_prep);
                    out.push(MirStatement::Assign {
                        place: key_local,
                        rvalue: key_rv,
                    });
                    let item_local =
                        self.fresh_local(&"_gitem".into(), (*item_ty).clone(), ctx.locals);
                    if let Some(il) = item_lambda {
                        let i_param = il.params[0].name.clone();
                        let i_bind = self.fresh_local(&i_param, current_ty.clone(), ctx.locals);
                        ctx.bind(&i_param, i_bind);
                        out.push(MirStatement::Assign {
                            place: i_bind,
                            rvalue: MirRvalue::Use(MirOperand::Local(current)),
                        });
                        let mut item_prep = Vec::new();
                        let item_rv = match &il.body {
                            LambdaBody::Expr(e) => {
                                let (mut prep, rv) =
                                    lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                                item_prep.append(&mut prep);
                                rv
                            }
                            LambdaBody::Block(_) => MirRvalue::Use(MirOperand::Local(current)),
                        };
                        out.append(&mut item_prep);
                        out.push(MirStatement::Assign {
                            place: item_local,
                            rvalue: item_rv,
                        });
                    } else {
                        out.push(MirStatement::Assign {
                            place: item_local,
                            rvalue: MirRvalue::Use(MirOperand::Local(current)),
                        });
                    }
                    ctx.pop_scope();
                    self.emit_list_add_call(*key_list, &list_class_of(key_ty), key_local, ctx, out);
                    self.emit_list_add_call(
                        *item_list,
                        &list_class_of(item_ty),
                        item_local,
                        ctx,
                        out,
                    );
                }
            }
            return;
        }

        match &ops[idx] {
            LinqOp::Where(lambda) => {
                ctx.push_scope();
                let param_name = lambda.params[0].name.clone();
                let param_local = self.fresh_local(&param_name, current_ty.clone(), ctx.locals);
                ctx.bind(&param_name, param_local);

                out.push(MirStatement::Assign {
                    place: param_local,
                    rvalue: MirRvalue::Use(MirOperand::Local(current)),
                });

                let mut cond_prep = Vec::new();
                let cond_rv = match &lambda.body {
                    LambdaBody::Expr(e) => {
                        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        cond_prep.append(&mut prep);
                        rv
                    }
                    LambdaBody::Block(_) => MirRvalue::Use(MirOperand::ConstBool(true)),
                };
                let cond_local = self.fresh_local(&"_where".into(), TypeId::Bool, ctx.locals);
                out.append(&mut cond_prep);
                out.push(MirStatement::Assign {
                    place: cond_local,
                    rvalue: cond_rv,
                });

                let mut then_body = Vec::new();
                self.materialize_linq_ops(
                    ops,
                    idx + 1,
                    current,
                    current_ty,
                    target,
                    ctx,
                    &mut then_body,
                );

                out.push(MirStatement::If {
                    cond: MirOperand::Local(cond_local),
                    then_body,
                    else_body: vec![],
                });

                ctx.pop_scope();
            }
            LinqOp::Select(lambda) => {
                ctx.push_scope();
                let param_name = lambda.params[0].name.clone();
                let param_local = self.fresh_local(&param_name, current_ty.clone(), ctx.locals);
                ctx.bind(&param_name, param_local);

                out.push(MirStatement::Assign {
                    place: param_local,
                    rvalue: MirRvalue::Use(MirOperand::Local(current)),
                });

                let mut new_prep = Vec::new();
                let new_rv = match &lambda.body {
                    LambdaBody::Expr(e) => {
                        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        new_prep.append(&mut prep);
                        rv
                    }
                    LambdaBody::Block(_) => MirRvalue::Use(MirOperand::Local(current)),
                };
                // 中间 Select 投影类型可能异于最终类型（如
                // `Select(x => x.Name).Select(n => n.Length)`），以体推断为准。
                let new_ty = match &lambda.body {
                    LambdaBody::Expr(e) => infer_type_from_spanned(e, ctx),
                    LambdaBody::Block(_) => current_ty.clone(),
                };
                let new_local = self.fresh_local(&"_sel".into(), new_ty.clone(), ctx.locals);
                out.append(&mut new_prep);
                out.push(MirStatement::Assign {
                    place: new_local,
                    rvalue: new_rv,
                });

                self.materialize_linq_ops(ops, idx + 1, new_local, &new_ty, target, ctx, out);

                ctx.pop_scope();
            }
            LinqOp::Let { ident, value } => {
                // 求值 value（param 绑到 current）→ 绑定 `ident`，元素自身继续前流。
                ctx.push_scope();
                let param_name = value.params[0].name.clone();
                let param_local = self.fresh_local(&param_name, current_ty.clone(), ctx.locals);
                ctx.bind(&param_name, param_local);
                out.push(MirStatement::Assign {
                    place: param_local,
                    rvalue: MirRvalue::Use(MirOperand::Local(current)),
                });
                let value_ty = match &value.body {
                    LambdaBody::Expr(e) => infer_type_from_spanned(e, ctx),
                    LambdaBody::Block(_) => current_ty.clone(),
                };
                let v_local = self.fresh_local(ident, value_ty.clone(), ctx.locals);
                let mut v_prep = Vec::new();
                let v_rv = match &value.body {
                    LambdaBody::Expr(e) => {
                        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        v_prep.append(&mut prep);
                        rv
                    }
                    LambdaBody::Block(_) => MirRvalue::Use(MirOperand::Local(current)),
                };
                out.append(&mut v_prep);
                out.push(MirStatement::Assign {
                    place: v_local,
                    rvalue: v_rv,
                });
                ctx.bind(ident, v_local);
                self.materialize_linq_ops(ops, idx + 1, current, current_ty, target, ctx, out);
                ctx.pop_scope();
            }
            LinqOp::Join {
                outer,
                inner,
                source,
                on_left,
                on_right,
            } => {
                // Inner join：外层当前元素绑定 `outer`；内层源逐元素绑定 `inner`；
                // 命中后后续子句在 (outer, inner) 双变量作用域内继续。
                ctx.push_scope();
                let outer_local = self.fresh_local(outer, current_ty.clone(), ctx.locals);
                ctx.bind(outer, outer_local);
                out.push(MirStatement::Assign {
                    place: outer_local,
                    rvalue: MirRvalue::Use(MirOperand::Local(current)),
                });
                let Some((recv_type, inner_elem_ty)) =
                    list_source_info(&operand_from_expr(&source.node, ctx), ctx)
                else {
                    // 非 List 内层源：诚实跳过 join（保持外层流）。
                    ctx.pop_scope();
                    self.materialize_linq_ops(ops, idx + 1, current, current_ty, target, ctx, out);
                    return;
                };
                let inner_op = operand_from_expr(&source.node, ctx);
                let count_local = self.fresh_local(&"_jcnt".into(), TypeId::Int, ctx.locals);
                out.push(MirStatement::Assign {
                    place: count_local,
                    rvalue: MirRvalue::MethodCall {
                        receiver: inner_op.clone(),
                        method: "get_Count".to_string(),
                        args: vec![],
                        receiver_type: recv_type.clone(),
                        impl_class: Some(recv_type.clone()),
                        target_fn: Some(format!("{}::get_Count", recv_type)),
                        is_virtual: false,
                        params: vec![],
                    },
                });
                let idx_local = self.fresh_local(&"_jidx".into(), TypeId::Int, ctx.locals);
                out.push(MirStatement::Assign {
                    place: idx_local,
                    rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
                });
                let elem_local =
                    self.fresh_local(&"_jelem".into(), inner_elem_ty.clone(), ctx.locals);

                let mut inner_loop = Vec::new();
                inner_loop.push(MirStatement::Assign {
                    place: elem_local,
                    rvalue: MirRvalue::MethodCall {
                        receiver: inner_op.clone(),
                        method: "get_Item".to_string(),
                        args: vec![MirOperand::Local(idx_local)],
                        receiver_type: recv_type.clone(),
                        impl_class: Some(recv_type.clone()),
                        target_fn: Some(format!("{}::get_Item", recv_type)),
                        is_virtual: false,
                        params: vec![],
                    },
                });
                ctx.push_scope();
                let inner_local = self.fresh_local(inner, inner_elem_ty.clone(), ctx.locals);
                ctx.bind(inner, inner_local);
                inner_loop.push(MirStatement::Assign {
                    place: inner_local,
                    rvalue: MirRvalue::Use(MirOperand::Local(elem_local)),
                });
                // 命中条件：on_left(outer) == on_right(inner)。等值 key 类型按
                // on_left 表达式推断（typeck 已保证 on_left/on_right 同型）。
                let l_ty = match &on_left.body {
                    LambdaBody::Expr(e) => infer_type_from_spanned(e, ctx),
                    LambdaBody::Block(_) => current_ty.clone(),
                };
                let mut l_prep = Vec::new();
                let l_rv = match &on_left.body {
                    LambdaBody::Expr(e) => {
                        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        l_prep.append(&mut prep);
                        rv
                    }
                    LambdaBody::Block(_) => MirRvalue::Use(MirOperand::Local(current)),
                };
                let l_local = self.fresh_local(&"_jl".into(), l_ty.clone(), ctx.locals);
                inner_loop.append(&mut l_prep);
                inner_loop.push(MirStatement::Assign {
                    place: l_local,
                    rvalue: l_rv,
                });
                let mut r_prep = Vec::new();
                let r_rv = match &on_right.body {
                    LambdaBody::Expr(e) => {
                        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        r_prep.append(&mut prep);
                        rv
                    }
                    LambdaBody::Block(_) => MirRvalue::Use(MirOperand::Local(current)),
                };
                let r_local = self.fresh_local(&"_jr".into(), l_ty, ctx.locals);
                inner_loop.append(&mut r_prep);
                inner_loop.push(MirStatement::Assign {
                    place: r_local,
                    rvalue: r_rv,
                });
                let match_local = self.fresh_local(&"_jmatch".into(), TypeId::Bool, ctx.locals);
                inner_loop.push(MirStatement::Assign {
                    place: match_local,
                    rvalue: MirRvalue::Binary {
                        op: BinOp::Eq,
                        left: MirOperand::Local(l_local),
                        right: MirOperand::Local(r_local),
                    },
                });
                let mut then_body = Vec::new();
                self.materialize_linq_ops(
                    ops,
                    idx + 1,
                    current,
                    current_ty,
                    target,
                    ctx,
                    &mut then_body,
                );
                inner_loop.push(MirStatement::If {
                    cond: MirOperand::Local(match_local),
                    then_body,
                    else_body: vec![],
                });
                ctx.pop_scope();
                inner_loop.push(MirStatement::Assign {
                    place: idx_local,
                    rvalue: MirRvalue::Binary {
                        op: BinOp::Add,
                        left: MirOperand::Local(idx_local),
                        right: MirOperand::ConstInt(1),
                    },
                });
                out.push(MirStatement::While {
                    cond: MirRvalue::Binary {
                        op: BinOp::Lt,
                        left: MirOperand::Local(idx_local),
                        right: MirOperand::Local(count_local),
                    },
                    body: inner_loop,
                    foreach_source: Some(inner_op.clone()),
                });
                ctx.pop_scope();
            }
            LinqOp::OrderBy { .. } | LinqOp::GroupBy { .. } => {
                // OrderBy / GroupBy 需要缓冲物化，由 `materialize_chain_inner`
                // 处理；流式回退时跳过（诚实子集：元素保持源顺序/不分组）。
                self.materialize_linq_ops(ops, idx + 1, current, current_ty, target, ctx, out);
            }
        }
    }

    /// Emit `list.Add(value)` for a `List_<T>` receiver with method resolution.
    fn emit_list_add_call(
        &mut self,
        list: LocalId,
        list_class: &str,
        value: LocalId,
        ctx: &mut LowerCtx,
        out: &mut Vec<MirStatement>,
    ) {
        let (impl_class, target_fn) = resolve_method_target(
            ctx.registry,
            &list_class.to_string().into(),
            &"Add".into(),
            ctx.owner.clone(),
        );
        let is_virtual = is_virtual_member(ctx.layouts, list_class, "Add", &[]);
        let add_place = self.fresh_local(&"_add".into(), TypeId::Void, ctx.locals);
        out.push(MirStatement::Assign {
            place: add_place,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(list),
                method: "Add".to_string(),
                args: vec![MirOperand::Local(value)],
                receiver_type: list_class.to_string(),
                impl_class,
                target_fn,
                is_virtual,
                params: vec![],
            },
        });
    }

    /// 分配 GroupBy pair 缓冲（keys/items 平行 List）并推导分组产物类型。
    /// key/item lambda 带捕获 → `None`（诚实跳过，同 OrderBy 限制）；
    /// 等值判定只依赖 `linq_key_compare_rvalue` 支持的 key 类型（与 orderby
    /// 同一支持面），避免 String/结构等不可靠指针相等落入未定义行为。
    fn prepare_linq_groupby(
        &mut self,
        chain: &LinqChain,
        k: usize,
        src_elem_ty: &TypeId,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) -> Option<LinqGroupByPlan> {
        let LinqOp::GroupBy { key, element } = &chain.operators[k] else {
            return None;
        };
        if !Self::compute_captures(key, ctx).captures.is_empty() {
            return None;
        }
        if let Some(el) = element {
            if !Self::compute_captures(el, ctx).captures.is_empty() {
                return None;
            }
        }
        let key_ty = self.infer_linq_lambda_type(key, src_elem_ty, ctx);
        if matches!(key_ty, TypeId::Infer | TypeId::Error) {
            return None;
        }
        linq_key_compare_rvalue(&key_ty, LocalId(0), LocalId(1), ctx)?;
        let item_ty = element
            .as_ref()
            .and_then(|el| {
                let t = self.infer_linq_lambda_type(el, src_elem_ty, ctx);
                if matches!(t, TypeId::Infer | TypeId::Error) {
                    None
                } else {
                    Some(t)
                }
            })
            .unwrap_or_else(|| src_elem_ty.clone());
        let key_class = list_class_of(&key_ty);
        let item_class = list_class_of(&item_ty);
        let pair_keys = self.fresh_local(
            &"_linq_pair_keys".into(),
            TypeId::Named(key_class.clone().into()),
            ctx.locals,
        );
        stmts.push(MirStatement::Assign {
            place: pair_keys,
            rvalue: MirRvalue::New {
                class: key_class,
                args: vec![],
                ctor_params: vec![],
            },
        });
        let pair_items = self.fresh_local(
            &"_linq_pair_items".into(),
            TypeId::Named(item_class.clone().into()),
            ctx.locals,
        );
        stmts.push(MirStatement::Assign {
            place: pair_items,
            rvalue: MirRvalue::New {
                class: item_class,
                args: vec![],
                ctor_params: vec![],
            },
        });
        let group_class = mangle_generic("Grouping", &[key_ty.clone(), item_ty.clone()]);
        Some(LinqGroupByPlan {
            pair_keys,
            pair_items,
            key_ty,
            item_ty,
            group_ty: TypeId::Named(group_class.clone().into()),
            groups_class: mangle_generic("List", &[TypeId::Named(group_class.into())]),
            key_lambda: key.clone(),
            item_lambda: element.clone(),
        })
    }

    /// 以 `param_ty` 为范围变量类型推断单表达式 lambda 的返回类型。
    /// 构造只含参数绑定的轻量 LowerCtx，与 `lift_linq_cmp_multi` 同构。
    fn infer_linq_lambda_type(
        &mut self,
        lambda: &LambdaExpr,
        param_ty: &TypeId,
        ctx: &LowerCtx,
    ) -> TypeId {
        let LambdaBody::Expr(e) = &lambda.body else {
            return param_ty.clone();
        };
        let saved = self.next_local;
        self.next_local = 0;
        let mut locals = IndexMap::new();
        let mut scopes = vec![IndexMap::new()];
        let param_name = lambda.params[0].name.clone();
        let p_local = self.fresh_local(&param_name, param_ty.clone(), &mut locals);
        scopes[0].insert(param_name, p_local);
        let lambda_ctx = LowerCtx {
            scopes,
            loop_scopes: Vec::new(),
            locals: &mut locals,
            array_lengths: IndexMap::new(),
            owner: None,
            class_fields: &[],
            fn_sigs: ctx.fn_sigs,
            registry: ctx.registry,
            layouts: ctx.layouts,
            host_linkage: ctx.host_linkage,
            type_sizes: ctx.type_sizes,
            fn_ret: TypeId::Int,
            expr_types: ctx.expr_types,
        };
        let ty = infer_type_from_spanned(e, &lambda_ctx);
        self.next_local = saved;
        ty
    }

    /// 顺序扫描 pair 缓冲（keys/items 平行 List），按 key 首次出现序构建分组。
    ///
    /// 两遍实现（避免「在读取容器的同一天然循环内对其 Add」——NLL
    /// `E_ITERATOR_INVALIDATION` 会拒绝 scan+Add 同环）：
    /// - Pass A：对每个 pair 线性扫描此前 pair 的 key（只读输入缓冲 `pair_keys`）；
    ///   命中首现 → `firsts.Add(首现 pair 下标)`；未命中 → `new Grouping(key)`
    ///   追加进 `groups` 且 `firsts.Add(i)`。此遍 `firsts`/`groups` 只写不读，
    ///   无 scan+Add 冲突。
    /// - Pass B：对每个 pair，组下标 = `firsts[0..firsts[i])` 中「首现」个数
    ///   （分组按首现序创建，下标即其前驱首现计数）；`groups[gi].Add(pair_items[i])`
    ///   填充元素。此遍 `firsts`/`groups`/`pair_items` 只读，`group_ref.Add`
    ///   作用于分组对象自身（非容器列表），无失效风险。
    ///
    /// 等值判定复用 `linq_key_compare_rvalue`（`cmp == 0`）。
    fn emit_groupby_groups(
        &mut self,
        plan: &LinqGroupByPlan,
        groups: LocalId,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        let key_class = list_class_of(&plan.key_ty);
        let item_class = list_class_of(&plan.item_ty);
        let group_class = mangle_generic("Grouping", &[plan.key_ty.clone(), plan.item_ty.clone()]);
        let int_class = list_class_of(&TypeId::Int);

        // ---- Pass A：构建唯一分组（首现序）----
        let pair_count = self.fresh_local(&"_gcnt".into(), TypeId::Int, ctx.locals);
        stmts.push(MirStatement::Assign {
            place: pair_count,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(plan.pair_keys),
                method: "get_Count".to_string(),
                args: vec![],
                receiver_type: key_class.clone(),
                impl_class: Some(key_class.clone()),
                target_fn: Some(format!("{key_class}::get_Count")),
                is_virtual: false,
                params: vec![],
            },
        });
        let firsts = self.fresh_local(
            &"_gfirsts".into(),
            TypeId::Named(int_class.clone().into()),
            ctx.locals,
        );
        stmts.push(MirStatement::Assign {
            place: firsts,
            rvalue: MirRvalue::New {
                class: int_class.clone(),
                args: vec![],
                ctor_params: vec![],
            },
        });
        let pair_idx = self.fresh_local(&"_gpidx".into(), TypeId::Int, ctx.locals);
        stmts.push(MirStatement::Assign {
            place: pair_idx,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });
        let pair_key = self.fresh_local(&"_gpkey".into(), plan.key_ty.clone(), ctx.locals);
        let scan_j = self.fresh_local(&"_gj".into(), TypeId::Int, ctx.locals);
        let found = self.fresh_local(&"_gfound".into(), TypeId::Bool, ctx.locals);
        let first_occ = self.fresh_local(&"_gfirstocc".into(), TypeId::Int, ctx.locals);
        let cand = self.fresh_local(&"_gcand".into(), plan.key_ty.clone(), ctx.locals);
        let cmp = self.fresh_local(&"_gcmp".into(), TypeId::Int, ctx.locals);
        let eq = self.fresh_local(&"_geq".into(), TypeId::Bool, ctx.locals);

        let mut outer_a = Vec::new();
        outer_a.push(MirStatement::Assign {
            place: pair_key,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(plan.pair_keys),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(pair_idx)],
                receiver_type: key_class.clone(),
                impl_class: Some(key_class.clone()),
                target_fn: Some(format!("{key_class}::get_Item")),
                is_virtual: false,
                params: vec![],
            },
        });
        outer_a.push(MirStatement::Assign {
            place: scan_j,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });
        outer_a.push(MirStatement::Assign {
            place: found,
            rvalue: MirRvalue::Use(MirOperand::ConstBool(false)),
        });

        // 扫描循环：仅当未命中时扫描（`if !found` 守卫，避免 break）。
        let mut guarded = Vec::new();
        guarded.push(MirStatement::Assign {
            place: cand,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(plan.pair_keys),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(scan_j)],
                receiver_type: key_class.clone(),
                impl_class: Some(key_class.clone()),
                target_fn: Some(format!("{key_class}::get_Item")),
                is_virtual: false,
                params: vec![],
            },
        });
        guarded.push(MirStatement::Assign {
            place: cmp,
            rvalue: linq_key_compare_rvalue(&plan.key_ty, pair_key, cand, ctx)
                .unwrap_or(MirRvalue::Use(MirOperand::ConstInt(1))),
        });
        guarded.push(MirStatement::Assign {
            place: eq,
            rvalue: MirRvalue::Binary {
                op: BinOp::Eq,
                left: MirOperand::Local(cmp),
                right: MirOperand::ConstInt(0),
            },
        });
        let hit = vec![
            MirStatement::Assign {
                place: first_occ,
                rvalue: MirRvalue::Use(MirOperand::Local(scan_j)),
            },
            MirStatement::Assign {
                place: found,
                rvalue: MirRvalue::Use(MirOperand::ConstBool(true)),
            },
        ];
        guarded.push(MirStatement::If {
            cond: MirOperand::Local(eq),
            then_body: hit,
            else_body: vec![],
        });
        let mut scan = vec![MirStatement::If {
            cond: MirOperand::Local(found),
            then_body: vec![],
            else_body: guarded,
        }];
        scan.push(MirStatement::Assign {
            place: scan_j,
            rvalue: MirRvalue::Binary {
                op: BinOp::Add,
                left: MirOperand::Local(scan_j),
                right: MirOperand::ConstInt(1),
            },
        });
        outer_a.push(MirStatement::While {
            cond: MirRvalue::Binary {
                op: BinOp::Lt,
                left: MirOperand::Local(scan_j),
                right: MirOperand::Local(pair_idx),
            },
            body: scan,
            // GroupBy 去重扫描：迭代编译器内部 `pairs` 缓冲，非源级枚举。
            foreach_source: None,
        });

        // 扫描后决策：命中 → `firsts.Add(首现下标)`；未命中 →
        // `new Grouping(pair_key)` + `groups.Add(g)` + `firsts.Add(pair_idx)`。
        let mut add_found = Vec::new();
        self.emit_list_add_call(firsts, &int_class, first_occ, ctx, &mut add_found);
        let new_group = self.fresh_local(
            &"_gnew".into(),
            TypeId::Named(group_class.clone().into()),
            ctx.locals,
        );
        let mut make_new = Vec::new();
        make_new.push(MirStatement::Assign {
            place: new_group,
            rvalue: MirRvalue::New {
                class: group_class.clone(),
                args: vec![MirOperand::Local(pair_key)],
                ctor_params: vec![],
            },
        });
        self.emit_list_add_call(groups, &plan.groups_class, new_group, ctx, &mut make_new);
        self.emit_list_add_call(firsts, &int_class, pair_idx, ctx, &mut make_new);
        outer_a.push(MirStatement::If {
            cond: MirOperand::Local(found),
            then_body: add_found,
            else_body: make_new,
        });
        outer_a.push(MirStatement::Assign {
            place: pair_idx,
            rvalue: MirRvalue::Binary {
                op: BinOp::Add,
                left: MirOperand::Local(pair_idx),
                right: MirOperand::ConstInt(1),
            },
        });
        stmts.push(MirStatement::While {
            cond: MirRvalue::Binary {
                op: BinOp::Lt,
                left: MirOperand::Local(pair_idx),
                right: MirOperand::Local(pair_count),
            },
            body: outer_a,
            // GroupBy Pass A：迭代编译器内部 `pairs` 缓冲，非源级枚举。
            foreach_source: None,
        });

        // ---- Pass B：按组填充元素 ----
        let i2 = self.fresh_local(&"_gbi".into(), TypeId::Int, ctx.locals);
        stmts.push(MirStatement::Assign {
            place: i2,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });
        let f = self.fresh_local(&"_gf".into(), TypeId::Int, ctx.locals);
        let gi = self.fresh_local(&"_ggi".into(), TypeId::Int, ctx.locals);
        let k = self.fresh_local(&"_gk".into(), TypeId::Int, ctx.locals);
        let fk = self.fresh_local(&"_gfk".into(), TypeId::Int, ctx.locals);
        let is_first = self.fresh_local(&"_gisf".into(), TypeId::Bool, ctx.locals);
        let gref = self.fresh_local(
            &"_gref".into(),
            TypeId::Named(group_class.clone().into()),
            ctx.locals,
        );
        let gitem = self.fresh_local(&"_gitem".into(), plan.item_ty.clone(), ctx.locals);

        let mut outer_b = Vec::new();
        outer_b.push(MirStatement::Assign {
            place: f,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(firsts),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(i2)],
                receiver_type: int_class.clone(),
                impl_class: Some(int_class.clone()),
                target_fn: Some(format!("{int_class}::get_Item")),
                is_virtual: false,
                params: vec![],
            },
        });
        outer_b.push(MirStatement::Assign {
            place: gi,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });
        outer_b.push(MirStatement::Assign {
            place: k,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });
        let mut count_step = Vec::new();
        count_step.push(MirStatement::Assign {
            place: fk,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(firsts),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(k)],
                receiver_type: int_class.clone(),
                impl_class: Some(int_class.clone()),
                target_fn: Some(format!("{int_class}::get_Item")),
                is_virtual: false,
                params: vec![],
            },
        });
        count_step.push(MirStatement::Assign {
            place: is_first,
            rvalue: MirRvalue::Binary {
                op: BinOp::Eq,
                left: MirOperand::Local(fk),
                right: MirOperand::Local(k),
            },
        });
        let inc_gi = vec![MirStatement::Assign {
            place: gi,
            rvalue: MirRvalue::Binary {
                op: BinOp::Add,
                left: MirOperand::Local(gi),
                right: MirOperand::ConstInt(1),
            },
        }];
        count_step.push(MirStatement::If {
            cond: MirOperand::Local(is_first),
            then_body: inc_gi,
            else_body: vec![],
        });
        count_step.push(MirStatement::Assign {
            place: k,
            rvalue: MirRvalue::Binary {
                op: BinOp::Add,
                left: MirOperand::Local(k),
                right: MirOperand::ConstInt(1),
            },
        });
        outer_b.push(MirStatement::While {
            cond: MirRvalue::Binary {
                op: BinOp::Lt,
                left: MirOperand::Local(k),
                right: MirOperand::Local(f),
            },
            body: count_step,
            // GroupBy Pass B 计数：迭代编译器内部缓冲，非源级枚举。
            foreach_source: None,
        });
        outer_b.push(MirStatement::Assign {
            place: gref,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(groups),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(gi)],
                receiver_type: plan.groups_class.clone(),
                impl_class: Some(plan.groups_class.clone()),
                target_fn: Some(format!("{}::get_Item", plan.groups_class)),
                is_virtual: false,
                params: vec![],
            },
        });
        outer_b.push(MirStatement::Assign {
            place: gitem,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(plan.pair_items),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(i2)],
                receiver_type: item_class.clone(),
                impl_class: Some(item_class.clone()),
                target_fn: Some(format!("{item_class}::get_Item")),
                is_virtual: false,
                params: vec![],
            },
        });
        self.emit_list_add_call(gref, &group_class, gitem, ctx, &mut outer_b);
        outer_b.push(MirStatement::Assign {
            place: i2,
            rvalue: MirRvalue::Binary {
                op: BinOp::Add,
                left: MirOperand::Local(i2),
                right: MirOperand::ConstInt(1),
            },
        });
        stmts.push(MirStatement::While {
            cond: MirRvalue::Binary {
                op: BinOp::Lt,
                left: MirOperand::Local(i2),
                right: MirOperand::Local(pair_count),
            },
            body: outer_b,
            // GroupBy Pass B：迭代编译器内部缓冲，非源级枚举。
            foreach_source: None,
        });
    }

    /// Untyped-path variant of `lower_list_foreach` for lambda bodies.
    pub(super) fn lower_list_foreach_untyped(
        &mut self,
        var: &Ident,
        iter: &Spanned<Expr>,
        body: &Block,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        // foreach 变量自成一作用域（与 typed 路径 / typeck `Stmt::For` 对齐）。
        ctx.push_scope();
        let (mut iter_prep, list_op) = lower_arg_operand(self, &iter.node, ctx);
        stmts.append(&mut iter_prep);

        let recv_type = class_from_expr(&iter.node, ctx);
        let elem_ty = infer_type_from_spanned(iter, ctx)
            .enumerable_elem()
            .unwrap_or(TypeId::Infer);

        let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
        let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
        let elem_local = self.fresh_local(var, elem_ty, ctx.locals);
        ctx.enter_loop_body();
        ctx.bind(var, elem_local);

        stmts.push(MirStatement::Assign {
            place: count_local,
            rvalue: MirRvalue::MethodCall {
                receiver: list_op.clone(),
                method: "get_Count".to_string(),
                args: vec![],
                receiver_type: recv_type.clone(),
                impl_class: Some(recv_type.clone()),
                target_fn: Some(format!("{}::get_Count", recv_type)),
                is_virtual: false,
                params: vec![],
            },
        });
        stmts.push(MirStatement::Assign {
            place: idx_local,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });

        let mut while_body = Vec::new();
        while_body.push(MirStatement::Assign {
            place: elem_local,
            rvalue: MirRvalue::MethodCall {
                receiver: list_op.clone(),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(idx_local)],
                receiver_type: recv_type.clone(),
                impl_class: Some(recv_type.clone()),
                target_fn: Some(format!("{}::get_Item", recv_type)),
                is_virtual: false,
                params: vec![],
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
            foreach_source: Some(list_op.clone()),
        });
        ctx.pop_scope();
    }

    /// RFC 044：`foreach (var x in <IEnumerable<T>>) { body }` → 协议化路径。
    ///
    /// 走 `GetEnumerator()` → `while (MoveNext()) { Current; body }` 接口协议，
    /// 非索引路径（`get_Count`/`get_Item`）。使用方：
    /// - 消费 yield 序列（编译器合成状态机类实现 IEnumerable<T>）
    /// - 消费任意自定义 IEnumerable<T> 实现
    ///
    /// 接口方法调用经 codegen `emit_iface_method_call` 胖指针分派：接收者 local
    /// 持有 `{ ptr obj, ptr itable }` 盒地址，codegen 解包后经 itable 槽调用。
    pub(super) fn lower_enumerable_foreach(
        &mut self,
        var: &Ident,
        elem_ty: &TypeId,
        iter: &Spanned<Expr>,
        body: &TypedBlock,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        ctx.push_scope();
        let (mut iter_prep, iter_op) = lower_arg_operand(self, &iter.node, ctx);
        stmts.append(&mut iter_prep);

        let iface_name = mangle_generic("IEnumerable", std::slice::from_ref(elem_ty));
        let enum_name = mangle_generic("IEnumerator", std::slice::from_ref(elem_ty));
        let enum_ty = TypeId::Named(enum_name.clone().into());

        // 1. 创建枚举器局部：`var _enumerator = iter.GetEnumerator();`
        let enum_local = self.fresh_local(&"_enumerator".into(), enum_ty.clone(), ctx.locals);
        stmts.push(MirStatement::Assign {
            place: enum_local,
            rvalue: MirRvalue::MethodCall {
                receiver: iter_op,
                method: "GetEnumerator".into(),
                args: vec![],
                receiver_type: iface_name.clone(),
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
        });

        // 2. 创建循环变量局部
        let elem_local = self.fresh_local(var, elem_ty.clone(), ctx.locals);
        ctx.enter_loop_body();
        ctx.bind(var, elem_local);

        // 3. 构建循环体：`{ elem = _enumerator.Current; body; }`
        let mut while_body = Vec::new();
        while_body.push(MirStatement::Assign {
            place: elem_local,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(enum_local),
                method: "get_Current".into(),
                args: vec![],
                receiver_type: enum_name.clone(),
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
        });
        while_body.extend(self.lower_typed_block(body, ctx));
        ctx.exit_loop_body();

        // 4. `while (enumerator.MoveNext()) { ... }`
        stmts.push(MirStatement::While {
            cond: MirRvalue::MethodCall {
                receiver: MirOperand::Local(enum_local),
                method: "MoveNext".into(),
                args: vec![],
                receiver_type: enum_name,
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
            body: while_body,
            foreach_source: None,
        });
        ctx.pop_scope();
    }

    /// Untyped variant of `lower_enumerable_foreach` for lambda bodies.
    pub(super) fn lower_enumerable_foreach_untyped(
        &mut self,
        var: &Ident,
        elem_ty: &TypeId,
        iter: &Spanned<Expr>,
        body: &Block,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        ctx.push_scope();
        let (mut iter_prep, iter_op) = lower_arg_operand(self, &iter.node, ctx);
        stmts.append(&mut iter_prep);

        let iface_name = mangle_generic("IEnumerable", std::slice::from_ref(elem_ty));
        let enum_name = mangle_generic("IEnumerator", std::slice::from_ref(elem_ty));
        let enum_ty = TypeId::Named(enum_name.clone().into());

        let enum_local = self.fresh_local(&"_enumerator".into(), enum_ty.clone(), ctx.locals);
        stmts.push(MirStatement::Assign {
            place: enum_local,
            rvalue: MirRvalue::MethodCall {
                receiver: iter_op,
                method: "GetEnumerator".into(),
                args: vec![],
                receiver_type: iface_name.clone(),
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
        });

        let elem_local = self.fresh_local(var, elem_ty.clone(), ctx.locals);
        ctx.enter_loop_body();
        ctx.bind(var, elem_local);

        let mut while_body = Vec::new();
        while_body.push(MirStatement::Assign {
            place: elem_local,
            rvalue: MirRvalue::MethodCall {
                receiver: MirOperand::Local(enum_local),
                method: "get_Current".into(),
                args: vec![],
                receiver_type: enum_name.clone(),
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
        });
        while_body.extend(self.lower_block(body, ctx));
        ctx.exit_loop_body();

        stmts.push(MirStatement::While {
            cond: MirRvalue::MethodCall {
                receiver: MirOperand::Local(enum_local),
                method: "MoveNext".into(),
                args: vec![],
                receiver_type: enum_name,
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            },
            body: while_body,
            foreach_source: None,
        });
        ctx.pop_scope();
    }
}

pub(super) fn lower_query(q: &QueryExpr, ctx: &LowerCtx) -> LinqChain {
    let from_var: Ident = q
        .clauses
        .iter()
        .find_map(|c| match c {
            QueryClause::From { ident, .. } => Some(ident.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "x".into());

    let source = q
        .clauses
        .iter()
        .find_map(|c| match c {
            QueryClause::From { source, .. } => Some(operand_from_expr(&source.node, ctx)),
            _ => None,
        })
        .unwrap_or(MirOperand::ConstInt(0));

    let mut operators = Vec::new();
    // 当前流动的元素名（range var）。From 建立；`group ... into g` 重绑为
    // 分组标识；let/join 只引入额外绑定、元素名不变（select/where/orderby
    // 的 lambda param 一律用 range_var，body 中额外变量经作用域解析）。
    let mut range_var = from_var.clone();
    for c in &q.clauses {
        match c {
            QueryClause::From { .. } => {}
            QueryClause::Where(e) => {
                operators.push(LinqOp::Where(lambda_of(e, &range_var)));
            }
            QueryClause::OrderBy { key, descending } => {
                // RFC 019 M5 / orderby 真排序：key lambda 的 param 名必须与
                // 当前 range var 一致（如 `orderby p.Age` 的 body 引用 "p"）。
                operators.push(LinqOp::OrderBy {
                    key: lambda_of(key, &range_var),
                    descending: *descending,
                });
            }
            QueryClause::Let { ident, value } => {
                operators.push(LinqOp::Let {
                    ident: ident.clone(),
                    value: lambda_of(value, &range_var),
                });
            }
            QueryClause::Join {
                ident,
                source,
                on_left,
                on_right,
            } => {
                operators.push(LinqOp::Join {
                    outer: range_var.clone(),
                    inner: ident.clone(),
                    source: source.clone(),
                    on_left: lambda_of(on_left, &range_var),
                    on_right: lambda_of(on_right, ident),
                });
            }
            QueryClause::GroupBy {
                key,
                element,
                into_ident,
            } => {
                operators.push(LinqOp::GroupBy {
                    key: lambda_of(key, &range_var),
                    element: element.as_ref().map(|e| lambda_of(e, &range_var)),
                });
                // range var 重绑：`into g` 用 g，缺省沿用 from range var
                //（C# 语义：缺省时 range var 在 select 中表示分组）。
                range_var = into_ident.clone().unwrap_or_else(|| from_var.clone());
            }
        }
    }
    let select_lambda = if let Expr::Lambda(l) = &q.select.node {
        l.clone()
    } else {
        LambdaExpr {
            params: vec![LambdaParam {
                name: range_var.clone(),
                ty: None,
                default: None,
            }],
            body: LambdaBody::Expr(Box::new(Spanned::new(q.select.node.clone(), Span::DUMMY))),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        }
    };
    operators.push(LinqOp::Select(select_lambda));
    let source_len = array_len_of_operand(&source, ctx);
    LinqChain {
        source,
        source_len,
        operators,
    }
}

/// 查询子句表达式 → `LambdaExpr`：已是 lambda 则原样（body 自含参数名），
/// 否则以 `param` 为参名的单表达式 lambda（body 引用 range var）。
fn lambda_of(e: &Spanned<Expr>, param: &Ident) -> LambdaExpr {
    if let Expr::Lambda(l) = &e.node {
        return l.clone();
    }
    LambdaExpr {
        params: vec![LambdaParam {
            name: param.clone(),
            ty: None,
            default: None,
        }],
        body: LambdaBody::Expr(Box::new(Spanned::new(e.node.clone(), e.span))),
        is_expression_tree: false,
        is_async: false,
        captures: vec![],
    }
}

pub(super) fn try_lower_linq_chain(expr: &Expr, ctx: &LowerCtx) -> Option<LinqChain> {
    let (source_expr, operators) = collect_linq_method_ops(expr)?;
    let source = operand_from_expr(&source_expr, ctx);
    let source_len = array_len_of_operand(&source, ctx);
    Some(LinqChain {
        source,
        source_len,
        operators,
    })
}

/// Peel a terminal LINQ operator (`Any` / `Count` / `First` / `FirstOrDefault`)
/// and the optional stream prefix (`Where`/`Select`/`OrderBy`). Returns `None`
/// when the call is not a recognized terminal or the source is neither a
/// compile-time array nor `List_<T>` (honest subset — no fake full LINQ / Queryable).
pub(super) fn try_parse_linq_terminal(
    expr: &Expr,
    ctx: &LowerCtx,
) -> Option<(LinqChain, &'static str)> {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = expr
    else {
        return None;
    };
    let kind = match method.as_str() {
        "Any" => "Any",
        "Count" => "Count",
        "First" => "First",
        "FirstOrDefault" => "FirstOrDefault",
        // RFC 007：泛型物化终端 `ToList` / `ToArray`——无参、无谓词，仅作链尾。
        "ToList" if args.is_empty() => "ToList",
        "ToArray" if args.is_empty() => "ToArray",
        _ => return None,
    };
    let predicate = match args.len() {
        0 => None,
        1 => Some(lambda_from_arg(args.first()?)?),
        _ => return None,
    };
    let (source_expr, mut operators) = match collect_linq_method_ops(&receiver.node) {
        Some((src, ops)) => (src, ops),
        None => (receiver.node.clone(), vec![]),
    };
    // RFC 007：LINQ 源必须是**叶表达式**（数组字面量 / 标识 / 字段 / this /
    // 索引 / Cast——`operand_from_expr` 可物化的形态，即数组与 `List_<T>`
    // 变量）。方法调用 / `new` / 二元等非叶表达式不可能是 LINQ 源——例如
    // `BarcodePixels.PackRgba(bm).ToArray()`（receiver 是方法调用，返回
    // `List<byte>`）须放行给 List facade 的 OOP 路径（`rt_list_to_array`）；
    // 若在此对非叶 source 执行 `operand_from_expr` 会 ICE（Discriminant(12)
    // MethodCall in operand_from_expr——875a2f59 引入 ToList/ToArray 终端后
    // 劫持了任意 `.ToArray()`/`.ToList()` 调用，typeck 侧已有同源保护）。
    if !matches!(
        &source_expr,
        Expr::CollectionExpr { .. }
            | Expr::Ident(_)
            | Expr::Field { .. }
            | Expr::This
            | Expr::Index { .. }
            | Expr::Cast { .. }
    ) {
        return None;
    }
    let source = operand_from_expr(&source_expr, ctx);
    let source_len = array_len_of_operand(&source, ctx);
    let is_list = list_source_info(&source, ctx).is_some();
    if source_len.is_none() && !is_list {
        return None;
    }
    if let Some(pred) = predicate {
        operators.push(LinqOp::Where(pred));
    }
    Some((
        LinqChain {
            source,
            source_len,
            operators,
        },
        kind,
    ))
}

impl MirBuilder {
    /// Expand `src.(Where|Select)*.(Any|Count|First|FirstOrDefault)(pred?)`
    /// into an indexed while-loop that yields a scalar local. Shared with
    /// foreach/materialize for array + `List_<T>` sources only.
    pub(super) fn lower_linq_terminal(
        &mut self,
        kind: &str,
        chain: LinqChain,
        ctx: &mut LowerCtx,
    ) -> Option<(Vec<MirStatement>, LocalId)> {
        let src_elem_ty = source_elem_ty(&chain.source, ctx)
            .or_else(|| list_source_info(&chain.source, ctx).map(|(_, e)| e))?;
        let result_elem_ty = projected_elem_ty(&chain.operators, &src_elem_ty, self, ctx);

        let mut stmts: Vec<MirStatement> = Vec::new();
        // RFC 007：泛型物化终端——`ToList` → `List_<T>`、`ToArray` → `T[]`。
        // 复用 `materialize_linq_chain_to_list`（含 OrderBy/GroupBy 缓冲）把
        // 链物化为 List，ToArray 再经 `rt_list_to_array` 转数组。不做标量
        // 终端所需的 found 旗标/循环，直接整链物化。
        if kind == "ToList" || kind == "ToArray" {
            let list_ty =
                TypeId::Named(mangle_generic("List", std::slice::from_ref(&result_elem_ty)).into());
            let list_local = self.fresh_local(&"_linq_collect".into(), list_ty.clone(), ctx.locals);
            self.materialize_linq_chain_to_list(chain, list_local, &list_ty, ctx, &mut stmts);
            if kind == "ToArray" {
                // 复用 List facade `ToArray()` 的 codegen 内置分发（`call ptr
                // @rt_list_to_array`）——裸 `MirRvalue::Call { func: "rt_list_to_array" }`
                // 走通用 emit_call，返回类型按 expected=Int 误标 i32，与
                // `declare ptr @rt_list_to_array(ptr)` 冲突导致运行时 oom。
                let list_class = match &list_ty {
                    TypeId::Named(n) => n.to_string(),
                    _ => return None,
                };
                let arr_ty = TypeId::Array {
                    elem: Box::new(result_elem_ty),
                };
                let arr_local =
                    self.fresh_local(&"_linq_to_array".into(), arr_ty.clone(), ctx.locals);
                stmts.push(MirStatement::Assign {
                    place: arr_local,
                    rvalue: MirRvalue::MethodCall {
                        receiver: MirOperand::Local(list_local),
                        method: "ToArray".to_string(),
                        args: vec![],
                        receiver_type: list_class,
                        impl_class: None,
                        target_fn: None,
                        is_virtual: false,
                        params: vec![],
                    },
                });
                return Some((stmts, arr_local));
            }
            return Some((stmts, list_local));
        }
        let (result_ty, result_init) = match kind {
            "Any" => (TypeId::Bool, MirRvalue::Use(MirOperand::ConstBool(false))),
            "Count" => (TypeId::Int, MirRvalue::Use(MirOperand::ConstInt(0))),
            "First" | "FirstOrDefault" => {
                (result_elem_ty.clone(), zero_rvalue_for(&result_elem_ty))
            }
            _ => return None,
        };
        let result_local = self.fresh_local(&format!("_linq_{kind}").into(), result_ty, ctx.locals);
        stmts.push(MirStatement::Assign {
            place: result_local,
            rvalue: result_init,
        });
        let found_local = {
            let f = self.fresh_local(&"_found".into(), TypeId::Bool, ctx.locals);
            stmts.push(MirStatement::Assign {
                place: f,
                rvalue: MirRvalue::Use(MirOperand::ConstBool(false)),
            });
            f
        };

        let var_local = self.fresh_local(&"_linq_item".into(), result_elem_ty.clone(), ctx.locals);

        // 不用 `Break`：终端常作为 Call 实参物化到 prep，嵌套 Break 在 CFG
        // 展平时会破坏控制流（Access Violation）。改用 found 旗标跳过后续命中。
        let body_stmts = match kind {
            "Any" => {
                vec![MirStatement::If {
                    cond: MirOperand::Local(found_local),
                    then_body: vec![],
                    else_body: vec![
                        MirStatement::Assign {
                            place: result_local,
                            rvalue: MirRvalue::Use(MirOperand::ConstBool(true)),
                        },
                        MirStatement::Assign {
                            place: found_local,
                            rvalue: MirRvalue::Use(MirOperand::ConstBool(true)),
                        },
                    ],
                }]
            }
            "Count" => {
                vec![MirStatement::Assign {
                    place: result_local,
                    rvalue: MirRvalue::Binary {
                        op: BinOp::Add,
                        left: MirOperand::Local(result_local),
                        right: MirOperand::ConstInt(1),
                    },
                }]
            }
            "First" | "FirstOrDefault" => {
                vec![MirStatement::If {
                    cond: MirOperand::Local(found_local),
                    then_body: vec![],
                    else_body: vec![
                        MirStatement::Assign {
                            place: result_local,
                            rvalue: MirRvalue::Use(MirOperand::Local(var_local)),
                        },
                        MirStatement::Assign {
                            place: found_local,
                            rvalue: MirRvalue::Use(MirOperand::ConstBool(true)),
                        },
                    ],
                }]
            }
            _ => return None,
        };

        // OrderBy / Join → materialize the chain into a temp List, then run
        // the terminal aggregation over `operators = []`. Source is guaranteed
        // array / List by `try_parse_linq_terminal`, so the materializer
        // produces a valid list.
        let needs_buf = chain.operators.iter().any(linq_op_requires_materialization);
        let resume_chain = if needs_buf {
            let temp_list_ty =
                TypeId::Named(mangle_generic("List", std::slice::from_ref(&result_elem_ty)).into());
            let temp_list =
                self.fresh_local(&"_linq_sorted".into(), temp_list_ty.clone(), ctx.locals);
            self.materialize_linq_chain_to_list(
                chain.clone(),
                temp_list,
                &temp_list_ty,
                ctx,
                &mut stmts,
            );
            LinqChain {
                source: MirOperand::Local(temp_list),
                source_len: None,
                operators: vec![],
            }
        } else {
            chain.clone()
        };
        // 排序物化后从 temp List 再枚举——其元素类型已是最终投影类型。
        let resume_src_elem_ty = if needs_buf {
            result_elem_ty.clone()
        } else {
            src_elem_ty
        };
        self.emit_linq_source_loop(
            &resume_chain,
            &resume_src_elem_ty,
            var_local,
            &result_elem_ty,
            body_stmts,
            ctx,
            &mut stmts,
        )?;

        if kind == "First" {
            // 空序列硬失败。不用 `new InvalidOperationException`+Throw：该组合在
            // Call 实参 prep 中与 Where 过滤叠加时曾触发 0xc0000005；`rt_panic`
            // 与 Parse/OOB 等运行时硬错误同径，语义诚实（非静默零值）。
            // `FirstOrDefault` 保留零初值（default(T)），不 panic。
            stmts.push(MirStatement::If {
                cond: MirOperand::Local(found_local),
                then_body: vec![],
                else_body: vec![MirStatement::Assign {
                    place: self.fresh_local(&"_panic".into(), TypeId::Void, ctx.locals),
                    rvalue: MirRvalue::Call {
                        func: "rt_panic".into(),
                        args: vec![MirOperand::ConstString(
                            "Sequence contains no elements".into(),
                        )],
                    },
                }],
            });
        }

        Some((stmts, result_local))
    }

    /// Shared array / List indexed loop used by terminal aggregations.
    fn emit_linq_source_loop(
        &mut self,
        chain: &LinqChain,
        src_elem_ty: &TypeId,
        var_local: LocalId,
        var_ty: &TypeId,
        body_stmts: Vec<MirStatement>,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) -> Option<()> {
        if let Some(count) = chain.source_len {
            let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
            let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
            let elem_local = self.fresh_local(&"_elem".into(), src_elem_ty.clone(), ctx.locals);
            stmts.push(MirStatement::Assign {
                place: count_local,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(count as i64)),
            });
            stmts.push(MirStatement::Assign {
                place: idx_local,
                rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
            });
            let mut while_body = Vec::new();
            while_body.push(MirStatement::Assign {
                place: elem_local,
                rvalue: MirRvalue::IndexGet {
                    array: chain.source.clone(),
                    index: MirOperand::Local(idx_local),
                    elem_type: src_elem_ty.clone(),
                },
            });
            self.apply_linq_ops(
                &chain.operators,
                0,
                elem_local,
                src_elem_ty,
                var_local,
                var_ty,
                body_stmts,
                ctx,
                &mut while_body,
            );
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
                foreach_source: Some(chain.source.clone()),
            });
            return Some(());
        }

        let (recv_type, _) = list_source_info(&chain.source, ctx)?;
        let count_local = self.fresh_local(&"_count".into(), TypeId::Int, ctx.locals);
        let idx_local = self.fresh_local(&"_idx".into(), TypeId::Int, ctx.locals);
        let elem_local = self.fresh_local(&"_elem".into(), src_elem_ty.clone(), ctx.locals);
        stmts.push(MirStatement::Assign {
            place: count_local,
            rvalue: MirRvalue::MethodCall {
                receiver: chain.source.clone(),
                method: "get_Count".to_string(),
                args: vec![],
                receiver_type: recv_type.clone(),
                impl_class: Some(recv_type.clone()),
                target_fn: Some(format!("{}::get_Count", recv_type)),
                is_virtual: false,
                params: vec![],
            },
        });
        stmts.push(MirStatement::Assign {
            place: idx_local,
            rvalue: MirRvalue::Use(MirOperand::ConstInt(0)),
        });
        let mut while_body = Vec::new();
        while_body.push(MirStatement::Assign {
            place: elem_local,
            rvalue: MirRvalue::MethodCall {
                receiver: chain.source.clone(),
                method: "get_Item".to_string(),
                args: vec![MirOperand::Local(idx_local)],
                receiver_type: recv_type.clone(),
                impl_class: Some(recv_type.clone()),
                target_fn: Some(format!("{}::get_Item", recv_type)),
                is_virtual: false,
                params: vec![],
            },
        });
        self.apply_linq_ops(
            &chain.operators,
            0,
            elem_local,
            src_elem_ty,
            var_local,
            var_ty,
            body_stmts,
            ctx,
            &mut while_body,
        );
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
            foreach_source: Some(chain.source.clone()),
        });
        Some(())
    }
}

fn zero_rvalue_for(ty: &TypeId) -> MirRvalue {
    match ty {
        TypeId::Bool => MirRvalue::Use(MirOperand::ConstBool(false)),
        TypeId::Float | TypeId::Double => MirRvalue::Use(MirOperand::ConstFloat(0.0)),
        TypeId::Named(_)
        | TypeId::String
        | TypeId::Object
        | TypeId::Array { .. }
        | TypeId::IEnumerable { .. }
        | TypeId::IQueryable { .. }
        | TypeId::Task { .. }
        | TypeId::Span { .. } => MirRvalue::Use(MirOperand::ConstNull),
        _ => MirRvalue::Use(MirOperand::ConstInt(0)),
    }
}

/// Walk stream ops to the projected element type after `Select` (Where/OrderBy
/// preserve the current element type).
fn projected_elem_ty(
    ops: &[LinqOp],
    src_elem: &TypeId,
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
) -> TypeId {
    let mut cur = src_elem.clone();
    for op in ops {
        if let LinqOp::Select(lambda) = op {
            ctx.push_scope();
            let param_name = lambda.params[0].name.clone();
            let param_local = builder.fresh_local(&param_name, cur.clone(), ctx.locals);
            ctx.bind(&param_name, param_local);
            cur = match &lambda.body {
                LambdaBody::Expr(e) => infer_type_from_spanned(e, ctx),
                LambdaBody::Block(b) => {
                    if let Some(tail) = &b.tail {
                        infer_type_from_spanned(tail, ctx)
                    } else {
                        cur.clone()
                    }
                }
            };
            ctx.pop_scope();
        }
    }
    cur
}

/// Build the comparison rvalue for two key values:
/// - primitives / bool / char → `<ty>.Compare` (codegen emits trinary −1/0/1)
/// - `string` → `string.Compare`
/// - named class → `CompareTo` instance call when the method resolves
/// - otherwise `None` (unsupported key type → keep the documented skip)
fn linq_key_compare_rvalue(
    key_ty: &TypeId,
    ka: LocalId,
    kb: LocalId,
    ctx: &LowerCtx,
) -> Option<MirRvalue> {
    let ka_op = MirOperand::Local(ka);
    let kb_op = MirOperand::Local(kb);
    match key_ty {
        TypeId::Int
        | TypeId::Long
        | TypeId::Short
        | TypeId::Byte
        | TypeId::UInt
        | TypeId::ULong
        | TypeId::UShort
        | TypeId::SByte
        | TypeId::Float
        | TypeId::Double
        | TypeId::Bool
        | TypeId::Char => {
            let name = type_id_to_field_name(key_ty);
            Some(MirRvalue::Call {
                func: format!("{name}.Compare"),
                args: vec![ka_op, kb_op],
            })
        }
        TypeId::String => Some(MirRvalue::Call {
            func: "string.Compare".into(),
            args: vec![ka_op, kb_op],
        }),
        TypeId::Named(class) => {
            let (impl_class, target_fn) =
                resolve_method_target(ctx.registry, class, &"CompareTo".into(), ctx.owner.clone());
            target_fn.as_ref()?;
            let is_virtual = is_virtual_member(ctx.layouts, class.as_ref(), "CompareTo", &[]);
            Some(MirRvalue::MethodCall {
                receiver: ka_op,
                method: "CompareTo".to_string(),
                args: vec![kb_op],
                receiver_type: class.to_string(),
                impl_class,
                target_fn,
                is_virtual,
                params: vec![],
            })
        }
        _ => None,
    }
}

/// Look up the compile-time length of an array local, if `source` is a `Local` bound
/// to an `ArrayLit` via `lower_let`.
fn array_len_of_operand(source: &MirOperand, ctx: &LowerCtx) -> Option<usize> {
    if let MirOperand::Local(id) = source {
        ctx.array_lengths.get(id).copied()
    } else {
        None
    }
}

/// Resolve the element type of an array-typed local, used to type the
/// per-iteration slot in `lower_linq_foreach`.
fn source_elem_ty(source: &MirOperand, ctx: &LowerCtx) -> Option<TypeId> {
    if let MirOperand::Local(id) = source {
        let (_, ty) = ctx.locals.get(id)?;
        if let TypeId::Array { elem } = ty {
            return Some((**elem).clone());
        }
    }
    None
}

/// If `source` is a `List_<T>` local, return `(receiver_type, elem_type)` so
/// `lower_linq_foreach` can emit `get_Count` / `Get(i)` method calls.
fn list_source_info(source: &MirOperand, ctx: &LowerCtx) -> Option<(String, TypeId)> {
    if let MirOperand::Local(id) = source {
        let (_, ty) = ctx.locals.get(id)?;
        if let TypeId::Named(name) = ty {
            if name.starts_with("List_") {
                let elem_ty = ty.enumerable_elem()?;
                return Some((name.to_string(), elem_ty));
            }
        }
    }
    None
}

/// `List_<T>` 的 mangled 类名（与 `materialize_chain_inner`/`prepare_linq_sort`
/// 的命名一致）。
fn list_class_of(ty: &TypeId) -> String {
    mangle_generic("List", std::slice::from_ref(ty))
}

fn collect_linq_method_ops(expr: &Expr) -> Option<(Expr, Vec<LinqOp>)> {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            let op = match method.as_str() {
                "Where" => LinqOp::Where(lambda_from_arg(args.first()?)?),
                "Select" => LinqOp::Select(lambda_from_arg(args.first()?)?),
                "OrderBy" => LinqOp::OrderBy {
                    key: lambda_from_arg(args.first()?)?,
                    descending: false,
                },
                "OrderByDescending" => LinqOp::OrderBy {
                    key: lambda_from_arg(args.first()?)?,
                    descending: true,
                },
                _ => return None,
            };
            match collect_linq_method_ops(&receiver.node) {
                Some((source, mut ops)) => {
                    ops.push(op);
                    Some((source, ops))
                }
                None => Some((receiver.node.clone(), vec![op])),
            }
        }
        _ => None,
    }
}

fn lambda_from_arg(arg: &Spanned<Expr>) -> Option<LambdaExpr> {
    match &arg.node {
        Expr::Lambda(l) => Some(l.clone()),
        body => Some(LambdaExpr {
            params: vec![LambdaParam {
                name: "x".into(),
                ty: None,
                default: None,
            }],
            body: LambdaBody::Expr(Box::new(Spanned::new(body.clone(), arg.span))),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        }),
    }
}

pub(super) fn builtin_static_method(receiver: &Expr, method: &Ident) -> Option<String> {
    let Expr::Ident(name) = receiver else {
        return None;
    };
    let name_str = name.as_str();
    // Stub facade 类（Console/File/Task/Assert/Window/...）：所有静态方法都
    // 走 `Class.Method` 路由（`.` 分隔符），由 codegen 拦截器决定是否
    // 发射 ABI。未识别的方法 fall through 到用户函数路径，但 stub 方法
    // 体为空（行为靠 codegen 拦截器实现），不会影响语义。
    //
    // 不在此拦截会走 `user_type_static_method_func` → `Class::Method`
    // （`::` 分隔符），codegen `strip_prefix("Class.")` 拦截失败，调用
    // 空 stub 用户函数（行为错误，如 `Console.Write` 不输出、
    // `Path.Combine` 返回未定义值导致后续崩溃）。
    //
    // `is_builtin_facade` 是 stub facade 类的单一事实源（typeck 跳过方法体
    // 类型检查 + MIR lower 路由静态调用都依赖此函数）。新增 facade 类时
    // 只需在 `is_builtin_facade` 中追加。
    if typeck::is_builtin_facade(name_str) {
        return Some(format!("{name_str}.{method}"));
    }
    // RFC 032 M1：基元类型 static abstract 方法拦截。
    // 单态化后 `T.Add(a, b)` 已被 `substitute_expr` 替换为 `int.Add(a, b)` 等。
    // MIR lower 在此将 `int.Add` / `double.Multiply` 等转为
    // `MirRvalue::Call { func: "int.Add" }`，codegen `try_emit_primitive_static`
    // 拦截器直接发射 LLVM `add`/`fadd`/`mul`/`sdiv`/`fdiv` 等指令（零运行时开销）。
    // 方法名与 INumber<T>/IAddable<T>/ISubtractable<T>/IMultiplicable<T>/IDivisible<T>
    // /IEquatable<T>/IHashable<T>/IComparable<T> 接口契约一致。
    if is_primitive_numeric_or_string_type(name) {
        return primitive_static_func_name(name, method.as_str()).map(|s| s.to_string());
    }
    None
}

/// RFC 032 M1：判定 name 是否为支持 static abstract 接口的基元类型。
///
/// 数值类型（int/long/short/byte/float/double）支持 INumber/IAddable/ISubtractable
/// /IMultiplicable/IDivisible/IEquatable/IHashable/IComparable 全套。
/// bool/char/string 仅支持 IEquatable/IHashable/IComparable 子集（但 M1 范围内
/// 暂只处理数值类型；bool/char/string 的 static abstract 调用走通用方法解析路径）。
fn is_primitive_numeric_or_string_type(name: &Ident) -> bool {
    matches!(
        name.as_str(),
        "int"
            | "long"
            | "short"
            | "byte"
            | "float"
            | "double"
            | "uint"
            | "ulong"
            | "ushort"
            | "sbyte"
            | "bool"
            | "char"
            | "string"
    )
}

/// RFC 032 M1：判定 name 是否为数值基元类型（不含 bool/char/string）。
///
/// 用于 `Zero` / `One` 属性拦截——这些属性仅在 `INumber<T>` 接口上声明，
/// bool/char/string 不实现 `INumber<T>`，因此不应进入 `T.Zero` 拦截路径。
pub(super) fn is_primitive_numeric_type(name: &Ident) -> bool {
    matches!(
        name.as_str(),
        "int"
            | "long"
            | "short"
            | "byte"
            | "float"
            | "double"
            | "uint"
            | "ulong"
            | "ushort"
            | "sbyte"
    )
}

/// RFC 032 M1：基元类型 static abstract 方法的 func 名映射表。
///
/// 返回 `<type>.<method>` 形式的静态字符串，供 codegen `try_emit_primitive_static`
/// 拦截器识别。所有组合均为有限可枚举集，避免动态字符串分配。
fn primitive_static_func_name(type_name: &Ident, method: &str) -> Option<&'static str> {
    // 编译期 const 表——type × method → func 名。
    // 用 const 静态字符串数组避免 String 分配。
    // 注：M1 范围内只覆盖数值类型的 INumber 方法 + 通用 Equals/GetHashCode/Compare。
    match (type_name.as_str(), method) {
        ("int", "Add") => Some("int.Add"),
        ("int", "Subtract") => Some("int.Subtract"),
        ("int", "Multiply") => Some("int.Multiply"),
        ("int", "Divide") => Some("int.Divide"),
        ("int", "Negate") => Some("int.Negate"),
        ("int", "Equals") => Some("int.Equals"),
        ("int", "GetHashCode") => Some("int.GetHashCode"),
        ("int", "Compare") => Some("int.Compare"),
        ("int", "Parse") => Some("int.Parse"),
        ("int", "TryParse") => Some("int.TryParse"),
        ("int", "ToString") => Some("int.ToString"),
        ("int", "Zero") => Some("int.Zero"),
        ("int", "One") => Some("int.One"),
        ("int", "MinValue") => Some("int.MinValue"),
        ("int", "MaxValue") => Some("int.MaxValue"),
        ("long", "Add") => Some("long.Add"),
        ("long", "Subtract") => Some("long.Subtract"),
        ("long", "Multiply") => Some("long.Multiply"),
        ("long", "Divide") => Some("long.Divide"),
        ("long", "Negate") => Some("long.Negate"),
        ("long", "Equals") => Some("long.Equals"),
        ("long", "GetHashCode") => Some("long.GetHashCode"),
        ("long", "Compare") => Some("long.Compare"),
        ("long", "Parse") => Some("long.Parse"),
        ("long", "TryParse") => Some("long.TryParse"),
        ("long", "ToString") => Some("long.ToString"),
        ("long", "Zero") => Some("long.Zero"),
        ("long", "One") => Some("long.One"),
        ("long", "MinValue") => Some("long.MinValue"),
        ("long", "MaxValue") => Some("long.MaxValue"),
        ("short", "Add") => Some("short.Add"),
        ("short", "Subtract") => Some("short.Subtract"),
        ("short", "Multiply") => Some("short.Multiply"),
        ("short", "Divide") => Some("short.Divide"),
        ("short", "Negate") => Some("short.Negate"),
        ("short", "Equals") => Some("short.Equals"),
        ("short", "GetHashCode") => Some("short.GetHashCode"),
        ("short", "Compare") => Some("short.Compare"),
        ("short", "Parse") => Some("short.Parse"),
        ("short", "TryParse") => Some("short.TryParse"),
        ("short", "ToString") => Some("short.ToString"),
        ("short", "Zero") => Some("short.Zero"),
        ("short", "One") => Some("short.One"),
        ("short", "MinValue") => Some("short.MinValue"),
        ("short", "MaxValue") => Some("short.MaxValue"),
        ("byte", "Add") => Some("byte.Add"),
        ("byte", "Subtract") => Some("byte.Subtract"),
        ("byte", "Multiply") => Some("byte.Multiply"),
        ("byte", "Divide") => Some("byte.Divide"),
        ("byte", "Negate") => Some("byte.Negate"),
        ("byte", "Equals") => Some("byte.Equals"),
        ("byte", "GetHashCode") => Some("byte.GetHashCode"),
        ("byte", "Compare") => Some("byte.Compare"),
        ("byte", "Parse") => Some("byte.Parse"),
        ("byte", "TryParse") => Some("byte.TryParse"),
        ("byte", "ToString") => Some("byte.ToString"),
        ("byte", "Zero") => Some("byte.Zero"),
        ("byte", "One") => Some("byte.One"),
        ("byte", "MinValue") => Some("byte.MinValue"),
        ("byte", "MaxValue") => Some("byte.MaxValue"),
        ("float", "Add") => Some("float.Add"),
        ("float", "Subtract") => Some("float.Subtract"),
        ("float", "Multiply") => Some("float.Multiply"),
        ("float", "Divide") => Some("float.Divide"),
        ("float", "Negate") => Some("float.Negate"),
        ("float", "Equals") => Some("float.Equals"),
        ("float", "GetHashCode") => Some("float.GetHashCode"),
        ("float", "Compare") => Some("float.Compare"),
        ("float", "Parse") => Some("float.Parse"),
        ("float", "TryParse") => Some("float.TryParse"),
        ("float", "ToString") => Some("float.ToString"),
        ("float", "Zero") => Some("float.Zero"),
        ("float", "One") => Some("float.One"),
        ("float", "MinValue") => Some("float.MinValue"),
        ("float", "MaxValue") => Some("float.MaxValue"),
        ("float", "Epsilon") => Some("float.Epsilon"),
        ("float", "NaN") => Some("float.NaN"),
        ("float", "PositiveInfinity") => Some("float.PositiveInfinity"),
        ("float", "NegativeInfinity") => Some("float.NegativeInfinity"),
        ("double", "Add") => Some("double.Add"),
        ("double", "Subtract") => Some("double.Subtract"),
        ("double", "Multiply") => Some("double.Multiply"),
        ("double", "Divide") => Some("double.Divide"),
        ("double", "Negate") => Some("double.Negate"),
        ("double", "Equals") => Some("double.Equals"),
        ("double", "GetHashCode") => Some("double.GetHashCode"),
        ("double", "Compare") => Some("double.Compare"),
        ("double", "Parse") => Some("double.Parse"),
        ("double", "TryParse") => Some("double.TryParse"),
        ("double", "ToString") => Some("double.ToString"),
        ("double", "Zero") => Some("double.Zero"),
        ("double", "One") => Some("double.One"),
        ("double", "MinValue") => Some("double.MinValue"),
        ("double", "MaxValue") => Some("double.MaxValue"),
        ("double", "Epsilon") => Some("double.Epsilon"),
        ("double", "NaN") => Some("double.NaN"),
        ("double", "PositiveInfinity") => Some("double.PositiveInfinity"),
        ("double", "NegativeInfinity") => Some("double.NegativeInfinity"),
        ("uint", "Add") => Some("uint.Add"),
        ("uint", "Subtract") => Some("uint.Subtract"),
        ("uint", "Multiply") => Some("uint.Multiply"),
        ("uint", "Divide") => Some("uint.Divide"),
        ("uint", "Negate") => Some("uint.Negate"),
        ("uint", "Equals") => Some("uint.Equals"),
        ("uint", "GetHashCode") => Some("uint.GetHashCode"),
        ("uint", "Compare") => Some("uint.Compare"),
        ("uint", "Parse") => Some("uint.Parse"),
        ("uint", "TryParse") => Some("uint.TryParse"),
        ("uint", "ToString") => Some("uint.ToString"),
        ("uint", "Zero") => Some("uint.Zero"),
        ("uint", "One") => Some("uint.One"),
        ("uint", "MinValue") => Some("uint.MinValue"),
        ("uint", "MaxValue") => Some("uint.MaxValue"),
        ("ulong", "Add") => Some("ulong.Add"),
        ("ulong", "Subtract") => Some("ulong.Subtract"),
        ("ulong", "Multiply") => Some("ulong.Multiply"),
        ("ulong", "Divide") => Some("ulong.Divide"),
        ("ulong", "Negate") => Some("ulong.Negate"),
        ("ulong", "Equals") => Some("ulong.Equals"),
        ("ulong", "GetHashCode") => Some("ulong.GetHashCode"),
        ("ulong", "Compare") => Some("ulong.Compare"),
        ("ulong", "Parse") => Some("ulong.Parse"),
        ("ulong", "TryParse") => Some("ulong.TryParse"),
        ("ulong", "ToString") => Some("ulong.ToString"),
        ("ulong", "Zero") => Some("ulong.Zero"),
        ("ulong", "One") => Some("ulong.One"),
        ("ulong", "MinValue") => Some("ulong.MinValue"),
        ("ulong", "MaxValue") => Some("ulong.MaxValue"),
        ("ushort", "Add") => Some("ushort.Add"),
        ("ushort", "Subtract") => Some("ushort.Subtract"),
        ("ushort", "Multiply") => Some("ushort.Multiply"),
        ("ushort", "Divide") => Some("ushort.Divide"),
        ("ushort", "Negate") => Some("ushort.Negate"),
        ("ushort", "Equals") => Some("ushort.Equals"),
        ("ushort", "GetHashCode") => Some("ushort.GetHashCode"),
        ("ushort", "Compare") => Some("ushort.Compare"),
        ("ushort", "Parse") => Some("ushort.Parse"),
        ("ushort", "TryParse") => Some("ushort.TryParse"),
        ("ushort", "ToString") => Some("ushort.ToString"),
        ("ushort", "Zero") => Some("ushort.Zero"),
        ("ushort", "One") => Some("ushort.One"),
        ("ushort", "MinValue") => Some("ushort.MinValue"),
        ("ushort", "MaxValue") => Some("ushort.MaxValue"),
        ("sbyte", "Add") => Some("sbyte.Add"),
        ("sbyte", "Subtract") => Some("sbyte.Subtract"),
        ("sbyte", "Multiply") => Some("sbyte.Multiply"),
        ("sbyte", "Divide") => Some("sbyte.Divide"),
        ("sbyte", "Negate") => Some("sbyte.Negate"),
        ("sbyte", "Equals") => Some("sbyte.Equals"),
        ("sbyte", "GetHashCode") => Some("sbyte.GetHashCode"),
        ("sbyte", "Compare") => Some("sbyte.Compare"),
        ("sbyte", "Parse") => Some("sbyte.Parse"),
        ("sbyte", "TryParse") => Some("sbyte.TryParse"),
        ("sbyte", "ToString") => Some("sbyte.ToString"),
        ("sbyte", "Zero") => Some("sbyte.Zero"),
        ("sbyte", "One") => Some("sbyte.One"),
        ("sbyte", "MinValue") => Some("sbyte.MinValue"),
        ("sbyte", "MaxValue") => Some("sbyte.MaxValue"),
        ("bool", "Equals") => Some("bool.Equals"),
        ("bool", "GetHashCode") => Some("bool.GetHashCode"),
        ("bool", "Compare") => Some("bool.Compare"),
        ("bool", "Parse") => Some("bool.Parse"),
        ("bool", "TryParse") => Some("bool.TryParse"),
        ("bool", "ToString") => Some("bool.ToString"),
        ("char", "Equals") => Some("char.Equals"),
        ("char", "GetHashCode") => Some("char.GetHashCode"),
        ("char", "Compare") => Some("char.Compare"),
        ("char", "Parse") => Some("char.Parse"),
        ("char", "TryParse") => Some("char.TryParse"),
        ("char", "ToString") => Some("char.ToString"),
        ("char", "IsDigit") => Some("char.IsDigit"),
        ("char", "IsLetter") => Some("char.IsLetter"),
        ("char", "IsWhiteSpace") => Some("char.IsWhiteSpace"),
        ("char", "IsUpper") => Some("char.IsUpper"),
        ("char", "IsLower") => Some("char.IsLower"),
        ("char", "ToUpper") => Some("char.ToUpper"),
        ("char", "ToLower") => Some("char.ToLower"),
        ("string", "Equals") => Some("string.Equals"),
        ("string", "GetHashCode") => Some("string.GetHashCode"),
        ("string", "Compare") => Some("string.Compare"),
        ("string", "CompareOrdinal") => Some("string.CompareOrdinal"),
        ("string", "IsNullOrEmpty") => Some("string.IsNullOrEmpty"),
        ("string", "IsNullOrWhiteSpace") => Some("string.IsNullOrWhiteSpace"),
        ("string", "FromCharCount") => Some("string.FromCharCount"),
        ("string", "Format") => Some("string.Format"),
        ("string", "Concat") => Some("string.Concat"),
        ("string", "Join") => Some("string.Join"),
        _ => None,
    }
}
