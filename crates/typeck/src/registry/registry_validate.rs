use super::*;
use crate::generics::mangle_generic;
use std::collections::HashMap;

/// 类型标识符字节（`[A-Za-z0-9]`），用于泛型词法替换的边界判定。
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

impl TypeRegistry {
    /// C# subtyping: class extends class; class implements interface; interface extends interface.
    pub fn is_subtype(&self, sub: &Ident, sup: &Ident) -> bool {
        if sub == sup {
            return true;
        }
        let Some(sub_ty) = self.types.get(sub) else {
            return false;
        };
        for base in &sub_ty.bases {
            if self.is_subtype(base, sup) {
                return true;
            }
        }
        // Interface implementation: class / struct 均可通过 bases 满足接口
        if self.is_interface(sup) && matches!(sub_ty.kind, TypeKind::Class | TypeKind::Struct) {
            return self.implements_interface(sub, sup);
        }
        false
    }

    /// C# extension method resolution: instance call desugars to static call with receiver first.
    ///
    /// 决策 #8（RFC 010）：候选集合化 + 优先级消解。
    /// - 收集所有可见且可访问的候选；
    /// - 无候选：返回 `Ok(None)`；
    /// - 唯一候选：返回 `Ok(Some(...))`；
    /// - 多候选：按 C# 规则 1（同命名空间优先）+ 规则 2（更具体接收者优先）
    ///   + 规则 2.5（实参个数匹配优先）消解；若仍并列，返回 `Err(AmbiguousExtensionCall)`。
    ///
    /// 决策 #7（RFC 010）：泛型扩展方法支持。
    /// 返回 `ExtensionResolution.call_name` 已按接收者类型 mangle（如 `FooExt::Id_int`），
    /// MIR/codegen 可直接使用；`inferred_arg` 标识泛型扩展，typeck 据此触发单态化。
    ///
    /// `arg_count` 为调用点**值实参个数（不含接收者）**，用于规则 2.5：
    /// `AddTransient<TService,TImpl>(this IServiceCollection)`（0 值参）与
    /// `AddTransient<TService>(this IServiceCollection, Func<...>)`（1 值参）
    /// 并列时按实参个数区分，消除歧义。
    pub fn resolve_extension(
        &self,
        ty: &Ident,
        method: &Ident,
        arg_count: usize,
        type_args: &[Ident],
        ctx: &AccessContext,
    ) -> Result<Option<ExtensionResolution>, OopError> {
        self.resolve_extension_with_arg_types(ty, method, arg_count, type_args, &[], ctx)
    }

    /// [`resolve_extension`] 的增强版：额外接收调用点实参类型名（`arg_type_names`，
    /// 已去掉接收者，按值实参顺序对齐），用于在接收者特异性并列时按值参数类型
    /// 消歧（如 `AddSingleton<T>(T instance)` 与 `AddSingleton<T>(Func<...> factory)`
    /// 的并列）。`arg_type_names` 为空时退化为仅按接收者特异性解析（历史语义）。
    #[allow(clippy::too_many_arguments)]
    pub fn resolve_extension_with_arg_types(
        &self,
        ty: &Ident,
        method: &Ident,
        arg_count: usize,
        type_args: &[Ident],
        arg_type_names: &[Ident],
        ctx: &AccessContext,
    ) -> Result<Option<ExtensionResolution>, OopError> {
        let scope = &ctx.extension_scope;
        // 候选：(container, method_sig, namespace, ext_ty, inferred_arg, template_key, mangle_base)
        let mut candidates: Vec<(
            Ident,
            OopMethodSig,
            Vec<Ident>,
            Ident,
            Option<Ident>,
            Ident,
            Ident,
        )> = Vec::new();
        for (ext_ty, methods) in &self.extensions {
            for em in methods {
                if &em.method.name != method {
                    continue;
                }
                if !scope.is_visible(&em.namespace) {
                    continue;
                }
                if !self.can_access(em.method.vis, &em.container, ctx) {
                    continue;
                }
                if !self.can_access_type(&em.container, ctx) {
                    continue;
                }
                // 显式 type_args：先按泛型参数个数过滤（C# generic arity 匹配）。
                // `AddTransient<TService,TImpl>`（2 泛型）与
                // `AddTransient<TService>(Func<...>)`（1 泛型）由 type_args 个数区分。
                if !type_args.is_empty() && em.method.generics.len() != type_args.len() {
                    continue;
                }
                // 决策 #7（RFC 010）：泛型扩展方法接收者类型推断。
                // 若 ext_ty 是方法声明的泛型参数（如 `static T Id<T>(this T x)` 中的 T），
                // 则将 T 绑定到具体接收者类型 ty，实例化方法签名后加入候选。
                let is_generic_receiver = em.generic_params.contains(ext_ty);
                if is_generic_receiver {
                    let Some(inferred_map) = self.unify_receiver(ty, ext_ty, &em.generic_params)
                    else {
                        continue;
                    };
                    let inst_sig = self.instantiate_extension_sig(&em.method, &inferred_map);
                    // 泛型扩展的接收者"形式类型"保留为泛型参数名（ext_ty），
                    // 用于 select_best_extension 的特异性比较：泛型参数不构成任何具体子类型关系，
                    // 不会优于具体类型的候选（规则 1/2 仅适用具体类型）。
                    candidates.push((
                        em.container.clone(),
                        inst_sig,
                        em.namespace.clone(),
                        ext_ty.clone(),
                        Some(ty.clone()),
                        em.template_key.clone(),
                        em.mangle_base.clone(),
                    ));
                } else {
                    // 非泛型：常规类型匹配（相等或子类型）
                    if ty != ext_ty && !self.is_subtype(ty, ext_ty) {
                        continue;
                    }
                    candidates.push((
                        em.container.clone(),
                        em.method.clone(),
                        em.namespace.clone(),
                        ext_ty.clone(),
                        None,
                        em.template_key.clone(),
                        em.mangle_base.clone(),
                    ));
                }
            }
        }
        match candidates.len() {
            0 => Ok(None),
            1 => Ok(Some(
                self.make_resolution(candidates.pop().unwrap(), type_args),
            )),
            _ => {
                let best = self.select_best_extension(
                    &candidates,
                    &scope.enclosing,
                    arg_count,
                    ty,
                    method,
                    type_args,
                    arg_type_names,
                )?;
                Ok(Some(self.make_resolution(best, type_args)))
            }
        }
    }

