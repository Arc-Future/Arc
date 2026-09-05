use super::*;
use std::collections::HashMap;

/// RFC 004 P0 Phase 1：可装箱基元名（重载解析 `object` 形参放行）。
/// 仅含拥有运行时 typeinfo 的 8 个基元；struct/enum 留待 Phase 2/4。
fn is_boxable_primitive_name(name: &Ident) -> bool {
    matches!(
        name.as_str(),
        "int" | "long" | "short" | "byte" | "char" | "float" | "double" | "bool"
    )
}

impl TypeRegistry {
    pub fn resolve_field(
        &self,
        ty: &Ident,
        field: &Ident,
        ctx: &AccessContext,
    ) -> Result<Ident, OopError> {
        if !self.can_access_type(ty, ctx) {
            return Err(OopError::InaccessibleType { ty: ty.to_string() });
        }
        let mut current = ty.clone();
        let mut first = true;
        loop {
            // CD-30：入口类型按调用点 namespace 链消歧（`Arc.Drawing` 内
            // `ImageNative` → 本包类型），基类链沿用短名主索引。
            let nom = if first {
                first = false;
                self.lookup_type(&current, &ctx.enclosing_namespace)
            } else {
                self.types.get(&current)
            };
            let Some(nom) = nom else {
                return Err(OopError::UndefinedType(current.to_string()));
            };
            if let Some(f) = nom.fields.get(field) {
                if !self.can_access(f.vis, &current, ctx) {
                    return Err(OopError::InaccessibleMember {
                        ty: ty.to_string(),
                        member: field.to_string(),
                    });
                }
                return Ok(f.ty.clone());
            }
            // Walk base class chain
            let next = nom.bases.iter().find(|b| self.is_class(b)).cloned();
            match next {
                Some(b) => current = b,
                None => {
                    return Err(OopError::UnknownField {
                        ty: ty.to_string(),
                        field: field.to_string(),
                    });
                }
            }
        }
    }

    /// Look up `FieldInfo` for a field on `ty` (walking base class chain).
    pub fn field_info(&self, ty: &Ident, field: &Ident) -> Option<&FieldInfo> {
        let mut current = ty.clone();
        loop {
            let nom = self.types.get(&current)?;
            if let Some(f) = nom.fields.get(field) {
                return Some(f);
            }
            current = nom.bases.iter().find(|b| self.is_class(b)).cloned()?;
        }
    }

    /// Resolve an instance method overload by argument type names (minimal C# rules).
    pub fn resolve_method_overload(
        &self,
        ty: &Ident,
        method: &Ident,
        arg_types: &[Ident],
        ctx: &AccessContext,
    ) -> Result<(Ident, OopMethodSig), OopError> {
        let candidates = self.collect_method_overloads(ty, method, ctx)?;
        let matching: Vec<_> = candidates
            .iter()
            .filter(|(_, sig)| {
                sig.params.len() == arg_types.len()
                    && sig
                        .params
                        .iter()
                        .zip(arg_types.iter())
                        .all(|(param, found)| self.param_assignable(&param.ty, found))
            })
            .collect();
        match matching.len() {
            0 => {
                // 重载 miss 属常规降级路径（调用点回落「首候选」或报无匹配重载），
                // 每次 miss 都打印会洪泛 stderr（MIR 单态化阶段可达数万行、数 MB），
                // 填满 OS 管道缓冲后令 pipe 消费方（如 e2e 测试的 `arc build` 子进程）
                // 阻塞死锁。仅在显式 `ARC_DEBUG_OVL` 下输出（对齐 ARC_DEBUG_* 门控约定）。
                if std::env::var("ARC_DEBUG_OVL").is_ok() {
                    eprintln!(
                        "[OVL] fail ty={} method={} args={:?}",
                        ty, method, arg_types
                    );
                    for (decl, sig) in &candidates {
                        eprintln!(
                            "  cand {}::{}({}): generics={:?}",
                            decl,
                            method,
                            sig.params
                                .iter()
                                .map(|p| p.ty.to_string())
                                .collect::<Vec<_>>()
                                .join(","),
                            sig.generics
                        );
                    }
                }
                Err(OopError::NoMatchingOverload {
                    ty: ty.to_string(),
                    method: method.to_string(),
                })
            }
            1 => Ok((matching[0].0.clone(), matching[0].1.clone())),
            _ => {
                // C# 重载解析：多个候选可分配时，优先「全参数精确匹配」的候选。
                // 例：`sb.Append(0)` 同时匹配 `Append(int)`（精确）与 `Append(long)`/
                // `Append(double)`（隐式加宽）——应选 `Append(int)`，否则误报歧义。
                // 数值加宽在 [RFC 007] 下使字面量实参可命中多个重载；精确匹配
                // 优先于加宽，与 C# 的「最佳匹配」规则一致。仅当无精确候选或
                // 精确候选不唯一时，才判定为真正的歧义。
                let exact: Vec<_> = matching
                    .iter()
                    .filter(|(_, sig)| {
                        sig.params
                            .iter()
                            .zip(arg_types.iter())
                            .all(|(p, f)| p.ty == *f)
                    })
                    .collect();
                if exact.len() == 1 {
                    Ok((exact[0].0.clone(), exact[0].1.clone()))
                } else {
                    Err(OopError::AmbiguousOverload {
                        ty: ty.to_string(),
                        method: method.to_string(),
                    })
                }
            }
        }
    }

