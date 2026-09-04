use ast::*;
use indexmap::IndexMap;

use ast::ExpressionTree;

use crate::check_expr::demangle_func_type_with;
use crate::checker::check_native::box_to_object;
use crate::checker::TypeChecker;
use crate::error::TypeError;
use crate::type_id::TypeId;
use crate::typed::{TypedBlock, TypedStmt};

impl TypeChecker {
    pub(crate) fn check_expression_lambda(
        &mut self,
        l: &LambdaExpr,
        elem_ty: Option<TypeId>,
    ) -> Result<ExpressionTree, TypeError> {
        if l.is_expression_tree {
            return Err(TypeError::QueryableRequiresExpression);
        }
        Self::reject_lambda_defaults_outside_iife(&l.params)?;
        if let Some(elem) = elem_ty {
            self.scopes.push(IndexMap::new());
            for p in &l.params {
                let pty =
                    p.ty.as_ref()
                        .map(|t| self.lower_type(&t.node))
                        .transpose()?
                        .unwrap_or_else(|| elem.clone());
                self.scopes.last_mut().unwrap().insert(p.name.clone(), pty);
            }
            match &l.body {
                LambdaBody::Expr(e) => {
                    self.check_expr_at(e.span, &e.node)?;
                }
                LambdaBody::Block(b) => {
                    for stmt in &b.stmts {
                        self.check_stmt(&stmt.node)?;
                    }
                    if let Some(tail) = &b.tail {
                        self.check_expr_at(tail.span, &tail.node)?;
                    }
                }
            }
            self.scopes.pop();
        }
        ExpressionTree::from_lambda(l, &[]).ok_or(TypeError::QueryableRequiresExpression)
    }

