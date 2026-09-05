//! Class layout metadata for codegen: field offsets and interface vtables.

use crate::call_args::fold_param_default;
use crate::oop_types::{
    method_params_match, EnumVariantInfo, NominalType, OopMethodSig, TypeKind, TypeRegistry,
};
use crate::type_fqn;
use ast::{Expr, Ident, MethodModifier, Spanned};
use indexmap::IndexMap;
use indexmap::IndexSet;

/// Byte size of Arc object header (refcount + vtable pointer).
pub const HEADER_SIZE: u32 = 16;

#[derive(Clone, Debug)]
pub struct FieldLayout {
    pub name: Ident,
    pub ty: Ident,
    pub offset: u32,
}

/// RFC 006 M4：静态字段布局信息——供 codegen 发射 `@__static_<Class>_<field>`
/// 全局变量与 `__sinit_<Class>` 静态初始化器。
///
/// `init` 为字段声明中的初始化器（`static int _count = 0;` 的 `= 0` 部分），
/// 由 typeck 收集，codegen 在 `__sinit_<Class>` 函数体内 emit 为 store 指令。
/// `None` 时全局变量保持 `zeroinitializer`，无 `__sinit` 调用。
#[derive(Clone, Debug)]
pub struct StaticFieldLayout {
    pub class: Ident,
    pub field: Ident,
    pub ty: Ident,
    pub init: Option<Spanned<Expr>>,
    /// RFC 006 A3：是否惰性初始化（`static readonly` 且初始化器为
    /// 非编译期常量表达式，如 `new`/方法调用）。`true` 时 codegen（S3）
    /// 首次访问才构造并加线程安全 guard；`false` 保持急切 `__sinit` 零开销。
    pub is_lazy: bool,
}

/// RFC 018 M2: 方法布局信息（供 codegen declared_methods 数组使用）。
#[derive(Clone, Debug)]
pub struct MethodLayout {
    pub name: Ident,
    pub return_type: Ident,
    pub param_count: u32,
    /// 形参类型名（与 `OopMethodSig.params[].ty` 同源）；供 itable 适配器 thunk
    /// 转发带参方法 ABI（RFC 009 P1-C2.6 带参协变返回）。
    pub param_types: Vec<Ident>,
    pub is_static: bool,
    /// 完整链接名（`Class::M` 或重载时 `Class::M_paramtypes`，与 MIR 函数符号一致）。
    /// 由 typeck `method_link_name_for` 计算，codegen vtable/itable 槽位直接引用。
    pub link_name: String,
}

/// RFC 018 M3+: 属性布局信息（供 codegen declared_properties / GetProperties）。
#[derive(Clone, Debug)]
pub struct PropertyLayout {
    pub name: Ident,
    pub property_type: Ident,
    pub can_read: bool,
    pub can_write: bool,
}

/// 虚方法槽（CD-10/D1 修复：槽位身份 = 完整签名）。
///
/// 槽位键含**方法名 + 形参类型**（对齐 C# MethodTable 槽语义：重载各占其槽、
/// `override` 复用基类同名同签名槽位）。`impl_class` / `link_name` 为该类
/// vtable 中该槽的**最终实现**（沿继承链解析的最派生 override），在 typeck
/// 一次性解析，codegen 直接引用，杜绝「按名取实现」导致的错位分派。
#[derive(Clone, Debug)]
pub struct VirtualSlot {
    pub name: Ident,
    pub ret: Ident,
    pub params: Vec<Ident>,
    /// 最派生实现类（该槽在此类 vtable 中的最终 override / 实现）。
    pub impl_class: Ident,
    /// 完整链接名（含重载消歧后缀），如 `Calc::Describe_string`。
    pub link_name: String,
}

#[derive(Clone, Debug)]
pub struct ClassLayout {
    pub name: Ident,
    pub fields: Vec<FieldLayout>,
    pub parent: Option<Ident>,
    /// 完整接口集（自身声明 + 接口继承父接口 + variance 视图 + **类父链继承**）。
    /// 类父链继承（CD-11/D2 修复）：派生类继承基类的接口实现，须发射自己的
    /// itable 使接口引用分派命中派生类 override。
    pub interfaces: Vec<Ident>,
    /// (method_name, params) -> implementing class。签名即键：重载各占其键，
    /// 派生 override 覆盖基类同签名条目（沿继承链的最终实现）。
    pub method_impl: IndexMap<(Ident, Vec<Ident>), Ident>,
    /// Ordered vtable slots: 签名槽（name + params + ret + 最终实现）for virtual/override/abstract methods.
    pub virtual_slots: Vec<VirtualSlot>,
    /// Whether this class needs a vtable (true iff virtual_slots non-empty).
    pub has_vtable: bool,
    /// 构造函数参数类型名列表（RFC 023 M1：DI 工厂生成所需）。
    /// 每个内层 Vec 是一个构造函数的参数类型名（按声明顺序）；空 Vec 表示无参构造。
    /// 多构造函数重载时按声明顺序记录全部；DI 工厂生成默认取首个（M1 限制）。
    pub constructors: Vec<Vec<Ident>>,
    /// RFC 018 M2: 本类型声明的方法列表（含重载，不含继承）。
    pub declared_methods: Vec<MethodLayout>,
    /// RFC 018 M3+: 本类型声明的属性列表（不含继承）。
    pub declared_properties: Vec<PropertyLayout>,
}

impl ClassLayout {
    /// 对象字节数（header + 字段），含末字段 `_handle` 的 ptr 宽。
    pub fn size_bytes(&self) -> u32 {
        let empty = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            shadowed_types: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            entry_package: None,
            delegate_aliases: std::collections::HashMap::new(),
        };
        let end = self
            .fields
            .last()
            .map(|f| {
                let (sz, _) =
                    abi_size_align(&empty, self.name.as_str(), f.name.as_str(), f.ty.as_str());
                f.offset + sz
            })
            .unwrap_or(HEADER_SIZE);
        end.max(HEADER_SIZE)
    }
}

#[derive(Clone, Debug, Default)]
pub struct StructLayout {
    pub name: Ident,
    pub fields: Vec<FieldLayout>,
    pub is_readonly: bool,
    /// RFC 009 M4：`[SoA]` attribute 标记。true 时 codegen 将该 struct 数组
    /// 布局从 AoS 重排为 SoA（每字段独立连续数组）。
    /// 仅 struct 可置 true；由 `layouts_from_registry` 从 `NominalType.soa` 透传。
    pub soa: bool,
    /// RFC 004 P0 Phase 2：struct 显式声明的接口集（自身 `bases` 接口 + 接口
    /// 继承扁平化 + variance 视图）。codegen 据此发射 `@.itable.{Struct}_Box_{Iface}`。
    pub interfaces: Vec<Ident>,
    /// (method_name, params) -> implementing struct。签名即键，与 `ClassLayout`
    /// 同构（struct 无父链，仅自身声明方法）。
    pub method_impl: IndexMap<(Ident, Vec<Ident>), Ident>,
    /// 本 struct 声明的方法列表（含重载）。`link_name` 与 MIR 函数符号一致，
    /// 供 codegen 值接收者 thunk 直接引用具体实现符号。
    pub declared_methods: Vec<MethodLayout>,
}

/// RFC 004 M1：variant 类型布局信息（codegen 用）。
///
/// 内存布局：`{ u8 tag; [3 x u8] pad; <payload_union> }`。
/// - `tag`：case discriminant（u8，由 `EnumVariantInfo.discriminant` 转换）
/// - `pad`：3 字节对齐填充，保证 payload 4 字节对齐
/// - `payload_union`：所有 case 的 payload 类型组成的 LLVM union
///
/// case 列表与 discriminant 顺序与 `TypeRegistry::variant_cases` 一致。
/// 无 payload 的 case `payload` 为 None，仅占 tag 不占 union slot。
#[derive(Clone, Debug)]
pub struct VariantLayout {
    pub name: Ident,
    pub cases: Vec<EnumVariantInfo>,
}

