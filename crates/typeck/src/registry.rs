use ast::*;
use hir::{HirItem, HirModule, ImportBinding};
use indexmap::IndexMap;

use crate::field_keyword::uses_field;
use crate::oop_types::*;
use crate::resolve_instantiated_type_name;

#[cfg(test)]
mod package_internal_test;
pub mod registry_resolve;
mod registry_validate;
#[cfg(test)]
mod resolve_static_overload_test;
pub use registry_resolve::substitute_generic_in_ty_name;

/// 属性「访问器形态」判定的**单一事实源**（RFC 006 / `[Builtin]` 自动属性）。
///
/// 返回 `true` 表示属性须按 **custom 访问器** 注册（注册 `get_X`/`set_X`
/// 方法、不生成 backing field）：
/// - 索引器（`get_Item`/`set_Item`，永不生成 backing field）；
/// - 带访问器体（`{ get { … } }`，真实 custom 属性）；
/// - `[Builtin]` 属性（虽以 `{ get; }` 书写，访问器由 codegen 拦截直射
///   `rt_*` ABI——见 `is_builtin_property_attr`）。
///
/// 返回 `false` 表示普通自动属性（生成 backing field）。
///
/// **维护规则**：registry / check_class / check_generics 的全部调用点必须使用
/// 本函数，**禁止**各自复制判定（历史教训：单态化路径漏补 `[Builtin]` 分支
/// 曾致 `List<T>.Count` 注册为 backing field → MIR FieldGet 读 `RtList*`
/// 垃圾偏移 → 运行期静默错乱）。abstract 属性由调用方按职责附加
/// （注册层需要、访问器检查层不需要——见各调用点注释）。
pub fn property_has_custom_accessors(p: &ast::PropertyDef) -> bool {
    p.is_indexer()
        || p.get_body.is_some()
        || p.set_body.is_some()
        || crate::builtin_facade::is_builtin_property_attr(&p.attributes)
}

fn push_method(methods: &mut IndexMap<Ident, Vec<OopMethodSig>>, sig: OopMethodSig) {
    methods.entry(sig.name.clone()).or_default().push(sig);
}

fn find_method_sig<'a>(
    methods: &'a IndexMap<Ident, Vec<OopMethodSig>>,
    name: &Ident,
    sig: &OopMethodSig,
) -> Option<&'a OopMethodSig> {
    methods
        .get(name)?
        .iter()
        .find(|m| method_params_match(m, sig))
}

/// RFC 032 M2：查找类中匹配的 `public static` 方法（用于校验 `static abstract`
/// 接口成员的实现）。与 `find_method_sig` 的差异：仅匹配 `modifier == Static`。
fn find_static_method_sig<'a>(
    methods: &'a IndexMap<Ident, Vec<OopMethodSig>>,
    name: &Ident,
    sig: &OopMethodSig,
) -> Option<&'a OopMethodSig> {
    methods
        .get(name)?
        .iter()
        .find(|m| m.modifier == MethodModifier::Static && method_params_match(m, sig))
}

fn iter_method_sigs(
    methods: &IndexMap<Ident, Vec<OopMethodSig>>,
) -> impl Iterator<Item = &OopMethodSig> {
    methods.values().flat_map(|sigs| sigs.iter())
}

impl TypeRegistry {
    pub fn from_module(module: &HirModule) -> Self {
        Self::from_module_with_entry(module, &std::collections::HashMap::new(), None)
    }

    /// CD-30：`from_module` 的包感知变体——注册期即注入包图与入口包名，
    /// 使同名类型注册应用「入口包优先」遮蔽规则（见 [`TypeRegistry::shadow_insert`]）。
    /// 无包信息时（单模块/测试）与 [`from_module`] 行为一致（后写覆盖）。
    pub fn from_module_with_entry(
        module: &HirModule,
        file_packages: &std::collections::HashMap<ast::FileId, String>,
        entry_package: Option<&str>,
    ) -> Self {
        let mut reg = Self {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: indexmap::IndexSet::new(),
            declared_properties: IndexMap::new(),
            file_packages: file_packages.clone(),
            internals_visible_to: std::collections::HashMap::new(),
            shadowed_types: std::collections::HashMap::new(),
            synth_hosts: IndexMap::new(),
            builtin_static_props: IndexMap::new(),
            entry_package: entry_package.map(str::to_string),
            delegate_aliases: std::collections::HashMap::new(),
        };
        reg.register_module(module, &[]);
        reg
    }

    /// CD-30：同名类型注册的「入口包优先」遮蔽。
    ///
    /// 顶层类型遮蔽依赖包 internal 类：入口包（当前项目）声明的类型恒优先于
    /// 依赖包的同名类型。规则：
    /// - 已有旧类型属入口包、新类型非入口包 → 跳过注册（依赖不遮蔽入口）；
    /// - 新类型属入口包、旧类型非入口包 → 覆盖（入口遮蔽依赖）；
    /// - 其余（同包 / 全依赖 / 无包信息）→ 保持后写覆盖的历史行为。
    ///
    /// **被遮蔽方按 FQN 保留**（`shadowed_types`）：`Arc.Drawing.ImageNative`
    /// 与全局 `ImageNative` 按全限定名天然共存——依赖包内部代码沿调用点
    /// namespace 链经 `lookup_type` 仍可解析到本包类型，不因短名冲突而丢失。
    /// 两个方向都保留：入口包覆盖依赖包时保存被覆盖的依赖包类型；依赖包
    /// 撞入口包时保存被拒的依赖包类型（注册顺序无关）。
    fn shadow_insert(&mut self, key: Ident, nom: NominalType) {
        if let Some(existing) = self.types.get(&key) {
            let old_entry = self.package_is_entry(existing.span);
            let new_entry = self.package_is_entry(nom.span);
            if old_entry && !new_entry {
                // 依赖包不得覆盖入口包声明——依赖包类型按 FQN 保留。
                let fqn = type_fqn(&nom.namespace, nom.name.as_str());
                self.shadowed_types.entry(fqn).or_insert(nom);
                return;
            }
            // 入口包覆盖依赖包 / 同包后写覆盖：被覆盖方按 FQN 保留。
            let fqn = type_fqn(&existing.namespace, existing.name.as_str());
            self.shadowed_types
                .entry(fqn)
                .or_insert_with(|| existing.clone());
            // 入口包覆盖依赖包 → 落到下方 insert
        }
        self.types.insert(key, nom);
    }

    /// CD-30（C# 语义）：沿调用点 namespace 链解析类型名。
    ///
    /// `namespace A.B` 内引用未限定名 `T` → 依次尝试 FQN `A.B.T` → `A.T` →
    /// `T`（`shadowed_types`——被短名遮蔽的依赖包类型），最后回退短名主索引
    /// `types`。对齐 C# 名称查找（当前 ns → 父 ns → 全局）；入口包对全局名的
    /// 优先由 `shadow_insert` 的遮蔽规则保证。无包图/单模块（enclosing 空）
    /// 时退化为纯短名解析，行为与历史一致。
    pub fn lookup_type(&self, name: &Ident, enclosing: &[Ident]) -> Option<&NominalType> {
        for i in (0..=enclosing.len()).rev() {
            let fqn = type_fqn(&enclosing[..i], name.as_str());
            if let Some(t) = self.shadowed_types.get(&fqn) {
                return Some(t);
            }
        }
        self.types.get(name)
    }

