//! 符号属性表（RFC 012 M1 D3.1）。
//!
//! 存储 `DefId → Vec<ResolvedAttribute>` 映射，作为 typeck 产物的一部分，
//! 供后续阶段（如 ORM EntityMap 构建、GenerateTo 代码注入）编译期查询。
//!
//! # 架构红线
//!
//! `AttributeTable` 是编译器**通用机制**——它是「符号 → 属性」的纯映射，
//! 不感知「表」「列」「主键」等 ORM 含义。ORM 语义归 `arc-orm` 子库
//! （RFC 012 D4.1）；属性表只负责存储与查询。
//!
//! # Phase 0 范围
//!
//! M1 仅消费五个内置属性（`Table` / `Column` / `Key` / `Required` /
//! `MaxLength`）：`attr_type` 字段使用 `BUILTIN_ATTR_TYPE` 占位，
//! `named_args` 与 `ResolvedArg::Type` 留作 M3 扩展点，但数据结构
//! 一次到位避免后续破坏性变更。

use ast::{ExpressionTree, Ident, Span};
use hir::DefId;
use indexmap::IndexMap;

use crate::type_id::TypeId;

/// [Builtin] 属性解析后的元数据，记录 ABI 符号名供 codegen 分发。
#[derive(Clone, Debug)]
pub struct BuiltinMeta {
    /// ABI 符号名（如 "rt_parallel_for"），空串表示自动推导。
    pub abi: String,
}

impl BuiltinMeta {
    pub fn abi_or_derive(&self, class: &str, method: &str) -> String {
        if self.abi.is_empty() {
            format!("{}.{}", class, method)
        } else {
            self.abi.clone()
        }
    }
}

/// Phase 0 内置属性的占位 `attr_type`（RFC 009 D3.1）。
///
/// M3 引入 `Attribute` 根基类后，`attr_type` 指向 `Attribute` 派生类的
/// `DefId`；M1 阶段内置属性没有真正的派生类符号，使用 `u32::MAX`
/// 占位以避免引入未使用字段或 `Option` 包裹（保持 `ResolvedAttribute`
/// 字段非空的语义一致性）。
pub const BUILTIN_ATTR_TYPE: DefId = DefId(u32::MAX);

/// 已解析的属性（参数已类型检查），RFC 009 D3.1。
#[derive(Clone, Debug)]
pub struct ResolvedAttribute {
    /// 属性类符号 DefId（M3 起，指向 `Attribute` 派生类）。
    /// Phase 0 内置属性使用 [`BUILTIN_ATTR_TYPE`] 占位。
    pub attr_type: DefId,
    /// 属性名（如 `Table` / `Column` / `Key`，兼容 Phase 0 内置属性）。
    pub name: Ident,
    /// 已类型检查的位置参数。
    pub args: Vec<ResolvedArg>,
    /// 已类型检查的命名参数（M3 起；Phase 0 内置属性无命名参数）。
    pub named_args: Vec<(Ident, ResolvedArg)>,
    /// 属性附加的声明目标种类。
    pub target: AttributeTarget,
    /// 源码位置（用于诊断）。
    pub span: Span,
}

impl ResolvedAttribute {
    /// 构造 Phase 0 内置属性实例。
    ///
    /// `attr_type` 自动填充 [`BUILTIN_ATTR_TYPE`]，`named_args` 为空。
    /// M3 起由 `resolve_attr` 模块按属性类 DefId 构造完整实例。
    pub fn builtin(
        name: Ident,
        args: Vec<ResolvedArg>,
        target: AttributeTarget,
        span: Span,
    ) -> Self {
        Self {
            attr_type: BUILTIN_ATTR_TYPE,
            name,
            args,
            named_args: Vec::new(),
            target,
            span,
        }
    }
}

