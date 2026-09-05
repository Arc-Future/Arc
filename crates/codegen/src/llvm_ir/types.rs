//! Arc TypeId → LLVM IR type string mapping (RFC 015 Phase A).
//!
//! Uses opaque pointers (LLVM 15+): all pointer types are `ptr`.

use ast::TypeId;
use mir::MirOperand;
use typeck::ProgramLayouts;

/// RFC 008：`Func`/`Action`（含 mangled `Func_*`/`Action_*`）委托类型。
/// 跨函数边界统一为 `arc_closure*`；调用时按 env 是否为 null 选择裸/捕获 ABI。
///
/// 可空委托（`Func<...>?`）与委托同形——解包 `Nullable` 后判别，与 MIR 侧
/// `lower_type::is_delegate_type` 对齐（可空形参/字段经闭包 ABI 调用）。
pub fn is_delegate_type(ty: &TypeId) -> bool {
    let inner = match ty {
        TypeId::Nullable { inner } => inner.as_ref(),
        other => other,
    };
    match inner {
        TypeId::Func { .. } => true,
        TypeId::Named(n) => {
            n.as_str() == "Action"
                || n.as_str() == "Func"
                || n.starts_with("Func_")
                || n.starts_with("Action_")
        }
        _ => false,
    }
}

/// Return type of a `Func`/`Action` (including mangled `Func_*` / `Action_*`).
///
/// Used by IndirectCall codegen so `Func<string>` / `Lazy<string>` invoke with
/// `call ptr` rather than the legacy hardcoded `call i32` (Windows 0xC0000005).
pub fn delegate_ret_type(
    ty: &TypeId,
    _layouts: &ProgramLayouts,
    arg_count: usize,
) -> Option<TypeId> {
    let inner = match ty {
        TypeId::Nullable { inner } => inner.as_ref(),
        other => other,
    };
    match inner {
        TypeId::Func { ret, .. } => Some(ret.as_ref().clone()),
        TypeId::Named(n) if n.starts_with("Action_") => Some(TypeId::Void),
        TypeId::Named(n) if n.starts_with("Func_") => {
            let rest = n.strip_prefix("Func_")?;
            // Nested Func/Action mangling is unsupported here.
            if rest.contains("Func_") || rest.contains("Action_") {
                return None;
            }
            // 类型参数个数已知（= 调用点实参数 + 1 个返回）：单参 Func_X 的整段
            // 即返回类型——`Func<Task<int>>` → `Func_Task_int`，返回段含下划线，
            // rsplit 会误拆成 `int`（调用点按 i32 收 64 位指针 → inttoptr 截断，
            // channels readall_growth 0xC0000005 实证）。多参按尾段回退旧语义。
            let ret_part = match arg_count {
                0 => rest,
                n => {
                    let mut cur = rest;
                    for _ in 0..n {
                        cur = cur.split_once('_')?.1;
                    }
                    cur
                }
            };
            Some(demangle_simple_type_part(ret_part))
        }
        _ => None,
    }
}

pub(super) fn demangle_simple_type_part(s: &str) -> TypeId {
    match s {
        "int" => TypeId::Int,
        "long" => TypeId::Long,
        "short" => TypeId::Short,
        "byte" => TypeId::Byte,
        "char" => TypeId::Char,
        "float" => TypeId::Float,
        "double" => TypeId::Double,
        "bool" => TypeId::Bool,
        "void" => TypeId::Void,
        "string" => TypeId::String,
        "object" => TypeId::Object,
        "uint" => TypeId::UInt,
        "ulong" => TypeId::ULong,
        "ushort" => TypeId::UShort,
        "sbyte" => TypeId::SByte,
        other => TypeId::Named(other.into()),
    }
}

/// Map an Arc TypeId to its LLVM IR type string.
pub fn llvm_type_of(ty: &TypeId, layouts: &ProgramLayouts) -> String {
    match ty {
        TypeId::Void => "void".into(),
        TypeId::Bool => "i1".into(),
        TypeId::Int => "i32".into(),
        TypeId::Long => "i64".into(),
        TypeId::Short => "i16".into(),
        TypeId::Byte => "i8".into(),
        TypeId::Char => "i32".into(),
        TypeId::Float => "float".into(),
        TypeId::Double => "double".into(),
        TypeId::UInt => "i32".into(),
        TypeId::ULong => "i64".into(),
        TypeId::UShort => "i16".into(),
        TypeId::SByte => "i8".into(),
        TypeId::String => "ptr".into(),
        TypeId::Object => "ptr".into(),
        TypeId::Named(n) => named_type(n, layouts),
        TypeId::Generic(_) => "ptr".into(),
        TypeId::Ref { inner, .. } => llvm_type_of(inner, layouts),
        TypeId::Func { .. } => "ptr".into(),
        TypeId::Task { .. } => "ptr".into(),
        TypeId::IEnumerable { .. } => "ptr".into(),
        TypeId::IQueryable { .. } => "ptr".into(),
        TypeId::Array { .. } => "ptr".into(),
        TypeId::Expression { .. } => "ptr".into(),
        TypeId::Nullable { inner } => {
            // RFC 004 §值类型视图 ABI：值类型 `T?`（int?/double? 等）为内联
            // `{ i1 HasValue; T Value }` 聚合（对齐 .NET `Nullable<T>`），消除既有
            // 「指针装箱」表示的非空值悬垂解引用 AV。引用类型 `T?`（string? 等）
            // 保持 `ptr`（`null` 或句柄）。分派判定见 `nullable_value_llvm_type`。
            match nullable_value_llvm_type(inner, layouts) {
                Some(agg) => agg,
                None => "ptr".into(),
            }
        }
        TypeId::Vector { elem, n } => {
            let elem_ty = llvm_type_of(elem, layouts);
            format!("<{n} x {elem_ty}>")
        }
        // RFC 005：Span 局部/传参存为 ptr（指向栈上 `{ ptr, i32 }` 胖指针）。
        TypeId::Span { .. } => "ptr".into(),
        TypeId::Infer | TypeId::Error => "i32".into(),
    }
}

/// 基元值类型判定（int/long/short/byte/char/float/double/bool 及无符号变体）。
///
/// 不含 struct/enum——`T?` 内联布局仅覆盖基元值类型（RFC 004 §值类型视图 ABI 的
/// 可空视图边界）。此为可空值类型/引用类型的唯一判定点，`llvm_type_of`/
/// `llvm_size_of`/`llvm_align_of` 与 codegen 各可空路径均以此分派。
pub fn is_primitive_value_type(ty: &TypeId) -> bool {
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
            | TypeId::UInt
            | TypeId::ULong
            | TypeId::UShort
            | TypeId::SByte
    )
}

