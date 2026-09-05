//! RFC 007：调用点可选/命名实参绑定（typeck 脱糖）。

use crate::call_args::{
    bind_call_args, fold_param_default_lookup, validate_param_defaults_lookup, ParamSlot,
};
use crate::checker::check_native::box_to_object;
use crate::checker::TypeChecker;
use crate::error::TypeError;
use crate::oop_types::{AccessContext, ConstValue, ParamSig};
use crate::type_id::TypeId;
use crate::typed::TypedExpr;
use ast::*;
use indexmap::IndexMap;

impl TypeChecker {
    /// RFC 007 M2b：解析 `Type.ConstField` 为已注册的 const 值。
    ///
    /// 枚举成员（`ServiceLifetime.Scoped`）亦按判别值折叠为整型常量，
    /// 使枚举成员可作为形参默认值（如 `[Inject(ServiceLifetime.Scoped)]`）。
    pub(crate) fn lookup_const_field(&self, type_name: &str, field: &str) -> Option<ConstValue> {
        self.registry.types.get(type_name).and_then(|n| {
            n.const_values.get(field).cloned().or_else(|| {
                n.variants
                    .iter()
                    .find(|v| v.name.as_str() == field)
                    .map(|v| ConstValue::Int(v.discriminant as i64))
            })
        })
    }

    pub(crate) fn fold_param_default_expr(&self, expr: &Expr) -> Option<ConstValue> {
        fold_param_default_lookup(expr, &|t, f| self.lookup_const_field(t, f))
    }

    pub(crate) fn validate_params_m2b(&self, params: &[Param]) -> Result<(), TypeError> {
        validate_param_defaults_lookup(params, &|t, f| self.lookup_const_field(t, f))
    }

    pub(crate) fn param_sig_type_id(&self, ty_name: &Ident) -> TypeId {
        let name_str = ty_name.as_str();
        // `Task<...>` 优先解构：`Task<string[]>` 与其数组 `Task<string>[]` 的 mangle
        // 共用 `Task_string_arr`（`Task_{arr}` 后缀扁平编码无法区分嵌套），在 `_arr`
        // 剥离前按 Task-of-array 解码，否则 await 时报 "expected Task<T>, found
        // Task_string[]"（RFC 009：async 返回 `T[]` 时触发，如 `ReadAllLinesAsync`）。
        if let Some(inner_mangle) = name_str.strip_prefix("Task_") {
            let inner = self.param_sig_type_id(&Ident::from(inner_mangle));
            return TypeId::Task {
                inner: Box::new(inner),
            };
        }
        if let Some(inner) = name_str.strip_suffix("_arr") {
            let inner_ty = self.param_sig_type_id(&Ident::from(inner));
            return TypeId::Array {
                elem: Box::new(inner_ty),
            };
        }
        match name_str {
            "int" => TypeId::Int,
            "long" => TypeId::Long,
            "short" => TypeId::Short,
            "byte" => TypeId::Byte,
            "char" => TypeId::Char,
            "float" => TypeId::Float,
            "double" => TypeId::Double,
            "bool" => TypeId::Bool,
            "uint" => TypeId::UInt,
            "ulong" => TypeId::ULong,
            "ushort" => TypeId::UShort,
            "sbyte" => TypeId::SByte,
            "string" => TypeId::String,
            "object" => TypeId::Object,
            "void" => TypeId::Void,
            other => {
                if let Some(ty) = self.registry.delegate_aliases.get(other) {
                    return ty.clone();
                }
                if let Some(rest) = other.strip_prefix("Func_") {
                    if let Some(ty) = self.parse_func_field_name(rest) {
                        return ty;
                    }
                }
                if let Some(rest) = other.strip_prefix("Action_") {
                    if let Some(ty) = self.parse_action_field_name(rest) {
                        return ty;
                    }
                }
                self.resolve_type_param(&Ident::from(other))
                    .unwrap_or_else(|| TypeId::Named(Ident::from(other)))
            }
        }
    }

