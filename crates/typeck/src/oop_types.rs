use ast::*;
use indexmap::IndexMap;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Struct,
    Class,
    /// C# `static class` — no instances; hosts extension methods.
    StaticClass,
    Interface,
    Enum,
    /// RFC 004 M1：variant 标签联合类型（tagged union）。
    /// 栈上值类型（`tag (u8) + payload union`），switch 强制编译期穷尽性检查。
    Variant,
}

/// Method signature for OOP checking (erased from AST).
#[derive(Clone, Debug, PartialEq)]
pub struct OopMethodSig {
    pub name: Ident,
    pub vis: Visibility,
    pub params: Vec<ParamSig>,
    pub ret: Ident,
    pub modifier: MethodModifier,
    pub is_async: bool,
    /// 方法自身的泛型参数名列表（如 `static T Identity<T>(...)` → `["T"]`）。
    /// 决策 #7（RFC 010）：泛型扩展方法支持。单态化后为空。
    pub generics: Vec<Ident>,
    /// RFC 004 M1：`static abstract` 接口成员标记。
    ///
    /// true 表示此方法来自接口的 `static abstract` 声明——实现类无需提供
    /// 实例方法（typeck 在 `check_interface_impl` 跳过此类方法校验）。
    /// 调用走编译期单态化分派，不走实例 vtable。
    pub is_static_abstract: bool,
}

/// Parameter signature within a method signature.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSig {
    pub name: Ident,
    pub ty: Ident,
    pub is_ref: bool,
    pub is_out: bool,
    /// RFC 009 P1-F #8：C# `in` 参数修饰符（`readonly ref` 语义）。
    /// 与 `is_ref`/`is_out` 互斥；MIR/codegen 按 `ref` 处理（addr-of），
    /// readonly 约束在 typeck `check_stmt` 层强制（不可赋值、不可传给 `out`/`ref`）。
    pub is_in: bool,
    /// RFC 005：`params Span`/`ReadOnlySpan` 可变实参（仅末位）。
    pub is_params: bool,
    /// RFC 007：可选参数默认值（已折叠为常量）。
    pub default: Option<ConstValue>,
}

#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub name: Ident,
    pub ty: Ident,
    pub vis: Visibility,
    pub is_const: bool,
    pub is_readonly: bool,
    /// RFC 006 M1：init-only 自动属性 backing field；仅 ctor / 对象初始化器可写。
    pub is_init_only: bool,
    /// RFC 006 A1：auto-property 的 per-accessor 可见性。None = 继承字段
    /// （属性自身）可见性。`get_vis` 作用于读；`set_vis` 作用于写（set/init）。
    /// 普通字段恒为 None。
    pub get_vis: Option<Visibility>,
    pub set_vis: Option<Visibility>,
    /// RFC 006：`static` 修饰符标记。true 表示类级别静态字段。
    pub is_static: bool,
    /// RFC 006 M4：字段初始化器（`static int _count = 0;` / `int _max = 100;` 的 `= …`）。
    /// 静态字段（`is_static == true`）的 init 由 codegen 在 `__sinit_<Class>`
    /// 函数体内 emit；实例字段的 init 由 typeck 注入 `__ctor::<Class>` body
    ///（base 调用之后、用户语句之前，C# 语义；含跨文件 partial 合并后的字段）。
    /// const 字段的 init 由 typeck 在 `const_values` 中折叠为常量，此处为 None。
    pub init: Option<Spanned<Expr>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstValue {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    /// RFC 007：`null` 默认值。
    Null,
}

/// Enum variant metadata (discriminant index + optional payload fields).
///
/// RFC 004 M1：复用于 variant case 信息——
/// - `Enum` kind：`fields` 为 enum variant 字段（一般为空），`payload` 始终 None
/// - `Variant` kind：`fields` 为空，`payload` 为 case 的单一 payload 类型（无 payload case 为 None）
#[derive(Clone, Debug)]
pub struct EnumVariantInfo {
    pub name: Ident,
    pub fields: Vec<(Ident, Ident)>,
    pub discriminant: u32,
    /// RFC 004 M1：variant case 的单一 payload 类型（None = 无 payload case 如 `Null`）。
    /// 仅 `TypeKind::Variant` 时使用；`TypeKind::Enum` 始终为 None。
    pub payload: Option<Ident>,
}

