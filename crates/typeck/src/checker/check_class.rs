use super::*;
use crate::builtin_facade::is_builtin_facade;
use crate::field_keyword::uses_field;
use crate::{AttributeTarget, BuiltinMeta};

/// CS0663 辅助：ctor 形参的 AST 类型名（Named 末段 / 基元名），供泛型模板
/// ctor 重载冲突检测的逐位置比较（模板形参名命中 class.generics）。
fn ctor_param_type_name(ty: &ast::Type) -> Ident {
    match ty {
        ast::Type::Named { path, .. } => path.last().cloned().unwrap_or_else(|| "unknown".into()),
        ast::Type::Ref { inner, .. } | ast::Type::Nullable { inner } => {
            ctor_param_type_name(&inner.node)
        }
        ast::Type::Array { inner } => format!("{}_arr", ctor_param_type_name(&inner.node)).into(),
        ast::Type::Func { .. } => "Func".into(),
        ast::Type::ConstInt(_) => "int".into(),
        ast::Type::Infer => "unknown".into(),
    }
}

/// RFC 006 A1：可见性"严格程度"序——Public 最宽、Private 最严。
/// 用于校验访问器级可见性不能比属性自身可见性更宽。
fn accessor_vis_rank(v: Visibility) -> u8 {
    match v {
        Visibility::Public => 3,
        Visibility::Internal => 2,
        Visibility::Protected => 1,
        Visibility::Private => 0,
    }
}

/// RFC 017 M4-link Phase B §D2.1：按类来源决定其成员函数的 `FnLinkage`。
///
/// - **builtin facade 类**（Console/File/MD5/...，`is_builtin_facade` 返回 true）：
///   无条件 `LinkonceOdr`——跨 `.o` 必然重复（主程序与 lib 都 `using Arc;`）。
/// - **std 库类**（`class.span.file_id` 命中 `self.std_file_ids` 集合，
///   即源码文件位于 `std/` 目录）：同样 `LinkonceOdr`，同理由跨 `.o` 重复。
/// - **其他类**（用户源码定义的 class/struct/interface）：
///   `User`→MIR `External`——单一权威定义来源，保证符号 ABI 稳定。
///
/// 单态化实例（`Box_int`/`List_string` 等）的 `LinkonceOdr` 标注由
/// `check_generics.rs` 在 `check_fn_inner(.., FnLinkage::Monomorphized)` 路径
/// 单独处理，不经过此函数。
impl TypeChecker {
    fn fn_linkage_for_class(&self, class: &ClassDef) -> FnLinkage {
        // builtin facade 列表：stub facade 类缺省 LinkonceOdr
        if is_builtin_facade(&class.name) {
            return FnLinkage::Monomorphized;
        }
        // std 库类：类名命中管线注入的 std 库类名集合 → LinkonceOdr 跨 .o 去重
        if self.std_class_names.contains(class.name.as_str()) {
            return FnLinkage::Monomorphized;
        }
        FnLinkage::User
    }

    /// 判定方法是否为 codegen stub（跳过方法体 typeck）。
    ///
    /// 1. 方法已登记在 `builtin_registry`（`[Builtin]`）→ skip。
    /// 2. 否则若类为 stub facade（含 `List_*` / `Span` / `Array` 等）→ skip。
    ///
    /// **类级回退不可删**：集合单态（如 `List_Type`）的 stub 体写 `return 0`
    /// （占位 `T`），且索引器常无 `[Builtin]`。删回退后 `using Arc` 即在
    /// Reflection mono 上爆发 `expected Type, found int`（3fc02495 回归；
    /// d3b4964c 恢复）。
    fn is_builtin_stub_method(&self, class_name: &Ident, method_name: &Ident) -> bool {
        if let Some(def_id) = self
            .member_def_ids
            .get(&(class_name.clone(), method_name.clone()))
        {
            if self.builtin_registry.contains_key(def_id) {
                return true;
            }
        }
        is_builtin_facade(class_name.as_str())
    }
}

impl TypeChecker {
    pub(crate) fn check_class(&mut self, class: &ClassDef) -> Result<(), TypeError> {
        self.validate_where_clause(&class.generics, &class.where_clause)?;
        // RFC 006 A1：访问器级可见性必须比属性自身可见性更严格（或相等），
        // 不能更宽（对齐 C# CS0275/CS0276 语义）。
        for prop in &class.properties {
            self.validate_accessor_visibility(class, prop)?;
        }
        // class/struct 禁止 variance 修饰
        for p in &class.generics {
            if p.variance != Variance::Invariant {
                return Err(TypeError::VarianceNotOnInterface);
            }
        }
        if !class.generics.is_empty() {
            self.class_templates
                .insert(class.name.clone(), class.clone());
            self.push_type_params(&class.generics);
            // RFC 004 M1：泛型类体进入时同步 push where_clause，供
            // `check_static_abstract_call` 查询 `T` 的接口约束。
            self.where_clause_scope.push(class.where_clause.clone());
            // 须在 check_class_inner 之前注册 stub：`emit_ctor_fns` 路径 type-check
            // ctor 体时经 registry.resolve_field 查字段（RFC 009 M4-1 不在
            // from_module 注册泛型模板 fields）。
            // 同时填充模板构造器签名：泛型基类（如 `DbSet<T> : EntityQueryable<T>`）
            // 的 ctor body 脱糖 `: base(...)` 经 `resolve_bind_ctor(base_name, …)`
            // 查 `registry.ctor_signatures`——模板 stub 若留空构造器，`__ctor::Base`
            // 绑定即报 `no matching constructor`（RFC 026 M0 数据源与 from_module
            // 一致，均从 `ConstructorDef.params` 提取）。仅 stub 新建时写入，
            // 不覆盖同名非泛型类的构造器元数据。
            if !self.registry.types.contains_key(&class.name) {
                self.registry.types.insert(
                    class.name.clone(),
                    NominalType {
                        name: class.name.clone(),
                        kind: TypeKind::Class,
                        vis: class.vis,
                        is_abstract: class.is_abstract,
                        is_record: class.is_record,
                        is_readonly: false,
                        generic_params: class.generics.iter().map(|p| p.name.clone()).collect(),
                        span: Span::DUMMY,
                        fields: IndexMap::new(),
                        methods: IndexMap::new(),
                        bases: Vec::new(),
                        base_types: Vec::new(),
                        variants: Vec::new(),
                        namespace: Vec::new(),
                        const_values: IndexMap::new(),
                        constructors: crate::registry::ctors_from_ast(&class.constructors),
                        soa: false,
                        required_props: Default::default(),
                    },
                );
            }
            let result = self.check_class_inner(class, false);
            self.where_clause_scope.pop();
            self.pop_type_params();
            return result;
        }
        self.check_generic_interface_impls(class)?;
        // RFC 032 v0.11: Pass 2 骨架模式——宏容器类跳过方法体检查。
        // 容器识别通过 `macro_container_names`（反向推断预计算结果），
        // 与 `collect_macros` Pass 1b 使用相同规则，避免双入口双规则。
        // 非 Skeleton 模式（Pass 4）或非容器类照常 `emit_fns=true`。
        let emit_fns = !(self.macro_pass_mode == super::MacroPassMode::Skeleton
            && self.macro_container_names.contains(&class.name));
        self.check_class_inner(class, emit_fns)
    }

    /// RFC 006 A1：校验访问器级可见性不能比属性自身可见性更宽。
    ///
    /// 访问器（get/set/init）若显式声明可见性（Some），必须严格于或等于属性
    /// 自身可见性；比属性更宽的越权声明报错（对齐 C# CS0275/CS0276 语义）。
    /// None（继承属性可见性）无需校验。
    fn validate_accessor_visibility(
        &self,
        class: &ClassDef,
        prop: &PropertyDef,
    ) -> Result<(), TypeError> {
        if let Some(v) = prop.get_vis {
            if accessor_vis_rank(v) > accessor_vis_rank(prop.vis) {
                return Err(TypeError::Oop(format!(
                    "accessor visibility of `get` on property `{}` of `{}` must be more restrictive than the property's visibility (RFC 006 A1)",
                    prop.name, class.name
                )));
            }
        }
        if let Some(v) = prop.set_vis {
            if accessor_vis_rank(v) > accessor_vis_rank(prop.vis) {
                return Err(TypeError::Oop(format!(
                    "accessor visibility of `set`/`init` on property `{}` of `{}` must be more restrictive than the property's visibility (RFC 006 A1)",
                    prop.name, class.name
                )));
            }
        }
        Ok(())
    }

    /// Check that a class properly implements its generic interface bases.
    ///
    /// For each generic interface base (e.g., `class Score : IComparable<int>`),
    /// instantiate the interface and verify the class's methods match the
    /// instantiated signatures. Non-generic interfaces are handled by
    /// `registry.validate_all`.
    ///
    /// Called for non-generic classes at definition time, and for monomorphized
    /// generic class instances (e.g., `ComparableBox_int`) at instantiation time
    /// via `instantiate_generic_class`.
    pub(crate) fn check_generic_interface_impls(
        &mut self,
        class: &ClassDef,
    ) -> Result<(), TypeError> {
        self.check_generic_interface_impls_named(&class.name, &class.bases)
    }

    /// RFC 004 / RFC 006：按类型名 + bases 校验泛型接口实现（class 与 struct 共用）。
    pub(crate) fn check_generic_interface_impls_named(
        &mut self,
        type_name: &Ident,
        bases: &[Type],
    ) -> Result<(), TypeError> {
        for base in bases {
            let Type::Named { path, generics } = base else {
                continue;
            };
            if generics.is_empty() {
                continue;
            }
            let Some(iface_name) = path.last() else {
                continue;
            };
            if !self.registry.is_generic_template(iface_name)
                || !self.registry.is_interface(iface_name)
            {
                continue;
            }
            let args: Vec<TypeId> = generics
                .iter()
                .map(|g| self.lower_type(&g.node))
                .collect::<Result<_, _>>()?;
            let inst = self.instantiate_generic_interface(iface_name, &args)?;
            let inst_name = match inst {
                TypeId::Named(ref n) => n.clone(),
                _ => continue,
            };
            // RFC 004 M2：使用 `try_check_interface_impl` 保留详细错误信息
            // （如缺失的方法名、签名不匹配详情），而非 `implements_interface`
            // 的布尔返回值导致错误被吞掉。`OopError` 的 Display 已包含
            // "class X does not implement interface Y: ..." 前缀，无需重复。
            if let Err(e) = self
                .registry
                .try_check_interface_impl(type_name, &inst_name)
            {
                return Err(TypeError::Oop(e.to_string()));
            }
        }
        Ok(())
    }

