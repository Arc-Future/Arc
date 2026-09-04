use super::*;
use crate::field_keyword::uses_field;
use ast::Variance;

impl TypeChecker {
    pub(crate) fn push_type_params(&mut self, params: &[GenericParam]) {
        let mut scope = IndexMap::new();
        for p in params {
            scope.insert(p.name.clone(), TypeId::Generic(p.name.clone()));
        }
        self.type_param_scope.push(scope);
    }

    pub(crate) fn pop_type_params(&mut self) {
        self.type_param_scope.pop();
    }

    pub(crate) fn resolve_type_param(&self, name: &Ident) -> Option<TypeId> {
        self.type_param_scope
            .iter()
            .rev()
            .find_map(|s| s.get(name).cloned())
    }

    /// GAP #5 扩展：泛型委托单态化。
    ///
    /// `delegate R Map<T, R>(T x);` 在引用点 `Map<int, string>` 处按实参
    /// 替换签名 AST（`substitute_type_ast`）并 lowering，产出具体
    /// `TypeId::Func`。委托本就是 Func 别名，无 shell/stub 注册需求。
    /// 环检测复用 `recursion_iface` 栈；where 子句约束在单态化前按实参
    /// 校验（与 `instantiate_generic_class` 同序：arity → constraints →
    /// 替换；环短路在前，避免在途环重复报告）。
    pub(crate) fn instantiate_generic_delegate(
        &mut self,
        def: &Ident,
        args: &[TypeId],
    ) -> Result<TypeId, TypeError> {
        let template = self
            .delegate_templates
            .get(def)
            .ok_or_else(|| TypeError::GenericTypeNeedsArgs(def.to_string()))?
            .clone();
        if template.generics.len() != args.len() {
            return Err(TypeError::GenericArity {
                name: def.to_string(),
                expected: template.generics.len(),
                found: args.len(),
            });
        }
        let mangled = mangle_generic(def, args);
        // 负缓存短路：违约实例化点不进正缓存，重触达会重跑检查导致违约明细
        // 重复入池。命中即返回缓存哨兵，不重跑检查（明细零重复入池），哨兵
        // 仍沿 `?` 冒泡保持「违约实参不得参与单态化」契约。
        if let Some(sentinel) = self.violated.get(mangled.as_str()) {
            return Err(sentinel.clone());
        }
        let _recursion_token = match self.recursion_iface.enter(&mangled) {
            Ok(token) => token,
            Err(cycle) => {
                cycle.report("泛型委托");
                // 在途环：短路返回 mangle 占位名（与泛型接口策略一致）。
                return Ok(TypeId::Named(mangled.into()));
            }
        };
        // 约束校验：模板为整份 clone（非 self 借用），直接传引用；语义与类
        // 路径一致——批量收集全部违约入池（DiagnosticBag），哨兵返回中止
        // 当前实例化，单态化前拦截。违约同时登记负缓存：同实例化点重触达
        // 由入口负缓存短路，不再重跑检查（明细零重复入池）。
        if let Err(e) = self.check_constraints(&template.where_clause, &template.generics, args) {
            self.violated.insert(mangled.clone(), e.clone());
            return Err(e);
        }
        let map: IndexMap<Ident, TypeId> = template
            .generics
            .iter()
            .map(|p| p.name.clone())
            .zip(args.iter())
            .map(|(n, a)| (n, a.clone()))
            .collect();
        let mut params = Vec::with_capacity(template.params.len());
        for p in &template.params {
            let substituted = substitute_type_ast(&p.ty.node, &map);
            params.push(self.lower_type(&substituted)?);
        }
        let ret = match &template.ret {
            Some(ret) => {
                let substituted = substitute_type_ast(&ret.node, &map);
                self.lower_type(&substituted)?
            }
            None => TypeId::Void,
        };
        Ok(TypeId::Func {
            params,
            ret: Box::new(ret),
        })
    }

    pub(crate) fn instantiate_generic_class(
        &mut self,
        def: &Ident,
        args: &[TypeId],
    ) -> Result<TypeId, TypeError> {
        // RFC 038：栈式 in-progress 环检测。mangled 在 enter 前先算出。
        // enter 命中（Err）= 同 mangled 已在栈上（真环）；Ok = 首次进入，令牌持到函数结束。
        let mangled = mangle_generic(def, args);
        // 负缓存短路：违约实例化点不进正缓存，重触达会重跑检查导致违约明细
        // 重复入池。与 delegate/iface/fn 路径同序（mangled → 短路 → guard）。
        if let Some(sentinel) = self.violated.get(mangled.as_str()) {
            return Err(sentinel.clone());
        }
        let _recursion_token = match self.recursion_class.enter(&mangled) {
            Ok(token) => token,
            Err(cycle) => {
                cycle.report("泛型类");
                if args.iter().any(type_contains_generic) {
                    // 泛型占位符 stub 自引用环：stub 由外层注册，直接返回占位名。
                    return Ok(TypeId::Named(mangled.into()));
                }
                // 互相递归泛型类在途环：短路返回占位壳（register_recursive_class_shell
                // 先插空占位截断递归，再完整 lower 签名）。
                if !self.registry.types.contains_key(mangled.as_str()) {
                    self.register_recursive_class_shell(def, mangled.as_str(), args)?;
                }
                return Ok(TypeId::Named(mangled.into()));
            }
        };
        // RFC: 递归检测嵌套在 Func/Array/Task/IEnumerable 等复合类型中的
        // `Generic` 类型参数。仅检测直接 `Generic` 会让 `List<Action<T>>`
        // 错误进入完整实例化路径，导致 `substitute_class_def` 通过
        // `type_id_to_ast` 还原类型时丢失信息，产生非法 LLVM 标识符。
        if args.iter().any(type_contains_generic) {
            // RFC 037 M1.2: 含 Generic 占位符的泛型类实例（如 `Signal<T>`
            // 在 `Element.GetValue<T>` 方法体内）需注册 stub NominalType，
            // 否则 `signal.Value` 等字段访问因 `registry.types` 中无
            // `Signal_T` 条目而失败。stub 仅注册一次，复用模板字段/方法
            // 签名（替换模板类型参数为 args 实参，保留外层 Generic 占位符）。
            //
            // 必须先检查 `instantiated` 集合并标记，再调用 stub 注册——
            // stub 注册内 `register_monomorphized_class` 会 `lower_type` 处理
            // 字段/方法签名，若模板 body 自引用（如 `List<T>.FindAll() : List<T>`
            // / `Signal<T>._changedHandlers : List<Action<T,T>>`）将触发
            // 同一 mangled 类型的递归 `instantiate_generic_class` 调用。
            // 无 `instantiated` 标记会无限递归导致栈溢出。
            if self.instantiated.contains(&mangled) {
                return Ok(TypeId::Named(mangled.into()));
            }
            self.instantiated.insert(mangled.clone());
            // RFC 044 M3：stub 也登记 mono_origins，使 `__Yield_X_0_T` 可回退到
            // 类模板判定接口实现（`register_parametrized_generic_stub` 会清除 stub
            // 的接口 bases 防 itable 虚分派，子类型判定须回退模板 AST bases）。
            self.mono_origins
                .insert(mangled.clone(), (def.clone(), args.to_vec()));
            if !self.registry.types.contains_key(mangled.as_str()) {
                self.register_parametrized_generic_stub(def, mangled.as_str(), args)?;
            }
            return Ok(TypeId::Named(mangled.into()));
        }
        // RFC 038 M2 缺口收口：外部符号泛型类回退。依赖库泛型类（如 std/DI 的
        // `InjectAttribute<T>`）对消费方仅以签名视图（ExternalSymbolEntry）
        // 提供，不在 `class_templates`；消费方把其作为泛型基类
        // （`class X : InjectAttribute<IUserService>`）实例化时无 AST 模板可用。
        // 对齐 `instantiate_generic_interface` 的 registry 回退——按签名注册 mono 壳
        //（跳过 body 检查：外部模板 body 未加载，且其自身已通过 Arc 包 typeck）。
        if !self.class_templates.contains_key(def) {
            if self.registry.is_class(def) && self.registry.is_generic_template(def) {
                return self.register_generic_class_from_registry(def, args);
            }
            return Err(TypeError::GenericTypeNeedsArgs(def.to_string()));
        }
        let (where_clause, generics): (Vec<TypeConstraint>, Vec<GenericParam>) = {
            let template = self
                .class_templates
                .get(def)
                .ok_or_else(|| TypeError::GenericTypeNeedsArgs(def.to_string()))?;
            (template.where_clause.clone(), template.generics.clone())
        };
        if generics.len() != args.len() {
            return Err(TypeError::GenericArity {
                name: def.to_string(),
                expected: generics.len(),
                found: args.len(),
            });
        }
        // 违约登记负缓存：同实例化点重触达由入口短路返回缓存哨兵。
        if let Err(e) = self.check_constraints(&where_clause, &generics, args) {
            self.violated.insert(mangled.clone(), e.clone());
            return Err(e);
        }
        // 已完整注册（memoize 命中）：直接返回。在途环已由顶部 RecursionGuard 拦截。
        if self.instantiated.contains(&mangled) {
            return Ok(TypeId::Named(mangled.into()));
        }
        self.instantiated.insert(mangled.clone());
        self.mono_origins
            .insert(mangled.clone(), (def.clone(), args.to_vec()));
        let map = substitution_map(&generics, args);
        let template = self
            .class_templates
            .get(def)
            .ok_or_else(|| TypeError::GenericTypeNeedsArgs(def.to_string()))?;
        let inst = substitute_class_def(template, &mangled, &map);
        // 恢复模板声明侧的包 / 命名空间上下文：force-instantiate 与跨包调用点
        // 单态化时，current_package 往往仍是消费端（用户 Program）。若不切回，
        // 方法体内访问库内 `internal` 类型成员（如 `DispatchContext.BindJson`）
        // 会被 `resolve_field` 的 `can_access_type` 拒掉，body check 失败 →
        // 扩展方法（`SendAsync`）永不单态化 → LLVM undefined value（RFC 019 M-B /
        // 与 `ensure_type_accessible` 的 mono 豁免同因，但成员查找未走该豁免）。
        let (tmpl_ns, tmpl_span) = self
            .registry
            .types
            .get(def)
            .map(|n| (n.namespace.clone(), n.span))
            .unwrap_or((Vec::new(), Span::DUMMY));
        let prev_pkg = self.current_package.clone();
        let prev_ns = self.enclosing_namespace.clone();
        self.enter_package_for_span(tmpl_span);
        // span 缺失时按模板类型声明包名回退（与 `package_of` 同源）。
        if self.current_package.is_none() {
            if let Some(pkg) = self.registry.package_of(def) {
                self.current_package = Some(pkg.to_string());
            }
        }
        self.enclosing_namespace = tmpl_ns;
        // 单态化期间跳过类型可见性门禁（见 `ensure_type_accessible` 注释）。
        self.mono_depth += 1;
        let result = (|| {
            self.register_monomorphized_class(&inst, &map)?;
            self.check_class_inner(&inst, true)?;
            self.check_generic_interface_impls(&inst)?;
            Ok(())
        })();
        if let Err(e) = &result {
            eprintln!("[DIAG-MONO] full instantiate {mangled} failed: {e:?}");
        }
        self.mono_depth -= 1;
        self.current_package = prev_pkg;
        self.enclosing_namespace = prev_ns;
        result?;
        Ok(TypeId::Named(mangled.into()))
    }