/// 已类型检查的属性参数，RFC 009 D3.1。
///
/// Phase 0 仅使用 `String` / `Int` / `Bool` 三个变体；`Type` 留作 M3
/// 扩展点（用户自定义属性的类型化参数，如 `[Range(typeof(int))]`）。
#[derive(Clone, Debug)]
pub enum ResolvedArg {
    String(String),
    Int(i64),
    Bool(bool),
    /// M3 起：类型引用参数。
    Type(TypeId),
    /// RFC 009 M4-7：Lambda 表达式参数树化的 ExpressionTree。
    ///
    /// parser 把 attribute 位置的 Lambda 收为 `AttributeArg::Lambda`，
    /// typeck 在 `convert_arg` 中调用 `ExpressionTree::from_lambda` 把
    /// Lambda AST 树化为 IR，存入此变体。供宏特性派生类构造函数的
    /// `Expression<T>` 参数（如 `[Inject(typeof(T), ctx => ...)]`）使用。
    Expression(ExpressionTree),
    /// 枚举成员路径：`<EnumName>.<Variant>`（如 `ServiceLifetime.Singleton`）。
    ///
    /// 供自定义属性以具名枚举参数传入（如 `[Inject(ServiceLifetime.Transient)]`）。
    /// 由 `resolve_member_path` 解析：`path[0]` 为注册枚举、`path[1]` 为其成员。
    Enum {
        name: Ident,
        variant: Ident,
    },
}

