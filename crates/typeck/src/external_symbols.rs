//! RFC 017 M4-link Phase B：跨 `.ao` 包符号注册（外部符号消费）。
//!
//! 当主程序 `using` 一个预编译 `.ao` 依赖包时，typeck 需要能查询该包的
//! `exports[]` 以解析跨包类型引用，而**不重新解析源码**。
//!
//! 本模块定义 typeck 侧的「外部符号」语义视图——从 `aopkg_format::ExportEntry`
//! 转换而来。`TypeChecker::register_external_symbols` 接收这些条目并将其
//! 注册到 `TypeRegistry`，使 `check_module` 期间对跨包类型的引用能命中
//! 已注册的 `NominalType`。
//!
//! ## 与 `aopkg_format` 的关系
//!
//! `aopkg_format::TypeSig`/`ExportEntry` 是**线格式**（关心二进制序列化、tag 编码、
//! 长度前缀）；本模块的 `ExternalTypeRef`/`ExternalSymbolEntry` 是**语义视图**
//! （关心类型解析、OOP 语义）。两者分层避免 typeck 依赖 `arc` crate
//! （依赖方向约束：`arc` → `typeck`，不可反向）。
//!
//! 转换由 `arc::aopkg_symbol_table` 模块负责。
//!
//! ## Reregister 模式
//!
//! `check_module` 内部用 `TypeRegistry::from_module` 重建 registry，会清空之前
//! 注册的外部符号。为保持与 `native_modules` 一致的行为，本模块采用相同的
//! 「缓存 + 重注册」模式：`register_external_symbols` 缓存条目并立即注册，
//! `reregister_external_symbols` 在 `check_module` 重建 registry 后被调用以
//! 重新注册缓存的外部符号。

use ast::{MethodModifier, Visibility};
use indexmap::IndexMap;

use crate::checker::TypeChecker;
use crate::oop_types::{
    CtorSig, EnumVariantInfo, FieldInfo, NominalType, OopMethodSig, ParamSig, TypeKind,
};
use ast::Ident;

/// 外部符号条目（从 `.ao` exports[] 加载的语义信息）。
///
/// 字段对齐 `aopkg_format::ExportEntry`，但仅保留 typeck 解析所需的语义信息，
/// 不含二进制序列化字段（symbol_id 用于稳定排序但 typeck 不依赖）。
#[derive(Debug, Clone)]
pub struct ExternalSymbolEntry {
    /// 符号全名（如 `Calculator` / `Calculator.Compute` / `Arc.IO.Stream`）。
    pub name: String,
    pub kind: ExternalSymbolKind,
    pub visibility: Visibility,
    pub type_sig: ExternalTypeRef,
}

/// 外部符号种类（对齐 `aopkg_format::ExportKind` 的 typeck 侧视图）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalSymbolKind {
    Class,
    Struct,
    Interface,
    Enum,
    Variant,
    /// C# `static class`（导出为 Module 类型）。
    Module,
    Method,
    StaticMethod,
    Field,
    /// RFC 006 V5：静态字段（`static readonly T X = ...`）。注册为 `is_static`，
    /// 避免跨包消费方把外部静态字段误判为实例字段触发 ABI 布局递归。
    StaticField,
    Constant,
    Property,
    /// RFC 017 M4-link Phase B：自由函数（`ExportKind::Function`）。
    ///
    /// 自由函数不参与 typeck 类型解析（不是 `NominalType`），仅由 `register_external_symbols`
    /// 缓存到 `external_symbols` 列表；codegen 通过 typeck 暴露的 API 取出列表，
    /// 按签名发射 `declare <ret> @<name>(<params>)`（DeclareOnly linkage）。
    /// 定义来自被链接的 lib.o，跨 `.o` 不重复（external linkage 单一定义来源）。
    Function,
    /// RFC 038 M2：构造函数（`ExportKind::Constructor`）。
    ///
    /// 条目 `name = "<Class>.ctor"`，`type_sig` 为 `Method`（receiver = Class、
    /// params = 形参类型、ret = Unit）。typeck 将其注册到 `NominalType.constructors`
    /// 供 `new T(...)` 约束校验；codegen 读取该条目发射 arity-mangled ctor
    /// `declare`（`__ctor_<Class>` / `__ctor_<Class>_<arity>`），定义来自被链接的 lib.o。
    Constructor,
}