#[derive(Clone, Debug)]
pub struct NominalType {
    pub name: Ident,
    pub kind: TypeKind,
    /// RFC 025：类型级可见性（`public class` / `internal class` 等）。
    ///
    /// - `Public`：跨包可见
    /// - `Internal`：仅同包可见（与成员 `internal` 共用 `can_access` 包规则）
    /// - `Private`/`Protected`：解析器无修饰符时默认为 `Private`；类型级暂不强制，
    ///   避免无修饰符类被误杀（顶层默认 `internal` 语义另轨对齐）
    pub vis: Visibility,
    /// C# `abstract class` — 不可直接实例化。
    /// RFC 012 M4-1：GenerateToAttribute<T> 标记为 abstract，强制用户派生。
    pub is_abstract: bool,
    /// RFC 006：`record` / `record struct`（值相等 / `with` 仅对此为 true）。
    pub is_record: bool,
    /// C# `readonly struct` — 所有字段必须为 readonly，不可包含可变方法。
    pub is_readonly: bool,
    pub fields: IndexMap<Ident, FieldInfo>,
    /// Method name → overload list (distinct parameter signatures).
    pub methods: IndexMap<Ident, Vec<OopMethodSig>>,
    /// Class: one base class + interfaces. Interface: extended interfaces.
    pub bases: Vec<Ident>,
    /// Original AST base types (preserves generic args, e.g., `IComparable<int>`).
    /// Used by TypeChecker for generic interface impl checking; `bases` stores
    /// the simple name only (for non-generic fast path in `validate_all`).
    pub base_types: Vec<Type>,
    pub span: Span,
    /// Populated when `kind == Enum`.
    pub variants: Vec<EnumVariantInfo>,
    /// Declared type parameters (`class Box<T>` → `["T"]`). Empty when monomorphized.
    pub generic_params: Vec<Ident>,
    /// Declaring namespace path (`namespace A.B;` → `["A", "B"]`). Root when empty.
    pub namespace: Vec<Ident>,
    /// const field values (filled by typeck during `check_class_inner`).
    pub const_values: IndexMap<Ident, ConstValue>,
    /// 构造函数签名表（由 `check_class_inner` 填充，用于 `new()` 约束校验）。
    pub constructors: Vec<CtorSig>,
    /// RFC 009 M4：`[SoA]` attribute 标记——struct 数组采用 SoA 布局。
    /// 为 true 时 codegen 应发射 `rt_soa_array` ABI 调用而非普通 AoS 数组。
    pub soa: bool,
    /// RFC 006 M3：`required` 属性名集合；`new` 时须由对象初始化器或选中 ctor 体赋值。
    pub required_props: indexmap::IndexSet<Ident>,
}

/// 构造函数签名（用于 `new()` 约束校验与 RFC 007 M2 可选/命名绑定）。
///
/// `param_types` 为参数类型名列表（与 `ParamSig.ty` 同源），空列表表示无参构造。
/// `params` 含形参名与默认值，供 `new T(...)` 脱糖；与 `param_types` 等长且类型一致。
#[derive(Clone, Debug, PartialEq)]
pub struct CtorSig {
    pub vis: Visibility,
    pub param_types: Vec<Ident>,
    /// RFC 007 M2：形参槽（名称 + 默认值）。
    pub params: Vec<ParamSig>,
    /// RFC 006 M4：ctor 体赋值的成员名（SetsRequiredMembers 等价；免除对象初始化器 required）。
    pub sets_required_members: indexmap::IndexSet<Ident>,
}