/// 基元值类型的 C ABI 存储 LLVM 类型（对齐 [`llvm_field_type`]：`bool` → `i32`）。
///
/// `Nullable<T>` 的 `Value` 字段是聚合内的**存储槽**（非局部 `i1` 视图），须用
/// C ABI 存储表示——`bool` 存 `i32`（`llvm_size_of`/`llvm_align_of` 按 4/4，与
/// `abi_size_align` SSoT 一致）。其余基元存储类型与 `llvm_type_of` 相同。由此在
/// 可空视图统一 bool 的双轨表示（局部 `i1` / 存储 `i32`），消除 `bool?` 局部
/// `{ i1, i1 }` 与 `?.` 字段路径（经 `llvm_field_type` → `{ i1, i32 }`）的类型
/// 不一致。
pub fn primitive_value_storage_llvm_type(ty: &TypeId) -> &'static str {
    match ty {
        TypeId::Bool => "i32",
        TypeId::Int | TypeId::UInt | TypeId::Char => "i32",
        TypeId::Long | TypeId::ULong => "i64",
        TypeId::Short | TypeId::UShort => "i16",
        TypeId::Byte | TypeId::SByte => "i8",
        TypeId::Float => "float",
        TypeId::Double => "double",
        // 非基元值类型：调用方经 `is_primitive_value_type` 前置判定，不会命中。
        _ => "i32",
    }
}

/// 值类型 `T?`（`T` 为基元值类型）的内联聚合 LLVM 类型 `{ i1, T }`。
///
/// 引用类型 `T?`（string?/class? 等）保持 `ptr`，返回 `None`。此为可空值类型
/// 布局的唯一判定点——`llvm_type_of`/`llvm_size_of`/`llvm_align_of` 均以此分派，
/// 杜绝「指针装箱」与「内联」双轨表示。
///
/// `Value` 字段按 [`primitive_value_storage_llvm_type`] 的 C ABI 存储表示取类型
/// （`bool` → `i32`），与字段路径 `llvm_field_type` 一致。
pub fn nullable_value_llvm_type(inner: &TypeId, _layouts: &ProgramLayouts) -> Option<String> {
    if is_primitive_value_type(inner) {
        let t = primitive_value_storage_llvm_type(inner);
        Some(format!("{{ i1, {t} }}"))
    } else {
        None
    }
}

/// 从内联可空聚合类型串 `{ i1, T }` 提取内层类型 `T`；非聚合返回 `None`。
///
/// 供 `emit_coalesce` 等仅持有 LLVM 类型串（非 `TypeId`）的路径判定 `T?` 内层
/// 类型。格式与 [`nullable_value_llvm_type`] 严格一致（`{ i1, T }`）。
pub fn nullable_aggregate_inner(agg_ty: &str) -> Option<&str> {
    agg_ty.strip_prefix("{ i1, ")?.strip_suffix(" }")
}

fn named_type(name: &str, layouts: &ProgramLayouts) -> String {
    if name == "object" {
        return "ptr".into();
    }
    // Structs are stored by reference (ptr to alloca'd struct), matching
    // `emit_struct_lit` which returns a ptr. The struct layout type
    // `%struct.{name}` is only used for `alloca` sizing.
    if layouts.structs.contains_key(name) {
        return "ptr".into();
    }
    // RFC 004 M1：variant 也是栈上值类型，按引用传递（ptr to alloca'd variant）。
    // `%variant.{name}` 仅用于 `alloca` sizing；local slot 存储的是 ptr。
    if layouts.variants.contains_key(name) {
        return "ptr".into();
    }
    if layouts.enums.contains(name) {
        return "i32".into();
    }
    if is_iface_name(name) {
        // Interface values are fat pointers stored as `ptr` to a `{ ptr, ptr }` struct.
        return "ptr".into();
    }
    // Class or unknown named type → pointer
    "ptr".into()
}

pub fn is_iface_name(name: &str) -> bool {
    name.starts_with('I') && name.chars().nth(1).is_some_and(|c| c.is_uppercase())
}

/// Mangled generic interface root: `IGetter_Dog` / `IGetter_IAnimal` → `IGetter`;
/// non-generic `IShape` → `IShape`.
pub fn iface_generic_root(name: &str) -> &str {
    name.split('_').next().unwrap_or(name)
}

/// 递归剥开接口装箱/拆箱包装，取返回值的**所有权源局部**。
///
/// `return (IFace)new T(...)` / `return (IFace)<local>` 在 MIR 中体现为
/// `MirOperand::Iface{ object: Local(id), .. }` 等包装。装箱结果与底层源是
/// **同一强引用**——return 即把所有权移交调用方。codegen 据此把该源局部列为
/// `returned_local`，令同步 epilogue 跳过其 `rt_arc_dec`，避免
/// 「对象 rc 1→0 现场释放、`ret` 交出悬垂指针」（DI 装饰工厂闭包回归根因，
/// 见 emit_cfg.rs Return 分支）。非 `Local` 源（字段/静态/常量）返回 `None`：
/// 它们本就不在 epilogue drop 集合（该集合只收纳局部），无需排除。
pub fn returned_owner_local(op: Option<&MirOperand>) -> Option<mir::LocalId> {
    let mut cur = op;
    loop {
        match cur {
            Some(MirOperand::Local(id)) => return Some(*id),
            Some(MirOperand::Iface { object, .. })
            | Some(MirOperand::UnboxIface { object, .. })
            | Some(MirOperand::UnboxString { object })
            | Some(MirOperand::UnboxGeneric { object, .. }) => cur = Some(object.as_ref()),
            _ => return None,
        }
    }
}

/// Integer LLVM IR types from widest to narrowest (RFC 015 Phase 2).
/// Used by `coerce_value` and `emit_binary` to decide sext/trunc direction.
pub const INT_TYS: [&str; 4] = ["i64", "i32", "i16", "i8"];

/// Rank of an integer LLVM IR type: `Some(0)` for `i64` (widest) … `Some(3)` for `i8`.
/// Non-integer types return `None`.
pub fn int_rank(ty: &str) -> Option<usize> {
    INT_TYS.iter().position(|&x| x == ty)
}