/// 外部类型引用（对齐 `aopkg_format::TypeSig` 的 typeck 侧视图）。
///
/// 仅包含 typeck 解析跨包类型所需的变体——线格式中的 `Func`/`Closure`/
/// `TaskHandle`/`Span`/`Expression`/`Nullable`/`Array`/`Tuple`/`Property` 等
/// 复合变体由 `Named` 兜底（typeck 通过类型名查找已注册的 `NominalType`）。
#[derive(Debug, Clone)]
pub enum ExternalTypeRef {
    Int,
    Long,
    Float,
    Double,
    Bool,
    String,
    Unit,
    Null,
    Object,
    UInt,
    ULong,
    UShort,
    SByte,
    /// 命名类型 + 可选泛型实参。
    Named {
        fqn: String,
        generic_args: Vec<ExternalTypeRef>,
    },
    /// 泛型参数引用（`T` 是第 N 个泛型参数）。
    GenericParam {
        param_index: u8,
    },
    /// `List<T>` 嵌套类型。
    List {
        element_type: Box<ExternalTypeRef>,
    },
    /// 方法签名（用于方法导出条目）。
    Method {
        receiver: Box<ExternalTypeRef>,
        params: Vec<ExternalTypeRef>,
        ret: Box<ExternalTypeRef>,
        is_virtual: bool,
    },
    /// RFC 017 M4-link Phase B：自由函数签名（`ExportKind::Function` 条目）。
    ///
    /// typeck 不消费此变体（自由函数不参与类型解析）；codegen 取出
    /// `external_symbols` 列表后按此签名发射 `declare <ret> @<name>(<params>)`。
    Func {
        params: Vec<ExternalTypeRef>,
        ret: Box<ExternalTypeRef>,
        /// 0 = 无捕获，1 = 有捕获（用于闭包）。
        captures: bool,
    },
    /// variant 类型 + case 列表。
    Variant {
        fqn: String,
        cases: Vec<ExternalVariantCase>,
    },
}

/// variant case 的外部视图（对齐 `aopkg_format::VariantCase`）。
#[derive(Debug, Clone)]
pub struct ExternalVariantCase {
    pub case_name: String,
    pub payload_type: ExternalTypeRef,
    pub discriminant: u32,
}

impl TypeChecker {
    /// 注册外部符号（从 `.ao` 包 `exports[]` 加载）。
    ///
    /// 必须在 `check_module` **之前**调用：注册的符号会立即可见于 registry，
    /// 同时被缓存以供 `reregister_external_symbols` 在 `check_module` 重建
    /// registry 后重新注册（`from_module` 会清空 registry）。
    ///
    /// 类型条目（Class/Struct/Interface/Enum/Variant/Module）创建新的
    /// `NominalType` 条目；成员条目（Method/StaticMethod/Field/Constant/
    /// Property）追加到对应类型的 `NominalType`。
    ///
    /// 重名冲突处理：若 `registry.types` 已含同名 `NominalType`（如本地源码
    /// 已定义同名类型），跳过注册并保持本地定义优先——本地源码永远是权威。
    pub fn register_external_symbols(&mut self, entries: &[ExternalSymbolEntry]) {
        self.external_symbols = entries.to_vec();
        self.register_external_symbols_inner(&self.external_symbols.clone());
    }

    /// 重注册缓存的外部符号。
    ///
    /// `check_module` 用 `TypeRegistry::from_module` 重建 registry 后调用此方法，
    /// 将之前 `register_external_symbols` 缓存的符号重新注册。不更新缓存。
    pub(crate) fn reregister_external_symbols(&mut self) {
        let entries = self.external_symbols.clone();
        self.register_external_symbols_inner(&entries);
    }