#[derive(Debug, Error, PartialEq)]
pub enum OopError {
    #[error("undefined type `{0}`")]
    UndefinedType(String),
    #[error("class `{class}` does not implement interface `{iface}`: missing method `{method}`")]
    MissingInterfaceMethod {
        class: String,
        iface: String,
        method: String,
    },
    #[error(
        "class `{class}` does not implement interface `{iface}`: missing property `{property}`"
    )]
    MissingInterfaceProperty {
        class: String,
        iface: String,
        property: String,
    },
    #[error("class `{class}` method `{method}` incompatible with `{base}`: {detail}")]
    LspViolation {
        class: String,
        method: String,
        base: String,
        detail: String,
    },
    #[error("class `{0}` may only inherit from one base class")]
    MultipleInheritance(String),
    #[error("unknown field `{field}` on type `{ty}`")]
    UnknownField { ty: String, field: String },
    #[error("unknown method `{method}` on type `{ty}`")]
    UnknownMethod { ty: String, method: String },
    #[error("no overload of `{method}` on `{ty}` matches the argument types")]
    NoMatchingOverload { ty: String, method: String },
    /// RFC 038 M2-G3b：跨包泛型方法「模板缺失」诊断（报错 > 静默推断）。
    ///
    /// 库中非 static 类的泛型方法（M2-G1b 收集边界）经 `.aopkg` 外部符号注册为
    /// `generics` 空、形参仍为 `T0` 占位符的退化签名——消费端只拿到签名拿不到
    /// 方法体模板，无法单态化。此错误显式指出模板缺口，而非误导性的「无匹配重载」。
    #[error(
        "generic method `{method}` on `{ty}` has no body template in the referenced package \
         (only its signature was exported; generic methods on non-static classes are not yet \
         collected — M2-G1b). Move it to a `public static class` so the body can be injected"
    )]
    MissingGenericTemplate { ty: String, method: String },
    #[error("call to `{method}` on `{ty}` is ambiguous for the argument types")]
    AmbiguousOverload { ty: String, method: String },
    #[error("cannot assign `{found}` to `{expected}`: not a subtype")]
    NotSubtype { expected: String, found: String },
    #[error("`override` method `{method}` in `{class}` has no matching virtual/abstract method in the base chain (CD-10/D1: override 必须按签名对齐基类虚方法)")]
    NoMatchingOverrideBase { class: String, method: String },
    #[error("member `{member}` on `{ty}` is not accessible from this context")]
    InaccessibleMember { ty: String, member: String },
    /// RFC 025：`internal class` 等类型级可见性跨包拒绝。
    #[error("type `{ty}` is not accessible from this context")]
    InaccessibleType { ty: String },
    #[error("abstract method `{method}` in non-abstract class `{class}`")]
    AbstractInConcreteClass { class: String, method: String },
    #[error("ambiguous extension method call: `{method}` on `{receiver}` matches multiple candidates: {candidates}")]
    AmbiguousExtensionCall {
        method: String,
        receiver: String,
        candidates: String,
    },
}

/// Extension method registered from a `static class` (`this T receiver` first parameter).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionMethod {
    pub container: Ident,
    pub method: OopMethodSig,
    /// Namespace of the declaring `static class`.
    pub namespace: Vec<Ident>,
    /// 方法声明的泛型参数名列表（`static T Id<T>(this T x)` → `["T"]`）。
    /// 决策 #7（RFC 010）：用于接收者类型推断（unify_receiver）。
    pub generic_params: Vec<Ident>,
    /// 泛型扩展方法体模板在 `extension_fn_templates` 中的键
    /// （`method_link_name` 产物 + `_<arity>` 后缀，如
    /// `ServiceCollectionExtensions::AddTransient_2`）。
    /// arity 后缀仅用于消解 HashMap 键冲突（同 `method_link_name` 不同泛型元数，
    /// 如 `AddTransient<T,TImpl>` vs `AddTransient<T>`），**不**进入符号 mangle。
    pub template_key: Ident,
    /// 符号 mangle 基底（`method_link_name` 产物，无 arity 后缀，如
    /// `ServiceCollectionExtensions::AddTransient`、`IdExt::Id`）。
    /// `call_name` 与单态化方法体符号均以此 mangle，保持逐字节一致；
    /// 不同泛型元数已由 mangle 后缀的 type_args 区分（`AddTransient_A_B`
    /// vs `AddTransient_A`），无需 arity 后缀参与符号命名。
    pub mangle_base: Ident,
}