/// Whether an LLVM IR integer type corresponds to an unsigned Arc integer.
/// C# `byte` (u8) is unsigned; Arc currently has no sbyte/ushort/uint/ulong.
/// Used to pick `zext`/`uitofp`/`fptoui` vs `sext`/`sitofp`/`fptosi`.
pub fn is_unsigned_int_ty(ty: &str) -> bool {
    ty == "i8"
}

// ---- Dictionary<K,V> generic dispatch helpers (M3) ----

const KNOWN_TYPE_SUFFIXES: &[&str] = &[
    "int", "long", "short", "byte", "char", "float", "double", "bool", "string", "void", "uint",
    "ulong", "ushort", "sbyte",
];

/// RFC 024 M1: Parse a `ConcurrentDictionary_K_V` mangled name into `(K_suffix, V_suffix)`.
pub fn parse_concurrent_dict_kv(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("ConcurrentDictionary_")?;
    for k in KNOWN_TYPE_SUFFIXES {
        if let Some(v_rest) = rest.strip_prefix(&format!("{k}_")) {
            return Some((k.to_string(), v_rest.to_string()));
        }
    }
    let mut parts = rest.splitn(2, '_');
    Some((parts.next()?.to_string(), parts.next()?.to_string()))
}

/// RFC 024: Parse a single-generic concurrent collection name (Queue/Bag/Stack).
/// Returns `Some(elem_suffix)` for `ConcurrentQueue_int`, etc.
pub fn parse_concurrent_single_elem(name: &str) -> Option<&str> {
    for prefix in &[
        "ConcurrentQueue_",
        "ConcurrentBag_",
        "ConcurrentStack_",
        "BlockingCollection_",
    ] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if rest.is_empty() {
                return None;
            }
            // 重载 ctor 会带 arity 后缀（如 `BlockingCollection_int_1`）；剥掉 `_N`。
            let base = if let Some((ty, suf)) = rest.rsplit_once('_') {
                if !suf.is_empty() && suf.chars().all(|c| c.is_ascii_digit()) {
                    ty
                } else {
                    rest
                }
            } else {
                rest
            };
            // 元素后缀允许含 `_`（数组类型如 `byte[]` → `byte_arr`；泛型如 `List_int`）。
            // 尾部 `_N`（arity）已由 rsplit_once + 全数字判定剥掉，不会误判。
            return if !base.is_empty() { Some(base) } else { None };
        }
    }
    None
}

/// RFC 024 M7：PCC 底层 kind（与 `rt_blocking_collection.c` / `rt_abi.h` 对齐）。
/// `0`=Queue，`1`=Bag，`2`=Stack；非内置 PCC 返回 `None`。
pub fn pcc_kind_from_type_name(name: &str) -> Option<i32> {
    if name.starts_with("ConcurrentQueue_") {
        Some(0)
    } else if name.starts_with("ConcurrentBag_") {
        Some(1)
    } else if name.starts_with("ConcurrentStack_") {
        Some(2)
    } else {
        None
    }
}