    /// First overload with `method` on `ty` (legacy / inference fallback).
    pub fn resolve_method(
        &self,
        ty: &Ident,
        method: &Ident,
        ctx: &AccessContext,
    ) -> Result<OopMethodSig, OopError> {
        self.resolve_method_with_declaring(ty, method, ctx)
            .map(|(_, sig)| sig)
    }

    /// 无显式 type_args 时，从实参 mangled 类型名推断方法级泛型实参。
    ///
    /// 例：`Assert.Empty(xs)` 且 `xs: List_int`、签名 `Empty<T>(List_T)` →
    /// 推断 `T = int`，返回已替换签名 + `type_args = ["int"]`（供 AST 回写与 MIR mono）。
    ///
    /// 仅当恰好一个泛型候选可唯一绑定全部 `generics` 时成功；否则回传
    /// [`OopError::NoMatchingOverload`] / [`OopError::AmbiguousOverload`]。
    pub fn resolve_method_infer_type_args(
        &self,
        ty: &Ident,
        method: &Ident,
        arg_types: &[Ident],
        ctx: &AccessContext,
    ) -> Result<(Ident, OopMethodSig, Vec<Ident>), OopError> {
        let candidates = self.collect_method_overloads(ty, method, ctx)?;
        let mut matching: Vec<(Ident, OopMethodSig, Vec<Ident>)> = Vec::new();
        for (declaring, sig) in &candidates {
            if sig.generics.is_empty() || sig.params.len() != arg_types.len() {
                continue;
            }
            let mut map = HashMap::new();
            let unified = sig
                .params
                .iter()
                .zip(arg_types.iter())
                .all(|(param, found)| {
                    // 泛型占位符参数（如 `DependencyProperty_T`/`Signal_T`）经合一绑定
                    // T；非泛型参数（如 `Element`）须按**继承赋值**检查（`param_assignable`
                    // → `is_subtype`），而非 `unify_generic_ty_name` 的严格相等——否则
                    // `SetBinding(Input child, ...)` 无法匹配 `SetBinding(Element, ...)`
                    //（RFC 037 泛型方法推断：子类实参应可赋给基类形参）。
                    if sig
                        .generics
                        .iter()
                        .any(|g| ty_name_has_generic_segment(&param.ty, g))
                    {
                        unify_generic_ty_name(&param.ty, found.as_str(), &sig.generics, &mut map)
                    } else {
                        self.param_assignable(&param.ty, found)
                    }
                });
            if !unified {
                continue;
            }
            if sig.generics.iter().any(|g| !map.contains_key(g)) {
                continue;
            }
            let type_args: Vec<Ident> = sig
                .generics
                .iter()
                .map(|g| map.get(g).expect("bound above").clone())
                .collect();
            let assignable = sig
                .params
                .iter()
                .zip(arg_types.iter())
                .all(|(param, found)| {
                    // Lambda 常为 `Func_Infer_bool`：合一阶段已跳过 Infer 绑定；
                    // 此处不得再因 `Func_int_bool`↔`Func_Infer_bool` 否决候选。
                    let found_s = found.as_str();
                    if found_s == "Infer" || found_s.split('_').any(|p| p == "Infer") {
                        return true;
                    }
                    let substituted =
                        substitute_generic_in_ty_name(&param.ty, &sig.generics, &type_args);
                    self.param_assignable(&substituted.into(), found)
                });
            if assignable {
                matching.push((declaring.clone(), sig.clone(), type_args));
            }
        }
        match matching.len() {
            0 => Err(OopError::NoMatchingOverload {
                ty: ty.to_string(),
                method: method.to_string(),
            }),
            1 => {
                let (declaring, sig_template, type_args) = matching.remove(0);
                let generics = sig_template.generics.clone();
                let mut sig = sig_template;
                sig.ret = substitute_generic_in_ty_name(&sig.ret, &generics, &type_args).into();
                sig.params = sig
                    .params
                    .iter()
                    .map(|p| {
                        let mut p = p.clone();
                        p.ty = substitute_generic_in_ty_name(&p.ty, &generics, &type_args).into();
                        p
                    })
                    .collect();
                Ok((declaring, sig, type_args))
            }
            _ => Err(OopError::AmbiguousOverload {
                ty: ty.to_string(),
                method: method.to_string(),
            }),
        }
    }