#[derive(Clone, Debug, Default)]
pub struct ProgramLayouts {
    pub classes: IndexMap<Ident, ClassLayout>,
    pub structs: IndexMap<Ident, StructLayout>,
    pub enums: IndexSet<Ident>,
    /// 枚举成员判别值表（枚举名 → [(成员名, discriminant)]，按声明序）。
    ///
    /// codegen `__sinit_<Class>` 直 emit 路径（`emit_static_init_expr`）不经 MIR，
    /// 无法复用 MIR 的 `enum_variant_operand` 折叠——静态字段初始化器中的
    /// 枚举成员访问（`HorizontalAlignment.Stretch`）须据此表折叠为 `i32` 常量，
    /// 否则落入零值兜底，枚举 0 值静默顶替真实判别值（Left=0 顶替 Stretch=3）。
    /// 与 `enums` 同步由 `layouts_from_registry` 填充，二者同源一致性由类型注册表保证。
    pub enum_variants: IndexMap<Ident, Vec<(Ident, i64)>>,
    pub interfaces: IndexMap<Ident, InterfaceLayout>,
    /// RFC 004 M1：variant 类型布局表，供 codegen 发射 `%variant.{Name}` 类型
    /// 与构造/提取/析构 IR 使用。
    pub variants: IndexMap<Ident, VariantLayout>,
    /// RFC 006 M4：所有类（含基类链继承的静态字段）的静态字段布局。
    /// 由 `layouts_from_registry` 收集，codegen 据此发射全局变量与 `__sinit_<Class>` 函数。
    pub static_fields: Vec<StaticFieldLayout>,
    /// RFC 037 M-D0：`[Observable]` auto-property 集合（(类名, 属性名) 对）。
    ///
    /// 由管线层从 `TypeChecker::observable_properties()`（AttributeTable
    /// `has_attr(def_id, "Observable")` 查询）填充；codegen 据此在 FieldSet
    /// 发射点合成「相等性短路 + 隐藏通知通道（`Signal<T>`）」。
    /// custom-accessor 属性（无字段注册）不在此集合内，codegen 不插桩。
    pub observable_properties: IndexSet<(Ident, Ident)>,
    /// RFC 018 M2：类型键 → 点分全限定名（`Ns.Type`）映射。键与 classes/
    /// structs/interfaces/enums 各表同源（短名胜者 / 碰撞输家 FQN），值由
    /// HIR namespace 经 `type_fqn` 拼接。codegen 发射 RtTypeInfo.full_name/ns
    /// 时查表；键缺失回退键名。`name` 字段与 type_id 哈希输入不受影响
    /// （RFC 026 `type_name_to_id` 勿动共识）。
    pub type_full_names: IndexMap<String, String>,
}

/// Interface layout for codegen: method signatures and property signatures for vtable emission.
#[derive(Clone, Debug)]
pub struct InterfaceLayout {
    pub name: Ident,
    /// (method_name, return_type, param_types) — methods declared on the interface.
    pub methods: Vec<(Ident, Ident, Vec<Ident>)>,
    /// (property_name, property_type) — properties declared on the interface.
    pub properties: Vec<(Ident, Ident)>,
    /// RFC 006「接口泛型方法分派」：泛型方法实例化槽位名（如 "Get__Seed"）。
    /// 由 pipeline 在 MIR lowering 后填充（`collect_iface_generic_instances`），
    /// 供 `emit_itables`（发射槽位）与 `iface_method_index`（查找槽位）共享。
    /// 槽位顺序：methods → properties → generic_instances（全局排序确定）。
    /// 仅存实例化名——槽位发射（mangle）与查找（position）均只需名称，签名
    /// 由调用点携带，无需在此冗余。
    pub generic_instances: Vec<String>,
}