/// Parse a `HashSet_T` mangled name into the element suffix `T`.
///
/// 嵌套泛型元素（如 `HashSet_ChannelReaderWaiter_int` 的后缀本身含 `_`）是
/// 合法单态化类名——旧实现 `contains('_')` 一刀拒绝，使集合的分发臂整体落回
/// stub 空体（Enqueue no-op → 数据黑洞，channels backpressure NULL waiter
/// 0xC0000005 实证）。`{集合}_{T}` 前缀在 mangle 契约下即该集合的单态化，
/// 后缀含 `_` 不构成误配；元素布局判定交给 `dict_kv_is_scalar` /
/// `list_elem_is_ref`（按注册布局走 ptr/ref 分支）。
pub fn parse_set_elem(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("HashSet_")?;
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

/// Parse a `Queue_T` mangled name into the element suffix `T`.
/// 嵌套泛型元素的合法性与 `parse_set_elem` 同理（`contains('_')` 旧拒绝曾使
/// `Queue_ChannelReaderWaiter_int` 的 Enqueue 降级 stub 空体）。
pub fn parse_queue_elem(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("Queue_")?;
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

/// Parse a `Stack_T` mangled name into the element suffix `T`.
pub fn parse_stack_elem(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("Stack_")?;
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

/// 判定单态化类名/类型名是否为**泛型模板（未单态化）占位名**。
///
/// 模板 stub 类（如 `CoreChannelWriter_T`）会以模板 mangling 名注册进
/// `layouts.classes`（其方法无 body、字段/签名含占位段）。任何对其的
/// ARC class 判定（`arc_class_place`）都必须返回 false——否则泛型 async
/// 方法的 T 参数在单态化为值类型（如 int）后仍被当 class 引用做帧授予
/// `rt_arc_inc`，把整型值当对象指针（RFC 046 唤醒链崩溃 #12 实证：
/// `rt_arc_inc(0x2)`）。
///
/// 占位段按 mangle 惯例为独立 `T` / `T0` / `T1`…（`_` 分段整段匹配，
/// 与 `substitute_generic_in_ty_name` 的整段语义一致——不会误伤
/// `CancellationToken` 这类含 `T` 字母但不含独立 `T` 段的名字）。
pub fn is_generic_template_name(name: &str) -> bool {
    name.split('_').any(|seg| {
        seg == "T"
            || (seg.len() >= 2
                && seg.starts_with('T')
                && seg[1..].bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Parse a `Dictionary_K_V` mangled name into `(K_suffix, V_suffix)`.
/// Returns `None` if the name doesn't start with `Dictionary_`.
pub fn parse_dict_kv(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("Dictionary_")?;
    for k in KNOWN_TYPE_SUFFIXES {
        if let Some(v_rest) = rest.strip_prefix(&format!("{k}_")) {
            return Some((k.to_string(), v_rest.to_string()));
        }
    }
    let mut parts = rest.splitn(2, '_');
    Some((parts.next()?.to_string(), parts.next()?.to_string()))
}

/// LLVM IR type string for a Dictionary K/V type suffix.
///
/// RFC 004 §值类型视图 ABI：用户定义 enum 按 `i32` 判别值（值类型）处理，
/// 与 `llvm_type_of(Named(enum))` → `i32` 一致——此前 enum 落入 `_ => "ptr"`
/// 默认分支，`Dictionary<E,V>` 键/值 ABI 被当作引用类型 → `inttoptr ptr` 坏 IR。
pub fn dict_kv_llvm_ty(suffix: &str, layouts: &ProgramLayouts) -> &'static str {
    if layouts.enums.contains(suffix) {
        return "i32";
    }
    match suffix {
        "int" | "char" | "uint" => "i32",
        "long" | "ulong" => "i64",
        "short" | "ushort" => "i16",
        "byte" | "sbyte" => "i8",
        "bool" => "i1",
        "float" => "float",
        "double" => "double",
        "string" | "void" => "ptr",
        _ => "ptr",
    }
}

/// Whether a K/V suffix is a scalar stored in pointer bits (needs conversion).
/// Covers int-family + float/double/bool。`string` is already `ptr` (no cast)。
///
/// RFC 004：enum 是标量（`i32` 判别值，inttoptr 装箱），走 `rt_hash_int`/`rt_eq_int`
/// 快路径，零装箱、无 trampoline。
pub fn dict_kv_is_scalar(suffix: &str, layouts: &ProgramLayouts) -> bool {
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
            | "uint"
            | "ulong"
            | "ushort"
            | "sbyte"
    ) || layouts.enums.contains(suffix)
}

/// RFC 032 M2：判断 K 后缀是否为用户自定义类型（非基元、非 string、非 enum）。
///
/// 用户类型键需要生成 trampoline 调用其 `GetHashCode`/`Equals` 静态方法，
/// 实现零装箱哈希（替代 `rt_hash_int` 的指针哈希）。enum 是标量（i32 判别值），
/// 走标量快路径，非用户类型。
pub fn dict_kv_is_user_type(suffix: &str, layouts: &ProgramLayouts) -> bool {
    !KNOWN_TYPE_SUFFIXES.contains(&suffix)
        && suffix != "string"
        && !suffix.is_empty()
        && !layouts.enums.contains(suffix)
}

/// RFC 038 M2：判断 user-type 后缀类型是否声明了 `Equals` 静态方法。
///
/// 用于 `Dictionary.ContainsValue` 对 user-type value 的相等性选择——
/// 对齐 C# `EqualityComparer<TValue>.Default` 语义：仅当 value 类型实现了
/// `Equals(T,T)` 时才生成 `@__dict_eq_{V}` trampoline（值相等）；否则退化到
/// **引用相等**（runtime `rt_dict_contains_value` 传 eq = null 时做指针比较）。
///
/// 泛型单态化后类名（如 `List_int`）即 layouts.classes 的 key；`List<T>` 未
/// 实现 Equals，故返回 false，避免引用未定义符号 `List_int_Equals`。
pub fn dict_value_has_equals(suffix: &str, layouts: &ProgramLayouts) -> bool {
    layouts.classes.get(suffix).is_some_and(|c| {
        c.declared_methods
            .iter()
            .any(|m| m.name.as_str() == "Equals")
    })
}

/// Runtime hash function name for a key type suffix.
/// `string` uses `rt_hash_str`; long/ulong/double keys use `rt_hash_long`
/// （64 位全量混，避免低 32 位截断导致仅高位不同的键簇聚/误判）；其余标量用
/// `rt_hash_int`（指针位 identity）。用户类型键由 `dict_user_hash_fn` 单独处理。
pub fn dict_hash_fn(k_suffix: &str) -> &'static str {
    match k_suffix {
        "string" => "@rt_hash_str",
        "long" | "ulong" | "double" => "@rt_hash_long",
        _ => "@rt_hash_int",
    }
}

/// Runtime eq function name for a key type suffix.
/// All scalar keys use `rt_eq_int` (compares pointer bits); `string` uses `rt_eq_str`.
/// 用户类型键由 `dict_user_eq_fn` 单独处理（trampoline 调用 `K_Equals`）。
pub fn dict_eq_fn(k_suffix: &str) -> &'static str {
    if k_suffix == "string" {
        "@rt_eq_str"
    } else {
        "@rt_eq_int"
    }
}

/// RFC 032 M2：用户类型键的哈希 trampoline 函数名。
///
/// 返回 `@__dict_hash_{K}`，对应 trampoline 调用 `@{K}_GetHashCode(ptr %key)`。
/// trampoline 定义由 `ModuleEmitter::emit_dict_user_trampolines` 统一发射，
/// 按 K 类型去重（多个 `Dictionary<K, *>` 实例共享同一 trampoline）。
pub fn dict_user_hash_fn(k_suffix: &str) -> String {
    format!("@__dict_hash_{k_suffix}")
}

/// RFC 032 M2：用户类型键的相等性 trampoline 函数名。
///
/// 返回 `@__dict_eq_{K}`，对应 trampoline 调用 `@{K}_Equals(ptr %a, ptr %b)`
/// 并将 `i1` 结果 `zext` 为 `i32`（runtime `rt_eq_fn` 签名要求 `int32_t` 返回）。
pub fn dict_user_eq_fn(k_suffix: &str) -> String {
    format!("@__dict_eq_{k_suffix}")
}

/// Runtime comparison function name for a key type suffix (sorted containers).
/// All scalar keys use `rt_cmp_int` (compares pointer bits); `string` uses `rt_cmp_str`.
pub fn dict_cmp_fn(k_suffix: &str) -> &'static str {
    if k_suffix == "string" {
        "@rt_cmp_str"
    } else {
        "@rt_cmp_int"
    }
}

/// Parse a `SortedDictionary_K_V` mangled name into `(K_suffix, V_suffix)`.
/// Returns `None` if the name doesn't start with `SortedDictionary_`.
pub fn parse_sorted_dict_kv(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("SortedDictionary_")?;
    // Try known type suffixes first (handles nested types like SortedDictionary_string_int)
    for k in KNOWN_TYPE_SUFFIXES {
        if let Some(v_rest) = rest.strip_prefix(&format!("{k}_")) {
            return Some((k.to_string(), v_rest.to_string()));
        }
    }
    let mut parts = rest.splitn(2, '_');
    Some((parts.next()?.to_string(), parts.next()?.to_string()))
}

/// Parse a `LinkedList_T` mangled name into the element suffix `T`.
/// Returns `None` if the name doesn't start with `LinkedList_` (but not `LinkedListNode_`).
pub fn parse_linked_list_elem(name: &str) -> Option<&str> {
    // `LinkedListNode_*` 也以 `LinkedList_` 为前缀——必须先排除，否则
    // 节点属性会被误判进 LinkedList ABI（Value 静默走 stub/`return 0`）。
    if name.starts_with("LinkedListNode_") {
        return None;
    }
    let rest = name.strip_prefix("LinkedList_")?;
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

/// Parse a `LinkedListNode_T` mangled name into the element suffix `T`.
pub fn parse_linked_list_node_elem(name: &str) -> Option<&str> {
    name.strip_prefix("LinkedListNode_")
}

/// 无 ArcHeader 的 runtime 透传句柄：禁止 `rt_arc_inc`/`rt_arc_dec`。
///
/// - `LinkedListNode_*`：`RtLinkedListNode*`；inc/dec 会把 `value` 槽当 refcount 写坏。
/// - `CancellationToken` / `CancellationTokenSource`：共享 `RtCts*`（Treiber stack +
///   atomic flag）。对句柄做 ARC retain 会把 `stack_top` 低 32 位当 refcount 递增，
///   随后 `Cancel` 遍历坏栈 → ACCESS_VIOLATION（cancellation_e2e Token+Cancel）。
/// - `Lock` / `Mutex` / `Semaphore` / `Thread` / `ThreadPoolScheduler`：`rt_*_create`
///   返回裸同步/调度结构（如 `rt_monitor_obj*`）。FieldSet 对其 `rt_arc_inc` 会把
///   `CRITICAL_SECTION`/`pthread_mutex_t` 首字段当 refcount 写坏 → 竞态
///   `Monitor.Enter` 下 0xC0000005（Lazy 字段 `Lock sync` 并发首次求值）。
/// - `TcpClient` / `TcpListener` / `Socket` / `UdpClient`：`new` 由 emit_new 接线为
///   `@rt_socket_create`，返回裸 `RtSocket*`（`{ SOCKET fd; int closed }`，无
///   ArcHeader）。ARC 把 fd 当 refcount、offset 8 的未初始化 padding 当 vtable →
///   局部 epilogue drop / 字段引用 / List 元素 上 `rt_arc_dec` 会以 `vt[2]` 读
///   0xffffffffffffffff → ACCESS_VIOLATION（RFC 025 S1 http11_complete_e2e 首次
///   打通 HttpClient.Get 时实证；此前 Get/Post 从未被 e2e 覆盖）。句柄生命周期由
///   `rt_socket_close` 显式管理。
/// - `NamedPipeServerStream` / `NamedPipeClientStream`：同形裸 `RtPipe*`（无
///   ArcHeader，offset 0 = `is_server`）。漏豁免时 async Main 完成回调对局部
///   client 的 `rt_arc_dec` 把 `is_server` 1→0 走释放分支，读 offset 8 当 vtable
///   解引用 → 批测「case 1 PASS 后下一 case BEGIN 前」批进程 0xC0000005
///   （RFC 048 M0 l2_pipe_smoke 边界崩溃实证）。句柄生命周期由 `rt_pipe_close`
///   显式管理。
pub fn is_opaque_runtime_handle(class: &str) -> bool {
    class.starts_with("LinkedListNode_")
        || class == "CancellationToken"
        || class == "CancellationTokenSource"
        // RFC 008 AsyncStream：TaskCompletionSource<T> 对象即 RtTask*（PENDING 态
        // 句柄，无 ArcHeader）——豁免 ARC inc/dec，生命周期随 await 链收口。
        || class.starts_with("TaskCompletionSource")
        || class == "Lock"
        || class == "Mutex"
        || class == "Semaphore"
        || class == "Thread"
        || class == "ThreadPoolScheduler"
        || matches!(
            class,
            "TcpClient" | "TcpListener" | "Socket" | "UdpClient"
                | "NamedPipeServerStream" | "NamedPipeClientStream"
        )
}

/// 运行时门面 `new` 构造的**唯一事实来源**：`new T()` 必须路由到 `@rt_*_create()`
/// ABI（对象即裸句柄，与 `is_opaque_runtime_handle` 的 ARC 豁免一一对应），
/// 禁止走通用 calloc+vtable+ctor。普通路径 `emit_call` 与静态路径
/// `emit_static::emit_static_new_expr` 都据此对齐——新增门面类型只许在此登记一次，
/// 任何一侧再加内联活动清单即是缺陷（曾因 emit_static 漏 `Lock` 致全量回归崩溃）。
///
/// 返回 `(RT 目标名, i32 形参个数)`：`arity==0` 无参（目标已含尾括号），调用方装配
/// `call ptr @<target>(i32 <a0>...).`；`None` → 非「简单形」门面（Thread/Socket 族
/// 需过程式闭包/绑定逻辑，仍由 emit_call 专用分支处理）。
pub fn runtime_facade_new_spec(tname: &str) -> Option<(&'static str, u8)> {
    if tname == "Lock" {
        return Some(("@rt_lock_create()", 0));
    }
    if tname == "Mutex" {
        return Some(("@rt_mutex_create()", 0));
    }
    // CancellationToken 与 CancellationTokenSource 均创建真实 RtCts（canceled=0），
    // 否则通用 calloc 会把 vtable 顶位(offset 8)误读为 canceled → 恒已取消。
    if tname == "CancellationToken" || tname == "CancellationTokenSource" {
        return Some(("@rt_cts_create()", 0));
    }
    // RFC 008：TaskCompletionSource<T> 对象即 PENDING 态 RtTask*。
    if tname.starts_with("TaskCompletionSource") {
        return Some(("@rt_task_create_pending()", 0));
    }
    if tname == "Semaphore" {
        return Some(("@rt_semaphore_create", 2));
    }
    if tname == "ThreadPoolScheduler" {
        return Some(("@rt_threadpool_create", 2));
    }
    None
}

/// 该类型 `new` 是否须拦截为运行时门面构造（普通路径与静态路径的拦截守卫）。
/// 覆盖 `runtime_facade_new_spec` 的简单形 + Thread/Socket 族过程式两类。
pub fn is_runtime_facade_new(tname: &str) -> bool {
    matches!(
        tname,
        "Lock"
            | "Mutex"
            | "Semaphore"
            | "CancellationToken"
            | "CancellationTokenSource"
            | "Thread"
            | "ThreadPoolScheduler"
            | "TcpClient"
            | "TcpListener"
            | "Socket"
            | "UdpClient"
    ) || tname.starts_with("TaskCompletionSource")
}

/// Parse a `SortedSet_T` mangled name into the element suffix `T`.
/// Returns `None` if the name doesn't start with `SortedSet_`.
pub fn parse_sorted_set_elem(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("SortedSet_")?;
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

/// Generate LLVM IR text to box a scalar value into `ptr` for the Dictionary ABI.
///
/// - int-family: direct `inttoptr`
/// - `float`:    `bitcast float → i32`, then `inttoptr i32 → ptr`
/// - `double`:   `bitcast double → i64`, then `inttoptr i64 → ptr`
/// - `bool`:     `zext i1 → i32`, then `inttoptr i32 → ptr`
///
/// `val` is the SSA value (e.g. `%key`). `prefix` disambiguates temporaries
/// (e.g. `"k"` → `%k.p`, `"v"` → `%v.p`). Returns `(ir_text, ptr_value)`.
pub fn dict_kv_scalar_to_ptr(
    suffix: &str,
    layouts: &ProgramLayouts,
    val: &str,
    prefix: &str,
) -> (String, String) {
    let ty = dict_kv_llvm_ty(suffix, layouts);
    let result = format!("%{prefix}.p");
    match suffix {
        "float" => {
            let bc = format!("%{prefix}.bc");
            (
                format!(
                    "  {bc} = bitcast {ty} {val} to i32\n  {result} = inttoptr i32 {bc} to ptr\n"
                ),
                result,
            )
        }
        "double" => {
            let bc = format!("%{prefix}.bc");
            (
                format!(
                    "  {bc} = bitcast {ty} {val} to i64\n  {result} = inttoptr i64 {bc} to ptr\n"
                ),
                result,
            )
        }
        "bool" => {
            let z = format!("%{prefix}.z");
            (
                format!("  {z} = zext {ty} {val} to i32\n  {result} = inttoptr i32 {z} to ptr\n"),
                result,
            )
        }
        _ => (format!("  {result} = inttoptr {ty} {val} to ptr\n"), result),
    }
}

/// Generate LLVM IR text to unbox a `ptr` back to a scalar type.
///
/// Inverse of [`dict_kv_scalar_to_ptr`]. `ptr_val` is the SSA `ptr` value
/// (e.g. `%r`). `prefix` disambiguates temporaries. Returns `(ir_text, result)`.
pub fn dict_kv_ptr_to_scalar(
    suffix: &str,
    layouts: &ProgramLayouts,
    ptr_val: &str,
    prefix: &str,
) -> (String, String) {
    let ty = dict_kv_llvm_ty(suffix, layouts);
    let result = format!("%{prefix}");
    match suffix {
        "float" => {
            let pi = format!("%{prefix}.pi");
            (
                format!("  {pi} = ptrtoint ptr {ptr_val} to i32\n  {result} = bitcast i32 {pi} to {ty}\n"),
                result,
            )
        }
        "double" => {
            let pi = format!("%{prefix}.pi");
            (
                format!("  {pi} = ptrtoint ptr {ptr_val} to i64\n  {result} = bitcast i64 {pi} to {ty}\n"),
                result,
            )
        }
        "bool" => {
            let pi = format!("%{prefix}.pi");
            (
                format!(
                    "  {pi} = ptrtoint ptr {ptr_val} to i32\n  {result} = trunc i32 {pi} to {ty}\n"
                ),
                result,
            )
        }
        _ => (
            format!("  {result} = ptrtoint ptr {ptr_val} to {ty}\n"),
            result,
        ),
    }
}

// ---- List<T> generic dispatch helpers (RFC 007 Phase 2) ----

/// Parse a `List_T` mangled name and return the element type suffix.
/// Returns `None` if the name doesn't start with `List_`.
pub fn parse_list_elem(name: &str) -> Option<&str> {
    name.strip_prefix("List_")
}

/// Parse `ListEnumerator_int` → `int`, `ListEnumerator_string` → `string`, etc.
pub fn parse_enumerator_elem(name: &str) -> Option<&str> {
    name.strip_prefix("ListEnumerator_")
}

/// Parse `Weak_T` mangled name into the target type suffix `T`.
/// Returns `None` if the name doesn't start with `Weak_`.
///
/// Used by `emit_stubs.rs` (ctor stub), `builtin_dispatch.rs` (TryGet inline)
/// and `arc_drop.rs` (drop sequence) to recognise any `Weak<T>` monomorphization.
pub fn parse_weak_elem(name: &str) -> Option<&str> {
    name.strip_prefix("Weak_")
}

/// Parse `DictEnumerator_int_string` → (\"int\", \"string\"), etc.
pub fn parse_dict_enumerator_kv(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("DictEnumerator_")?;
    // Try known type suffixes first
    for k in KNOWN_TYPE_SUFFIXES {
        if let Some(v_rest) = rest.strip_prefix(&format!("{k}_")) {
            return Some((k.to_string(), v_rest.to_string()));
        }
    }
    // Fallback: split on underscore
    let mut parts = rest.splitn(2, '_');
    Some((parts.next()?.to_string(), parts.next()?.to_string()))
}

/// LLVM IR type string for a List element type suffix.
///
/// Layout-aware：用户定义枚举（`layouts.enums`）按 `i32` 值类型处理（与
/// `llvm_type_of(Named(enum))` → `i32` 一致）。此前枚举落入 `_ => "ptr"`
/// 默认分支，`List<enum>` 被当作引用类型装箱 → `Add` 对枚举值做
/// `inttoptr` + `rt_arc_inc` → 0xC0000005（CD-7：`List<AILeaseKind>`）。
pub fn list_elem_llvm_ty(suffix: &str, layouts: &ProgramLayouts) -> &'static str {
    if layouts.enums.contains(suffix) {
        return "i32";
    }
    match suffix {
        "int" | "char" | "uint" => "i32",
        "long" | "ulong" => "i64",
        "short" | "ushort" => "i16",
        "byte" | "sbyte" => "i8",
        "bool" => "i1",
        "float" => "float",
        "double" => "double",
        "string" | "void" => "ptr",
        _ => "ptr",
    }
}

/// Element byte size for `rt_list_create(elem_size, eq_fn)`.
///
/// Layout-aware：用户定义枚举按 4 字节值类型（`i32`）。此前枚举落入
/// `_ => 8` 默认分支，`rt_list_create` 按 8 字节引用槽分配 → 与 `Add`
/// 的 4 字节 i32 存储错位（CD-7）。
pub fn list_elem_size(suffix: &str, layouts: &ProgramLayouts) -> i32 {
    if layouts.enums.contains(suffix) {
        return 4;
    }
    match suffix {
        "int" | "char" | "float" | "uint" => 4,
        "long" | "double" | "string" | "ulong" => 8,
        "short" | "ushort" => 2,
        "byte" | "sbyte" => 1,
        "bool" => 4,
        _ => 8,
    }
}

/// Runtime eq function for a List element type suffix.
/// Returns `@rt_list_eq_str` for string, `null` for value types (runtime uses memcmp).
pub fn list_eq_fn(suffix: &str) -> Option<&'static str> {
    if suffix == "string" {
        Some("@rt_list_eq_str")
    } else {
        None
    }
}

/// Whether a List element suffix is a reference type needing ARC maintenance.
/// `string` is `char*` (no ArcHeader) → false.
/// User-defined class types have `ArcHeader` → true.
/// Value types (int/long/short/byte/char/float/double/bool) → false.
/// Variant types are value types (stack-allocated tagged unions) → false.
/// User-defined `struct` types are heap objects **without** ArcHeader
/// （calloc + `__ctor_*`，与类同形态但无引用计数字段）——List 存储其裸指针，
/// 任何 `rt_arc_inc`/`rt_arc_dec` 都会把首字段（常为 string 指针）当 refcount
/// 改写 → 字符串指针逐字节前移、数据损坏（`rt_list_push` + `get_Item` 双侧）。
/// 与 opaque runtime handles 同源：一律跳过 ARC。
/// Func_/Action_ delegates are bare fn ptrs or `%arc_closure` values — no ArcHeader.
/// Treating them as class refs made `rt_list_push` → `rt_arc_inc` on a code
/// address (Signal.OnChanging) → 0xC0000005.
pub fn list_elem_is_ref(suffix: &str, layouts: &ProgramLayouts) -> bool {
    // 用户定义枚举是 4 字节值类型（i32），非引用类型——此前落入 `_ => true`
    // 默认分支被当作引用维护 ARC（CD-7：`List<AILeaseKind>` Add/get_Item 崩溃）。
    if layouts.enums.contains(suffix) {
        return false;
    }
    // Opaque runtime handles (TcpClient/Socket/Lock/Mutex/…) are raw structs
    // without ArcHeader — ARC inc/dec on them writes the refcount over the
    // handle's first field (e.g. RtSocket.fd → socket corruption). 与
    // arc_drop/FieldSet/emit_variant 同源：一律跳过 ARC。
    if is_opaque_runtime_handle(suffix) {
        return false;
    }
    // RFC 004 M2：variant 是栈上值类型，不需要 ARC 维护
    if layouts.variants.contains_key(suffix) {
        return false;
    }
    // 用户自定义 struct：堆对象但无 ArcHeader，List 存储裸指针，禁 ARC（同上）。
    if layouts.structs.contains_key(suffix) {
        return false;
    }
    // RFC 008 / RFC 037：委托不是带 ArcHeader 的类实例
    if suffix.starts_with("Func_") || suffix.starts_with("Action_") {
        return false;
    }
    // 数组（`{Elem}_arr`）：raw 指针、无 ArcHeader、无 typeinfo 全局常量（同 string/struct）。
    // 由 GC 管理，不参与 rt_arc_inc/dec —— 否则 rt_list_push 会以 rt_arc_inc_ref 把
    // refcount 写进数组首字节 → 数据损坏（`List<byte[]>` 0x0A→0x0C 实测）。
    // 排除已注册 nominal：泛型实例 mangle 同样以 `_arr` 结尾（`List<List<byte[]>>`
    // 元素 `List_byte_arr` 是带 ArcHeader 的类实例，需 ARC）；数组 mangle 名
    // 不是 nominal 类型、不在 layouts.classes，可判别。
    if suffix.ends_with("_arr") && !layouts.classes.contains_key(suffix) {
        return false;
    }
    // 接口值（{ptr obj, ptr itable} fat pointer）：offset 0 是对象指针，无 ArcHeader。
    // List 存储其裸指针不得走 rt_arc_inc/dec —— 否则 rt_arc_inc/dec 会把 obj 字段当
    // refcount 改写，接口分派以被污染的值解引用 → 原生崩溃（List<IMessage> 实测）。
    // 用 layouts.interfaces（含泛型接口 mangled 名）判定，避免 is_iface_name 的 I+大写
    // 启发式把 Item/Image 等类名误判为接口。
    if layouts.interfaces.contains_key(suffix) {
        return false;
    }
    !matches!(
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
            | "void"
            | "uint"
            | "ulong"
            | "ushort"
            | "sbyte"
    )
}

/// ARC inc callback for class-type elements, or `null` for value types.
pub fn list_arc_inc_fn(suffix: &str, layouts: &ProgramLayouts) -> Option<&'static str> {
    if list_elem_is_ref(suffix, layouts) {
        Some("@rt_list_arc_inc_ref")
    } else {
        None
    }
}

/// ARC dec callback for class-type elements, or `null` for value types.
pub fn list_arc_dec_fn(suffix: &str, layouts: &ProgramLayouts) -> Option<&'static str> {
    if list_elem_is_ref(suffix, layouts) {
        Some("@rt_list_arc_dec_ref")
    } else {
        None
    }
}