    fn eval_const_init(
        &mut self,
        init: &Option<Spanned<Expr>>,
        field_name: &Ident,
        class_name: &Ident,
    ) -> Result<ConstValue, TypeError> {
        let init = init.as_ref().ok_or_else(|| {
            TypeError::Oop(format!(
                "const field `{field_name}` on `{class_name}` must have an initializer"
            ))
        })?;
        match &init.node {
            Expr::IntLit(n) => Ok(ConstValue::Int(*n)),
            Expr::FloatLit(FloatLitValue::Double(f)) => Ok(ConstValue::Float(*f)),
            Expr::FloatLit(FloatLitValue::Float(f)) => Ok(ConstValue::Float(*f as f64)),
            Expr::BoolLit(b) => Ok(ConstValue::Bool(*b)),
            Expr::StringLit(s) => Ok(ConstValue::String(s.clone())),
            // 常量表达式：一元负号字面量（`-1` / `-0.5`）。
            Expr::Unary {
                op: UnaryOp::Neg,
                expr,
            } => match &expr.node {
                Expr::IntLit(n) => Ok(ConstValue::Int(-*n)),
                Expr::FloatLit(FloatLitValue::Double(f)) => Ok(ConstValue::Float(-*f)),
                Expr::FloatLit(FloatLitValue::Float(f)) => Ok(ConstValue::Float(-(*f as f64))),
                _ => Err(TypeError::Oop(format!(
                    "const field `{field_name}` on `{class_name}` initializer must be a constant expression"
                ))),
            },
            // 常量表达式：浮点特殊常量（C# 惯用法 `double.PositiveInfinity` 等）。
            Expr::Field { receiver, field } => {
                if let Expr::Ident(ty) = &receiver.node {
                    if matches!(ty.as_str(), "double" | "float") {
                        let v = match field.as_str() {
                            "PositiveInfinity" => Some(f64::INFINITY),
                            "NegativeInfinity" => Some(f64::NEG_INFINITY),
                            "NaN" => Some(f64::NAN),
                            _ => None,
                        };
                        if let Some(v) = v {
                            return Ok(ConstValue::Float(v));
                        }
                    }
                }
                Err(TypeError::Oop(format!(
                    "const field `{field_name}` on `{class_name}` initializer must be a constant expression"
                )))
            }
            _ => Err(TypeError::Oop(format!(
                "const field `{field_name}` on `{class_name}` initializer must be a constant expression"
            ))),
        }
    }