    /// RFC 037 M3：携带显式 type_args 的方法重载解析。
    ///
    /// WPF 同构 DP 模型中，`Element.GetValue<T>(DependencyProperty<T> prop)` 的
    /// 方法签名在 typeck 注册时，param.ty 被 mangle 为 `DependencyProperty_T`
    /// （`T` 是方法泛型参数占位符）。当调用方携带显式 `<string>`/`<double>` 等
    /// type_args 时，需要将占位符 `T` 替换为具体类型实参（如 `string`），才能
    /// 让 `param_assignable` 匹配成功。
    ///
    /// 替换规则：按 `_` 拆分 param.ty 字符串，将每个片段与方法 `generics` 列表
    /// 比对——若匹配则替换为对应的 type_arg。例：
    ///   - param.ty = `DependencyProperty_T`, generics = ["T"], type_args = ["string"]
    ///     → 拆分为 ["DependencyProperty", "T"]
    ///     → "T" 命中 generics[0]，替换为 type_args[0] = "string"
    ///     → 重组为 `DependencyProperty_string`
    ///
    /// 嵌套泛型同样适用：`Func_T_T_bool` + T→int → `Func_int_int_bool`。
    ///
    /// 返回的 `OopMethodSig.ret` 也按同规则替换——`GetValue<T>` 的 ret 为 `T`，
    /// 替换后变为具体类型，供调用方推断方法返回类型。
    pub fn resolve_method_with_type_args(
        &self,
        ty: &Ident,
        method: &Ident,
        arg_types: &[Ident],
        type_args: &[Ident],
        ctx: &AccessContext,
    ) -> Result<(Ident, OopMethodSig), OopError> {
        if type_args.is_empty() {
            return self.resolve_method_overload(ty, method, arg_types, ctx);
        }
        let candidates = self.collect_method_overloads(ty, method, ctx)?;
        let matching: Vec<_> = candidates
            .iter()
            .filter(|(_, sig)| {
                if sig.generics.len() != type_args.len() {
                    return false;
                }
                if sig.params.len() != arg_types.len() {
                    return false;
                }
                sig.params
                    .iter()
                    .zip(arg_types.iter())
                    .all(|(param, found)| {
                        let substituted =
                            substitute_generic_in_ty_name(&param.ty, &sig.generics, type_args);
                        // RFC 006 M4：带 lambda 实参的泛型方法（如
                        // `BindCollection<int>(host, sig, (args) => …)`）。
                        // 合成/推断路径 lambda 实参 mangled 为 `Func_*_Infer`
                        //（params 全 Infer），而形参替换后为 `Action_CollectionChangedEventArgs_int`
                        // ≡ `Func_CollectionChangedEventArgs_int_void`——段数与
                        // `Func_*_Infer` 不等，param_assignable 严格相等与
                        // func_name_infer_compatible 段对齐均失败，导致 filter
                        // 整体失败、回落未替换签名报 `expected _T, found _int`。
                        // 对齐 resolve_method_infer_type_args 的 Infer 放行：Func/
                        // Action 形参收到含 Infer 的 lambda 实参时视为可分配
                        //（真实 lambda arity 由后置 check_func_lambda 校验）。
                        let sub = substituted.as_str();
                        let is_func_param = sub == "Func"
                            || sub.starts_with("Func_")
                            || sub == "Action"
                            || sub.starts_with("Action_");
                        let found_s = found.as_str();
                        let is_infer_lambda = found_s.split('_').any(|p| p == "Infer");
                        if is_func_param && is_infer_lambda {
                            true
                        } else {
                            self.param_assignable(&substituted.clone().into(), found)
                                || func_name_infer_compatible(&substituted, found.as_str())
                        }
                    })
            })
            .collect();
        match matching.len() {
            0 => Err(self.overload_diagnostic(ty, method, &candidates)),
            1 => {
                let (declaring, sig_template) = (matching[0].0.clone(), matching[0].1.clone());
                let generics = sig_template.generics.clone();
                let mut sig = sig_template;
                sig.ret = substitute_generic_in_ty_name(&sig.ret, &generics, type_args).into();
                sig.params = sig
                    .params
                    .iter()
                    .map(|p| {
                        let mut p = p.clone();
                        p.ty = substitute_generic_in_ty_name(&p.ty, &generics, type_args).into();
                        p
                    })
                    .collect();
                Ok((declaring, sig))
            }
            _ => Err(OopError::AmbiguousOverload {
                ty: ty.to_string(),
                method: method.to_string(),
            }),
        }
    }