    /// `span` 所属文件是否属于入口包。无包图信息（单模块/测试）时视同入口，
    /// 不触发遮蔽——保持历史后写覆盖行为。
    fn package_is_entry(&self, span: ast::Span) -> bool {
        match self.file_packages.get(&span.file_id) {
            Some(pkg) => self.entry_package.as_deref() == Some(pkg.as_str()),
            None => true,
        }
    }

    /// `class` 的 `prop` 是否为 `[Builtin]` **静态**自动属性（无真实 getter 体，
    /// codegen 按源码形 `"Class.Prop"` 分派）。见 `TypeRegistry.builtin_static_props`。
    pub fn is_builtin_static_prop(&self, class: &str, prop: &str) -> bool {
        self.builtin_static_props
            .get(class)
            .is_some_and(|props| props.contains(prop))
    }

    /// 登记 `[Builtin]` 静态自动属性（register_class / register_monomorphized_class
    /// 属性循环调用；SSoT 延伸——与 `property_has_custom_accessors` 同源判定，
    /// 仅静态侧）。
    pub(crate) fn record_builtin_static_prop(&mut self, class: &Ident, p: &ast::PropertyDef) {
        if p.modifier == ast::MethodModifier::Static
            && crate::builtin_facade::is_builtin_property_attr(&p.attributes)
        {
            self.builtin_static_props
                .entry(class.clone())
                .or_default()
                .insert(p.name.clone());
        }
    }

    /// RFC 009 M5-4: 将额外模块的类型注册到当前 registry（合并而非替换）。
    ///
    /// 用于 Pass 4 对 Source Generator 生成的 `Program` 进行 typeck 时——
    /// 生成代码可引用原模块已注册的类型，但原模块无法引用生成代码中的
    /// 类型（Pass 2 已完成）。本方法把生成代码的新类型增量加入 registry，
    /// 供 Pass 4 typeck 解析类型引用。
    pub fn register_module(&mut self, module: &HirModule, namespace: &[Ident]) {
        self.register_module_inner(module, namespace);
    }

    fn register_module_inner(&mut self, module: &HirModule, namespace: &[Ident]) {
        let mut path = namespace.to_vec();
        if let Some(name) = &module.name {
            path.push(name.clone());
        }
        for item in &module.items {
            self.register_item(item, &path);
        }
        for child in &module.children {
            self.register_module_inner(child, &path);
        }
    }

