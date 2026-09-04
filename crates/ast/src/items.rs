//! Item-level AST definitions: namespaces, types, functions, fields, properties.

use crate::{Block, Expr, Ident, LambdaExpr, NativeModule, Spanned, Type};

/// C#-style attribute: `[Compile("id")]` or `[Attr(1, true)]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    /// Dotted path, e.g. `["Compile"]` or `["System", "Obsolete"]`.
    pub path: Vec<Ident>,
    /// Positional arguments (string/int/bool literals only — Sprint 1 subset).
    pub args: Vec<AttributeArg>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttributeArg {
    String(String),
    Int(i64),
    Bool(bool),
    /// RFC 012 M3：命名参数（`Label = "Age"` / `Min = 0`）。
    ///
    /// `name` 是公共可设置属性的标识符；`value` 是字面量或 `Type` 变体。
    /// 命名参数必须出现在所有位置参数之后（C# 规范）。
    Named {
        name: Ident,
        value: Box<AttributeArg>,
    },
    /// RFC 012 M3：类型引用参数（`typeof(User)`）。
    ///
    /// 用于用户自定义属性的类型化位置/命名参数。语法在 parser 层识别
    /// `typeof ( Type )`，此处保存已解析的 AST `Type` 节点；typeck 在
    /// `convert_arg` 中将其解析为 `TypeId` 后存入 `ResolvedArg::Type`。
    Type(Spanned<Type>),
    /// RFC 012 M3：成员路径常量（`AttributeTargets.Class`）。
    ///
    /// 用于属性参数中的编译期常量引用。当前仅 typeck 识别
    /// `AttributeTargets.<Name>` 并解析为位掩码 int；其他路径报错。
    /// `|` 组合（`A | B`）在 parser 层折叠为 `Int(N)`（仅当两端均为
    /// `MemberPath` 或 `Int` 时）。
    MemberPath(Vec<Ident>),
    /// RFC 009 M4-7：Lambda 表达式参数（`x => x.Age >= 18`）。
    ///
    /// 用于宏特性派生类构造函数的 `Expression<T>` 形参。parser 在
    /// `parse_attribute_arg` 中识别 Lambda 语法（带括号 / 单参数无括号），
    /// 把 `LambdaExpr` AST 存入此变体；typeck 在 `convert_arg` 中调用
    /// `ExpressionTree::from_lambda` 将其树化为 IR。
    Lambda(LambdaExpr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub items: Vec<Spanned<Item>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Namespace(NamespaceItem),
    Use(UseItem),
    Struct(StructDef),
    Class(ClassDef),
    Interface(InterfaceDef),
    Enum(EnumDef),
    Fn(FnDef),
    /// RFC 004 M1：代数数据类型 variant（`variant Name { | Case1 of T1 | Case2 }`）
    Variant(VariantDef),
    /// GAP #5：`delegate` 关键字——委托类型定义为 Func<...> 的类型别名。
    Delegate(DelegateDef),
    /// 跨语言互操作契约（RFC 016）。仅在 `.ani` 文件中出现。
    Native(NativeModule),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamespaceItem {
    /// Dotted namespace path, e.g. `["Arc", "Collections"]` for `namespace Arc.Collections`.
    pub path: Vec<Ident>,
    pub items: Vec<Spanned<Item>>,
    /// RFC 016 M3 §3.4 能力 gating Phase 1+（[4.4 能力系统]）：
    /// namespace 声明的能力集，形如 `namespace X capability io, db { ... }`。
    /// 调用有 `capability` 标签的 native module 时，当前 enclosing namespace
    /// 的有效能力集（含沿父链继承的）必须包含该 capability，否则 typeck 报错。
    /// 空 `Vec` 表示未声明（仅可调用无 capability 要求的 native module）。
    pub capabilities: Vec<Ident>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UseItem {
    /// `using Alias = N.T;` — `Some(Alias)`; otherwise `None`.
    pub alias: Option<Ident>,
    pub path: Vec<Ident>,
    /// RFC 003：`global using` 标记（单 TU 下与普通 using 语义相同）。
    pub is_global: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDef {
    pub vis: Visibility,
    pub name: Ident,
    pub ty: Spanned<Type>,
    pub is_readonly: bool,
    pub is_const: bool,
    /// RFC 006：`static` 修饰符标记。true 表示类级别静态字段（非实例字段）。
    /// 与 `is_const` 互斥（const 隐含 static）；与 `is_readonly` 可组合。
    pub is_static: bool,
    pub init: Option<Spanned<Expr>>,
    pub attributes: Vec<Attribute>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyDef {
    pub vis: Visibility,
    /// 普通属性为源码标识符；索引器（`this[...]`）固定为 `"Item"`（C# 元数据名）。
    pub name: Ident,
    pub ty: Spanned<Type>,
    pub has_get: bool,
    pub has_set: bool,
    /// RFC 006 M1：`init` 访问器（与 `has_set` 互斥）。
    pub has_init: bool,
    /// RFC 006 M3：`required` 成员——每次 `new` 的对象初始化器须赋值（或由选中 ctor 体赋值）。
    pub is_required: bool,
    /// Custom getter body; `None` for auto-property (`get;`).
    pub get_body: Option<Block>,
    /// Custom setter body; `None` for auto-property (`set;`).
    pub set_body: Option<Block>,
    /// RFC 006 A1：per-accessor 可见性。None = 继承属性自身可见性（C# 默认）。
    /// set_vis 同时作用于 set 与 init 访问器（二者互斥，共享 set_body）。
    pub get_vis: Option<Visibility>,
    pub set_vis: Option<Visibility>,
    pub modifier: MethodModifier,
    pub attributes: Vec<Attribute>,
    /// RFC 004 M1：`static abstract` 修饰符标记。
    ///
    /// true 表示此属性是接口的 `static abstract T Prop { get; }` 成员——
    /// 实现类须提供 `public static T Prop { get; }` 静态实现，调用形式
    /// `T.Prop`（T 为泛型参数）或 `Type.Prop`（具体类型）。
    /// typeck 跳过实例校验；codegen 拦截器发射基元指令或 `Type_get_Prop` 符号。
    pub is_static_abstract: bool,
    /// C# 索引器参数列表（`T this[int index]` / `V this[K key]`）。
    /// 空 = 普通命名属性；非空 = 索引器，注册为 `get_Item`/`set_Item`。
    pub index_params: Vec<Param>,
    /// 属性初值（C# `T Prop { get; } = expr;`）。仅 auto-property（无访问器体）
    /// 允许携带；语义 = backing field 初值，随后续构造注入机制在每个 ctor 起始执行
    /// `this.Prop = expr;`。与表达式体 `=> expr` 不同：初值**只算一次**（构造期），
    /// getter 读 backing field 零成本；表达式体每次访问重算。禁止把初值降级为
    /// `=>`（语义与性能均不等价）。
    pub init: Option<Spanned<Expr>>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

impl PropertyDef {
    /// 是否为 C# `this[...]` 索引器。
    pub fn is_indexer(&self) -> bool {
        !self.index_params.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstructorDef {
    pub vis: Visibility,
    pub params: Vec<Param>,
    pub body: Block,
    /// 构造器初始化器 `: base(args)` 中的实参列表。
    /// None 表示无初始化器；Some(vec) 表示存在 `: base(...)`，vec 可为空（`: base()`）。
    /// typeck 在 check ctor 时若 base_args 存在，会前置一条对基类 `__ctor::Base`
    /// 的调用到 body 起始处，由 codegen 自然发射——无需 codegen 侧特殊处理。
    /// 当前不支持 `: this(...)`（构造器链转发），后续 RFC 单独跟进。
    pub base_args: Option<Vec<Spanned<Expr>>>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructDef {
    pub vis: Visibility,
    pub is_readonly: bool,
    /// RFC 006 M3：`record struct` 声明。parser 完成位置参数脱糖与
    /// Equals / Deconstruct 合成后仍走 `Item::Struct`。
    pub is_record: bool,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub where_clause: Vec<TypeConstraint>,
    pub fields: Vec<FieldDef>,
    /// 结构体可实现接口（如 IEquatable<T>）。M1 阶段暂不激活 interface dispatch codegen，
    /// 但 AST 和 Typeck 已为此预留字段。
    pub bases: Vec<Type>,
    pub properties: Vec<PropertyDef>,
    pub methods: Vec<Spanned<MethodDef>>,
    pub constructors: Vec<Spanned<ConstructorDef>>,
    pub attributes: Vec<Attribute>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassDef {
    pub vis: Visibility,
    /// C# `static class` — sealed, no instances; may hold extension methods.
    pub is_static: bool,
    /// C# `abstract class` — 不可直接实例化，必须派生。
    /// RFC 009 M4-1：GenerateToAttribute<T> 基类标记为 abstract，强制用户派生。
    pub is_abstract: bool,
    /// RFC 037：partial class 标记。true 表示此声明是某个 partial group 的成员；
    /// typeck 在 collect 阶段按 (namespace, name, generic_arity) 分组并合并为
    /// 单一 ClassDef。合并后等同普通 class，下游 typeck/codegen 零 partial 感知。
    pub is_partial: bool,
    /// RFC 006：`record` / `record struct` 声明（`record class` 已硬拒，RFC 002）。
    /// parser 完成位置参数脱糖与 Equals / Deconstruct 合成后仍走 `Item::Class`。
    /// 禁止与 `is_partial` 同时为 true。
    pub is_record: bool,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub where_clause: Vec<TypeConstraint>,
    pub bases: Vec<Type>,
    pub fields: Vec<FieldDef>,
    pub properties: Vec<PropertyDef>,
    pub methods: Vec<Spanned<MethodDef>>,
    pub constructors: Vec<Spanned<ConstructorDef>>,
    pub attributes: Vec<Attribute>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
    /// RFC 044：yield 状态机合成类的宿主类名（编译器内部字段，parser 恒 None）。
    ///
    /// C# Roslyn 的状态机是宿主嵌套类，天然可访问宿主 private 成员；Arc 合成类
    /// 注入为顶级类型，以本字段记录宿主，typeck `can_access` 据此放行等价可见性
    /// （仅放行自己的宿主，精确复刻嵌套类语义，无越权面）。顶层函数脱糖时为 None。
    pub synthesized_host: Option<Ident>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InterfaceDef {
    pub vis: Visibility,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub where_clause: Vec<TypeConstraint>,
    /// Base interfaces (e.g. `interface IQueryable<T> : IEnumerable<T>`).
    pub bases: Vec<Type>,
    pub methods: Vec<MethodSig>,
    pub properties: Vec<PropertyDef>,
    pub attributes: Vec<Attribute>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumDef {
    pub vis: Visibility,
    pub name: Ident,
    pub variants: Vec<EnumVariant>,
    /// 枚举声明级属性（`[Attr] enum Color { ... }`）。与 class/struct 等一致，
    /// 经通用 AttributeTable 收集（通用属性系统：任何声明均可附加属性）。
    pub attributes: Vec<Attribute>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumVariant {
    pub name: Ident,
    pub fields: Vec<FieldDef>,
    /// Enum variant explicit discriminant value (M1: None = auto-increment).
    pub discriminant: Option<i64>,
    /// 枚举成员级属性（`[Display("无")] None`）。与字段/方法等成员一致，
    /// 经通用 AttributeTable 收集（通用属性系统：任何声明均可附加属性）。
    pub attributes: Vec<Attribute>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

/// RFC 004 M1：variant 标签联合类型定义。
///
/// `variant Value { | Int of int | Str of string | Null }`
///
/// 栈上值类型，布局为 `{ u8 tag; [3 x u8] pad; union payload; }`。
/// case 顺序决定 tag 值（0, 1, 2, ...）。case payload 为单一类型——
/// 多字段场景需先声明 struct 作为 payload（如 `Node of TreeNode`），
/// 禁止 tuple `(T1, T2)` 违反硬约束。
///
/// M1 仅支持非泛型 variant + 单字段/无字段 case；M2 扩展泛型 + 多字段 struct payload。
#[derive(Clone, Debug, PartialEq)]
pub struct VariantDef {
    pub vis: Visibility,
    pub name: Ident,
    /// RFC 004 M2：泛型参数（M1 范围内置为空 Vec）。
    pub generics: Vec<GenericParam>,
    /// RFC 004 M2：where 子句约束（M1 范围内置为空 Vec）。
    pub where_clause: Vec<TypeConstraint>,
    pub cases: Vec<VariantCase>,
    pub attributes: Vec<Attribute>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

/// RFC 004 M1：variant case（标签联合的一个分支）。
///
/// `| Int of int` → `VariantCase { name: "Int", payload: Some(int) }`
/// `| Null`       → `VariantCase { name: "Null", payload: None }`
///
/// case 名称遵循 PascalCase 约定。payload 为单一类型——多字段场景需先声明
/// struct（如 `Node of TreeNode`），禁止 tuple 违反硬约束。
#[derive(Clone, Debug, PartialEq)]
pub struct VariantCase {
    pub name: Ident,
    /// `None` = 无 payload case（如 `Null`）；`Some(ty)` = 单一类型 payload。
    pub payload: Option<Spanned<Type>>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

/// GAP #5：`delegate` 关键字——将委托类型定义为 `Func<ret_type, params_types...>` 的类型别名。
///
/// `public delegate int Converter(int value);` → `Func<int, int>`
/// GAP #5 扩展：泛型委托 `delegate R Map<T, R>(T x);` 按实参单态化
/// （typeck `instantiate_generic_delegate`），where 子句约束在实例化期
/// 按实参校验（`check_constraints`，与类/接口约束体系共用）。
#[derive(Clone, Debug, PartialEq)]
pub struct DelegateDef {
    pub vis: Visibility,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: Option<Spanned<Type>>,
    /// `where T : bound` 约束（扁平化条目，与 class/struct/interface/variant
    /// 同构复用 `TypeConstraint`）。字段顺序对应语法顺序：
    /// `delegate ret Name<G>(params) where ...;`
    pub where_clause: Vec<TypeConstraint>,
    pub attributes: Vec<Attribute>,
    pub doc: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Variance {
    #[default]
    Invariant,
    /// `out T` — 协变（仅接口泛型参数）。
    Covariant,
    /// `in T` — 逆变（仅接口泛型参数）。
    Contravariant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GenericParam {
    pub name: Ident,
    /// RFC 009 P1-C2：仅接口声明可非 `Invariant`。
    pub variance: Variance,
}

impl GenericParam {
    pub fn new(name: Ident) -> Self {
        Self {
            name,
            variance: Variance::Invariant,
        }
    }
}

/// 泛型约束种类
///
/// 对应 C# 的 `where T : <bound>` 语法。`Type` 为接口/基类约束；
/// `Class`/`Struct`/`New` 为元约束（不引用具体类型）。
#[derive(Clone, Debug, PartialEq)]
pub enum ConstraintKind {
    /// 类型约束（接口/基类）：`where T : IComparable<T>`
    Type(Spanned<Type>),
    /// 引用类型约束：`where T : class`
    Class,
    /// 值类型约束：`where T : struct`
    Struct,
    /// 无参构造约束：`where T : new()`
    /// 值类型隐式满足；引用类型须有 public 无参构造函数。
    /// new() 必须是同 param 的最后一个约束（C# 规范强制）。
    New,
}

/// A type constraint in a `where` clause: `T : IComparable`.
///
/// Constrains a generic type parameter to implement (be a subtype of) the
/// given interface or inherit from the given class. Checked at instantiation
/// time by typeck.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeConstraint {
    /// The type parameter being constrained (e.g., `T`).
    pub param: Ident,
    /// 约束种类（Type/Class/Struct/New）。
    pub kind: ConstraintKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodModifier {
    None,
    Virtual,
    Override,
    Abstract,
    Static,
    /// `override abstract` 组合（C# 标准）：派生抽象类重新声明基类方法为抽象，
    /// 强制更下级派生类再次实现。语义等同于 Abstract，但记录 override 语义
    /// 供 typeck 校验基类存在可重写的方法。
    OverrideAbstract,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MethodSig {
    pub vis: Visibility,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub where_clause: Vec<TypeConstraint>,
    pub params: Vec<Param>,
    pub ret: Option<Spanned<Type>>,
    pub is_async: bool,
    pub modifier: MethodModifier,
    pub attributes: Vec<Attribute>,
    /// RFC 004 M1：`static abstract` 修饰符标记。
    ///
    /// true 表示此方法是接口的 `static abstract T Method(...)` 成员——
    /// 实现类须提供 `public static T Method(...)` 静态实现，调用形式
    /// `T.Method(...)`（T 为泛型参数）或 `Type.Method(...)`（具体类型）。
    /// typeck 跳过实例校验；codegen 拦截器发射基元指令或 `Type_Method` 符号。
    pub is_static_abstract: bool,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MethodDef {
    pub sig: MethodSig,
    pub body: Option<Block>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnDef {
    pub vis: Visibility,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub where_clause: Vec<TypeConstraint>,
    pub params: Vec<Param>,
    pub ret: Option<Spanned<Type>>,
    pub body: Option<Block>,
    pub is_async: bool,
    pub attributes: Vec<Attribute>,
    /// C# `///` XML 文档注释原文（RFC 017）。typeck/codegen 不解析 XML；
    /// docgen 模块据此生成 .xml 产物。None 表示无文档注释。
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub ty: Spanned<Type>,
    /// 参数级属性（如 `[Description("...")]`，供 `[AITool]` 参数 schema 描述）。
    pub attributes: Vec<Attribute>,
    /// C# extension receiver: `this T name` as the first parameter (not instance `this`).
    pub is_extension_receiver: bool,
    /// C# `ref` parameter modifier.
    pub is_ref: bool,
    /// C# `out` parameter modifier.
    pub is_out: bool,
    /// RFC 009 P1-F #8：C# `in` 参数修饰符（`readonly ref` 语义）。
    ///
    /// `in T p` 在调用端按引用传递（避免值类型拷贝），但在方法体内
    /// 视为只读——禁止赋值、禁止传给 `out`/`ref` 参数。与 `ref`/`out`
    /// 互斥（由 parser 强制）。MIR/codegen 按 `ref` 处理（addr-of），
    /// readonly 约束在 typeck 层强制。
    pub is_in: bool,
    /// RFC 005：`params ReadOnlySpan<T>` / `params Span<T>` 可变实参。
    ///
    /// 仅允许末位形参；类型须为 Span/ROS（禁止 `params T[]`）。调用点脱糖为
    /// 栈缓冲 + Span 胖指针（零堆热路径）。与 `ref`/`out`/`in`/`this`/默认值互斥。
    pub is_params: bool,
    /// RFC 007：可选参数默认值（编译期常量表达式）。
    pub default: Option<Spanned<Expr>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
    Internal,
    Protected,
}
