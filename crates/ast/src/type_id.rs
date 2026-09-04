//! Semantic type identifiers shared across compiler stages.

use crate::Ident;
use std::fmt;

/// Distinguishes the two storage contracts a `TypeId::Ref` slot can carry.
///
/// `Ref` unifies two reference semantics with the same `llvm_type_of` (the
/// inner type) but different slot invariants on the codegen side:
///
/// - [`RefKind::Var`] — by-reference parameter (`ref`/`out`/`in`) and user
///   `ref T` syntax. The slot holds a pointer P to the caller's storage.
///   Read = double load (`P = load slot; v = load P`); write = store through
///   `*P`; byref forwarding = `load slot` yields P.
/// - [`RefKind::Value`] — struct instance `this`. The slot holds the instance
///   address itself (the inner value). Read = single load; the slot is the
///   value slot. Constructed only by struct member checking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefKind {
    /// Slot holds a pointer to the referenced storage (byref parameter).
    Var,
    /// Slot holds the referenced value itself (struct instance `this`).
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TypeId {
    Void,
    Int,
    Long,
    Short,
    Byte,
    Char,
    Float,
    Double,
    Bool,
    /// Unsigned 32-bit integer (C# uint / System.UInt32).
    UInt,
    /// Unsigned 64-bit integer (C# ulong / System.UInt64).
    ULong,
    /// Unsigned 16-bit integer (C# ushort / System.UInt16).
    UShort,
    /// Signed 8-bit integer (C# sbyte / System.SByte).
    SByte,
    String,
    /// Root type of all reference types (RFC 016 M1).
    /// Class instances implicitly inherit from Object; value types require boxing (M2).
    Object,
    Named(Ident),
    Generic(Ident),
    Ref {
        inner: Box<TypeId>,
        mutable: bool,
        kind: RefKind,
    },
    Func {
        params: Vec<TypeId>,
        ret: Box<TypeId>,
    },
    Task {
        inner: Box<TypeId>,
    },
    IEnumerable {
        inner: Box<TypeId>,
    },
    IQueryable {
        inner: Box<TypeId>,
    },
    Array {
        elem: Box<TypeId>,
    },
    Expression {
        inner: Box<TypeId>,
    },
    Nullable {
        inner: Box<TypeId>,
    },
    /// SIMD vector value type (RFC 011 Phase 2): `Vector<T, N>`.
    /// Monomorphizes to LLVM `<N x T>`. `n` ∈ {4, 8, 16}; `elem` ∈ {Float, Double}.
    Vector {
        elem: Box<TypeId>,
        n: u32,
    },
    /// RFC 005：安全连续切片视图（胖指针 `{ data, length }`）。
    /// `mutable == true` → `Span<T>`；`false` → `ReadOnlySpan<T>`。
    /// 用户面无裸指针；借用寿命由 borrowck 约束（B1–B5）。
    Span {
        elem: Box<TypeId>,
        mutable: bool,
    },
    Infer,
    Error,
}

impl TypeId {
    pub fn is_iqueryable(&self) -> bool {
        matches!(self, TypeId::IQueryable { .. })
    }

    pub fn is_ienumerable(&self) -> bool {
        match self {
            TypeId::IEnumerable { .. } | TypeId::Array { .. } => true,
            // `List_<T>` monomorphizations implement `IEnumerable<T>` and
            // should be treated as enumerable by LINQ method resolution.
            // Without this, `list.Where(...)` / `from x in list ...` fail at
            // typeck with "unknown method `Where`" because the LINQ path is
            // not recognized.
            TypeId::Named(name) => name.starts_with("List_"),
            _ => false,
        }
    }

    pub fn enumerable_elem(&self) -> Option<TypeId> {
        match self {
            TypeId::IEnumerable { inner } | TypeId::Array { elem: inner } => {
                Some((**inner).clone())
            }
            // RFC 005：`foreach` over Span / ReadOnlySpan → element type（索引脱糖，非堆枚举器）。
            TypeId::Span { elem, .. } => Some((**elem).clone()),
            // `List_<T>` / `{elem}_arr` mangle 名（registry 存的字段/OOP 形参类型）：
            // 数组字段以 `{elem}_arr` 存储（与 `types_compatible` 的等价判定一致），
            // 补齐元素类型解析使 `_items[i]` 与局部数组行为一致。
            // RFC 045 P3（yield 消费侧）：方法调用返回的 `IEnumerable<T>` 经
            // 签名表以 mangle 名（`IEnumerable_int`）存储，未走 lower_type 的
            // 结构化还原——foreach 迭代变量元素推断须解码（探针实证
            // `foreach (var v in seq.S(3))` 的 v 落 Infer → `sum += v` 报错）。
            TypeId::Named(name) => list_elem_from_mangled(name)
                .or_else(|| arr_elem_from_mangled(name))
                .or_else(|| ienumerable_elem_from_mangled(name))
                .or_else(|| collection_elem_from_mangled(name)),
            _ => None,
        }
    }