    fn register_item(&mut self, item: &HirItem, namespace: &[Ident]) {
        match item {
            HirItem::Struct { def_ast, span, .. } => {
                let mut methods = IndexMap::new();
                for m in &def_ast.methods {
                    let sig = method_sig_from_ast(&m.node.sig);
                    push_method(&mut methods, sig);
                }
                let mut fields = fields_from_ast(&def_ast.fields);
                let mut required_props = indexmap::IndexSet::new();
                for p in &def_ast.properties {
                    if p.is_required {
                        required_props.insert(p.name.clone());
                    }
                    if p.is_static_abstract {
                        continue;
                    }
                    self.record_builtin_static_prop(&def_ast.name, p);
                    let ty = type_path_name(&p.ty.node).unwrap_or_else(|| "unknown".into());
                    // 访问器形态判定走单一事实源（含 `[Builtin]` 自动属性——
                    // 与 class 路径对齐，struct 出现 `[Builtin]` 属性时行为一致）。
                    let is_custom = property_has_custom_accessors(p);
                    if is_custom {
                        if p.is_indexer() {
                            push_indexer_accessors(&mut methods, p, &ty);
                        } else {
                            // RFC 006 A2：访问器体引用 `field` 时，该属性仍"自动"——
                            // 为它合成 backing field（名={Prop}__backing，与属性名区分，
                            // 否则 MIR 判定为 auto 属性绕过访问器），与 get/set 方法并存注册。
                            // `field` 在体内重写为 `this.<backing>`（见 field_keyword.rs）。
                            // 索引器（Item）保守不支持。
                            if uses_field(&p.get_body, &p.set_body) {
                                let fname = crate::field_keyword::backing_field_name(&p.name);
                                fields.insert(
                                    fname.clone(),
                                    FieldInfo {
                                        name: fname,
                                        ty: ty.clone(),
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
                            if p.has_get {
                                push_method(
                                    &mut methods,
                                    OopMethodSig {
                                        name: format!("get_{}", p.name).into(),
                                        vis: p.get_vis.unwrap_or(p.vis),
                                        params: vec![],
                                        ret: ty.clone(),
                                        modifier: p.modifier,
                                        is_async: false,
                                        generics: vec![],
                                        is_static_abstract: false,
                                    },
                                );
                            }
                            if p.has_set || p.has_init {
                                push_method(
                                    &mut methods,
                                    OopMethodSig {
                                        name: format!("set_{}", p.name).into(),
                                        vis: p.set_vis.unwrap_or(p.vis),
                                        params: vec![ParamSig {
                                            name: "value".into(),
                                            ty: ty.clone(),
                                            is_ref: false,
                                            is_out: false,
                                            is_in: false,
                                            is_params: false,
                                            default: None,
                                        }],
                                        ret: "void".into(),
                                        modifier: p.modifier,
                                        is_async: false,
                                        generics: vec![],
                                        is_static_abstract: false,
                                    },
                                );
                                if p.has_init {
                                    self.init_only_props
                                        .insert((def_ast.name.clone(), p.name.clone()));
                                }
                            }
                        }
                    } else {
                        // RFC 006 M4：auto `{ get; init; }` / `{ get; set; }` 进 fields。
                        // 属性初值（`{ get; } = expr;`）随 backing field 注册为 field init，
                        // 供 ctor 注入机制在每个构造器起始执行 `this.Prop = expr;`。
                        fields.insert(
                            p.name.clone(),
                            FieldInfo {
                                name: p.name.clone(),
                                ty,
                                vis: p.vis,
                                is_const: false,
                                is_readonly: false,
                                is_init_only: p.has_init && !p.has_set,
                                get_vis: p.get_vis,
                                set_vis: p.set_vis,
                                is_static: false,
                                init: p.init.clone(),
                            },
                        );
                    }
                }
                let constructors: Vec<CtorSig> = ctors_from_ast(&def_ast.constructors);
                let bases: Vec<Ident> = def_ast.bases.iter().filter_map(type_path_name).collect();
                let base_types: Vec<ast::Type> = def_ast.bases.clone();
                self.shadow_insert(
                    def_ast.name.clone(),
                    NominalType {
                        name: def_ast.name.clone(),
                        kind: TypeKind::Struct,
                        vis: def_ast.vis,
                        is_abstract: false,
                        is_record: def_ast.is_record,
                        is_readonly: def_ast.is_readonly,
                        fields,
                        methods,
                        bases: bases.clone(),
                        base_types,
                        span: *span,
                        variants: vec![],
                        generic_params: def_ast.generics.iter().map(|g| g.name.clone()).collect(),
                        namespace: namespace.to_vec(),
                        const_values: IndexMap::new(),
                        constructors,
                        // RFC 009 D3：从 `[SoA]` attribute 解析，使 layout/codegen 可识别。
                        // 与 `has_parallelize_attr` 同形：取属性路径最后一段匹配。
                        soa: def_ast.attributes.iter().any(|a| {
                            a.path.last().is_some_and(|name| {
                                let s = name.as_str();
                                s == "SoA" || s == "SoAAttribute"
                            })
                        }),
                        required_props,
                    },
                );
            }
            HirItem::Class { def_ast, span, .. } => {
                // RFC 012 M4-1: 泛型类模板不注册到 `registry.types`。
                //
                // 同名类按泛型 arity 重载时（C# 风格 arity overloading），
                // 如 `GenerateToAttribute`（非泛型）与 `GenerateToAttribute<T>`（泛型），
                // 若两者都写入 `registry.types`（键为简单类名），后者会覆盖前者，
                // 导致 `inherited_field_types` 在自引用 bases 链上无限递归。
                //
                // 泛型模板仅在 `class_templates`（由 `check_class` 填充）中保留，
                // 实例化时通过 mangle 名（如 `GenerateToAttribute_Bar`）注册到
                // `registry.types`。非泛型同名类保留在 `registry.types` 中供
                // 类型解析与 `is_class` 判定使用。
                if !def_ast.generics.is_empty() {
                    return;
                }
                if def_ast.is_static {
                    self.register_static_class(def_ast, namespace, *span);
                } else {
                    self.register_class(def_ast, namespace, *span);
                }
            }
            HirItem::Interface { def_ast, span, .. } => {
                let bases: Vec<_> = def_ast.bases.iter().filter_map(type_path_name).collect();
                let mut methods = IndexMap::new();
                for sig in &def_ast.methods {
                    let s = method_sig_from_ast(sig);
                    push_method(&mut methods, s);
                }
                let fields = IndexMap::new();
                // 接口属性签名收集（声明序；供 layout 扁平序 properties 构建——
                // 见 typeck layout.rs 接口分支；反射 declared_properties 亦受益）。
                let mut iface_declared_props: Vec<crate::oop_types::DeclaredPropertySig> =
                    Vec::new();
                for p in &def_ast.properties {
                    let ty = type_path_name(&p.ty.node).unwrap_or_else(|| "unknown".into());
                    iface_declared_props.push(crate::oop_types::DeclaredPropertySig {
                        name: p.name.clone(),
                        ty: ty.clone(),
                        can_read: p.has_get,
                        can_write: p.has_set || p.has_init,
                    });
                    // RFC 007：接口索引器注册为 get_Item/set_Item 方法，禁止走 field 路径。
                    if p.is_indexer() {
                        push_indexer_accessors(&mut methods, p, &ty);
                        continue;
                    }
                    // 接口的 property 是方法契约（`T Prop { get; set; }`），
                    // 不是字段——类的 custom property（`T Prop { get { ... } }`）
                    // 注册为 `get_Prop` / `set_Prop` 方法。若接口注册为 field，
                    // `check_interface_impl` 会在类的 fields 中查找，找不到就报
                    // "missing property"。此处与类一致注册为方法，使验证逻辑能
                    // 在类的方法表中匹配 `get_Prop` / `set_Prop`。
                    if p.has_get {
                        let getter = OopMethodSig {
                            name: format!("get_{}", p.name).into(),
                            vis: p.get_vis.unwrap_or(p.vis),
                            params: vec![],
                            ret: ty.clone(),
                            modifier: ast::MethodModifier::Abstract,
                            is_async: false,
                            generics: vec![],
                            is_static_abstract: false,
                        };
                        push_method(&mut methods, getter);
                    }
                    if p.has_set {
                        let setter = OopMethodSig {
                            name: format!("set_{}", p.name).into(),
                            vis: p.set_vis.unwrap_or(p.vis),
                            params: vec![ParamSig {
                                name: "value".into(),
                                ty: ty.clone(),
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
                        };
                        push_method(&mut methods, setter);
                    }
                }
                self.declared_properties
                    .insert(def_ast.name.clone(), iface_declared_props);
                self.shadow_insert(
                    def_ast.name.clone(),
                    NominalType {
                        name: def_ast.name.clone(),
                        kind: TypeKind::Interface,
                        vis: def_ast.vis,
                        is_abstract: false,
                        is_record: false,
                        is_readonly: false,
                        fields,
                        methods,
                        bases,
                        base_types: def_ast.bases.clone(),
                        span: *span,
                        variants: vec![],
                        generic_params: def_ast.generics.iter().map(|g| g.name.clone()).collect(),
                        namespace: namespace.to_vec(),
                        const_values: IndexMap::new(),
                        constructors: vec![],
                        soa: false,
                        required_props: Default::default(),
                    },
                );
            }
            HirItem::Enum { def_ast, span, .. } => {
                self.shadow_insert(
                    def_ast.name.clone(),
                    NominalType {
                        name: def_ast.name.clone(),
                        kind: TypeKind::Enum,
                        vis: def_ast.vis,
                        is_abstract: false,
                        is_record: false,
                        is_readonly: false,
                        fields: IndexMap::new(),
                        methods: IndexMap::new(),
                        bases: vec![],
                        base_types: vec![],
                        span: *span,
                        variants: variants_from_ast(&def_ast.variants),
                        generic_params: vec![],
                        namespace: namespace.to_vec(),
                        const_values: IndexMap::new(),
                        constructors: vec![],
                        soa: false,
                        required_props: Default::default(),
                    },
                );
            }
            // RFC 004 M1：variant 标签联合类型注册
            HirItem::Variant { def_ast, span, .. } => {
                self.shadow_insert(
                    def_ast.name.clone(),
                    NominalType {
                        name: def_ast.name.clone(),
                        kind: TypeKind::Variant,
                        vis: def_ast.vis,
                        is_abstract: false,
                        is_record: false,
                        is_readonly: false,
                        fields: IndexMap::new(),
                        methods: IndexMap::new(),
                        bases: vec![],
                        base_types: vec![],
                        span: *span,
                        variants: variant_cases_from_ast(&def_ast.cases),
                        generic_params: def_ast.generics.iter().map(|g| g.name.clone()).collect(),
                        namespace: namespace.to_vec(),
                        const_values: IndexMap::new(),
                        constructors: vec![],
                        soa: false,
                        required_props: Default::default(),
                    },
                );
            }
            // GAP #5：delegate 委托类型——仅在 check_module_items 中注册 alias，
            // 此处不创建 NominalType。
            HirItem::Delegate { .. } => {}
            _ => {}
        }
    }

    fn register_class(&mut self, def_ast: &ClassDef, namespace: &[Ident], span: Span) {
        let bases: Vec<_> = def_ast.bases.iter().filter_map(type_path_name).collect();
        let mut methods = IndexMap::new();
        for m in &def_ast.methods {
            let sig = method_sig_from_ast(&m.node.sig);
            push_method(&mut methods, sig);
        }
        let mut fields = fields_from_ast(&def_ast.fields);
        let mut required_props = indexmap::IndexSet::new();
        let mut declared_props: Vec<crate::oop_types::DeclaredPropertySig> = Vec::new();
        for p in &def_ast.properties {
            if p.is_required {
                required_props.insert(p.name.clone());
            }
            // 登记 `[Builtin]` 静态自动属性（MIR 源码形分派判定依据）。
            self.record_builtin_static_prop(&def_ast.name, p);
            // RFC 004 M1：接口 `static abstract T Prop { get; }` 不入 `fields`——
            // 它们是编译期通过 `interface_templates` 解析的（见
            // `check_static_abstract_field`），不参与 itable 虚分派。若入 `fields`，
            // `layouts_from_registry` 会为 `ilayout.properties` 生成 itable 槽位，
            // 要求实现类提供 `{Class}_get_{Prop}` 符号，但 static abstract 属性
            // 由 codegen 拦截器直接发射，无此符号 → "use of undefined value"。
            if p.is_static_abstract {
                continue;
            }
            let ty = type_path_name(&p.ty.node).unwrap_or_else(|| "unknown".into());
            // RFC 018 M3+：declared 属性名枚举（含自动属性 / 自定义访问器 / 索引器 Item）。
            declared_props.push(crate::oop_types::DeclaredPropertySig {
                name: p.name.clone(),
                ty: ty.clone(),
                can_read: p.has_get,
                can_write: p.has_set || p.has_init,
            });
            // RFC 018 M2 step 6: abstract property（`abstract T Prop { get; }`）
            // 无 get_body，但必须注册为 `get_Prop` abstract 方法（无 backing field），
            // 否则 MIR lower `is_custom_accessor_property` 失败走 FieldGet 路径，
            // 访问不存在的 backing field。OverrideAbstract 同理（重新声明为抽象）。
            // RFC 007：索引器永不生成 backing field，始终注册 get_Item/set_Item。
            // 访问器形态判定走单一事实源 `property_has_custom_accessors`；
            // abstract 属性（RFC 018 M2）无 backing field，须注册 get_X abstract
            // 方法，故在此附加 `is_abstract_prop`（检查层不需要此附加）。
            let is_abstract_prop = matches!(
                p.modifier,
                ast::MethodModifier::Abstract | ast::MethodModifier::OverrideAbstract
            );
            let is_custom = property_has_custom_accessors(p) || is_abstract_prop;
            if is_custom {
                if p.is_indexer() {
                    push_indexer_accessors(&mut methods, p, &ty);
                } else {
                    // RFC 006 A2：访问器体引用 `field` 时，该属性仍"自动"——合成
                    // backing field（名={Prop}__backing，与属性名区分），与 get/set
                    // 方法并存注册（见 field_keyword.rs）。
                    if uses_field(&p.get_body, &p.set_body) {
                        let fname = crate::field_keyword::backing_field_name(&p.name);
                        fields.insert(
                            fname.clone(),
                            FieldInfo {
                                name: fname,
                                ty: ty.clone(),
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
                    if p.has_get {
                        let getter = OopMethodSig {
                            name: format!("get_{}", p.name).into(),
                            vis: p.get_vis.unwrap_or(p.vis),
                            params: vec![],
                            ret: ty.clone(),
                            modifier: p.modifier,
                            is_async: false,
                            generics: vec![],
                            is_static_abstract: false,
                        };
                        push_method(&mut methods, getter);
                    }
                    if p.has_set || p.has_init {
                        let setter = OopMethodSig {
                            name: format!("set_{}", p.name).into(),
                            vis: p.set_vis.unwrap_or(p.vis),
                            params: vec![ParamSig {
                                name: "value".into(),
                                ty: ty.clone(),
                                is_ref: false,
                                is_out: false,
                                is_in: false,
                                is_params: false,
                                default: None,
                            }],
                            ret: "void".into(),
                            modifier: p.modifier,
                            is_async: false,
                            generics: vec![],
                            is_static_abstract: false,
                        };
                        push_method(&mut methods, setter);
                        if p.has_init {
                            self.init_only_props
                                .insert((def_ast.name.clone(), p.name.clone()));
                        }
                    }
                }
            } else {
                fields.insert(
                    p.name.clone(),
                    FieldInfo {
                        name: p.name.clone(),
                        ty,
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
        }
        self.declared_properties
            .insert(def_ast.name.clone(), declared_props);
        if let Some(host) = &def_ast.synthesized_host {
            self.synth_hosts.insert(def_ast.name.clone(), host.clone());
        }
        self.shadow_insert(
            def_ast.name.clone(),
            NominalType {
                name: def_ast.name.clone(),
                kind: TypeKind::Class,
                vis: def_ast.vis,
                is_abstract: def_ast.is_abstract,
                is_record: def_ast.is_record,
                is_readonly: false,
                fields,
                methods,
                bases,
                base_types: def_ast.bases.clone(),
                span,
                variants: vec![],
                generic_params: def_ast.generics.iter().map(|g| g.name.clone()).collect(),
                namespace: namespace.to_vec(),
                // RFC 012 M3-5: 注册阶段即预填字面量 const 值，
                // 使 `[AttributeUsage(AttributeTargets.X)]` 等成员路径
                // 解析在 `check_class_inner` 之前即可成功。
                const_values: const_values_from_ast(&def_ast.fields),
                constructors: { ctors_from_ast(&def_ast.constructors) },
                soa: false,
                required_props,
            },
        );
    }

    fn register_static_class(&mut self, def_ast: &ClassDef, namespace: &[Ident], span: Span) {
        let mut methods = IndexMap::new();
        // 同名方法数（含扩展方法），用于 `method_link_name` 的 overload 参数后缀——
        // 与 check_class 中 `extension_fn_templates` 的模板键保持一致。
        let name_counts: std::collections::HashMap<&ast::Ident, usize> = {
            let mut counts: std::collections::HashMap<&ast::Ident, usize> =
                std::collections::HashMap::new();
            for m in &def_ast.methods {
                *counts.entry(&m.node.sig.name).or_insert(0) += 1;
            }
            counts
        };
        for m in &def_ast.methods {
            let sig = method_sig_from_ast(&m.node.sig);
            if let Some(ext_ty) = extension_target_type(&m.node.sig) {
                let mut ext_sig = sig.clone();
                if !ext_sig.params.is_empty() {
                    ext_sig.params.remove(0);
                }
                // 决策 #7：泛型扩展方法保留泛型参数名，供 resolve_extension 接收者推断使用。
                let generic_params = sig.generics.clone();
                // 模板键 = method_link_name 产物（含 overload 参数后缀）+ 泛型个数。
                // 与 check_class 里 `extension_fn_templates` 的插入键必须逐字节一致。
                //
                // 泛型个数后缀消除"仅泛型个数不同的重载"的模板键冲突：如
                // `AddTransient<TService,TImpl>(this IServiceCollection)` 与
                // `AddTransient<TService>(this IServiceCollection)` 参数列表在类型
                // 擦除后完全相同，`method_link_name` 产出同一键 → extension_fn_templates
                // 后写覆盖 → 2 个 type_args 实例化时命中 1 泛型模板 → GenericArity 错误。
                // RFC 006：mangle 基底一律经单一权威函数 `extension_mangle_base`
                // 计算（registry 与 check_class 共用），保证调用点 call_name 与
                // 单态化方法体符号逐字节一致。
                let mangle_base: Ident = extension_mangle_base(
                    def_ast.name.as_str(),
                    &sig,
                    name_counts
                        .get(&m.node.sig.name)
                        .copied()
                        .unwrap_or(1)
                        .max(1),
                )
                .into();
                let template_key = format!("{}_{}", mangle_base, sig.generics.len()).into();
                self.extensions
                    .entry(ext_ty)
                    .or_default()
                    .push(ExtensionMethod {
                        container: def_ast.name.clone(),
                        method: ext_sig,
                        namespace: namespace.to_vec(),
                        generic_params,
                        template_key,
                        mangle_base,
                    });
            } else {
                push_method(&mut methods, sig);
            }
        }
        // RFC 029 M5（静态类属性）：static class 的自定义属性 getter/setter 注册为
        // `get_Prop` / `set_Prop` 方法（与 struct/普通类的注册一致），否则成员访问
        // `registry.resolve_method` 找不到 getter——`BarcodeReader.IsZxingAvailable`
        // 报 `no field or property`。auto-property 在 static class 上被 typeck 拒绝
        // （check_static_class），此处仅处理显式访问器体。
        for p in &def_ast.properties {
            let ty = type_path_name(&p.ty.node).unwrap_or_else(|| "unknown".into());
            // 访问器形态判定走单一事实源；auto-property 在 static class 上已被
            // typeck 拒绝，此处仅显式访问器体（与 `property_has_custom_accessors`
            // 等价，`[Builtin]` 分支不会触发）。
            let is_custom = property_has_custom_accessors(p);
            if !is_custom {
                continue;
            }
            if p.has_get {
                push_method(
                    &mut methods,
                    OopMethodSig {
                        name: format!("get_{}", p.name).into(),
                        vis: p.get_vis.unwrap_or(p.vis),
                        params: vec![],
                        ret: ty.clone(),
                        modifier: p.modifier,
                        is_async: false,
                        generics: vec![],
                        is_static_abstract: false,
                    },
                );
            }
            if p.has_set || p.has_init {
                push_method(
                    &mut methods,
                    OopMethodSig {
                        name: format!("set_{}", p.name).into(),
                        vis: p.set_vis.unwrap_or(p.vis),
                        params: vec![ParamSig {
                            name: "value".into(),
                            ty: ty.clone(),
                            is_ref: false,
                            is_out: false,
                            is_in: false,
                            is_params: false,
                            default: None,
                        }],
                        ret: "void".into(),
                        modifier: p.modifier,
                        is_async: false,
                        generics: vec![],
                        is_static_abstract: false,
                    },
                );
            }
        }
        if let Some(host) = &def_ast.synthesized_host {
            self.synth_hosts.insert(def_ast.name.clone(), host.clone());
        }
        self.shadow_insert(
            def_ast.name.clone(),
            NominalType {
                name: def_ast.name.clone(),
                kind: TypeKind::StaticClass,
                vis: def_ast.vis,
                is_abstract: false,
                is_record: false,
                is_readonly: false,
                fields: IndexMap::new(),
                methods,
                bases: vec![],
                base_types: vec![],
                span,
                variants: vec![],
                generic_params: vec![],
                namespace: namespace.to_vec(),
                const_values: IndexMap::new(),
                constructors: vec![],
                soa: false,
                required_props: Default::default(),
            },
        );
    }

    /// All namespace paths that declare at least one extension method.
    pub fn extension_namespace_paths(&self) -> Vec<Vec<Ident>> {
        let mut paths: Vec<Vec<Ident>> = self
            .extensions
            .values()
            .flat_map(|methods| methods.iter().map(|em| em.namespace.clone()))
            .collect();
        paths.sort();
        paths.dedup();
        paths
    }

    /// Resolve `using` import paths to extension-visible namespace paths.
    pub fn resolve_extension_imports(&self, imports: &[ImportBinding]) -> Vec<Vec<Ident>> {
        let known = self.extension_namespace_paths();
        let mut resolved = Vec::new();
        for import in imports {
            match import.kind {
                hir::ImportKind::Namespace | hir::ImportKind::Alias => {
                    Self::match_import_to_namespaces(&import.path, &known, &mut resolved);
                }
                hir::ImportKind::Type => {
                    if let Some(target) = import.path.last() {
                        if let Some(ns) = self.types.get(target).map(|t| t.namespace.clone()) {
                            if known.iter().any(|k| k == &ns) && !resolved.contains(&ns) {
                                resolved.push(ns);
                            }
                        }
                    }
                    // `using A.B;` may import a nested namespace (not only a type alias).
                    Self::match_import_to_namespaces(&import.path, &known, &mut resolved);
                }
            }
        }
        resolved
    }

    fn match_import_to_namespaces(
        import_path: &[Ident],
        known: &[Vec<Ident>],
        out: &mut Vec<Vec<Ident>>,
    ) {
        for ns in known {
            if crate::oop_types::namespace_matches_import(import_path, ns) && !out.contains(ns) {
                out.push(ns.clone());
            }
        }
    }

    pub fn is_generic_template(&self, name: &Ident) -> bool {
        self.types
            .get(name)
            .is_some_and(|t| !t.generic_params.is_empty())
    }

    pub fn is_static_class(&self, name: &Ident) -> bool {
        matches!(
            self.types.get(name).map(|t| t.kind),
            Some(TypeKind::StaticClass)
        )
    }

    pub fn get(&self, name: &Ident) -> Option<&NominalType> {
        self.types.get(name)
    }

    pub fn is_interface(&self, name: &Ident) -> bool {
        matches!(
            self.types.get(name).map(|t| t.kind),
            Some(TypeKind::Interface)
        )
    }

    pub fn is_class(&self, name: &Ident) -> bool {
        matches!(self.types.get(name).map(|t| t.kind), Some(TypeKind::Class))
    }

    pub fn is_struct(&self, name: &Ident) -> bool {
        matches!(self.types.get(name).map(|t| t.kind), Some(TypeKind::Struct))
    }

    pub fn is_enum(&self, name: &Ident) -> bool {
        matches!(self.types.get(name).map(|t| t.kind), Some(TypeKind::Enum))
    }

    /// RFC 005 自动 Copy 判定：`name`（应为已注册 struct）的字段传递闭包内
    /// 是否含 class 句柄。判定规则（与 abi_size_align 的类型分类对齐）：
    /// 基元 / `string`（不可变、不参与 ARC）/ `enum` → 纯值；已注册 struct →
    /// 递归下沉（visited 防环）；class / interface / variant / 数组（`{elem}_arr`
    /// 未注册名）/ delegate / 其余未知名 → 句柄。
    pub fn contains_class_handle(&self, name: &Ident) -> bool {
        let mut visited = std::collections::HashSet::new();
        self.contains_class_handle_inner(name, &mut visited)
    }

    fn contains_class_handle_inner(
        &self,
        name: &Ident,
        visited: &mut std::collections::HashSet<Ident>,
    ) -> bool {
        if !visited.insert(name.clone()) {
            return false;
        }
        let Some(nom) = self.nominal_lookup(name) else {
            return true;
        };
        if nom.kind != TypeKind::Struct {
            return true;
        }
        for f in nom.fields.values() {
            if f.is_static || f.is_const {
                continue;
            }
            let t = f.ty.as_str();
            if matches!(
                t,
                "int"
                    | "uint"
                    | "long"
                    | "ulong"
                    | "short"
                    | "ushort"
                    | "byte"
                    | "sbyte"
                    | "char"
                    | "float"
                    | "double"
                    | "bool"
                    | "string"
            ) {
                continue;
            }
            let tid = Ident::from(t);
            if self.is_enum(&tid) {
                continue;
            }
            if self.is_struct(&tid) {
                if self.contains_class_handle_inner(&tid, visited) {
                    return true;
                }
                continue;
            }
            return true;
        }
        false
    }

    /// RFC 037 M1: 判定 `name` 是否为值类型（用于 `param_assignable` 的 object 快捷路径）。
    ///
    /// 值类型 = 基元类型（int/long/short/byte/char/float/double/bool）或 struct/enum。
    /// 注意：`string` 在 C# 中是引用类型，此处返回 false。
    /// 此方法用于覆盖 mangled 泛型类（如 Signal_T、List_Func_T_T_bool）
    // 未注册到 registry.types 的场景——它们不是值类型，可赋值给 object。
    pub fn is_value_type_name(&self, name: &Ident) -> bool {
        matches!(
            name.as_str(),
            "int" | "long" | "short" | "byte" | "char" | "float" | "double" | "bool"
        ) || self.is_struct(name)
            || self.is_enum(name)
    }

    /// CD-30：按名取 nominal 类型——短名主索引 `types` 优先，命中即返；未命中
    /// 则回退 `shadowed_types`（FQN 键）。当下游拿到**已沿调用点 namespace 链解析
    /// 出的 FQN**（如批隔离下同名 variant `Batch.CaseN.Content`）时，短名索引查不到
    /// （它被遮蔽后按 FQN 存于 `shadowed_types`），须在此兜底。
    fn nominal_lookup(&self, name: &Ident) -> Option<&NominalType> {
        self.types
            .get(name)
            .or_else(|| self.shadowed_types.get(name.as_str()))
    }

    /// 可变版本的 `nominal_lookup`：按 **实际存储键**定位条目（短名胜者落 `types`，
    /// 被遮蔽输家落 `shadowed_types[FQN]`）。FQN 闭环要求输家的成员登记写回其自身
    /// 条目，而非误落短名胜者的主索引条目。
    pub(crate) fn nominal_mut(&mut self, name: &Ident) -> Option<&mut NominalType> {
        self.types
            .get_mut(name)
            .or_else(|| self.shadowed_types.get_mut(name.as_str()))
    }

    pub fn enum_variant<'a>(
        &'a self,
        enum_name: &Ident,
        variant: &Ident,
    ) -> Option<&'a EnumVariantInfo> {
        let nom = self.nominal_lookup(enum_name)?;
        if nom.kind != TypeKind::Enum {
            return None;
        }
        nom.variants.iter().find(|v| &v.name == variant)
    }

    pub fn enum_variants(&self, enum_name: &Ident) -> &[EnumVariantInfo] {
        self.nominal_lookup(enum_name)
            .filter(|t| t.kind == TypeKind::Enum)
            .map(|t| t.variants.as_slice())
            .unwrap_or(&[])
    }

    /// RFC 031 M1：判断类型是否为 variant 标签联合。
    pub fn is_variant(&self, name: &Ident) -> bool {
        matches!(
            self.nominal_lookup(name).map(|t| t.kind),
            Some(TypeKind::Variant)
        )
    }

    /// RFC 031 M1：获取 variant 类型的所有 case 信息（按 discriminant 顺序）。
    pub fn variant_cases(&self, variant_name: &Ident) -> &[EnumVariantInfo] {
        self.nominal_lookup(variant_name)
            .filter(|t| t.kind == TypeKind::Variant)
            .map(|t| t.variants.as_slice())
            .unwrap_or(&[])
    }

    /// RFC 031 M1：按 case 名查找 variant case 信息。
    pub fn variant_case<'a>(
        &'a self,
        variant_name: &Ident,
        case: &Ident,
    ) -> Option<&'a EnumVariantInfo> {
        let nom = self.nominal_lookup(variant_name)?;
        if nom.kind != TypeKind::Variant {
            return None;
        }
        nom.variants.iter().find(|v| &v.name == case)
    }

    /// CD-30（C# 单一身份）：按**精确名**取 NominalType，短名主索引命中不了时
    /// 回落碰撞输家的 FQN 索引。
    ///
    /// 短名主索引 `types` 只保留同名类型的「胜者」（键=短名），被遮蔽「输家」按其
    /// FQN 存于 `shadowed_types`。调用方传入的 `name` 可能已是 FQN（如 `Med.Shape`，
    /// 经类型解析 FQN 路由后），此时 `types.get(name)` 落空，须回落 `shadowed_types`
    /// 才能取到正确的 NominalType 及其成员/构造表。两索引键（短名 vs FQN）互不重叠，
    /// 对无碰撞 std 零行为变化。
    pub(crate) fn nominal_type(&self, name: &Ident) -> Option<&NominalType> {
        self.types
            .get(name)
            .or_else(|| self.shadowed_types.get(name.as_str()))
    }

    /// 构造函数签名查询（RFC 023 M0：为 codegen 工厂生成暴露）。
    ///
    /// 返回类型的所有构造函数签名；类型未注册或无构造函数返回空切片。
    /// `param_types` 为参数类型名列表（与 `ParamSig.ty` 同源），空列表表示无参构造。
    /// codegen 在 M1 拦截 `Add(ServiceDescriptor)` 后调用此 API 获取实现类型构造函数参数，
    /// 生成类型化工厂函数 `__di_factory_Foo(sp) { return new Foo(sp.GetRequiredService<P1>(), ...); }`。
    pub fn ctor_signatures(&self, name: &Ident) -> &[CtorSig] {
        self.nominal_type(name)
            .map(|t| t.constructors.as_slice())
            .unwrap_or(&[])
    }

    pub fn type_name_from_type_id(name: &str) -> Ident {
        name.into()
    }
}

/// Exact signature match for interface implementation.
/// 返回类型归一化：registry 将非泛型 `Task`（= Task<Void>）经
/// `type_id_to_field_name` 存为 `Task_void`，而类方法签名经 `type_path_name`
/// 可能存为裸 `Task`。二者语义等价，比较时归一以免误报 LSP 违规
/// （如 `EmailNotifier.Handle` 返回 `Task` vs 接口 `Task_void`）。
fn normalize_ret_type(s: &str) -> &str {
    if s == "Task" {
        "Task_void"
    } else {
        s
    }
}

fn signatures_compatible(iface: &OopMethodSig, class: &OopMethodSig) -> Result<(), String> {
    if iface.params.len() != class.params.len() {
        return Err(format!(
            "parameter count mismatch: expected {}, found {}",
            iface.params.len(),
            class.params.len()
        ));
    }
    for (ip, cp) in iface.params.iter().zip(class.params.iter()) {
        if ip.ty != cp.ty {
            return Err(format!(
                "parameter type `{}` does not match interface `{}`",
                cp.ty, ip.ty
            ));
        }
    }
    if normalize_ret_type(&iface.ret) != normalize_ret_type(&class.ret) {
        return Err(format!(
            "return type `{}` does not match interface `{}`",
            class.ret, iface.ret
        ));
    }
    Ok(())
}

/// LSP (C# rules): return types covariant (same or subtype), parameters invariant (same).
fn lsp_compatible(base: &OopMethodSig, derived: &OopMethodSig) -> Result<(), String> {
    if base.params.len() != derived.params.len() {
        return Err("parameter count changed in override".into());
    }
    for (bp, dp) in base.params.iter().zip(derived.params.iter()) {
        if bp.ty != dp.ty {
            return Err(format!(
                "parameter type changed from `{}` to `{}` (parameters are invariant)",
                bp.ty, dp.ty
            ));
        }
    }
    // Return type: must be same (MVP; full LSP allows covariant return types in C# 9+)
    if normalize_ret_type(&base.ret) != normalize_ret_type(&derived.ret) {
        return Err(format!(
            "return type changed from `{}` to `{}` (must match for LSP)",
            base.ret, derived.ret
        ));
    }
    Ok(())
}

fn fields_from_ast(fields: &[FieldDef]) -> IndexMap<Ident, FieldInfo> {
    fields
        .iter()
        .map(|f| {
            // RFC 044 M2：`Type::Infer` 字段 = 合成类提升字段（var/foreach 迭代
            // 变量/解构目标），类型由状态机方法体的首次赋值后置推断回填
            // （`__infer__` 哨兵，见 check_stmt 赋值检查）。
            let ty = match &f.ty.node {
                ast::Type::Infer => "__infer__".into(),
                _ => type_path_name(&f.ty.node).unwrap_or_else(|| "unknown".into()),
            };
            (
                f.name.clone(),
                FieldInfo {
                    name: f.name.clone(),
                    ty,
                    vis: f.vis,
                    is_const: f.is_const,
                    is_readonly: f.is_readonly,
                    is_init_only: false,
                    get_vis: None,
                    set_vis: None,
                    is_static: f.is_static,
                    // RFC 006 M4：保留静态字段初始化器，供 codegen 在 __sinit 中 emit。
                    // const 字段由 typeck 在 const_values 折叠为常量，init 不再需要。
                    init: if f.is_static && !f.is_const {
                        f.init.clone()
                    } else {
                        None
                    },
                },
            )
        })
        .collect()
}

fn variants_from_ast(variants: &[EnumVariant]) -> Vec<EnumVariantInfo> {
    // RFC 004：显式 `= N` 覆盖判别值，未显式者按前一成员 +1 自动递增（对齐
    // C# 枚举语义）。此前忽略 `v.discriminant` 恒取声明序下标——`enum E { A = 3 }`
    // 的 `E.A` 被静默折叠为 0，与文档「可通过显式 = N 覆盖」不符。
    let mut next_discriminant: i64 = 0;
    variants
        .iter()
        .map(|v| {
            let discriminant = v.discriminant.unwrap_or(next_discriminant);
            next_discriminant = discriminant.wrapping_add(1);
            EnumVariantInfo {
                name: v.name.clone(),
                fields: v
                    .fields
                    .iter()
                    .map(|f| {
                        (
                            f.name.clone(),
                            type_path_name(&f.ty.node).unwrap_or_else(|| "unknown".into()),
                        )
                    })
                    .collect(),
                discriminant: discriminant as u32,
                // RFC 004 M1：Enum variants 无 variant payload（始终 None）。
                payload: None,
            }
        })
        .collect()
}

/// RFC 004 M1：从 AST `VariantDef.cases` 构造 variant case 元信息。
///
/// case 顺序决定 discriminant（tag 值，0, 1, 2, ...）。
/// `payload` 为 case 的单一 payload 类型（`None` = 无 payload case 如 `Null`）。
fn variant_cases_from_ast(cases: &[VariantCase]) -> Vec<EnumVariantInfo> {
    cases
        .iter()
        .enumerate()
        .map(|(i, c)| EnumVariantInfo {
            name: c.name.clone(),
            // variant case 无多字段（fields 始终为空，payload 才是数据载体）
            fields: vec![],
            discriminant: i as u32,
            payload: c.payload.as_ref().and_then(|t| type_path_name(&t.node)),
        })
        .collect()
}

/// Extract constructor signatures from AST (RFC 026 M0：为 ctor_signatures API 提供数据)。
///
/// `from_module` 注册阶段即填充 `constructors`，使 `TypeRegistry::ctor_signatures`
/// 在不执行完整 `check_class_inner` 的情况下也可用。`check_class_inner` 后续会再次
/// 填充（幂等覆盖），两侧数据源一致（均从 `ConstructorDef.params` 提取类型名）。
pub(crate) fn ctors_from_ast(ctors: &[Spanned<ConstructorDef>]) -> Vec<CtorSig> {
    ctors
        .iter()
        .map(|c| {
            let mut sig = ctor_sig_from_params(c.node.vis, &c.node.params);
            sig.sets_required_members = members_assigned_in_ctor_body(&c.node.body);
            sig
        })
        .collect()
}

/// RFC 006 M4：扫描 ctor 体中的 `this.P = …` / `P = …` 赋值目标。
pub(crate) fn members_assigned_in_ctor_body(body: &Block) -> indexmap::IndexSet<Ident> {
    let mut set = indexmap::IndexSet::new();
    for stmt in &body.stmts {
        if let Stmt::Assign { target, .. } = &stmt.node {
            match &target.node {
                Expr::Field { receiver, field } => {
                    let is_this = matches!(receiver.node, Expr::This)
                        || matches!(&receiver.node, Expr::Ident(id) if id.as_str() == "this");
                    if is_this {
                        set.insert(field.clone());
                    }
                }
                Expr::Ident(name) => {
                    set.insert(name.clone());
                }
                _ => {}
            }
        }
    }
    set
}

/// 从构造器形参列表构建 `CtorSig`（含 RFC 007 默认值折叠）。
pub(crate) fn ctor_sig_from_params(vis: Visibility, params: &[Param]) -> CtorSig {
    let params: Vec<ParamSig> = params
        .iter()
        .map(|p| ParamSig {
            name: p.name.clone(),
            ty: type_path_name(&p.ty.node).unwrap_or_else(|| "unknown".into()),
            is_ref: p.is_ref,
            is_out: p.is_out,
            is_in: p.is_in,
            is_params: p.is_params,
            default: p
                .default
                .as_ref()
                .and_then(|e| crate::call_args::fold_param_default(&e.node)),
        })
        .collect();
    let param_types: Vec<Ident> = params.iter().map(|p| p.ty.clone()).collect();
    CtorSig {
        vis,
        param_types,
        params,
        sets_required_members: Default::default(),
    }
}

/// Extract literal const field values from AST (RFC 009 M3-5: 属性参数路径常量预填)。
///
/// `from_module` 注册阶段即填充 `const_values`，使 `resolve_member_path` 等查询
/// 在不执行完整 `check_class_inner` 的情况下也可用。`check_class_inner` 后续会再次
/// 填充（幂等覆盖，参见 `eval_const_init`）。
///
/// 仅识别字面量初始化器（IntLit/FloatLit/BoolLit/StringLit）；非字面量
/// 初始化器（如路径表达式、二进制表达式）跳过，留待 `check_class_inner`
/// 在 Pass 2 报告错误或求值（保持与原有错误信息一致）。
fn const_values_from_ast(fields: &[FieldDef]) -> IndexMap<Ident, ConstValue> {
    let mut out = IndexMap::new();
    for f in fields {
        if !f.is_const {
            continue;
        }
        let Some(init) = &f.init else {
            continue;
        };
        let cv = match &init.node {
            Expr::IntLit(n) => ConstValue::Int(*n),
            Expr::FloatLit(ast::FloatLitValue::Double(x)) => ConstValue::Float(*x),
            Expr::FloatLit(ast::FloatLitValue::Float(x)) => ConstValue::Float(*x as f64),
            Expr::BoolLit(b) => ConstValue::Bool(*b),
            Expr::StringLit(s) => ConstValue::String(s.clone()),
            _ => continue, // 非字面量：留给 check_class_inner 处理
        };
        out.insert(f.name.clone(), cv);
    }
    out
}

/// RFC 007：将索引器注册为 `get_Item` / `set_Item`（带索引参数）。
fn push_indexer_accessors(
    methods: &mut IndexMap<Ident, Vec<OopMethodSig>>,
    p: &PropertyDef,
    ty: &Ident,
) {
    let index_params: Vec<ParamSig> = p
        .index_params
        .iter()
        .map(|ip| ParamSig {
            name: ip.name.clone(),
            ty: type_path_name(&ip.ty.node).unwrap_or_else(|| "unknown".into()),
            is_ref: ip.is_ref,
            is_out: ip.is_out,
            is_in: ip.is_in,
            is_params: ip.is_params,
            default: None,
        })
        .collect();
    if p.has_get {
        push_method(
            methods,
            OopMethodSig {
                name: "get_Item".into(),
                vis: p.get_vis.unwrap_or(p.vis),
                params: index_params.clone(),
                ret: ty.clone(),
                modifier: p.modifier,
                is_async: false,
                generics: vec![],
                is_static_abstract: false,
            },
        );
    }
    if p.has_set {
        let mut set_params = index_params;
        set_params.push(ParamSig {
            name: "value".into(),
            ty: ty.clone(),
            is_ref: false,
            is_out: false,
            is_in: false,
            is_params: false,
            default: None,
        });
        push_method(
            methods,
            OopMethodSig {
                name: "set_Item".into(),
                vis: p.set_vis.unwrap_or(p.vis),
                params: set_params,
                ret: "void".into(),
                modifier: p.modifier,
                is_async: false,
                generics: vec![],
                is_static_abstract: false,
            },
        );
    }
}

fn method_sig_from_ast(sig: &MethodSig) -> OopMethodSig {
    OopMethodSig {
        name: sig.name.clone(),
        vis: sig.vis,
        params: sig
            .params
            .iter()
            .map(|p| ParamSig {
                name: p.name.clone(),
                ty: type_path_name(&p.ty.node).unwrap_or_else(|| "unknown".into()),
                is_ref: p.is_ref,
                is_out: p.is_out,
                is_in: p.is_in,
                is_params: p.is_params,
                default: p
                    .default
                    .as_ref()
                    .and_then(|e| crate::call_args::fold_param_default(&e.node)),
            })
            .collect(),
        ret: sig
            .ret
            .as_ref()
            .and_then(|t| type_path_name(&t.node))
            .unwrap_or_else(|| "void".into()),
        modifier: sig.modifier,
        is_async: sig.is_async,
        // 决策 #7：保留方法泛型参数名列表，供泛型扩展方法接收者推断使用。
        generics: sig.generics.iter().map(|g| g.name.clone()).collect(),
        // RFC 004 M1：透传 `static abstract` 标记，供 `check_interface_impl` 跳过实例校验。
        is_static_abstract: sig.is_static_abstract,
    }
}

fn extension_target_type(sig: &MethodSig) -> Option<Ident> {
    let first = sig.params.first()?;
    if !first.is_extension_receiver {
        return None;
    }
    type_path_name(&first.ty.node)
}

pub(crate) fn type_path_name(ty: &Type) -> Option<Ident> {
    match ty {
        // 泛型实例化类型（如 `List<Element>`、`Dictionary<string,int>`）必须 mangle
        // 为单态化命名（`List_Element`、`Dictionary_string_int`），否则 registry
        // 存储 `List` 而非 `List_Element`，导致后续 `this.Children.Add(child)` 等
        // 方法调用的 receiver_type 退化为未单态化名，codegen 无法触发 builtin
        // dispatch（`parse_list_elem` 返回 None），亦无法匹配已实例化的
        // `@List_Element_Add` 符号，最终链接失败（`use of undefined value '@List_Add'`）。
        Type::Named { path, generics } if !generics.is_empty() => {
            resolve_instantiated_type_name(ty).map(Ident::from)
        }
        // 裸 `Action`（无类型实参）≡ `Func<void>`；须与 `type_id_to_field_name(Func{[],Void})`
        // / `Action<…>`→`Func_*_void`（上方 generics 分支）一致，否则多 overload 静态方法
        // （如 `Assert.Throws`）出现 call=`Throws_string_Action`、define=`Throws_string_Func_void`。
        Type::Named { path, .. } => {
            let name = path.last().cloned()?;
            if name.as_str() == "Action" {
                Some("Func_void".into())
            } else {
                Some(name)
            }
        }
        // 可空引用类型（`T?`）归约为内部类型名：`object?` → "object"、`ILogger?` → "ILogger"。
        // C# 中可空引用类型与基础类型在签名兼容性上等价（仅编译期标注，非独立类型），
        // 故 OOP 签名比较（signatures_compatible/lsp_compatible 按名等价）与方法调用
        // 返回类型解析（TypeId::Named(ret) → canonical_type）均按内部类型名处理。
        // 修复前 `Type::Nullable` 落入 `_ => None` 分支，导致可空返回类型被存为 "void"、
        // 可空参数被存为 "unknown"，进而触发 "expected object?, found void" 与
        // "return type `object` does not match interface `void`" 等错误。
        Type::Nullable { inner } => type_path_name(&inner.node),
        // `T[]` → `{elem}_arr`，与 `type_id_to_field_name(TypeId::Array{…})` 对齐；
        // 否则 registry 形参落成 "unknown"，`Write(byte[]…)` 等重载永不匹配。
        Type::Array { inner } => {
            let elem = type_path_name(&inner.node)?;
            Some(format!("{elem}_arr").into())
        }
        _ => None,
    }
}