    /// 注册表感知的 mangle 名段切分（S0 根因 A）：`Func_...` / `Action_...` 实参名
    /// 以 `_` 连接，但复合泛型实参自身 mangle 也含 `_`（如
    /// `ObservableCollection_int`），朴素 `split('_')` 会把
    /// `Func_ObservableCollection_int_ObservableCollection_int_bool` 误切成 4 参。
    /// 此处按「已知类型名（含单态化 `X_Y`）+ 基元 + `_arr` 数组」做**最长匹配**
    /// 贪婪切分，还原真实参数边界（RFC 011 委托 `Func<C1, C2>` 场景）。
    fn split_mangled_type_names(&self, name: &str) -> Option<Vec<TypeId>> {
        let mut rest = name;
        let mut out = Vec::new();
        while !rest.is_empty() {
            let mut best_len = 0usize;
            // 注册表已知类型名（含单态化 `X_Y`）——最长优先。
            for k in self.registry.types.keys() {
                let ks = k.as_str();
                if rest.starts_with(ks) && ks.len() > best_len {
                    best_len = ks.len();
                }
            }
            // 比已匹配注册名更长的段：经 param_sig_type_id 解码为非恒等 Named
            // （基元 / `X_arr` 数组 / 可解析别名）则取为最长段。
            for cand_len in (best_len + 1..=rest.len()).rev() {
                let cand = &rest[..cand_len];
                if let TypeId::Named(n) = self.param_sig_type_id(&Ident::from(cand)) {
                    if n.as_str() == cand {
                        continue;
                    }
                }
                best_len = cand_len;
                break;
            }
            if best_len == 0 {
                return None;
            }
            out.push(self.param_sig_type_id(&Ident::from(&rest[..best_len])));
            rest = &rest[best_len..];
        }
        Some(out)
    }

    fn parse_func_field_name(&self, rest: &str) -> Option<TypeId> {
        let names = self.split_mangled_type_names(rest)?;
        let (ret, params) = names.split_last()?;
        Some(TypeId::Func {
            params: params.to_vec(),
            ret: Box::new(ret.clone()),
        })
    }

    fn parse_action_field_name(&self, rest: &str) -> Option<TypeId> {
        let names = self.split_mangled_type_names(rest)?;
        Some(TypeId::Func {
            params: names,
            ret: Box::new(TypeId::Void),
        })
    }

    pub(crate) fn param_slots_from_sigs(&self, params: &[ParamSig]) -> Vec<ParamSlot> {
        params
            .iter()
            .map(|p| ParamSlot {
                name: p.name.clone(),
                ty: self.canonical_type(&self.param_sig_type_id(&p.ty)),
                default: p.default.clone(),
                is_params: p.is_params,
            })
            .collect()
    }

    /// RFC 005：扩展方法调用点实参绑定。
    ///
    /// 扩展方法 `sig.params` 已剔除 `this` 接收者（注册时 `remove(0)`），故直接
    /// 由其构建槽位。末位 `params Span`/`ReadOnlySpan` 槽经 [`bind_args_with_params_span`]
    /// 把尾随实参**保留为独立实参**并返回 [`ParamsSpanInfo`] 标注——由 MIR 的**单一
    /// 物化点** `SpanFromStack` 收集发射胖指针，与用户方法/自由函数路径统一，
    /// 根除「各调用形态碰巧保留脱糖」的路径分裂（typeck 不再注入 `StackSpanLit`）。
    /// 非 params 扩展亦经同一绑定路径统一校验。
    pub(crate) fn bind_extension_args(
        &mut self,
        sig_params: &[ParamSig],
        args: &[Spanned<Expr>],
    ) -> Result<(Vec<Spanned<Expr>>, Option<ParamsSpanInfo>), TypeError> {
        let slots = self.param_slots_from_sigs(sig_params);
        self.bind_args_to_slots(&slots, args)
    }