// ---- Tensor<T> generic dispatch helpers (RFC 021 Phase 1) ----

/// Parse a `Tensor_T` mangled name and return the element type suffix.
/// Returns `None` if the name doesn't start with `Tensor_`.
pub fn parse_tensor_elem(name: &str) -> Option<&str> {
    name.strip_prefix("Tensor_")
}

// ---- Vector<T, N> const-generic dispatch helpers (RFC 021 Phase 2) ----

/// Parse a mangled Vector class name `"Vector_{elem}_{n}"` into `(elem_llvm_ty, n)`.
/// Returns `None` if the class is not a valid Vector instantiation.
/// Used by `emit_new` to intercept `new Vector<T, N>()` and emit a value-type
/// `zeroinitializer` instead of the malloc + ctor path used by reference types.
pub fn parse_vector_class(class: &str) -> Option<(&'static str, u32)> {
    let rest = class.strip_prefix("Vector_")?;
    // Element suffix is "float" or "double" (no underscores), so split on last '_'.
    let (elem, n_str) = rest.rsplit_once('_')?;
    let elem_ty = match elem {
        "float" => "float",
        "double" => "double",
        _ => return None,
    };
    let n: u32 = n_str.parse().ok()?;
    if matches!(n, 4 | 8 | 16) {
        Some((elem_ty, n))
    } else {
        None
    }
}