pub fn layouts_from_registry(reg: &TypeRegistry) -> ProgramLayouts {
    let mut classes = IndexMap::new();
    let mut structs = IndexMap::new();
    let mut enums = IndexSet::new();
    let mut enum_variants: IndexMap<Ident, Vec<(Ident, i64)>> = IndexMap::new();
    let mut interfaces = IndexMap::new();
    let mut variants = IndexMap::new();
    // RFC 006 M4：收集所有类的静态字段（含基类链继承），供 codegen 发射全局变量与 __sinit。
    let mut static_fields: Vec<StaticFieldLayout> = Vec::new();
    // RFC 018 M2：类型键 → 点分 FQN 映射（与各布局表同键同源）。
    let mut type_full_names: IndexMap<String, String> = IndexMap::new();
    // CD-11/D2：每类的「自身声明接口闭包」（自身 bases 接口 + AST 祖先 + variance 视图），
    // 供传播 pass 把类父链继承的接口并入派生类 `interfaces`。
    let mut own_ifaces: IndexMap<Ident, Vec<Ident>> = IndexMap::new();
    // CD-18 G1：层级中被派生类覆写的实例方法签名集合（基类普通方法据此进 vtable，
    // 参与默认虚分派）。一次性计算，供各类的 `collect_virtual_slots` 共享。
    let overridden_sigs = overridden_signature_set(reg);
    for (name, ty) in &reg.types {
        if ty.kind == TypeKind::Enum {
            enums.insert(name.clone());
            // 同步收集成员判别值表（codegen sinit 直 emit 路径的常量折叠数据源）。
            // discriminant 语义对齐 MIR `enum_variant_operand`（`discriminant as i64`）。
            enum_variants.insert(
                name.clone(),
                ty.variants
                    .iter()
                    .map(|v| (v.name.clone(), v.discriminant as i64))
                    .collect(),
            );
            continue;
        }
        if ty.kind == TypeKind::Variant {
            variants.insert(
                name.clone(),
                VariantLayout {
                    name: name.clone(),
                    cases: ty.variants.clone(),
                },
            );
            continue;
        }
        if ty.kind == TypeKind::Interface {
            // RFC 004 M1：`static abstract` 成员不进入 itable——它们是编译期
            // 通过类型约束解析的（基元类型由 `try_emit_primitive_static` 拦截器
            // 直接发射 LLVM 指令，零运行时开销），不参与虚分派。若纳入 itable，
            // codegen 会要求实现类提供对应符号（如 `ComparableBox_int_Compare`），
            // 但用户类未实现 static abstract 成员 → 触发 "use of undefined value"。
            // RFC 006：泛型方法声明（`sig.generics` 非空）不占常规 itable 槽位——
            // 其 ABI 取决于类型实参，无法经固定槽位分派。仅经 `generic_instances`
            // 槽位分派（pipeline 在 MIR lowering 后填充）。
            //
            // CD-12/D3 修复：**接口继承扁平化**——父接口方法（沿 AST 继承链）
            // 槽位在前（COM 式继承布局），子接口自身方法在后。`iface_method_index`
            // 与 `emit_itables` 均按此扁平列表计算槽位，`IChild : IBase` 引用经
            // `IChild` 调用父接口方法命中正确槽位。签名去重（名+形参），
            // 重载接口方法各占其槽。
            let mut methods: Vec<(Ident, Ident, Vec<Ident>)> = Vec::new();
            let mut seen: IndexSet<(Ident, Vec<Ident>)> = IndexSet::new();
            let mut push_method = |mname: &Ident, ret: &Ident, params: Vec<Ident>| {
                if seen.insert((mname.clone(), params.clone())) {
                    methods.push((mname.clone(), ret.clone(), params));
                }
            };
            for ancestor in reg.collect_ast_iface_ancestors(name) {
                let Some(aty) = reg.types.get(&ancestor) else {
                    continue;
                };
                for sigs in aty.methods.values() {
                    for sig in sigs {
                        if !sig.is_static_abstract && sig.generics.is_empty() {
                            // 同下方自身循环：属性访问器不入 methods（见 285 循环注释）。
                            let is_getter =
                                sig.name.as_str().starts_with("get_") && sig.params.is_empty();
                            let is_setter =
                                sig.name.as_str().starts_with("set_") && sig.params.len() == 1;
                            if !is_getter && !is_setter {
                                push_method(
                                    &sig.name,
                                    &sig.ret,
                                    sig.params.iter().map(|p| p.ty.clone()).collect(),
                                );
                            }
                        }
                    }
                }
            }
            for sigs in ty.methods.values() {
                for sig in sigs {
                    if !sig.is_static_abstract && sig.generics.is_empty() {
                        // 属性访问器（`get_X` 零参 / `set_X` 单参）**不入**
                        // `methods`——接口属性由 `properties` 列表统一表示（下述
                        // 反推），`emit_itables` / `iface_method_index` 均按
                        // "methods 在前、properties 在后"计算槽位。若 getter 留在
                        // methods：发射侧对无真实实现的 getter 跳过槽位（fns 不
                        // 推进），而调用侧 `iface_method_index` 按 methods 索引
                        // 命中 → 槽位错位（`get_Body` 抢先槽 0、属性序整体偏移，
                        // web_ssr_html_bridge 运行时 AV 根因）。
                        let is_getter =
                            sig.name.as_str().starts_with("get_") && sig.params.is_empty();
                        let is_setter =
                            sig.name.as_str().starts_with("set_") && sig.params.len() == 1;
                        if !is_getter && !is_setter {
                            push_method(
                                &sig.name,
                                &sig.ret,
                                sig.params.iter().map(|p| p.ty.clone()).collect(),
                            );
                        }
                    }
                }
            }
            // 接口属性在 HIR→registry 阶段注册为 `get_X` / `set_X` 方法
            // （见 registry.rs 接口注册路径），非 fields。properties 列表按
            // **扁平序**收集：祖先接口属性在前（CD-12/D3 COM 式布局）、自身
            // 属性在后，均取自 `declared_properties`（注册期按声明序收集，
            // 见 registry.rs 接口分支；getter 已排除出 methods，不能再从
            // methods 反推）。
            let mut prop_set: IndexMap<Ident, Ident> = IndexMap::new();
            for ancestor in reg.collect_ast_iface_ancestors(name) {
                if let Some(props) = reg.declared_properties.get(&ancestor) {
                    for p in props {
                        if p.can_read {
                            prop_set.entry(p.name.clone()).or_insert(p.ty.clone());
                        }
                    }
                }
            }
            // 自身属性按声明序追加（`declared_properties` 保留接口声明序；
            // 已由祖先贡献的同名属性不重复）。
            if let Some(props) = reg.declared_properties.get(name) {
                for p in props {
                    if p.can_read {
                        prop_set.entry(p.name.clone()).or_insert(p.ty.clone());
                    }
                }
            }
            let properties: Vec<(Ident, Ident)> = prop_set.into_iter().collect();
            interfaces.insert(
                name.clone(),
                InterfaceLayout {
                    name: name.clone(),
                    methods,
                    properties,
                    generic_instances: Vec::new(),
                },
            );
            type_full_names.insert(
                name.as_str().to_string(),
                type_fqn(&ty.namespace, ty.name.as_str()),
            );
            continue;
        }
        if ty.kind == TypeKind::Struct {
            let mut fields = Vec::new();
            let mut offset = 0u32;
            for f in ty.fields.values() {
                if f.is_const || f.is_static {
                    continue;
                }
                let (size, align) =
                    abi_size_align(reg, name.as_str(), f.name.as_str(), f.ty.as_str());
                offset = align_offset(offset, align);
                fields.push(FieldLayout {
                    name: f.name.clone(),
                    ty: f.ty.clone(),
                    offset,
                });
                offset += size;
            }
            // RFC 006 V2：struct（值类型）静态字段收集。与 Class 分支并列——struct
            // 静态字段（`static readonly X = new X(...)`，如 Vector3.Zero / Guid.Empty）
            // 须写入 `layouts.static_fields`，供 codegen 发射 `@__static_<Struct>_<field>`
            // 全局与 `__sinit_<Struct>`（急切 beforefieldinit）。struct 无类继承链，
            // `collect_static_fields` 的基类 walk 在首个 struct 即终止。
            collect_static_fields(reg, name, &mut static_fields);
            // RFC 004 P0 Phase 2：struct 接口闭包收集（与 class 分支同源）。struct
            // 无父链，仅自身 `bases` 接口 + 接口继承扁平化 + variance 视图。
            let mut ifaces: Vec<Ident> = ty
                .bases
                .iter()
                .filter(|b| reg.is_interface(b))
                .cloned()
                .collect();
            let mut inherited = Vec::new();
            for iface in &ifaces {
                for ancestor in reg.collect_ast_iface_ancestors(iface) {
                    if !ifaces.contains(&ancestor) && !inherited.contains(&ancestor) {
                        inherited.push(ancestor);
                    }
                }
            }
            ifaces.extend(inherited);
            let declared_ifaces = ifaces.clone();
            for iface in &declared_ifaces {
                let Some(ity) = reg.types.get(iface) else {
                    continue;
                };
                for base in &ity.bases {
                    if !reg.is_interface(base) || ifaces.contains(base) {
                        continue;
                    }
                    if reg.iface_extends_via_ast(iface, base) {
                        continue;
                    }
                    ifaces.push(base.clone());
                }
            }
            let mut struct_method_impl = IndexMap::new();
            collect_methods(reg, name, &mut struct_method_impl);
            // RFC 004 P0 Phase 2：struct 声明方法列表（link_name 与 MIR 符号一致，
            // 供值接收者 thunk 引用具体实现）。
            let struct_declared_methods: Vec<MethodLayout> = ty
                .methods
                .values()
                .flat_map(|overloads| overloads.iter())
                .filter(|sig| !matches!(sig.modifier, MethodModifier::Static))
                .map(|sig| MethodLayout {
                    name: sig.name.clone(),
                    return_type: sig.ret.clone(),
                    param_count: sig.params.len() as u32,
                    param_types: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    is_static: false,
                    link_name: method_link_name_of(reg, name, sig),
                })
                .collect();
            structs.insert(
                name.clone(),
                StructLayout {
                    name: name.clone(),
                    fields,
                    is_readonly: ty.is_readonly,
                    // RFC 009 D3：从 `NominalType.soa` 透传，使 codegen 可识别 SoA struct
                    soa: ty.soa,
                    interfaces: ifaces,
                    method_impl: struct_method_impl,
                    declared_methods: struct_declared_methods,
                },
            );
            continue;
        }
        if ty.kind != TypeKind::Class {
            continue;
        }
        // Skip generic class templates — only monomorphized instantiations
        // (e.g., `ListEnumerator_int`, not `ListEnumerator`) get layouts/itables.
        if !ty.generic_params.is_empty() {
            continue;
        }
        let parent = ty.bases.iter().find(|b| reg.is_class(b)).cloned();
        // + variance 协变/逆变视图（`IGetter_Dog` → `IGetter_IAnimal`；
        //   `IConsumer_IAnimal` → `IConsumer_Dog`，供适配器 itable）。
        let mut interfaces: Vec<_> = ty
            .bases
            .iter()
            .filter(|b| reg.is_interface(b))
            .cloned()
            .collect();
        let mut inherited = Vec::new();
        for iface in &interfaces {
            for ancestor in reg.collect_ast_iface_ancestors(iface) {
                if !interfaces.contains(&ancestor) && !inherited.contains(&ancestor) {
                    inherited.push(ancestor);
                }
            }
        }
        interfaces.extend(inherited);
        // Variance 合成基类只在接口 `bases`、不在 `base_types`：纳入 class.interfaces
        // 以便发射 `@.itable.{Class}_{VarianceView}` 适配器 thunk itable。
        let declared = interfaces.clone();
        for iface in &declared {
            let Some(ity) = reg.types.get(iface) else {
                continue;
            };
            for base in &ity.bases {
                if !reg.is_interface(base) || interfaces.contains(base) {
                    continue;
                }
                if reg.iface_extends_via_ast(iface, base) {
                    continue;
                }
                interfaces.push(base.clone());
            }
        }
        // CD-11/D2：保存自身声明接口闭包（不含类父链继承），供传播 pass 使用。
        own_ifaces.insert(name.clone(), interfaces.clone());

        let mut fields = Vec::new();
        let mut offset = HEADER_SIZE;
        collect_fields(reg, name, &mut fields, &mut offset);

        // RFC 006 M4：收集本类（含基类链继承）的静态字段，供 codegen 发射全局变量与 __sinit。
        // const 字段不在此收集（由 typeck const_values 折叠为编译期常量，无运行时存储）。
        collect_static_fields(reg, name, &mut static_fields);

        let mut method_impl = IndexMap::new();
        collect_methods(reg, name, &mut method_impl);

        let mut virtual_slots: Vec<VirtualSlot> = Vec::new();
        collect_virtual_slots(reg, name, &overridden_sigs, &mut virtual_slots);
        // RFC 006 M3：**每个 class 都带 vtable**（类型身份句柄）。vtable slot0 恒为
        // `@.typeinfo.{Class}`，使 `o is T` 对任意 class 值（含无虚方法、无 class 字段
        // 的 plain class，如 `List<string>`）都能经 rt_obj_isa 判别；这也是 C# 语义
        // （每个引用类型对象都携带 MethodTable 指针）。此前 `has_vtable` 仅当
        // `virtual_slots` 非空或含 class 字段——无虚方法无 class 字段的 class 对象
        // 其 header vtable 槽为 null，`o is List<string>` 恒 false（OOP 挂账）。
        //
        // 虚方法 / finalizer 是否真实在场由 `virtual_slots`（emit_vtables 依此决定
        // 槽位与 slot 1/2 的 finalizer/walk 指针）单独决定；`has_vtable` 仅表示
        // 「对象有类型身份 vtable」，故恒为 true。含 class 字段的无虚方法 class
        // 仍会经 finalizer 释放嵌套字段（见 emit_vtables 的 has_class_fields 分支）。
        let _has_class_field = fields.iter().any(|f| {
            reg.types
                .get(&f.ty)
                .is_some_and(|t| matches!(t.kind, TypeKind::Class))
        });
        // RFC 006 M3：恒为 true（见上方注释）。虚方法/finalizer 是否真实在场由
        // `virtual_slots` / `has_class_field` 在 emit_vtables 单独决定。
        let has_vtable = true;

        // RFC 023 M1: 收集构造函数参数类型列表，供 DI 工厂生成使用。
        // 源数据与 `TypeRegistry::ctor_signatures` 一致（均来自 `ConstructorDef.params`）。
        let constructors: Vec<Vec<Ident>> = ty
            .constructors
            .iter()
            .map(|c| c.param_types.clone())
            .collect();

        // RFC 018 M2: 收集本类型声明的方法（含重载，不含继承）。
        // `link_name` 与 MIR 函数符号逐字节一致（method_link_name_for 语义），
        // 供 codegen vtable/itable 槽位直接引用（CD-10/D1：重载消歧后缀）。
        let declared_methods: Vec<MethodLayout> = ty
            .methods
            .values()
            .flat_map(|overloads| overloads.iter())
            .map(|sig| {
                let static_count = reg.method_overload_count_kind(name, &sig.name, true);
                let instance_count = reg.method_overload_count_kind(name, &sig.name, false);
                let is_static = matches!(sig.modifier, MethodModifier::Static);
                let link = if static_count > 0 && instance_count > 0 {
                    crate::oop_types::method_link_name_static_abi(
                        name.as_str(),
                        sig,
                        static_count,
                        instance_count,
                    )
                } else if is_static {
                    crate::oop_types::method_link_name(name.as_str(), sig, static_count.max(1))
                } else {
                    crate::oop_types::method_link_name(name.as_str(), sig, instance_count.max(1))
                };
                MethodLayout {
                    name: sig.name.clone(),
                    return_type: sig.ret.clone(),
                    param_count: sig.params.len() as u32,
                    param_types: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    is_static,
                    link_name: link,
                }
            })
            .collect();

        // RFC 018 M3+: 本类型声明的属性（自动属性 + 自定义访问器；不含继承）。
        let declared_properties: Vec<PropertyLayout> = reg
            .declared_properties
            .get(name)
            .map(|props| {
                props
                    .iter()
                    .map(|p| PropertyLayout {
                        name: p.name.clone(),
                        property_type: p.ty.clone(),
                        can_read: p.can_read,
                        can_write: p.can_write,
                    })
                    .collect()
            })
            .unwrap_or_default();

        classes.insert(
            name.clone(),
            ClassLayout {
                name: name.clone(),
                fields,
                parent,
                interfaces,
                method_impl,
                virtual_slots,
                has_vtable,
                constructors,
                declared_methods,
                declared_properties,
            },
        );
        type_full_names.insert(
            name.as_str().to_string(),
            type_fqn(&ty.namespace, ty.name.as_str()),
        );

        // RFC 006 M4：收集本类（含基类链继承）的静态字段（非 const）。
        // const 字段已由 typeck 在 `const_values` 中折叠为编译期常量，
        // codegen 通过 `try_const_operand` 直接内联，无需全局变量存储。
        collect_static_fields(reg, name, &mut static_fields);
    }
    // CD-11/D2：类父链接口传播——派生类继承基类的接口实现（C# 语义：
    // `TalkDerived : TalkBase(ITalk)` 中 `td as ITalk` 须命中派生类 override）。
    // 按 own_ifaces 并集（自身闭包 ∪ 各祖先 own 闭包）追加，保持确定性顺序。
    let class_names: Vec<Ident> = classes.keys().cloned().collect();
    for cname in class_names {
        let mut chain: Vec<Ident> = Vec::new();
        {
            let mut cur = classes.get(cname.as_str()).and_then(|c| c.parent.clone());
            while let Some(pn) = cur {
                chain.push(pn.clone());
                cur = classes.get(pn.as_str()).and_then(|c| c.parent.clone());
            }
        }
        let clayout = classes.get_mut(cname.as_str()).expect("class exists");
        for pn in chain {
            if let Some(o) = own_ifaces.get(pn.as_str()) {
                for i in o {
                    if !clayout.interfaces.contains(i) {
                        clayout.interfaces.push(i.clone());
                    }
                }
            }
        }
    }
    // CD-30 批处理扩容（阶段 A）：跨命名空间同名类——碰撞输家按 FQN 物化。
    //
    // 根因：`shadow_insert` 已按 FQN 在 `shadowed_types` 中保留被遮蔽类的
    // NominalType，但主循环只遍历 `reg.types`（短名胜者）→ 被遮蔽类无
    // ClassLayout，MIR/codegen 按短名坍塌 → comdat 串用、反射缺失。
    // 此处把 `shadowed_types` 里的 class 输家（如 `BatchX.Case1.Shape`，
    // 胜者 `BatchX.Case2.Shape` 占 `classes["Shape"]`）以其 FQN 为键物化出
    // **独立** ClassLayout，与胜者短名键并存 → 两个同名类在 `ProgramLayouts`
    // 中可区分（其 `name` 即 FQN，标识命名空间归属）。
    //
    // 决策 (A)：不改 `classes` 公共键型、不改胜者短名查找；仅碰撞输家走 FQN。
    // std 稳定面零变化——单入口包无同名碰撞 → `shadowed_types` 为空 → 本块
    // 零物化 → `classes` 逐字节不变 → e2e 零回归。B/MIR、C/codegen 阶段
    // 再按 FQN 发射符号 / 寻址（本轮保证类型身份层可区分）。
    for (fqn, ty) in &reg.shadowed_types {
        // 泛型模板经实例化另注册；无命名空间时 FQN==短名，与胜者短名键冲突
        // 无法共存（C# 同命名空间不可能同名）→ 防御跳过。
        if !ty.generic_params.is_empty() {
            continue;
        }
        match ty.kind {
            TypeKind::Class => {
                if classes.get(fqn.as_str()).is_some() {
                    continue;
                }
                classes.insert(fqn.clone().into(), shadowed_class_layout(reg, fqn, ty));
            }
            // CD-30 批处理扩容：碰撞输家 variant / enum 同样按 FQN 物化布局。
            // 主循环只遍历 `reg.types`（短名胜者）→ 其它命名空间的同名 variant /
            // enum（如批处理 `BatchVariants.Case{n}.Content`）无布局，MIR 沿 FQN
            // 构造 / 模式定位时 `%variant.{FQN}` / enum 成员无定义，触发
            // getelementptr unsized / 成员缺失。此处以 FQN 为键与胜者短名键并存。
            TypeKind::Variant => {
                if variants.get(fqn.as_str()).is_some() {
                    continue;
                }
                variants.insert(
                    fqn.clone().into(),
                    VariantLayout {
                        name: fqn.clone().into(),
                        cases: ty.variants.clone(),
                    },
                );
            }
            TypeKind::Enum => {
                if enums.contains(fqn.as_str()) {
                    continue;
                }
                enums.insert(fqn.clone().into());
                enum_variants.insert(
                    fqn.clone().into(),
                    ty.variants
                        .iter()
                        .map(|v| (v.name.clone(), v.discriminant as i64))
                        .collect(),
                );
            }
            _ => {}
        }
    }
    ProgramLayouts {
        classes,
        structs,
        enums,
        enum_variants,
        interfaces,
        variants,
        static_fields,
        observable_properties: Default::default(),
        type_full_names,
    }
}