    /// 将候选元组转换为 `ExtensionResolution`，生成 MIR 调用名。
    /// mangle 基底一律用 `mangle_base`（`method_link_name` 产物，含 overload
    /// 参数后缀，无 arity 后缀），与 `instantiate_generic_extension_fn_by_key`
    /// 的 `mangle_generic(template.name, args)` 逐字节一致：
    /// - 显式 type_args（`AddTransient<TService,TImpl>`）：
    ///   `...AddTransient_Greeter_Greeter`；
    /// - 泛型接收者（`inferred_arg` 为 `Some`）：`...Id_int`；
    /// - 非泛型：`Container::Method`。
    ///
    /// `template_key`（含 arity 后缀）仅用于 `extension_fn_templates` HashMap
    /// 查找键消解，不进入符号 mangle。
    fn make_resolution(
        &self,
        cand: (
            Ident,
            OopMethodSig,
            Vec<Ident>,
            Ident,
            Option<Ident>,
            Ident,
            Ident,
        ),
        type_args: &[Ident],
    ) -> ExtensionResolution {
        let (container, sig, _ns, ext_ty, inferred_arg, template_key, mangle_base) = cand;
        let mangle_self = |args: &[TypeId]| mangle_generic(mangle_base.as_str(), args);
        let (call_name, resolved_type_args) = if !type_args.is_empty() {
            let tids: Vec<TypeId> = type_args.iter().map(|t| TypeId::Named(t.clone())).collect();
            (mangle_self(&tids), type_args.to_vec())
        } else if let Some(arg_ty) = &inferred_arg {
            (mangle_self(&[TypeId::Named(arg_ty.clone())]), Vec::new())
        } else {
            // 非泛型：call_name 用 `mangle_base`（`method_link_name` 产物，含
            // overload 参数后缀），与单态化方法体符号逐字节一致。此前硬编码
            // `{container}::{name}` 会丢掉重载后缀（如
            // `LogInformation_ILogger_string_ReadOnlySpan_string`），导致调用点
            // 与定义符号不匹配 → tree-shake 剪掉定义 → LLVM undefined value。
            (mangle_base.as_str().to_string(), Vec::new())
        };
        // 用显式 type_args（或接收者推断的单个泛型实参）实例化 sig 的值参与返回
        // 类型，使 check-time 实参兼容性校验与 codegen 签名一致。此前泛型扩展的
        // sig 保留泛型参数名（如 `AddSingleton<T>(T instance)` 的 `TService`），
        // 在带值实参的调用点（`AddSingleton<ILoggerFactory>(factory)`）会误报
        // `type mismatch: expected TService, found LoggerFactory`。
        let subst_args: Vec<Ident> = if !type_args.is_empty() {
            type_args.to_vec()
        } else if let Some(ai) = &inferred_arg {
            vec![ai.clone()]
        } else {
            Vec::new()
        };
        let sig = if subst_args.is_empty() {
            sig
        } else {
            let mut s = sig.clone();
            s.params = s
                .params
                .iter()
                .map(|p| ParamSig {
                    ty: Self::substitute_generic_tokens(&p.ty, &s.generics, &subst_args).into(),
                    ..p.clone()
                })
                .collect();
            s.ret = Self::substitute_generic_tokens(&s.ret, &s.generics, &subst_args).into();
            s
        };
        ExtensionResolution {
            container,
            call_name,
            sig,
            this_ty: ext_ty,
            inferred_arg,
            type_args: resolved_type_args,
            template_key,
            mangle_base,
        }
    }