/// LLVM IR type string for a Tensor element type suffix.
/// Phase 1 supports only `float` and `double` (RFC 021 design decision 10).
pub fn tensor_elem_llvm_ty(suffix: &str) -> &'static str {
    match suffix {
        "float" => "float",
        "double" => "double",
        _ => "double", // fallback; typeck should prevent other types
    }
}

/// Element byte size for `rt_tensor_create(rows, cols, elem_size)`.
pub fn tensor_elem_size(suffix: &str) -> i32 {
    match suffix {
        "float" => 4,
        "double" => 8,
        _ => 8,
    }
}

/// Size of a type in bytes (for struct layout / alloca sizing).
///
/// Must match `typeck::abi_size_align` / [2.2] C ABI：`bool` → 4（int32_t）。
pub fn llvm_size_of(ty: &TypeId) -> u64 {
    match ty {
        TypeId::Void => 0,
        TypeId::Bool => 4,
        TypeId::Int => 4,
        TypeId::Long => 8,
        TypeId::Short => 2,
        TypeId::Byte => 1,
        TypeId::Char => 4,
        TypeId::Float => 4,
        TypeId::Double => 8,
        TypeId::UInt => 4,
        TypeId::ULong => 8,
        TypeId::UShort => 2,
        TypeId::SByte => 1,
        TypeId::String => 8,
        TypeId::Nullable { inner }
            // 值类型 `T?` 内联 `{ i1, T }`：size = 2 × align(inner)（`i1` 占 0 偏移，
            // `T` 对齐到下一边界，`int?`=8B、`double?`=16B）。引用类型 `T?` = ptr(8)。
            if is_primitive_value_type(inner) => {
                2 * llvm_align_of(inner) as u64
            }
        _ => 8,
    }
}