/// CD-30 批处理扩容（阶段 A）：为被遮蔽（碰撞输家）class 构建 ClassLayout。
///
/// 被遮蔽类不在 `reg.types` 主索引（按 FQN 存于 `shadowed_types`），主循环依赖
/// `reg.types.get(name)` 的收集器对它不可用，故直接按该 NominalType 自身声明
/// 物化：实例字段偏移、本类声明方法（link_name 按其**自身**方法表计算重载数——
/// reg 表短名已被胜者占用，若按 reg 查会拿到胜者的重载数导致 link_name 污染）、
/// 构造器签名。`interfaces` / `virtual_slots` / 静态字段等跨类派生信息由 B/C
/// 阶段沿 FQN 解析（本轮只保证两个同名类在类型身份层可区分）。`name` 取 FQN，
/// 使该 ClassLayout 自描述地标识其命名空间归属。
fn shadowed_class_layout(reg: &TypeRegistry, fqn: &str, ty: &NominalType) -> ClassLayout {
    let mut fields = Vec::new();
    let mut offset = HEADER_SIZE;
    for f in ty.fields.values() {
        if f.is_const || f.is_static {
            continue;
        }
        let (size, align) = abi_size_align(reg, fqn, f.name.as_str(), f.ty.as_str());
        offset = align_offset(offset, align);
        fields.push(FieldLayout {
            name: f.name.clone(),
            ty: f.ty.clone(),
            offset,
        });
        offset += size;
    }
    // 按本类型自身方法表统计重载数（同 `method_overload_count_kind` 语义，
    // 但取 `ty.methods` 而非 `reg.types`——后者短名已被胜者占用）。
    let self_overload_count = |method: &Ident, want_static: bool| -> usize {
        ty.methods
            .get(method)
            .map(|sigs| {
                sigs.iter()
                    .filter(|s| matches!(s.modifier, MethodModifier::Static) == want_static)
                    .count()
            })
            .unwrap_or(0)
    };
    let declared_methods: Vec<MethodLayout> = ty
        .methods
        .values()
        .flat_map(|overloads| overloads.iter())
        .map(|sig| {
            let static_count = self_overload_count(&sig.name, true);
            let instance_count = self_overload_count(&sig.name, false);
            let is_static = matches!(sig.modifier, MethodModifier::Static);
            let link = if static_count > 0 && instance_count > 0 {
                crate::oop_types::method_link_name_static_abi(
                    fqn,
                    sig,
                    static_count,
                    instance_count,
                )
            } else if is_static {
                crate::oop_types::method_link_name(fqn, sig, static_count.max(1))
            } else {
                crate::oop_types::method_link_name(fqn, sig, instance_count.max(1))
            };
            MethodLayout {
                name: sig.name.clone(),
                return_type: sig.ret.clone(),
                param_count: sig.params.len() as u32,
                param_types: sig.params.iter().map(|p| p.ty.clone()).collect(),
                is_static,
                link_name: link,
            }
        })
        .collect();
    let mut method_impl = IndexMap::new();
    for (mname, sigs) in &ty.methods {
        for sig in sigs {
            let params: Vec<Ident> = sig.params.iter().map(|p| p.ty.clone()).collect();
            method_impl.insert((mname.clone(), params), fqn.into());
        }
    }
    ClassLayout {
        name: fqn.into(),
        fields,
        parent: ty.bases.iter().find(|b| reg.is_class(b)).cloned(),
        // 接口/虚槽/静态字段等跨类派生信息由 C 阶段沿 FQN 解析，本轮不物化。
        interfaces: Vec::new(),
        method_impl,
        virtual_slots: Vec::new(),
        has_vtable: true,
        constructors: ty
            .constructors
            .iter()
            .map(|c| c.param_types.clone())
            .collect(),
        declared_methods,
        declared_properties: Vec::new(),
    }
}