/// 扩展方法解析结果（决策 #7/#8，RFC 010）。
///
/// `resolve_extension` 返回的完整解析信息，供 typeck 触发单态化、MIR 生成调用名。
#[derive(Clone, Debug)]
pub struct ExtensionResolution {
    /// 声明扩展方法的静态类（如 `FooExt`）。
    pub container: Ident,
    /// MIR/codegen 调用目标全名。
    /// 非泛型：`FooExt::Id`；泛型（已实例化）：`FooExt::Id_int`。
    pub call_name: String,
    /// 实例化后的方法签名（泛型参数已擦除；**不含 `this` 接收者形参**——
    /// 注册时 `ext_sig.params.remove(0)` 已剥离）。
    pub sig: OopMethodSig,
    /// 扩展方法接收者（`this`）的目标类型：注册时的扩展键（如
    /// `AddTransient<T>(this IServiceCollection)` 的 `IServiceCollection`）。
    /// 调用方据此判定接收者是否须包装为接口胖指针（`MirOperand::Iface`）。
    pub this_ty: Ident,
    /// 泛型扩展方法推断出的接收者类型参数（非泛型为 `None`）。
    /// typeck 据此触发 `instantiate_generic_extension_fn` 单态化方法体。
    pub inferred_arg: Option<Ident>,
    /// 显式 type_args 的泛型扩展方法实参（如 `AddTransient<Greeter, Greeter>` →
    /// `["Greeter", "Greeter"]`）。调用方据此触发
    /// `instantiate_generic_extension_fn_by_key` 单态化方法体；
    /// 非显式 type_args 时为 `vec![]`。
    pub type_args: Vec<Ident>,
    /// 泛型扩展方法体模板键（`ExtensionMethod.template_key` 透传）。
    /// 仅用于 `extension_fn_templates` 查找（含 arity 后缀消解键冲突），
    /// **不**用于符号 mangle。
    pub template_key: Ident,
    /// 符号 mangle 基底（`ExtensionMethod.mangle_base` 透传）。
    /// `call_name` 与单态化方法体符号均以此 mangle，保证逐字节一致。
    pub mangle_base: Ident,
}

/// C# extension lookup scope: imported namespaces plus enclosing declaration namespace.
#[derive(Clone, Debug, Default)]
pub struct ExtensionScope {
    /// Full namespace paths imported via `using` (resolved against registered static classes).
    pub imported: Vec<Vec<Ident>>,
    /// Namespace of the calling function or method body.
    pub enclosing: Vec<Ident>,
}

impl ExtensionScope {
    /// Whether an extension declared in `container_ns` is visible from this scope.
    pub fn is_visible(&self, container_ns: &[Ident]) -> bool {
        if container_ns == self.enclosing.as_slice() {
            return true;
        }
        self.imported
            .iter()
            .any(|imp| namespace_matches_import(imp, container_ns))
    }
}

/// `using N;` matches the container namespace exactly, as a prefix, or by final segment.
pub fn namespace_matches_import(imported: &[Ident], container: &[Ident]) -> bool {
    if imported == container {
        return true;
    }
    if container.len() >= imported.len() && container[..imported.len()] == imported[..] {
        return true;
    }
    if imported.len() == 1 {
        return container.last() == imported.first();
    }
    false
}

/// CD-30（C# 语义）：类型的全限定名（FQN）。
///
/// `namespace A.B` + 名 `T` → `"A.B.T"`；全局（空 namespace）→ `"T"`。
/// `shadowed_types` 以此为键，使同短名、不同 namespace 的类型按 FQN 共存。
pub fn type_fqn(namespace: &[Ident], name: &str) -> String {
    if namespace.is_empty() {
        name.to_string()
    } else {
        format!(
            "{}.{}",
            namespace
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join("."),
            name
        )
    }
}

/// RFC 018 M3+：declared 属性签名（供 GetProperties / DeclaredProperties 名枚举）。
#[derive(Clone, Debug)]
pub struct DeclaredPropertySig {
    pub name: Ident,
    pub ty: Ident,
    pub can_read: bool,
    pub can_write: bool,
}