    /// 选择最佳扩展方法候选（C# 优先级规则简化版）。
    ///
    /// 规则 1：同命名空间（与调用点 enclosing）优先；若同命名空间内有候选则只在其中筛选。
    /// 规则 2：更具体的接收者类型优先（子类优先于父类）。
    /// 规则 2.5：实参个数匹配优先——存在 `params.len() == arg_count` 的候选时，
    ///   仅保留严格匹配者（C# 先筛 applicable 再比 specificity；`AddTransient<T,TImpl>`
    ///   与 `AddTransient<T>(Func<...>)` 即按 0/1 实参区分）。
    /// 规则 3：若仍并列，报 `AmbiguousExtensionCall`。
    ///
    /// 注：Arc 无隐式 using、无类内扩展，规则 3/4 不适用。
    fn select_best_extension(
        &self,
        candidates: &[(
            Ident,
            OopMethodSig,
            Vec<Ident>,
            Ident,
            Option<Ident>,
            Ident,
            Ident,
        )],
        enclosing_ns: &[Ident],
        arg_count: usize,
        receiver_ty: &Ident,
        method: &Ident,
        type_args: &[Ident],
        arg_type_names: &[Ident],
    ) -> Result<
        (
            Ident,
            OopMethodSig,
            Vec<Ident>,
            Ident,
            Option<Ident>,
            Ident,
            Ident,
        ),
        OopError,
    > {
        // 规则 1：同命名空间优先。若存在与调用点 enclosing 相同命名空间的候选，则仅在其中筛选。
        let same_ns: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, (_, _, ns, _, _, _, _))| ns.as_slice() == enclosing_ns)
            .map(|(i, _)| i)
            .collect();
        let mut pool: Vec<usize> = if !same_ns.is_empty() {
            same_ns
        } else {
            (0..candidates.len()).collect()
        };

        // 规则 2.5：实参个数匹配优先。候选的 `params` 已去掉接收者（注册时
        // `ext_sig.params.remove(0)`），与调用点值实参个数直接对齐。
        // 尾随 `params` 槽（`is_params`，RFC 005）接受可变个数：仅要求
        // `arg_count >= 固定参数个数`；非 params 候选要求严格相等。
        // 例如 `LogTrace(string, params ReadOnlySpan<string>)`（1 固定）与
        // `LogTrace(EventId, string, params ...)`（2 固定）在 1 值实参时仅前者匹配。
        let arity_matched: Vec<usize> = pool
            .iter()
            .copied()
            .filter(|&i| {
                let p = &candidates[i].1.params;
                if p.last().is_some_and(|s| s.is_params) {
                    arg_count >= p.len() - 1
                } else {
                    p.len() == arg_count
                }
            })
            .collect();
        if !arity_matched.is_empty() && arity_matched.len() != pool.len() {
            pool = arity_matched;
        }

        // 规则 2：在 pool 中选最具体的接收者类型。
        // 候选 A 比 B 更具体 ⇔ A.ext_ty ≠ B.ext_ty ∧ is_subtype(A.ext_ty, B.ext_ty)。
        // 一个候选被"支配"当且仅当存在另一个候选严格更具体。
        // 胜出者 = 不被任何其他候选支配的候选集合。
        let mut winners: Vec<usize> = Vec::new();
        for &i in &pool {
            let ext_i = &candidates[i].3;
            let mut dominated = false;
            for &j in &pool {
                if i == j {
                    continue;
                }
                let ext_j = &candidates[j].3;
                if ext_j != ext_i && self.is_subtype(ext_j, ext_i) {
                    // j 严格更具体，i 被支配
                    dominated = true;
                    break;
                }
            }
            if !dominated {
                winners.push(i);
            }
        }

        if winners.len() == 1 {
            Ok(candidates[winners[0]].clone())
        } else {
            // 规则 2.8：值参数类型消歧。接收者特异性并列（如 `AddSingleton<T>(T instance)`
            // 与 `AddSingleton<T>(Func<IServiceProvider,T> factory)`）时，按实参类型与
            // 值参数类型的匹配度筛选。仅当有显式实参类型名（非空）且恰好一个候选
            // 的值参数全部兼容实参时才采用，否则保持并列走规则 3 报歧义。
            if !arg_type_names.is_empty() {
                let mut compat_winners: Vec<usize> = winners
                    .iter()
                    .copied()
                    .filter(|&i| {
                        self.extension_value_params_compatible(
                            &candidates[i],
                            type_args,
                            arg_type_names,
                        )
                    })
                    .collect();
                // C# better-function-member：全具体值形参的候选优先于含泛型
                // 通配形参的候选（`Contribute(string, string)` 优于
                // `Contribute<T>(string, T)`——同签名兼容时非泛型/更具体者胜）。
                if compat_winners.len() > 1 {
                    let specific: Vec<usize> = compat_winners
                        .iter()
                        .copied()
                        .filter(|&i| {
                            self.extension_value_params_specific(
                                &candidates[i],
                                type_args,
                                arg_type_names,
                            )
                        })
                        .collect();
                    if !specific.is_empty() && specific.len() < compat_winners.len() {
                        compat_winners = specific;
                    }
                }
                if !compat_winners.is_empty() && compat_winners.len() != winners.len() {
                    if compat_winners.len() == 1 {
                        return Ok(candidates[compat_winners[0]].clone());
                    }
                    // 多个兼容候选仍并列 → 收敛 pool 后重跑接收者特异性比较。
                    let narrowed: Vec<usize> = compat_winners;
                    let mut narrow_winners: Vec<usize> = Vec::new();
                    for &i in &narrowed {
                        let ext_i = &candidates[i].3;
                        let mut dominated = false;
                        for &j in &narrowed {
                            if i == j {
                                continue;
                            }
                            let ext_j = &candidates[j].3;
                            if ext_j != ext_i && self.is_subtype(ext_j, ext_i) {
                                dominated = true;
                                break;
                            }
                        }
                        if !dominated {
                            narrow_winners.push(i);
                        }
                    }
                    if narrow_winners.len() == 1 {
                        return Ok(candidates[narrow_winners[0]].clone());
                    }
                }
            }
            // 规则 3：并列 → 报歧义错误
            let candidates_str = winners
                .iter()
                .map(|&i| {
                    let (c, _, ns, ext_ty, _, _, _) = &candidates[i];
                    format!(
                        "{}::{} (on {}, container {})",
                        ns.join("."),
                        method,
                        ext_ty,
                        c
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            Err(OopError::AmbiguousExtensionCall {
                method: method.to_string(),
                receiver: receiver_ty.to_string(),
                candidates: candidates_str,
            })
        }
    }

    /// 判断扩展候选的值参数是否**全部具体**（替换后无裸泛型通配形参）——
    /// better-function-member 消歧的特异性判据（兼容性判据的对偶）。
    fn extension_value_params_specific(
        &self,
        cand: &(
            Ident,
            OopMethodSig,
            Vec<Ident>,
            Ident,
            Option<Ident>,
            Ident,
            Ident,
        ),
        type_args: &[Ident],
        arg_type_names: &[Ident],
    ) -> bool {
        let sig = &cand.1;
        for (k, param) in sig.params.iter().enumerate() {
            let subst = Self::substitute_generic_tokens(&param.ty, &sig.generics, type_args);
            // 仍是裸泛型参数名 → 通配形参 → 不具体。
            if sig.generics.iter().any(|g| g.as_str() == subst) {
                return false;
            }
            let _ = arg_type_names.get(k);
        }
        true
    }

    /// 判断扩展候选的值参数是否全部兼容调用点实参类型名。
    ///
    /// - 形参类型先按显式 `type_args` 做词法级泛型替换（含复合类型名如
    ///   `Func_IServiceProvider_TService` 内的 `TService`）；
    /// - 替换后若形参仍为裸泛型参数名（未提供对应实参类型）→ 视为通配，兼容；
    /// - 否则要求实参类型名等于形参类型名，或是其子类型。
    fn extension_value_params_compatible(
        &self,
        cand: &(
            Ident,
            OopMethodSig,
            Vec<Ident>,
            Ident,
            Option<Ident>,
            Ident,
            Ident,
        ),
        type_args: &[Ident],
        arg_type_names: &[Ident],
    ) -> bool {
        let sig = &cand.1;
        // 尾随 `params` 槽（RFC 005）接受可变个数：仅要求 `arg_type_names.len() >= 固定个数`；
        // 非 params 候选要求严格相等。params 槽逐元素类型校验交由 check_call_bind 的
        // span 打包处理，此处仅对固定参数做消歧。
        let is_params = sig.params.last().is_some_and(|p| p.is_params);
        let fixed = if is_params {
            sig.params.len() - 1
        } else {
            sig.params.len()
        };
        if (is_params && arg_type_names.len() < fixed)
            || (!is_params && arg_type_names.len() != fixed)
        {
            return false;
        }
        for (k, param) in sig.params.iter().enumerate() {
            if is_params && k >= fixed {
                // params 槽：接受任意剩余实参，不再逐元素比对。
                break;
            }
            let subst = Self::substitute_generic_tokens(&param.ty, &sig.generics, type_args);
            // 仍是裸泛型参数名（无可替换实参）→ 通配，视为兼容。
            if sig.generics.iter().any(|g| g.as_str() == subst) {
                continue;
            }
            let arg = arg_type_names[k].as_str();
            if subst == arg {
                continue;
            }
            if self.is_subtype(&arg.into(), &subst.into()) {
                continue;
            }
            return false;
        }
        true
    }

    /// 对类型名做词法级泛型替换：把 `name` 中出现（词法边界分隔）的泛型参数名
    /// 替换为对应的显式 `type_args`。复合类型名（如 `Func_IServiceProvider_TService`）
    /// 中嵌套的泛型参数也因此可替换。
    fn substitute_generic_tokens(name: &str, generics: &[Ident], type_args: &[Ident]) -> String {
        let mut out = name.to_string();
        for (i, g) in generics.iter().enumerate() {
            let Some(ta) = type_args.get(i) else {
                continue;
            };
            let gs = g.as_str();
            let tas = ta.as_str();
            if gs.is_empty() || !name.contains(gs) {
                continue;
            }
            let bytes = out.as_bytes();
            let mut new = Vec::with_capacity(bytes.len());
            let mut idx = 0;
            while idx < bytes.len() {
                if bytes[idx..].starts_with(gs.as_bytes())
                    && (idx == 0 || !is_ident_byte(bytes[idx - 1]))
                    && (idx + gs.len() >= bytes.len() || !is_ident_byte(bytes[idx + gs.len()]))
                {
                    new.extend_from_slice(tas.as_bytes());
                    idx += gs.len();
                } else {
                    new.push(bytes[idx]);
                    idx += 1;
                }
            }
            out = String::from_utf8(new).unwrap_or(out);
        }
        out
    }

    /// 接收者类型与形式参数类型合一（决策 #7：泛型扩展方法支持）。
    ///
    /// - 若 `formal_ty` 是泛型参数：将其绑定到 `receiver_ty`，返回 `Some({formal_ty → receiver_ty})`。
    /// - 若 `formal_ty` 不是泛型参数：要求 `receiver_ty == formal_ty`（或子类型），返回空映射或 `None`。
    pub fn unify_receiver(
        &self,
        receiver_ty: &Ident,
        formal_ty: &Ident,
        generic_params: &[Ident],
    ) -> Option<HashMap<Ident, Ident>> {
        if generic_params.contains(formal_ty) {
            // 泛型参数：绑定到接收者类型
            let mut map = HashMap::new();
            map.insert(formal_ty.clone(), receiver_ty.clone());
            Some(map)
        } else {
            // 非泛型参数：直接比较类型名（接受子类型）
            if receiver_ty == formal_ty || self.is_subtype(receiver_ty, formal_ty) {
                Some(HashMap::new())
            } else {
                None
            }
        }
    }

    /// 用推断出的类型映射实例化扩展方法签名（替换泛型参数为具体类型）。
    pub fn instantiate_extension_sig(
        &self,
        sig: &OopMethodSig,
        map: &HashMap<Ident, Ident>,
    ) -> OopMethodSig {
        let subst = |ty: &Ident| -> Ident {
            if let Some(concrete) = map.get(ty) {
                concrete.clone()
            } else {
                ty.clone()
            }
        };
        OopMethodSig {
            name: sig.name.clone(),
            vis: sig.vis,
            params: sig
                .params
                .iter()
                .map(|p| ParamSig {
                    name: p.name.clone(),
                    ty: subst(&p.ty),
                    is_ref: p.is_ref,
                    is_out: p.is_out,
                    is_in: p.is_in,
                    is_params: p.is_params,
                    default: None,
                })
                .collect(),
            ret: subst(&sig.ret),
            modifier: sig.modifier,
            is_async: sig.is_async,
            // 实例化后方法不再带泛型参数
            generics: vec![],
            is_static_abstract: sig.is_static_abstract,
        }
    }

    pub fn implements_interface(&self, class: &Ident, iface: &Ident) -> bool {
        if !self.is_interface(iface) {
            return false;
        }
        // Must explicitly declare the interface in `bases`.
        // Structural matching (check_interface_impl) is insufficient:
        // a class like CancellationTokenSource has Dispose() but doesn't
        // declare `: IDisposable`, so no itable is emitted.
        // RFC 006：record struct 同样走 bases 显式声明（IEquatable/IHashable）。
        let class_ty = match self.types.get(class) {
            Some(ty) => ty,
            None => return false,
        };
        if !matches!(class_ty.kind, TypeKind::Class | TypeKind::Struct) {
            return false;
        }
        if !class_ty.bases.contains(iface) {
            return false;
        }
        self.check_interface_impl(class, iface).is_ok()
    }

    /// Resolve the class whose itable should back `class` → `iface`.
    ///
    /// Walks the class inheritance chain and returns the **most-derived** type
    /// that satisfies `iface`（CD-11/D2：`TalkDerived : TalkBase(ITalk)` 中
    /// `ITalk it = td` 须引用 `@.itable.TalkDerived_ITalk`，其槽位经 override
    /// 链解析命中派生类实现，而非基类的直接声明 itable）。`is_subtype` 沿
    /// `bases` 链递归，故基类链继承的接口亦命中（与 layout 接口传播一致）。
    pub fn interface_impl_class(&self, class: &Ident, iface: &Ident) -> Option<Ident> {
        if !self.is_class(class) || !self.is_interface(iface) {
            return None;
        }
        let mut cur = class.clone();
        loop {
            if self.is_subtype(&cur, iface) {
                return Some(cur);
            }
            let ty = self.types.get(&cur)?;
            let parent = ty.bases.iter().find(|b| self.is_class(b))?;
            cur = parent.clone();
        }
    }

    /// Interface name to use in `@.itable.{class}_{iface}` for a class that
    /// satisfies `target_iface` (possibly via variance).
    ///
    /// - Exact / AST interface inheritance (`IChild : IBase`) → `target_iface`
    ///   （须有 `@.itable.{class}_{IBase}`，见 layout 传递闭包）
    /// - Variance mono（`IGetter_Dog` → `IGetter_IAnimal`）→ **目标** itable
    ///   （`@.itable.{class}_{IGetter_IAnimal}`，槽位为适配器 thunk）
    pub fn interface_itable_name(&self, impl_class: &Ident, target_iface: &Ident) -> Ident {
        if self.implements_interface(impl_class, target_iface) {
            return target_iface.clone();
        }
        if let Some(ty) = self.types.get(impl_class) {
            for base in &ty.bases {
                if !self.is_interface(base) {
                    continue;
                }
                if base == target_iface {
                    return target_iface.clone();
                }
                // AST 接口继承：使用目标接口 itable（非子接口）
                if self.iface_extends_via_ast(base, target_iface) {
                    return target_iface.clone();
                }
                // Variance：适配器 itable 以目标接口命名（thunk 做返回转型）
                if self.is_subtype(base, target_iface) {
                    return target_iface.clone();
                }
            }
        }
        target_iface.clone()
    }

    /// `iface` 是否经 AST `base_types`（非 variance 合成 `bases`）继承 `ancestor`。
    ///
    /// 用于区分 `IChild : IBase` 与协变单态 `IGetter_Dog`→`IGetter_IAnimal`
    ///（后者只写入 `bases`，`base_types` 为空）。
    pub fn iface_extends_via_ast(&self, iface: &Ident, ancestor: &Ident) -> bool {
        if iface == ancestor {
            return true;
        }
        let Some(ty) = self.types.get(iface) else {
            return false;
        };
        for bt in &ty.base_types {
            let Some(parent) = super::type_path_name(bt) else {
                continue;
            };
            if !self.is_interface(&parent) {
                continue;
            }
            if self.iface_extends_via_ast(&parent, ancestor) {
                return true;
            }
        }
        false
    }

    /// 收集接口经 AST 声明的传递父接口（不含自身）。供 layout / typeinfo 使用。
    pub fn collect_ast_iface_ancestors(&self, iface: &Ident) -> Vec<Ident> {
        let mut out = Vec::new();
        let mut stack = vec![iface.clone()];
        let mut seen = std::collections::HashSet::new();
        seen.insert(iface.clone());
        while let Some(cur) = stack.pop() {
            let Some(ty) = self.types.get(&cur) else {
                continue;
            };
            for bt in &ty.base_types {
                let Some(parent) = super::type_path_name(bt) else {
                    continue;
                };
                if !self.is_interface(&parent) || !seen.insert(parent.clone()) {
                    continue;
                }
                out.push(parent.clone());
                stack.push(parent);
            }
        }
        out
    }

    /// RFC 004 M2：返回详细的接口实现错误（用于在调用点构造有意义的诊断）。
    /// 与 `implements_interface` 的差异：返回 `Result` 而非 `bool`，保留
    /// `OopError` 的具体信息（如缺失的方法名、签名不匹配详情）。
    pub fn try_check_interface_impl(&self, class: &Ident, iface: &Ident) -> Result<(), OopError> {
        let Some(ty) = self.types.get(class) else {
            return Err(OopError::UndefinedType(format!(
                "{class} or {iface} is not a registered class/struct/interface"
            )));
        };
        if !matches!(ty.kind, TypeKind::Class | TypeKind::Struct) || !self.is_interface(iface) {
            return Err(OopError::UndefinedType(format!(
                "{class} or {iface} is not a registered class/struct/interface"
            )));
        }
        self.check_interface_impl(class, iface)
    }

    /// Check all classes/structs implement declared interfaces + LSP on overrides.
    pub fn validate_all(&self) -> Result<(), Vec<OopError>> {
        let mut errors = Vec::new();
        for (name, ty) in &self.types {
            if !matches!(ty.kind, TypeKind::Class | TypeKind::Struct) {
                continue;
            }
            if ty.kind == TypeKind::Class {
                let class_bases: Vec<_> = ty.bases.iter().filter(|b| self.is_class(b)).collect();
                if class_bases.len() > 1 {
                    errors.push(OopError::MultipleInheritance(name.to_string()));
                }
            }
            // Interface satisfaction (skip generic interface templates —
            // those are checked by TypeChecker with proper instantiation).
            // 含 AST 接口继承父接口（`IChild : IBase` → 亦校验 IBase 成员）。
            for base in &ty.bases {
                if self.is_interface(base) && !self.is_generic_template(base) {
                    if let Err(e) = self.check_interface_impl(name, base) {
                        errors.push(e);
                    }
                    for ancestor in self.collect_ast_iface_ancestors(base) {
                        if self.is_generic_template(&ancestor) {
                            continue;
                        }
                        if let Err(e) = self.check_interface_impl(name, &ancestor) {
                            errors.push(e);
                        }
                    }
                }
            }
            // LSP on class base overrides（struct 无 class 继承）
            if ty.kind == TypeKind::Class {
                for base in &ty.bases {
                    if self.is_class(base) {
                        errors.extend(self.check_lsp_overrides(name, base));
                    }
                }
            }
            // CD-18/G2：具体类（非 abstract）必须实现继承链上全部抽象方法（C# CS0534）。
            // 仅检查「自身声明」会漏掉沿 bases 链继承的抽象方法（如 `BadImpl : AbsBase`
            // 未 override `Compute` 曾静默通过）。规则：
            //   1. 非抽象类不得声明 `abstract`/`override abstract` 方法；
            //   2. 非抽象类必须为链上每个抽象方法提供非抽象的匹配实现；
            //   3. 抽象类可保留未实现的抽象方法（继续抽象）；
            //   4. 接口抽象方法由 itable 覆盖检查（`check_interface_impl`）负责，不在此重复。
            // 实现判定：链中「声明点之下」（更派生侧）存在 `find_method_sig` 命中的
            // 非抽象（Override/Virtual/普通）同签名方法即视为已实现；`override abstract`
            // 是更下层的抽象再声明，接管该抽象要求，避免重复报错。抽象属性 `get_X`
            // 另可由同类型 public 字段满足（自动属性 override，与接口 `is_satisfied_by_public_field`
            // 语义一致——如 `RuntimePropertyInfo.PropertyType` 由 codegen 拦截 handle 读取）。
            if !ty.is_abstract {
                let mut chain: Vec<Ident> = Vec::new();
                let mut cur = name.clone();
                loop {
                    chain.push(cur.clone());
                    let Some(nom) = self.types.get(&cur) else {
                        break;
                    };
                    let Some(next) = nom.bases.iter().find(|b| self.is_class(b)).cloned() else {
                        break;
                    };
                    cur = next;
                }
                for i in 0..chain.len() {
                    let Some(decl_ty) = self.types.get(&chain[i]) else {
                        continue;
                    };
                    for asig in iter_method_sigs(&decl_ty.methods) {
                        if !matches!(
                            asig.modifier,
                            MethodModifier::Abstract | MethodModifier::OverrideAbstract
                        ) {
                            continue;
                        }
                        let superseded = chain[..i].iter().any(|cn| {
                            self.types.get(cn).is_some_and(|c_ty| {
                                find_method_sig(&c_ty.methods, &asig.name, asig).is_some_and(
                                    |csig| {
                                        matches!(
                                            csig.modifier,
                                            MethodModifier::Abstract
                                                | MethodModifier::OverrideAbstract
                                        )
                                    },
                                )
                            })
                        });
                        if superseded {
                            continue;
                        }
                        let implemented = chain[..i].iter().any(|cn| {
                            self.types.get(cn).is_some_and(|c_ty| {
                                find_method_sig(&c_ty.methods, &asig.name, asig).is_some_and(
                                    |csig| {
                                        !matches!(
                                            csig.modifier,
                                            MethodModifier::Abstract
                                                | MethodModifier::OverrideAbstract
                                        )
                                    },
                                ) || is_satisfied_by_public_field(&asig.name, asig, c_ty)
                            })
                        });
                        if !implemented {
                            errors.push(OopError::AbstractInConcreteClass {
                                class: name.to_string(),
                                method: asig.name.to_string(),
                            });
                        }
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Interface satisfaction: class must expose all interface methods (name + signature compatible).
    ///
    /// RFC 004 M1：`static abstract` 接口成员跳过实例校验——这些成员由
    /// 实现类提供 `public static` 方法，不属于实例方法表。基元类型
    /// （int/double 等）的 `static abstract` 实现由编译器内置拦截器
    /// 直接发射，无需在 registry 中注册。
    ///
    /// RFC 004 M2：用户自定义类型（class/struct）必须为接口的每个
    /// `static abstract` 方法提供匹配的 `public static` 实现。基元类型
    /// 不在 registry 中注册（约束校验走 `satisfies_constraint` 的
    /// `is_builtin_primitive` 路径），因此此处只需校验用户类型。
    /// `static abstract` 属性的校验需要访问 AST `interface_templates`，
    /// 由 `TypeChecker` 单独处理。
    pub fn check_interface_impl(&self, class: &Ident, iface: &Ident) -> Result<(), OopError> {
        let class_ty = self
            .types
            .get(class)
            .ok_or_else(|| OopError::UndefinedType(class.to_string()))?;
        let iface_ty = self
            .types
            .get(iface)
            .ok_or_else(|| OopError::UndefinedType(iface.to_string()))?;

        for isig in iter_method_sigs(&iface_ty.methods) {
            if isig.is_static_abstract {
                // RFC 004 M2：用户类型必须提供匹配的 public static 方法。
                // 基元类型不在 registry 中注册，不会走到此路径。
                let Some(csig) = find_static_method_sig(&class_ty.methods, &isig.name, isig) else {
                    return Err(OopError::MissingInterfaceMethod {
                        class: class.to_string(),
                        iface: iface.to_string(),
                        method: isig.name.to_string(),
                    });
                };
                if let Err(detail) = signatures_compatible(isig, csig) {
                    return Err(OopError::LspViolation {
                        class: class.to_string(),
                        method: isig.name.to_string(),
                        base: iface.to_string(),
                        detail,
                    });
                }
                continue;
            }
            let csig = find_method_sig(&class_ty.methods, &isig.name, isig);
            if csig.is_none() {
                // 回退：`get_X` 接口方法可由类 public 字段 `X` 满足
                // （如 `interface IShape { string Name { get; } }` 注册为
                // `get_Name` 方法，`class Rectangle : IShape { public string Name; }`
                // 通过 public 字段提供等价访问器语义）。
                if !is_satisfied_by_public_field(&isig.name, isig, class_ty) {
                    return Err(OopError::MissingInterfaceMethod {
                        class: class.to_string(),
                        iface: iface.to_string(),
                        method: isig.name.to_string(),
                    });
                }
                // 字段满足 getter → 跳过签名兼容性检查（字段已通过类型匹配）
                continue;
            }
            let csig = csig.unwrap();
            if let Err(detail) = signatures_compatible(isig, csig) {
                return Err(OopError::LspViolation {
                    class: class.to_string(),
                    method: isig.name.to_string(),
                    base: iface.to_string(),
                    detail,
                });
            }
        }
        for (pname, prop) in &iface_ty.fields {
            let Some(cfield) = class_ty.fields.get(pname) else {
                return Err(OopError::MissingInterfaceProperty {
                    class: class.to_string(),
                    iface: iface.to_string(),
                    property: pname.to_string(),
                });
            };
            if cfield.ty != prop.ty {
                return Err(OopError::LspViolation {
                    class: class.to_string(),
                    method: pname.to_string(),
                    base: iface.to_string(),
                    detail: format!(
                        "property type `{}` does not match interface `{}`",
                        cfield.ty, prop.ty
                    ),
                });
            }
        }
        Ok(())
    }

    /// Liskov Substitution Principle: override signatures must be compatible with base.
    pub fn check_lsp_overrides(&self, class: &Ident, base: &Ident) -> Vec<OopError> {
        let mut errors = Vec::new();
        let Some(class_ty) = self.types.get(class) else {
            return errors;
        };
        let Some(base_ty) = self.types.get(base) else {
            return errors;
        };

        for csig in iter_method_sigs(&class_ty.methods) {
            if let Some(bsig) = find_method_sig(&base_ty.methods, &csig.name, csig) {
                // RFC 006 G1 默认虚 dispatch：派生类声明与基类**同签名**的实例方法
                // 即视为覆写，无需显式 `override` 关键字（基类普通方法亦默认虚分派）。
                // 因此不再强制 `override` 修饰符（显式 `override` 语义与隐式一致），
                // 仅校验签名兼容性（LSP）。
                if let Err(detail) = lsp_compatible(bsig, csig) {
                    errors.push(OopError::LspViolation {
                        class: class.to_string(),
                        method: csig.name.to_string(),
                        base: base.to_string(),
                        detail,
                    });
                }
            } else if matches!(
                csig.modifier,
                MethodModifier::Override | MethodModifier::OverrideAbstract
            ) {
                // CD-10/D1：`override` 必须沿完整基类链按**签名**命中虚/abstract
                // 方法。基类链深于一层时（`C : B : A`，A 声明虚方法、B 未覆写、
                // C 直接覆写），立即基类查不到，须继续上溯。
                let mut found = false;
                let mut cur = base_ty.bases.iter().find(|b| self.is_class(b)).cloned();
                while !found {
                    match cur {
                        Some(cn) => {
                            if let Some(at) = self.types.get(&cn) {
                                if find_method_sig(&at.methods, &csig.name, csig).is_some() {
                                    found = true;
                                }
                                cur = at.bases.iter().find(|b| self.is_class(b)).cloned();
                            } else {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                if !found {
                    errors.push(OopError::NoMatchingOverrideBase {
                        class: class.to_string(),
                        method: csig.name.to_string(),
                    });
                }
            }
        }
        errors
    }
}

/// 检查接口 getter 方法（`get_X`）是否可由类的 public 字段 `X` 满足。
///
/// 接口属性 `T X { get; }` 注册为 `get_X` 方法（零参，返回 T）。
/// 若实现类未提供显式 `get_X` 方法但声明了同名 public 字段，
/// 且字段类型与 getter 返回类型一致，则视为满足接口契约。
/// 此规则对齐 C# CLR 的 trivial property 实现语义。
fn is_satisfied_by_public_field(
    method_name: &Ident,
    iface_sig: &OopMethodSig,
    class_ty: &NominalType,
) -> bool {
    // 仅处理 `get_` 前缀的 getter 方法
    let field_name = match method_name.as_str().strip_prefix("get_") {
        Some(name) => name,
        None => return false,
    };
    // getter 必须无参
    if !iface_sig.params.is_empty() {
        return false;
    }
    let field = match class_ty.fields.get(field_name) {
        Some(f) => f,
        None => return false,
    };
    // 字段必须为 public，且类型与 getter 返回类型一致
    if field.vis != Visibility::Public || field.ty != iface_sig.ret {
        return false;
    }
    true
}