/// RFC 006 M4：收集 `class` 及其基类链中所有非 const 静态字段。
///
/// 与 `collect_fields` 对偶——后者收集实例字段（用于布局），
/// 本函数收集静态字段（用于全局变量 + `__sinit_<Class>` 发射）。
///
/// 同名字段在派生类中覆盖基类（与 C# 语义一致）；继承链上每个类的静态字段
/// 都会被收集，因为静态字段在 C# 中是类级别的（每个类有自己的存储位置）。
fn collect_static_fields(reg: &TypeRegistry, class: &Ident, out: &mut Vec<StaticFieldLayout>) {
    let mut current = Some(class.clone());
    while let Some(cn) = current {
        let Some(nom) = reg.types.get(&cn) else {
            break;
        };
        for (fname, finfo) in nom.fields.iter() {
            // const 字段已折叠为编译期常量（const_values），无运行时存储
            if finfo.is_const {
                continue;
            }
            if !finfo.is_static {
                continue;
            }
            // 同名静态字段去重（派生类 hide 基类静态字段时取派生类定义）
            if out.iter().any(|s| s.class == cn && s.field == *fname) {
                continue;
            }
            out.push(StaticFieldLayout {
                class: cn.clone(),
                field: fname.clone(),
                ty: finfo.ty.clone(),
                init: finfo.init.clone(),
                // RFC 006 A3 S1：惰性判定（Option C 混合），见 `is_lazy_static_field`。
                is_lazy: is_lazy_static_field(reg, finfo.is_readonly, &finfo.ty, &finfo.init),
            });
        }
        current = nom.bases.iter().find(|b| reg.is_class(b)).cloned();
    }
}

/// RFC 006 A3 S1：判定静态字段是否惰性初始化（Option C 混合）。
///
/// 惰性 = `static readonly` 且初始化器为非编译期常量表达式（class `new`/方法调用/
/// 复杂表达式）——首次访问才构造并加线程安全 guard；字面量常量 / 可变 `static`
/// / 无初始化器 / **值类型 `new`** → 急切 `__sinit` 零开销。
///
/// 值类型 `new`（如 `static readonly Vector3 Zero = new Vector3(0,0,0)`）判定为
/// **急切**（对齐 .NET beforefieldinit 惯用法：struct 构造无堆分配、无 observable
/// 副作用，急切零 guard 优于惰性分支），避免对热路径加 guard 开销。
fn is_lazy_static_field(
    reg: &TypeRegistry,
    is_readonly: bool,
    field_ty: &Ident,
    init: &Option<Spanned<Expr>>,
) -> bool {
    if !is_readonly {
        return false;
    }
    let Some(init) = init else {
        return false;
    };
    // 值类型 `new`（struct/基元构造）→ 急切零成本（beforefieldinit 惯用法）。
    if matches!(init.node, Expr::New { .. }) && reg.is_value_type_name(field_ty) {
        return false;
    }
    fold_param_default(&init.node).is_none()
}

fn collect_fields(reg: &TypeRegistry, class: &Ident, out: &mut Vec<FieldLayout>, offset: &mut u32) {
    let Some(nom) = reg.types.get(class) else {
        return;
    };
    if let Some(parent) = nom.bases.iter().find(|b| reg.is_class(b)) {
        collect_fields(reg, parent, out, offset);
    }
    for f in nom.fields.values() {
        // RFC 006 M3：const 与 static 字段均不进入实例 layout。
        // - const：编译期常量折叠，无运行时存储
        // - static：类级别共享存储，通过 `@__static_<Class>_<field>` 全局变量访问
        //   （codegen M4 发射），不占用实例对象 header 后的字段槽位
        if f.is_const || f.is_static {
            continue;
        }
        if out.iter().any(|x| x.name == f.name) {
            continue;
        }
        let (size, align) = abi_size_align(reg, class.as_str(), f.name.as_str(), f.ty.as_str());
        *offset = align_offset(*offset, align);
        out.push(FieldLayout {
            name: f.name.clone(),
            ty: f.ty.clone(),
            offset: *offset,
        });
        *offset += size;
    }
}

/// ABI size/align for a field type — **single source of truth**.
///
/// Aligns with [2.2 类型系统](docs) C 后端映射：`bool` → `int32_t`（4/4）。
/// Class fields, struct fields, and codegen `class_size` must all use this.
///
/// `owner`/`field`：运行时 facade 的 `_handle` 在 LLVM 侧存 `ptr`（8 字节），
/// 即使用户面声明为 `int`（历史约定）。须按 8/8 布局，禁止 4 字节槽写入 ptr。
pub fn abi_size_align(reg: &TypeRegistry, owner: &str, field: &str, ty: &str) -> (u32, u32) {
    if field == "_handle" && is_runtime_handle_owner(owner) {
        return (8, 8);
    }
    if let Some(nom) = reg.types.get(&Ident::from(ty)) {
        match nom.kind {
            TypeKind::Enum => return (4, 4),
            TypeKind::Struct => {
                let mut total: u32 = 0;
                let mut max_align: u32 = 1;
                for f in nom.fields.values() {
                    if f.is_const || f.is_static {
                        continue;
                    }
                    let (elem_size, elem_align) =
                        abi_size_align(reg, ty, f.name.as_str(), f.ty.as_str());
                    total = align_offset(total, elem_align);
                    total += elem_size;
                    max_align = max_align.max(elem_align);
                }
                let size = total.max(1);
                let size = align_offset(size, max_align.max(1));
                return (size.max(1), max_align.max(1));
            }
            TypeKind::Class | TypeKind::Interface => return (8, 8),
            _ => {}
        }
    }
    match ty {
        // bool → int32_t（规范）；与 class field_size 历史路径一致
        "int" | "uint" | "bool" | "char" | "float" => (4, 4),
        "long" | "ulong" | "double" | "string" => (8, 8),
        "short" | "ushort" => (2, 2),
        "byte" | "sbyte" => (1, 1),
        // 未知命名类型默认引用（ptr）
        _ => (8, 8),
    }
}