    /// RFC 007 M2b 绑定结果：实参列表 + 可选 `params Span<T>` 标注（RFC 005）。
    pub(crate) fn bind_args_to_slots(
        &mut self,
        slots: &[ParamSlot],
        args: &[Spanned<Expr>],
    ) -> Result<(Vec<Spanned<Expr>>, Option<ParamsSpanInfo>), TypeError> {
        if slots.last().is_some_and(|s| s.is_params) {
            return self.bind_args_with_params_span(slots, args);
        }
        // RFC 006 M2：调用实参目标上下文——按位置/命名槽填入期望类型后再 check。
        let prepared_args = self.prepare_call_args_target_typed(slots, args)?;
        // RFC 004 M1：Func/Action 槽上的自由函数方法组 → lambda。
        let prepared_args = self.prepare_call_args_method_group(slots, &prepared_args)?;
        let (mut bound, tys) = bind_call_args(slots, &prepared_args, |e, expected| {
            // RFC 017 残余补全：集合表达式实参对 `T[]` 槽——按目标元素类型
            // 检查并以槽类型承载（与赋值目标 try_bind_collection_array_target
            // 同语义）；否则单元素集合被独立推断为元素类型
            //（`app.Inject(["dep"], cb)` 的 `["dep"]` → string）。
            if let (Expr::CollectionExpr { .. }, TypeId::Array { .. }) = (e, expected) {
                if self.try_bind_collection_array_target(e, expected)? {
                    return Ok((expected.clone(), e.clone()));
                }
            }
            let te = self.check_expr(e)?;
            Ok((te.ty, te.expr))
        })?;
        for (i, ty) in tys.iter().enumerate() {
            // 默认值填充槽：ty 已等于形参类型，跳过。
            // Func/Action 槽收到内联 lambda（未绑定 → Func_Infer 名）：以槽形参
            // 类型定向校验 λ（arity + 体），放行透传——名字比对无法处理 Func_Infer
            // 与目标 mangle 名（扩展方法实参统一绑定路径，与实例方法路径同规则）。
            if matches!(
                &slots[i].ty,
                TypeId::Named(n)
                    if n.as_str() == "Func"
                        || n.as_str().starts_with("Func_")
                        || n.as_str() == "Action"
                        || n.as_str().starts_with("Action_")
            ) {
                if let Expr::Lambda(l) = &bound[i].node {
                    let tname = match &slots[i].ty {
                        TypeId::Named(n) => n.as_str(),
                        _ => unreachable!(),
                    };
                    if let Some(TypeId::Func { params, ret }) =
                        crate::check_expr::demangle_func_type_with(tname, l.params.len(), &|s| {
                            self.registry.types.contains_key(s)
                        })
                    {
                        self.check_func_lambda(l, &params, &ret)?;
                    }
                    continue;
                }
            }
            if !self.types_compatible(&slots[i].ty, ty) {
                // RFC 004 §D9 / RFC 037 M2：实参类型不匹配时尝试隐式 variant
                // 构造。典型场景：方法形参为 `ContentVariant`，调用方传入
                // `string` → 自动包装为 `ContentVariant.Text(string)`。
                let span = bound[i].span;
                if let Some(coerced) =
                    self.coerce_to_variant(bound[i].node.clone(), ty, &slots[i].ty)
                {
                    bound[i] = Spanned::new(coerced, span);
                } else {
                    return Err(TypeError::Mismatch {
                        expected: slots[i].ty.display(),
                        found: ty.display(),
                    });
                }
            } else {
                // RFC 004 P0 Phase 1：object 形参 + string/基元实参 → 装箱。
                let span = bound[i].span;
                let param_ty = self.type_name_of(&slots[i].ty).unwrap_or_default();
                let boxed = box_to_object(
                    &self.registry,
                    bound[i].node.clone(),
                    ty,
                    param_ty.as_str(),
                    span,
                );
                bound[i] = Spanned::new(boxed, span);
            }
        }
        Ok((bound, None))
    }