impl ResolvedArg {
    /// 若为 `String` 变体则返回内部值，否则 `None`。
    pub fn as_string(&self) -> Option<&str> {
        match self {
            ResolvedArg::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 若为 `Int` 变体则返回内部值，否则 `None`。
    pub fn as_int(&self) -> Option<i64> {
        match self {
            ResolvedArg::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// 若为 `Bool` 变体则返回内部值，否则 `None`。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ResolvedArg::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// 属性附加的声明目标种类，RFC 009 D3.1。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeTarget {
    Class,
    Struct,
    Interface,
    Enum,
    Method,
    Property,
    Field,
    Parameter,
    /// 枚举成员（`[Display("无")] None`）。通用属性系统：任何声明均可附加属性。
    EnumMember,
}

/// RFC 012 M3-6: `AttributeTargets` 位掩码常量（与 std/Arc/Attribute.as 对齐）。
///
/// 用于 `validate_attribute_usage` 中 `ValidOn` 位掩码校验：把
/// [`AttributeTarget`] 转换为对应 bit，与属性类的 `[AttributeUsage]`
/// 元属性的 `ValidOn` 参数按位 AND，判断目标合法性。
///
/// 值与 `std/Arc/Attribute.as` 中 `AttributeTargets` 类的 `public const int`
/// 字段一一对应；变更任一侧需同步另一侧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum AttributeTargetsBit {
    Class = 1,
    Struct = 2,
    Interface = 4,
    Enum = 8,
    Method = 16,
    Property = 32,
    Field = 64,
    Parameter = 128,
    EnumMember = 256,
    All = 511,
    /// Method | Property（与 `std/Arc/Attribute.as` 的 `MethodOrProperty` 对齐）。
    MethodOrProperty = 48,
}

/// 符号属性表（RFC 012），存储 `DefId → Vec<ResolvedAttribute>` 映射。
///
/// 注册到 typeck 产物，供后续阶段（如 ORM EntityMap 构建）查询。
/// 使用 `IndexMap` 保持插入顺序，便于诊断输出与跨运行确定性。
///
/// RFC 012 M5-2b: `Debug` + `Clone` derive 使 `Value::AttributeTable(Rc<AttributeTable>)`
/// 能保留 `Value: Debug` 约束，供受限求值器诊断输出使用；`Clone` 使
/// `expand_source_generators` 能 clone typeck 产物构造 `GeneratorContext`
/// 值（共享 Rc，避免持有 `&TypeChecker` 跨迭代借用）。
#[derive(Clone, Debug)]
pub struct AttributeTable {
    attrs: IndexMap<DefId, Vec<ResolvedAttribute>>,
}

impl AttributeTable {
    pub fn new() -> Self {
        Self {
            attrs: IndexMap::new(),
        }
    }

    /// 注册属性到符号。同一符号可注册多个属性；`AllowMultiple` / 去重
    /// 校验在 `resolve_attr` 模块完成，本模块仅负责存储。
    pub fn register(&mut self, def_id: DefId, attr: ResolvedAttribute) {
        self.attrs.entry(def_id).or_default().push(attr);
    }

    /// 查询符号是否含有指定名称的属性。
    pub fn has_attr(&self, def_id: DefId, name: &str) -> bool {
        self.find_attr(def_id, name).is_some()
    }

    /// 获取符号的所有属性。
    pub fn get_attrs(&self, def_id: DefId) -> &[ResolvedAttribute] {
        self.attrs.get(&def_id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// 获取符号指定名称的属性（取第一个；`AllowMultiple` 场景由调用方
    /// 通过 `get_attrs` 自行遍历）。
    pub fn find_attr(&self, def_id: DefId, name: &str) -> Option<&ResolvedAttribute> {
        self.attrs
            .get(&def_id)?
            .iter()
            .find(|a| a.name.as_str() == name)
    }

    /// 获取所有标记了 `[Table]` 的类型符号（RFC 009 D4.1）。
    ///
    /// 返回 `(DefId, Option<table_name>)` 列表：`table_name` 取自
    /// `[Table("name")]` 第一个位置参数；未提供参数则返回 `None`，
    /// 由调用方（如 `arc-orm` 子库）回退到类型名。
    ///
    /// **架构红线**：本方法仅做通用的「按属性名筛选 + 取参数」操作，
    /// 不感知「表」「实体」等 ORM 语义。命名沿用 RFC D3.3 约定。
    pub fn table_types(&self) -> Vec<(DefId, Option<String>)> {
        self.attrs
            .iter()
            .filter_map(|(def_id, attrs)| {
                let attr = attrs.iter().find(|a| a.name.as_str() == "Table")?;
                let name = attr
                    .args
                    .first()
                    .and_then(ResolvedArg::as_string)
                    .map(|s| s.to_string());
                Some((*def_id, name))
            })
            .collect()
    }

    /// M3 起：按属性类 `DefId` 查询。
    ///
    /// Phase 0 内置属性 `attr_type` 均为 [`BUILTIN_ATTR_TYPE`]，故此方法
    /// 在 M1 阶段对内置属性无意义；保留 API 以便 M3 直接可用而无需
    /// 破坏性变更。
    pub fn find_attr_by_type(&self, def_id: DefId, attr_type: DefId) -> Option<&ResolvedAttribute> {
        self.attrs
            .get(&def_id)?
            .iter()
            .find(|a| a.attr_type == attr_type)
    }

    /// RFC 032 B2: 按插入顺序迭代所有 `(DefId, &[ResolvedAttribute])` 条目。
    ///
    /// 通用机制 API——调用方（如 arc crate 的 QIF 收集器）按自身语义规则
    /// 筛选属性名 / 目标种类。typeck 不感知「测试」「Fact」等 QIF 语义。
    pub fn iter(&self) -> impl Iterator<Item = (DefId, &[ResolvedAttribute])> {
        self.attrs.iter().map(|(k, v)| (*k, v.as_slice()))
    }
}

impl Default for AttributeTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Ident {
        Ident::from(s)
    }

    fn table_attr(name_arg: Option<&str>, target: AttributeTarget) -> ResolvedAttribute {
        let args = match name_arg {
            Some(s) => vec![ResolvedArg::String(s.to_string())],
            None => vec![],
        };
        ResolvedAttribute::builtin(n("Table"), args, target, Span::DUMMY)
    }

    fn column_attr(col: &str, target: AttributeTarget) -> ResolvedAttribute {
        ResolvedAttribute::builtin(
            n("Column"),
            vec![ResolvedArg::String(col.to_string())],
            target,
            Span::DUMMY,
        )
    }

    #[test]
    fn register_and_get_attrs_returns_in_insertion_order() {
        let mut table = AttributeTable::new();
        let def = DefId(1);
        table.register(def, table_attr(Some("users"), AttributeTarget::Class));
        table.register(def, column_attr("id", AttributeTarget::Field));

        let attrs = table.get_attrs(def);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].name.as_str(), "Table");
        assert_eq!(attrs[1].name.as_str(), "Column");
    }

    #[test]
    fn get_attrs_unknown_def_returns_empty_slice() {
        let table = AttributeTable::new();
        assert!(table.get_attrs(DefId(99)).is_empty());
    }

    #[test]
    fn has_attr_and_find_attr_by_name() {
        let mut table = AttributeTable::new();
        let def = DefId(7);
        table.register(def, column_attr("age", AttributeTarget::Field));

        assert!(table.has_attr(def, "Column"));
        assert!(!table.has_attr(def, "Key"));

        let found = table
            .find_attr(def, "Column")
            .expect("Column must be present");
        assert_eq!(found.name.as_str(), "Column");
        assert_eq!(found.target, AttributeTarget::Field);
        assert_eq!(found.attr_type, BUILTIN_ATTR_TYPE);
        assert_eq!(found.named_args.len(), 0);
        let col_name = found.args[0].as_string().expect("first arg must be string");
        assert_eq!(col_name, "age");
    }

    #[test]
    fn find_attr_returns_none_for_missing_name() {
        let mut table = AttributeTable::new();
        let def = DefId(2);
        table.register(def, table_attr(Some("t"), AttributeTarget::Class));

        assert!(table.find_attr(def, "Column").is_none());
        assert!(table.find_attr(DefId(99), "Table").is_none());
    }

    #[test]
    fn table_types_returns_named_and_unnamed_table_attrs() {
        let mut table = AttributeTable::new();
        // User 类：[Table("users")]
        table.register(DefId(1), table_attr(Some("users"), AttributeTarget::Class));
        // Product 类：[Table]（无参数，回退到类型名）
        table.register(DefId(2), table_attr(None, AttributeTarget::Class));
        // 干扰项：DefId(3) 只有 [Column]，不应出现在 table_types 中
        table.register(DefId(3), column_attr("id", AttributeTarget::Field));

        let mut tables = table.table_types();
        tables.sort_by_key(|(def_id, _)| def_id.0);

        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0], (DefId(1), Some("users".to_string())));
        assert_eq!(tables[1], (DefId(2), None));
    }

    #[test]
    fn find_attr_by_type_matches_only_equal_attr_type() {
        let mut table = AttributeTable::new();
        let def = DefId(5);
        table.register(def, table_attr(Some("t"), AttributeTarget::Class));

        // Phase 0 内置属性的 attr_type 均为 BUILTIN_ATTR_TYPE
        let found = table
            .find_attr_by_type(def, BUILTIN_ATTR_TYPE)
            .expect("must match BUILTIN_ATTR_TYPE");
        assert_eq!(found.name.as_str(), "Table");

        // 其他 DefId 不匹配
        assert!(table.find_attr_by_type(def, DefId(42)).is_none());
    }

    #[test]
    fn resolved_arg_accessors_distinguish_variants() {
        let s = ResolvedArg::String("x".to_string());
        let i = ResolvedArg::Int(42);
        let b = ResolvedArg::Bool(true);

        assert_eq!(s.as_string(), Some("x"));
        assert_eq!(s.as_int(), None);
        assert_eq!(i.as_int(), Some(42));
        assert_eq!(i.as_string(), None);
        assert_eq!(b.as_bool(), Some(true));
        assert_eq!(b.as_int(), None);
    }
}