    fn expression_func_elem(ty: &TypeId) -> Option<TypeId> {
        match ty {
            TypeId::Expression { inner } => match inner.as_ref() {
                TypeId::Func { params, .. } if params.len() == 1 => Some(params[0].clone()),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn check_func_lambda(
        &mut self,
        l: &LambdaExpr,
        params: &[TypeId],
        ret: &TypeId,
    ) -> Result<(), TypeError> {
        if l.is_expression_tree {
            return Err(TypeError::QueryableRequiresExpression);
        }
        Self::reject_lambda_defaults_outside_iife(&l.params)?;
        if l.params.len() != params.len() {
            return Err(TypeError::Mismatch {
                expected: format!("{} parameter(s)", params.len()),
                found: format!("{} parameter(s)", l.params.len()),
            });
        }
        // RFC 009 M6: async lambda 鐨勮繑鍥炵被鍨嬫槸 `Task<T>`锛宐ody 鏈熸湜杩斿洖 `T`銆?
        // 鍚屾椂璁剧疆 `in_async = true` 浣?body 鍐呯殑 `await` 鍚堟硶銆?
        let body_expected: TypeId = if l.is_async {
            ret.task_inner().cloned().unwrap_or(TypeId::Void)
        } else {
            ret.clone()
        };
        let prev_async = self.in_async;
        self.in_async = l.is_async;
        // RFC 009 M6: block-body lambda 鐨?`return` 璇彞闇€瑕佹纭殑 return_slot銆?
        // 鎺ㄥ叆 body_expected 浣?`return expr` 妫€鏌ヤ笌 lambda 杩斿洖绫诲瀷鍖归厤銆?
        self.return_slot.push(body_expected.clone());
        self.scopes.push(IndexMap::new());
        for (i, p) in l.params.iter().enumerate() {
            let pty =
                p.ty.as_ref()
                    .map(|t| self.lower_type(&t.node))
                    .transpose()?
                    .unwrap_or_else(|| params[i].clone());
            self.scopes.last_mut().unwrap().insert(p.name.clone(), pty);
        }
        let result = match &l.body {
            LambdaBody::Expr(e) => {
                let checked = self.check_expr_at(e.span, &e.node)?;
                if !self.types_compatible(&body_expected, &checked.ty) {
                    return Err(TypeError::Mismatch {
                        expected: body_expected.display(),
                        found: checked.ty.display(),
                    });
                }
                Ok(())
            }
            LambdaBody::Block(b) => {
                for stmt in &b.stmts {
                    self.check_stmt(&stmt.node)?;
                }
                if let Some(tail) = &b.tail {
                    let checked = self.check_expr_at(tail.span, &tail.node)?;
                    if !self.types_compatible(&body_expected, &checked.ty) {
                        return Err(TypeError::Mismatch {
                            expected: body_expected.display(),
                            found: checked.ty.display(),
                        });
                    }
                }
                Ok(())
            }
        };
        self.scopes.pop();
        self.return_slot.pop();
        self.in_async = prev_async;
        result
    }
    pub(crate) fn check_block(
        &mut self,
        block: &Block,
        expected_ret: &TypeId,
    ) -> Result<TypedBlock, TypeError> {
        let mut typed_stmts = Vec::new();
        for stmt in &block.stmts {
            match &stmt.node {
                // RFC 004 M2锛氬睍寮€涓?Let锛堝０鏄?寮冨厓锛? Deconstruct MethodCall
                Stmt::DeconstructAssign {
                    declare,
                    targets,
                    value,
                } => {
                    typed_stmts.extend(
                        self.check_deconstruct_assign(*declare, targets, value, stmt.span)?,
                    );
                }
                // RFC 005 搂7.3锛歚lock (expr) { }` 鈫?Enter + try/finally Exit
                Stmt::Lock { expr, body } => {
                    typed_stmts.extend(self.check_lock_stmt(expr, body, stmt.span)?);
                }
                other => typed_stmts.push(self.check_stmt(other)?),
            }
        }
        let checked_tail = if let Some(tail) = &block.tail {
            let te = self.check_expr_at(tail.span, &tail.node)?;
            if !self.types_compatible(expected_ret, &te.ty) && !matches!(expected_ret, TypeId::Void)
            {
                return Err(TypeError::Mismatch {
                    expected: expected_ret.display(),
                    found: te.ty.display(),
                });
            }
            // RFC 045 P3：保留 tail 重写（收窄 Cast / Unbox），供 MIR 重下降。
            Some(Box::new(Spanned::new(te.expr, tail.span)))
        } else {
            None
        };
        Ok(TypedBlock {
            stmts: typed_stmts,
            tail: checked_tail,
        })
    }

    pub(crate) fn check_stmt(&mut self, stmt: &Stmt) -> Result<TypedStmt, TypeError> {
        match stmt {
            Stmt::Let {
                mutable: _,
                name,
                ty,
                init,
            } => {
                let declared = ty
                    .as_ref()
                    .map(|t| self.lower_type(&t.node))
                    .transpose()?
                    .unwrap_or(TypeId::Infer);
                // RFC 016 v2 M2 / RFC 016 M3锛氫繚瀛?check_expr 閲嶅啓鍚庣殑琛ㄨ揪寮?
                // 锛堝 FFI 瑁呯鎻掑叆鐨?Expr::Box锛夛紝浼犻€掑埌 TypedStmt::Let銆?
                // 浠呭湪璧?check_expr 鐨勮矾寰勶紙else 鍒嗘敮锛夋湁鍊硷紱鍏朵粬璺緞锛圠ambda/
                // 绌洪泦鍚堬級淇濈暀鍘?init銆?
                let mut rewritten_init: Option<Spanned<Expr>> = None;
                let final_ty = if let Some(init) = init {
                    if matches!(declared, TypeId::Expression { .. }) {
                        // RFC 008 M3锛氭柟娉曠粍 鈫?Expression 纭嫆缁濓紙椤绘樉寮?lambda锛夈€?
                        if !matches!(init.node, Expr::Lambda(_)) {
                            self.reject_method_group_to_expression(&init.node)?;
                        }
                        if matches!(init.node, Expr::Lambda(_)) {
                            if let Expr::Lambda(l) = &init.node {
                                let elem = Self::expression_func_elem(&declared);
                                self.check_expression_lambda(l, elem)?;
                            }
                            self.canonical_type(&declared)
                        } else {
                            let checked = self.check_expr_at(init.span, &init.node)?;
                            if !self.types_compatible(&declared, &checked.ty) {
                                return Err(TypeError::Mismatch {
                                    expected: declared.display(),
                                    found: checked.ty.display(),
                                });
                            }
                            rewritten_init = Some(Spanned::new(checked.expr, init.span));
                            self.canonical_type(&declared)
                        }
                    } else if let TypeId::Func { params, ret } = &declared {
                        // RFC 004 M1锛氶潪 lambda 椤诲畬鏁存鏌ワ紱鏂规硶缁勮劚绯栦负 lambda銆?
                        // 绂佹鏃ц涓猴細璺宠繃 init 瀵艰嚧 NoSuch/绛惧悕閿欓潤榛橀€氳繃銆?
                        if let Expr::Lambda(l) = &init.node {
                            self.check_func_lambda(l, params, ret)?;
                        } else if let Some((lambda, _)) =
                            self.try_method_group_to_lambda(&init.node, Some((params, ret)))?
                        {
                            self.check_func_lambda(&lambda, params, ret)?;
                            rewritten_init = Some(Spanned::new(Expr::Lambda(lambda), init.span));
                        } else {
                            self.reject_deferred_method_group(&init.node)?;
                            let checked = self.check_expr_at(init.span, &init.node)?;
                            if !self.types_compatible(&declared, &checked.ty) {
                                return Err(TypeError::Mismatch {
                                    expected: declared.display(),
                                    found: checked.ty.display(),
                                });
                            }
                            rewritten_init = Some(Spanned::new(checked.expr, init.span));
                        }
                        self.canonical_type(&declared)
                    } else if matches!(
                        init.node,
                        Expr::CollectionExpr { ref elements } if elements.is_empty()
                    ) && matches!(declared, TypeId::Array { .. })
                    {
                        // Empty collection `[]` with declared array type uses the declared
                        // type (e.g., `int[] empty = []` 鈫?int[]). Only `var x = []` falls
                        // back to object[].
                        self.canonical_type(&declared)
                    } else {
                        // RFC 065锛氭樉寮忕被鍨嬪眬閮ㄤ笂鐨勭洰鏍囩被鍨?`new()`銆?
                        // RFC 017锛歚List<T> x = [鈥;` 闆嗗悎鐩爣鑴辩硸銆?
                        let prepared = if !matches!(declared, TypeId::Infer) {
                            self.prepare_target_expr(&init.node, &declared, init.span)?
                        } else {
                            init.node.clone()
                        };
                        // RFC 017 + RFC 005: array-target collection expr binds by element type
                        // (e.g. byte[] = [1,2,3]) without weakening array invariance.
                        if !matches!(declared, TypeId::Infer)
                            && self.try_bind_collection_array_target(&prepared, &declared)?
                        {
                            rewritten_init = Some(Spanned::new(prepared, init.span));
                            self.canonical_type(&declared)
                        } else {
                            let checked = self.check_expr(&prepared)?;
                            // RFC 004 搂D9 / RFC 037 M2锛歵ypes_compatible 澶辫触鏃跺皾璇?
                            // 闅愬紡 variant 鏋勯€狅紙濡?`ContentVariant c = "Click"` 鈫?
                            // `ContentVariant.Text("Click")`锛夈€傛涔?/ 鏃犲尮閰嶅垯鍥為€€鍒?
                            // 鍘熷绫诲瀷涓嶅尮閰嶉敊璇€?
                            let (init_expr, init_ty) = if !matches!(declared, TypeId::Infer)
                                && !self.types_compatible(&declared, &checked.ty)
                            {
                                match self.coerce_to_variant(
                                    checked.expr.clone(),
                                    &checked.ty,
                                    &declared,
                                ) {
                                    Some(coerced) => (coerced, self.canonical_type(&declared)),
                                    None => {
                                        return Err(TypeError::Mismatch {
                                            expected: declared.display(),
                                            found: checked.ty.display(),
                                        });
                                    }
                                }
                            } else if matches!(declared, TypeId::Infer) {
                                (checked.expr, self.canonical_type(&checked.ty))
                            } else {
                                // RFC 004 P0 Phase 1：object 局部声明接 string/基元 → 装箱。
                                let param_ty = self.type_name_of(&declared).unwrap_or_default();
                                let boxed = box_to_object(
                                    &self.registry,
                                    checked.expr,
                                    &checked.ty,
                                    param_ty.as_str(),
                                    init.span,
                                );
                                (boxed, self.canonical_type(&declared))
                            };
                            rewritten_init = Some(Spanned::new(init_expr, init.span));
                            init_ty
                        }
                    }
                } else {
                    self.canonical_type(&declared)
                };
                if init.is_none()
                    && ty.is_some()
                    && self.is_nullable_ref_type(&final_ty)
                    && !final_ty.is_nullable()
                {
                    // RFC 067锛歚Deconstruct(out 鈥?` 鑴辩硸鐢ㄧ殑 `__pos_*` / `__discard_*`
                    // 鍦ㄥ悓鍧楅殢鍚庣敱 out 瀹炲弬璧嬪€硷紱涓?`check_deconstruct_assign` 鐩存彃
                    // TypedStmt::Let{init:None} 瀵归綈锛屼笉鍦ㄦ纭嫆銆?
                    let synth = {
                        let s = name.as_str();
                        s.starts_with("__pos_") || s.starts_with("__discard_")
                    };
                    if !synth {
                        return Err(TypeError::UninitializedNonNull(name.to_string()));
                    }
                }
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), final_ty.clone());
                let final_init = rewritten_init.or_else(|| init.clone());
                Ok(TypedStmt::Let {
                    name: name.clone(),
                    ty: final_ty,
                    init: final_init,
                })
            }
            Stmt::Expr(e) => {
                // RFC 016 v2 M2 / RFC 016 M3锛氫娇鐢?TypedExpr.expr 鑰岄潪鍘熷 expr锛?
                // 淇濊瘉 typeck 閲嶅啓鍚庣殑 AST 鑺傜偣锛堝 FFI 瑁呯鎻掑叆鐨?Expr::Box銆?
                // Cast鈫扷nbox 杞崲锛夎兘浼犻€掑埌 MIR lower銆?
                let checked = self.check_expr_at(e.span, &e.node)?;
                // RFC 037 M-D0 强化：订阅返回的退订 token 不得作为裸表达式语句丢弃（G2 编译期拒绝）。
                self.reject_discarded_subscribe_token(&e.node)?;
                Ok(TypedStmt::Expr(Spanned::new(checked.expr, e.span)))
            }
            Stmt::Return(val) => {
                let expected = self.return_slot.last().cloned().unwrap_or(TypeId::Void);
                // out 形参确定性赋值检查须在 return 表达式求值**之后**进行：
                // `return dict.TryGetValue(k, out v);` 的 `v` 由 RefArg 求值路径
                // `mark_assigned`，若先检查再求值会把尚未定值的 `v` 误判为未赋值
                // （out 形参转发缺陷）。`return;`（无值）分支不受影响。
                match val {
                    None => {
                        if !matches!(self.canonical_type(&expected), TypeId::Void) {
                            return Err(TypeError::MissingReturnValue {
                                expected: expected.display(),
                            });
                        }
                        if let Some(flow) = &self.out_flow {
                            let missing = flow.unassigned();
                            if !missing.is_empty() {
                                return Err(TypeError::Oop(format!(
                                    "out parameter `{}` must be assigned before control leaves the current method",
                                    missing[0]
                                )));
                            }
                        }
                        Ok(TypedStmt::Return(None))
                    }
                    Some(v) => {
                        // RFC 004 M1锛歚return Foo;` 鏂规硶缁?鈫?lambda銆?
                        let after_mg = self.maybe_coerce_method_group(&v.node, &expected)?;
                        // RFC 065锛歚return new(...)` 鎸夎繑鍥炵被鍨嬪～鐩爣绫诲瀷銆?
                        // RFC 017锛歚return [鈥;` 鈫?`List<T>` 鐩爣鑴辩硸銆?
                        let prepared = self.prepare_target_expr(&after_mg, &expected, v.span)?;
                        let checked = self.check_expr(&prepared)?;
                        let ty = checked.ty;
                        if matches!(self.canonical_type(&expected), TypeId::Void) {
                            return Err(TypeError::VoidReturnWithValue(ty.display()));
                        }
                        // RFC 004 搂D9 / RFC 037 M2锛歵ypes_compatible 澶辫触鏃跺皾璇?
                        // 闅愬紡 variant 鏋勯€狅紙濡?`return "Click";` 鍦ㄨ繑鍥炵被鍨嬩负
                        // `ContentVariant` 鐨勫嚱鏁颁腑 鈫?`return ContentVariant.Text("Click");`锛夈€?
                        let final_expr = if !self.types_compatible(&expected, &ty) {
                            match self.coerce_to_variant(checked.expr.clone(), &ty, &expected) {
                                Some(coerced) => coerced,
                                None => {
                                    return Err(TypeError::Mismatch {
                                        expected: expected.display(),
                                        found: ty.display(),
                                    });
                                }
                            }
                        } else {
                            // RFC 004 P0 Phase 1：object 返回类型接 string/基元 → 装箱。
                            let param_ty = self.type_name_of(&expected).unwrap_or_default();
                            box_to_object(
                                &self.registry,
                                checked.expr,
                                &ty,
                                param_ty.as_str(),
                                v.span,
                            )
                        };
                        // RFC 016 v2 M2锛氫娇鐢?TypedExpr.expr 浼犻€掗噸鍐欏悗鐨?AST
                        // 锛堝 Cast鈫扷nbox 杞崲銆丗FI 瑁呯鑺傜偣銆乿ariant 闅愬紡鏋勯€狅級銆?
                        if let Some(flow) = &self.out_flow {
                            let missing = flow.unassigned();
                            if !missing.is_empty() {
                                return Err(TypeError::Oop(format!(
                                    "out parameter `{}` must be assigned before control leaves the current method",
                                    missing[0]
                                )));
                            }
                        }
                        Ok(TypedStmt::Return(Some(Spanned::new(final_expr, v.span))))
                    }
                }
            }
            Stmt::While { cond, body } => {
                let checked = self.check_expr_at(cond.span, &cond.node)?;
                self.loop_depth += 1;
                let typed_body = self.check_block(body, &TypeId::Void)?;
                self.loop_depth -= 1;
                Ok(TypedStmt::While {
                    cond: Spanned::new(checked.expr, cond.span),
                    body: typed_body,
                })
            }
            Stmt::For { var, iter, body } => {
                let checked_iter = self.check_expr_at(iter.span, &iter.node)?;
                let iter_ty = checked_iter.ty;
                let elem = iter_ty.enumerable_elem().unwrap_or(TypeId::Infer);
                self.scopes
                    .push(IndexMap::from([(var.clone(), elem.clone())]));
                self.loop_depth += 1;
                let typed_body = self.check_block(body, &TypeId::Void)?;
                self.loop_depth -= 1;
                self.scopes.pop();
                Ok(TypedStmt::For {
                    var: var.clone(),
                    elem_ty: elem,
                    iter: Spanned::new(checked_iter.expr, iter.span),
                    body: typed_body,
                })
            }
            Stmt::ForC {
                init,
                cond,
                inc,
                body,
            } => {
                // init clause 鈥?type-check inline; introduces scope
                self.scopes.push(IndexMap::new());
                if let Some(ref init_stmt) = init {
                    self.check_stmt(&init_stmt.node)?;
                }
                // cond clause 鈥?must be bool if present
                let typed_cond = if let Some(ref c) = cond {
                    let checked = self.check_expr_at(c.span, &c.node)?;
                    Some(Spanned::new(checked.expr, c.span))
                } else {
                    None
                };
                // body
                self.loop_depth += 1;
                let typed_body = self.check_block(body, &TypeId::Void)?;
                // inc clause 鈥?type-check inline (inside loop scope)
                if let Some(ref inc_stmt) = inc {
                    self.check_stmt(&inc_stmt.node)?;
                }
                self.loop_depth -= 1;
                self.scopes.pop();
                Ok(TypedStmt::ForC {
                    init: init
                        .as_ref()
                        .map(|s| Spanned::new((*s.node).clone(), s.span)),
                    cond: typed_cond,
                    inc: inc
                        .as_ref()
                        .map(|s| Spanned::new((*s.node).clone(), s.span)),
                    body: typed_body,
                })
            }
            Stmt::Assign { target, value } => {
                // Rewrite bare instance field target: `_field = v` 鈫?`this._field = v`.
                let target = if let Expr::Ident(name) = &target.node {
                    if let Some(field_expr) = self.rewrite_bare_instance_field(name) {
                        Spanned::new(field_expr, target.span)
                    } else {
                        target.clone()
                    }
                } else {
                    target.clone()
                };
                // place 校验：赋值目标只能是变量 / 字段 / 索引 / null 条件成员——
                // 其余形态（lambda、调用、字面量等）响亮拒绝。此前兜底路径对
                // 任意目标放行，MIR 降级再静默丢弃，赋值凭空消失。
                match &target.node {
                    Expr::Ident(_)
                    | Expr::Field { .. }
                    | Expr::Index { .. }
                    | Expr::NullCond { .. } => {}
                    _ => {
                        return Err(TypeError::Oop(
                            "assignment target must be a variable, field, indexer, or null-conditional member"
                                .into(),
                        ));
                    }
                }
                // RFC 074锛歚recv?.member = expr` 鈥?璇彞褰㈢┖鏉′欢璧嬪€笺€?
                if let Expr::NullCond { access } = &target.node {
                    return self.check_null_cond_assign(&target, access, value);
                }
                // `string` 涓嶅彲鍙橈細鎷掔粷 `s[i] = c`锛圕# 鍚屼负鍙 Chars 绱㈠紩鍣級銆?
                if let Expr::Index { receiver, .. } = &target.node {
                    let recv = self.check_expr_at(receiver.span, &receiver.node)?;
                    if recv.ty == TypeId::String {
                        return Err(TypeError::Oop("string indexer is read-only".into()));
                    }
                    // RFC 005 V2锛歚ReadOnlySpan` 绱㈠紩鍙銆?
                    if matches!(recv.ty, TypeId::Span { mutable: false, .. }) {
                        return Err(TypeError::Oop("ReadOnlySpan indexer is read-only".into()));
                    }
                }
                // RFC 005 B3 / V5锛氱姝㈠皢 Span 鍐欏叆 class 瀛楁锛堥€冮€革級銆?
                if let Expr::Field { receiver, field } = &target.node {
                    let recv = self.check_expr_at(receiver.span, &receiver.node)?;
                    let val_preview = self.check_expr_at(value.span, &value.node)?;
                    if val_preview.ty.is_span() {
                        if let Some(tname) = self.type_name_of(&recv.ty) {
                            if self.registry.is_class(&tname) {
                                return Err(TypeError::Oop(format!(
                                    "E_SPAN_ESCAPE: cannot store `{}` in class field `{tname}.{field}`",
                                    val_preview.ty.display()
                                )));
                            }
                        }
                    }
                }
                if let Expr::Field { receiver, field } = &target.node {
                    let recv = self.check_expr_at(receiver.span, &receiver.node)?;
                    if let Some(tname) = self.type_name_of(&recv.ty) {
                        if self
                            .registry
                            .resolve_field(&tname, field, &self.access_ctx())
                            .is_ok()
                        {
                            if let Some(finfo) = self.registry.field_info(&tname, field) {
                                if finfo.is_const {
                                    return Err(TypeError::Oop(format!(
                                        "const field `{field}` on `{tname}` cannot be assigned"
                                    )));
                                }
                                if finfo.is_readonly && !self.in_ctor {
                                    return Err(TypeError::Oop(format!(
                                        "readonly field `{field}` on `{tname}` can only be assigned in a constructor"
                                    )));
                                }
                                // RFC 006 M1锛歩nit-only 鑷姩灞炴€т粎 ctor / 瀵硅薄鍒濆鍖栧櫒鍙啓銆?
                                // 瀵硅薄鍒濆鍖栧櫒璧?`Expr::New` 瀛楁鏍￠獙锛屼笉缁忔湰 Assign 璺緞銆?
                                if finfo.is_init_only && !self.in_ctor {
                                    return Err(TypeError::Oop(format!(
                                        "init-only property `{field}` on `{tname}` can only be assigned in a constructor or object initializer"
                                    )));
                                }
                                // RFC 006 A1：auto-property 写访问（setter/init）看
                                // `set_vis`（比属性自身可见性更严格时可拦截外部写入）。
                                if let Some(sv) = finfo.set_vis {
                                    if !self.registry.can_access(sv, &tname, &self.access_ctx()) {
                                        return Err(TypeError::Oop(format!(
                                            "setter of property `{field}` on `{tname}` is not accessible from this context"
                                        )));
                                    }
                                }
                                // RFC 004 搂D9锛氬叕寮€瀛楁璧嬪€间篃闇€闅愬紡 variant 鏋勯€犮€?
                                // 鏃ц矾寰勪粎鏍￠獙 const/readonly 鍚庤惤鍏ュ厹搴曪紝鏈 Field
                                // 鐩爣鍋氱被鍨嬫鏌ワ紝瀵艰嚧 `box.Value = "x"`锛圴alue:
                                // ContentLike锛夐潤榛樺啓鍏?string ptr銆?
                                let mut field_ty = finfo.ty.clone();
                                // RFC 044 M2（合成类字段类型后置解析）：`__infer__`
                                // 哨兵字段（yield 状态机提升的 var 局部 / foreach 迭代
                                // 变量 / 解构目标）首次赋值时从值类型推断并回填 registry
                                // 字段表——HIR 脱糖在 typeck 之前拿不到类型，此处后置
                                // 解析；后续读取/布局均按回填后的类型。
                                if field_ty == "__infer__" {
                                    let val_ty = self.check_expr_at(value.span, &value.node)?.ty;
                                    let inferred = crate::generics::type_id_to_field_name(&val_ty);
                                    if let Some(nom) = self.registry.types.get_mut(&tname) {
                                        if let Some(fi) = nom.fields.get_mut(field) {
                                            fi.ty = inferred.clone();
                                        }
                                    }
                                    field_ty = inferred;
                                }
                                // 委托别名（`public delegate int Converter(int);`）字段的
                                // 类型名是别名而非 `Func_*`：须展开为 `Func`，否则与右侧
                                // lambda/函数实参比较时 `Named("Converter")` 恒不等。
                                let delegate_ty = self
                                    .registry
                                    .delegate_aliases
                                    .get(field_ty.as_str())
                                    .cloned();
                                let expected = delegate_ty
                                    .clone()
                                    .unwrap_or_else(|| TypeId::Named(field_ty.clone()));
                                // C6：委托字段赋 lambda 须做形参类型推断与 body 校验。
                                // 旧路径对 Field 目标仅经 prepare_target_expr（不推导
                                // lambda 形参类型），导致 `f.Callback = s => ...` 中 `s`
                                // 无类型 → body 报 NoSuch。与 Let `Func` 声明（check_func_lambda）
                                // 及 M5 事件签名路径对齐。委托别名同样走此推断。
                                let is_delegate_field = delegate_ty.is_some()
                                    || matches!(field_ty.as_str(), "Func" | "Action")
                                    || field_ty.starts_with("Func_")
                                    || field_ty.starts_with("Action_");
                                if is_delegate_field && matches!(value.node, Expr::Lambda(_)) {
                                    if let Expr::Lambda(l) = &value.node {
                                        let func_ty = delegate_ty.clone().or_else(|| {
                                            demangle_func_type_with(
                                                &field_ty,
                                                l.params.len(),
                                                &|s| self.registry.types.contains_key(s),
                                            )
                                        });
                                        if let Some(TypeId::Func { params, ret }) = func_ty {
                                            self.check_func_lambda(l, &params, &ret)?;
                                            return Ok(TypedStmt::Assign {
                                                target: target.clone(),
                                                value: value.clone(),
                                            });
                                        }
                                    }
                                }
                                let prepared =
                                    self.prepare_target_expr(&value.node, &expected, value.span)?;
                                let checked_val = self.check_expr(&prepared)?;
                                let final_val_expr =
                                    if !self.types_compatible(&expected, &checked_val.ty) {
                                        match self.coerce_to_variant(
                                            checked_val.expr.clone(),
                                            &checked_val.ty,
                                            &expected,
                                        ) {
                                            Some(coerced) => coerced,
                                            None => {
                                                return Err(TypeError::Mismatch {
                                                    expected: expected.display(),
                                                    found: checked_val.ty.display(),
                                                });
                                            }
                                        }
                                    } else {
                                        checked_val.expr
                                    };
                                return Ok(TypedStmt::Assign {
                                    target: target.clone(),
                                    value: Spanned::new(final_val_expr, value.span),
                                });
                            }
                        } else {
                            let setter: Ident = format!("set_{field}").into();
                            match self
                                .registry
                                .resolve_method(&tname, &setter, &self.access_ctx())
                            {
                                Ok(sig) => {
                                    // RFC 006 M2锛氳嚜瀹氫箟 init 璁块棶鍣ㄤ粎 ctor / 瀵硅薄鍒濆鍖栧櫒鍙啓銆?
                                    if self
                                        .registry
                                        .init_only_props
                                        .contains(&(tname.clone(), field.clone()))
                                        && !self.in_ctor
                                    {
                                        return Err(TypeError::Oop(format!(
                                            "init-only property `{field}` on `{tname}` can only be assigned in a constructor or object initializer"
                                        )));
                                    }
                                    // RFC 065锛氬睘鎬ц祴鍊煎彸渚х洰鏍囩被鍨?`new()`銆?
                                    // RFC 017锛歚prop = [鈥;` 鈫?`List<T>` 鐩爣鑴辩硸銆?
                                    let prepared = if let Some(param) = sig.params.first() {
                                        let expected = TypeId::Named(param.ty.clone());
                                        self.prepare_target_expr(
                                            &value.node,
                                            &expected,
                                            value.span,
                                        )?
                                    } else {
                                        value.node.clone()
                                    };
                                    let checked_val = self.check_expr(&prepared)?;
                                    let val_ty = checked_val.ty;
                                    // RFC 004 搂D9 / RFC 037 M2锛歱roperty setter
                                    // 褰㈠弬绫诲瀷涓嶅尮閰嶆椂灏濊瘯闅愬紡 variant 鏋勯€犮€?
                                    // 鍏稿瀷鍦烘櫙锛歚button.Content = "Click"` 鈫?
                                    // setter 褰㈠弬涓?`ContentVariant`锛屽瓧绗︿覆 "Click"
                                    // 琚嚜鍔ㄥ寘瑁呬负 `ContentVariant.Text("Click")`銆?
                                    let final_val_expr = if let Some(param) = sig.params.first() {
                                        let param_ty = &param.ty;
                                        let expected = TypeId::Named(param_ty.clone());
                                        if !self.types_compatible(&expected, &val_ty) {
                                            match self.coerce_to_variant(
                                                checked_val.expr.clone(),
                                                &val_ty,
                                                &expected,
                                            ) {
                                                Some(coerced) => coerced,
                                                None => {
                                                    return Err(TypeError::Mismatch {
                                                        expected: param_ty.to_string(),
                                                        found: val_ty.display(),
                                                    });
                                                }
                                            }
                                        } else {
                                            // RFC 006 M3 + RFC 004 P0 Phase 1：object 形参
                                            // 接收 string/基元实参 → 装箱。
                                            box_to_object(
                                                &self.registry,
                                                checked_val.expr,
                                                &val_ty,
                                                param_ty.as_str(),
                                                value.span,
                                            )
                                        }
                                    } else {
                                        checked_val.expr
                                    };
                                    return Ok(TypedStmt::Assign {
                                        target: target.clone(),
                                        value: Spanned::new(final_val_expr, value.span),
                                    });
                                }
                                Err(_) => {
                                    let getter: Ident = format!("get_{field}").into();
                                    if self
                                        .registry
                                        .resolve_method(&tname, &getter, &self.access_ctx())
                                        .is_ok()
                                    {
                                        return Err(TypeError::Oop(format!(
                                            "property `{field}` on `{tname}` is read-only"
                                        )));
                                    }
                                }
                            }
                        }
                    }
                }
                // RFC 009 P1-F #8锛歚in` 鍙傛暟 readonly 寮哄埗鈥斺€旇嫢璧嬪€肩洰鏍囨槸
                // 鏍囪瘑绗︿笖鍏剁被鍨嬩负 `TypeId::Ref { mutable: false }`锛堝嵆 `in` 鍙傛暟锛夛紝
                // 鎷掔粷鍐欏叆銆傚悓鏍烽€傜敤浜?`ref readonly` 灞€閮紙鏈潵鎵╁睍锛夈€?
                if let Expr::Ident(name) = &target.node {
                    if let Some(TypeId::Ref { mutable: false, .. }) = self.resolve_value_name(name)
                    {
                        return Err(TypeError::Oop(format!(
                            "cannot assign to `in` parameter `{name}` (readonly ref)"
                        )));
                    }
                }
                self.check_expr_at(target.span, &target.node)?;
                // RFC 065锛氬眬閮ㄨ祴鍊煎彸渚х洰鏍囩被鍨?`new()`銆?
                // RFC 004 M1锛歚f = Double` 鏂规硶缁?鈫?lambda銆?
                let assign_target_ty = if let Expr::Ident(name) = &target.node {
                    self.resolve_value_name(name)
                        .map(|target_ty| match target_ty {
                            TypeId::Ref { inner, .. } => *inner,
                            other => other,
                        })
                } else {
                    None
                };
                let prepared = if let Some(ref target_ty) = assign_target_ty {
                    let after_mg = self.maybe_coerce_method_group(&value.node, target_ty)?;
                    self.prepare_target_expr(&after_mg, target_ty, value.span)?
                } else {
                    value.node.clone()
                };
                let checked_val = if let Some(ref target_ty) = assign_target_ty {
                    if self.try_bind_collection_array_target(&prepared, target_ty)? {
                        crate::typed::TypedExpr {
                            ty: target_ty.clone(),
                            expr: prepared.clone(),
                            linq_path: None,
                            expression_tree: None,
                        }
                    } else {
                        self.check_expr(&prepared)?
                    }
                } else {
                    self.check_expr(&prepared)?
                };
                // RFC 004 搂D9 / RFC 037 M2锛氬厹搴曡祴鍊艰矾寰勮ˉ types_compatible 妫€鏌?+
                // 闅愬紡 variant 鏋勯€犮€傛鍓嶆璺緞鏃犵被鍨嬫牎楠岋紙浠?Field-with-setter
                // 璺緞鏈夛級锛屽鑷?`ContentVariant c; c = "Click";` 杩欑被鐩存帴鍙橀噺
                // 璧嬪€兼棤娉曡Е鍙?variant 闅愬紡鏋勯€犮€傛澶勪粎瀵?Ident 鐩爣琛ユ鏌モ€斺€?
                // Index / 澶嶆潅鐩爣鐨勫厓绱犵被鍨嬫帹瀵肩暀寰呭悗缁€?
                let final_val_expr = if let Expr::Ident(name) = &target.node {
                    if let Some(target_ty) = self.resolve_value_name(name) {
                        let target_ty = match target_ty {
                            TypeId::Ref { inner, .. } => *inner,
                            other => other,
                        };
                        if !self.types_compatible(&target_ty, &checked_val.ty) {
                            match self.coerce_to_variant(
                                checked_val.expr.clone(),
                                &checked_val.ty,
                                &target_ty,
                            ) {
                                Some(coerced) => coerced,
                                None => checked_val.expr,
                            }
                        } else {
                            // RFC 006 M3 + RFC 004 P0 Phase 1：object 局部变量
                            // 赋值接 string/基元 → 装箱（统一入口）。
                            let param_ty = self.type_name_of(&target_ty).unwrap_or_default();
                            box_to_object(
                                &self.registry,
                                checked_val.expr,
                                &checked_val.ty,
                                param_ty.as_str(),
                                value.span,
                            )
                        }
                    } else {
                        checked_val.expr
                    }
                } else {
                    checked_val.expr
                };
                if let Some(flow) = &mut self.out_flow {
                    if let Expr::Ident(name) = &target.node {
                        flow.mark_assigned(name);
                    }
                }
                if let Some(flow) = &mut self.null_flow {
                    if let Expr::Ident(name) = &target.node {
                        flow.un_narrow(name);
                    }
                }
                Ok(TypedStmt::Assign {
                    target: target.clone(),
                    value: Spanned::new(final_val_expr, value.span),
                })
            }
            Stmt::Break => {
                if self.loop_depth == 0 {
                    return Err(TypeError::BreakOutsideLoop);
                }
                Ok(TypedStmt::Break)
            }
            Stmt::Continue => {
                if self.loop_depth == 0 {
                    return Err(TypeError::ContinueOutsideLoop);
                }
                Ok(TypedStmt::Continue)
            }
            Stmt::Throw { expr } => {
                let ty = self.check_expr_at(expr.span, &expr.node)?.ty;
                if !self.is_throwable_class(&ty) {
                    return Err(TypeError::Mismatch {
                        expected: "Throwable class (e.g. Exception)".into(),
                        found: ty.display(),
                    });
                }
                Ok(TypedStmt::Throw { expr: expr.clone() })
            }
            Stmt::TryCatch {
                try_body,
                catch_ty,
                catch_name,
                when_cond,
                catch_body,
                finally,
            } => {
                let catch_type = self.lower_type(&catch_ty.node)?;
                if !self.is_throwable_class(&catch_type) {
                    return Err(TypeError::Mismatch {
                        expected: "Throwable class in catch".into(),
                        found: catch_type.display(),
                    });
                }
                let typed_try = self.check_block(try_body, &TypeId::Void)?;
                self.scopes.push(IndexMap::new());
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(catch_name.clone(), catch_type.clone());
                let typed_when = if let Some(w) = when_cond {
                    let wty = self.check_expr_at(w.span, &w.node)?;
                    if !matches!(self.canonical_type(&wty.ty), TypeId::Bool) {
                        return Err(TypeError::Mismatch {
                            expected: "bool".into(),
                            found: wty.ty.display(),
                        });
                    }
                    Some(Spanned::new(wty.expr, w.span))
                } else {
                    None
                };
                let typed_catch = self.check_block(catch_body, &TypeId::Void)?;
                self.scopes.pop();
                let typed_finally = finally
                    .as_ref()
                    .map(|f| self.check_block(f, &TypeId::Void))
                    .transpose()?;
                Ok(TypedStmt::TryCatch {
                    try_body: typed_try,
                    catch_ty: catch_type,
                    catch_name: catch_name.clone(),
                    when_cond: typed_when,
                    catch_body: typed_catch,
                    finally: typed_finally,
                })
            }
            Stmt::TryFinally { body, finally } => {
                let typed_body = self.check_block(body, &TypeId::Void)?;
                let typed_finally = self.check_block(finally, &TypeId::Void)?;
                Ok(TypedStmt::TryFinally {
                    body: typed_body,
                    finally: typed_finally,
                })
            }
            Stmt::Using {
                name,
                ty,
                init,
                body,
            } => {
                let declared = ty
                    .as_ref()
                    .map(|t| self.lower_type(&t.node))
                    .transpose()?
                    .unwrap_or(TypeId::Infer);
                let checked = self.check_expr_at(init.span, &init.node)?;
                let final_ty = if matches!(declared, TypeId::Infer) {
                    self.canonical_type(&checked.ty)
                } else {
                    if !self.types_compatible(&declared, &checked.ty) {
                        return Err(TypeError::Mismatch {
                            expected: declared.display(),
                            found: checked.ty.display(),
                        });
                    }
                    self.canonical_type(&declared)
                };
                // `using` is not allowed in async methods 鈥?use `await using` instead.
                if self.in_async {
                    return Err(TypeError::Oop(
                        "`using` is not allowed in async methods; use `await using`".into(),
                    ));
                }
                self.require_idisposable(&final_ty)?;
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), final_ty.clone());
                let typed_body = self.check_block(body, &TypeId::Void)?;
                Ok(TypedStmt::Using {
                    name: name.clone(),
                    ty: final_ty,
                    init: init.clone(),
                    body: typed_body,
                })
            }
            Stmt::UsingVar { name, ty, init } => {
                let declared = ty
                    .as_ref()
                    .map(|t| self.lower_type(&t.node))
                    .transpose()?
                    .unwrap_or(TypeId::Infer);
                let checked = self.check_expr_at(init.span, &init.node)?;
                let final_ty = if matches!(declared, TypeId::Infer) {
                    self.canonical_type(&checked.ty)
                } else {
                    if !self.types_compatible(&declared, &checked.ty) {
                        return Err(TypeError::Mismatch {
                            expected: declared.display(),
                            found: checked.ty.display(),
                        });
                    }
                    self.canonical_type(&declared)
                };
                // `using` is not allowed in async methods 鈥?use `await using` instead.
                if self.in_async {
                    return Err(TypeError::Oop(
                        "`using` is not allowed in async methods; use `await using`".into(),
                    ));
                }
                self.require_idisposable(&final_ty)?;
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), final_ty.clone());
                Ok(TypedStmt::UsingVar {
                    name: name.clone(),
                    ty: final_ty,
                    init: Spanned::new(checked.expr, init.span),
                })
            }
            Stmt::AwaitUsing {
                name,
                ty,
                init,
                body,
            } => {
                self.require_async_context("await using")?;
                let declared = ty
                    .as_ref()
                    .map(|t| self.lower_type(&t.node))
                    .transpose()?
                    .unwrap_or(TypeId::Infer);
                let checked = self.check_expr_at(init.span, &init.node)?;
                let final_ty = if matches!(declared, TypeId::Infer) {
                    self.canonical_type(&checked.ty)
                } else {
                    if !self.types_compatible(&declared, &checked.ty) {
                        return Err(TypeError::Mismatch {
                            expected: declared.display(),
                            found: checked.ty.display(),
                        });
                    }
                    self.canonical_type(&declared)
                };
                self.require_iasyncdisposable(&final_ty)?;
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), final_ty.clone());
                let typed_body = self.check_block(body, &TypeId::Void)?;
                Ok(TypedStmt::AwaitUsing {
                    name: name.clone(),
                    ty: final_ty,
                    init: init.clone(),
                    body: typed_body,
                })
            }
            Stmt::AwaitUsingVar { name, ty, init } => {
                self.require_async_context("await using var")?;
                let declared = ty
                    .as_ref()
                    .map(|t| self.lower_type(&t.node))
                    .transpose()?
                    .unwrap_or(TypeId::Infer);
                let checked = self.check_expr_at(init.span, &init.node)?;
                let final_ty = if matches!(declared, TypeId::Infer) {
                    self.canonical_type(&checked.ty)
                } else {
                    if !self.types_compatible(&declared, &checked.ty) {
                        return Err(TypeError::Mismatch {
                            expected: declared.display(),
                            found: checked.ty.display(),
                        });
                    }
                    self.canonical_type(&declared)
                };
                self.require_iasyncdisposable(&final_ty)?;
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), final_ty.clone());
                Ok(TypedStmt::AwaitUsingVar {
                    name: name.clone(),
                    ty: final_ty,
                    init: Spanned::new(checked.expr, init.span),
                })
            }
            Stmt::DeconstructAssign { .. } => Err(TypeError::Oop(
                "internal: DeconstructAssign must be expanded in check_block".into(),
            )),
            Stmt::Lock { .. } => Err(TypeError::Oop(
                "internal: Lock must be expanded in check_block".into(),
            )),
            // RFC 044：yield 在 hir 脱糖为状态机，typeck 不应见到本节点。
            Stmt::YieldReturn { .. } | Stmt::YieldBreak => Err(TypeError::Oop(
                "internal: yield must be desugared in hir before typeck".into(),
            )),
        }
    }

    /// RFC 037 M-D0 强化（G1/G2 生命周期编译期拒绝）：订阅返回的退订 token 不得丢弃。
    ///
    /// 用户面订阅入口（§5.3「观察者入口契约」）：
    /// - `ObserveProperty("…")` 返回的通道句柄（编译器合成隐藏通道的 `Signal`，
    ///   结构判定：接收者表达式本身是 `ObserveProperty` 方法调用）；
    /// - `ObservableCollection<T>` 实例（接收者类型名为 `ObservableCollection_*`）。
    ///
    /// 订阅类方法（`Subscribe` / `OnChanged` / `OnChanging`）作为**表达式语句**
    /// （裸调用、结果未使用）出现 = 编译期错误——token 被丢弃即无法配对退订，
    /// 破坏 G2 确定性退订契约。提示须绑定变量以便配对退订。
    ///
    /// 放行形态：绑定变量（`int t = ...`）、作实参传他函数、`return`、条件判断
    /// 等非裸语句一律不受影响——本函数只挂在 `Stmt::Expr` 表达式语句检查处。
    ///
    /// 边界（不误伤 std 内部）：裸 `Signal_*` 订阅**不**在此列——`std/UI/Core/Components/
    /// Button.OnClick` 刻意弃 token（控件生命周期内常驻订阅，随元素销毁确定退订），
    /// `Signal.OnChanging` 亦被 `examples/UnitTest/Arc/SignalTests.as` 用作校验钩子；
    /// 规则仅约束用户面出口（ObserveProperty 通道句柄 + ObservableCollection 实例）。
    fn reject_discarded_subscribe_token(&mut self, stmt_expr: &Expr) -> Result<(), TypeError> {
        let Expr::MethodCall {
            receiver, method, ..
        } = stmt_expr
        else {
            return Ok(());
        };
        let m = method.as_str();
        if m != "Subscribe" && m != "OnChanged" && m != "OnChanging" {
            return Ok(());
        }
        // 通道句柄判定：接收者是 `ObserveProperty("…")` 调用（编译器合成观察通道）。
        // 外层 `check_expr` 已通过，说明该调用经 `check_observable_observe_call`
        // 判定为合成形态（用户自定义同名方法会被该钩子拦截），结构判定即精确。
        if matches!(
            &receiver.node,
            Expr::MethodCall {
                method: inner,
                ..
            } if inner.as_str() == "ObserveProperty"
        ) {
            return Err(TypeError::Oop(format!(
                "订阅 `{m}` 返回的退订 token 不得丢弃：须绑定变量（`int t = ...`）以便配对退订（G2）"
            )));
        }
        // 集合判定：接收者类型名为 `ObservableCollection_*`（泛型单态命名）。
        // 接收者已随外层调用整体类型检查通过，此处复检仅取类型、必然成功；
        // 防御性失败时按非集合放行，避免误伤。
        let is_collection = self
            .check_expr(&receiver.node)
            .map(|te| {
                self.type_name_of(&te.ty)
                    .map(|n| n.as_str().starts_with("ObservableCollection_"))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if is_collection {
            return Err(TypeError::Oop(format!(
                "订阅 `{m}` 返回的退订 token 不得丢弃：须绑定变量（`int t = ...`）以便配对退订（G2）"
            )));
        }
        Ok(())
    }

    /// RFC 009 搂7.3锛歚lock (expr) { body }` 鈫?
    /// `Lock __lock_N = expr; Monitor.Enter(__lock_N); try { body } finally { Monitor.Exit(__lock_N); }`
    pub(crate) fn check_lock_stmt(
        &mut self,
        expr: &Spanned<Expr>,
        body: &Block,
        span: Span,
    ) -> Result<Vec<TypedStmt>, TypeError> {
        let checked = self.check_expr_at(expr.span, &expr.node)?;
        if !matches!(&checked.ty, TypeId::Named(n) if n.as_str() == "Lock") {
            return Err(TypeError::Mismatch {
                expected: "Lock".into(),
                found: checked.ty.display(),
            });
        }
        let lock_ty = self.canonical_type(&checked.ty);
        let tmp_name: Ident = format!("__lock_{}", span.start).into();
        self.scopes
            .last_mut()
            .unwrap()
            .insert(tmp_name.clone(), lock_ty.clone());

        let tmp_arg = Spanned::new(Expr::Ident(tmp_name.clone()), span);
        let enter_ast = Expr::MethodCall {
            receiver: Box::new(Spanned::new(Expr::Ident(Ident::from("Monitor")), span)),
            method: Ident::from("Enter"),
            args: vec![tmp_arg.clone()],
            type_args: vec![],
            params_span: None,
        };
        let exit_ast = Expr::MethodCall {
            receiver: Box::new(Spanned::new(Expr::Ident(Ident::from("Monitor")), span)),
            method: Ident::from("Exit"),
            args: vec![tmp_arg],
            type_args: vec![],
            params_span: None,
        };
        let enter_checked = self.check_expr(&enter_ast)?;
        let exit_checked = self.check_expr(&exit_ast)?;
        let typed_body = self.check_block(body, &TypeId::Void)?;

        Ok(vec![
            TypedStmt::Let {
                name: tmp_name,
                ty: lock_ty,
                init: Some(Spanned::new(checked.expr, expr.span)),
            },
            TypedStmt::Expr(Spanned::new(enter_checked.expr, span)),
            TypedStmt::TryFinally {
                body: typed_body,
                finally: TypedBlock {
                    stmts: vec![TypedStmt::Expr(Spanned::new(exit_checked.expr, span))],
                    tail: None,
                },
            },
        ])
    }

    /// RFC 004 M2/M7: `var (x, y) = e` / `(x, _) = e` / `(a, (b, c)) = e`
    /// -> Let* + recursive `e.Deconstruct(out ...)`.
    pub(crate) fn check_deconstruct_assign(
        &mut self,
        declare: bool,
        targets: &[DeconstructTarget],
        value: &Spanned<Expr>,
        span: Span,
    ) -> Result<Vec<TypedStmt>, TypeError> {
        self.expand_deconstruct_level(declare, targets, value, span, &mut 0)
    }

    fn expand_deconstruct_level(
        &mut self,
        declare: bool,
        targets: &[DeconstructTarget],
        value: &Spanned<Expr>,
        span: Span,
        temp_counter: &mut u32,
    ) -> Result<Vec<TypedStmt>, TypeError> {
        if targets.len() < 2 {
            return Err(TypeError::Oop(
                "deconstruct assignment requires at least two targets".into(),
            ));
        }
        let checked_value = self.check_expr_at(value.span, &value.node)?;
        let tname = self.type_name_of(&checked_value.ty).ok_or_else(|| {
            TypeError::Oop(format!(
                "cannot deconstruct value of type `{}`",
                checked_value.ty.display()
            ))
        })?;
        let method: Ident = "Deconstruct".into();
        let candidates = self
            .registry
            .collect_method_overloads(&tname, &method, &self.access_ctx())
            .map_err(|e| TypeError::Oop(e.to_string()))?;
        let matching: Vec<_> = candidates
            .iter()
            .filter(|(_, sig)| {
                sig.params.len() == targets.len()
                    && sig.params.iter().all(|p| p.is_out)
                    && !sig.params.iter().any(|p| p.is_ref || p.is_in)
            })
            .collect();
        let (_declaring, sig) = match matching.len() {
            1 => matching[0],
            0 => {
                return Err(TypeError::Oop(format!(
                    "no matching `Deconstruct` with {} out parameter(s) on `{}`",
                    targets.len(),
                    tname
                )));
            }
            _ => {
                return Err(TypeError::Oop(format!(
                    "ambiguous `Deconstruct` overload on `{}`",
                    tname
                )));
            }
        };

        let mut out_stmts = Vec::new();
        let mut args: Vec<Spanned<Expr>> = Vec::with_capacity(targets.len());
        let mut nested_work: Vec<(Ident, Vec<DeconstructTarget>)> = Vec::new();
        // RFC 044 M2：提升字段目标的 out 回写对（字段名, 临时局部名）。
        let mut field_writes: Vec<(Ident, Ident)> = Vec::new();
        let mut discard_i = 0u32;

        for (target, param) in targets.iter().zip(sig.params.iter()) {
            let expected = self.param_sig_type_id(&param.ty);
            match target {
                DeconstructTarget::Bind(None) => {
                    let name: Ident = format!("__discard_{}_{}", span.start, discard_i).into();
                    discard_i += 1;
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert(name.clone(), expected.clone());
                    out_stmts.push(TypedStmt::Let {
                        name: name.clone(),
                        ty: expected,
                        init: None,
                    });
                    let ident = Spanned::new(Expr::Ident(name), span);
                    args.push(Spanned::new(
                        Expr::RefArg {
                            is_out: true,
                            expr: Box::new(ident),
                        },
                        span,
                    ));
                }
                DeconstructTarget::Bind(Some(name)) => {
                    if declare {
                        let in_method_scope = self
                            .scopes
                            .last()
                            .map(|s| s.contains_key(name))
                            .unwrap_or(false);
                        if in_method_scope {
                            return Err(TypeError::Oop(format!(
                                "variable `{name}` is already defined in this scope"
                            )));
                        }
                        // RFC 044 M2：yield 状态机合成类的提升字段（解构目标
                        // 被 HIR 改写为 `__loc_*`）——不声明局部，类型从 Deconstruct
                        // out 参数类型推断回填（合成类字段类型后置解析）。
                        let is_hoisted_field = self
                            .current_class
                            .as_ref()
                            .map(|c| self.is_instance_field_of(c, name))
                            .unwrap_or(false);
                        if !is_hoisted_field {
                            self.scopes
                                .last_mut()
                                .unwrap()
                                .insert(name.clone(), expected.clone());
                            out_stmts.push(TypedStmt::Let {
                                name: name.clone(),
                                ty: expected,
                                init: None,
                            });
                        } else {
                            self.backfill_infer_field(name, &expected);
                            // RFC 044 M2：提升字段不能作 out 目标（MIR RefArg 仅支持
                            // 局部地址）——out 写入临时局部，调用后回写字段。
                            let tmp: Ident =
                                format!("__decon_field_{}_{}", span.start, *temp_counter).into();
                            *temp_counter += 1;
                            self.scopes
                                .last_mut()
                                .unwrap()
                                .insert(tmp.clone(), expected.clone());
                            out_stmts.push(TypedStmt::Let {
                                name: tmp.clone(),
                                ty: expected,
                                init: None,
                            });
                            field_writes.push((name.clone(), tmp.clone()));
                            let ident = Spanned::new(Expr::Ident(tmp.clone()), span);
                            args.push(Spanned::new(
                                Expr::RefArg {
                                    is_out: true,
                                    expr: Box::new(ident),
                                },
                                span,
                            ));
                            continue;
                        }
                    } else {
                        let existing = self
                            .resolve_value_name(name)
                            .ok_or_else(|| TypeError::Undefined(name.to_string()))?;
                        if !self.types_compatible(&expected, &existing)
                            && !self.types_compatible(&existing, &expected)
                        {
                            return Err(TypeError::Mismatch {
                                expected: expected.display(),
                                found: existing.display(),
                            });
                        }
                    }
                    let ident = Spanned::new(Expr::Ident(name.clone()), span);
                    args.push(Spanned::new(
                        Expr::RefArg {
                            is_out: true,
                            expr: Box::new(ident),
                        },
                        span,
                    ));
                }
                DeconstructTarget::Nested(inner) => {
                    let tmp: Ident =
                        format!("__decon_nest_{}_{}", span.start, *temp_counter).into();
                    *temp_counter += 1;
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert(tmp.clone(), expected.clone());
                    out_stmts.push(TypedStmt::Let {
                        name: tmp.clone(),
                        ty: expected,
                        init: None,
                    });
                    let ident = Spanned::new(Expr::Ident(tmp.clone()), span);
                    args.push(Spanned::new(
                        Expr::RefArg {
                            is_out: true,
                            expr: Box::new(ident),
                        },
                        span,
                    ));
                    nested_work.push((tmp, inner.clone()));
                }
            }
        }

        let call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(checked_value.expr, value.span)),
                method,
                args,
                type_args: Vec::new(),
                params_span: None,
            },
            span,
        );
        out_stmts.push(TypedStmt::Expr(call));

        // RFC 044 M2：临时局部 → 提升字段回写（字段类型已由 Deconstruct out
        // 参数类型推断回填）。
        for (field, tmp) in field_writes {
            out_stmts.push(TypedStmt::Assign {
                target: Spanned::new(Expr::Ident(field), span),
                value: Spanned::new(Expr::Ident(tmp), span),
            });
        }

        for (tmp, inner) in nested_work {
            let nested_val = Spanned::new(Expr::Ident(tmp), span);
            out_stmts.extend(self.expand_deconstruct_level(
                declare,
                &inner,
                &nested_val,
                span,
                temp_counter,
            )?);
        }
        Ok(out_stmts)
    }

    /// RFC 044 M2：合成类提升字段（`__infer__` 哨兵）的类型后置推断回填——
    /// 从 Deconstruct out 参数类型（解构场景）或字段赋值类型（Assign 场景）推断。
    fn backfill_infer_field(&mut self, name: &Ident, ty: &TypeId) {
        let Some(class_name) = &self.current_class else {
            return;
        };
        let Some(nom) = self.registry.types.get_mut(class_name) else {
            return;
        };
        let Some(fi) = nom.fields.get_mut(name) else {
            return;
        };
        if fi.ty == "__infer__" {
            fi.ty = crate::generics::type_id_to_field_name(ty);
        }
    }

    fn require_idisposable(&self, final_ty: &TypeId) -> Result<(), TypeError> {
        if let TypeId::Named(ref class_name) = final_ty {
            let iface: Ident = "IDisposable".into();
            if !self.registry.implements_interface(class_name, &iface) {
                return Err(TypeError::Oop(format!(
                    "`using` resource type `{}` must implement IDisposable",
                    class_name
                )));
            }
            Ok(())
        } else {
            Err(TypeError::Oop(format!(
                "`using` resource must be a class type implementing IDisposable, found {}",
                final_ty.display()
            )))
        }
    }

    fn require_async_context(&self, construct: &str) -> Result<(), TypeError> {
        if !self.in_async {
            return Err(TypeError::Oop(format!(
                "`{construct}` can only be used in async methods",
            )));
        }
        Ok(())
    }

    fn require_iasyncdisposable(&self, final_ty: &TypeId) -> Result<(), TypeError> {
        if let TypeId::Named(ref class_name) = final_ty {
            let iface: Ident = "IAsyncDisposable".into();
            if !self.registry.implements_interface(class_name, &iface) {
                return Err(TypeError::Oop(format!(
                    "`await using` resource type `{}` must implement IAsyncDisposable",
                    class_name
                )));
            }
            Ok(())
        } else {
            Err(TypeError::Oop(format!(
                "`await using` resource must be a class type implementing IAsyncDisposable, found {}",
                final_ty.display()
            )))
        }
    }

    /// RFC 074锛歚recv?.member = value` 璇彞褰㈢┖鏉′欢璧嬪€笺€?
    ///
    /// 璇箟锛歚P?.A = B` 鈮?`if (P is not null) P.A = B;`锛圥 涓€娆★紱B 浠呴潪绌猴級銆?
    /// 浠呭瓧娈?灞炴€х洰鏍囷紱鎷掔粷 `?.Method(...) = 鈥銆?
    fn check_null_cond_assign(
        &mut self,
        target: &Spanned<Expr>,
        access: &Spanned<Expr>,
        value: &Spanned<Expr>,
    ) -> Result<TypedStmt, TypeError> {
        let Expr::Field { receiver, field } = &access.node else {
            return Err(TypeError::Oop(
                "null-conditional assignment requires `?.` field or property target (not a method call)"
                    .into(),
            ));
        };
        let recv = self.check_expr_at(receiver.span, &receiver.node)?;
        if !recv.ty.is_nullable() {
            return Err(TypeError::Oop(format!(
                "`?.` assignment requires nullable receiver, found `{}`",
                recv.ty.display()
            )));
        }
        let Some(tname) = self.type_name_of(&recv.ty) else {
            return Err(TypeError::Oop(format!(
                "cannot assign via `?.` on type `{}`",
                recv.ty.display()
            )));
        };
        if let Some(finfo) = self.registry.field_info(&tname, field) {
            if finfo.is_const {
                return Err(TypeError::Oop(format!(
                    "const field `{field}` on `{tname}` cannot be assigned"
                )));
            }
            if finfo.is_readonly && !self.in_ctor {
                return Err(TypeError::Oop(format!(
                    "readonly field `{field}` on `{tname}` can only be assigned in a constructor"
                )));
            }
            if finfo.is_init_only && !self.in_ctor {
                return Err(TypeError::Oop(format!(
                    "init-only property `{field}` on `{tname}` can only be assigned in a constructor or object initializer"
                )));
            }
            // RFC 006 A1：`?.` 写访问同样看 setter 可见性。
            if let Some(sv) = finfo.set_vis {
                if !self.registry.can_access(sv, &tname, &self.access_ctx()) {
                    return Err(TypeError::Oop(format!(
                        "setter of property `{field}` on `{tname}` is not accessible from this context"
                    )));
                }
            }
            let expected = TypeId::Named(finfo.ty.clone());
            let prepared = self.apply_target_typed_new(&value.node, &expected)?;
            let checked_val = self.check_expr(&prepared)?;
            let final_val_expr = if !self.types_compatible(&expected, &checked_val.ty) {
                match self.coerce_to_variant(checked_val.expr.clone(), &checked_val.ty, &expected) {
                    Some(coerced) => coerced,
                    None => {
                        return Err(TypeError::Mismatch {
                            expected: expected.display(),
                            found: checked_val.ty.display(),
                        });
                    }
                }
            } else {
                checked_val.expr
            };
            return Ok(TypedStmt::Assign {
                target: target.clone(),
                value: Spanned::new(final_val_expr, value.span),
            });
        }
        let setter: Ident = format!("set_{field}").into();
        match self
            .registry
            .resolve_method(&tname, &setter, &self.access_ctx())
        {
            Ok(sig) => {
                if self
                    .registry
                    .init_only_props
                    .contains(&(tname.clone(), field.clone()))
                    && !self.in_ctor
                {
                    return Err(TypeError::Oop(format!(
                        "init-only property `{field}` on `{tname}` can only be assigned in a constructor or object initializer"
                    )));
                }
                let prepared = if let Some(param) = sig.params.first() {
                    let expected = TypeId::Named(param.ty.clone());
                    self.apply_target_typed_new(&value.node, &expected)?
                } else {
                    value.node.clone()
                };
                let checked_val = self.check_expr(&prepared)?;
                let val_ty = checked_val.ty;
                let final_val_expr = if let Some(param) = sig.params.first() {
                    let expected = TypeId::Named(param.ty.clone());
                    if !self.types_compatible(&expected, &val_ty) {
                        match self.coerce_to_variant(checked_val.expr.clone(), &val_ty, &expected) {
                            Some(coerced) => coerced,
                            None => {
                                return Err(TypeError::Mismatch {
                                    expected: param.ty.to_string(),
                                    found: val_ty.display(),
                                });
                            }
                        }
                    } else {
                        checked_val.expr
                    }
                } else {
                    checked_val.expr
                };
                Ok(TypedStmt::Assign {
                    target: target.clone(),
                    value: Spanned::new(final_val_expr, value.span),
                })
            }
            Err(_) => {
                let getter: Ident = format!("get_{field}").into();
                if self
                    .registry
                    .resolve_method(&tname, &getter, &self.access_ctx())
                    .is_ok()
                {
                    Err(TypeError::Oop(format!(
                        "property `{field}` on `{tname}` is read-only"
                    )))
                } else {
                    Err(TypeError::Oop(format!(
                        "type `{tname}` has no field or settable property `{field}`"
                    )))
                }
            }
        }
    }

    fn is_throwable_class(&self, ty: &TypeId) -> bool {
        matches!(ty, TypeId::Named(_))
    }
}