    fn register_external_symbols_inner(&mut self, entries: &[ExternalSymbolEntry]) {
        // 两遍扫描：第一遍注册所有「类型条目」到 registry.types；
        // 第二遍注册「成员条目」到对应类型的 methods/fields。
        // 第一遍完成后才能保证第二遍的成员条目能找到所属类型。
        //
        // 注册键使用**短名**（FQN 的最后一段），与 `TypeRegistry::from_module`
        // 的行为对齐——后者用 `def_ast.name`（短名）作 key，并存储 namespace
        // 在 `NominalType.namespace`。typeck `check_module` 末尾将所有
        // `registry.types` 条目注册到全局 scope（按短名），使 `using Lib;`
        // 后 `Calculator` 可被命中。
        for entry in entries {
            if !is_type_kind(entry.kind) {
                continue;
            }
            let (_, short_name) = split_namespace(&entry.name);
            // 本地源码优先：若已存在同名类型（来自 HIR 注册），跳过外部注册。
            if self.registry.types.contains_key(short_name.as_str()) {
                continue;
            }
            let nominal = match entry_to_nominal_type(entry) {
                Some(n) => n,
                None => continue,
            };
            self.registry.types.insert(short_name.into(), nominal);
        }

        for entry in entries {
            if is_type_kind(entry.kind) {
                continue;
            }
            // 成员条目：解析所属类型 FQN 与成员名（形如 `Lib.Calc.Compute`
            // → type_fqn = "Lib.Calc", member = "Compute"）。
            let (type_fqn, member_name) = match entry.name.rsplit_once('.') {
                Some((ty, mem)) => (ty, mem),
                None => continue, // 成员条目必须有 `Type.Member` 形式
            };
            // 仅当所属类型是外部注册的（即来自本模块第一遍或前次调用）才追加成员；
            // 本地类型已有完整 AST 注册的 methods/fields，无需外部补充。
            let is_external_type = entries
                .iter()
                .any(|e| is_type_kind(e.kind) && e.name == type_fqn);
            if !is_external_type {
                continue;
            }
            // 用短名查 registry（与第一遍注册时的键一致）。
            let (_, type_short_name) = split_namespace(type_fqn);
            let nominal = match self.registry.types.get_mut::<str>(&type_short_name) {
                Some(n) => n,
                None => continue,
            };
            register_member(nominal, type_fqn, member_name, entry);
        }
    }
}

/// 判断是否为「类型条目」（创建 `NominalType` 而非追加成员）。
fn is_type_kind(kind: ExternalSymbolKind) -> bool {
    matches!(
        kind,
        ExternalSymbolKind::Class
            | ExternalSymbolKind::Struct
            | ExternalSymbolKind::Interface
            | ExternalSymbolKind::Enum
            | ExternalSymbolKind::Variant
            | ExternalSymbolKind::Module
    )
}