    /// RFC 037 M1.2: 为含 Generic 类型参数的泛型类实例注册 stub NominalType。
    ///
    /// 当 `instantiate_generic_class` 在外层泛型上下文（如 `Element.GetValue<T>`
    /// 方法体中）被调用时，args 含 `Generic("T")` 占位符——无法真正单态化，
    /// 但仍需注册 stub NominalType 使字段/方法解析可成功。
    ///
    /// stub 复用模板的字段/方法/bases 定义，**替换模板自身的类型参数为
    /// args 中的实参**（args 可能含外层 Generic 占位符）。例：
    ///   - `List<Func<T,T,bool>>`（在 `Signal<T>` 体内）mangle 为
    ///     `List_Func_T_T_bool`，stub 应将 List 模板的 `T` 替换为
    ///     `Func<T,T,bool>`（mangled `Func_T_T_bool`），使 `Add` 方法
    ///     参数类型为 `Func_T_T_bool` 而非占位符 `T`——否则
    ///     `_changingHandlers.Add(handler)` 重载解析失败。
    ///
    /// 不调用 `check_class_inner`——避免在泛型上下文中重复类型检查模板 body
    /// （模板检查已在定义处完成）。
    fn register_parametrized_generic_stub(
        &mut self,
        template_name: &Ident,
        mangled: &str,
        args: &[TypeId],
    ) -> Result<(), TypeError> {
        let template = self
            .class_templates
            .get(template_name)
            .ok_or_else(|| TypeError::GenericTypeNeedsArgs(template_name.to_string()))?;
        // 构造替换 map：模板类型参数 → args 实参（args 可能含 Generic 占位符）。
        // 例：List 模板 generics = ["T"]，args = [Named("Func_T_T_bool")]
        //     → map = {"T" => Named("Func_T_T_bool")}
        // substitute_class_def 会在 AST 层面替换 T，使 Add(item: T) → Add(item: Func_T_T_bool)
        let map: IndexMap<Ident, TypeId> = template
            .generics
            .iter()
            .map(|p| p.name.clone())
            .zip(args.iter())
            .map(|(n, a)| (n, a.clone()))
            .collect();
        let inst = substitute_class_def(template, mangled, &map);
        self.register_monomorphized_class(&inst, &map)?;
        // 仅清理 stub 的**接口** bases：stub 仅用于泛型上下文中的类型解析，
        // 不应参与 itable 虚分派（其接口方法不存在实际符号定义）。
        // 保留后 `layouts_from_registry` 不会为此 stub 生成 itable，
        // 从而避免 LLVM 报 "use of undefined value" 错误。
        // 但必须保留 **class** base：继承字段解析（`resolve_field` 沿
        // `bases` 找 `is_class` 基类）与子类型判定（`param_assignable` →
        // `is_subtype`）依赖它——如 `DependencyProperty<T> : DependencyProperty`
        // 的 `Id` 字段访问与 `Register(DependencyProperty)` 重载匹配。class base
        // 不产生 itable，保留无虚分派风险。
        let class_bases: Vec<Ident> = self
            .registry
            .types
            .get(mangled)
            .map(|e| {
                e.bases
                    .iter()
                    .filter(|b| self.registry.is_class(b))
                    .cloned()
                    .collect::<Vec<Ident>>()
            })
            .unwrap_or_default();
        if let Some(entry) = self.registry.types.get_mut(mangled) {
            entry.bases = class_bases;
            entry.base_types.clear();
        }
        Ok(())
    }

    /// 为「互相递归泛型类」重入注册**先占位、后完整签名**的壳。
    ///
    /// 与 `register_parametrized_generic_stub` 同走 `substitute_class_def` +
    /// `register_monomorphized_class` 完整 lower 字段/方法签名，但关键区别是
    /// **先插入空占位条目**：互相递归签名（`AsyncStream<T>.MoveNextCore(
    /// AsyncStreamEnumerator<T>)` ↔ `AsyncStreamEnumerator<T>._stream: AsyncStream<T>`）
    /// 在 lowering 期间会再次触发 `instantiate_generic_class` 重入。若占位条目
    /// 尚未入表，`registry.types.contains_key(mangled)` 恒 false，重入侧会再次
    /// 完整注册 → 无限递归 → 栈溢出。先插空占位使重入侧
    /// `contains_key` 立即为真，截断递归（深至多两层：A 壳 → B 壳 → A 命中占位）。
    ///
    /// 完整签名必须保留（不能仅 class bases / 空成员）：嵌套侧方法体 check
    /// （`AsyncStreamEnumerator.MoveNextAsync` 调 `_stream.MoveNextCore`）发生在
    /// **外层完整注册完成前**，壳若无方法签名会报 `unknown method`。外层完整
    /// 实例化在 `register_monomorphized_class` 末尾整体 `insert` 覆盖本壳
    /// （两者 `inst` 由同一 `substitute_class_def` 产出，内容一致，幂等覆盖）。
    fn register_recursive_class_shell(
        &mut self,
        template_name: &Ident,
        mangled: &str,
        args: &[TypeId],
    ) -> Result<(), TypeError> {
        let template = self
            .class_templates
            .get(template_name)
            .ok_or_else(|| TypeError::GenericTypeNeedsArgs(template_name.to_string()))?;
        // 替换 map：模板类型参数 → args 实参（AST 层替换）。
        let map: IndexMap<Ident, TypeId> = template
            .generics
            .iter()
            .map(|p| p.name.clone())
            .zip(args.iter())
            .map(|(n, a)| (n, a.clone()))
            .collect();
        let inst = substitute_class_def(template, mangled, &map);
        // 从模板取元信息（vis / abstract / record / span / namespace），先插入
        // 空占位条目，使重入侧 `contains_key(mangled)` 立即为真 → 截断互相递归。
        let (tmpl_span, tmpl_ns, vis, is_abstract, is_record) = self
            .registry
            .types
            .get(template_name)
            .map(|n| {
                (
                    n.span,
                    n.namespace.clone(),
                    n.vis,
                    n.is_abstract,
                    n.is_record,
                )
            })
            .unwrap_or((Span::DUMMY, Vec::new(), Visibility::Public, false, false));
        self.registry.types.insert(
            mangled.into(),
            NominalType {
                name: mangled.into(),
                kind: TypeKind::Class,
                vis,
                is_abstract,
                is_record,
                is_readonly: false,
                fields: IndexMap::new(),
                methods: IndexMap::new(),
                bases: vec![],
                base_types: vec![],
                span: tmpl_span,
                variants: vec![],
                generic_params: vec![],
                namespace: tmpl_ns,
                const_values: IndexMap::new(),
                constructors: vec![],
                soa: false,
                required_props: indexmap::IndexSet::new(),
            },
        );
        // 完整 lower 字段/方法签名（占位已在表，重入被截断），整体覆盖占位。
        self.register_monomorphized_class(&inst, &map)?;
        Ok(())
    }