/// RFC 009 M4：按字段类型名字符串计算字节大小（用于 SoA 数组分配）。
///
/// 与 `typeck::abi_size_align` / `llvm_size_of` 同源：`bool` → 4。
pub fn llvm_size_of_type_str(ty: &str) -> u64 {
    match ty {
        "void" => 0,
        "bool" => 4,
        "int" | "Int" => 4,
        "long" | "Long" => 8,
        "short" | "Short" => 2,
        "byte" | "Byte" => 1,
        "char" | "Char" => 4,
        "float" | "Float" => 4,
        "double" | "Double" => 8,
        "uint" | "UInt" => 4,
        "ulong" | "ULong" => 8,
        "ushort" | "UShort" => 2,
        "sbyte" | "SByte" => 1,
        "string" | "String" => 8,
        // 引用类型（class/struct/array/Task/Func 等）按 ptr 大小处理
        _ => 8,
    }
}

/// LLVM alignment for a type.
pub fn llvm_align_of(ty: &TypeId) -> u32 {
    match ty {
        TypeId::Bool => 4,
        TypeId::Int => 4,
        TypeId::Long => 8,
        TypeId::Short => 2,
        TypeId::Byte => 1,
        TypeId::Char => 4,
        TypeId::Float => 4,
        TypeId::Double => 8,
        TypeId::UInt => 4,
        TypeId::ULong => 8,
        TypeId::UShort => 2,
        TypeId::SByte => 1,
        TypeId::String => 8,
        TypeId::Nullable { inner }
            // 值类型 `T?` 内联 `{ i1, T }` 的对齐 = `T` 的对齐；引用类型 `T?` = ptr(8)。
            if is_primitive_value_type(inner) => {
                llvm_align_of(inner)
            }
        _ => 8,
    }
}

