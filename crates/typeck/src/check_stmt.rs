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
        // RFC 009 M6: async lambda 的返回类型是 `Task<T>`，body 期望返回 `T`。
        // 同时设置 `in_async = true` 使 body 内的 `await` 合法。
        let body_expected: TypeId = if l.is_async {
            ret.task_inner().cloned().unwrap_or(TypeId::Void)
        } else {
            ret.clone()
        };
        let prev_async = self.in_async;
        self.in_async = l.is_async;
        // RFC 009 M6: block-body lambda 的 `return` 语句需要正确的 return_slot。
        // 推入 body_expected 使 `return expr` 检查与 lambda 返回类型匹配。
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
                // RFC 004 M2：展开为 Let（声明/弃元）+ Deconstruct MethodCall
                Stmt::DeconstructAssign {
                    declare,
                    targets,
                    value,
                } => {
                    typed_stmts.extend(
                        self.check_deconstruct_assign(*declare, targets, value, stmt.span)?,
                    );
                }
                // RFC 005 §7.3：`lock (expr) { }` → Enter + try/finally Exit
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
                // RFC 016 v2 M2 / RFC 016 M3：保存 check_expr 重写后的表达式
                // （如 FFI 装箱插入的 Expr::Box），传递到 TypedStmt::Let。
                // 仅在走 check_expr 的路径（else 分支）有值；其他路径（Lambda/
                // 空集合）保留原 init。
                let mut rewritten_init: Option<Spanned<Expr>> = None;
                let final_ty = if let Some(init) = init {
                    if matches!(declared, TypeId::Expression { .. }) {
                        // RFC 008 M3：方法组 → Expression 硬拒绝（须显式 lambda）。
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
                        // RFC 004 M1：非 lambda 须完整检查；方法组脱糖为 lambda。
                        // 禁止旧行为：跳过 init 导致 NoSuch/签名错静默通过。
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
                        // type (e.g., `int[] empty = []` → int[]). Only `var x = []` falls
                        // back to object[].
                        self.canonical_type(&declared)
                    } else {
                        // RFC 065：显式类型局部上的目标类型 `new()`。
                        // RFC 017：`List<T> x = […];` 集合目标脱糖。
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
                            // RFC 004 §D9 / RFC 037 M2：types_compatible 失败时尝试
                            // 隐式 variant 构造（如 `ContentVariant c = "Click"` →
                            // `ContentVariant.Text("Click")`）。歧义 / 无匹配则回退到
                            // 原始类型不匹配错误。
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
                    // RFC 067：`Deconstruct(out …)` 脱糖用的 `__pos_*` / `__discard_*`
                    // 在同块随后由 out 实参赋值；与 `check_deconstruct_assign` 直插
                    // TypedStmt::Let{init:None} 对齐，不在此硬拒。
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
                // RFC 016 v2 M2 / RFC 016 M3：使用 TypedExpr.expr 而非原始 expr，
                // 保证 typeck 重写后的 AST 节点（如 FFI 装箱插入的 Expr::Box、
                // Cast→Unbox 转换）能传递到 MIR lower。
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
                        // RFC 004 M1：`return Foo;` 方法组 → lambda。
                        let after_mg = self.maybe_coerce_method_group(&v.node, &expected)?;
                        // RFC 065：`return new(...)` 按返回类型填目标类型。
                        // RFC 017：`return […];` → `List<T>` 目标脱糖。
                        let prepared = self.prepare_target_expr(&after_mg, &expected, v.span)?;
                        let checked = self.check_expr(&prepared)?;
                        let ty = checked.ty;
                        if matches!(self.canonical_type(&expected), TypeId::Void) {
                            return Err(TypeError::VoidReturnWithValue(ty.display()));
                        }
                        // RFC 004 §D9 / RFC 037 M2：types_compatible 失败时尝试
                        // 隐式 variant 构造（如 `return "Click";` 在返回类型为
                        // `ContentVariant` 的函数中 → `return ContentVariant.Text("Click");`）。
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
                        // RFC 016 v2 M2：使用 TypedExpr.expr 传递重写后的 AST
                        // （如 Cast→Unbox 转换、FFI 装箱节点、variant 隐式构造）。
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
                // init clause — type-check inline; introduces scope
                self.scopes.push(IndexMap::new());
                if let Some(ref init_stmt) = init {
                    self.check_stmt(&init_stmt.node)?;
                }
                // cond clause — must be bool if present
                let typed_cond = if let Some(ref c) = cond {
                    let checked = self.check_expr_at(c.span, &c.node)?;
                    Some(Spanned::new(checked.expr, c.span))
                } else {
                    None
                };
                // body
                self.loop_depth += 1;
                let typed_body = self.check_block(body, &TypeId::Void)?;
                // inc clause — type-check inline (inside loop scope)
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
                // Rewrite bare instance field target: `_field = v` → `this._field = v`.
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
                // RFC 074：`recv?.member = expr` — 语句形空条件赋值。
                if let Expr::NullCond { access } = &target.node {
                    return self.check_null_cond_assign(&target, access, value);
                }
                // `string` 不可变：拒绝 `s[i] = c`（C# 同为只读 Chars 索引器）。
                if let Expr::Index { receiver, .. } = &target.node {
                    let recv = self.check_expr_at(receiver.span, &receiver.node)?;
                    if recv.ty == TypeId::String {
                        return Err(TypeError::Oop("string indexer is read-only".into()));
                    }
                    // RFC 005 V2：`ReadOnlySpan` 索引只读。
                    if matches!(recv.ty, TypeId::Span { mutable: false, .. }) {
                        return Err(TypeError::Oop("ReadOnlySpan indexer is read-only".into()));
                    }
                }
                // RFC 005 B3 / V5：禁止将 Span 写入 class 字段（逃逸）。
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
                                // RFC 006 M1：init-only 自动属性仅 ctor / 对象初始化器可写。
                                // 对象初始化器走 `Expr::New` 字段校验，不经本 Assign 路径。
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
                                // RFC 004 §D9：公开字段赋值也需隐式 variant 构造。
                                // 旧路径仅校验 const/readonly 后落入兜底，未对 Field
                                // 目标做类型检查，导致 `box.Value = "x"`（Value:
                                // ContentLike）静默写入 string ptr。
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
                                    // RFC 006 M2：自定义 init 访问器仅 ctor / 对象初始化器可写。
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
                                    // RFC 065：属性赋值右侧目标类型 `new()`。
                                    // RFC 017：`prop = […];` → `List<T>` 目标脱糖。
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
                                    // RFC 004 §D9 / RFC 037 M2：property setter
                                    // 形参类型不匹配时尝试隐式 variant 构造。
                                    // 典型场景：`button.Content = "Click"` →
                                    // setter 形参为 `ContentVariant`，字符串 "Click"
                                    // 被自动包装为 `ContentVariant.Text("Click")`。
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
                // RFC 009 P1-F #8：`in` 参数 readonly 强制——若赋值目标是
                // 标识符且其类型为 `TypeId::Ref { mutable: false }`（即 `in` 参数），
                // 拒绝写入。同样适用于 `ref readonly` 局部（未来扩展）。
                if let Expr::Ident(name) = &target.node {
                    if let Some(TypeId::Ref { mutable: false, .. }) = self.resolve_value_name(name)
                    {
                        return Err(TypeError::Oop(format!(
                            "cannot assign to `in` parameter `{name}` (readonly ref)"
                        )));
                    }
                }
                self.check_expr_at(target.span, &target.node)?;
                // RFC 065：局部赋值右侧目标类型 `new()`。
                // RFC 004 M1：`f = Double` 方法组 → lambda。
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
                // RFC 004 §D9 / RFC 037 M2：兜底赋值路径补 types_compatible 检查 +
                // 隐式 variant 构造。此前此路径无类型校验（仅 Field-with-setter
                // 路径有），导致 `ContentVariant c; c = "Click";` 这类直接变量
                // 赋值无法触发 variant 隐式构造。此处仅对 Ident 目标补检查——
                // Index / 复杂目标的元素类型推导留待后续。
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
                // `using` is not allowed in async methods — use `await using` instead.
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
                // `using` is not allowed in async methods — use `await using` instead.
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

    /// RFC 009 §7.3：`lock (expr) { body }` →
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

    /// RFC 074：`recv?.member = value` 语句形空条件赋值。
    ///
    /// 语义：`P?.A = B` ≡ `if (P is not null) P.A = B;`（P 一次；B 仅非空）。
    /// 仅字段/属性目标；拒绝 `?.Method(...) = …`。
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