/// 将类型条目转为 `NominalType`（成员字段留空，由第二遍或后续填充）。
fn entry_to_nominal_type(entry: &ExternalSymbolEntry) -> Option<NominalType> {
    let kind = match entry.kind {
        ExternalSymbolKind::Class => TypeKind::Class,
        ExternalSymbolKind::Struct => TypeKind::Struct,
        ExternalSymbolKind::Interface => TypeKind::Interface,
        ExternalSymbolKind::Enum => TypeKind::Enum,
        ExternalSymbolKind::Variant => TypeKind::Variant,
        ExternalSymbolKind::Module => TypeKind::StaticClass,
        _ => return None,
    };
    // 从命名空间路径解析：`A.B.C` → namespace=[A,B], name=C
    let (namespace, name) = split_namespace(&entry.name);
    // variant/enum case 从 type_sig 提取（枚举成员也经 cases 透传，含判别值）。
    let variants = match &entry.type_sig {
        ExternalTypeRef::Variant { cases, .. } => cases
            .iter()
            .map(|c| EnumVariantInfo {
                name: c.case_name.clone().into(),
                fields: Vec::new(),
                discriminant: c.discriminant,
                payload: match &c.payload_type {
                    ExternalTypeRef::Unit => None,
                    other => Some(type_ref_to_name(other).into()),
                },
            })
            .collect(),
        _ => Vec::new(),
    };
    // 泛型参数：从 Named.generic_args 中的 GenericParam 提取 arity
    let generic_params = match &entry.type_sig {
        ExternalTypeRef::Named { generic_args, .. } => generic_params_from_args(generic_args),
        _ => Vec::new(),
    };
    Some(NominalType {
        name: name.into(),
        kind,
        vis: entry.visibility,
        is_abstract: false,
        is_record: false,
        is_readonly: false,
        fields: IndexMap::new(),
        methods: IndexMap::new(),
        bases: Vec::new(),
        base_types: Vec::new(),
        span: ast::Span::DUMMY,
        variants,
        generic_params,
        namespace,
        const_values: IndexMap::new(),
        constructors: Vec::new(),
        soa: false,
        required_props: Default::default(),
    })
}

/// 从泛型实参列表提取泛型参数名占位（外部包仅知 arity，不知参数名；
/// typeck 解析时按位置匹配，参数名仅用于诊断）。
fn generic_params_from_args(args: &[ExternalTypeRef]) -> Vec<Ident> {
    args.iter()
        .enumerate()
        .filter_map(|(i, a)| match a {
            ExternalTypeRef::GenericParam { .. } => Some(format!("T{i}").into()),
            _ => None,
        })
        .collect()
}

/// 将 `A.B.C` 拆分为 namespace=`[A, B]` + name=`C`。
fn split_namespace(fqn: &str) -> (Vec<Ident>, String) {
    let parts: Vec<&str> = fqn.rsplitn(2, '.').collect();
    match parts.as_slice() {
        [name] => (Vec::new(), name.to_string()),
        [name, ns_prefix] => (
            ns_prefix.split('.').map(|s| s.into()).collect(),
            name.to_string(),
        ),
        _ => (Vec::new(), fqn.to_string()),
    }
}

/// 将 `ExternalTypeRef` 转为类型名（用于 variant payload / field 类型）。
fn type_ref_to_name(t: &ExternalTypeRef) -> String {
    match t {
        ExternalTypeRef::Int => "int".into(),
        ExternalTypeRef::Long => "long".into(),
        ExternalTypeRef::Float => "float".into(),
        ExternalTypeRef::Double => "double".into(),
        ExternalTypeRef::Bool => "bool".into(),
        ExternalTypeRef::String => "string".into(),
        ExternalTypeRef::Unit => "void".into(),
        ExternalTypeRef::Null => "null".into(),
        ExternalTypeRef::Object => "object".into(),
        ExternalTypeRef::UInt => "uint".into(),
        ExternalTypeRef::ULong => "ulong".into(),
        ExternalTypeRef::UShort => "ushort".into(),
        ExternalTypeRef::SByte => "sbyte".into(),
        ExternalTypeRef::Named { fqn, .. } => fqn.clone(),
        ExternalTypeRef::GenericParam { param_index } => format!("T{param_index}"),
        ExternalTypeRef::List { element_type } => {
            format!("List_{}", type_ref_to_name(element_type))
        }
        ExternalTypeRef::Method { .. } => "method".into(),
        ExternalTypeRef::Func { .. } => "func".into(),
        ExternalTypeRef::Variant { fqn, .. } => fqn.clone(),
    }
}