    /// 静态/实例泛型方法的 typed_fn **模板** link 名（param 类型仍为占位符）。
    ///
    /// `resolve_method_with_type_args` 返回的签名已把 `T`/`List_T` 换成 concrete；
    /// MIR mono 须链接到 `push_typed_fn` 注册的模板名（如 `Assert::Contains_T_List_T`），
    /// 再追加 `__int`。用替换后的 `Contains_int_List_int` 会找不到模板体。
    pub fn method_generic_template_link_name(
        &self,
        ty: &Ident,
        method: &Ident,
        arg_types: &[Ident],
        type_args: &[Ident],
        ctx: &AccessContext,
    ) -> Option<String> {
        if type_args.is_empty() {
            return None;
        }
        let candidates = self.collect_method_overloads(ty, method, ctx).ok()?;
        let matching: Vec<_> = candidates
            .iter()
            .filter(|(_, sig)| {
                if sig.generics.len() != type_args.len() || sig.params.len() != arg_types.len() {
                    return false;
                }
                sig.params
                    .iter()
                    .zip(arg_types.iter())
                    .all(|(param, found)| {
                        let substituted =
                            substitute_generic_in_ty_name(&param.ty, &sig.generics, type_args);
                        // typeck 已完成绑定校验；MIR 侧仅命名所需——未绑定 lambda
                        // 实参（`Func_Infer_*`，替换后 `Func_Greeter` 与其元数
                        // 兼容）亦按 λ 软兼容放行，保证模板唯一命中回填占位符
                        // 基底（否则回退替换后基底 → mono 名分叉 arc-prune-001）。
                        let sub_ident: Ident = substituted.clone().into();
                        self.param_assignable(&sub_ident, found)
                            || func_name_infer_compatible(substituted.as_str(), found.as_str())
                    })
            })
            .collect();
        match matching.as_slice() {
            [(_, template)] => Some(self.method_link_name_for(ty, template)),
            _ => None,
        }
    }

    /// [`method_generic_template_link_name`] 的窄匹配退化版：仅按「泛型参数
    /// 个数 + 值实参个数」在方法表中筛模板，**不做**替换后形参与实参的类型
    /// 比对（调用点可能携带未绑定 lambda——`Func_Infer_*` 与替换后 mangle
    /// 名不严格相等，类型比对恒失配）。唯一命中返回模板的**占位符** link
    /// 基底（`Provide_Func_T`），供 MIR 拼 `__{type_args}` 后缀——避免回退
    /// 到替换后签名基底（`Provide_Func_Greeter`）与 mono body 命名分叉
    ///（模板克隆名 `Provide_Func_T__Greeter` 对不上调用点
    /// `Provide_Func_Greeter__Greeter` → arc-prune-001）。
    ///
    /// 类型比对仍由 typeck 在解析阶段完成（本函数只服务 MIR 目标命名，
    /// 不改变 typeck 已校验的绑定）。
    pub fn method_generic_template_link_name_by_arity(
        &self,
        ty: &Ident,
        method: &Ident,
        arg_count: usize,
        type_arg_count: usize,
        ctx: &AccessContext,
    ) -> Option<String> {
        let candidates = self.collect_method_overloads(ty, method, ctx).ok()?;
        let matching: Vec<_> = candidates
            .iter()
            .filter(|(_, sig)| {
                sig.generics.len() == type_arg_count && sig.params.len() == arg_count
            })
            .map(|(_, sig)| sig)
            .collect();
        match matching.as_slice() {
            [template] => Some(self.method_link_name_for(ty, template)),
            _ => None,
        }
    }

    /// Like [`resolve_method`] but also returns the **declaring class** (walked
    /// up the inheritance hierarchy). Used by MIR lower to mangle method-call
    /// symbols correctly when the method is inherited from a base class (e.g.,
    /// `Window.GetValue<T>` is declared on `Element` → symbol `@Element_GetValue`).
    pub fn resolve_method_with_declaring(
        &self,
        ty: &Ident,
        method: &Ident,
        ctx: &AccessContext,
    ) -> Result<(Ident, OopMethodSig), OopError> {
        self.resolve_method_overload(ty, method, &[], ctx)
            .or_else(|e| match e {
                OopError::NoMatchingOverload { .. } | OopError::AmbiguousOverload { .. } => {
                    let candidates = self.collect_method_overloads(ty, method, ctx)?;
                    Ok(candidates.into_iter().next().unwrap())
                }
                other => Err(other),
            })
    }

