//! RFC 008：方法组 → 委托（自由函数 / 静态 / 实例 / M3 命名空间静态与扩展）。

use crate::checker::check_type::is_func_mangled_name;
use crate::checker::TypeChecker;
use crate::error::TypeError;
use crate::generics::type_id_to_ast;
use crate::oop_types::OopMethodSig;
use crate::type_id::TypeId;
use ast::*;

impl TypeChecker {
    /// 局部/参数遮蔽时不是方法组；全局 `fn_defs` 中的自由函数才是。
    pub(crate) fn is_free_fn_method_group(&self, name: &Ident) -> bool {
        for scope in self.scopes.iter().skip(1).rev() {
            if scope.contains_key(name) {
                return false;
            }
        }
        self.fn_defs.contains_key(name)
    }

    /// 接收者为裸类型名（仅全局类型哨兵、无局部遮蔽）——静态方法组 `C.Foo`。
    fn receiver_is_bare_type_name(&self, receiver: &Expr) -> Option<Ident> {
        let Expr::Ident(name) = receiver else {
            return None;
        };
        if !self.registry.types.contains_key(name) {
            return None;
        }
        // scopes[0] 含类型名哨兵；局部/参数遮蔽时按实例接收者处理。
        for scope in self.scopes.iter().skip(1).rev() {
            if scope.contains_key(name) {
                return None;
            }
        }
        Some(name.clone())
    }