/// Byte size only — convenience for codegen class_size.
pub fn abi_size_of(reg: &TypeRegistry, ty: &str) -> u32 {
    abi_size_align(reg, "", "", ty).0
}

/// Align `offset` up to the next multiple of `align`.
fn align_offset(offset: u32, align: u32) -> u32 {
    if align == 0 {
        return offset;
    }
    let mask = align - 1;
    (offset + mask) & !mask
}

/// 单态化 / 非泛型 runtime facade：`_handle` 槽存 opaque `ptr`。
fn is_runtime_handle_owner(owner: &str) -> bool {
    matches!(
        owner,
        "StringBuilder" | "BlockingCollection" | "LinkedListNode"
    ) || owner.starts_with("Dictionary_")
        || owner.starts_with("Tensor_")
        || owner.starts_with("List_")
        || owner.starts_with("HashSet_")
        || owner.starts_with("Queue_")
        || owner.starts_with("Stack_")
        || owner.starts_with("LinkedList_")
        || owner.starts_with("SortedSet_")
        || owner.starts_with("SortedDictionary_")
        || owner.starts_with("Concurrent")
        || owner.starts_with("BlockingCollection_")
}

fn collect_methods(
    reg: &TypeRegistry,
    class: &Ident,
    out: &mut IndexMap<(Ident, Vec<Ident>), Ident>,
) {
    let Some(nom) = reg.types.get(class) else {
        return;
    };
    if let Some(parent) = nom.bases.iter().find(|b| reg.is_class(b)) {
        collect_methods(reg, parent, out);
    }
    for (mname, sigs) in &nom.methods {
        for sig in sigs {
            // CD-10/D1：签名即键（名+形参类型）。派生 override 覆盖基类同签名
            // 条目 → 沿链的最终实现；重载各占其键，互不劫持。
            let params: Vec<Ident> = sig.params.iter().map(|p| p.ty.clone()).collect();
            out.insert((mname.clone(), params), class.clone());
        }
    }
}

/// CD-18 G1：方法是否进入 vtable 参与虚分派。
///
/// - `Virtual` / `Override` / `Abstract` / `OverrideAbstract` 显式虚方法恒进 vtable；
/// - 基类**普通实例方法**若被某（传递）派生类以**同签名**方法覆写（无论是否显式
///   `override`），也须进 vtable——RFC 006「基类方法默认虚 dispatch」：否则基类引用
///   调用会被静默降级为静态分派（CD-18 G1 根因）。`overridden` 为**本类**（作为基类）
///   被覆写的签名集合（见 `overridden_signature_set`；键含声明类，跨类同名不污染）。
fn is_dispatchable(
    class: &Ident,
    sig: &OopMethodSig,
    overridden: &IndexSet<(Ident, Ident, Vec<Ident>)>,
) -> bool {
    if matches!(
        sig.modifier,
        MethodModifier::Virtual
            | MethodModifier::Override
            | MethodModifier::Abstract
            | MethodModifier::OverrideAbstract
    ) {
        return true;
    }
    if matches!(sig.modifier, MethodModifier::Static) {
        return false;
    }
    let params: Vec<Ident> = sig.params.iter().map(|p| p.ty.clone()).collect();
    overridden.contains(&(class.clone(), sig.name.clone(), params))
}

/// CD-18 G1：收集「被派生类覆写」的实例方法签名集合。
///
/// 签名 = **(声明类, 方法名, 形参类型)**。遍历每个类沿其**传递基类链**上溯：若某
/// 祖先声明了与当前类**同签名**的实例方法，则该签名视为被覆写——无论基类是否
/// `virtual`、派生类是否显式 `override`（RFC 006 G1：同签名实例方法即覆写）。
/// 据此基类普通实例方法进入 vtable，参与默认虚分派。
///
/// **键含声明类**（CD-29 根因修复）：仅**被覆写者所在的基类**的该方法进入 vtable——
/// 同名同参方法在不同类（`JsonWriter.WriteString(string)` vs 任意 std 类的
/// `WriteString(string)`）互不污染。旧实现以 `(方法名, 形参)` 全局签名作键，任何
/// 类的覆写都会把**所有类**的同名同参方法判为虚（is_dispatchable 误判 →
/// 非虚方法进 vtable → 调用点虚分派槽位错位——`WriteString(string,string)` 经
/// 槽 3 误调单参 `WriteString(string)`，参数错位致 JSON 序列化值缺失）。
fn overridden_signature_set(reg: &TypeRegistry) -> IndexSet<(Ident, Ident, Vec<Ident>)> {
    let mut set = IndexSet::new();
    for (_cname, cty) in &reg.types {
        if cty.kind != TypeKind::Class {
            continue;
        }
        for (mname, csigs) in &cty.methods {
            for csig in csigs {
                if matches!(csig.modifier, MethodModifier::Static) {
                    continue;
                }
                let mut cur = cty.bases.iter().find(|b| reg.is_class(b)).cloned();
                while let Some(bn) = cur {
                    let Some(bty) = reg.types.get(&bn) else {
                        break;
                    };
                    if bty.methods.get(mname).is_some_and(|bsigs| {
                        bsigs.iter().any(|b| {
                            !matches!(b.modifier, MethodModifier::Static)
                                && method_params_match(b, csig)
                        })
                    }) {
                        // 被覆写的基类方法归属 bn——只有 bn 的该方法需要进 vtable。
                        set.insert((
                            bn.clone(),
                            mname.clone(),
                            csig.params.iter().map(|p| p.ty.clone()).collect(),
                        ));
                        break;
                    }
                    cur = bty.bases.iter().find(|b| reg.is_class(b)).cloned();
                }
            }
        }
    }
    set
}

/// 类方法链接名（与 MIR 函数符号逐字节一致，见 `method_link_name_for`）。
fn method_link_name_of(reg: &TypeRegistry, class: &Ident, sig: &OopMethodSig) -> String {
    let static_count = reg.method_overload_count_kind(class, &sig.name, true);
    let instance_count = reg.method_overload_count_kind(class, &sig.name, false);
    if static_count > 0 && instance_count > 0 {
        crate::oop_types::method_link_name_static_abi(
            class.as_str(),
            sig,
            static_count,
            instance_count,
        )
    } else if matches!(sig.modifier, MethodModifier::Static) {
        crate::oop_types::method_link_name(class.as_str(), sig, static_count.max(1))
    } else {
        crate::oop_types::method_link_name(class.as_str(), sig, instance_count.max(1))
    }
}

/// CD-10/D1：收集虚方法槽（签名槽）。
///
/// 基类链先序收集（基类槽位在前）；派生类声明与基类虚/抽象方法**同签名**的
/// 实例方法即该槽的最派生实现（G1 默认虚 dispatch：无需显式 `override`，
/// 对齐旧 method_impl 按名解析语义；显式 `override` 与隐式同签名覆写同一路径）；
/// 新签名（重载 / 新虚方法）追加新槽。`overridden`（见 `overridden_signature_set`）
/// 使基类普通实例方法在被派生类覆写时也进入 vtable（RFC 006「基类方法默认虚 dispatch」）。
fn collect_virtual_slots(
    reg: &TypeRegistry,
    class: &Ident,
    overridden: &IndexSet<(Ident, Ident, Vec<Ident>)>,
    out: &mut Vec<VirtualSlot>,
) {
    let Some(nom) = reg.types.get(class) else {
        return;
    };
    if let Some(parent) = nom.bases.iter().find(|b| reg.is_class(b)) {
        collect_virtual_slots(reg, parent, overridden, out);
    }
    for (mname, sigs) in &nom.methods {
        for sig in sigs {
            let params: Vec<Ident> = sig.params.iter().map(|p| p.ty.clone()).collect();
            // G1 默认虚 dispatch（与基类 method_impl 按名解析语义一致）：派生类
            // 声明与基类虚/抽象方法**同签名**的实例方法即该槽的最派生实现——即使未
            // 显式标 `override`（如 `DeepSeekChatClient.CompleteAsync` 无
            // override 关键字，AIChatClient 基类声明 abstract）。未更新槽位实现
            // 会导致 vtable 槽指向基类抽象方法 → 调用崩溃。static 方法不参与虚分派。
            if !matches!(sig.modifier, MethodModifier::Static) {
                if let Some(existing) = out
                    .iter_mut()
                    .find(|s| s.name == *mname && s.params == params)
                {
                    existing.ret = sig.ret.clone();
                    existing.impl_class = class.clone();
                    existing.link_name = method_link_name_of(reg, class, sig);
                    continue;
                }
            }
            if is_dispatchable(class, sig, overridden) {
                out.push(VirtualSlot {
                    name: mname.clone(),
                    ret: sig.ret.clone(),
                    params,
                    impl_class: class.clone(),
                    link_name: method_link_name_of(reg, class, sig),
                });
            }
        }
    }
}