    pub fn with_enumerable_elem(&self, elem: TypeId) -> TypeId {
        match self {
            TypeId::IEnumerable { .. } => TypeId::IEnumerable {
                inner: Box::new(elem),
            },
            TypeId::IQueryable { .. } => TypeId::IQueryable {
                inner: Box::new(elem),
            },
            TypeId::Array { .. } => TypeId::Array {
                elem: Box::new(elem),
            },
            // `List_<T>` monomorphization: rebuild the mangled name with the
            // new element type so `Select` on `List_Person` returning string
            // yields `List_string` (not the unchanged `List_Person`).
            TypeId::Named(name) if name.starts_with("List_") => {
                let elem_suffix = type_to_mangled_suffix(&elem);
                TypeId::Named(format!("List_{elem_suffix}").into())
            }
            other => other.clone(),
        }
    }

    pub fn is_task(&self) -> bool {
        matches!(self, TypeId::Task { .. })
    }

    pub fn task_inner(&self) -> Option<&TypeId> {
        match self {
            TypeId::Task { inner } => Some(inner.as_ref()),
            _ => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            TypeId::Void => "void".into(),
            TypeId::Int => "int".into(),
            TypeId::Long => "long".into(),
            TypeId::Short => "short".into(),
            TypeId::Byte => "byte".into(),
            TypeId::Char => "char".into(),
            TypeId::Float => "float".into(),
            TypeId::Double => "double".into(),
            TypeId::Bool => "bool".into(),
            TypeId::UInt => "uint".into(),
            TypeId::ULong => "ulong".into(),
            TypeId::UShort => "ushort".into(),
            TypeId::SByte => "sbyte".into(),
            TypeId::String => "string".into(),
            TypeId::Object => "object".into(),
            TypeId::Named(n) => n.to_string(),
            TypeId::Generic(n) => n.to_string(),
            TypeId::Task { inner } => format!("Task<{}>", inner.display()),
            TypeId::IEnumerable { inner } => format!("IEnumerable<{}>", inner.display()),
            TypeId::IQueryable { inner } => format!("IQueryable<{}>", inner.display()),
            TypeId::Array { elem } => format!("{}[]", elem.display()),
            TypeId::Expression { inner } => match inner.as_ref() {
                TypeId::Func { params, ret } => {
                    let ps: Vec<_> = params.iter().map(|p| p.display()).collect();
                    format!("Expression<Func<{}, {}>>", ps.join(", "), ret.display())
                }
                other => format!("Expression<{}>", other.display()),
            },
            TypeId::Func { params, ret } => {
                let ps: Vec<_> = params.iter().map(|p| p.display()).collect();
                format!("({}) -> {}", ps.join(", "), ret.display())
            }
            TypeId::Ref { inner, mutable, .. } => {
                if *mutable {
                    format!("mutable borrow of {}", inner.display())
                } else {
                    format!("borrow of {}", inner.display())
                }
            }
            TypeId::Infer => "_".into(),
            TypeId::Error => "<error>".into(),
            TypeId::Nullable { inner } => format!("{}?", inner.display()),
            TypeId::Vector { elem, n } => format!("Vector<{}, {}>", elem.display(), n),
            TypeId::Span { elem, mutable } => {
                if *mutable {
                    format!("Span<{}>", elem.display())
                } else {
                    format!("ReadOnlySpan<{}>", elem.display())
                }
            }
        }
    }

    /// RFC 005：是否为 `Span`/`ReadOnlySpan`。
    pub fn is_span(&self) -> bool {
        matches!(self, TypeId::Span { .. })
    }

    /// RFC 005：可变 `Span<T>`（可写索引）。
    pub fn is_mut_span(&self) -> bool {
        matches!(self, TypeId::Span { mutable: true, .. })
    }

    /// RFC 005：切片元素类型。
    pub fn span_elem(&self) -> Option<&TypeId> {
        match self {
            TypeId::Span { elem, .. } => Some(elem.as_ref()),
            _ => None,
        }
    }

    pub fn is_nullable(&self) -> bool {
        matches!(self, TypeId::Nullable { .. })
    }

    pub fn nullable_inner(&self) -> Option<&TypeId> {
        match self {
            TypeId::Nullable { inner } => Some(inner.as_ref()),
            _ => None,
        }
    }