    pub(crate) fn collect_method_overloads(
        &self,
        ty: &Ident,
        method: &Ident,
        ctx: &AccessContext,
    ) -> Result<Vec<(Ident, OopMethodSig)>, OopError> {
        if !self.can_access_type(ty, ctx) {
            return Err(OopError::InaccessibleType { ty: ty.to_string() });
        }
        let mut result: Vec<(Ident, OopMethodSig)> = Vec::new();
        let mut current = ty.clone();
        let mut first = true;
        loop {
            // CD-30：入口类型按调用点 namespace 链消歧（`Arc.Drawing` 内
            // `ImageNative.Decode` → 本包 `Arc.Drawing.ImageNative`），基类链
            // 沿用短名主索引。
            let nom = if first {
                first = false;
                self.lookup_type(&current, &ctx.enclosing_namespace)
            } else {
                self.types.get(&current)
            };
            let Some(nom) = nom else {
                // UndefinedType 是正常错误信号（内建类型 string/int 不入 types 表，
                // 见 string_builtin_members_tests；调用方按 Err 分流），不打日志。
                return Err(OopError::UndefinedType(current.to_string()));
            };
            if let Some(sigs) = nom.methods.get(method) {
                for sig in sigs {
                    if !self.can_access(sig.vis, &current, ctx) {
                        continue;
                    }
                    if !result
                        .iter()
                        .any(|(_, existing)| method_params_match(existing, sig))
                    {
                        result.push((current.clone(), sig.clone()));
                    }
                }
            }
            for base in &nom.bases {
                if self.is_interface(base) {
                    // Variance 合成基类（`in`/`out` 适配器视图）只进 `bases`、不进
                    // `base_types`——不得参与重载收集，否则 `IConsumer_IAnimal` 会
                    // 同时看到 `Consume(IAnimal)` 与基类 `Consume(Dog)` 而歧义。
                    if !self.iface_extends_via_ast(&current, base) {
                        continue;
                    }
                    if let Some(sigs) = self.types.get(base).and_then(|i| i.methods.get(method)) {
                        for sig in sigs {
                            if !self.can_access(sig.vis, base, ctx) {
                                continue;
                            }
                            if !result
                                .iter()
                                .any(|(_, existing)| method_params_match(existing, sig))
                            {
                                result.push((base.clone(), sig.clone()));
                            }
                        }
                    }
                }
            }
            let next = nom.bases.iter().find(|b| self.is_class(b)).cloned();
            match next {
                Some(b) => current = b,
                None => break,
            }
        }
        if result.is_empty() {
            return Err(OopError::UnknownMethod {
                ty: ty.to_string(),
                method: method.to_string(),
            });
        }
        Ok(result)
    }

    fn param_assignable(&self, expected: &Ident, found: &Ident) -> bool {
        // null 字面量（类型化 Nullable<Infer>，字段名收敛为 "Infer"）可匹配
        // 任意引用类型参数（重载解析用）。例：`host.CreateSession(null)` 绑定
        // AISessionOptions；`__AIToolHost.Create(null)` 绑定 IServiceProvider。
        // 值类型参数不接受 null。
        if (found.as_str() == "Nullable_Infer" || found.as_str() == "Infer")
            && !self.is_value_type_name(expected)
        {
            return true;
        }
        // RFC 016 D2 / RFC 037 M1: object 接受任何引用类型。
        // 值类型（int/bool/struct/enum 等）需要装箱才能赋值给 object。
        // 此快捷路径覆盖 mangled 泛型类（如 Signal_T、List_Func_T_T_bool）
        // 未注册到 registry.types 的场景——这些类型无法通过 is_subtype 找到
        // 基类链，但它们是引用类型，应可赋值给 object。
        // RFC 004 P0 Phase 1：基元（int/long/short/byte/char/float/double/bool）
        // → object 装箱亦放行；Phase 2：struct → object 装箱亦放行；enum 留待 Phase 4。
        if expected == "object"
            && (!self.is_value_type_name(found)
                || is_boxable_primitive_name(found)
                || self.is_struct(found))
        {
            return true;
        }
        // RFC 007: 数值类型隐式加宽——重载解析中方法实参与变量赋值语义一致。
        // 例：`List<long>.Add(0)` 的整数字面量实参 `int` 可加宽为形参 `long`。
        // 与 check_type::numeric_implicit_convertible 共用同一数值集合，避免
        // 赋值与方法参数出现不一致的双轨行为。
        if numeric_widen_assignable(expected.as_str(), found.as_str()) {
            return true;
        }
        self.is_subtype(found, expected)
    }

    /// 无目标类型 lambda（`Func_*_Infer`）的重载软匹配：仅当**唯一**候选兼容时成功。
    ///
    /// 不可并入 [`param_assignable`]：否则 `Run(Action)` / `Run<T>(Func<T>)` 会因
    /// `Func_Infer` 同时匹配两者而歧义。也不回退「首个同名签名」（会把三参
    /// `Assert.Throws` 错绑到两参，报 expected 2 / found 3）。
    pub fn resolve_method_overload_lambda_soft(
        &self,
        ty: &Ident,
        method: &Ident,
        arg_types: &[Ident],
        ctx: &AccessContext,
    ) -> Result<(Ident, OopMethodSig), OopError> {
        let candidates = self.collect_method_overloads(ty, method, ctx)?;
        let matching: Vec<_> = candidates
            .iter()
            .filter(|(_, sig)| {
                sig.params.len() == arg_types.len()
                    && sig
                        .params
                        .iter()
                        .zip(arg_types.iter())
                        .all(|(param, found)| {
                            self.param_assignable(&param.ty, found)
                                || func_name_infer_compatible(param.ty.as_str(), found.as_str())
                        })
            })
            .collect();
        match matching.len() {
            1 => Ok((matching[0].0.clone(), matching[0].1.clone())),
            0 => Err(OopError::NoMatchingOverload {
                ty: ty.to_string(),
                method: method.to_string(),
            }),
            _ => Err(OopError::AmbiguousOverload {
                ty: ty.to_string(),
                method: method.to_string(),
            }),
        }
    }