impl ProgramLayouts {
    /// RFC 005 自动 Copy 判定（codegen 侧桥）：`name` 为已注册 struct 且其
    /// 字段传递闭包内无 class 句柄 → true（可逐字段复制、赋值后源仍可用）。
    /// 与 typeck `TypeRegistry::contains_class_handle` 同规则：基元/`string`/
    /// enum → 纯值；已注册 struct → 递归下沉（visited 防环，命中已访问 →
    /// false，与 typeck 一致）；class / interface / variant / 数组 / delegate /
    /// 未知名 → 句柄。`copy_struct_inner` 返回「含句柄」语义（与 typeck 侧
    /// 同构，便于逐行审计一致性），此处取反为「可 Copy」。
    /// codegen 无 TypeRegistry 访问，据 `layouts.structs` / `layouts.enums`
    /// 独立判定；两处判定的一致性由同一规则文本约束。
    pub fn is_copy_struct(&self, name: &str) -> bool {
        if !self.structs.contains_key(name) {
            return false;
        }
        let mut visited = std::collections::HashSet::new();
        !self.copy_struct_inner(name, &mut visited)
    }

    /// 字段传递闭包是否含 class 句柄（true = 含句柄，不可 Copy——语义与
    /// typeck `contains_class_handle_inner` 同构）。
    fn copy_struct_inner(
        &self,
        name: &str,
        visited: &mut std::collections::HashSet<String>,
    ) -> bool {
        if !visited.insert(name.to_string()) {
            return false;
        }
        let Some(s) = self.structs.get(name) else {
            // 未知名视为句柄（与 typeck 侧 nominal_lookup miss 分支同构）。
            return true;
        };
        for f in &s.fields {
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
            if self.enums.contains(t) {
                continue;
            }
            if self.structs.contains_key(t) {
                if self.copy_struct_inner(t, visited) {
                    return true;
                }
                continue;
            }
            return true;
        }
        false
    }