    pub fn strip_nullable(&self) -> &TypeId {
        match self {
            TypeId::Nullable { inner } => inner.as_ref(),
            other => other,
        }
    }
}

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}

/// Extract the element `TypeId` from a monomorphized `List_T` name.
/// Returns `None` for non-`List_` names so existing `Named` types are unaffected.
fn list_elem_from_mangled(name: &str) -> Option<TypeId> {
    let suffix = name.strip_prefix("List_")?;
    Some(mangled_suffix_to_type_id(suffix))
}

/// Extract the element `TypeId` from a mangled `IEnumerable_{elem}` name
/// (method signature `ret` 以 mangle 名存储，RFC 045 P3 foreach 消费侧解码)。
fn ienumerable_elem_from_mangled(name: &str) -> Option<TypeId> {
    let suffix = name.strip_prefix("IEnumerable_")?;
    Some(mangled_suffix_to_type_id(suffix))
}

/// Extract the element `TypeId` from mangled stub-collection names
/// (`HashSet_string` / `Queue_int` / `ConcurrentBag_List_string` …).
///
/// `foreach` 对 stub 集合（typeck builtin facade）走索引脱糖，迭代变量元素
/// 类型须从 mangle 名解码——缺失时 elem 落 `Infer` → int 兜底 → 元素以 i32
/// ABI 传入 `string` 形参（get_Item 返回类型错位 → clang 拒绝 IR）。
/// 与 `builtin_facade::COLLECTION_PREFIXES` 的单元素集合对齐（Dictionary 族
/// 双类型参数迭代 `KeyValuePair<K,V>`，不在本表）。
fn collection_elem_from_mangled(name: &str) -> Option<TypeId> {
    for prefix in [
        "HashSet_",
        "SortedSet_",
        "Queue_",
        "Stack_",
        "LinkedList_",
        "ConcurrentQueue_",
        "ConcurrentBag_",
        "ConcurrentStack_",
        "BlockingCollection_",
        "ListEnumerator_",
    ] {
        if let Some(suffix) = name.strip_prefix(prefix) {
            return Some(mangled_suffix_to_type_id(suffix));
        }
    }
    None
}

/// Extract the element `TypeId` from a mangled `{elem}_arr` name. This is the
/// inverse of `type_id_to_field_name(TypeId::Array{…})`; the registry stores
/// array-typed fields/OOP params under that mangled name, so indexing/foreach
/// over such receivers must decode it to stay consistent with local arrays.
/// Returns `None` for names not ending in `_arr`.
fn arr_elem_from_mangled(name: &str) -> Option<TypeId> {
    let suffix = name.strip_suffix("_arr")?;
    Some(mangled_suffix_to_type_id(suffix))
}

/// Map a mangled suffix (element of `List_<suffix>` / `{suffix}_arr`) back to its
/// `TypeId`: primitive names map to their variants, anything else is a `Named`
/// class/struct element suffix.
fn mangled_suffix_to_type_id(suffix: &str) -> TypeId {
    match suffix {
        "int" => TypeId::Int,
        "long" => TypeId::Long,
        "short" => TypeId::Short,
        "byte" => TypeId::Byte,
        "char" => TypeId::Char,
        "float" => TypeId::Float,
        "double" => TypeId::Double,
        "bool" => TypeId::Bool,
        "uint" => TypeId::UInt,
        "ulong" => TypeId::ULong,
        "ushort" => TypeId::UShort,
        "sbyte" => TypeId::SByte,
        "string" => TypeId::String,
        "void" => TypeId::Void,
        "object" => TypeId::Object,
        other => TypeId::Named(other.into()),
    }
}

/// Inverse of `list_elem_from_mangled`: map a `TypeId` to its mangled suffix
/// used in `List_<suffix>` names. Used by `with_enumerable_elem` to rebuild
/// the monomorphized name when `Select` changes the element type.
fn type_to_mangled_suffix(ty: &TypeId) -> String {
    match ty {
        TypeId::Int => "int".into(),
        TypeId::Long => "long".into(),
        TypeId::Short => "short".into(),
        TypeId::Byte => "byte".into(),
        TypeId::Char => "char".into(),
        TypeId::Float => "float".into(),
        TypeId::Double => "double".into(),
        TypeId::Bool => "bool".into(),
        TypeId::UInt => "uint".into(),
        TypeId::ULong => "ulong".into(),
        TypeId::UShort => "ushort".into(),
        TypeId::SByte => "sbyte".into(),
        TypeId::String => "string".into(),
        TypeId::Void => "void".into(),
        TypeId::Object => "object".into(),
        TypeId::Named(n) => n.to_string(),
        // Fallback for uncommon types (Generic/Ref/Func/...): use display().
        other => other.display(),
    }
}