    /// RFC 005：`params ReadOnlySpan<T>` / `params Span<T>` 调用点**纯标注**。
    ///
    /// 固定前缀按普通槽绑定；尾随实参**保留为独立实参**（不再打包为
    /// `Expr::StackSpanLit`），并返回 [`ParamsSpanInfo`] 供 MIR 单一物化点收集为
    /// `MirRvalue::SpanFromStack`。单尾随实参且已是 Span/ROS（或 `Span→ROS`）时
    /// 直接透传（C# 语义），由 MIR 判定，不二次打包。
    fn bind_args_with_params_span(
        &mut self,
        slots: &[ParamSlot],
        args: &[Spanned<Expr>],
    ) -> Result<(Vec<Spanned<Expr>>, Option<ParamsSpanInfo>), TypeError> {
        let n = slots.len();
        debug_assert!(n >= 1 && slots[n - 1].is_params);
        let params_ty = &slots[n - 1].ty;
        let fixed = n - 1;
        let fixed_slots = &slots[..fixed];

        let mut positional: Vec<&Spanned<Expr>> = Vec::new();
        let mut named: Vec<(&Ident, &Spanned<Expr>)> = Vec::new();
        let mut seen_named = false;
        for a in args {
            match &a.node {
                Expr::NamedArg { name, expr } => {
                    seen_named = true;
                    named.push((name, expr.as_ref()));
                }
                _ => {
                    if seen_named {
                        return Err(TypeError::Oop(
                            "positional argument cannot follow named argument".into(),
                        ));
                    }
                    positional.push(a);
                }
            }
        }

        let params_named = named
            .iter()
            .find(|(name, _)| **name == slots[n - 1].name)
            .map(|(_, e)| *e);
        for (name, _) in &named {
            if fixed_slots.iter().all(|s| s.name != **name) && **name != slots[n - 1].name {
                return Err(TypeError::Oop(format!("unknown named argument `{name}`")));
            }
        }

        let fixed_from_pos = if params_named.is_some() {
            positional.len().min(fixed)
        } else if positional.len() >= fixed {
            fixed
        } else {
            positional.len()
        };

        let mut out: Vec<Spanned<Expr>> = Vec::with_capacity(n);
        for (i, slot) in fixed_slots.iter().enumerate() {
            if i < fixed_from_pos {
                let a = positional[i];
                let prepared = self.prepare_target_expr(&a.node, &slot.ty, a.span)?;
                let prepared = self.maybe_coerce_method_group(&prepared, &slot.ty)?;
                if self.try_bind_collection_array_target(&prepared, &slot.ty)? {
                    out.push(Spanned::new(prepared, a.span));
                } else {
                    let te = self.check_expr(&prepared)?;
                    if !self.types_compatible(&slot.ty, &te.ty) {
                        return Err(TypeError::Mismatch {
                            expected: slot.ty.display(),
                            found: te.ty.display(),
                        });
                    }
                    out.push(Spanned::new(te.expr, a.span));
                }
            } else if let Some((_, e)) = named.iter().find(|(name, _)| **name == slot.name) {
                let prepared = self.prepare_target_expr(&e.node, &slot.ty, e.span)?;
                let prepared = self.maybe_coerce_method_group(&prepared, &slot.ty)?;
                if self.try_bind_collection_array_target(&prepared, &slot.ty)? {
                    out.push(Spanned::new(prepared, e.span));
                } else {
                    let te = self.check_expr(&prepared)?;
                    if !self.types_compatible(&slot.ty, &te.ty) {
                        return Err(TypeError::Mismatch {
                            expected: slot.ty.display(),
                            found: te.ty.display(),
                        });
                    }
                    out.push(Spanned::new(te.expr, e.span));
                }
            } else if let Some(def) = &slot.default {
                // `default(T)` 语义填充：Null 默认值以 default(槽类型) 表达而非
                // null 字面量——值类型/stub 槽（如 CancellationToken）不接受
                // null 字面量，但 default(T) 对任意类型合法（RFC 007）。
                let expr = match def {
                    ConstValue::Null => Expr::Default {
                        ty: Spanned::new(crate::generics::type_id_to_ast(&slot.ty), Span::DUMMY),
                    },
                    other => crate::call_args::const_to_expr(other),
                };
                out.push(Spanned::new(expr, Span::DUMMY));
            } else {
                return Err(TypeError::Mismatch {
                    expected: format!("argument for parameter `{}`", slot.name),
                    found: "missing".into(),
                });
            }
        }

        match params_ty {
            TypeId::Span { elem, mutable } => {
                let elem_ty = elem.as_ref().clone();
                let mutable = *mutable;
                if let Some(e) = params_named {
                    if fixed_from_pos < positional.len() {
                        return Err(TypeError::Oop(
                            "cannot combine named `params` argument with trailing positional elements"
                                .into(),
                        ));
                    }
                    let te = self.check_expr_at(e.span, &e.node)?;
                    if !self.types_compatible(params_ty, &te.ty) {
                        return Err(TypeError::Mismatch {
                            expected: params_ty.display(),
                            found: te.ty.display(),
                        });
                    }
                    out.push(Spanned::new(te.expr, e.span));
                } else {
                    let trailing = &positional[fixed_from_pos..];
                    if trailing.len() == 1 {
                        let te = self.check_expr_at(trailing[0].span, &trailing[0].node)?;
                        if !self.types_compatible(params_ty, &te.ty)
                            && !self.types_compatible(&elem_ty, &te.ty)
                        {
                            return Err(TypeError::Mismatch {
                                expected: format!(
                                    "{} or {}",
                                    params_ty.display(),
                                    elem_ty.display()
                                ),
                                found: te.ty.display(),
                            });
                        }
                        out.push(Spanned::new(te.expr, trailing[0].span));
                    } else {
                        for a in trailing {
                            let prepared = self.prepare_target_expr(&a.node, &elem_ty, a.span)?;
                            let te = self.check_expr(&prepared)?;
                            if !self.types_compatible(&elem_ty, &te.ty) {
                                return Err(TypeError::Mismatch {
                                    expected: elem_ty.display(),
                                    found: te.ty.display(),
                                });
                            }
                            out.push(Spanned::new(te.expr, a.span));
                        }
                    }
                };
                let info = ParamsSpanInfo {
                    fixed,
                    elem: elem_ty,
                    mutable,
                };
                Ok((out, Some(info)))
            }
            TypeId::Array { elem } => {
                let elem_ty = elem.as_ref().clone();
                if let Some(e) = params_named {
                    if fixed_from_pos < positional.len() {
                        return Err(TypeError::Oop(
                            "cannot combine named `params` argument with trailing positional elements"
                                .into(),
                        ));
                    }
                    let te = self.check_expr_at(e.span, &e.node)?;
                    if !self.types_compatible(params_ty, &te.ty) {
                        return Err(TypeError::Mismatch {
                            expected: params_ty.display(),
                            found: te.ty.display(),
                        });
                    }
                    out.push(Spanned::new(te.expr, e.span));
                } else {
                    let trailing = &positional[fixed_from_pos..];
                    if trailing.len() == 1 {
                        let te = self.check_expr_at(trailing[0].span, &trailing[0].node)?;
                        if self.types_compatible(params_ty, &te.ty) {
                            out.push(Spanned::new(te.expr, trailing[0].span));
                        } else {
                            let prepared = self.prepare_target_expr(
                                &trailing[0].node,
                                &elem_ty,
                                trailing[0].span,
                            )?;
                            let te2 = self.check_expr(&prepared)?;
                            if !self.types_compatible(&elem_ty, &te2.ty) {
                                return Err(TypeError::Mismatch {
                                    expected: elem_ty.display(),
                                    found: te2.ty.display(),
                                });
                            }
                            let coll = Expr::CollectionExpr {
                                elements: vec![CollectionElement::Element(Spanned::new(
                                    te2.expr,
                                    trailing[0].span,
                                ))],
                            };
                            out.push(Spanned::new(coll, Span::DUMMY));
                        }
                    } else {
                        let elements: Vec<CollectionElement> = trailing
                            .iter()
                            .map(|a| {
                                let prepared =
                                    self.prepare_target_expr(&a.node, &elem_ty, a.span)?;
                                let te = self.check_expr(&prepared)?;
                                if !self.types_compatible(&elem_ty, &te.ty) {
                                    return Err(TypeError::Mismatch {
                                        expected: elem_ty.display(),
                                        found: te.ty.display(),
                                    });
                                }
                                Ok(CollectionElement::Element(Spanned::new(te.expr, a.span)))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let coll = Expr::CollectionExpr { elements };
                        if self.try_bind_collection_array_target(&coll, params_ty)? {
                            out.push(Spanned::new(coll, Span::DUMMY));
                        } else {
                            let te = self.check_expr(&coll)?;
                            if !self.types_compatible(params_ty, &te.ty) {
                                return Err(TypeError::Mismatch {
                                    expected: params_ty.display(),
                                    found: te.ty.display(),
                                });
                            }
                            out.push(Spanned::new(te.expr, Span::DUMMY));
                        }
                    }
                }
                Ok((out, None))
            }
            _ => Err(TypeError::Oop(
                "`params` requires `Span<T>`, `ReadOnlySpan<T>`, or `T[]`".into(),
            )),
        }
    }

    /// RFC 005：`new T(...)` 构造点无 `params_span` 字段，故当绑定解析返回 params 标注时，
    /// 把尾随实参重新打包为 [`Expr::StackSpanLit`]（复用集合字面量物化路径，保持构造行为
    /// 不变）。方法/自由函数调用则走 MIR 的**单一物化点** `SpanFromStack`，不入此路径。
    ///
    /// 单尾随实参且已是 Span/ROS（C# 语义）时直接透传，不二次打包（与 MIR 判定一致）。
    pub(crate) fn rewrap_params_trailing_as_stack_span(
        &mut self,
        mut args: Vec<Spanned<Expr>>,
        info: &ParamsSpanInfo,
    ) -> Result<Vec<Spanned<Expr>>, TypeError> {
        if info.fixed >= args.len() {
            return Ok(args);
        }
        let trailing = args.split_off(info.fixed);
        if trailing.len() == 1 {
            let te = self.check_expr_at(trailing[0].span, &trailing[0].node)?;
            let span_ty = TypeId::Span {
                elem: Box::new(info.elem.clone()),
                mutable: info.mutable,
            };
            if self.types_compatible(&span_ty, &te.ty) {
                let mut out = args;
                out.push(trailing.into_iter().next().unwrap());
                return Ok(out);
            }
        }
        let span = trailing.first().map(|e| e.span).unwrap_or(Span::DUMMY);
        let mut out = args;
        out.push(Spanned::new(
            Expr::StackSpanLit {
                elements: trailing,
                mutable: info.mutable,
                elem: info.elem.clone(),
            },
            span,
        ));
        Ok(out)
    }

    /// RFC 004 M1：按槽期望 `Func`/`Action` 将自由函数方法组脱糖为 lambda。
    fn prepare_call_args_method_group(
        &self,
        slots: &[ParamSlot],
        args: &[Spanned<Expr>],
    ) -> Result<Vec<Spanned<Expr>>, TypeError> {
        let mut out = Vec::with_capacity(args.len());
        let mut positional_idx = 0usize;
        for a in args {
            match &a.node {
                Expr::NamedArg { name, expr } => {
                    let coerced = if let Some(slot) = slots.iter().find(|s| s.name == *name) {
                        self.maybe_coerce_method_group(&expr.node, &slot.ty)?
                    } else {
                        expr.node.clone()
                    };
                    out.push(Spanned::new(
                        Expr::NamedArg {
                            name: name.clone(),
                            expr: Box::new(Spanned::new(coerced, expr.span)),
                        },
                        a.span,
                    ));
                }
                _ => {
                    let coerced = if positional_idx < slots.len() {
                        self.maybe_coerce_method_group(&a.node, &slots[positional_idx].ty)?
                    } else {
                        a.node.clone()
                    };
                    positional_idx += 1;
                    out.push(Spanned::new(coerced, a.span));
                }
            }
        }
        Ok(out)
    }

    /// 按位置/命名形参槽对实参应用目标类型 `new()`（RFC 006 M2）。
    fn prepare_call_args_target_typed(
        &self,
        slots: &[ParamSlot],
        args: &[Spanned<Expr>],
    ) -> Result<Vec<Spanned<Expr>>, TypeError> {
        let mut out = Vec::with_capacity(args.len());
        let mut positional_idx = 0usize;
        for a in args {
            match &a.node {
                Expr::NamedArg { name, expr } => {
                    let prepared = if let Some(slot) = slots.iter().find(|s| s.name == *name) {
                        self.prepare_target_expr(&expr.node, &slot.ty, expr.span)?
                    } else {
                        expr.node.clone()
                    };
                    out.push(Spanned::new(
                        Expr::NamedArg {
                            name: name.clone(),
                            expr: Box::new(Spanned::new(prepared, expr.span)),
                        },
                        a.span,
                    ));
                }
                _ => {
                    let prepared = if let Some(slot) = slots.get(positional_idx) {
                        self.prepare_target_expr(&a.node, &slot.ty, a.span)?
                    } else {
                        a.node.clone()
                    };
                    out.push(Spanned::new(prepared, a.span));
                    positional_idx += 1;
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn try_bind_free_fn_call(
        &mut self,
        name: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<Option<TypedExpr>, TypeError> {
        let Some(fdef) = self.fn_defs.get(name).cloned() else {
            return Ok(None);
        };
        // 预折叠默认值，避免 lower_type 与 const lookup 同时借用 self。
        let folded_defaults: Vec<Option<ConstValue>> = fdef
            .params
            .iter()
            .map(|p| {
                p.default
                    .as_ref()
                    .and_then(|e| self.fold_param_default_expr(&e.node))
            })
            .collect();
        self.validate_params_m2b(&fdef.params)?;
        let mut slots = Vec::with_capacity(fdef.params.len());
        for (i, p) in fdef.params.iter().enumerate() {
            let ty = self.lower_type(&p.ty.node)?;
            if let Some(ref d) = folded_defaults[i] {
                crate::call_args::check_default_type(&ty, d)?;
            }
            slots.push(ParamSlot {
                name: p.name.clone(),
                ty,
                default: folded_defaults[i].clone(),
                is_params: p.is_params,
            });
        }
        // RFC 005：声明点校验 `params` 类型。
        if let Some(p) = fdef.params.iter().find(|p| p.is_params) {
            let pty = self.lower_type(&p.ty.node)?;
            if !matches!(
                self.canonical_type(&pty),
                TypeId::Span { .. } | TypeId::Array { .. }
            ) {
                return Err(TypeError::Oop(
                    "`params` requires `Span<T>`, `ReadOnlySpan<T>`, or `T[]`".into(),
                ));
            }
        }
        let (bound, params_span) = self.bind_args_to_slots(&slots, args)?;
        let ret = self.fn_return_type(fdef.ret.as_ref(), fdef.is_async)?;
        Ok(Some(TypedExpr {
            ty: ret,
            expr: Expr::Call {
                func: Box::new(Spanned::new(Expr::Ident(name.clone()), Span::DUMMY)),
                args: bound,
                type_args: vec![],
                params_span,
            },
            linq_path: None,
            expression_tree: None,
        }))
    }

    pub(crate) fn resolve_bind_method_call(
        &mut self,
        recv_ty: &Ident,
        method: &Ident,
        args: &[Spanned<Expr>],
        ctx: &AccessContext,
    ) -> Result<
        (
            Ident,
            crate::oop_types::OopMethodSig,
            Vec<Spanned<Expr>>,
            Option<ParamsSpanInfo>,
        ),
        TypeError,
    > {
        let candidates = self
            .registry
            .collect_method_overloads(recv_ty, method, ctx)
            .map_err(|e| TypeError::Oop(e.to_string()))?;
        let mut matches = Vec::new();
        for (decl, sig) in &candidates {
            let slots = self.param_slots_from_sigs(&sig.params);
            match self.bind_args_to_slots(&slots, args) {
                Ok((bound, params_span)) => {
                    matches.push((decl.clone(), sig.clone(), bound, params_span));
                }
                Err(e) => {
                    if std::env::var("ARC_DEBUG_BIND").is_ok() {
                        eprintln!(
                            "[BIND] {}.{} reject: {} | sig={:?}",
                            decl, sig.name, e, sig.params
                        );
                    }
                }
            }
        }
        match matches.len() {
            0 => {
                if std::env::var("ARC_DEBUG_BIND").is_ok() {
                    for a in args {
                        eprintln!("[BIND] arg: {:?}", a);
                    }
                }
                Err(TypeError::Oop(format!(
                    "no matching overload for `{recv_ty}.{method}`"
                )))
            }
            1 => Ok(matches.pop().unwrap()),
            _ => {
                // CD-18/G3（对齐 C# §Overload resolution）：normal form 候选优先于
                // expanded form 候选。`Sum(int)` 与 `Sum(params ReadOnlySpan<int>)`
                // 对 `c.Sum(5)` 都能绑定——前者无 params 标注（normal form）应胜出；
                // 仅当所有候选都需 params 展开（expanded form）时才进入既有歧义判定。
                let normal: Vec<_> = matches
                    .iter()
                    .filter(|(_, _, _, params_span)| params_span.is_none())
                    .collect();
                if normal.len() == 1 {
                    Ok((
                        normal[0].0.clone(),
                        normal[0].1.clone(),
                        normal[0].2.clone(),
                        normal[0].3.clone(),
                    ))
                } else {
                    Err(TypeError::Oop(format!(
                        "ambiguous overload for `{recv_ty}.{method}`"
                    )))
                }
            }
        }
    }

    /// RFC 007 M2：`new T(...)` 可选/命名实参绑定，脱糖为完整位置实参。
    pub(crate) fn resolve_bind_ctor(
        &mut self,
        type_name: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<
        (
            crate::oop_types::CtorSig,
            Vec<Spanned<Expr>>,
            Option<ParamsSpanInfo>,
        ),
        TypeError,
    > {
        let candidates = self.registry.ctor_signatures(type_name).to_vec();
        if candidates.is_empty() {
            if std::env::var("ARC_DEBUG_CTOR").is_ok() {
                eprintln!(
                    "[ctor-bind] {} args={} candidates=EMPTY",
                    type_name,
                    args.len()
                );
            }
            // 无显式构造：仅允许无实参 `new T()`（既有语义）；有实参则失败。
            if args.is_empty() {
                return Ok((
                    crate::oop_types::CtorSig {
                        vis: Visibility::Public,
                        param_types: vec![],
                        params: vec![],
                        sets_required_members: Default::default(),
                    },
                    vec![],
                    None,
                ));
            }
            return Err(TypeError::Oop(format!(
                "no matching constructor for `{type_name}`"
            )));
        }
        let mut matches = Vec::new();
        for ctor in &candidates {
            let slots = self.param_slots_from_sigs(&ctor.params);
            if let Ok((bound, params_span)) = self.bind_args_to_slots(&slots, args) {
                matches.push((ctor.clone(), bound, params_span))
            }
        }
        match matches.len() {
            0 => Err(TypeError::Oop(format!(
                "no matching constructor for `{type_name}`"
            ))),
            1 => Ok(matches.pop().unwrap()),
            _ => {
                // CD-18/G3（对齐 C# §Overload resolution）：normal form 构造候选
                // 优先于需 params 展开的候选，与 `resolve_bind_method_call` 同规则。
                let normal: Vec<_> = matches
                    .iter()
                    .filter(|(_, _, params_span)| params_span.is_none())
                    .collect();
                if normal.len() == 1 {
                    Ok((
                        normal[0].0.clone(),
                        normal[0].1.clone(),
                        normal[0].2.clone(),
                    ))
                } else {
                    Err(TypeError::Oop(format!(
                        "ambiguous constructor overload for `{type_name}`"
                    )))
                }
            }
        }
    }

    /// RFC 007 M2c：lambda 立即调用（IIFE）可选/命名实参脱糖。
    ///
    /// `Func`/`Action` 不携带默认元数据；仅当 callee 字面量为 `Lambda` 时填默认。
    pub(crate) fn check_lambda_iife_call(
        &mut self,
        lambda: &LambdaExpr,
        args: &[Spanned<Expr>],
        type_args: &[Spanned<Type>],
    ) -> Result<TypedExpr, TypeError> {
        if !type_args.is_empty() {
            return Err(TypeError::Oop(
                "lambda immediate call cannot take type arguments".into(),
            ));
        }
        if lambda.is_expression_tree {
            return Err(TypeError::QueryableRequiresExpression);
        }
        if lambda.is_async {
            return Err(TypeError::Oop(
                "async lambda immediate call with defaults is not supported (RFC 007 M2c)".into(),
            ));
        }
        let slots =
            crate::call_args::lambda_param_slots(&lambda.params, &mut |t| self.lower_type(t))?;
        let (bound, params_span) = self.bind_args_to_slots(&slots, args)?;
        // 校验 body（形参入作用域），推断返回类型。
        let param_tys: Vec<TypeId> = slots.iter().map(|s| s.ty.clone()).collect();
        self.scopes.push(IndexMap::new());
        for (p, ty) in lambda.params.iter().zip(param_tys.iter()) {
            self.scopes
                .last_mut()
                .unwrap()
                .insert(p.name.clone(), ty.clone());
        }
        let ret_ty = match &lambda.body {
            LambdaBody::Expr(e) => self.check_expr_at(e.span, &e.node)?.ty,
            LambdaBody::Block(b) => {
                self.return_slot.push(TypeId::Infer);
                for stmt in &b.stmts {
                    self.check_stmt(&stmt.node)?;
                }
                let ty = if let Some(tail) = &b.tail {
                    self.check_expr_at(tail.span, &tail.node)?.ty
                } else {
                    TypeId::Void
                };
                self.return_slot.pop();
                ty
            }
        };
        self.scopes.pop();
        Ok(TypedExpr {
            ty: ret_ty,
            expr: Expr::Call {
                func: Box::new(Spanned::new(Expr::Lambda(lambda.clone()), Span::DUMMY)),
                args: bound,
                type_args: vec![],
                params_span,
            },
            linq_path: None,
            expression_tree: None,
        })
    }

    /// RFC 007 M2c / D7：非 IIFE 使用处拒绝带默认值的 lambda。
    ///
    /// `TypeId::Func` 不携带默认槽；经委托省略实参须独立 RFC。
    pub(crate) fn reject_lambda_defaults_outside_iife(
        params: &[LambdaParam],
    ) -> Result<(), TypeError> {
        if crate::call_args::lambda_has_defaults(params) {
            return Err(TypeError::Oop(
                "lambda parameter defaults are only supported on immediate calls \
                 (IIFE); Func/Action types do not carry default metadata (RFC 007 M2c/D7)"
                    .into(),
            ));
        }
        Ok(())
    }
}