    pub(crate) fn check_class_inner(
        &mut self,
        class: &ClassDef,
        emit_fns: bool,
    ) -> Result<(), TypeError> {
        // 泛型模板类 Pass 2 用 `emit_fns=false` 跳过方法体，但 ctor 须进
        // `typed_fns`（`__ctor::Signal_1` 等）供 MIR `generate_generic_class_ctors`
        // 从模板克隆 mono ctor；方法体仍仅 `emit_fns` 时 emit。
        let emit_ctor_fns = emit_fns || !class.generics.is_empty();
        let skip_body = is_builtin_facade(&class.name);
        // RFC 012 M4-7: Pass 4（Full 模式）跳过属性解析——Pass 2 已完成
        // 属性注册，重复 `register` 会在 AttributeTable 中产生重复条目。
        let resolve_attrs = self.macro_pass_mode == super::MacroPassMode::Skeleton;
        // FQN 闭环（comdat 跨命名空间同名类，C# 体系方案）：碰撞输家（其 FQN 已按 FQN
        // 存于 `shadowed_types`）的类型身份（this 参数、ctor/方法 link 名、MIR 函数名、
        // 方法体解析、成员登记）统一走 FQN，与调用点 `Geo.Shape_Mul` 符号一致——否则
        // prune 报 `undefined symbol` 且成员登记误落短名胜者条目。胜者/无碰撞维持短名。
        let class_id: Ident = {
            let fqn = crate::oop_types::type_fqn(&self.enclosing_namespace, class.name.as_str());
            if self.registry.shadowed_types.contains_key(&fqn) {
                fqn.into()
            } else {
                class.name.clone()
            }
        };
        let prev = self.current_class.clone();
        self.current_class = Some(class_id.clone());
        self.scopes.push(IndexMap::new());
        // `this` in instance methods
        self.scopes
            .last_mut()
            .unwrap()
            .insert("this".into(), TypeId::Named(class_id.clone()));

        // RFC 040 §5: 非泛型类继承泛型基类实例化时，主动触发基类单态化注册。
        // 例：class Derived : Base<Concrete> 须注册 Base_Concrete 到 registry.types，
        // 否则 collect_method_overloads / inherited_field_types 沿 bases 链行走
        // 时找不到 mono 基类条目（is_class 返回 false → 链中断），方法/字段解析失败。
        // 仅非泛型类需要此处理（泛型模板自身在 instantiate_generic_class 中处理）。
        if class.generics.is_empty() {
            for base in &class.bases {
                if let ast::Type::Named { path, generics } = base {
                    if !generics.is_empty() {
                        let name = match path.last() {
                            Some(n) => n.clone(),
                            None => continue,
                        };
                        if self.registry.is_generic_template(&name) && self.registry.is_class(&name)
                        {
                            let args: Vec<TypeId> = generics
                                .iter()
                                .map(|g| self.lower_type(&g.node))
                                .collect::<Result<_, _>>()?;
                            self.instantiate_generic_class(&name, &args)?;
                        }
                    }
                }
            }
        }

        // RFC 012 M1: 收集 class 自身属性并注册到 attribute_table。
        // 分配 DefId 并填入 `class_def_ids` 反查表，供外部消费者按类名查询。
        // RFC 012 M3: 使用 `ensure_class_def_id` 复用前向引用时预分配的 DefId，
        // 保证 `attr_type` 反查链一致。
        // RFC 009 M4-1: 传入 `generic_arity` 以支持同名类按 arity 重载。
        // RFC 009 M4-7: Pass 4 跳过属性解析（Pass 2 已完成，避免重复注册）。
        if resolve_attrs {
            let class_def_id = self.ensure_class_def_id(&class.name, class.generics.len());
            self.resolve_attributes(
                &class.attributes,
                AttributeTarget::Class,
                class_def_id,
                None,
            );
        }

        for (fname, fty) in self.inherited_field_types(&class.name) {
            if !self.scopes.last().unwrap().contains_key(&fname) {
                self.scopes.last_mut().unwrap().insert(fname, fty);
            }
        }
        for f in &class.fields {
            // RFC 012 M1: 收集 field 属性（含 [Column] / [Key] / [Required] / [MaxLength]）。
            // RFC 012 M4-7: Pass 4 跳过属性解析。
            if resolve_attrs && !f.attributes.is_empty() {
                let field_def_id = self.alloc_member_def_id(&class.name, &f.name);
                self.resolve_attributes(&f.attributes, AttributeTarget::Field, field_def_id, None);
            }
            let fty = self.lower_type(&f.ty.node)?;
            self.scopes
                .last_mut()
                .unwrap()
                .insert(f.name.clone(), fty.clone());
            // 泛型模板不在 from_module 注册 fields（RFC 009 M4-1）；ctor 体
            // type-check 经 registry.resolve_field，须同步实例字段表。
            if !class.generics.is_empty() {
                if let Some(nom) = self.registry.types.get_mut(&class.name) {
                    // 同名非泛型类共享 `registry.types` key（如泛型模板
                    // `DependencyProperty<T>` 与非泛型基类 `DependencyProperty`）。
                    // 仅当该条目确为模板自身 stub（`check_class` 创建的
                    // generic_params 非空条目）时同步字段——否则会把模板的
                    // `T` 型字段（如 `DefaultValue`）写入同名具体类，污染其字段表，
                    // 使 `collect_fields` 沿基类链读到 `T` 型字段而跳过 mono 类
                    // 已替换的 `double` 字段（对齐下方 `write_ctors` 的同名碰撞规避）。
                    if !nom.generic_params.is_empty() {
                        nom.fields.insert(
                            f.name.clone(),
                            FieldInfo {
                                name: f.name.clone(),
                                // RFC 044 M2：Infer 字段（var 提升）注册哨兵，
                                // 由赋值点推断回填（见 check_stmt）。
                                ty: if fty == TypeId::Infer {
                                    "__infer__".into()
                                } else {
                                    type_id_to_field_name(&fty)
                                },
                                vis: f.vis,
                                is_const: f.is_const,
                                is_readonly: f.is_readonly,
                                is_init_only: false,
                                get_vis: None,
                                set_vis: None,
                                is_static: f.is_static,
                                init: if f.is_static && !f.is_const {
                                    f.init.clone()
                                } else {
                                    None
                                },
                            },
                        );
                    }
                }
            }

            if f.is_const {
                let cv = self.eval_const_init(&f.init, &f.name, &class.name)?;
                if let Some(nom) = self.registry.types.get_mut(&class.name) {
                    nom.const_values.insert(f.name.clone(), cv);
                }
            }
        }
        for prop in &class.properties {
            // RFC 012 M1: 收集 property 属性（property 上的 [Column] 等）。
            // RFC 012 M4-7: Pass 4 跳过属性解析。
            if resolve_attrs && !prop.attributes.is_empty() {
                let prop_def_id = self.alloc_member_def_id(&class.name, &prop.name);
                self.resolve_attributes(
                    &prop.attributes,
                    AttributeTarget::Property,
                    prop_def_id,
                    Some(prop),
                );
            }
            // 访问器形态判定走单一事实源（registry.rs `property_has_custom_accessors`）。
            let is_custom = crate::registry::property_has_custom_accessors(prop);
            // RFC 006 A2：访问器体引用 `field` 的属性虽为 custom（有 get/set 方法），
            // 但仍"自动"——需为它合成 backing field（名={Prop}__backing，与属性名
            // 区分），与 auto 属性同路径注册。
            let uses_field = is_custom && uses_field(&prop.get_body, &prop.set_body);
            if is_custom && !uses_field {
                continue;
            }
            let pty = self.lower_type(&prop.ty.node)?;
            // 仅 auto 属性把属性名放入类作用域（裸名可用）；field 属性用合成 backing
            // 名，不放作用域（访问器经 `this.<backing>` 显式访问，见 field_keyword.rs）。
            if !uses_field {
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(prop.name.clone(), pty.clone());
            }
            if !class.generics.is_empty() {
                if let Some(nom) = self.registry.types.get_mut(&class.name) {
                    // 同名非泛型类碰撞规避（见上方实例字段同步的同名注释）。
                    if !nom.generic_params.is_empty() {
                        let fname = if uses_field {
                            crate::field_keyword::backing_field_name(&prop.name)
                        } else {
                            prop.name.clone()
                        };
                        nom.fields.insert(
                            fname.clone(),
                            FieldInfo {
                                name: fname,
                                ty: type_id_to_field_name(&pty),
                                vis: prop.vis,
                                is_const: false,
                                is_readonly: false,
                                is_init_only: prop.has_init && !prop.has_set,
                                get_vis: prop.get_vis,
                                set_vis: prop.set_vis,
                                is_static: false,
                                init: None,
                            },
                        );
                    }
                }
            }
        }

        // RFC 009 M4-2: `from_module` 已用 AST 估算值预填 `nom.constructors`
        // （供 `validate_all` 早期 `new()` 约束查询）。这里清空后由 typeck
        // 精确填值（基于已 lower 的 `TypeId`，避免重复追加导致计数翻倍）。
        //
        // 泛型模板类（`class.generics` 非空）不进 `registry.types`（RFC 012
        // M4-1：模板存 `class_templates`，注册时经 mangle 名实例化）——其
        // ctor 写入须跳过，否则同名非泛型类的构造器元数据会被模板覆盖
        // （如 `InjectAttribute<T>` 覆盖 `InjectAttribute`，导致属性构造器
        // 匹配失败）。
        let write_ctors = class.generics.is_empty();
        if write_ctors {
            if let Some(nom) = self.registry.types.get_mut(&class.name) {
                nom.constructors.clear();
            }
        }

        // RFC 023 已知缺口收口 · CS0663 对齐（C#）：泛型模板类 ctor 重载「仅以
        // 类型形参与具体类型区分」在实例化后可能签名冲突——`C<T>(T)` + `C(int)`
        // 实例化 `C<int>` 后两 ctor 均 (int)，mangle 消歧在替换后失效（符号碰撞、
        // 后声明者覆盖 → 调用方按错误签名执行，探针实证垃圾值）。C# 在声明处报
        // CS0663；Arc 对齐报编译错误（单一惯用法：合法代码不受影响）。
        if !class.generics.is_empty() {
            for (i, ca) in class.constructors.iter().enumerate() {
                for cb in class.constructors.iter().skip(i + 1) {
                    let a = &ca.node.params;
                    let b = &cb.node.params;
                    if a.len() != b.len() {
                        continue;
                    }
                    let mut differs = 0usize;
                    let mut consistent = true;
                    for k in 0..a.len() {
                        let an = ctor_param_type_name(&a[k].ty.node);
                        let bn = ctor_param_type_name(&b[k].ty.node);
                        if an == bn {
                            continue;
                        }
                        differs += 1;
                        let a_is_param = class.generics.iter().any(|g| g.name == an);
                        let b_is_param = class.generics.iter().any(|g| g.name == bn);
                        if a_is_param == b_is_param {
                            // 两侧同为类型形参或同为具体类型：实例化后可能仍不同
                            // （`C(T1)` vs `C(T2)` 对齐 C# 合法），或属重复定义
                            // （另有检测），不按 CS0663 冲突处理。
                            consistent = false;
                            break;
                        }
                    }
                    if consistent && differs > 0 {
                        return Err(TypeError::Oop(format!(
                            "CS0663: generic class `{}` cannot define overloaded constructors that \
                             differ only on type parameter vs concrete type positions; \
                             instantiation would collide (arity {})",
                            class.name, a.len()
                        )));
                    }
                }
            }
        }

        for ctor in &class.constructors {
            self.scopes.push(IndexMap::new());
            self.scopes
                .last_mut()
                .unwrap()
                .insert("this".into(), TypeId::Named(class_id.clone()));
            let mut ctor_params = vec![("this".into(), TypeId::Named(class_id.clone()))];
            // 同步收集参数类型名，用于填充 NominalType.constructors（new() 约束校验）
            let mut ctor_param_type_names: Vec<Ident> = Vec::new();
            for p in &ctor.node.params {
                let pty = self.lower_type(&p.ty.node)?;
                // 与方法一致：`ref`/`out` → mutable Ref；`in` → readonly Ref。
                // primary ctor 的 by-ref 形参不捕获为字段，但仍须按指针 ABI 进入 MIR/codegen。
                let final_ty = if p.is_in {
                    TypeId::Ref {
                        inner: Box::new(pty.clone()),
                        mutable: false,
                        kind: ast::RefKind::Var,
                    }
                } else if p.is_ref || p.is_out {
                    TypeId::Ref {
                        inner: Box::new(pty.clone()),
                        mutable: true,
                        kind: ast::RefKind::Var,
                    }
                } else {
                    pty.clone()
                };
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(p.name.clone(), final_ty.clone());
                ctor_params.push((p.name.clone(), final_ty));
                let mangled = type_id_to_field_name(&pty);
                ctor_param_type_names.push(mangled);
            }
            // RFC 007 M2：构造器可选参数后缀规则（与方法/自由函数同形）。
            self.validate_params_m2b(&ctor.node.params)?;
            // 构造器 body 脱糖（仅 emit_fns && !skip_body）：
            // 1. `: base(args)` → 前置 `__ctor::Base` 调用（RFC 009 L1）
            //    RFC 007 M2b：`: base(...)` 与 `new` 同形——命名/可选实参绑定后填满位置列表。
            // 2. 实例字段初始化器 → base 之后、用户 body 之前（C# 语义；
            //    跨文件 partial 合并后字段列表已含两侧声明，见 RFC 037）
            // typeck 识别 `__ctor::` 前缀并跳过 resolve；mir/codegen 自然处理。
            let body_with_base: Block = if emit_ctor_fns && !skip_body {
                let mut stmts: Vec<Spanned<Stmt>> = Vec::new();
                if let Some(base_args) = ctor.node.base_args.as_ref() {
                    let base_name = class
                        .bases
                        .iter()
                        .find_map(|b| match b {
                            ast::Type::Named { path, .. } => path.last().cloned(),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            TypeError::Oop(format!(
                                "class `{}` has `: base(...)` initializer but no base class",
                                class.name
                            ))
                        })?;
                    // 与 `new Base(...)` 同路径：命名/可选 → 完整位置实参。
                    let (sig, bound_base_args, _params_span) =
                        self.resolve_bind_ctor(&base_name, base_args)?;
                    let mut call_args = vec![Spanned::new(Expr::Ident("this".into()), Span::DUMMY)];
                    call_args.extend(bound_base_args.iter().cloned());
                    let base_arity = bound_base_args.len();
                    // RFC 023 已知缺口收口：base 调用符号按签名消歧（与 `new`
                    // 路径 resolve_ctor_params 同规则）——同 arity 不同类型参数的
                    // ctor 重载若不消歧，`__ctor::Base_1` 被后声明者覆盖，
                    // `: base(...)` 按错误签名执行（探针实证 @__ctor_Base_1
                    // undefined symbol）。`ctor_link_name` 无参返回 `__ctor::Base`。
                    let base_collision = self
                        .registry
                        .ctor_signatures(&base_name)
                        .iter()
                        .filter(|c| c.param_types.len() == base_arity)
                        .count()
                        > 1;
                    let base_ctor_ident = crate::oop_types::ctor_link_name(
                        &base_name,
                        &sig.param_types,
                        base_collision,
                    );
                    let base_call = Spanned::new(
                        Expr::Call {
                            func: Box::new(Spanned::new(
                                Expr::Ident(base_ctor_ident.into()),
                                Span::DUMMY,
                            )),
                            args: call_args,
                            type_args: vec![],
                            params_span: None,
                        },
                        Span::DUMMY,
                    );
                    stmts.push(Spanned::new(Stmt::Expr(base_call), Span::DUMMY));
                }
                stmts.extend(instance_field_init_stmts(class));
                stmts.extend(ctor.node.body.stmts.iter().cloned());
                Block {
                    stmts,
                    tail: ctor.node.body.tail.clone(),
                }
            } else {
                ctor.node.body.clone()
            };
            let typed_body = if emit_ctor_fns && !skip_body {
                self.return_slot.push(TypeId::Void);
                let prev_in_ctor = self.in_ctor;
                self.in_ctor = true;
                let out_params: IndexSet<Ident> = ctor
                    .node
                    .params
                    .iter()
                    .filter(|p| p.is_out)
                    .map(|p| p.name.clone())
                    .collect();
                let prev_flow = self.out_flow.take();
                self.out_flow = if out_params.is_empty() {
                    None
                } else {
                    Some(OutParamState::new(out_params))
                };
                // ctor 是实例方法；嵌套 `instantiate_generic_class` 时外层
                // static 方法的 `current_fn_is_static` 须在此隔离，否则 ctor 体
                // 读实例字段（如 `Value = initial`）误报 static 违规。
                let prev_fn_static = self.current_fn_is_static;
                self.current_fn_is_static = false;
                let typed_body = match self.check_block(&body_with_base, &TypeId::Void) {
                    Ok(tb) => {
                        if let Some(flow) = &self.out_flow {
                            let missing = flow.unassigned();
                            if !missing.is_empty() {
                                self.current_fn_is_static = prev_fn_static;
                                self.out_flow = prev_flow;
                                self.in_ctor = prev_in_ctor;
                                self.return_slot.pop();
                                self.scopes.pop();
                                self.current_class = prev;
                                return Err(TypeError::Oop(format!(
                                    "out parameter `{}` must be assigned before control leaves the current method",
                                    missing[0]
                                )));
                            }
                        }
                        self.out_flow = prev_flow;
                        tb
                    }
                    Err(e) => {
                        self.current_fn_is_static = prev_fn_static;
                        self.out_flow = prev_flow;
                        self.in_ctor = prev_in_ctor;
                        self.return_slot.pop();
                        self.scopes.pop();
                        self.current_class = prev;
                        return Err(e);
                    }
                };
                self.current_fn_is_static = prev_fn_static;
                self.in_ctor = prev_in_ctor;
                self.return_slot.pop();
                Some(typed_body)
            } else {
                None
            };
            self.scopes.pop();
            // 填充 NominalType.constructors 元数据，供 new() 约束与 RFC 007 绑定。
            // 无参构造的 param_types 为空 vec；new() 约束据此判定 public 无参构造是否存在。
            // 预折叠默认值，避免 get_mut(registry) 与 fold 同时借用。
            let ctor_defaults: Vec<Option<crate::oop_types::ConstValue>> = ctor
                .node
                .params
                .iter()
                .map(|p| {
                    p.default
                        .as_ref()
                        .and_then(|e| self.fold_param_default_expr(&e.node))
                })
                .collect();
            if write_ctors {
                if let Some(nom) = self.registry.nominal_mut(&class_id) {
                    let mut ctor_params_sig: Vec<ParamSig> =
                        Vec::with_capacity(ctor.node.params.len());
                    for (i, p) in ctor.node.params.iter().enumerate() {
                        ctor_params_sig.push(ParamSig {
                            name: p.name.clone(),
                            ty: ctor_param_type_names[i].clone(),
                            is_ref: p.is_ref,
                            is_out: p.is_out,
                            is_in: p.is_in,
                            is_params: p.is_params,
                            default: ctor_defaults[i].clone(),
                        });
                    }
                    // 去重：`from_module` 的 register_class 已按 `ctors_from_ast`
                    // 填充同签名构造器，此处再 push 会造成双登记（碰撞输家的
                    // `nominal_mut` 落其自身条目后由 2→4 触发 ambiguous overload）。
                    // 相同 param_types 视为同一构造器，跳过重复追加。
                    let already = nom
                        .constructors
                        .iter()
                        .any(|c| c.param_types == ctor_param_type_names);
                    if !already {
                        nom.constructors.push(CtorSig {
                            vis: ctor.node.vis,
                            param_types: ctor_param_type_names.clone(),
                            params: ctor_params_sig,
                            sets_required_members: crate::registry::members_assigned_in_ctor_body(
                                &ctor.node.body,
                            ),
                        });
                    }
                }
            }
            if emit_ctor_fns {
                // ctor 重载 mangle：无参 ctor 保持 `__ctor::Class`（兼容 emit_new
                // 的 `new Class()` 路径）；有参 ctor 默认 `__ctor::Class_<arity>`。
                // 当存在同参数量、不同类型参数的 ctor 重载（`C(int)` / `C(string)`）
                // 时按签名追加类型名消歧——否则两 ctor 符号碰撞、后者覆盖前者，
                // 调用方按错误签名执行 → AV。与 codegen 调用点经 `ctor_link_name`
                // 共享同一 mangle 决策，保证定义/调用符号一致。
                let ctor_arity = ctor_params.len().saturating_sub(1); // 减去 this
                let ctor_collision = ctor_arity > 0
                    && class
                        .constructors
                        .iter()
                        .filter(|c| c.node.params.len() == ctor_arity)
                        .count()
                        > 1;
                let ctor_name: Ident = crate::oop_types::ctor_link_name(
                    class_id.as_str(),
                    &ctor_param_type_names,
                    ctor_collision,
                )
                .into();
                self.push_typed_fn(
                    ctor_name,
                    Some(class_id.clone()),
                    true,
                    ctor_params,
                    TypeId::Void,
                    Some(body_with_base),
                    typed_body,
                    false,
                    self.fn_linkage_for_class(class),
                    false,
                    // RFC 009 M3：构造函数不支持 `[Parallelize]` 属性，恒为 false。
                    false,
                );
            }
        }

        // 无显式构造函数但有实例字段初始化器时，合成无参 `__ctor::Class`
        // （否则 codegen 只发空 stub，`_maxValue = 100` 等非零初值永不执行——
        // 跨文件 partial UnitTest 曾因此失败）。
        if emit_ctor_fns && !skip_body && class.constructors.is_empty() {
            let field_inits = instance_field_init_stmts(class);
            if !field_inits.is_empty() {
                let body = Block {
                    stmts: field_inits,
                    tail: None,
                };
                self.scopes.push(IndexMap::new());
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert("this".into(), TypeId::Named(class_id.clone()));
                let ctor_params = vec![("this".into(), TypeId::Named(class_id.clone()))];
                self.return_slot.push(TypeId::Void);
                let prev_in_ctor = self.in_ctor;
                self.in_ctor = true;
                let prev_fn_static = self.current_fn_is_static;
                self.current_fn_is_static = false;
                let typed_body = match self.check_block(&body, &TypeId::Void) {
                    Ok(tb) => tb,
                    Err(e) => {
                        self.current_fn_is_static = prev_fn_static;
                        self.in_ctor = prev_in_ctor;
                        self.return_slot.pop();
                        self.scopes.pop();
                        self.current_class = prev;
                        return Err(e);
                    }
                };
                self.current_fn_is_static = prev_fn_static;
                self.in_ctor = prev_in_ctor;
                self.return_slot.pop();
                self.scopes.pop();
                if write_ctors {
                    if let Some(nom) = self.registry.nominal_mut(&class_id) {
                        // 去重（同前）：合成无参 ctor 与已登记的无参 ctor 避免双登记。
                        let already = nom.constructors.iter().any(|c| c.param_types.is_empty());
                        if !already {
                            nom.constructors.push(CtorSig {
                                vis: Visibility::Public,
                                param_types: vec![],
                                params: vec![],
                                sets_required_members: Default::default(),
                            });
                        }
                    }
                }
                let ctor_name: Ident = format!("__ctor::{}", class_id).into();
                self.push_typed_fn(
                    ctor_name,
                    Some(class_id.clone()),
                    true,
                    ctor_params,
                    TypeId::Void,
                    Some(body),
                    Some(typed_body),
                    false,
                    self.fn_linkage_for_class(class),
                    false,
                    false,
                );
            }
        }

        for method in &class.methods {
            let m = &method.node;
            // RFC 004 M2：static 方法不再跳过——降级到 typed_fns 以支持
            // `T.Add(a, b)` 路由到 `@Class_Add` 符号。static 方法无 `this`
            // 参数与 `this` 作用域，其余检查与实例方法一致。
            let is_static = m.sig.modifier == MethodModifier::Static;
            // RFC 012 M1: 先解析/登记 [Builtin]，再决定 skip_body——避免
            // 「尚未入 registry → 误检 stub 体」的次序债。
            // RFC 012 M4-7: Pass 4 跳过属性解析。
            if resolve_attrs && !m.sig.attributes.is_empty() {
                let method_def_id = self.alloc_member_def_id(&class.name, &m.sig.name);
                self.resolve_attributes(
                    &m.sig.attributes,
                    AttributeTarget::Method,
                    method_def_id,
                    None,
                );
                let attrs = self.attribute_table.get_attrs(method_def_id);
                for attr in attrs {
                    if attr.name.as_str() == "Builtin" {
                        let abi = attr
                            .named_args
                            .iter()
                            .find(|(n, _)| n.as_str() == "ABI")
                            .and_then(|(_, v)| match v {
                                crate::ResolvedArg::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        self.builtin_registry
                            .insert(method_def_id, BuiltinMeta { abi });
                    }
                }
            }
            let method_skip_body = self.is_builtin_stub_method(&class.name, &m.sig.name);
            self.scopes.push(IndexMap::new());
            // RFC 004 补齐：实例类方法级泛型参数（如 `UnaryCall<TResp>() where TResp : IMessage, new()`）
            // 亦须注册到 type_param_scope 与 where_clause_scope，否则方法体内裸用 `TResp` 作类型
            // （局部变量注解等）报 `OOP: undefined type TResp`（与 static class 方法级泛型同构，
            // 见 check_static_class；此处覆盖实例类方法）。
            let has_method_generics = !m.sig.generics.is_empty();
            if has_method_generics {
                self.push_type_params(&m.sig.generics);
                self.where_clause_scope.push(m.sig.where_clause.clone());
            }
            let mut method_params: Vec<(Ident, TypeId)> = Vec::new();
            if !is_static {
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert("this".into(), TypeId::Named(class_id.clone()));
                method_params.push(("this".into(), TypeId::Named(class_id.clone())));
            }
            for p in &m.sig.params {
                let pty = self.lower_type(&p.ty.node)?;
                // RFC 009 P1-F #8：`in` 参数为 `readonly ref`——`mutable: false`。
                let final_ty = if p.is_in {
                    TypeId::Ref {
                        inner: Box::new(pty),
                        mutable: false,
                        kind: ast::RefKind::Var,
                    }
                } else if p.is_ref || p.is_out {
                    TypeId::Ref {
                        inner: Box::new(pty),
                        mutable: true,
                        kind: ast::RefKind::Var,
                    }
                } else {
                    pty
                };
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(p.name.clone(), final_ty.clone());
                method_params.push((p.name.clone(), final_ty));
            }
            // RFC 007：方法形参可选后缀 / 默认值常量性（含 M2b）。
            self.validate_params_m2b(&m.sig.params)?;
            // RFC 005：`params` 仅允许 Span/ROS（禁止 `params T[]`）。
            for p in &m.sig.params {
                if p.is_params {
                    let pty = self.lower_type(&p.ty.node)?;
                    let canon = self.canonical_type(&pty);
                    if !matches!(canon, TypeId::Span { .. } | TypeId::Array { .. }) {
                        self.scopes.pop();
                        if has_method_generics {
                            self.pop_type_params();
                            self.where_clause_scope.pop();
                        }
                        self.current_class = prev;
                        return Err(TypeError::Oop(
                            "`params` requires `Span<T>`, `ReadOnlySpan<T>`, or `T[]`".into(),
                        ));
                    }
                }
            }
            let ret = self.check_method_return(m.sig.ret.as_ref(), m.sig.is_async)?;
            if m.sig.is_async && !ret.is_task() {
                self.scopes.pop();
                if has_method_generics {
                    self.pop_type_params();
                    self.where_clause_scope.pop();
                }
                self.current_class = prev;
                return Err(TypeError::AsyncReturn(ret.display()));
            }
            if m.sig.is_async && m.sig.params.iter().any(|p| p.is_ref || p.is_out || p.is_in) {
                self.scopes.pop();
                if has_method_generics {
                    self.pop_type_params();
                    self.where_clause_scope.pop();
                }
                self.current_class = prev;
                return Err(TypeError::Oop(
                    "ref/out/in parameters are not allowed in async methods".into(),
                ));
            }
            // Body check errors are pushed to self.errors but don't prevent
            // symbol registration.  push_typed_fn below receives None as
            // typed_body, which codegen handles gracefully.
            let typed_body = if emit_fns && !method_skip_body {
                let body_expected = self.body_return_slot(&ret, m.sig.is_async);
                let prev_async = self.in_async;
                self.in_async = m.sig.is_async;
                self.return_slot.push(body_expected.clone());
                let out_params: IndexSet<Ident> = m
                    .sig
                    .params
                    .iter()
                    .filter(|p| p.is_out)
                    .map(|p| p.name.clone())
                    .collect();
                let prev_flow = self.out_flow.take();
                self.out_flow = if out_params.is_empty() {
                    None
                } else {
                    Some(OutParamState::new(out_params))
                };
                let typed_body = if let Some(body) = &m.body {
                    // RFC 006 M2：进入方法体前设置 current_fn_is_static，
                    // check_expr_inner 据此拦截静态方法内访问实例字段。
                    let prev_fn_static = self.current_fn_is_static;
                    self.current_fn_is_static = is_static;
                    let result = self.check_block(body, &body_expected);
                    self.current_fn_is_static = prev_fn_static;
                    match result {
                        Ok(tb) => Some(tb),
                        Err(e) => {
                            self.errors.push(e);
                            None
                        }
                    }
                } else {
                    None
                };
                if let Some(flow) = &self.out_flow {
                    // 无方法体（abstract/extern）不离开任何控制流，out 形参
                    // 确定性赋值检查仅对有体方法生效——否则泛型基类的抽象
                    // out 形参方法（如 ChannelReader<T>.TryRead）在单态化
                    // 注册期被误判失败，级联拖垮整个类图。
                    let missing = if m.body.is_some() {
                        flow.unassigned()
                    } else {
                        Vec::new()
                    };
                    if !missing.is_empty() {
                        self.out_flow = prev_flow;
                        self.return_slot.pop();
                        self.in_async = prev_async;
                        self.scopes.pop();
                        if has_method_generics {
                            self.pop_type_params();
                            self.where_clause_scope.pop();
                        }
                        self.current_class = prev;
                        return Err(TypeError::Oop(format!(
                            "out parameter `{}` must be assigned before control leaves the current method",
                            missing[0]
                        )));
                    }
                }
                self.out_flow = prev_flow;
                self.return_slot.pop();
                self.in_async = prev_async;
                typed_body
            } else {
                None
            };
            self.scopes.pop();
            if has_method_generics {
                self.pop_type_params();
                self.where_clause_scope.pop();
            }
            if emit_fns {
                // Register the function even if its body check failed — the
                // signature is valid and other methods (e.g. Main) need to
                // resolve calls to it.  A body error only means codegen will
                // see a None typed_body, not that the symbol is absent.
                let checked_body = typed_body;
                let mut oop_params = Vec::new();
                for p in &m.sig.params {
                    let pty = type_id_to_field_name(
                        &method_params
                            .iter()
                            .find(|(n, _)| n == &p.name)
                            .map(|(_, t)| t.clone())
                            .unwrap_or(TypeId::Int),
                    );
                    oop_params.push(ParamSig {
                        name: p.name.clone(),
                        ty: pty,
                        is_ref: p.is_ref,
                        is_out: p.is_out,
                        is_in: p.is_in,
                        is_params: p.is_params,
                        default: p
                            .default
                            .as_ref()
                            .and_then(|e| self.fold_param_default_expr(&e.node)),
                    });
                }
                let oop_ret = type_id_to_field_name(&ret);
                let oop_sig = OopMethodSig {
                    name: m.sig.name.clone(),
                    vis: m.sig.vis,
                    params: oop_params,
                    ret: oop_ret,
                    modifier: m.sig.modifier,
                    is_async: m.sig.is_async,
                    generics: m.sig.generics.iter().map(|g| g.name.clone()).collect(),
                    is_static_abstract: m.sig.is_static_abstract,
                };
                let static_count = class
                    .methods
                    .iter()
                    .filter(|other| {
                        other.node.sig.name == m.sig.name
                            && matches!(other.node.sig.modifier, MethodModifier::Static)
                    })
                    .count()
                    .max(
                        self.registry
                            .method_overload_count_kind(&class_id, &m.sig.name, true),
                    );
                let instance_count = class
                    .methods
                    .iter()
                    .filter(|other| {
                        other.node.sig.name == m.sig.name
                            && !matches!(other.node.sig.modifier, MethodModifier::Static)
                    })
                    .count()
                    .max(
                        self.registry
                            .method_overload_count_kind(&class_id, &m.sig.name, false),
                    );
                let method_name: Ident = if static_count > 0 && instance_count > 0 {
                    method_link_name_static_abi(
                        class_id.as_str(),
                        &oop_sig,
                        static_count,
                        instance_count,
                    )
                    .into()
                } else {
                    let overload_count = class
                        .methods
                        .iter()
                        .filter(|other| other.node.sig.name == m.sig.name)
                        .count()
                        .max(self.registry.method_overload_count(&class_id, &m.sig.name));
                    method_link_name(class_id.as_str(), &oop_sig, overload_count).into()
                };
                // Store the DECLARED return type (Task<Void> / Task<T>),
                // not the body return slot (Void / T).  fn_returns and
                // codegen need the Task wrapper to correctly emit
                // `call ptr @Method` for async call sites.
                self.push_typed_fn(
                    method_name,
                    Some(class_id.clone()),
                    false,
                    method_params,
                    ret,
                    m.body.clone(),
                    checked_body,
                    m.sig.is_async,
                    self.fn_linkage_for_class(class),
                    is_static,
                    // RFC 009 M3：检测 `[Parallelize]` 属性，标记向量化候选。
                    Self::has_parallelize_attr(&m.sig.attributes),
                );
            }
        }

        for p in &class.properties {
            if p.has_set && p.has_init {
                self.current_class = prev;
                return Err(TypeError::Oop(format!(
                    "property `{}` cannot declare both `set` and `init`",
                    p.name
                )));
            }
            // 登记 `[Builtin]` 静态自动属性（源码形分派判定依据）。此处覆盖
            // **所有类**（含泛型模板 `Task<T>`——其静态成员不依赖类型参数，
            // 登记到模板名 `Task`；register_class 对泛型模板 early-return，
            // 若只在那里登记会漏掉 `Task.CompletedTask` → MIR 判定走真实
            // getter 路径 → fallthrough ICE：unresolved ident Task）。
            self.registry.record_builtin_static_prop(&class.name, p);
            // 访问器形态判定走单一事实源（registry.rs `property_has_custom_accessors`）。
            // 注：此处**不**附加 abstract 判定——abstract 属性无访问器体属合法
            // 形态，若按 custom 检查会在下方报 "get has no body"（注册层才附加）。
            let is_custom = crate::registry::property_has_custom_accessors(p);
            if !is_custom {
                continue;
            }
            // 索引器 auto `{ get; set; }` 允许无 body（facade / 接口实现由 codegen 拦截）。
            // `[Builtin]` 属性同样允许以 auto-property `{ get; }` 书写——访问器
            // 无独立类型检查体，由 codegen 经 get_X/set_X 拦截直射 rt_* ABI。
            let is_builtin_prop = crate::builtin_facade::is_builtin_property_attr(&p.attributes);
            if !p.is_indexer() && !is_builtin_prop {
                if p.has_get && p.get_body.is_none() {
                    self.current_class = prev;
                    return Err(TypeError::Oop(format!(
                        "property `{}` has custom accessors but `get` has no body",
                        p.name
                    )));
                }
                if p.has_set && p.set_body.is_none() {
                    self.current_class = prev;
                    return Err(TypeError::Oop(format!(
                        "property `{}` has custom accessors but `set` has no body",
                        p.name
                    )));
                }
                if p.has_init && p.set_body.is_none() && p.get_body.is_some() {
                    self.current_class = prev;
                    return Err(TypeError::Oop(format!(
                        "property `{}` has custom accessors but `init` has no body",
                        p.name
                    )));
                }
            }
            let prop_ty = self.lower_type(&p.ty.node)?;
            // RFC 006 A2：`field` 关键字——检查前把访问器体内的 `Ident("field")`
            // 重写为 `this.<backing>`（backing field 字段访问，见 field_keyword.rs）。
            // 重写后的体同时用于 check_block 与 push_typed_fn，保证 typed_body 与
            // 存储的 AST 一致（codegen 复用既有字段访问路径，无需改动）。
            let backing = crate::field_keyword::backing_field_name(&p.name);
            let get_body_rewritten = p
                .get_body
                .as_ref()
                .map(|b| crate::field_keyword::rewrite_field_block(b, &backing));
            let set_body_rewritten = p
                .set_body
                .as_ref()
                .map(|b| crate::field_keyword::rewrite_field_block(b, &backing));
            // RFC 004 M2：静态属性的 getter/setter 无 `this` 参数。
            // 原 code 无条件注入 `this`，导致 `Vector2::get_Zero` 签名为
            // `(ptr this) -> ptr`，但调用方（MIR `MirRvalue::Call`）无 `this`，
            // 触发 LLVM 参数数量不匹配。静态属性走 `MirRvalue::Call` 路径，
            // 签名必须与实例属性区分。
            let is_static_prop = p.modifier == MethodModifier::Static;
            let index_tys: Result<Vec<(Ident, TypeId)>, TypeError> = p
                .index_params
                .iter()
                .map(|ip| Ok((ip.name.clone(), self.lower_type(&ip.ty.node)?)))
                .collect();
            let index_tys = match index_tys {
                Ok(v) => v,
                Err(e) => {
                    self.current_class = prev;
                    return Err(e);
                }
            };
            if p.has_get && (p.get_body.is_some() || p.is_indexer()) {
                self.scopes.push(IndexMap::new());
                let mut get_params: Vec<(Ident, TypeId)> = Vec::new();
                if !is_static_prop {
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert("this".into(), TypeId::Named(class_id.clone()));
                    get_params.push(("this".into(), TypeId::Named(class_id.clone())));
                }
                for (n, t) in &index_tys {
                    self.scopes.last_mut().unwrap().insert(n.clone(), t.clone());
                    get_params.push((n.clone(), t.clone()));
                }
                let typed_body = if emit_fns && !skip_body {
                    if let Some(body) = get_body_rewritten.as_ref() {
                        self.return_slot.push(prop_ty.clone());
                        // RFC 006 M2：静态属性 getter 内禁止访问实例字段。
                        let prev_fn_static = self.current_fn_is_static;
                        self.current_fn_is_static = is_static_prop;
                        let result = self.check_block(body, &prop_ty);
                        self.current_fn_is_static = prev_fn_static;
                        let tb = match result {
                            Ok(tb) => Some(tb),
                            Err(e) => {
                                self.return_slot.pop();
                                self.scopes.pop();
                                self.current_class = prev;
                                return Err(e);
                            }
                        };
                        self.return_slot.pop();
                        tb
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.scopes.pop();
                if emit_fns && p.get_body.is_some() {
                    let getter_name: Ident = format!("{}::get_{}", class_id, p.name).into();
                    self.push_typed_fn(
                        getter_name,
                        Some(class_id.clone()),
                        false,
                        get_params,
                        prop_ty.clone(),
                        get_body_rewritten.clone(),
                        typed_body,
                        false,
                        self.fn_linkage_for_class(class),
                        is_static_prop,
                        // RFC 009 M3：property getter 不支持 `[Parallelize]` 属性。
                        false,
                    );
                }
            }
            if (p.has_set || p.has_init) && (p.set_body.is_some() || p.is_indexer()) {
                self.scopes.push(IndexMap::new());
                let mut set_params: Vec<(Ident, TypeId)> = Vec::new();
                if !is_static_prop {
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert("this".into(), TypeId::Named(class_id.clone()));
                    set_params.push(("this".into(), TypeId::Named(class_id.clone())));
                }
                for (n, t) in &index_tys {
                    self.scopes.last_mut().unwrap().insert(n.clone(), t.clone());
                    set_params.push((n.clone(), t.clone()));
                }
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert("value".into(), prop_ty.clone());
                set_params.push(("value".into(), prop_ty.clone()));
                let typed_body = if emit_fns && !skip_body {
                    if let Some(body) = set_body_rewritten.as_ref() {
                        self.return_slot.push(TypeId::Void);
                        // RFC 006 M2：静态属性 setter 内禁止访问实例字段。
                        let prev_fn_static = self.current_fn_is_static;
                        self.current_fn_is_static = is_static_prop;
                        // RFC 006 M2：自定义 init 体在构造期语义下检查（允许写其它 init-only）。
                        let prev_in_ctor = self.in_ctor;
                        if p.has_init {
                            self.in_ctor = true;
                        }
                        let result = self.check_block(body, &TypeId::Void);
                        self.in_ctor = prev_in_ctor;
                        self.current_fn_is_static = prev_fn_static;
                        let tb = match result {
                            Ok(tb) => Some(tb),
                            Err(e) => {
                                self.return_slot.pop();
                                self.scopes.pop();
                                self.current_class = prev;
                                return Err(e);
                            }
                        };
                        self.return_slot.pop();
                        tb
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.scopes.pop();
                if emit_fns && p.set_body.is_some() {
                    let setter_name: Ident = format!("{}::set_{}", class_id, p.name).into();
                    self.push_typed_fn(
                        setter_name,
                        Some(class_id.clone()),
                        false,
                        set_params,
                        TypeId::Void,
                        set_body_rewritten.clone(),
                        typed_body,
                        false,
                        self.fn_linkage_for_class(class),
                        is_static_prop,
                        // RFC 009 M3：property setter 不支持 `[Parallelize]` 属性。
                        false,
                    );
                }
            }
        }

        self.scopes.pop();
        self.current_class = prev;
        Ok(())
    }

    pub(crate) fn check_static_class(&mut self, class: &ClassDef) -> Result<(), TypeError> {
        let prev = self.current_class.clone();
        self.current_class = Some(class.name.clone());

        if !class.fields.is_empty() {
            return Err(TypeError::Oop(format!(
                "static class `{}` cannot have instance fields",
                class.name
            )));
        }
        if !class.constructors.is_empty() {
            return Err(TypeError::Oop(format!(
                "static class `{}` cannot have constructors",
                class.name
            )));
        }
        if !class.bases.is_empty() {
            return Err(TypeError::Oop(format!(
                "static class `{}` cannot inherit",
                class.name
            )));
        }

        // RFC 009 M1: 收集 static class 自身属性。
        // RFC 009 M3: 使用 `ensure_class_def_id` 复用前向引用时预分配的 DefId。
        // RFC 012 M4-1: 传入 `generic_arity`（static class 不允许泛型，恒为 0）。
        let class_def_id = self.ensure_class_def_id(&class.name, class.generics.len());
        self.resolve_attributes(
            &class.attributes,
            AttributeTarget::Class,
            class_def_id,
            None,
        );

        for method in &class.methods {
            let m = &method.node;
            if m.sig.modifier != MethodModifier::Static {
                return Err(TypeError::Oop(format!(
                    "static class `{}` method `{}` must be `static`",
                    class.name, m.sig.name
                )));
            }
            if m.sig.params.iter().skip(1).any(|p| p.is_extension_receiver) {
                return Err(TypeError::Oop(
                    "only the first parameter may be an extension receiver (`this Type name`)"
                        .into(),
                ));
            }

            // RFC 009 M1: 收集 static class 中 method 属性。
            if !m.sig.attributes.is_empty() {
                let method_def_id = self.alloc_member_def_id(&class.name, &m.sig.name);
                self.resolve_attributes(
                    &m.sig.attributes,
                    AttributeTarget::Method,
                    method_def_id,
                    None,
                );
                // 检测 [Builtin] 属性并注册到 builtin_registry
                let attrs = self.attribute_table.get_attrs(method_def_id);
                for attr in attrs {
                    if attr.name.as_str() == "Builtin" {
                        let abi = attr
                            .named_args
                            .iter()
                            .find(|(n, _)| n.as_str() == "ABI")
                            .and_then(|(_, v)| match v {
                                crate::ResolvedArg::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        self.builtin_registry
                            .insert(method_def_id, BuiltinMeta { abi });
                    }
                }
            }

            // RFC 004 修复：static class 方法级泛型参数（如
            // `Deserialize<T>() where T : IJsonDeserializable, new()`）须注册到
            // type_param_scope，否则 `T` 在 lower_type 解析失败报
            // `OOP: undefined type T`（普通类方法经 check_class 类级泛型
            // push_type_params 覆盖；static class 无类级泛型，须在方法级注册）。
            let has_method_generics = !m.sig.generics.is_empty();
            if has_method_generics {
                self.push_type_params(&m.sig.generics);
                // RFC 004 刀 2：与 check_fn 泛型分支同步——static class 方法级
                // 泛型的 where_clause 须注册到 where_clause_scope，供
                // check_generic_constraint_method_call 查询 `T` 的接口约束。
                self.where_clause_scope.push(m.sig.where_clause.clone());
            }
            self.scopes.push(IndexMap::new());
            let mut method_params = Vec::new();
            for p in &m.sig.params {
                let pty = self.lower_type(&p.ty.node)?;
                // RFC 009 P1-F #8：`in` 参数为 `readonly ref`——`mutable: false`。
                let final_ty = if p.is_in {
                    TypeId::Ref {
                        inner: Box::new(pty),
                        mutable: false,
                        kind: ast::RefKind::Var,
                    }
                } else if p.is_ref || p.is_out {
                    TypeId::Ref {
                        inner: Box::new(pty),
                        mutable: true,
                        kind: ast::RefKind::Var,
                    }
                } else {
                    pty
                };
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(p.name.clone(), final_ty.clone());
                method_params.push((p.name.clone(), final_ty));
            }
            // RFC 007：方法形参可选后缀 / 默认值常量性（含 M2b）。
            self.validate_params_m2b(&m.sig.params)?;
            // RFC 005：`params` 仅允许 Span/ROS（禁止 `params T[]`）。
            for p in &m.sig.params {
                if p.is_params {
                    let pty = self.lower_type(&p.ty.node)?;
                    let canon = self.canonical_type(&pty);
                    if !matches!(canon, TypeId::Span { .. } | TypeId::Array { .. }) {
                        self.scopes.pop();
                        if has_method_generics {
                            self.pop_type_params();
                            self.where_clause_scope.pop();
                        }
                        self.current_class = prev;
                        return Err(TypeError::Oop(
                            "`params` requires `Span<T>`, `ReadOnlySpan<T>`, or `T[]`".into(),
                        ));
                    }
                }
            }
            let ret = self.check_method_return(m.sig.ret.as_ref(), m.sig.is_async)?;
            if m.sig.is_async && !ret.is_task() {
                self.scopes.pop();
                if has_method_generics {
                    self.pop_type_params();
                    self.where_clause_scope.pop();
                }
                self.current_class = prev;
                return Err(TypeError::AsyncReturn(ret.display()));
            }
            if m.sig.is_async && m.sig.params.iter().any(|p| p.is_ref || p.is_out || p.is_in) {
                self.scopes.pop();
                if has_method_generics {
                    self.pop_type_params();
                    self.where_clause_scope.pop();
                }
                self.current_class = prev;
                return Err(TypeError::Oop(
                    "ref/out/in parameters are not allowed in async methods".into(),
                ));
            }
            let body_expected = self.body_return_slot(&ret, m.sig.is_async);
            let prev_async = self.in_async;
            self.in_async = m.sig.is_async;
            self.return_slot.push(body_expected.clone());
            let out_params: IndexSet<Ident> = m
                .sig
                .params
                .iter()
                .filter(|p| p.is_out)
                .map(|p| p.name.clone())
                .collect();
            let prev_flow = self.out_flow.take();
            self.out_flow = if out_params.is_empty() {
                None
            } else {
                Some(OutParamState::new(out_params))
            };
            let typed_body = if let Some(body) = &m.body {
                // RFC 006 M2：static class 方法恒为静态，进入方法体前置
                // current_fn_is_static = true，拦截实例字段访问。
                let prev_fn_static = self.current_fn_is_static;
                self.current_fn_is_static = true;
                let result = self.check_block(body, &body_expected);
                self.current_fn_is_static = prev_fn_static;
                match result {
                    Ok(tb) => Some(tb),
                    Err(e) => {
                        self.out_flow = prev_flow;
                        self.return_slot.pop();
                        self.in_async = prev_async;
                        self.scopes.pop();
                        if has_method_generics {
                            self.pop_type_params();
                            self.where_clause_scope.pop();
                        }
                        self.current_class = prev;
                        return Err(e);
                    }
                }
            } else {
                None
            };
            if let Some(flow) = &self.out_flow {
                let missing = flow.unassigned();
                if !missing.is_empty() {
                    self.out_flow = prev_flow;
                    self.return_slot.pop();
                    self.in_async = prev_async;
                    self.scopes.pop();
                    if has_method_generics {
                        self.pop_type_params();
                        self.where_clause_scope.pop();
                    }
                    self.current_class = prev;
                    return Err(TypeError::Oop(format!(
                        "out parameter `{}` must be assigned before control leaves the current method",
                        missing[0]
                    )));
                }
            }
            self.out_flow = prev_flow;
            self.return_slot.pop();
            self.in_async = prev_async;
            self.scopes.pop();
            if has_method_generics {
                self.pop_type_params();
                self.where_clause_scope.pop();
            }
            let mut oop_params = Vec::new();
            for p in &m.sig.params {
                let pty = type_id_to_field_name(
                    &method_params
                        .iter()
                        .find(|(n, _)| n == &p.name)
                        .map(|(_, t)| t.clone())
                        .unwrap_or(TypeId::Void),
                );
                oop_params.push(ParamSig {
                    name: p.name.clone(),
                    ty: pty,
                    is_ref: p.is_ref,
                    is_out: p.is_out,
                    is_in: p.is_in,
                    is_params: p.is_params,
                    default: p
                        .default
                        .as_ref()
                        .and_then(|e| self.fold_param_default_expr(&e.node)),
                });
            }
            let oop_ret = type_id_to_field_name(&ret);
            let oop_sig = OopMethodSig {
                name: m.sig.name.clone(),
                vis: m.sig.vis,
                params: oop_params,
                ret: oop_ret,
                modifier: m.sig.modifier,
                is_async: m.sig.is_async,
                generics: m.sig.generics.iter().map(|g| g.name.clone()).collect(),
                is_static_abstract: m.sig.is_static_abstract,
            };
            let static_count = class
                .methods
                .iter()
                .filter(|other| {
                    other.node.sig.name == m.sig.name
                        && matches!(other.node.sig.modifier, MethodModifier::Static)
                })
                .count()
                .max(
                    self.registry
                        .method_overload_count_kind(&class.name, &m.sig.name, true),
                );
            let instance_count = class
                .methods
                .iter()
                .filter(|other| {
                    other.node.sig.name == m.sig.name
                        && !matches!(other.node.sig.modifier, MethodModifier::Static)
                })
                .count()
                .max(
                    self.registry
                        .method_overload_count_kind(&class.name, &m.sig.name, false),
                );
            let method_name: Ident = if static_count > 0 && instance_count > 0 {
                method_link_name_static_abi(
                    class.name.as_str(),
                    &oop_sig,
                    static_count,
                    instance_count,
                )
                .into()
            } else {
                let overload_count = self
                    .registry
                    .method_overload_count(&class.name, &m.sig.name)
                    .max(
                        class
                            .methods
                            .iter()
                            .filter(|other| other.node.sig.name == m.sig.name)
                            .count(),
                    );
                method_link_name(class.name.as_str(), &oop_sig, overload_count).into()
            };
            // 决策 #7（RFC 010）：泛型**扩展**方法不直接 emit 方法体（含未解析的类型参数），
            // 而是将 AST 模板存入 `extension_fn_templates`，由调用点
            // `instantiate_generic_extension_fn` 按接收者类型单态化生成具体方法体。
            // 普通 `static class` 泛型方法（如 `Assert.Empty<T>`）必须 `push_typed_fn`，
            // 供 MIR `try_create_mono_body` 从 `Class::Method` 克隆 `Class::Method__T`。
            let is_extension = m
                .sig
                .params
                .first()
                .is_some_and(|p| p.is_extension_receiver);
            if !m.sig.generics.is_empty() && is_extension {
                // RFC 006：符号身份单一权威（存储即用）。直接复用 registry 注册时经
                // 单一权威 `extension_mangle_base` 计算并存储的
                // `ExtensionMethod.mangle_base` / `template_key`，作为本模板的符号基底
                // 与查找键（`template_key` 含 arity 后缀，仅作 HashMap 查找键，不进入
                // 符号 mangle）。这使实例化侧（`FnDef.name`）与调用点（`make_resolution`
                // 的 `mangle_base`）逐字节同源，从结构上消除 check_class 与 registry
                // 各自独立推导造成的符号漂移（tree-shake 剪定义 → LLVM undefined name）。
                // 匹配条件必须含**非接收者参数签名**：`em.method` 为去掉扩展接收者后
                // 的 `ext_sig`；`oop_sig.params[1..]` 为当前方法去掉接收者后的参数。
                // 仅按 (container, 方法名, 泛型个数) 匹配会在「同泛型个数的多个重载」
                // 下取到**首个**模板键（如 `AddSingleton<TService>(this IServiceCollection)`
                // 自实现版）→ 实例/工厂重载注册到错误键 → 调用点按正确
                // `template_key` 查不到 → `undefined name`。逐参数类型比较使每个重载
                // 精确映射到自己的 `ExtensionMethod.template_key`。
                let self_rest: Vec<String> = oop_sig
                    .params
                    .iter()
                    .skip(1)
                    .map(|p| p.ty.as_str().to_string())
                    .collect();
                let (template_name, template_key): (Ident, Ident) = self
                    .registry
                    .extensions
                    .values()
                    .flatten()
                    .find(|em| {
                        em.container == class.name
                            && em.method.name == m.sig.name
                            && em.generic_params.len() == m.sig.generics.len()
                            && em
                                .method
                                .params
                                .iter()
                                .map(|p| p.ty.as_str().to_string())
                                .collect::<Vec<_>>()
                                == self_rest
                    })
                    .map(|em| (em.mangle_base.clone(), em.template_key.clone()))
                    .unwrap_or_else(|| {
                        // 防御性回退（正常不触发）：与 registry `name_counts` 同语义重算。
                        let total = class
                            .methods
                            .iter()
                            .filter(|other| other.node.sig.name == m.sig.name)
                            .count()
                            .max(1);
                        let name = extension_mangle_base(class.name.as_str(), &oop_sig, total);
                        (
                            name.clone().into(),
                            format!("{name}_{}", m.sig.generics.len()).into(),
                        )
                    });
                let fn_def = FnDef {
                    vis: m.sig.vis,
                    name: template_name,
                    generics: m.sig.generics.clone(),
                    where_clause: m.sig.where_clause.clone(),
                    params: m.sig.params.clone(),
                    ret: m.sig.ret.clone(),
                    body: m.body.clone(),
                    is_async: m.sig.is_async,
                    attributes: m.sig.attributes.clone(),
                    doc: None,
                };
                self.extension_fn_templates.insert(template_key, fn_def);
                continue;
            }
            // RFC 004 刀 2 约束修复（Step 1）：static class 泛型方法注册到
            // fn_templates（键 = method_name，与 MIR mono 的模板基底一致）。
            // 否则调用点 `Maker.Make<IntFactory>` 走 OOP resolve_method_with_type_args，
            // 无 where_clause 可查 → 约束检查被跳过（`IntFactory : IFactory<int>` 被
            // 错误接受用于 `where T : IFactory<T>`）。注册后调用点可经
            // fn_templates 取 where_clause 验证约束（Step 2）。
            // fn_templates 仅被 instantiate_generic_fn（裸 Ident Call）消费，
            // static class 方法名带命名空间前缀（Maker::Make），不会与自由函数冲突。
            if !m.sig.generics.is_empty() {
                if std::env::var("ARC_DEBUG_TEMPLATES").is_ok() {
                    eprintln!(
                        "[check_class] generic method template {method_name} generics={:?}",
                        m.sig
                            .generics
                            .iter()
                            .map(|g| g.name.as_str())
                            .collect::<Vec<_>>()
                    );
                }
                if std::env::var("ARC_DEBUG_TEMPLATES").is_ok()
                    && method_name.as_str().contains("RegisterWeak")
                {
                    eprintln!("[check_class] registering generic fn template {method_name}");
                }
                let fn_def = FnDef {
                    vis: m.sig.vis,
                    name: method_name.clone(),
                    generics: m.sig.generics.clone(),
                    where_clause: m.sig.where_clause.clone(),
                    params: m.sig.params.clone(),
                    ret: m.sig.ret.clone(),
                    body: m.body.clone(),
                    is_async: m.sig.is_async,
                    attributes: m.sig.attributes.clone(),
                    doc: None,
                };
                self.fn_templates.insert(method_name.clone(), fn_def);
            }
            // Store declared return type, not body return slot.
            self.push_typed_fn(
                method_name,
                Some(class.name.clone()),
                false,
                method_params,
                ret,
                m.body.clone(),
                typed_body,
                m.sig.is_async,
                self.fn_linkage_for_class(class),
                true,
                // RFC 009 M3：检测 `[Parallelize]` 属性，标记向量化候选。
                // 扩展方法也支持 `[Parallelize]`（与实例方法一致）。
                Self::has_parallelize_attr(&m.sig.attributes),
            );
        }

        // RFC 036 M5（静态类属性）：`static class` 属性镜像 `check_class` 的
        // 自定义访问器处理，但成员恒为静态（无 `this` 参数，RFC 004 M2）。
        // 此前 static class 的 `class.properties` 完全未处理——属性 getter/setter
        // 静默不注册，`BarcodeReader.IsZxingAvailable` 等静态类属性成员访问
        // 报 `no field or property`。static class 无实例字段 → 索引器与
        // auto-property（需 backing field）均不允许，仅支持显式访问器体。
        let skip_body = is_builtin_facade(&class.name);
        for p in &class.properties {
            if p.is_indexer() {
                self.current_class = prev;
                return Err(TypeError::Oop(format!(
                    "static class `{}` cannot declare an indexer",
                    class.name
                )));
            }
            if p.has_set && p.has_init {
                self.current_class = prev;
                return Err(TypeError::Oop(format!(
                    "property `{}` cannot declare both `set` and `init`",
                    p.name
                )));
            }
            let is_custom = p.get_body.is_some() || p.set_body.is_some();
            if !is_custom {
                self.current_class = prev;
                return Err(TypeError::Oop(format!(
                    "static class `{}` property `{}` requires an explicit accessor body \
                     (auto-properties need instance backing fields, forbidden in static classes)",
                    class.name, p.name
                )));
            }
            if p.has_get && p.get_body.is_none() {
                self.current_class = prev;
                return Err(TypeError::Oop(format!(
                    "property `{}` has custom accessors but `get` has no body",
                    p.name
                )));
            }
            if p.has_set && p.set_body.is_none() {
                self.current_class = prev;
                return Err(TypeError::Oop(format!(
                    "property `{}` has custom accessors but `set` has no body",
                    p.name
                )));
            }
            if p.has_init && p.set_body.is_none() && p.get_body.is_some() {
                self.current_class = prev;
                return Err(TypeError::Oop(format!(
                    "property `{}` has custom accessors but `init` has no body",
                    p.name
                )));
            }
            let prop_ty = self.lower_type(&p.ty.node)?;
            if p.has_get {
                if let Some(get_body) = p.get_body.as_ref() {
                    self.scopes.push(IndexMap::new());
                    let typed_body = if !skip_body {
                        self.return_slot.push(prop_ty.clone());
                        // RFC 006 M2：静态属性 getter 内禁止访问实例字段。
                        let prev_fn_static = self.current_fn_is_static;
                        self.current_fn_is_static = true;
                        let result = self.check_block(get_body, &prop_ty);
                        self.current_fn_is_static = prev_fn_static;
                        let tb = match result {
                            Ok(tb) => Some(tb),
                            Err(e) => {
                                self.return_slot.pop();
                                self.scopes.pop();
                                self.current_class = prev;
                                return Err(e);
                            }
                        };
                        self.return_slot.pop();
                        tb
                    } else {
                        None
                    };
                    self.scopes.pop();
                    let getter_name: Ident = format!("{}::get_{}", class.name, p.name).into();
                    self.push_typed_fn(
                        getter_name,
                        Some(class.name.clone()),
                        false,
                        Vec::new(),
                        prop_ty.clone(),
                        p.get_body.clone(),
                        typed_body,
                        false,
                        self.fn_linkage_for_class(class),
                        true,
                        // RFC 009 M3：property getter 不支持 `[Parallelize]` 属性。
                        false,
                    );
                }
            }
            if p.has_set || p.has_init {
                if let Some(set_body) = p.set_body.as_ref() {
                    self.scopes.push(IndexMap::new());
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert("value".into(), prop_ty.clone());
                    let set_params = vec![("value".into(), prop_ty.clone())];
                    let typed_body = if !skip_body {
                        self.return_slot.push(TypeId::Void);
                        // RFC 006 M2：静态属性 setter 内禁止访问实例字段。
                        let prev_fn_static = self.current_fn_is_static;
                        self.current_fn_is_static = true;
                        let result = self.check_block(set_body, &TypeId::Void);
                        self.current_fn_is_static = prev_fn_static;
                        let tb = match result {
                            Ok(tb) => Some(tb),
                            Err(e) => {
                                self.return_slot.pop();
                                self.scopes.pop();
                                self.current_class = prev;
                                return Err(e);
                            }
                        };
                        self.return_slot.pop();
                        tb
                    } else {
                        None
                    };
                    self.scopes.pop();
                    let setter_name: Ident = format!("{}::set_{}", class.name, p.name).into();
                    self.push_typed_fn(
                        setter_name,
                        Some(class.name.clone()),
                        false,
                        set_params,
                        TypeId::Void,
                        p.set_body.clone(),
                        typed_body,
                        false,
                        self.fn_linkage_for_class(class),
                        true,
                        // RFC 009 M3：property setter 不支持 `[Parallelize]` 属性。
                        false,
                    );
                }
            }
        }

        self.current_class = prev;
        Ok(())
    }

    /// RFC 009 M1: 收集 struct 自身与 fields 的属性。
    ///
    /// 当前架构下 `HirItem::Struct` 仅在 `TypeRegistry::register_item` 中
    /// 注册类型元数据，不经过 `check_class` 流程。本方法在
    /// `check_module_items` 中显式调用，把 struct 自身与各 field 上的
    /// `ast::Attribute` 收集到 `attribute_table`。
    ///
    /// RFC readonly struct: 校验 readonly struct 的约束——所有字段必须是 readonly。
    pub(crate) fn collect_struct_attributes(&mut self, def: &StructDef) {
        let struct_def_id = self.alloc_symbol_def_id();
        self.class_def_ids.insert(def.name.clone(), struct_def_id);
        self.resolve_attributes(
            &def.attributes,
            AttributeTarget::Struct,
            struct_def_id,
            None,
        );

        if def.is_readonly {
            for f in &def.fields {
                if !f.is_readonly && !f.is_const {
                    self.errors.push(TypeError::Oop(format!(
                        "readonly struct `{}` field `{}` must be readonly or const",
                        def.name, f.name
                    )));
                }
            }
        }

        for f in &def.fields {
            if f.attributes.is_empty() {
                continue;
            }
            let field_def_id = self.alloc_member_def_id(&def.name, &f.name);
            self.resolve_attributes(&f.attributes, AttributeTarget::Field, field_def_id, None);
        }
    }

    /// RFC 012 M1: 收集 interface 自身、properties 与 methods 的属性。
    ///
    /// 与 `collect_struct_attributes` 同理，interface 在当前架构下不经过
    /// `check_class` 流程。本方法在 `check_module_items` 中对每个
    /// `HirItem::Interface` 显式调用。
    pub(crate) fn collect_interface_attributes(&mut self, def: &InterfaceDef) {
        let iface_def_id = self.alloc_symbol_def_id();
        self.class_def_ids.insert(def.name.clone(), iface_def_id);
        self.resolve_attributes(
            &def.attributes,
            AttributeTarget::Interface,
            iface_def_id,
            None,
        );

        for prop in &def.properties {
            if prop.attributes.is_empty() {
                continue;
            }
            let prop_def_id = self.alloc_member_def_id(&def.name, &prop.name);
            self.resolve_attributes(
                &prop.attributes,
                AttributeTarget::Property,
                prop_def_id,
                Some(prop),
            );
        }
        for m in &def.methods {
            if m.attributes.is_empty() {
                continue;
            }
            let method_def_id = self.alloc_member_def_id(&def.name, &m.name);
            self.resolve_attributes(&m.attributes, AttributeTarget::Method, method_def_id, None);
        }
    }

    /// 收集枚举自身与各枚举成员（variant）的属性。
    ///
    /// 通用属性系统（RFC 038）：任何声明均可附加属性，枚举成员亦不例外——
    /// 如 `[Display("无")] None`。枚举在当前架构下不经过 `check_class`
    /// 流程，本方法在 `check_module_items` 中对每个 `HirItem::Enum` 显式调用。
    ///
    /// - 枚举自身：分配 symbol DefId 并注册到 `class_def_ids`，属性目标为
    ///   [`AttributeTarget::Enum`]。
    /// - 每个带属性的枚举成员：分配 member DefId（键 `(枚举名, 成员名)`），
    ///   属性目标为 [`AttributeTarget::EnumMember`]。
    ///
    /// 属性经 `AttributeTable` 收集后，供外部消费者（如 UI 的
    /// `EnumItemsSourceGenerator`）按 DefId 编译期读取 `[Display]`/`[Description]`
    /// 等元数据，无需运行时反射。
    pub(crate) fn collect_enum_attributes(&mut self, def: &EnumDef) {
        let enum_def_id = self.alloc_symbol_def_id();
        self.class_def_ids.insert(def.name.clone(), enum_def_id);
        // RFC 038：缓存 EnumDef 供 `Enum.GetOptions<E>()` 泛型特化遍历变体。
        self.enum_defs.insert(def.name.clone(), def.clone());
        self.resolve_attributes(&def.attributes, AttributeTarget::Enum, enum_def_id, None);

        for v in &def.variants {
            if v.attributes.is_empty() {
                continue;
            }
            let member_def_id = self.alloc_member_def_id(&def.name, &v.name);
            self.resolve_attributes(
                &v.attributes,
                AttributeTarget::EnumMember,
                member_def_id,
                None,
            );
        }
    }
}

/// C#：实例字段初始化器按声明序注入每个构造函数（base 调用之后、用户 body 之前）。
///
/// 跳过 `static`/`const`（前者走 `__sinit`，后者编译期折叠）。跨文件 partial
/// 合并后 `class.fields` 已含所有分片，两侧初始化器一并生效。
/// 另含 auto-property 的属性初值（`{ get; } = expr;`，存于 `class.properties`）
/// ——其 backing field 已随属性注册，此处为每个 ctor 注入 `this.<Prop> = expr;`，
/// 语义与字段初始化器一致（构造期执行一次，getter 零成本读字段）。
fn instance_field_init_stmts(class: &ClassDef) -> Vec<Spanned<Stmt>> {
    let mut stmts = Vec::new();
    for f in &class.fields {
        if f.is_static || f.is_const {
            continue;
        }
        let Some(init) = &f.init else {
            continue;
        };
        let target = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(Expr::Ident("this".into()), Span::DUMMY)),
                field: f.name.clone(),
            },
            Span::DUMMY,
        );
        stmts.push(Spanned::new(
            Stmt::Assign {
                target,
                value: init.clone(),
            },
            Span::DUMMY,
        ));
    }
    // 属性初值注入：auto-property（无访问器体，非索引器/static abstract）带 `= expr;`。
    for p in &class.properties {
        let Some(init) = &p.init else {
            continue;
        };
        if p.is_indexer() || p.is_static_abstract || p.modifier == MethodModifier::Static {
            continue;
        }
        let target = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(Expr::Ident("this".into()), Span::DUMMY)),
                field: p.name.clone(),
            },
            Span::DUMMY,
        );
        stmts.push(Spanned::new(
            Stmt::Assign {
                target,
                value: init.clone(),
            },
            Span::DUMMY,
        ));
    }
    stmts
}