/// Map a field type string (from ClassLayout/StructLayout) to LLVM IR type.
///
/// Field storage matches C ABI：`bool` → `i32`（非 i1），与 size/align SSoT 一致。
pub fn llvm_field_type(ty_str: &str, layouts: &ProgramLayouts) -> String {
    match ty_str {
        "int" => "i32".into(),
        "long" => "i64".into(),
        "short" => "i16".into(),
        "byte" => "i8".into(),
        "char" => "i32".into(),
        "bool" => "i32".into(),
        "float" => "float".into(),
        "double" => "double".into(),
        "string" => "ptr".into(),
        "void" => "void".into(),
        "uint" => "i32".into(),
        "ulong" => "i64".into(),
        "ushort" => "i16".into(),
        "sbyte" => "i8".into(),
        other => {
            if layouts.structs.contains_key(other) {
                // RFC 012 S6 A1：Arc struct 字段按引用存储（`ptr`，指向 struct 的
                // 指针），与 `llvm_type_of` / `emit_struct_lit` / FieldSet 的
                // `store ptr` 一致。此前误用 `%struct.{other}`（内联值），导致
                // struct 字段读发射 `load %struct.X`、getter 返回 `ret %struct.X`，
                // 与函数签名 `ptr` 不匹配 → `arc build --dynamic` LLVM verifier 报错
                // "value doesn't match function result type 'ptr'"（Assembly.PackageMeta）。
                // 原生契约 struct（`native struct`）不在此表，走 `native_type_to_llvm`
                // 的 `%struct.X` 按值路径，不受影响。
                "ptr".into()
            } else if layouts.enums.contains(other) {
                "i32".into()
            } else {
                "ptr".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use typeck::ProgramLayouts;

    fn empty_layouts() -> ProgramLayouts {
        ProgramLayouts {
            classes: Default::default(),
            structs: Default::default(),
            enums: Default::default(),
            enum_variants: Default::default(),
            interfaces: Default::default(),
            variants: Default::default(),
            static_fields: Default::default(),
            observable_properties: Default::default(),
            type_full_names: Default::default(),
        }
    }

    /// bool 可空视图统一为 C ABI 存储 `i32`：`bool?` = `{ i1, i32 }`，与
    /// `llvm_size_of`/`llvm_align_of`（bool=4/4）及 `?.` 字段路径（`llvm_field_type`
    /// → i32）一致，杜绝 `{ i1, i1 }` 的类型不一致。
    #[test]
    fn bool_nullable_uses_i32_storage() {
        let layouts = empty_layouts();
        assert_eq!(
            nullable_value_llvm_type(&TypeId::Bool, &layouts).as_deref(),
            Some("{ i1, i32 }")
        );
        assert_eq!(primitive_value_storage_llvm_type(&TypeId::Bool), "i32");
        // 其余基元的存储类型与 `llvm_type_of` 相同（不回归）。
        assert_eq!(
            nullable_value_llvm_type(&TypeId::Int, &layouts).as_deref(),
            Some("{ i1, i32 }")
        );
        assert_eq!(
            nullable_value_llvm_type(&TypeId::Double, &layouts).as_deref(),
            Some("{ i1, double }")
        );
        // 引用类型可空保持 ptr（不回归）。
        assert_eq!(nullable_value_llvm_type(&TypeId::String, &layouts), None);
    }
}