    /// Byte size of a type name using this layout table + primitive ABI SSoT.
    pub fn size_of_ty(&self, ty: &str) -> u32 {
        if let Some(s) = self.structs.get(ty) {
            if let Some(last) = s.fields.last() {
                return last.offset + self.size_of_ty(last.ty.as_str());
            }
            return 1;
        }
        if self.enums.contains(ty) {
            return 4;
        }
        let empty = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            shadowed_types: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            entry_package: None,
            delegate_aliases: std::collections::HashMap::new(),
        };
        abi_size_align(&empty, "", "", ty).0
    }

    pub fn get_struct(&self, name: &str) -> Option<&StructLayout> {
        self.structs.get(name)
    }

    pub fn get(&self, class: &str) -> Option<&ClassLayout> {
        self.classes.get(class)
    }

    pub fn is_interface(&self, name: &str) -> bool {
        name.starts_with('I') && name.chars().nth(1).is_some_and(|c| c.is_uppercase())
    }

    /// Resolve static dispatch class for method on receiver type name.
    pub fn resolve_method_class(&self, receiver_type: &str, method: &str) -> Option<Ident> {
        if self.is_interface(receiver_type) {
            return None;
        }
        self.classes.get(receiver_type).and_then(|c| {
            c.method_impl
                .iter()
                .find(|((m, _), _)| m.as_str() == method)
                .map(|(_, v)| v.clone())
        })
    }

    /// CD-10/D1：虚方法槽索引（签名键）。
    ///
    /// 精确 (method, params) 匹配优先；仅当同名槽唯一时按名兜底（兼容
    /// 非重载路径与 params 缺失的调用点，避免重载歧义静默错位）。
    pub fn virtual_slot_index(&self, class: &str, method: &str, params: &[Ident]) -> usize {
        let Some(c) = self.classes.get(class) else {
            return 0;
        };
        if let Some(idx) = c
            .virtual_slots
            .iter()
            .position(|s| s.name.as_str() == method && s.params == params)
        {
            return idx;
        }
        let mut only: Option<usize> = None;
        for (i, s) in c.virtual_slots.iter().enumerate() {
            if s.name.as_str() == method {
                if only.is_some() {
                    return 0;
                }
                only = Some(i);
            }
        }
        only.unwrap_or(0)
    }

    /// CD-10/D1：虚方法声明返回类型（签名键）。
    pub fn virtual_method_ret_name(
        &self,
        class: &str,
        method: &str,
        params: &[Ident],
    ) -> Option<&str> {
        let c = self.classes.get(class)?;
        if let Some(s) = c
            .virtual_slots
            .iter()
            .find(|s| s.name.as_str() == method && s.params == params)
        {
            return Some(s.ret.as_str());
        }
        let matches: Vec<&VirtualSlot> = c
            .virtual_slots
            .iter()
            .filter(|s| s.name.as_str() == method)
            .collect();
        if matches.len() == 1 {
            return Some(matches[0].ret.as_str());
        }
        None
    }

    // ---- RFC 037 M-D0：`[Observable]` auto-property 通知合成布局查询 ----

    /// 属性是否为 `[Observable]` auto-property——codegen FieldSet 合成判定点。
    pub fn has_observable_property(&self, class: &str, field: &str) -> bool {
        self.observable_properties
            .iter()
            .any(|(c, f)| c.as_str() == class && f.as_str() == field)
    }

    /// 类是否带合成通知通道字段（存在 `[Observable]` auto-property）。
    ///
    /// codegen 据此在 LLVM struct 类型末尾追加 `ptr` 通道槽，并放大
    /// `class_size`（calloc 尺寸），保证通道 GEP 落在分配内。
    pub fn class_has_observable_channel(&self, class: &str) -> bool {
        self.observable_properties
            .iter()
            .any(|(c, _)| c.as_str() == class)
    }

    /// 该类全部 `[Observable]` auto-property 的规范序（类名, 属性名）列表。
    ///
    /// 按属性名升序排序保证确定性。该顺序是 **LLVM struct 发射
    /// （`emit_struct_types` 按序追加 N 个 `ptr`）、`class_size`
    /// （`align8(布局末) + N*8`）、通道偏移（`observable_channel_offset`
    /// 的 `k*8`）三处共享的唯一规范**——任一处错位都会使多属性类 GEP 与
    /// calloc 尺寸不一致，运行期必然崩溃（RFC 037 §5.3「每实例、每属性」
    /// 通道；2026-08-04 修复「多属性共享单通道槽」P0 缺陷）。
    pub fn class_observable_properties(&self, class: &str) -> Vec<(String, String)> {
        let mut props: Vec<(String, String)> = self
            .observable_properties
            .iter()
            .filter(|(c, _)| c.as_str() == class)
            .map(|(c, f)| (c.to_string(), f.to_string()))
            .collect();
        props.sort_by(|a, b| a.1.cmp(&b.1));
        props
    }

    /// 隐藏通知通道字段的字节偏移——紧随布局末字段之后、按 ptr（8 字节）对齐。
    ///
    /// **每 `[Observable]` auto-property 一个通道槽**（`ptr`，8 字节）：
    /// `align_to(8, 末字段末尾) + k*8`，k 为属性在规范序
    /// （`class_observable_properties`，按属性名升序）中的下标。
    /// 与 LLVM struct 布局一致（`emit_struct_types` 按规范序追加等量 `ptr`），
    /// `class_size` = `align_to(8, 末字段末尾) + N*8`。按符号静态定址
    /// （GEP 常量偏移），绝无运行期字符串查找（RFC 016 §4.2）。
    /// field 不在规范序中时返回槽区首址 `align8(base)`（防御——调用方应
    /// 先经 `has_observable_property` 判定，此处仅保证不越界）。
    pub fn observable_channel_offset(&self, class: &str, field: &str) -> u64 {
        let base = self
            .classes
            .get(class)
            .map(|c| c.size_bytes() as u64)
            .unwrap_or(HEADER_SIZE as u64);
        let aligned = (base + 7) & !7;
        match self
            .class_observable_properties(class)
            .iter()
            .position(|(_, f)| f == field)
        {
            Some(k) => aligned + (k as u64) * 8,
            None => aligned,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    #[test]
    fn bool_abi_matches_int32() {
        let reg = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            shadowed_types: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            entry_package: None,
            delegate_aliases: Default::default(),
        };
        assert_eq!(abi_size_align(&reg, "C", "f", "bool"), (4, 4));
        assert_eq!(abi_size_align(&reg, "C", "f", "int"), (4, 4));
        assert_eq!(abi_size_of(&reg, "bool"), 4);
    }

    #[test]
    fn facade_handle_is_ptr_wide() {
        let reg = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            shadowed_types: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            entry_package: None,
            delegate_aliases: Default::default(),
        };
        assert_eq!(
            abi_size_align(&reg, "StringBuilder", "_handle", "int"),
            (8, 8)
        );
        assert_eq!(abi_size_align(&reg, "List_int", "_handle", "int"), (8, 8));
        assert_eq!(abi_size_align(&reg, "Queue_int", "_handle", "int"), (8, 8));
    }

    // RFC 006 A3 S1：惰性判定分类矩阵。
    fn empty_reg() -> TypeRegistry {
        TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            shadowed_types: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            entry_package: None,
            delegate_aliases: Default::default(),
        }
    }
    fn lit_expr() -> Option<Spanned<Expr>> {
        Some(Spanned::new(Expr::IntLit(42), ast::Span::DUMMY))
    }
    fn new_expr() -> Option<Spanned<Expr>> {
        Some(Spanned::new(
            Expr::New {
                ty: ast::Type::named("Brush"),
                args: vec![],
                obj_init: None,
            },
            ast::Span::DUMMY,
        ))
    }
    // 值类型 `new`（struct/基元构造）→ 急切（beforefieldinit 惯用法），即使 readonly。
    fn new_struct_expr() -> Option<Spanned<Expr>> {
        Some(Spanned::new(
            Expr::New {
                ty: ast::Type::named("Vector3"),
                args: vec![],
                obj_init: None,
            },
            ast::Span::DUMMY,
        ))
    }

    #[test]
    fn lazy_classification_matrix() {
        let reg = empty_reg();
        // readonly + 非编译期常量初始化器（class new）→ 惰性
        assert!(is_lazy_static_field(
            &reg,
            true,
            &"Brush".into(),
            &new_expr()
        ));
        // readonly + 编译期常量（字面量）→ 急切
        assert!(!is_lazy_static_field(
            &reg,
            true,
            &"int".into(),
            &lit_expr()
        ));
        // readonly + 无初始化器 → 急切（无可惰性构造）
        assert!(!is_lazy_static_field(&reg, true, &"int".into(), &None));
        // 可变 static（非 readonly）→ 急切，无论初始化器形态
        assert!(!is_lazy_static_field(
            &reg,
            false,
            &"Brush".into(),
            &new_expr()
        ));
        assert!(!is_lazy_static_field(
            &reg,
            false,
            &"int".into(),
            &lit_expr()
        ));
        // readonly + 值类型 new（基元类型 field，is_value_type_name → true）→ 急切
        assert!(!is_lazy_static_field(
            &reg,
            true,
            &"int".into(),
            &new_struct_expr()
        ));
    }

    // RFC 006 V2：struct 静态字段收集。构造含静态字段的 struct 注册表，
    // 断言 `layouts_from_registry` 将 struct 静态字段写入 `static_fields`。
    fn struct_reg_with_static() -> TypeRegistry {
        use crate::oop_types::{FieldInfo, NominalType};
        use ast::Visibility;
        let field = |name: &str,
                     ty: &str,
                     is_static: bool,
                     init: Option<Spanned<Expr>>|
         -> (Ident, FieldInfo) {
            (
                name.into(),
                FieldInfo {
                    name: name.into(),
                    ty: ty.into(),
                    vis: Visibility::Public,
                    is_const: false,
                    is_readonly: is_static,
                    is_init_only: false,
                    get_vis: None,
                    set_vis: None,
                    is_static,
                    init,
                },
            )
        };
        let mut fields: IndexMap<Ident, _> = IndexMap::new();
        fields.insert("X".into(), field("X", "double", false, None).1);
        fields.insert("Z".into(), field("Z", "Vector3", true, new_struct_expr()).1);
        let nom = NominalType {
            name: "Vector3".into(),
            kind: TypeKind::Struct,
            vis: Visibility::Public,
            is_abstract: false,
            is_record: false,
            is_readonly: false,
            fields,
            methods: IndexMap::new(),
            bases: vec![],
            base_types: vec![],
            span: ast::Span::DUMMY,
            variants: vec![],
            generic_params: vec![],
            namespace: vec![],
            const_values: IndexMap::new(),
            constructors: vec![],
            soa: false,
            required_props: Default::default(),
        };
        let mut types = IndexMap::new();
        types.insert("Vector3".into(), nom);
        TypeRegistry {
            types,
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            shadowed_types: Default::default(),
            synth_hosts: Default::default(),
            builtin_static_props: Default::default(),
            entry_package: None,
            delegate_aliases: Default::default(),
        }
    }

    #[test]
    fn struct_static_field_collected() {
        let reg = struct_reg_with_static();
        let layouts = layouts_from_registry(&reg);
        let entry = layouts
            .static_fields
            .iter()
            .find(|s| s.class == "Vector3" && s.field == "Z");
        assert!(entry.is_some(), "struct static field Z must be collected");
        let entry = entry.unwrap();
        assert_eq!(entry.ty, "Vector3");
        // 值类型 `new` → 急切（is_lazy=false），对齐 beforefieldinit 惯用法。
        assert!(!entry.is_lazy, "value-type new static must be eager");
    }

    #[test]
    fn struct_static_field_instance_layout_excluded() {
        // struct 静态字段不占实例布局（StructLayout.fields 不含静态字段）。
        let reg = struct_reg_with_static();
        let layouts = layouts_from_registry(&reg);
        let sl = layouts.structs.get(&Ident::from("Vector3")).unwrap();
        assert!(sl.fields.iter().all(|f| f.name != "Z"));
    }

    fn layouts_with_structs(pairs: &[(&str, &[(&str, &str)])], enums: &[&str]) -> ProgramLayouts {
        let mut layouts = ProgramLayouts::default();
        for (name, fields) in pairs {
            let sl = StructLayout {
                name: Ident::from(*name),
                fields: fields
                    .iter()
                    .enumerate()
                    .map(|(i, (fname, fty))| FieldLayout {
                        name: Ident::from(*fname),
                        ty: Ident::from(*fty),
                        offset: i as u32 * 4,
                    })
                    .collect(),
                ..Default::default()
            };
            layouts.structs.insert(Ident::from(*name), sl);
        }
        for e in enums {
            layouts.enums.insert(Ident::from(*e));
        }
        layouts
    }

    #[test]
    fn is_copy_struct_semantics() {
        // RFC 005 自动 Copy 判定语义锚。回归背景：copy_struct_inner 返回
        // 「含句柄」语义（与 typeck contains_class_handle_inner 同构），曾被
        // is_copy_struct 直接当「可 Copy」使用而整体颠倒——全基元 struct 判
        // false 致端到端 `var b = a` 指针替换 alias 泄漏，含句柄 struct 反被
        // 判 Copy（更危险）。本矩阵锁定两方向。
        let l = layouts_with_structs(
            &[
                ("Point", &[("X", "int"), ("Y", "int")]),
                ("Labeled", &[("Name", "string"), ("N", "int")]),
                ("Tagged", &[("Kind", "Color"), ("N", "int")]),
                ("Nested", &[("P", "Point"), ("N", "int")]),
                ("Handle", &[("B", "Brush"), ("N", "int")]),
                ("NestedHandle", &[("P", "Handle"), ("N", "int")]),
            ],
            &["Color"],
        );
        assert!(l.is_copy_struct("Point"), "全基元字段 → Copy");
        assert!(l.is_copy_struct("Labeled"), "string 归纯值侧");
        assert!(l.is_copy_struct("Tagged"), "enum 字段纯值");
        assert!(l.is_copy_struct("Nested"), "嵌套纯值递归下沉");
        assert!(!l.is_copy_struct("Handle"), "class 字段 → 句柄，不 Copy");
        assert!(!l.is_copy_struct("NestedHandle"), "嵌套含句柄传播");
        assert!(!l.is_copy_struct("Unknown"), "未注册名非 struct");
    }
}