#[derive(Default)]
pub struct TypeRegistry {
    pub types: IndexMap<Ident, NominalType>,
    /// Extended type name → extension methods (receiver param stripped from sig).
    pub extensions: IndexMap<Ident, Vec<ExtensionMethod>>,
    /// RFC 006 M2：`(type, prop)` — 自定义 `init` 访问器属性（无 backing field 时的门禁键）。
    pub init_only_props: indexmap::IndexSet<(Ident, Ident)>,
    /// RFC 018 M3+：类型名 → 本类型声明的属性（不含继承；含自动属性与自定义访问器）。
    pub declared_properties: IndexMap<Ident, Vec<DeclaredPropertySig>>,
    /// RFC 026 M2：FileId → 包名；供 `can_access` 判定跨包 `internal`。
    ///
    /// 空表时保持单模块 MVP（`internal` 全可见），由管线 `set_file_packages` 注入后启用。
    pub file_packages: std::collections::HashMap<ast::FileId, String>,
    /// CD-30（C# 语义）：同短名、不同 namespace 的类型 FQN 索引。
    ///
    /// 对标 C#——类型按**全限定名**（`namespace + name`）区分，`Arc.Drawing.ImageNative`
    /// 与全局 `ImageNative` 是两个不同类型，天然共存，不做任何「入口包优先」遮蔽。
    /// 短名主索引 `types` 仅保留其一（先注册者/本地源码优先），冲突类型按 FQN 入本表；
    /// 解析时沿调用点当前 namespace 链选择正确条目（见 [`TypeRegistry::lookup_type`]）。
    pub shadowed_types: std::collections::HashMap<String, NominalType>,
    /// CD-30：入口包名（当前项目/入口模块的包名）。
    ///
    /// `shadow_insert` 据此判定同名类型归属：入口包声明的类型恒优先于依赖包的
    /// 同名类型（顶层类遮蔽依赖包 internal 类，对齐 C# 本包未限定名优先）。
    /// `None`（单模块/测试，`from_module` 路径）时不触发遮蔽，保持后写覆盖。
    pub entry_package: Option<String>,
    /// RFC 026 M2+：包名 → InternalsVisibleTo 列表（对标 C# `[assembly: InternalsVisibleTo]`）。
    ///
    /// 空表时仅同包可见；由管线 `set_internals_visible_to` 注入后启用。
    pub internals_visible_to: std::collections::HashMap<String, Vec<String>>,
    /// RFC 044：yield 状态机合成类名 → 宿主类名。
    ///
    /// C# 状态机是宿主嵌套类，天然可访问宿主 private 成员；Arc 合成类注入为顶级
    /// 类型，收集时据 `ClassDef::synthesized_host` 记录映射，`can_access` 据此对
    /// 宿主 private 成员放行（仅放行自己的宿主，等价嵌套可见性语义）。
    pub synth_hosts: indexmap::IndexMap<Ident, Ident>,
    /// `[Builtin]` **静态**自动属性名集合（类名 → 属性名）。
    ///
    /// 单一事实源延伸（`property_has_custom_accessors` 的静态侧）：此类属性无
    /// 真实 getter 方法体，语义完全在 codegen 按**源码形** `"Class.Prop"` 分派；
    /// MIR `user_type_static_property_func` 据此还原源码形（而非 mangled
    /// `Class::get_Prop`）。**不得**以"类在 facade 清单"整体代替本判定——
    /// facade 类中的普通静态属性（真实 body，如 `Path.DirectorySeparatorChar`）
    /// 必须走真实函数符号（历史教训：3d28f494 一律还原曾致
    /// `@Path.DirectorySeparatorChar` undefined value）。
    pub builtin_static_props: indexmap::IndexMap<Ident, indexmap::IndexSet<Ident>>,
    /// GAP #5：delegate 类型名 → TypeId::Func 的映射表。
    ///
    /// `public delegate int Converter(int value);` → `TypeId::Func { params: [TypeId::Int], ret: Box::new(TypeId::Int) }`
    pub delegate_aliases: std::collections::HashMap<String, TypeId>,
}

pub struct AccessContext {
    /// Enclosing type when checking from an instance/static method body.
    pub current_type: Option<Ident>,
    /// Extension method namespaces in scope for the current call site.
    pub extension_scope: ExtensionScope,
    /// CD-30（C# 语义）：调用点当前声明 namespace 链（`namespace A.B` → `["A","B"]`）。
    /// 类型解析沿此链自底向上选择同名类型（当前 ns → 父 ns → 全局）。
    pub enclosing_namespace: Vec<Ident>,
    /// RFC 025 M2：当前检查站点所属包名；`None` 时不启用跨包 `internal` 门禁。
    pub current_package: Option<String>,
    /// RFC 019 M-B：泛型类单态化期间为 true——与 `ensure_type_accessible` 的
    /// `mono_depth` 豁免对齐。成员查找（`resolve_field`/`resolve_method`）走
    /// `can_access_type`，若不豁免，库内 `internal` 类型（如 `DispatchContext`）
    /// 在消费端 force-instantiate 时会被误杀，body check 失败 → 扩展方法永不
    /// 单态化。
    pub skip_type_visibility: bool,
}