    /// 命名空间前缀：无局部遮蔽的 Ident / 嵌套 Field（`N` / `N.M`）。
    fn is_namespace_path_prefix(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ident(name) => {
                for scope in self.scopes.iter().skip(1).rev() {
                    if scope.contains_key(name) {
                        return false;
                    }
                }
                true
            }
            Expr::Field { receiver, field } => {
                for scope in self.scopes.iter().skip(1).rev() {
                    if scope.contains_key(field) {
                        return false;
                    }
                }
                self.is_namespace_path_prefix(&receiver.node)
            }
            _ => false,
        }
    }

    /// 嵌套 Field 是否呈 `Ns.Type` 静态路径（用于硬拒绝文案，不脱糖）。
    fn looks_like_ns_qualified_type_path(&self, receiver: &Expr) -> bool {
        let Expr::Field {
            receiver: prefix,
            field,
        } = receiver
        else {
            return false;
        };
        self.registry.types.contains_key(field) && self.is_namespace_path_prefix(&prefix.node)
    }

    /// 是否呈方法组表面（用于 Expression 边界硬拒绝，避免静默误绑）。
    pub(crate) fn looks_like_method_group_shape(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ident(name) => {
                self.is_free_fn_method_group(name)
                    || self.current_class.as_ref().is_some_and(|c| {
                        !self.current_fn_is_static
                            && self
                                .registry
                                .collect_method_overloads(c, name, &self.access_ctx())
                                .ok()
                                .map(|cands| {
                                    cands
                                        .iter()
                                        .any(|(_, s)| s.modifier != MethodModifier::Static)
                                })
                                .unwrap_or(false)
                    })
            }
            Expr::Field { .. } => true,
            Expr::Path(path) if path.len() > 1 => true,
            _ => false,
        }
    }

    /// 仍硬拒绝的形态：`Expr::Path` 多段（非嵌套 Field 表面）。
    pub(crate) fn reject_deferred_method_group(&self, expr: &Expr) -> Result<(), TypeError> {
        match expr {
            Expr::Path(path) if path.len() > 1 => Err(TypeError::Oop(
                "RFC 008 M3: qualified Path method groups are not supported; \
                 use nested `Ns.Type.Method` Field form for static groups"
                    .into(),
            )),
            _ => Ok(()),
        }
    }

    fn oop_sig_to_func_type(&self, sig: &OopMethodSig) -> Result<TypeId, TypeError> {
        if sig.params.iter().any(|p| p.is_ref || p.is_out || p.is_in) {
            return Err(TypeError::Oop(
                "RFC 008: method groups with ref/out/in parameters are not supported".into(),
            ));
        }
        if !sig.generics.is_empty() {
            return Err(TypeError::Oop(
                "RFC 008: generic method groups require explicit type args (deferred)".into(),
            ));
        }
        let params: Vec<TypeId> = sig
            .params
            .iter()
            .map(|p| self.param_sig_type_id(&p.ty))
            .collect();
        let ret = self.canonical_type(&self.param_sig_type_id(&sig.ret));
        Ok(TypeId::Func {
            params,
            ret: Box::new(ret),
        })
    }

    fn sig_matches_expected(
        &self,
        sig: &OopMethodSig,
        expected_params: &[TypeId],
        expected_ret: &TypeId,
    ) -> Result<bool, TypeError> {
        let found = self.oop_sig_to_func_type(sig)?;
        let TypeId::Func {
            params: found_params,
            ret: found_ret,
        } = found
        else {
            return Ok(false);
        };
        Ok(expected_params.len() == found_params.len()
            && expected_params
                .iter()
                .zip(found_params.iter())
                .all(|(e, f)| self.types_compatible(e, f))
            && expected_ret == found_ret.as_ref())
    }

    fn pick_matching_method(
        &self,
        ty: &Ident,
        method: &Ident,
        want_static: bool,
        expected: Option<(&[TypeId], &TypeId)>,
    ) -> Result<OopMethodSig, TypeError> {
        let candidates = self
            .registry
            .collect_method_overloads(ty, method, &self.access_ctx())
            .map_err(|e| TypeError::Oop(e.to_string()))?;
        let mut matched = Vec::new();
        for (_, sig) in candidates {
            let is_static = sig.modifier == MethodModifier::Static;
            if is_static != want_static {
                continue;
            }
            match expected {
                Some((ep, er)) => {
                    if self.sig_matches_expected(&sig, ep, er)? {
                        matched.push(sig);
                    }
                }
                None => matched.push(sig),
            }
        }
        match matched.len() {
            0 => Err(TypeError::Mismatch {
                expected: match expected {
                    Some((ep, er)) => TypeId::Func {
                        params: ep.to_vec(),
                        ret: Box::new(er.clone()),
                    }
                    .display(),
                    None => format!(
                        "a {} method group `{ty}.{method}`",
                        if want_static { "static" } else { "instance" }
                    ),
                },
                found: format!(
                    "no compatible {} method `{ty}.{method}`",
                    if want_static { "static" } else { "instance" }
                ),
            }),
            1 => Ok(matched.pop().unwrap()),
            _ => Err(TypeError::Oop(format!(
                "RFC 008: ambiguous method group `{ty}.{method}`"
            ))),
        }
    }

    fn build_method_group_lambda(
        &self,
        receiver: Spanned<Expr>,
        method: Ident,
        expected_params: &[TypeId],
        expected_ret: &TypeId,
    ) -> LambdaExpr {
        let lambda_params: Vec<LambdaParam> = expected_params
            .iter()
            .enumerate()
            .map(|(i, ty)| LambdaParam {
                name: format!("__mg_{i}").into(),
                ty: Some(Spanned::new(type_id_to_ast(ty), Span::DUMMY)),
                default: None,
            })
            .collect();
        let args: Vec<Spanned<Expr>> = lambda_params
            .iter()
            .map(|p| Spanned::new(Expr::Ident(p.name.clone()), Span::DUMMY))
            .collect();
        let call = Expr::MethodCall {
            receiver: Box::new(receiver),
            method,
            args,
            type_args: vec![],
            params_span: None,
        };
        let body = if matches!(expected_ret, TypeId::Void) {
            LambdaBody::Block(Block {
                stmts: vec![Spanned::new(
                    Stmt::Expr(Spanned::new(call, Span::DUMMY)),
                    Span::DUMMY,
                )],
                tail: None,
            })
        } else {
            LambdaBody::Expr(Box::new(Spanned::new(call, Span::DUMMY)))
        };
        LambdaExpr {
            params: lambda_params,
            body,
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        }
    }

    fn build_free_fn_lambda(
        &self,
        name: &Ident,
        expected_params: &[TypeId],
        expected_ret: &TypeId,
    ) -> LambdaExpr {
        let lambda_params: Vec<LambdaParam> = expected_params
            .iter()
            .enumerate()
            .map(|(i, ty)| LambdaParam {
                name: format!("__mg_{i}").into(),
                ty: Some(Spanned::new(type_id_to_ast(ty), Span::DUMMY)),
                default: None,
            })
            .collect();
        let args: Vec<Spanned<Expr>> = lambda_params
            .iter()
            .map(|p| Spanned::new(Expr::Ident(p.name.clone()), Span::DUMMY))
            .collect();
        let call = Expr::Call {
            func: Box::new(Spanned::new(Expr::Ident(name.clone()), Span::DUMMY)),
            args,
            type_args: vec![],
            params_span: None,
        };
        let body = if matches!(expected_ret, TypeId::Void) {
            LambdaBody::Block(Block {
                stmts: vec![Spanned::new(
                    Stmt::Expr(Spanned::new(call, Span::DUMMY)),
                    Span::DUMMY,
                )],
                tail: None,
            })
        } else {
            LambdaBody::Expr(Box::new(Spanned::new(call, Span::DUMMY)))
        };
        LambdaExpr {
            params: lambda_params,
            body,
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        }
    }

    /// 扩展方法组：`obj.Ext` → 与实例组相同 MethodCall 脱糖；MIR 再解析扩展。
    fn try_extension_method_group(
        &self,
        receiver: Spanned<Expr>,
        method: &Ident,
        recv_ty_name: &Ident,
        expected: Option<(&[TypeId], &TypeId)>,
    ) -> Result<Option<(LambdaExpr, TypeId)>, TypeError> {
        let ext = self
            .registry
            .resolve_extension(
                recv_ty_name,
                method,
                expected.map_or(0, |(ep, _)| ep.len()),
                &[],
                &self.access_ctx(),
            )
            .map_err(|e| TypeError::Oop(e.to_string()))?;
        let Some(ext) = ext else {
            return Ok(None);
        };
        if !ext.sig.generics.is_empty() || ext.inferred_arg.is_some() {
            return Err(TypeError::Oop(
                "RFC 008 M3: generic extension method groups are hard-rejected; \
                 use an explicit lambda"
                    .into(),
            ));
        }
        if let Some((ep, er)) = expected {
            if !self.sig_matches_expected(&ext.sig, ep, er)? {
                return Err(TypeError::Mismatch {
                    expected: TypeId::Func {
                        params: ep.to_vec(),
                        ret: Box::new(er.clone()),
                    }
                    .display(),
                    found: format!(
                        "extension `{}.{}` incompatible with method group",
                        ext.container, method
                    ),
                });
            }
        }
        let found_ty = self.oop_sig_to_func_type(&ext.sig)?;
        let (params, ret) = self.resolve_lambda_types(expected, &found_ty);
        Ok(Some((
            self.build_method_group_lambda(receiver, method.clone(), &params, &ret),
            found_ty,
        )))
    }

    /// 若 `expr` 为兼容方法组，脱糖为等价 lambda；否则 `Ok(None)`。
    ///
    /// `expected` 为期望委托签名（结构化 `Func` 参数/返回）。当期望为 std OOP 签名
    /// 里以 mangled 名（`Func_..._void`）存储的 `Action<...>`/`Func<...>` 时传 `None`，
    /// 脱糖按方法组自身签名进行，返回的 `TypeId` 供调用方反向校验委托兼容性。
    pub(crate) fn try_method_group_to_lambda(
        &self,
        expr: &Expr,
        expected: Option<(&[TypeId], &TypeId)>,
    ) -> Result<Option<(LambdaExpr, TypeId)>, TypeError> {
        self.reject_deferred_method_group(expr)?;

        // M1：自由函数名
        if let Expr::Ident(name) = expr {
            if self.is_free_fn_method_group(name) {
                let Some(found) = self.resolve_value_name(name) else {
                    return Ok(None);
                };
                let TypeId::Func {
                    params: found_params,
                    ret: found_ret,
                } = &found
                else {
                    return Ok(None);
                };
                if let Some((expected_params, expected_ret)) = expected {
                    if expected_params.len() != found_params.len()
                        || expected_params
                            .iter()
                            .zip(found_params.iter())
                            .any(|(e, f)| !self.types_compatible(e, f))
                        || expected_ret != found_ret.as_ref()
                    {
                        return Err(TypeError::Mismatch {
                            expected: TypeId::Func {
                                params: expected_params.to_vec(),
                                ret: Box::new(expected_ret.clone()),
                            }
                            .display(),
                            found: found.display(),
                        });
                    }
                }
                let (params, ret) = self.resolve_lambda_types(expected, &found);
                let lambda = self.build_free_fn_lambda(name, &params, &ret);
                return Ok(Some((lambda, found)));
            }

            // M2：同 class 无限定实例方法组 → `this.Foo`（需实例上下文）
            if let Some(class) = &self.current_class {
                if !self.current_fn_is_static {
                    let has_instance = self
                        .registry
                        .collect_method_overloads(class, name, &self.access_ctx())
                        .ok()
                        .map(|c| c.iter().any(|(_, s)| s.modifier != MethodModifier::Static))
                        .unwrap_or(false);
                    if has_instance {
                        let sig = self.pick_matching_method(class, name, false, expected)?;
                        let found_ty = self.oop_sig_to_func_type(&sig)?;
                        let (params, ret) = self.resolve_lambda_types(expected, &found_ty);
                        let receiver = Spanned::new(Expr::Ident("this".into()), Span::DUMMY);
                        let lambda =
                            self.build_method_group_lambda(receiver, name.clone(), &params, &ret);
                        return Ok(Some((lambda, found_ty)));
                    }
                }
            }
            return Ok(None);
        }

        // M2/M3：`C.Foo` / `obj.Foo` / 扩展；`Ns.Type.Foo` 硬拒绝立宪
        if let Expr::Field { receiver, field } = expr {
            if let Some(ty_name) = self.receiver_is_bare_type_name(&receiver.node) {
                // 裸类型名成员可能为静态字段/属性；无同名方法时退回常规检查。
                if self
                    .registry
                    .collect_method_overloads(&ty_name, field, &self.access_ctx())
                    .is_err()
                {
                    return Ok(None);
                }
                let sig = self.pick_matching_method(&ty_name, field, true, expected)?;
                let found_ty = self.oop_sig_to_func_type(&sig)?;
                let (params, ret) = self.resolve_lambda_types(expected, &found_ty);
                let lambda =
                    self.build_method_group_lambda(*receiver.clone(), field.clone(), &params, &ret);
                return Ok(Some((lambda, found_ty)));
            }

            // M3 立宪：命名空间限定静态（嵌套 Field）硬拒绝——namespaced 静态
            // MethodCall 符号路径尚未扎实，禁止仅 typeck 脱糖假绿。
            if self.looks_like_ns_qualified_type_path(&receiver.node) {
                return Err(TypeError::Oop(
                    "RFC 008 M3: namespace-qualified static method groups are hard-rejected \
                     (namespaced static call symbol path not solid); \
                     use bare `Type.Method` or an explicit lambda"
                        .into(),
                ));
            }

            // 实例 / 扩展：先解析接收者类型
            let recv_ty = self.resolve_field_receiver_type_for_method_group(&receiver.node)?;
            let Some(tname) = self.type_name_of(&recv_ty) else {
                return Err(TypeError::Oop(format!(
                    "RFC 008: cannot form method group on non-nominal receiver `{}`",
                    recv_ty.display()
                )));
            };

            match self.pick_matching_method(&tname, field, false, expected) {
                Ok(sig) => {
                    let found_ty = self.oop_sig_to_func_type(&sig)?;
                    let (params, ret) = self.resolve_lambda_types(expected, &found_ty);
                    return Ok(Some((
                        self.build_method_group_lambda(
                            *receiver.clone(),
                            field.clone(),
                            &params,
                            &ret,
                        ),
                        found_ty,
                    )));
                }
                Err(inst_err) => {
                    if let Some(found) =
                        self.try_extension_method_group(*receiver.clone(), field, &tname, expected)?
                    {
                        return Ok(Some(found));
                    }
                    // 实例方法与扩展均无同名成员 → 非方法组（字段/属性访问），
                    // 退回常规检查，避免 `obj.FuncField` 实参被误判为方法组而报错。
                    if self
                        .registry
                        .collect_method_overloads(&tname, field, &self.access_ctx())
                        .is_err()
                    {
                        return Ok(None);
                    }
                    return Err(inst_err);
                }
            }
        }

        Ok(None)
    }

    /// 方法组脱糖 lambda 的形参/返回类型：期望为结构化 Func 时沿用期望类型；
    /// 期望为 mangled Func 名（std OOP 签名存储形态，无法结构化分解）时，
    /// 回退到方法组自身签名——脱糖后的委托与期望的兼容性由调用方反向校验。
    fn resolve_lambda_types(
        &self,
        expected: Option<(&[TypeId], &TypeId)>,
        found: &TypeId,
    ) -> (Vec<TypeId>, TypeId) {
        match expected {
            Some((ep, er)) => (ep.to_vec(), er.clone()),
            None => match found {
                TypeId::Func { params, ret } => (params.clone(), ret.as_ref().clone()),
                _ => (Vec::new(), TypeId::Void),
            },
        }
    }

    /// 方法组实例接收者类型（不走完整 Field 检查，避免「无字段」假阴性）。
    ///
    /// M3 立宪：复杂接收者（`new C()`、嵌套 `a.b` 等）**硬拒绝**——需一次求值临时绑定，
    /// 本切片不交付；禁止脱糖为每次调用重求值的错误语义。
    fn resolve_field_receiver_type_for_method_group(
        &self,
        receiver: &Expr,
    ) -> Result<TypeId, TypeError> {
        match receiver {
            Expr::Ident(name) => {
                let ty = self
                    .resolve_value_name(name)
                    .or_else(|| {
                        if self.registry.types.contains_key(name) {
                            Some(TypeId::Named(name.clone()))
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| TypeError::Undefined(name.to_string()))?;
                Ok(match ty {
                    TypeId::Ref { inner, .. } => *inner,
                    other => other,
                })
            }
            Expr::This => {
                let class = self.current_class.as_ref().ok_or_else(|| {
                    TypeError::Oop("`this` is not valid outside instance method".into())
                })?;
                if self.current_fn_is_static {
                    return Err(TypeError::Oop(
                        "`this` is not valid in static method context".into(),
                    ));
                }
                Ok(TypeId::Named(class.clone()))
            }
            _ => Err(TypeError::Oop(
                "RFC 008 M3: complex instance receivers are hard-rejected \
                 (need once-eval temp binding); use a local then `obj.Method`"
                    .into(),
            )),
        }
    }

    /// Expression 期望类型下禁止方法组（M3 边界立宪）。
    pub(crate) fn reject_method_group_to_expression(&self, expr: &Expr) -> Result<(), TypeError> {
        if self.looks_like_method_group_shape(expr) {
            return Err(TypeError::Oop(
                "RFC 008 M3: method groups cannot convert to Expression<...>; \
                 use an explicit lambda `x => ...`"
                    .into(),
            ));
        }
        Ok(())
    }

    /// 期望为 `Func`/`Action` 时尝试方法组脱糖；返回重写后的表达式（或原样）。
    pub(crate) fn maybe_coerce_method_group(
        &self,
        expr: &Expr,
        expected: &TypeId,
    ) -> Result<Expr, TypeError> {
        if let TypeId::Expression { .. } = expected {
            self.reject_method_group_to_expression(expr)?;
            return Ok(expr.clone());
        }
        if let TypeId::Func { params, ret } = expected {
            if let Some((lambda, _)) = self.try_method_group_to_lambda(expr, Some((params, ret)))? {
                return Ok(Expr::Lambda(lambda));
            }
        } else if let TypeId::Named(n) = expected {
            // std OOP 签名把 `Action<...>`/`Func<...>` 参数以 mangled 名（`Func_..._void`）
            // 存储，无法结构化分解（内部类型名可含 `_`，demangle 不可靠）。此时按方法组
            // 自身签名脱糖为 lambda，再反向以字符串精确匹配校验委托类型兼容。
            if is_func_mangled_name(n.as_str()) {
                if let Some((lambda, found)) = self.try_method_group_to_lambda(expr, None)? {
                    if !self.types_compatible(expected, &found) {
                        return Err(TypeError::Mismatch {
                            expected: expected.display(),
                            found: found.display(),
                        });
                    }
                    return Ok(Expr::Lambda(lambda));
                }
            } else {
                self.reject_deferred_method_group(expr)?;
            }
        } else {
            self.reject_deferred_method_group(expr)?;
        }
        Ok(expr.clone())
    }
}