    /// RFC 038 M2 缺口收口：为「仅外部符号签名」的泛型类注册 mono 壳。
    ///
    /// 依赖库泛型类（如 std/DI 的 `InjectAttribute<T>`）对消费方以
    /// 签名视图（`ExternalSymbolEntry`）提供——不在 `class_templates`，且外部
    /// 符号表不携带基类/字段/方法体（`NominalType` 仅 generic_params 有值）。
    /// 消费方把其作为泛型基类（`class X : InjectAttribute<IUserService>`）实例化时，
    /// 无 AST 模板可用 `substitute_class_def`；本函数按 registry 中模板的
    /// 签名（generic_params 位置映射）构建并注册 mono 类型，使基类链行走
    /// （`inherited_field_types` / `collect_method_overloads`）可解析。仅签名、
    /// 不检查 body（外部模板 body 未加载，且其自身已通过 Arc 包 typeck）。
    fn register_generic_class_from_registry(
        &mut self,
        def: &Ident,
        args: &[TypeId],
    ) -> Result<TypeId, TypeError> {
        let mangled = mangle_generic(def, args);
        if self.instantiated.contains(&mangled) {
            return Ok(TypeId::Named(mangled.into()));
        }
        let template = self
            .registry
            .types
            .get(def)
            .ok_or_else(|| TypeError::GenericTypeNeedsArgs(def.to_string()))?;
        let generic_params: Vec<GenericParam> = template
            .generic_params
            .iter()
            .map(|n| GenericParam::new(n.clone()))
            .collect();
        let _template_fields = template.fields.clone();
        let template_methods = template.methods.clone();
        let template_base_types = template.base_types.clone();
        let template_vis = template.vis;
        let template_abstract = template.is_abstract;
        let template_record = template.is_record;
        let template_ns = template.namespace.clone();
        let template_props = self
            .registry
            .declared_properties
            .get(def)
            .cloned()
            .unwrap_or_default();
        if generic_params.len() != args.len() {
            return Err(TypeError::GenericArity {
                name: def.to_string(),
                expected: generic_params.len(),
                found: args.len(),
            });
        }
        self.instantiated.insert(mangled.clone());
        self.mono_origins
            .insert(mangled.clone(), (def.clone(), args.to_vec()));
        let map: IndexMap<Ident, TypeId> = generic_params
            .iter()
            .map(|p| p.name.clone())
            .zip(args.iter())
            .map(|(n, a)| (n, a.clone()))
            .collect();
        // 字段/方法签名按模板类型参数位置代入（外部签名不含 body，仅签名替换）。
        let mut fields = IndexMap::new();
        for (fname, finfo) in &template.fields {
            let new_ty =
                type_id_to_field_name(&substitute_type(&TypeId::Named(finfo.ty.clone()), &map));
            fields.insert(
                fname.clone(),
                FieldInfo {
                    ty: new_ty,
                    ..finfo.clone()
                },
            );
        }
        let mut methods: IndexMap<Ident, Vec<OopMethodSig>> = IndexMap::new();
        for (mname, sigs) in &template_methods {
            let mut new_sigs = Vec::new();
            for sig in sigs {
                let new_params: Vec<ParamSig> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let pty = type_id_to_field_name(&substitute_type(
                            &TypeId::Named(p.ty.clone()),
                            &map,
                        ));
                        ParamSig {
                            ty: pty,
                            ..p.clone()
                        }
                    })
                    .collect();
                let new_ret =
                    type_id_to_field_name(&substitute_type(&TypeId::Named(sig.ret.clone()), &map));
                new_sigs.push(OopMethodSig {
                    params: new_params,
                    ret: new_ret,
                    ..sig.clone()
                });
            }
            methods.insert(mname.clone(), new_sigs);
        }
        // 基类代入（对齐 instantiate_generic_interface：外部签名如携带 base_types
        // 则按类型参数替换后入 bases/base_types，保证基类链继续上行）。
        let mut bases: Vec<Ident> = Vec::new();
        let mut base_types: Vec<Type> = Vec::new();
        for base in &template_base_types {
            let substituted = substitute_type_ast(base, &map);
            if let Some(parent_name) = self.resolve_base_type_name(&substituted) {
                if !bases.contains(&parent_name) {
                    bases.push(parent_name);
                }
            }
            base_types.push(substituted);
        }
        self.registry.types.insert(
            mangled.clone().into(),
            NominalType {
                name: mangled.clone().into(),
                kind: TypeKind::Class,
                vis: template_vis,
                is_abstract: template_abstract,
                is_record: template_record,
                is_readonly: false,
                fields,
                methods,
                bases,
                base_types,
                span: Span::DUMMY,
                variants: vec![],
                generic_params: vec![],
                namespace: template_ns,
                const_values: IndexMap::new(),
                constructors: vec![],
                soa: false,
                required_props: Default::default(),
            },
        );
        // 属性签名随 mono 一并注册（对齐 instantiate_generic_interface 的 CD-32 收口）。
        if !template_props.is_empty() {
            let new_props: Vec<crate::oop_types::DeclaredPropertySig> = template_props
                .iter()
                .map(|p| crate::oop_types::DeclaredPropertySig {
                    name: p.name.clone(),
                    ty: type_id_to_field_name(&substitute_type(&TypeId::Named(p.ty.clone()), &map)),
                    can_read: p.can_read,
                    can_write: p.can_write,
                })
                .collect();
            self.registry
                .declared_properties
                .insert(mangled.clone().into(), new_props);
        }
        Ok(TypeId::Named(mangled.into()))
    }

    /// RFC 019 M-C：为含 Generic 类型参数的泛型接口实例注册 stub NominalType。
    ///
    /// 与 `register_parametrized_generic_stub`（类）对称：泛型方法体内
    /// `IRequestHandler<TRequest, TResponse>` 经 `lower_type` 得到 mangled 名
    /// `IRequestHandler_TRequest_TResponse`，需在 registry 注册该条目
    /// （方法签名把模板类型参数代入 args 实参，保留外层 Generic 占位符），
    /// 否则 `handler.Handle(request)` 等方法调用/转换因 registry 无此类型而
    /// 失败（`OOP: undefined type IRequestHandler_TRequest_TResponse`）。
    ///
    /// 仅注册一次；stub 不产生 itable（仅用于泛型上下文类型解析，具体实参
    /// 单态化时另行完整注册）。
    fn register_parametrized_generic_iface(
        &mut self,
        template_name: &Ident,
        mangled: &str,
        args: &[TypeId],
    ) -> Result<(), TypeError> {
        let iface_def = self.interface_templates.get(template_name).cloned();
        let generic_params: Vec<GenericParam> = match &iface_def {
            Some(d) => d.generics.clone(),
            None => {
                let iface =
                    self.registry.types.get(template_name).ok_or_else(|| {
                        TypeError::GenericTypeNeedsArgs(template_name.to_string())
                    })?;
                iface
                    .generic_params
                    .iter()
                    .map(|n| GenericParam::new(n.clone()))
                    .collect()
            }
        };
        let map: IndexMap<Ident, TypeId> = generic_params
            .iter()
            .map(|p| p.name.clone())
            .zip(args.iter())
            .map(|(n, a)| (n, a.clone()))
            .collect();
        let mut methods: IndexMap<Ident, Vec<OopMethodSig>> = IndexMap::new();
        if let Some(d) = &iface_def {
            for sig in &d.methods {
                let new_params: Vec<ParamSig> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let substituted = substitute_type_ast(&p.ty.node, &map);
                        let pty = self
                            .lower_type(&substituted)
                            .map(|t| type_id_to_field_name(&t))
                            .unwrap_or_else(|_| "unknown".into());
                        ParamSig {
                            name: p.name.clone(),
                            ty: pty,
                            is_ref: p.is_ref,
                            is_out: p.is_out,
                            is_in: p.is_in,
                            is_params: p.is_params,
                            default: None,
                        }
                    })
                    .collect();
                let new_ret = if let Some(ret) = &sig.ret {
                    let substituted = substitute_type_ast(&ret.node, &map);
                    self.lower_type(&substituted)
                        .map(|t| type_id_to_field_name(&t))
                        .unwrap_or_else(|_| "unknown".into())
                } else {
                    "void".into()
                };
                methods
                    .entry(sig.name.clone())
                    .or_default()
                    .push(OopMethodSig {
                        name: sig.name.clone(),
                        vis: sig.vis,
                        params: new_params,
                        ret: new_ret,
                        modifier: sig.modifier,
                        is_async: sig.is_async,
                        generics: vec![],
                        is_static_abstract: sig.is_static_abstract,
                    });
            }
        }
        let template_vis = iface_def
            .as_ref()
            .map(|d| d.vis)
            .or_else(|| self.registry.types.get(template_name).map(|t| t.vis))
            .unwrap_or(Visibility::Public);
        self.registry.types.insert(
            mangled.into(),
            NominalType {
                name: mangled.into(),
                kind: TypeKind::Interface,
                vis: template_vis,
                is_abstract: false,
                is_record: false,
                is_readonly: false,
                fields: IndexMap::new(),
                methods,
                bases: vec![],
                base_types: vec![],
                span: Span::DUMMY,
                variants: vec![],
                generic_params: vec![],
                namespace: vec![],
                const_values: IndexMap::new(),
                constructors: vec![],
                soa: false,
                required_props: Default::default(),
            },
        );
        Ok(())
    }

    pub(crate) fn instantiate_generic_interface(
        &mut self,
        def: &Ident,
        args: &[TypeId],
    ) -> Result<TypeId, TypeError> {
        // RFC 038：栈式 in-progress 环检测。mangled 在 enter 前先算出。
        let mangled = mangle_generic(def, args);
        // 负缓存短路：该实例化点已确认约束违约，直接沿原契约冒泡缓存哨兵，
        // 不重跑检查（违约明细零重复入池）。与 `instantiated` 正缓存对称
        // ——正缓存挡重复单态化，负缓存挡重复违约检查。
        if let Some(sentinel) = self.violated.get(mangled.as_str()) {
            return Err(sentinel.clone());
        }
        let _recursion_token = match self.recursion_iface.enter(&mangled) {
            Ok(token) => token,
            Err(cycle) => {
                cycle.report("泛型接口");
                // 在途环：短路返回占位名（接口无 shell 语义，靠 memoize + 本栈截断）。
                return Ok(TypeId::Named(mangled.into()));
            }
        };
        if args.iter().any(type_contains_generic) {
            // RFC 019 M-C：含 Generic 占位符的接口实例仍需注册 stub NominalType，
            // 否则泛型方法体内对接口的调用/转换失败（见
            // `register_parametrized_generic_iface`）。
            if self.instantiated.contains(&mangled) {
                return Ok(TypeId::Named(mangled.into()));
            }
            self.instantiated.insert(mangled.clone());
            if !self.registry.types.contains_key(mangled.as_str()) {
                self.register_parametrized_generic_iface(def, mangled.as_str(), args)?;
            }
            return Ok(TypeId::Named(mangled.into()));
        }
        if self.instantiated.contains(&mangled) {
            return Ok(TypeId::Named(mangled.into()));
        }
        // Prefer `interface_templates` (preserves generic args in method sigs)
        // over `registry.types` (which drops generics via `type_path_name`).
        // Fallback to registry for ad-hoc generic interfaces not in templates
        // (e.g. synthesized by builtins).
        let iface_def = self.interface_templates.get(def).cloned();
        let (generic_params, where_clause): (Vec<GenericParam>, Vec<TypeConstraint>) =
            match &iface_def {
                Some(d) => (d.generics.clone(), d.where_clause.clone()),
                None => {
                    let iface = self
                        .registry
                        .types
                        .get(def)
                        .ok_or_else(|| TypeError::GenericTypeNeedsArgs(def.to_string()))?;
                    (
                        iface
                            .generic_params
                            .iter()
                            .map(|n| GenericParam::new(n.clone()))
                            .collect(),
                        vec![],
                    )
                }
            };
        if generic_params.len() != args.len() {
            return Err(TypeError::GenericArity {
                name: def.to_string(),
                expected: generic_params.len(),
                found: args.len(),
            });
        }
        // Check `where` clause constraints。违约同时登记负缓存：同实例化点
        // 重触达由入口负缓存短路，不再重跑检查（明细零重复入池）。
        if let Err(e) = self.check_constraints(&where_clause, &generic_params, args) {
            self.violated.insert(mangled.clone(), e.clone());
            return Err(e);
        }
        self.instantiated.insert(mangled.clone());
        self.mono_origins
            .insert(mangled.clone(), (def.clone(), args.to_vec()));
        let map: IndexMap<Ident, TypeId> = generic_params
            .iter()
            .map(|p| p.name.clone())
            .zip(args.iter())
            .map(|(n, a)| (n, a.clone()))
            .collect();
        let mut methods: IndexMap<Ident, Vec<OopMethodSig>> = IndexMap::new();
        if let Some(d) = &iface_def {
            // Build methods from AST (preserves generic args like `IEnumerator<T>`)
            for sig in &d.methods {
                let new_params: Vec<ParamSig> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let substituted = substitute_type_ast(&p.ty.node, &map);
                        let pty = self
                            .lower_type(&substituted)
                            .map(|t| type_id_to_field_name(&t))
                            .unwrap_or_else(|_| "unknown".into());
                        ParamSig {
                            name: p.name.clone(),
                            ty: pty,
                            is_ref: p.is_ref,
                            is_out: p.is_out,
                            is_in: p.is_in,
                            is_params: p.is_params,
                            default: None,
                        }
                    })
                    .collect();
                let new_ret = if let Some(ret) = &sig.ret {
                    let substituted = substitute_type_ast(&ret.node, &map);
                    self.lower_type(&substituted)
                        .map(|t| type_id_to_field_name(&t))
                        .unwrap_or_else(|_| "unknown".into())
                } else {
                    "void".into()
                };
                methods
                    .entry(sig.name.clone())
                    .or_default()
                    .push(OopMethodSig {
                        name: sig.name.clone(),
                        vis: sig.vis,
                        params: new_params,
                        ret: new_ret,
                        modifier: sig.modifier,
                        is_async: sig.is_async,
                        generics: vec![],
                        is_static_abstract: sig.is_static_abstract,
                    });
            }
        } else {
            // Fallback: use registry methods (generics already lost — only safe
            // for non-generic-in-generic-arg positions like `int`).
            let iface = self
                .registry
                .types
                .get(def)
                .ok_or_else(|| TypeError::GenericTypeNeedsArgs(def.to_string()))?;
            for (mname, sigs) in &iface.methods {
                let mut new_sigs = Vec::new();
                for sig in sigs {
                    let new_params: Vec<ParamSig> = sig
                        .params
                        .iter()
                        .map(|p| {
                            let pty = type_id_to_field_name(&substitute_type(
                                &TypeId::Named(p.ty.clone()),
                                &map,
                            ));
                            ParamSig {
                                name: p.name.clone(),
                                ty: pty,
                                is_ref: p.is_ref,
                                is_out: p.is_out,
                                is_in: p.is_in,
                                is_params: p.is_params,
                                default: None,
                            }
                        })
                        .collect();
                    let new_ret = type_id_to_field_name(&substitute_type(
                        &TypeId::Named(sig.ret.clone()),
                        &map,
                    ));
                    new_sigs.push(OopMethodSig {
                        name: sig.name.clone(),
                        vis: sig.vis,
                        params: new_params,
                        ret: new_ret,
                        modifier: sig.modifier,
                        is_async: sig.is_async,
                        is_static_abstract: sig.is_static_abstract,
                        generics: vec![],
                    });
                }
                methods.insert(mname.clone(), new_sigs);
            }
        }
        let mut fields = IndexMap::new();
        // CD-32 根因修复：实例化接口的 `declared_properties` 必须同步收集——
        // 2ede392d 起 InterfaceLayout.properties 只从 `declared_properties` 构建
        // （methods 已排除 get_X 访问器），泛型接口实例化（`IAsyncEnumerator_string`
        // 等）此前从不写入 → Current 等接口属性无 itable 槽位 → `iface_method_index`
        // 兜底返回 0 → 调用错位（yield 迭代器 Current 读取垃圾值/空值）。对齐
        // 非泛型接口注册路径（registry.rs 接口分支），属性签名含类型替换。
        let mut iface_declared_props: Vec<crate::oop_types::DeclaredPropertySig> = Vec::new();
        if let Some(d) = &iface_def {
            for p in &d.properties {
                // RFC 004 M2：跳过 `static abstract` 属性（如 `INumber<T>.Zero`/`One`）。
                // 它们由实现类提供 `public static` 属性，不属于实例字段表；
                // typeck 通过 `check_static_abstract_field` 单独校验，codegen
                // 通过 `@Type_get_Zero` 拦截器发射。若入 `fields`，`check_interface_impl`
                // 会要求实现类提供同名实例字段 → 误报 "method signature mismatch"。
                if p.is_static_abstract {
                    continue;
                }
                let substituted = substitute_type_ast(&p.ty.node, &map);
                let new_ty = self
                    .lower_type(&substituted)
                    .map(|t| type_id_to_field_name(&t))
                    .unwrap_or_else(|_| "unknown".into());
                iface_declared_props.push(crate::oop_types::DeclaredPropertySig {
                    name: p.name.clone(),
                    ty: new_ty.clone(),
                    can_read: p.has_get,
                    can_write: p.has_set || p.has_init,
                });
                // RFC 007：接口索引器单态化为 get_Item/set_Item 方法，禁止入 fields。
                if p.is_indexer() {
                    let index_params: Vec<ParamSig> = p
                        .index_params
                        .iter()
                        .map(|ip| {
                            let pty = self
                                .lower_type(&substitute_type_ast(&ip.ty.node, &map))
                                .map(|t| type_id_to_field_name(&t))
                                .unwrap_or_else(|_| "unknown".into());
                            ParamSig {
                                name: ip.name.clone(),
                                ty: pty,
                                is_ref: ip.is_ref,
                                is_out: ip.is_out,
                                is_in: ip.is_in,
                                is_params: ip.is_params,
                                default: None,
                            }
                        })
                        .collect();
                    if p.has_get {
                        methods
                            .entry("get_Item".into())
                            .or_default()
                            .push(OopMethodSig {
                                name: "get_Item".into(),
                                vis: p.vis,
                                params: index_params.clone(),
                                ret: new_ty.clone(),
                                modifier: p.modifier,
                                is_async: false,
                                generics: vec![],
                                is_static_abstract: false,
                            });
                    }
                    if p.has_set {
                        let mut set_params = index_params;
                        set_params.push(ParamSig {
                            name: "value".into(),
                            ty: new_ty,
                            is_ref: false,
                            is_out: false,
                            is_in: false,
                            is_params: false,
                            default: None,
                        });
                        methods
                            .entry("set_Item".into())
                            .or_default()
                            .push(OopMethodSig {
                                name: "set_Item".into(),
                                vis: p.vis,
                                params: set_params,
                                ret: "void".into(),
                                modifier: p.modifier,
                                is_async: false,
                                generics: vec![],
                                is_static_abstract: false,
                            });
                    }
                    continue;
                }
                // 与非泛型 interface 注册一致（registry.rs）：property 是方法契约，
                // 注册为 get_Prop/set_Prop，供 check_interface_impl 与类 custom
                // property（get_Prop）匹配。禁止入 fields（否则报 missing property）。
                if p.has_get {
                    methods
                        .entry(format!("get_{}", p.name).into())
                        .or_default()
                        .push(OopMethodSig {
                            name: format!("get_{}", p.name).into(),
                            vis: p.vis,
                            params: vec![],
                            ret: new_ty.clone(),
                            modifier: ast::MethodModifier::Abstract,
                            is_async: false,
                            generics: vec![],
                            is_static_abstract: false,
                        });
                }
                if p.has_set {
                    methods
                        .entry(format!("set_{}", p.name).into())
                        .or_default()
                        .push(OopMethodSig {
                            name: format!("set_{}", p.name).into(),
                            vis: p.vis,
                            params: vec![ParamSig {
                                name: "value".into(),
                                ty: new_ty.clone(),
                                is_ref: false,
                                is_out: false,
                                is_in: false,
                                is_params: false,
                                default: None,
                            }],
                            ret: "void".into(),
                            modifier: ast::MethodModifier::Abstract,
                            is_async: false,
                            generics: vec![],
                            is_static_abstract: false,
                        });
                }
            }
        } else {
            let iface = self
                .registry
                .types
                .get(def)
                .ok_or_else(|| TypeError::GenericTypeNeedsArgs(def.to_string()))?;
            for (pname, finfo) in &iface.fields {
                let new_ty =
                    type_id_to_field_name(&substitute_type(&TypeId::Named(finfo.ty.clone()), &map));
                fields.insert(
                    pname.clone(),
                    FieldInfo {
                        name: finfo.name.clone(),
                        ty: new_ty,
                        vis: finfo.vis,
                        is_const: finfo.is_const,
                        is_readonly: finfo.is_readonly,
                        is_init_only: finfo.is_init_only,
                        get_vis: finfo.get_vis,
                        set_vis: finfo.set_vis,
                        is_static: finfo.is_static,
                        init: None,
                    },
                );
            }
        }
        // RFC 036：保留 AST 接口继承到单态实例的 `base_types`（并写入 `bases`）。
        // 例：`ICollection<T> : IEnumerable<T>` → `ICollection_int.base_types`
        // 含代入后的 `IEnumerable<int>`，layout 才能经 `collect_ast_iface_ancestors`
        // 把父接口纳入 class `implemented_interfaces`（`is IEnumerable<int>`）。
        // variance 合成基类只进 `bases`、不进 `base_types`（与非泛型注册一致）。
        let mut bases: Vec<Ident> = Vec::new();
        let mut base_types: Vec<Type> = Vec::new();
        let ast_bases: Vec<Type> = match &iface_def {
            Some(d) => d.bases.clone(),
            None => self
                .registry
                .types
                .get(def)
                .map(|t| t.base_types.clone())
                .unwrap_or_default(),
        };
        for base in &ast_bases {
            let substituted = substitute_type_ast(base, &map);
            if let Some(parent_name) = self.resolve_base_type_name(&substituted) {
                if !bases.contains(&parent_name) {
                    bases.push(parent_name);
                }
            }
            base_types.push(substituted);
        }
        // Compute covariant bases: for each `out T` param, if the argument
        // has supertypes, register those interface instantiations as bases.
        // e.g. IGetter<out T> with T=Dog → base IGetter_IAnimal because Dog : IAnimal.
        //
        // Contravariant `in T`: link after insert via `link_contravariant_adapter_views`
        // (IConsumer_IAnimal.bases ⊇ IConsumer_Dog). Overload collection skips
        // variance-synthesized bases（见 `collect_method_overloads`）。
        let mut covariant_base_args: Vec<Vec<TypeId>> = Vec::new();
        if iface_def.is_some() {
            for (gp, arg) in generic_params.iter().zip(args.iter()) {
                if gp.variance == Variance::Covariant {
                    if let TypeId::Named(arg_name) = arg {
                        if let Some(arg_ty) = self.registry.types.get(arg_name) {
                            for base in &arg_ty.bases {
                                if self.registry.is_interface(base) || self.registry.is_class(base)
                                {
                                    let mut v_args = args.to_vec();
                                    let idx = generic_params
                                        .iter()
                                        .position(|p| p.name == gp.name)
                                        .unwrap();
                                    v_args[idx] = TypeId::Named(base.clone());
                                    covariant_base_args.push(v_args);
                                }
                            }
                        }
                    }
                }
            }
        }
        for v_args in &covariant_base_args {
            let variant_mangled = mangle_generic(def, v_args);
            bases.push(variant_mangled.into());
            let _ = self.instantiate_generic_interface(def, v_args)?;
        }
        // 模板可见性透传到单态实例（internal 泛型类跨包仍不可见）。
        let template_vis = iface_def
            .as_ref()
            .map(|d| d.vis)
            .or_else(|| self.registry.types.get(def).map(|t| t.vis))
            .unwrap_or(Visibility::Public);
        // 泛型模板经此路径单态化的可能是接口、struct 或 enum（struct/enum 不在
        // `class_templates` 中，只能从 `registry.types` 回退注册）。若一律注册为
        // `TypeKind::Interface`，`CollectionChangedEventArgs<int>` 等泛型 struct
        // mono 会被 MIR 当作接口，字段访问 `args.Action` 错误走 `get_Action`
        // property 调用 → codegen "use of undefined value"。此处透传模板真实 kind。
        let mono_kind = match self.registry.types.get(def).map(|t| t.kind) {
            Some(TypeKind::Struct) => TypeKind::Struct,
            Some(TypeKind::Enum) => TypeKind::Enum,
            _ => TypeKind::Interface,
        };
        self.registry.types.insert(
            mangled.clone().into(),
            NominalType {
                name: mangled.clone().into(),
                kind: mono_kind,
                vis: template_vis,
                is_abstract: false,
                is_record: false,
                is_readonly: false,
                fields,
                methods,
                bases,
                base_types,
                span: Span::DUMMY,
                variants: vec![],
                generic_params: vec![],
                namespace: vec![],
                const_values: IndexMap::new(),
                constructors: vec![],
                soa: false,
                required_props: Default::default(),
            },
        );
        // CD-32 根因修复（续）：实例化接口的 declared_properties 随 NominalType
        // 一并注册——layout 的 InterfaceLayout.properties 依赖它发射接口属性槽位。
        self.registry
            .declared_properties
            .insert(mangled.clone().into(), iface_declared_props);
        // `in T`：在双方 mono 均已入表后互链适配器视图（不进 base_types）。
        self.link_contravariant_adapter_views(def, &mangled, args)?;
        self.scopes.last_mut().unwrap().insert(
            mangled.clone().into(),
            TypeId::Named(mangled.clone().into()),
        );
        Ok(TypeId::Named(mangled.into()))
    }

    pub(crate) fn instantiate_generic_fn(
        &mut self,
        def: &Ident,
        args: &[TypeId],
    ) -> Result<Ident, TypeError> {
        let (where_clause, generics): (Vec<TypeConstraint>, Vec<GenericParam>) = {
            let template = self
                .fn_templates
                .get(def)
                .ok_or_else(|| TypeError::Undefined(def.to_string()))?;
            (template.where_clause.clone(), template.generics.clone())
        };
        if generics.len() != args.len() {
            return Err(TypeError::GenericArity {
                name: def.to_string(),
                expected: generics.len(),
                found: args.len(),
            });
        }
        let mangled = mangle_generic(def, args);
        // 负缓存短路：违约实例化点不进正缓存，重触达会重跑检查导致违约明细
        // 重复入池。mangled 构造前移至检查之前，使负缓存查询与五路单态化
        // 入口保持同序（mangled → 短路 → check 登记 → 正缓存）。
        if let Some(sentinel) = self.violated.get(mangled.as_str()) {
            return Err(sentinel.clone());
        }
        if let Err(e) = self.check_constraints(&where_clause, &generics, args) {
            self.violated.insert(mangled.clone(), e.clone());
            return Err(e);
        }
        if std::env::var("ARC_DBG_MONO").is_ok()
            && (def.as_str().contains("EndpointDispatcher")
                || def.as_str().contains("BehaviorChain"))
        {
            eprintln!(
                "[mono] instantiate_generic_fn def={def} args={:?} mangled={mangled}",
                args.iter().map(|t| t.display()).collect::<Vec<_>>()
            );
        }
        if self.instantiated.contains(&mangled) {
            return Ok(mangled.into());
        }
        self.instantiated.insert(mangled.clone());
        let map = substitution_map(&generics, args);
        let template = self
            .fn_templates
            .get(def)
            .ok_or_else(|| TypeError::Undefined(def.to_string()))?;
        // RFC 038 泛型特化：`Enum.GetOptions<TEnum>()`（Arc 根，对标 System.Enum）
        // 在单态化为具体枚举 E 时，直接根据 E 各成员的 `[DisplayName]`/
        // `[Description]`（Arc.ComponentModel 通用属性）生成特化方法体（零反射、
        // 编译期烘焙）。仅对 std 的 `Enum::GetOptions` 模板生效；未标注属性的
        // 枚举成员回退模板体语义（成员名显示、空串描述）。
        let inst = substitute_fn_def(template, &mangled, &map);
        // template 借用至此结束；后续需可变 self（上下文切换 / body check）。
        let inst = if def.as_str() == "Enum::GetOptions" {
            match self.specialize_enum_options_body(&inst, args) {
                Some(body) => FnDef {
                    body: Some(body),
                    ..inst
                },
                None => inst,
            }
        } else {
            inst
        };
        // 恢复模板声明侧的包 / 命名空间上下文：force-instantiate 与跨包调用点
        // 单态化时，current_package 往往仍是消费端（用户 Program）。若不切回，
        // 方法体内访问库内 `internal` 类型成员会被 `can_access_type` 拒掉，
        // body check 失败 → 扩展方法永不单态化 → LLVM undefined value（RFC 019 M-B /
        // 与 `ensure_type_accessible` 的 mono 豁免同因，但成员查找未走该豁免）。
        //
        // 模板泛型函数定义在原始包，通过 fn_template_origins（收集期记录的
        // 声明 span + 命名空间）恢复声明包；span 缺失时按 `package_of` 回退
        // （与 instantiate_generic_class 同源，但 FnDef 不携带 span）。
        let (tmpl_span, tmpl_ns) = self
            .fn_template_origins
            .get(def)
            .cloned()
            .unwrap_or((Span::DUMMY, Vec::new()));
        let prev_pkg = self.current_package.clone();
        let prev_ns = self.enclosing_namespace.clone();
        self.enter_package_for_span(tmpl_span);
        // span 缺失时按模板声明包名回退（与 `package_of` 同源）。
        if self.current_package.is_none() {
            if let Some(pkg) = self.registry.package_of(def) {
                self.current_package = Some(pkg.to_string());
            }
        }
        self.enclosing_namespace = tmpl_ns;
        // RFC 017 M4-link Phase B：泛型函数单态化实例 → Monomorphized linkage
        // （codegen 发射为 linkonce_odr，跨 .o 弱符号去重）。
        self.mono_depth += 1;
        let result = self.check_fn_inner(DefId(0), &inst, true, FnLinkage::Monomorphized);
        self.mono_depth -= 1;
        self.current_package = prev_pkg;
        self.enclosing_namespace = prev_ns;
        result?;
        Ok(mangled.into())
    }

    /// RFC 038 泛型特化：为 `Enum.GetOptions<E>()` 生成编译期烘焙的方法体。
    ///
    /// Arc 拒绝运行时反射（RFC 004），改由**编译期**在 `Enum.GetOptions<E>()`
    /// 单态化为具体枚举 `E` 时，直接按 E 各成员的 `[DisplayName]`/`[Description]`
    /// 属性生成构造 `EnumOptions<E>` 的方法体（零反射、零运行时开销）。
    ///
    /// 生成的语义等价于（属性取值来自编译期属性表）：
    /// ```text
    /// EnumOptions<E> _options = new EnumOptions<E>();
    /// _options.Add(E.MemberA, "显示A", "描述A");
    /// _options.Add(E.MemberB, "显示B", "描述B");
    /// return _options;
    /// ```
    ///
    /// 仅对具体枚举类型特化；当 `E` 非当前编译单元的已知枚举（如仍为泛型
    /// 参数、或枚举位于未收集属性的其他单元）时返回 `None`，回退到模板体
    /// （返回空选项），保证调用点始终可编译。
    fn specialize_enum_options_body(
        &mut self,
        _inst: &FnDef,
        args: &[TypeId],
    ) -> Option<ast::Block> {
        // 取具体枚举类型名（仅 `Named` 形态可特化）。
        let enum_name = match args.first() {
            Some(TypeId::Named(n)) => n.clone(),
            _ => return None,
        };
        // 必须为本编译单元已收集属性的已知枚举。
        let enum_def = self.enum_defs.get(&enum_name)?.clone();
        if enum_def.variants.is_empty() {
            return None;
        }

        let span = ast::Span::DUMMY;
        let spanned = |e: ast::Expr| ast::Spanned::new(e, span);
        // 选项集合类型 `EnumOptions<E>`。
        let options_ty = |name: &Ident| ast::Type::Named {
            path: vec!["EnumOptions".into()],
            generics: vec![ast::Spanned::new(
                ast::Type::Named {
                    path: vec![name.clone()],
                    generics: vec![],
                },
                span,
            )],
        };

        let mut stmts: Vec<ast::Spanned<ast::Stmt>> = Vec::new();
        // EnumOptions<E> _options = new EnumOptions<E>();
        stmts.push(ast::Spanned::new(
            ast::Stmt::Let {
                mutable: false,
                name: "_options".into(),
                ty: Some(ast::Spanned::new(options_ty(&enum_name), span)),
                init: Some(spanned(ast::Expr::New {
                    ty: ast::Spanned::new(options_ty(&enum_name), span),
                    args: vec![],
                    obj_init: None,
                })),
            },
            span,
        ));

        // 逐个枚举成员：_options.Add(E.Member, "displayName", "desc");
        for v in &enum_def.variants {
            // 读取成员属性（无 `[DisplayName]` 回退成员名，无 `[Description]`
            // 回退空串；均为 Arc.ComponentModel 通用属性）。
            let member_def_id = self
                .member_def_ids
                .get(&(enum_name.clone(), v.name.clone()))
                .copied();
            let display = member_def_id
                .and_then(|id| self.attribute_table.find_attr(id, "DisplayName"))
                .and_then(|a| a.args.first().and_then(crate::ResolvedArg::as_string))
                .unwrap_or(v.name.as_str())
                .to_string();
            let description = member_def_id
                .and_then(|id| self.attribute_table.find_attr(id, "Description"))
                .and_then(|a| a.args.first().and_then(crate::ResolvedArg::as_string))
                .unwrap_or("")
                .to_string();

            let member_access = ast::Expr::Field {
                receiver: Box::new(spanned(ast::Expr::Ident(enum_name.clone()))),
                field: v.name.clone(),
            };
            let add_call = ast::Expr::MethodCall {
                receiver: Box::new(spanned(ast::Expr::Ident("_options".into()))),
                method: "Add".into(),
                args: vec![
                    spanned(member_access),
                    spanned(ast::Expr::StringLit(display)),
                    spanned(ast::Expr::StringLit(description)),
                ],
                type_args: vec![],
                params_span: None,
            };
            stmts.push(ast::Spanned::new(ast::Stmt::Expr(spanned(add_call)), span));
        }

        // return _options;
        stmts.push(ast::Spanned::new(
            ast::Stmt::Return(Some(spanned(ast::Expr::Ident("_options".into())))),
            span,
        ));
        Some(ast::Block { stmts, tail: None })
    }

    /// RFC 038 OOP 路径：静态 `Enum.*<E>()` 调用点发射特化方法体。
    ///
    /// `Enum.GetOptions<MyStatus>()` 解析为 `Expr::MethodCall`（receiver =
    /// `Ident("Enum")`），走 OOP 静态泛型路径——typeck 仅解析签名，方法体由
    /// MIR `try_create_mono_body` 从 `Enum::*` 模板克隆（无特化）。
    /// 本方法在调用点把 `Enum::*__MyStatus`（`__` 双下划线，与 MIR 单态化
    /// 命名一致）作为带特化方法体的 typed_fn 发射，MIR 发现该符号已存在即
    /// 跳过克隆，从而获得编译期烘焙的数据源（零反射）。
    ///
    /// 支持的烘焙方法（RFC 004 枚举能力增强）：
    /// - `GetOptions`：按成员属性烘焙 `EnumOptions<E>` 选项集合。
    /// - `HasFlag`：烘焙 `(value & flag) == flag` 位组合判断。
    /// - `IsDefined`：烘焙 `value == E.M1 || …` 成员穷举判断。
    /// - `GetNames` / `GetValues`：烘焙 `List<string>` / `List<E>` 成员名/值。
    pub(crate) fn maybe_emit_enum_baked_method(
        &mut self,
        tname: &Ident,
        method: &Ident,
        type_args: &[ast::Spanned<ast::Type>],
        arg_types: &[TypeId],
    ) {
        if tname.as_str() != "Enum" {
            return;
        }
        let m = method.as_str();
        if !matches!(
            m,
            "GetOptions" | "HasFlag" | "IsDefined" | "GetNames" | "GetValues"
        ) {
            return;
        }
        // 枚举类型来源：显式 `<E>`（GetOptions/GetNames/GetValues）或从实参推断
        // （HasFlag/IsDefined 泛型方法，调用点无显式 type_args）。两种形态统一
        // 以具体枚举类型特化，MIR 发现 `Enum::*__E` 已存在即跳过模板克隆。
        let enum_ty = if type_args.len() == 1 {
            match self.lower_type(&type_args[0].node) {
                Ok(TypeId::Named(n)) => TypeId::Named(n),
                _ => return,
            }
        } else {
            match arg_types.first() {
                Some(TypeId::Named(n)) if self.enum_defs.contains_key(n) => {
                    TypeId::Named(n.clone())
                }
                _ => return,
            }
        };
        let enum_name = match &enum_ty {
            TypeId::Named(n) => n.clone(),
            _ => return,
        };
        // 仅对本编译单元已收集属性的已知枚举特化；否则 MIR 克隆模板 stub。
        if !self.enum_defs.contains_key(&enum_name) {
            return;
        }
        let base = format!("Enum::{m}");
        let mangled = format!("{base}__{}", type_id_to_field_name(&enum_ty));
        if self.instantiated.contains(&mangled) {
            return;
        }
        let Some(template) = self.fn_templates.get(base.as_str()).cloned() else {
            return;
        };
        let map = substitution_map(&template.generics, std::slice::from_ref(&enum_ty));
        let inst = substitute_fn_def(&template, &mangled, &map);
        let body = match m {
            "GetOptions" => {
                self.specialize_enum_options_body(&inst, std::slice::from_ref(&enum_ty))
            }
            "HasFlag" => self.specialize_enum_has_flag_body(),
            "IsDefined" => self.specialize_enum_is_defined_body(&enum_name),
            "GetNames" => self.specialize_enum_names_body(&enum_name, false),
            "GetValues" => self.specialize_enum_names_body(&enum_name, true),
            _ => None,
        };
        if let Some(body) = body {
            self.instantiated.insert(mangled.clone());
            let inst = FnDef {
                name: mangled.clone().into(),
                body: Some(body),
                ..inst
            };
            let _ = self.check_fn_inner(DefId(0), &inst, true, FnLinkage::Monomorphized);
        }
    }

    /// 烘焙 `Enum.HasFlag<E>` 方法体：`return (value & flag) == flag;`。
    ///
    /// 位运算 `E & E` 由 RFC 004 枚举位运算 typeck 支撑（结果类型 E），
    /// 值/形参名须与 `Enum.HasFlag<T>(T value, T flag)` 声明一致。
    fn specialize_enum_has_flag_body(&self) -> Option<ast::Block> {
        let span = ast::Span::DUMMY;
        let spanned = |e: ast::Expr| ast::Spanned::new(e, span);
        let value = ast::Expr::Ident("value".into());
        let flag = ast::Expr::Ident("flag".into());
        let bitand = spanned(ast::Expr::Binary {
            op: ast::BinOp::BitAnd,
            left: Box::new(spanned(value)),
            right: Box::new(spanned(flag.clone())),
        });
        let body = spanned(ast::Expr::Binary {
            op: ast::BinOp::Eq,
            left: Box::new(bitand),
            right: Box::new(spanned(flag)),
        });
        Some(ast::Block {
            stmts: vec![ast::Spanned::new(ast::Stmt::Return(Some(body)), span)],
            tail: None,
        })
    }

    /// 烘焙 `Enum.IsDefined<E>` 方法体：`return value == E.M1 || value == E.M2 || …;`。
    fn specialize_enum_is_defined_body(&self, enum_name: &Ident) -> Option<ast::Block> {
        let span = ast::Span::DUMMY;
        let spanned = |e: ast::Expr| ast::Spanned::new(e, span);
        let enum_def = self.enum_defs.get(enum_name)?.clone();
        if enum_def.variants.is_empty() {
            return None;
        }
        let value = ast::Expr::Ident("value".into());
        let mut expr: Option<ast::Spanned<ast::Expr>> = None;
        for v in &enum_def.variants {
            let member = ast::Expr::Field {
                receiver: Box::new(spanned(ast::Expr::Ident(enum_name.clone()))),
                field: v.name.clone(),
            };
            let cmp = spanned(ast::Expr::Binary {
                op: ast::BinOp::Eq,
                left: Box::new(spanned(value.clone())),
                right: Box::new(spanned(member)),
            });
            expr = Some(match expr {
                Some(acc) => spanned(ast::Expr::Binary {
                    op: ast::BinOp::Or,
                    left: Box::new(acc),
                    right: Box::new(cmp),
                }),
                None => cmp,
            });
        }
        let return_expr = expr?;
        Some(ast::Block {
            stmts: vec![ast::Spanned::new(
                ast::Stmt::Return(Some(return_expr)),
                span,
            )],
            tail: None,
        })
    }

    /// 烘焙 `Enum.GetNames<E>` / `Enum.GetValues<E>` 方法体。
    ///
    /// `asValues = true` 烘焙成员值（元素类型 E，`_values.Add(E.Member)`）；
    /// 否则烘焙成员名（元素类型 string，`_names.Add("Member")`）。
    fn specialize_enum_names_body(&self, enum_name: &Ident, as_values: bool) -> Option<ast::Block> {
        let span = ast::Span::DUMMY;
        let spanned = |e: ast::Expr| ast::Spanned::new(e, span);
        let enum_def = self.enum_defs.get(enum_name)?.clone();
        if enum_def.variants.is_empty() {
            return None;
        }
        let elem_ty = if as_values {
            ast::Type::Named {
                path: vec![enum_name.clone()],
                generics: vec![],
            }
        } else {
            ast::Type::Named {
                path: vec!["string".into()],
                generics: vec![],
            }
        };
        let list_ty = ast::Type::Named {
            path: vec!["List".into()],
            generics: vec![ast::Spanned::new(elem_ty, span)],
        };
        let var = Ident::from(if as_values { "_values" } else { "_names" });

        let mut stmts: Vec<ast::Spanned<ast::Stmt>> = Vec::new();
        // List<X> _names = new List<X>();
        stmts.push(ast::Spanned::new(
            ast::Stmt::Let {
                mutable: false,
                name: var.clone(),
                ty: Some(ast::Spanned::new(list_ty.clone(), span)),
                init: Some(spanned(ast::Expr::New {
                    ty: ast::Spanned::new(list_ty, span),
                    args: vec![],
                    obj_init: None,
                })),
            },
            span,
        ));
        for v in &enum_def.variants {
            // _names.Add("Member") / _values.Add(E.Member)
            let item = if as_values {
                ast::Expr::Field {
                    receiver: Box::new(spanned(ast::Expr::Ident(enum_name.clone()))),
                    field: v.name.clone(),
                }
            } else {
                ast::Expr::StringLit(v.name.as_str().to_string())
            };
            let add_call = ast::Expr::MethodCall {
                receiver: Box::new(spanned(ast::Expr::Ident(var.clone()))),
                method: "Add".into(),
                args: vec![spanned(item)],
                type_args: vec![],
                params_span: None,
            };
            stmts.push(ast::Spanned::new(ast::Stmt::Expr(spanned(add_call)), span));
        }
        // return _names / _values;
        stmts.push(ast::Spanned::new(
            ast::Stmt::Return(Some(spanned(ast::Expr::Ident(var)))),
            span,
        ));
        Some(ast::Block { stmts, tail: None })
    }

    /// 单态化泛型扩展方法（决策 #7，RFC 010 / Iteration 4 扩展）。
    ///
    /// 按 `template_key`（`ExtensionMethod.template_key`，`method_link_name` 产物）
    /// 查找 `extension_fn_templates` 中的模板，以 `type_args` 实例化方法体，
    /// 生成 `Container::Method_<params>_arg1_arg2`（`mangle_generic` 单 `_` 连接）。
    /// 调用名由 `make_resolution` 以同一 `template_key` 为基底 mangle，
    /// 保证 MIR/codegen 符号与单态化方法体符号逐字节一致。
    ///
    /// 覆盖两种形态：
    /// - 泛型接收者（`static T Id<T>(this T x)`）：`type_args` = [接收者类型]；
    /// - 非泛型接收者 + 显式 type_args（`AddTransient<TService,TImpl>`）：`type_args`
    ///   = 调用点显式泛型实参（如 `[Greeter, Greeter]`）。
    pub(crate) fn instantiate_generic_extension_fn_by_key(
        &mut self,
        template_key: &Ident,
        type_args: &[Ident],
    ) -> Result<Ident, TypeError> {
        if std::env::var("ARC_DEBUG_REACH").is_ok() {
            eprintln!(
                "[g3a-ext] template_key={template_key} type_args={:?}",
                type_args
            );
        }
        let (where_clause, generics, mangle_base): (Vec<TypeConstraint>, Vec<GenericParam>, Ident) = {
            let template = self
                .extension_fn_templates
                .get(template_key)
                .ok_or_else(|| TypeError::Undefined(template_key.to_string()))?;
            (
                template.where_clause.clone(),
                template.generics.clone(),
                template.name.clone(),
            )
        };
        let args: Vec<TypeId> = type_args.iter().map(|t| TypeId::Named(t.clone())).collect();
        if generics.len() != args.len() {
            return Err(TypeError::GenericArity {
                name: template_key.to_string(),
                expected: generics.len(),
                found: args.len(),
            });
        }
        let mangled = mangle_generic(mangle_base.as_str(), &args);
        // 负缓存短路：该实例化点已确认约束违约，直接沿原契约冒泡缓存哨兵，
        // 不重跑检查（违约明细零重复入池）。与 `instantiated` 正缓存对称
        // ——正缓存挡重复单态化，负缓存挡重复违约检查。mangled 构造前移
        // 至检查之前，使负缓存查询与五路单态化入口保持同序。
        if let Some(sentinel) = self.violated.get(mangled.as_str()) {
            return Err(sentinel.clone());
        }
        if let Err(e) = self.check_constraints(&where_clause, &generics, &args) {
            // 违约登记负缓存：同实例化点重触达由入口负缓存短路，不再重跑
            // 检查（明细零重复入池）。哨兵沿原 `?` 契约冒泡中止本路单态化。
            self.violated.insert(mangled.clone(), e.clone());
            return Err(e);
        }
        if self.instantiated.contains(&mangled) {
            return Ok(mangled.into());
        }
        self.instantiated.insert(mangled.clone());
        let map = substitution_map(&generics, &args);
        let template = self
            .extension_fn_templates
            .get(template_key)
            .ok_or_else(|| TypeError::Undefined(template_key.to_string()))?;
        let inst = substitute_fn_def(template, &mangled, &map);
        // RFC 017 M4-link Phase B：泛型扩展方法单态化实例 → Monomorphized linkage。
        // RFC 023 已知缺口（di_decorate 红根因）：扩展方法单态化**必须**恢复
        // 模板声明侧包上下文——与 instantiate_generic_fn 的 fn_template_origins
        // 恢复同构；此前缺失使方法体内 `internal` 成员（如
        // `ServiceCollection._descriptors`，DecorationExtensions.Decorate 同包访问）
        // 被消费端 current_package 的跨包 can_access 拒绝 → 落
        // 「no field or property」误报。声明包按模板键的类前缀（`Class::Method`）
        // 反查 registry 条目（span + 命名空间）。
        let (tmpl_span, tmpl_ns): (ast::Span, Vec<Ident>) = {
            let class_part = template_key.as_str().split("::").next().unwrap_or("");
            self.registry
                .types
                .get(class_part)
                .map(|n| (n.span, n.namespace.clone()))
                .unwrap_or((Span::DUMMY, Vec::new()))
        };
        let prev_pkg = self.current_package.clone();
        let prev_ns = self.enclosing_namespace.clone();
        self.enter_package_for_span(tmpl_span);
        if self.current_package.is_none() {
            let class_part = template_key.as_str().split("::").next().unwrap_or("");
            if let Some(pkg) = self.registry.package_of(&class_part.into()) {
                self.current_package = Some(pkg.to_string());
            }
        }
        self.enclosing_namespace = tmpl_ns;
        let result = self.check_fn_inner(DefId(0), &inst, true, FnLinkage::Monomorphized);
        self.current_package = prev_pkg;
        self.enclosing_namespace = prev_ns;
        result?;
        Ok(mangled.into())
    }

    fn register_monomorphized_class(
        &mut self,
        class: &ClassDef,
        map: &IndexMap<Ident, TypeId>,
    ) -> Result<(), TypeError> {
        // RFC 037 M1: 修复泛型类单态化字段/方法签名 mangle 错误。
        //
        // `substitute_class_def`（在 `instantiate_generic_class` 中调用）
        // 已经在 AST 层面把所有类型参数替换为 map 中的 TypeId 经
        // `type_id_to_ast` 转换后的 `Type` 节点。此处 `lower_type` 已能
        // 得到正确的 `TypeId`，**不需要再 `substitute_type` 一次**。
        //
        // 旧实现冗余地调用 `substitute_type(.., map)`，当 map 的值中
        // 仍含 `Named("T")`（来自外层泛型上下文的占位符）时，会再次
        // 匹配 map 键 "T" 并替换为 map["T"]，导致嵌套类型——
        // 如 `List<Func<T, T, bool>>` 中 List 的 T 被替换为
        // `Func<T, T, bool>` 后，Add(item: T) 的 T 也被替换为
        // `Func<T, T, bool>`，再 substitute 一次得到
        // `Func<Func<T,T,bool>, Func<T,T,bool>, bool>`，mangle 为
        // `Func_Func_T_T_bool_Func_T_T_bool_bool` 而非正确的
        // `Func_T_T_bool`，方法重载解析失败。
        //
        // 同理，base 已被 `substitute_class_def` 处理（参见
        // `generics.rs::substitute_class_def` 末尾的 bases 循环），
        // 此处不再 `substitute_type_ast`。
        let _ = map;
        let mut fields = IndexMap::new();
        for f in &class.fields {
            let fty = type_id_to_field_name(&self.lower_type(&f.ty.node)?);
            fields.insert(
                f.name.clone(),
                FieldInfo {
                    name: f.name.clone(),
                    ty: fty,
                    vis: f.vis,
                    is_const: f.is_const,
                    is_readonly: f.is_readonly,
                    is_init_only: false,
                    get_vis: None,
                    set_vis: None,
                    is_static: f.is_static,
                    // RFC 006 M4：保留静态字段初始化器（与 fields_from_ast 一致）。
                    init: if f.is_static && !f.is_const {
                        f.init.clone()
                    } else {
                        None
                    },
                },
            );
        }
        for p in &class.properties {
            // 访问器形态判定走单一事实源（registry.rs `property_has_custom_accessors`）：
            // `[Builtin]` 自动属性在此归入 custom 路径（注册 get_X/set_X、无 backing
            // field），使 MIR `is_custom_accessor_property` 返回 true → 访问降为
            // MethodCall → codegen 拦截直射 rt_* ABI。单态化路径漏判曾致
            // `List<T>.Count { get; }` 注册为 backing field → MIR FieldGet 读
            // RtList* 垃圾偏移 → 运行期 list index out of bounds。
            let is_custom = crate::registry::property_has_custom_accessors(p);
            // `[Builtin]` 静态自动属性登记（mangled 类名；MIR 源码形分派判定依据）。
            self.registry.record_builtin_static_prop(&class.name, p);
            // RFC 006 A2：访问器体引用 `field` 的 custom 属性仍"自动"——pass1 为其
            // 合成 backing field（名=属性名）；pass2 照常注册 get/set 方法。
            let uses_field = is_custom && uses_field(&p.get_body, &p.set_body);
            if is_custom && !uses_field {
                continue;
            }
            let pty = type_id_to_field_name(&self.lower_type(&p.ty.node)?);
            let fname = if uses_field {
                crate::field_keyword::backing_field_name(&p.name)
            } else {
                p.name.clone()
            };
            fields.insert(
                fname.clone(),
                FieldInfo {
                    name: fname,
                    ty: pty,
                    vis: p.vis,
                    is_const: false,
                    is_readonly: false,
                    is_init_only: p.has_init && !p.has_set,
                    get_vis: p.get_vis,
                    set_vis: p.set_vis,
                    is_static: false,
                    init: None,
                },
            );
        }
        let mut methods: IndexMap<Ident, Vec<OopMethodSig>> = IndexMap::new();
        for p in &class.properties {
            // 同 pass1：访问器形态判定走单一事实源（含 `[Builtin]` auto 属性——
            // 注册 get_X/set_X 方法、无 backing field，使 MIR
            // `is_custom_accessor_property` 返回 true 而非降级为 FieldGet）。
            let is_custom = crate::registry::property_has_custom_accessors(p);
            if !is_custom {
                continue;
            }
            let pty = type_id_to_field_name(&self.lower_type(&p.ty.node)?);
            if p.is_indexer() {
                let index_params: Vec<ParamSig> = p
                    .index_params
                    .iter()
                    .map(|ip| {
                        Ok(ParamSig {
                            name: ip.name.clone(),
                            ty: type_id_to_field_name(&self.lower_type(&ip.ty.node)?),
                            is_ref: ip.is_ref,
                            is_out: ip.is_out,
                            is_in: ip.is_in,
                            is_params: ip.is_params,
                            default: None,
                        })
                    })
                    .collect::<Result<Vec<_>, TypeError>>()?;
                if p.has_get {
                    methods
                        .entry("get_Item".into())
                        .or_default()
                        .push(OopMethodSig {
                            name: "get_Item".into(),
                            vis: p.vis,
                            params: index_params.clone(),
                            ret: pty.clone(),
                            modifier: p.modifier,
                            is_async: false,
                            is_static_abstract: false,
                            generics: vec![],
                        });
                }
                if p.has_set {
                    let mut set_params = index_params;
                    set_params.push(ParamSig {
                        name: "value".into(),
                        ty: pty,
                        is_ref: false,
                        is_out: false,
                        is_in: false,
                        is_params: false,
                        default: None,
                    });
                    methods
                        .entry("set_Item".into())
                        .or_default()
                        .push(OopMethodSig {
                            name: "set_Item".into(),
                            vis: p.vis,
                            params: set_params,
                            ret: "void".into(),
                            modifier: p.modifier,
                            is_async: false,
                            is_static_abstract: false,
                            generics: vec![],
                        });
                }
                continue;
            }
            if p.has_get {
                let getter = OopMethodSig {
                    name: format!("get_{}", p.name).into(),
                    vis: p.get_vis.unwrap_or(p.vis),
                    params: vec![],
                    ret: pty.clone(),
                    modifier: p.modifier,
                    is_async: false,
                    is_static_abstract: false,
                    generics: vec![],
                };
                methods.entry(getter.name.clone()).or_default().push(getter);
            }
            if p.has_set || p.has_init {
                let setter = OopMethodSig {
                    name: format!("set_{}", p.name).into(),
                    vis: p.set_vis.unwrap_or(p.vis),
                    params: vec![ParamSig {
                        name: "value".into(),
                        ty: pty,
                        is_ref: false,
                        is_out: false,
                        is_in: false,
                        is_params: false,
                        default: None,
                    }],
                    ret: "void".into(),
                    modifier: p.modifier,
                    is_async: false,
                    is_static_abstract: false,
                    generics: vec![],
                };
                methods.entry(setter.name.clone()).or_default().push(setter);
                if p.has_init {
                    self.registry
                        .init_only_props
                        .insert((class.name.clone(), p.name.clone()));
                }
            }
        }
        for m in &class.methods {
            let sig = &m.node.sig;
            let mut params = Vec::new();
            for p in &sig.params {
                let pty = type_id_to_field_name(&self.lower_type(&p.ty.node)?);
                params.push(ParamSig {
                    name: p.name.clone(),
                    ty: pty,
                    is_ref: p.is_ref,
                    is_out: p.is_out,
                    is_in: p.is_in,
                    is_params: p.is_params,
                    // 默认值折叠对齐 registry.rs `method_sig_from_ast`——单态化
                    // 类签名丢失 default 会使省略实参调用（`Get(1)` 对
                    // `Get(T, ct = default)`）报 no matching overload。
                    default: p
                        .default
                        .as_ref()
                        .and_then(|e| crate::call_args::fold_param_default(&e.node)),
                });
            }
            let ret = if let Some(t) = &sig.ret {
                type_id_to_field_name(&self.lower_type(&t.node)?)
            } else {
                "void".into()
            };
            // 保留方法级泛型（如 `Mapper_int.Map<U>`），供
            // `resolve_method_with_type_args` / MIR method mono 使用。
            // 禁止清空——否则 `m.Map<string>(…)` 在单态化类上 NoMatchingOverload。
            let oop_sig = OopMethodSig {
                name: sig.name.clone(),
                vis: sig.vis,
                params,
                ret,
                modifier: sig.modifier,
                is_async: sig.is_async,
                is_static_abstract: sig.is_static_abstract,
                generics: sig.generics.iter().map(|g| g.name.clone()).collect(),
            };
            methods.entry(sig.name.clone()).or_default().push(oop_sig);
        }
        let bases: Vec<Ident> = class
            .bases
            .iter()
            .filter_map(|b| self.resolve_base_type_name(b))
            .collect();
        let required_props: indexmap::IndexSet<Ident> = class
            .properties
            .iter()
            .filter(|p| p.is_required)
            .map(|p| p.name.clone())
            .collect();
        // 保留模板的 span / namespace，供 `package_of` / 扩展方法作用域在
        // force-instantiate 与跨包单态化时恢复声明侧包身份（RFC 019 M-B）。
        let (tmpl_span, tmpl_ns) = self
            .mono_origins
            .get(class.name.as_str())
            .and_then(|(tmpl, _)| self.registry.types.get(tmpl))
            .map(|n| (n.span, n.namespace.clone()))
            .unwrap_or((Span::DUMMY, Vec::new()));
        self.registry.types.insert(
            class.name.clone(),
            NominalType {
                name: class.name.clone(),
                kind: TypeKind::Class,
                vis: class.vis,
                is_abstract: class.is_abstract,
                is_record: class.is_record,
                is_readonly: false,
                fields,
                methods,
                bases,
                base_types: vec![],
                span: tmpl_span,
                variants: vec![],
                generic_params: vec![],
                namespace: tmpl_ns,
                const_values: IndexMap::new(),
                // RFC 045（Element.SetValue<T> 内 `new Signal<T>` 崩溃根因）：
                // 泛型占位 stub（含 Generic 实参的实例）与完整实例化共用本注册——
                // 旧实现 constructors 恒空，stub 上 `new Signal_T(...)` 的 ctor
                // 查找失败（no matching constructor）；完整实例化的 constructors
                // 由后续 check_class 填充，stub 无 check 故必须在此克隆（模板
                // 替换后的 class.constructors）。
                constructors: crate::registry::ctors_from_ast(&class.constructors),
                soa: false,
                required_props,
            },
        );
        self.scopes
            .last_mut()
            .unwrap()
            .insert(class.name.clone(), TypeId::Named(class.name.clone()));
        Ok(())
    }

    /// Resolve a base type reference to its mangled name.
    /// Generic interfaces (e.g., `IEnumerable<T>`) are instantiated to their
    /// monomorphized form (e.g., `IEnumerable_int`) so that subtype checks work.
    fn resolve_base_type_name(&mut self, ty: &Type) -> Option<Ident> {
        if let Type::Named { path, generics } = ty {
            if !generics.is_empty() {
                let name = path.last()?;
                if self.registry.is_generic_template(name) && self.registry.is_interface(name) {
                    let args: Vec<TypeId> = generics
                        .iter()
                        .map(|g| self.lower_type(&g.node))
                        .collect::<Result<_, _>>()
                        .ok()?;
                    let inst = self.instantiate_generic_interface(name, &args).ok()?;
                    return Some(type_id_to_field_name(&inst));
                }
            }
        }
        let lowered = self.lower_type(ty).ok()?;
        Some(type_id_to_field_name(&lowered))
    }

    /// Check that concrete type arguments satisfy the `where` clause constraints.
    ///
    /// For each constraint `T : Bound`:
    /// - Find the index of `T` in `generics`
    /// - Get the corresponding `arg` from `args`
    /// - Substitute type params in `Bound`, then lower to `TypeId`
    /// - Check if `arg` satisfies the bound via `satisfies_constraint`
    ///
    /// Called after arity check, before monomorphization (which clears `where_clause`).
    ///
    /// 按 ConstraintKind 分派：
    /// - `Type(bound_ast)`：走 substitute + lower + satisfies_constraint 路径
    /// - `Class`：arg 必须为引用类型（class/string/array/...，排除 struct/基元）
    /// - `Struct`：arg 必须为值类型（基元或 struct）
    /// - `New`：值类型隐式满足；引用类型须有 public 无参构造函数
    ///
    /// 错误恢复语义（DiagnosticBag 模式）：完整遍历约束表收集**全部违约**
    /// 逐条推入错误池——而非首个违约即返回（fail-fast 会让用户按「修一个
    /// 报下一个」逐次循环修复）。全部违约入池后返回
    /// [`TypeError::ConstraintBatchViolated`] 哨兵沿 `?` 冒泡链传播：单
    /// `TypeError` 冒泡链保持不变，调用方以「有错误即短路」中止当前
    /// 单态化/表达式检查；违约明细由错误池逐条独立呈现。
    pub(crate) fn check_constraints(
        &mut self,
        where_clause: &[TypeConstraint],
        generics: &[GenericParam],
        args: &[TypeId],
    ) -> Result<(), TypeError> {
        if where_clause.is_empty() {
            return Ok(());
        }
        let map = substitution_map(generics, args);
        let mut violations: Vec<TypeError> = Vec::new();
        for constraint in where_clause {
            let Some(idx) = generics.iter().position(|p| p.name == constraint.param) else {
                continue;
            };
            let arg = &args[idx];
            match &constraint.kind {
                ConstraintKind::Type(bound_ast) => {
                    // 类型约束：含类型参数需 substitute，再 lower 为 TypeId 判定。
                    // bound 无法 lower 时该约束不可判定：收集 lower 错误并继续
                    // 其余约束（其余约束的判定不依赖本 bound——错误恢复语义）。
                    let bound_substituted = substitute_type_ast(&bound_ast.node, &map);
                    let bound_ty = match self.lower_type(&bound_substituted) {
                        Ok(t) => t,
                        Err(e) => {
                            violations.push(e);
                            continue;
                        }
                    };
                    if !self.satisfies_constraint(arg, &bound_ty) {
                        violations.push(TypeError::ConstraintNotSatisfied {
                            param: constraint.param.to_string(),
                            arg: arg.display(),
                            bound: bound_ty.display(),
                        });
                    }
                }
                ConstraintKind::Class => {
                    // 引用类型元约束：arg 必须为引用类型。
                    if !self.is_reference_type(arg) {
                        violations.push(TypeError::ConstraintNotSatisfied {
                            param: constraint.param.to_string(),
                            arg: arg.display(),
                            bound: "class".to_string(),
                        });
                    }
                }
                ConstraintKind::Struct => {
                    // 值类型元约束：arg 必须为值类型（基元或 struct）。
                    if !self.is_value_type(arg) {
                        violations.push(TypeError::ConstraintNotSatisfied {
                            param: constraint.param.to_string(),
                            arg: arg.display(),
                            bound: "struct".to_string(),
                        });
                    }
                }
                ConstraintKind::New => {
                    // C# 规范：值类型（基元/struct/enum）隐式满足 new() 约束。
                    if self.is_value_type(arg) {
                        continue;
                    }
                    // 引用类型须有 public 无参构造函数。C# 规范（语言参考
                    // §「无参数构造函数」）：类未声明任何实例构造函数时，
                    // 编译器为其隐式生成 public 无参默认构造函数——故
                    // `nominal.constructors` 为空也满足 `new()`。仅当显式
                    // 声明了构造函数时，才要求其中存在 public 无参数（与
                    // `new TRequest()` 实际 bound 的隐式默认构造回退保持
                    // 一致，避免隐式构造类违反约束）。
                    let has_public_parameterless_ctor = match arg {
                        TypeId::Named(n) => match self.registry.types.get(n.as_str()) {
                            Some(nominal) => {
                                nominal.constructors.is_empty()
                                    || nominal.constructors.iter().any(|c| {
                                        c.vis == Visibility::Public && c.param_types.is_empty()
                                    })
                            }
                            None => false,
                        },
                        _ => false,
                    };
                    if !has_public_parameterless_ctor {
                        violations.push(TypeError::ConstraintNotSatisfied {
                            param: constraint.param.to_string(),
                            arg: arg.display(),
                            bound: "new()".to_string(),
                        });
                    }
                }
            }
        }
        if violations.is_empty() {
            return Ok(());
        }
        // 全部违约入池；哨兵沿冒泡链传播以中止当前检查流（单态化不得以
        // 违约实参继续，否则下游级联错误）。
        let count = violations.len();
        self.errors.extend(violations);
        Err(TypeError::ConstraintBatchViolated { count })
    }

    /// Whether `arg` satisfies the constraint `: bound`.
    ///
    /// - Equal types trivially satisfy.
    /// - Primitives (int/double/bool/string/...) satisfy common builtin
    ///   interfaces (IComparable/IEquatable) without explicit registration,
    ///   mirroring C# where primitives implement these interfaces.
    /// - Named types use `is_subtype` (checks interface implementation via
    ///   the registry's `implements_interface`).
    ///
    /// 仅处理 ConstraintKind::Type 变体（已 lower 为 TypeId）。
    /// Class/Struct/New 元约束由 `check_constraints` 直接分派。
    fn satisfies_constraint(&self, arg: &TypeId, bound: &TypeId) -> bool {
        if arg == bound {
            return true;
        }
        if is_builtin_primitive(arg) && is_primitive_satisfiable_interface(bound) {
            return true;
        }
        // RFC 004 §值类型视图 ABI：enum 隐式满足 `IEquatable<E>`/`IHashable<E>`
        // （判别值比较/哈希，值语义）。此前 enum（`TypeId::Named`）落 `is_subtype`
        // 且 `bases` 为空 → 约束失败，`Dictionary<Color,int>` 报
        // "Color does not satisfy IEquatable_Color"。
        if self.is_enum_type(arg) && self.is_enum_satisfiable_interface(bound) {
            return true;
        }
        self.is_subtype(arg, bound)
    }

    /// Whether `arg` is a user-defined enum（`TypeId::Named` + registry `TypeKind::Enum`）。
    pub(crate) fn is_enum_type(&self, arg: &TypeId) -> bool {
        matches!(arg, TypeId::Named(n) if self.registry.is_enum(n))
    }

    /// Whether `bound` is `IEquatable<E>`/`IHashable<E>` whose suffix is a registered enum。
    fn is_enum_satisfiable_interface(&self, bound: &TypeId) -> bool {
        let name = match bound {
            TypeId::Named(n) => n.as_str(),
            _ => return false,
        };
        ["IEquatable_", "IHashable_"].iter().any(|prefix| {
            name.strip_prefix(prefix)
                .is_some_and(|suffix| self.registry.is_enum(&Ident::from(suffix)))
        })
    }

    /// RFC 009 P1-C2：声明期 where 校验——约束参数与边界中的类型参数须在 generics 中。
    pub(crate) fn validate_where_clause(
        &self,
        generics: &[GenericParam],
        where_clause: &[TypeConstraint],
    ) -> Result<(), TypeError> {
        let param_names: IndexSet<_> = generics.iter().map(|p| p.name.clone()).collect();
        for c in where_clause {
            if !param_names.contains(&c.param) {
                return Err(TypeError::UndefinedTypeParameter(c.param.to_string()));
            }
            if let ConstraintKind::Type(bound) = &c.kind {
                // 裸具名类型边界（`where T : IServiceProvider`）的头部是**名义类型**
                // （非类型参数），跳过收集——否则合法裸接口/类边界会被误判为
                // UndefinedTypeParameter。泛型边界（`where T : IBag<U>`）仍收集其泛型
                // 实参中的类型参数（`U` 须已声明）。
                let is_bare_named = matches!(
                    &bound.node,
                    Type::Named { generics, path } if generics.is_empty() && path.len() == 1
                );
                if !is_bare_named {
                    for name in collect_type_param_refs(&bound.node) {
                        if !param_names.contains(&name) {
                            return Err(TypeError::UndefinedTypeParameter(name.to_string()));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// RFC 009 P1-C2：接口 `in`/`out` 位置校验。
    pub(crate) fn validate_interface_variance(
        &self,
        iface: &InterfaceDef,
    ) -> Result<(), TypeError> {
        let param_var: IndexMap<_, _> = iface
            .generics
            .iter()
            .map(|p| (p.name.clone(), p.variance))
            .collect();
        if param_var.values().all(|v| *v == Variance::Invariant) {
            return Ok(());
        }
        // 基接口实参处于输出位（`IReadOnlyCollection<out T> : IEnumerable<T>`）。
        for b in &iface.bases {
            self.check_variance_in_type(b, &param_var, VariancePosition::Output)?;
        }
        for m in &iface.methods {
            if let Some(ret) = &m.ret {
                self.check_variance_in_type(&ret.node, &param_var, VariancePosition::Output)?;
            }
            for p in &m.params {
                self.check_variance_in_type(&p.ty.node, &param_var, VariancePosition::Input)?;
            }
        }
        for prop in &iface.properties {
            self.check_variance_in_type(&prop.ty.node, &param_var, VariancePosition::Output)?;
            if prop.has_set || prop.has_init {
                self.check_variance_in_type(&prop.ty.node, &param_var, VariancePosition::Input)?;
            }
            for ip in &prop.index_params {
                self.check_variance_in_type(&ip.ty.node, &param_var, VariancePosition::Input)?;
            }
        }
        Ok(())
    }

    /// 嵌套泛型实参的声明方差：接口模板取 `in`/`out`；其余保守不变。
    fn nested_type_arg_variances(&self, path: &[Ident], argc: usize) -> Vec<Variance> {
        let Some(name) = path.last() else {
            return vec![Variance::Invariant; argc];
        };
        if let Some(iface) = self.interface_templates.get(name) {
            let mut out: Vec<_> = iface.generics.iter().map(|p| p.variance).collect();
            out.resize(argc, Variance::Invariant);
            return out;
        }
        vec![Variance::Invariant; argc]
    }

    fn check_variance_in_type(
        &self,
        ty: &Type,
        params: &IndexMap<Ident, Variance>,
        pos: VariancePosition,
    ) -> Result<(), TypeError> {
        match ty {
            Type::Named { path, generics } => {
                if generics.is_empty() && path.len() == 1 {
                    if let Some(v) = params.get(&path[0]) {
                        match (v, pos) {
                            (Variance::Covariant, VariancePosition::Input) => {
                                return Err(TypeError::InvalidVariance {
                                    param: path[0].to_string(),
                                    variance: "out".into(),
                                    position: "input".into(),
                                });
                            }
                            (Variance::Contravariant, VariancePosition::Output) => {
                                return Err(TypeError::InvalidVariance {
                                    param: path[0].to_string(),
                                    variance: "in".into(),
                                    position: "output".into(),
                                });
                            }
                            _ => {}
                        }
                    }
                }
                // 嵌套实参：按目标类型形参的 `in`/`out` 合成极性（C# 对齐）。
                // 不变形参 → 子位置双向检查；协变保持；逆变翻转。
                let declared = self.nested_type_arg_variances(path, generics.len());
                for (g, decl_v) in generics.iter().zip(declared.iter()) {
                    match decl_v {
                        Variance::Invariant => {
                            self.check_variance_in_type(&g.node, params, VariancePosition::Input)?;
                            self.check_variance_in_type(&g.node, params, VariancePosition::Output)?;
                        }
                        Variance::Covariant => {
                            self.check_variance_in_type(&g.node, params, pos)?;
                        }
                        Variance::Contravariant => {
                            self.check_variance_in_type(&g.node, params, flip_variance_pos(pos))?;
                        }
                    }
                }
                Ok(())
            }
            Type::Array { inner } | Type::Nullable { inner } => {
                self.check_variance_in_type(&inner.node, params, pos)
            }
            Type::Ref { inner, .. } => self.check_variance_in_type(&inner.node, params, pos),
            Type::Func { params: fps, ret } => {
                for p in fps {
                    self.check_variance_in_type(&p.node, params, VariancePosition::Input)?;
                }
                self.check_variance_in_type(&ret.node, params, VariancePosition::Output)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// `in T` 适配器视图互链：`IConsumer_IAnimal.bases ⊇ IConsumer_Dog`（仅 `bases`，
    /// 不进 `base_types`）。在双方 mono 均已入 `mono_origins` / registry 后调用。
    fn link_contravariant_adapter_views(
        &mut self,
        template: &Ident,
        new_mono: &str,
        new_args: &[TypeId],
    ) -> Result<(), TypeError> {
        let has_in = self
            .interface_templates
            .get(template)
            .map(|d| {
                d.generics
                    .iter()
                    .any(|p| p.variance == Variance::Contravariant)
            })
            .unwrap_or(false);
        if !has_in {
            return Ok(());
        }
        let others: Vec<(String, Vec<TypeId>)> = self
            .mono_origins
            .iter()
            .filter(|(name, (t, _))| *t == *template && name.as_str() != new_mono)
            .map(|(name, (_, args))| (name.clone(), args.clone()))
            .collect();
        for (other_name, other_args) in others {
            // found=other 可赋给 expected=new → other.bases 含 new（适配器 itable）
            if self.variance_args_compatible(template, new_args, &other_args) {
                Self::push_variance_adapter_base(&mut self.registry, &other_name, new_mono);
            }
            if self.variance_args_compatible(template, &other_args, new_args) {
                Self::push_variance_adapter_base(&mut self.registry, new_mono, &other_name);
            }
        }
        Ok(())
    }

    fn push_variance_adapter_base(registry: &mut crate::TypeRegistry, onto: &str, view: &str) {
        let Some(ty) = registry.types.get_mut(onto) else {
            return;
        };
        let view_id: Ident = view.into();
        if !ty.bases.contains(&view_id) {
            ty.bases.push(view_id);
        }
    }

    fn variance_args_compatible(
        &self,
        template: &Ident,
        expected_args: &[TypeId],
        found_args: &[TypeId],
    ) -> bool {
        if expected_args.len() != found_args.len() {
            return false;
        }
        let variances: Vec<Variance> = self
            .interface_templates
            .get(template)
            .map(|d| d.generics.iter().map(|p| p.variance).collect())
            .unwrap_or_else(|| vec![Variance::Invariant; expected_args.len()]);
        if variances.len() != expected_args.len() {
            return false;
        }
        expected_args
            .iter()
            .zip(found_args.iter())
            .zip(variances.iter())
            .all(|((e, f), v)| match v {
                Variance::Invariant => e == f || self.types_compatible(e, f),
                Variance::Covariant => self.is_subtype(f, e),
                Variance::Contravariant => self.is_subtype(e, f),
            })
    }

    /// 同模板单态化名之间的 variance 赋值兼容（`IGetter_Dog` → `IGetter_Animal`）。
    pub(crate) fn variance_compatible_named(&self, expected: &Ident, found: &Ident) -> bool {
        let Some((e_tmpl, e_args)) = self.mono_origins.get(expected.as_str()) else {
            return false;
        };
        let Some((f_tmpl, f_args)) = self.mono_origins.get(found.as_str()) else {
            return false;
        };
        if e_tmpl != f_tmpl || e_args.len() != f_args.len() {
            return false;
        }
        self.variance_args_compatible(e_tmpl, e_args, f_args)
    }
}

/// Builtin primitive types that satisfy common interfaces without registration.
fn is_builtin_primitive(ty: &TypeId) -> bool {
    matches!(
        ty,
        TypeId::Int
            | TypeId::Long
            | TypeId::Short
            | TypeId::Byte
            | TypeId::Char
            | TypeId::Float
            | TypeId::Double
            | TypeId::Bool
            | TypeId::String
            | TypeId::UInt
            | TypeId::ULong
            | TypeId::UShort
            | TypeId::SByte
    )
}

/// Builtin interfaces that primitives naturally implement (compiler semantic rule).
///
/// C# primitives implement `IComparable`, `IComparable<T>`, `IEquatable<T>`.
/// The compiler treats these as satisfied for any primitive arg, avoiding the
/// need to register primitives as NominalType in the registry.
///
/// RFC 004 M1：扩展支持 `INumber<T>`/`IHashable<T>`/`IAddable<T>` 等
/// 数值/哈希接口。`INumber<T>` 仅数值基元（int/long/short/byte/float/double）
/// 满足；其他接口所有基元（含 bool/char/string）均满足。
///
/// 精确匹配内置接口名（非 starts_with 前缀匹配），避免误判
/// `IComparableThing` 这类用户定义类型为内置接口。
/// 泛型 mangle 形态（如 `IComparable_int`）需校验后缀为已知基元类型。
fn is_primitive_satisfiable_interface(bound: &TypeId) -> bool {
    let name = match bound {
        TypeId::Named(n) => n.as_str(),
        _ => return false,
    };
    // 非泛型形态：精确匹配
    name == "IComparable"
        || name == "IEquatable"
        // 泛型 mangle 形态：后缀必须为已知基元类型，防止 `IComparable_Widget` 误判
        || (name.starts_with("IComparable_")
            && is_known_primitive_mangle_suffix(&name["IComparable_".len()..]))
        || (name.starts_with("IEquatable_")
            && is_known_primitive_mangle_suffix(&name["IEquatable_".len()..]))
        || (name.starts_with("IHashable_")
            && is_known_primitive_mangle_suffix(&name["IHashable_".len()..]))
        // RFC 004 M1：INumber<T>/IAddable<T>/ISubtractable<T>/IMultiplicable<T>/IDivisible<T>
        // 仅数值基元满足（不含 bool/char/string）。
        || (name.starts_with("INumber_")
            && is_numeric_primitive_mangle_suffix(&name["INumber_".len()..]))
        || (name.starts_with("IAddable_")
            && is_numeric_primitive_mangle_suffix(&name["IAddable_".len()..]))
        || (name.starts_with("ISubtractable_")
            && is_numeric_primitive_mangle_suffix(&name["ISubtractable_".len()..]))
        || (name.starts_with("IMultiplicable_")
            && is_numeric_primitive_mangle_suffix(&name["IMultiplicable_".len()..]))
        || (name.starts_with("IDivisible_")
            && is_numeric_primitive_mangle_suffix(&name["IDivisible_".len()..]))
}

/// 已知基元类型的 mangle 后缀（与 `is_builtin_primitive` 列表一致）。
///
/// 包含 `string`：虽然 string 在 C# 中为引用类型，但作为泛型 mangle 后缀
/// 时仍属编译器认可的基元形态（`IComparable_string` 合法）。
fn is_known_primitive_mangle_suffix(suffix: &str) -> bool {
    matches!(
        suffix,
        "int"
            | "long"
            | "short"
            | "byte"
            | "char"
            | "float"
            | "double"
            | "bool"
            | "string"
            | "uint"
            | "ulong"
            | "ushort"
            | "sbyte"
    )
}

/// RFC 004 M1：数值基元类型的 mangle 后缀（不含 bool/char/string）。
///
/// `INumber<T>`/`IAddable<T>` 等数值运算接口仅数值基元满足；
/// `bool`/`char`/`string` 不支持加减乘除运算。
fn is_numeric_primitive_mangle_suffix(suffix: &str) -> bool {
    matches!(
        suffix,
        "int" | "long" | "short" | "byte" | "float" | "double" | "uint" | "ulong" | "ushort"
    )
}

#[derive(Clone, Copy)]
enum VariancePosition {
    Input,
    Output,
}

fn flip_variance_pos(pos: VariancePosition) -> VariancePosition {
    match pos {
        VariancePosition::Input => VariancePosition::Output,
        VariancePosition::Output => VariancePosition::Input,
    }
}

fn collect_type_param_refs(ty: &Type) -> Vec<Ident> {
    let mut out = Vec::new();
    collect_type_param_refs_inner(ty, &mut out);
    out
}

fn collect_type_param_refs_inner(ty: &Type, out: &mut Vec<Ident>) {
    match ty {
        Type::Named { path, generics } => {
            if generics.is_empty() && path.len() == 1 {
                out.push(path[0].clone());
            }
            for g in generics {
                collect_type_param_refs_inner(&g.node, out);
            }
        }
        Type::Array { inner } | Type::Nullable { inner } => {
            collect_type_param_refs_inner(&inner.node, out);
        }
        Type::Ref { inner, .. } => collect_type_param_refs_inner(&inner.node, out),
        Type::Func { params, ret } => {
            for p in params {
                collect_type_param_refs_inner(&p.node, out);
            }
            collect_type_param_refs_inner(&ret.node, out);
        }
        _ => {}
    }
}

/// 递归检测 `TypeId` 中是否包含 `TypeId::Generic(_)`。
///
/// 用于 `instantiate_generic_class`/`instantiate_generic_interface` 的
/// 早返回判定：当类型实参中嵌套有未绑定的 `Generic` 时（如
/// `List<Action<T>>` 的 `args = [Func{params:[Generic("T")],ret:Void}]`），
/// 不能进行完整实例化（否则 `substitute_class_def` 会通过
/// `type_id_to_ast` 还原类型时丢失信息），仅返回 mangled 名供后续绑定。
///
/// 仅检测直接 `Generic` 会让嵌套场景误进入完整实例化路径，产生
/// `(T) -> void` 这类非法 LLVM 标识符。
pub(crate) fn type_contains_generic(ty: &TypeId) -> bool {
    match ty {
        TypeId::Generic(_) => true,
        TypeId::Ref { inner, .. } => type_contains_generic(inner),
        TypeId::Func { params, ret } => {
            params.iter().any(type_contains_generic) || type_contains_generic(ret)
        }
        TypeId::Task { inner } => type_contains_generic(inner),
        TypeId::IEnumerable { inner } => type_contains_generic(inner),
        TypeId::IQueryable { inner } => type_contains_generic(inner),
        TypeId::Array { elem } => type_contains_generic(elem),
        TypeId::Expression { inner } => type_contains_generic(inner),
        TypeId::Nullable { inner } => type_contains_generic(inner),
        TypeId::Vector { elem, .. } => type_contains_generic(elem),
        TypeId::Span { elem, .. } => type_contains_generic(elem),
        _ => false,
    }
}