    pub fn method_overload_count(&self, declaring_type: &Ident, method: &Ident) -> usize {
        self.nominal_type(declaring_type)
            .and_then(|t| t.methods.get(method))
            .map(|s| s.len())
            .unwrap_or(1)
    }

    /// 同名方法在 static / instance 各自集合内的重载数（0 = 无）。
    pub fn method_overload_count_kind(
        &self,
        declaring_type: &Ident,
        method: &Ident,
        is_static: bool,
    ) -> usize {
        self.nominal_type(declaring_type)
            .and_then(|t| t.methods.get(method))
            .map(|sigs| {
                sigs.iter()
                    .filter(|s| (s.modifier == MethodModifier::Static) == is_static)
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn method_link_name_for(&self, declaring_type: &Ident, sig: &OopMethodSig) -> String {
        let static_count = self.method_overload_count_kind(declaring_type, &sig.name, true);
        let instance_count = self.method_overload_count_kind(declaring_type, &sig.name, false);
        if static_count > 0 && instance_count > 0 {
            method_link_name_static_abi(declaring_type.as_str(), sig, static_count, instance_count)
        } else {
            method_link_name(
                declaring_type.as_str(),
                sig,
                self.method_overload_count(declaring_type, &sig.name),
            )
        }
    }

    /// C# access rules（RFC 025 M2：`internal` 为包级可见；RFC 025 M2+：InternalsVisibleTo）。
    ///
    /// - `file_packages` 为空或调用方/声明方缺包身份 → 单模块 MVP（`internal` 可见）
    /// - 双方均有包名 → 仅同包可访问 `internal`；**声明方包的 `internals_visible_to`
    ///   列表命中调用方包名**时，跨包放行（对标 C# `[assembly: InternalsVisibleTo]`，
    ///   测试程序集 / 指定包可访问 internal）
    pub fn can_access(
        &self,
        member_vis: Visibility,
        declaring_type: &Ident,
        ctx: &AccessContext,
    ) -> bool {
        match member_vis {
            Visibility::Public => true,
            Visibility::Internal => match (
                ctx.current_package.as_deref(),
                // CD-30：声明方类型按调用点 namespace 链消歧后取包——短名
                // `ImageNative` 在 `Arc.Drawing` 包内指依赖包 internal 类
                // （shadowed_types FQN），其声明包是 std 包而非入口包 stub。
                self.lookup_type(declaring_type, &ctx.enclosing_namespace)
                    .and_then(|nom| self.file_packages.get(&nom.span.file_id))
                    .map(|s| s.as_str()),
            ) {
                (Some(caller), Some(decl)) => {
                    caller == decl
                        || self
                            .internals_visible_to
                            .get(decl)
                            .is_some_and(|list| list.iter().any(|p| p == caller))
                }
                _ => true,
            },
            Visibility::Private => ctx.current_type.as_ref().is_some_and(|t| {
                t == declaring_type
                    || self
                        .synth_hosts
                        .get(t)
                        .is_some_and(|host| host == declaring_type)
            }),
            Visibility::Protected => ctx.current_type.as_ref().is_some_and(|from| {
                from == declaring_type || self.is_subtype(from, declaring_type)
            }),
        }
    }

    /// RFC 025：类型级可见性（`internal class` / 顶层默认 `internal` 跨包不可见、同包可见）。
    ///
    /// 与成员 `internal` 共用包身份规则；仅 `Visibility::Internal` 启用门禁。
    /// 显式 `private`/`protected` 顶层类型暂保持命名可见（嵌套私有类型语义未单列）。
    pub fn can_access_type(&self, type_name: &Ident, ctx: &AccessContext) -> bool {
        if ctx.skip_type_visibility {
            return true;
        }
        // CD-30：按调用点 namespace 链消歧——`Arc.Drawing` 包内引用
        // `ImageNative` 判定的是本包 internal 类的可见性（同包可见），
        // 而非被入口包遮蔽后的全局 stub 类。
        let Some(nom) = self.lookup_type(type_name, &ctx.enclosing_namespace) else {
            return true;
        };
        match nom.vis {
            Visibility::Public => true,
            Visibility::Internal => self.can_access(Visibility::Internal, type_name, ctx),
            Visibility::Private | Visibility::Protected => true,
        }
    }

    /// RFC 025 M2：类型声明文件所属包名。
    pub fn package_of(&self, type_name: &Ident) -> Option<&str> {
        let nom = self.types.get(type_name)?;
        self.file_packages
            .get(&nom.span.file_id)
            .map(|s| s.as_str())
    }

    /// 0 匹配重载时的诊断：区分「真·无匹配」与「跨包泛型模板缺失」（RFC 038 M2-G3b）。
    ///
    /// 跨包泛型方法（非 static 类，M2-G1b 收集边界）经 `.aopkg` 外部符号注册为
    /// `generics` 空、形参保留**泛型参数源名**（`T`/`TService`…）的**退化签名**
    /// ——消费端只拿到签名拿不到方法体模板，无法单态化。检测特征：`generics` 空
    /// 且形参类型在 registry 中不可解析（既非已注册类型也非内建值类型）。
    /// 命中时返回 `MissingGenericTemplate` 针对性诊断，而非误导性的
    /// `NoMatchingOverload`（报错 > 静默推断）。
    fn overload_diagnostic(
        &self,
        ty: &Ident,
        method: &Ident,
        candidates: &[(Ident, OopMethodSig)],
    ) -> OopError {
        let missing_template = candidates.iter().any(|(_, sig)| {
            sig.generics.is_empty()
                && sig
                    .params
                    .iter()
                    .any(|p| !self.types.contains_key::<str>(p.ty.as_str()))
        });
        if missing_template {
            OopError::MissingGenericTemplate {
                ty: ty.to_string(),
                method: method.to_string(),
            }
        } else {
            OopError::NoMatchingOverload {
                ty: ty.to_string(),
                method: method.to_string(),
            }
        }
    }
}

/// 将形式参数 mangled 名与实参 mangled 名合一，绑定方法级泛型参数。
///
/// 与 [`substitute_generic_in_ty_name`] 互逆的窄路径：
/// - 形式名为裸泛型（`T`）→ 绑定到完整实参名；
/// - 等长 `_` 分段且对应段为泛型占位符 → 分段绑定（`List_T` ↔ `List_int`）；
/// - 无泛型占位符 → 要求字符串相等。
///
/// 冲突绑定（同一 `T` 推到不同具体类型）返回 `false`。
fn unify_generic_ty_name(
    formal: &str,
    found: &str,
    generics: &[Ident],
    map: &mut HashMap<Ident, Ident>,
) -> bool {
    // `Func_Infer_bool` 等：Infer 段不绑定、不冲突——由其它实参（如 `List_int`）定 T。
    if found == "Infer" {
        return true;
    }
    if generics.iter().any(|g| g.as_str() == formal) {
        return bind_generic(formal.into(), found.into(), map);
    }
    if !generics
        .iter()
        .any(|g| ty_name_has_generic_segment(formal, g))
    {
        return formal == found;
    }
    let f_parts: Vec<&str> = formal.split('_').collect();
    let a_parts: Vec<&str> = found.split('_').collect();
    if f_parts.len() != a_parts.len() {
        return false;
    }
    for (fp, ap) in f_parts.iter().zip(a_parts.iter()) {
        if *ap == "Infer" {
            continue;
        }
        if let Some(g) = generics.iter().find(|g| g.as_str() == *fp) {
            if !bind_generic(g.clone(), (*ap).into(), map) {
                return false;
            }
        } else if fp != ap {
            return false;
        }
    }
    true
}

fn ty_name_has_generic_segment(ty: &str, g: &Ident) -> bool {
    ty.split('_').any(|part| part == g.as_str())
}

fn bind_generic(g: Ident, concrete: Ident, map: &mut HashMap<Ident, Ident>) -> bool {
    match map.get(&g) {
        Some(existing) => existing == &concrete,
        None => {
            map.insert(g, concrete);
            true
        }
    }
}

/// RFC 037 M3：在 mangled 类型名中替换方法泛型参数占位符。
///
/// typeck 注册方法签名时，泛型参数 `T` 会被 mangle 到类型名中
/// （如 `DependencyProperty<T>` → `DependencyProperty_T`）。当调用方
/// 携带显式 type_args 时，需要将占位符替换为具体类型实参。
///
/// 按 `_` 拆分类型名，逐片段与 `generics` 比对——匹配则替换为对应的
/// `type_args` 元素，最后用 `_` 重新拼接。例：
///   - `DependencyProperty_T`, generics=["T"], type_args=["string"]
///     → ["DependencyProperty", "T"] → ["DependencyProperty", "string"]
///     → `DependencyProperty_string`
///
/// 不含 `_` 的简单类型名（如 `T`/`int`/`Window`）作为单片段处理：
///   - `T`, generics=["T"], type_args=["string"] → `string`
///
/// 方法调用目标名（`Class::Method`）按 `::` 分段后各自替换——否则
/// `Signal_T::Set` 的 `_` 拆分得到 ["Signal", "T::Set"]，`T` 段粘连
/// `::Set` 无法命中泛型占位符，替换静默失败（C2：`Signal<double>`
/// 单态化 Set→handler 参数传递损坏的直接根因）。
pub fn substitute_generic_in_ty_name(ty: &str, generics: &[Ident], type_args: &[Ident]) -> String {
    if generics.is_empty() {
        return ty.to_string();
    }
    // 快速路径：类型名不含任何泛型占位符时直接返回原值。
    if !generics.iter().any(|g| ty.contains(g.as_str())) {
        return ty.to_string();
    }
    if ty.contains("::") {
        return ty
            .split("::")
            .map(|part| substitute_underscore_segments(part, generics, type_args))
            .collect::<Vec<_>>()
            .join("::");
    }
    substitute_underscore_segments(ty, generics, type_args)
}

/// 单个 `::` 段内的 `_` 分段替换（见 `substitute_generic_in_ty_name`）。
fn substitute_underscore_segments(ty: &str, generics: &[Ident], type_args: &[Ident]) -> String {
    let parts: Vec<&str> = ty.split('_').collect();
    let mut result = String::with_capacity(ty.len());
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            result.push('_');
        }
        if let Some(pos) = generics.iter().position(|g| g.as_str() == *part) {
            result.push_str(&type_args[pos]);
        } else {
            result.push_str(part);
        }
    }
    result
}

/// `Func_*_Infer`（无目标类型 lambda）与 `Func_*` / `Action`（≡`Func_void`）按片段兼容。
fn func_name_infer_compatible(expected: &str, found: &str) -> bool {
    if !found.contains("Infer") {
        return false;
    }
    let expected_owned;
    let expected_norm = if expected == "Action" {
        "Func_void"
    } else if let Some(rest) = expected.strip_prefix("Action_") {
        // Action<T,…> ≡ Func<T,…,void>
        expected_owned = format!("Func_{rest}_void");
        expected_owned.as_str()
    } else {
        expected
    };
    let e_func = expected_norm == "Func" || expected_norm.starts_with("Func_");
    let f_func = found == "Func" || found.starts_with("Func_");
    if !e_func || !f_func {
        return false;
    }
    // 结构化元数比较：递归 demangle（嵌套 Func/Action 组自描述）后按**形参个数**
    // 与逐位类型名兼容判定。旧的 `_`-原子数比较在嵌套形参（如
    // `Func_object_Func_object_object_object`，6 原子）与未绑定 lambda
    // （`Func_Infer_Infer_Infer`，4 原子）间恒不等 → 软匹配零候选 →
    // 回退首签名 → arity 错绑（expected 2 / found 3）。
    let demangled_arity = |name: &str| -> Option<usize> {
        crate::check_expr::demangle_func_type_depth(name, None, 0, &|_| false).and_then(|f| match f
        {
            TypeId::Func { params, .. } => Some(params.len()),
            _ => None,
        })
    };
    if let (Some(e_arity), Some(f_arity)) = (demangled_arity(expected_norm), demangled_arity(found))
    {
        if e_arity == f_arity {
            return true;
        }
        // arity=None 回溯按 count 升序取首解：嵌套形参（如
        // `Func_object_Func_object_object_object`）在低 count 处被误切成
        // 「内层 Func 作 ret」的次优解（arity 1 ≠ 实际 2），直接返回 false。
        // 实参 λ 元数 f_arity 已知——以 `Some(f_arity)` 显式重解析期望签名
        // （count = f_arity + 1 的唯一切分），命中即兼容（具体参数类型由选中
        // 候选后的 λ 定向校验把关，此处只需元数对齐即可放行候选）。
        if crate::check_expr::demangle_func_type_depth(expected_norm, Some(f_arity), 0, &|_| false)
            .is_some()
        {
            return true;
        }
        return false;
    }
    if let Some(f_arity) = demangled_arity(found) {
        if crate::check_expr::demangle_func_type_depth(expected_norm, Some(f_arity), 0, &|_| false)
            .is_some()
        {
            return true;
        }
    }
    let e_parts: Vec<&str> = expected_norm.split('_').collect();
    let f_parts: Vec<&str> = found.split('_').collect();
    if e_parts.len() != f_parts.len() {
        return false;
    }
    e_parts
        .iter()
        .zip(f_parts.iter())
        .all(|(e, f)| *e == *f || *e == "Infer" || *f == "Infer")
}

/// 数值类型名集合（对齐 `check_type::numeric_implicit_convertible`，不含 bool）。
fn is_int_name(name: &str) -> bool {
    matches!(
        name,
        "int" | "long" | "short" | "byte" | "char" | "uint" | "ulong" | "ushort" | "sbyte"
    )
}

fn is_float_name(name: &str) -> bool {
    matches!(name, "float" | "double")
}

/// RFC 007: 数值类型隐式加宽（重载解析用）。与 `numeric_implicit_convertible`
/// 语义一致——整数↔整数宽窄均可、整数→浮点安全加宽、浮点↔浮点宽窄均可。
fn numeric_widen_assignable(expected: &str, found: &str) -> bool {
    if is_int_name(expected) && is_int_name(found) {
        return true;
    }
    if is_float_name(expected) && (is_int_name(found) || is_float_name(found)) {
        return true;
    }
    false
}