/// 将成员条目（方法/字段/常量）注册到 `NominalType` 的 methods/fields。
fn register_member(
    nominal: &mut NominalType,
    type_name: &str,
    member_name: &str,
    entry: &ExternalSymbolEntry,
) {
    match entry.kind {
        ExternalSymbolKind::Method | ExternalSymbolKind::StaticMethod => {
            let (params, ret, is_virtual) = match &entry.type_sig {
                ExternalTypeRef::Method {
                    params,
                    ret,
                    is_virtual,
                    ..
                } => (params, ret, *is_virtual),
                _ => return,
            };
            let modifier = if entry.kind == ExternalSymbolKind::StaticMethod {
                MethodModifier::Static
            } else if is_virtual {
                MethodModifier::Virtual
            } else {
                MethodModifier::None
            };
            let sig = OopMethodSig {
                name: member_name.into(),
                vis: entry.visibility,
                params: params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| ParamSig {
                        name: format!("arg{i}").into(),
                        ty: type_ref_to_name(p).into(),
                        is_ref: false,
                        is_out: false,
                        is_in: false,
                        is_params: false,
                        default: None,
                    })
                    .collect(),
                ret: type_ref_to_name(ret).into(),
                modifier,
                is_async: false,
                generics: Vec::new(),
                is_static_abstract: false,
            };
            nominal
                .methods
                .entry(member_name.into())
                .or_default()
                .push(sig);
        }
        ExternalSymbolKind::Field
        | ExternalSymbolKind::StaticField
        | ExternalSymbolKind::Constant
        | ExternalSymbolKind::Property => {
            let field_type = type_ref_to_name(&entry.type_sig);
            nominal.fields.insert(
                member_name.into(),
                FieldInfo {
                    name: member_name.into(),
                    ty: field_type.into(),
                    vis: entry.visibility,
                    is_const: entry.kind == ExternalSymbolKind::Constant,
                    is_readonly: false,
                    is_init_only: false,
                    get_vis: None,
                    set_vis: None,
                    // RFC 006 V5：静态字段须保留 is_static，避免跨包布局递归
                    // （如 `Guid.Empty` 被当作实例字段 → abi_size_align 无限递归）。
                    is_static: entry.kind == ExternalSymbolKind::StaticField,
                    init: None,
                },
            );
            // type_name 仅用于诊断占位，避免「unused variable」警告。
            let _ = type_name;
        }
        // RFC 038 M2：外部构造函数注册到 `NominalType.constructors`，
        // 供消费方 `new ExternalType(...)` 的约束校验与 codegen ctor 调用解析。
        ExternalSymbolKind::Constructor => {
            let params = match &entry.type_sig {
                ExternalTypeRef::Method { params, .. } => params,
                _ => return,
            };
            nominal.constructors.push(CtorSig {
                vis: entry.visibility,
                param_types: params
                    .iter()
                    .map(type_ref_to_name)
                    .map(Into::into)
                    .collect(),
                params: params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| ParamSig {
                        name: format!("arg{i}").into(),
                        ty: type_ref_to_name(p).into(),
                        is_ref: false,
                        is_out: false,
                        is_in: false,
                        is_params: false,
                        default: None,
                    })
                    .collect(),
                sets_required_members: Default::default(),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypeRegistry;

    fn make_class_entry(name: &str) -> ExternalSymbolEntry {
        ExternalSymbolEntry {
            name: name.to_string(),
            kind: ExternalSymbolKind::Class,
            visibility: Visibility::Public,
            type_sig: ExternalTypeRef::Named {
                fqn: name.to_string(),
                generic_args: Vec::new(),
            },
        }
    }

    fn make_method_entry(class: &str, method: &str) -> ExternalSymbolEntry {
        ExternalSymbolEntry {
            name: format!("{class}.{method}"),
            kind: ExternalSymbolKind::Method,
            visibility: Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: class.to_string(),
                    generic_args: Vec::new(),
                }),
                params: vec![ExternalTypeRef::Int],
                ret: Box::new(ExternalTypeRef::Int),
                is_virtual: false,
            },
        }
    }

    #[test]
    fn register_external_class_creates_nominal_type() {
        let mut tc = crate::TypeChecker::new();
        assert!(tc.registry().types.is_empty());
        tc.register_external_symbols(&[make_class_entry("Calculator")]);
        assert!(tc.registry().types.contains_key("Calculator"));
        let nom = &tc.registry().types["Calculator"];
        assert_eq!(nom.kind, TypeKind::Class);
        assert_eq!(nom.name.as_str(), "Calculator");
    }

    #[test]
    fn register_external_method_appends_to_class() {
        let mut tc = crate::TypeChecker::new();
        tc.register_external_symbols(&[
            make_class_entry("Calculator"),
            make_method_entry("Calculator", "Compute"),
        ]);
        let nom = &tc.registry().types["Calculator"];
        assert!(nom.methods.contains_key("Compute"));
        let sigs = &nom.methods["Compute"];
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].params.len(), 1);
        assert_eq!(sigs[0].params[0].ty.as_str(), "int");
        assert_eq!(sigs[0].ret.as_str(), "int");
        assert_eq!(sigs[0].modifier, MethodModifier::None);
    }

    #[test]
    fn register_external_static_method() {
        let mut tc = crate::TypeChecker::new();
        let mut entry = make_method_entry("Foo", "Bar");
        entry.kind = ExternalSymbolKind::StaticMethod;
        tc.register_external_symbols(&[make_class_entry("Foo"), entry]);
        let nom = &tc.registry().types["Foo"];
        let sig = &nom.methods["Bar"][0];
        assert_eq!(sig.modifier, MethodModifier::Static);
    }

    #[test]
    fn register_external_const_becomes_const_field() {
        let mut tc = crate::TypeChecker::new();
        let entry = ExternalSymbolEntry {
            name: "Foo.Pi".to_string(),
            kind: ExternalSymbolKind::Constant,
            visibility: Visibility::Public,
            type_sig: ExternalTypeRef::Double,
        };
        tc.register_external_symbols(&[make_class_entry("Foo"), entry]);
        let nom = &tc.registry().types["Foo"];
        let field = &nom.fields["Pi"];
        assert!(field.is_const);
        assert_eq!(field.ty.as_str(), "double");
    }

    #[test]
    fn register_external_variant_populates_cases() {
        let mut tc = crate::TypeChecker::new();
        let entry = ExternalSymbolEntry {
            name: "Result".to_string(),
            kind: ExternalSymbolKind::Variant,
            visibility: Visibility::Public,
            type_sig: ExternalTypeRef::Variant {
                fqn: "Result".to_string(),
                cases: vec![
                    ExternalVariantCase {
                        case_name: "Ok".to_string(),
                        payload_type: ExternalTypeRef::Int,
                        discriminant: 0,
                    },
                    ExternalVariantCase {
                        case_name: "Err".to_string(),
                        payload_type: ExternalTypeRef::Unit,
                        discriminant: 1,
                    },
                ],
            },
        };
        tc.register_external_symbols(&[entry]);
        let nom = &tc.registry().types["Result"];
        assert_eq!(nom.kind, TypeKind::Variant);
        assert_eq!(nom.variants.len(), 2);
        assert_eq!(nom.variants[0].name.as_str(), "Ok");
        assert_eq!(
            nom.variants[0].payload.as_ref().map(|s| s.as_str()),
            Some("int")
        );
        assert_eq!(nom.variants[1].payload, None);
    }

    #[test]
    fn local_type_takes_precedence_over_external() {
        let mut tc = crate::TypeChecker::new();
        // 预先注册一个本地 Calculator
        tc.registry.types.insert(
            "Calculator".into(),
            NominalType {
                name: "Calculator".into(),
                kind: TypeKind::Class,
                vis: Visibility::Public,
                is_abstract: false,
                is_record: false,
                is_readonly: false,
                fields: IndexMap::new(),
                methods: IndexMap::new(),
                bases: vec!["LocalBase".into()],
                base_types: vec![],
                span: ast::Span::DUMMY,
                variants: vec![],
                generic_params: vec![],
                namespace: vec![],
                const_values: IndexMap::new(),
                constructors: vec![],
                soa: false,
                required_props: Default::default(),
            },
        );
        // 再注册外部 Calculator — 应被跳过
        tc.register_external_symbols(&[make_class_entry("Calculator")]);
        let nom = &tc.registry().types["Calculator"];
        // bases 仍是本地版本（包含 LocalBase），证明外部注册被跳过
        assert_eq!(nom.bases[0].as_str(), "LocalBase");
    }

    #[test]
    fn reregister_preserves_external_symbols_after_check_module() {
        let mut tc = crate::TypeChecker::new();
        tc.register_external_symbols(&[make_class_entry("ExternalLib")]);
        assert!(tc.registry().types.contains_key("ExternalLib"));

        // 模拟 check_module 的 registry 重建
        tc.registry = TypeRegistry {
            types: IndexMap::new(),
            extensions: IndexMap::new(),
            init_only_props: Default::default(),
            declared_properties: Default::default(),
            file_packages: Default::default(),
            internals_visible_to: Default::default(),
            shadowed_types: Default::default(),
            synth_hosts: Default::default(),
            entry_package: Default::default(),
            builtin_static_props: Default::default(),
            delegate_aliases: Default::default(),
        };
        assert!(!tc.registry().types.contains_key("ExternalLib"));

        // reregister 应恢复外部符号
        tc.reregister_external_symbols();
        assert!(tc.registry().types.contains_key("ExternalLib"));
    }

    #[test]
    fn namespace_split_extracts_path() {
        let (ns, name) = split_namespace("Arc.IO.Stream");
        assert_eq!(ns.len(), 2);
        assert_eq!(ns[0].as_str(), "Arc");
        assert_eq!(ns[1].as_str(), "IO");
        assert_eq!(name, "Stream");
    }

    #[test]
    fn namespace_split_no_namespace() {
        let (ns, name) = split_namespace("Calculator");
        assert!(ns.is_empty());
        assert_eq!(name, "Calculator");
    }

    /// RFC 017 M4-link Phase B：验证 FQN 输入时类型按**短名**注册到 registry，
    /// 与 `TypeRegistry::from_module` 行为对齐（`using Lib;` 后 `Calculator` 可命中）。
    #[test]
    fn fqn_entry_registered_under_short_name() {
        let mut tc = crate::TypeChecker::new();
        // FQN 输入：包名 "Lib" + 类型 "Calculator"
        let class_entry = ExternalSymbolEntry {
            name: "Lib.Calculator".to_string(),
            kind: ExternalSymbolKind::Class,
            visibility: Visibility::Public,
            type_sig: ExternalTypeRef::Named {
                fqn: "Lib.Calculator".to_string(),
                generic_args: Vec::new(),
            },
        };
        let method_entry = ExternalSymbolEntry {
            name: "Lib.Calculator.Compute".to_string(),
            kind: ExternalSymbolKind::Method,
            visibility: Visibility::Public,
            type_sig: ExternalTypeRef::Method {
                receiver: Box::new(ExternalTypeRef::Named {
                    fqn: "Lib.Calculator".to_string(),
                    generic_args: Vec::new(),
                }),
                params: vec![ExternalTypeRef::Int],
                ret: Box::new(ExternalTypeRef::Int),
                is_virtual: false,
            },
        };
        tc.register_external_symbols(&[class_entry, method_entry]);

        // 类型按短名 "Calculator" 注册（不是 FQN "Lib.Calculator"）
        assert!(tc.registry().types.contains_key("Calculator"));
        assert!(!tc.registry().types.contains_key("Lib.Calculator"));

        // 成员正确追加：通过短名查找类型
        let nom = &tc.registry().types["Calculator"];
        assert_eq!(nom.name.as_str(), "Calculator");
        assert_eq!(nom.namespace.len(), 1);
        assert_eq!(nom.namespace[0].as_str(), "Lib");
        assert!(nom.methods.contains_key("Compute"));
    }
}