/// Whether two method signatures share the same parameter list (overload key).
pub fn method_params_match(a: &OopMethodSig, b: &OopMethodSig) -> bool {
    a.params.len() == b.params.len()
        && a.params.iter().zip(b.params.iter()).all(|(ap, bp)| {
            ap.ty == bp.ty
                && ap.is_ref == bp.is_ref
                && ap.is_out == bp.is_out
                && ap.is_in == bp.is_in
        })
}

/// MIR / codegen link name for a class method (`Class::M` or `Class::M_int` when overloaded).
pub fn method_link_name(class: &str, sig: &OopMethodSig, overload_count: usize) -> String {
    if overload_count <= 1 {
        format!("{class}::{}", sig.name)
    } else {
        let suffix: Vec<_> = sig
            .params
            .iter()
            .map(|p| {
                let mut s = p.ty.as_str().to_string();
                if p.is_ref {
                    s.push_str("_ref");
                }
                if p.is_out {
                    s.push_str("_out");
                }
                if p.is_in {
                    s.push_str("_in");
                }
                s
            })
            .collect();
        format!("{class}::{}_{}", sig.name, suffix.join("_"))
    }
}

/// 构造函数 link name（与 `push_typed_fn` / codegen `emit_new` 一致）。
///
/// 无参 ctor 保持 `__ctor::Class`；有参 ctor 当 `collision` 为 true 时
/// 用 `__ctor::Class_<arity>_<p0>_<p1>...`（含参数类型名消歧），否则用
/// `__ctor::Class_<arity>`（仅 arity，无冲突时与旧格式兼容）。
pub fn ctor_link_name(class: &str, param_types: &[Ident], collision: bool) -> String {
    if param_types.is_empty() {
        format!("__ctor::{class}")
    } else if !collision {
        format!("__ctor::{class}_{}", param_types.len())
    } else {
        format!(
            "__ctor::{class}_{}_{}",
            param_types.len(),
            param_types.join("_")
        )
    }
}

/// RFC 006：static / instance 同名时优先保留 static 的 Dictionary ABI 名
///（`Class::Equals` / `Class::GetHashCode`）；instance 强制按 arity 后缀消歧。
///
/// `static_count` / `instance_count` 为同名方法在各自修饰符集合内的重载数。
pub fn method_link_name_static_abi(
    class: &str,
    sig: &OopMethodSig,
    static_count: usize,
    instance_count: usize,
) -> String {
    let is_static = sig.modifier == MethodModifier::Static;
    if is_static {
        method_link_name(class, sig, static_count.max(1))
    } else {
        // 存在同名 static 时强制 mangling，避免与 `K_Equals` / `K_GetHashCode` 冲突。
        let count = if static_count > 0 {
            instance_count.max(2)
        } else {
            instance_count.max(1)
        };
        method_link_name(class, sig, count)
    }
}

/// RFC 006：泛型扩展方法符号 mangle 的**单一权威**函数。
///
/// 扩展方法恒声明于 `static class` 容器（成员恒 `static`），故其符号 mangle 基底
/// 即 `method_link_name(container, sig, overload_count)`。
///
/// 单一事实来源纪律：registry 注册（`ExtensionMethod.mangle_base`）与
/// check_class 模板存储（`FnDef.name`）都必须经此函数（或复用其已存储结果）
/// 计算，保证调用点 `call_name` 与单态化方法体符号逐字节一致——否则 tree-shake
/// 剪掉定义 → LLVM `undefined name`。
pub fn extension_mangle_base(container: &str, sig: &OopMethodSig, overload_count: usize) -> String {
    method_link_name(container, sig, overload_count)
}

impl AccessContext {
    pub fn none() -> Self {
        Self {
            current_type: None,
            extension_scope: ExtensionScope::default(),
            enclosing_namespace: vec![],
            current_package: None,
            skip_type_visibility: false,
        }
    }

    pub fn with_extension_scope(extension_scope: ExtensionScope) -> Self {
        Self {
            current_type: None,
            extension_scope,
            enclosing_namespace: vec![],
            current_package: None,
            skip_type_visibility: false,
        }
    }
}
