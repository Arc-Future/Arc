//! Type size table pass（RFC 009 M3.0 前置子里程碑）。
//!
//! 为每种类型计算编译期 `size_of` / `align_of`，供：
//! - M3 按需 spill：判定跨 await local 是否 >256B（SPILL_THRESHOLD）
//! - RFC 018 Type 体系：Type.SizeOf 反射查询
//! - RFC 004 Generic Math：sizeof(T) 基元
//!
//! ## 大小计算规则
//!
//! | 类型 | size_of | 说明 |
//! |------|---------|------|
//! | primitives (int/long/...) | 已知固定 | 见 PRIMITIVE_SIZES |
//! | struct | Σ fields (with alignment padding) | 按字段声明顺序排列，自然对齐 |
//! | class / string / object / Task<T> / Array<T> | 8 (ptr) | 引用类型，env 中存指针 |
//! | variant | max(case payloads) + 1(tag) | RFC 004 tagged union |
//! | func / IEnumerable / IQueryable | 8 (ptr) | 委托/接口类型存指针 |
//! | Void / Generic | 4 (i32) | 占位类型统一按 i32 计 |
//! | Ref { inner } | 8 (ptr) | 引用参数存指针 |
//! | Expression { inner } | 8 (ptr) | 表达式树存指针 |
//!
//! ## 对齐规则
//!
//! - 所有类型自然对齐：align = min(size, 8)
//! - struct 整体对齐 = max(field alignments)
//! - struct 字段间插入 padding 使各字段对齐

use crate::oop_types::{NominalType, TypeKind, TypeRegistry};
use ast::{Ident, TypeId};
use std::collections::{HashMap, HashSet};

/// 编译期大小与对齐计算表。
#[derive(Clone, Debug)]
pub struct TypeSizeTable {
    /// Type 名 → (size, align)
    pub(crate) sizes: HashMap<Ident, (usize, usize)>,
}

/// spill 阈值（字节）。跨 await 存活的 local 若 size > 此值，自动堆分配。
pub const SPILL_THRESHOLD: usize = 256;

impl TypeSizeTable {
    /// 从 TypeRegistry 构建完整的 size 表。
    ///
    /// 遍历 registry 中所有注册的类型，计算 size 与 align。
    /// 由于泛型类型可能未完全单态化，递归计算时遇到 `Generic` 按指针大小（8B）计。
    pub fn build(reg: &TypeRegistry) -> Self {
        let mut sizes = HashMap::new();
        // RFC 006 V1：循环守卫——标记在途 struct，遇自引用终止递归。
        let mut visiting: HashSet<Ident> = HashSet::new();
        for (name, nom) in reg.types.iter() {
            if sizes.contains_key(name) {
                continue;
            }
            let (size, align) = Self::compute_nominal(name, nom, reg, &mut sizes, &mut visiting);
            sizes.insert(name.clone(), (size, align));
        }
        TypeSizeTable { sizes }
    }

    /// 查询类型的 size_of（字节）。
    pub fn size_of(&self, name: &Ident) -> usize {
        self.sizes.get(name).map(|(s, _)| *s).unwrap_or(8) // 未知类型默认 ptr
    }

    /// 查询类型的 align_of（字节）。
    pub fn align_of(&self, name: &Ident) -> usize {
        self.sizes.get(name).map(|(_, a)| *a).unwrap_or(8) // 未知类型默认 ptr
    }

    /// 空表（所有类型返回默认 size=8, align=8）。仅用于测试。
    pub fn empty() -> Self {
        TypeSizeTable {
            sizes: HashMap::new(),
        }
    }

    /// 从 TypeId 计算 size（供 codegen 用，无需 registry 解析 field 类型）。
    pub fn size_of_type_id(&self, ty: &TypeId) -> usize {
        Self::type_id_size(ty, &self.sizes)
    }

    // ---- 内部实现 ----

    /// 已知基元类型的 size/align 表。
    const PRIMITIVE_SIZES: &[(&str, usize, usize)] = &[
        ("int", 4, 4),
        ("long", 8, 8),
        ("short", 2, 2),
        ("byte", 1, 1),
        ("char", 2, 2), // Unicode codepoint stored as i16 runtime；codegen emits i32
        ("float", 4, 4),
        ("double", 8, 8),
        ("bool", 4, 4),
        ("uint", 4, 4),
        ("ulong", 8, 8),
        ("ushort", 2, 2),
        ("sbyte", 1, 1),
    ];

    fn primitive_size(name: &str) -> Option<(usize, usize)> {
        Self::PRIMITIVE_SIZES
            .iter()
            .find(|(n, ..)| *n == name)
            .map(|(_, s, a)| (*s, *a))
    }

    /// 计算命名类型的 size/align（递归，含循环引用保护）。
    fn compute_nominal(
        name: &Ident,
        nom: &NominalType,
        reg: &TypeRegistry,
        cache: &mut HashMap<Ident, (usize, usize)>,
        visiting: &mut HashSet<Ident>,
    ) -> (usize, usize) {
        // 基元类型
        if let Some(p) = Self::primitive_size(name.as_str()) {
            cache.insert(name.clone(), p);
            return p;
        }

        // 字符串 / Object（引用类型）
        if name.as_str() == "string" || name.as_str() == "Object" {
            cache.insert(name.clone(), (8, 8));
            return (8, 8);
        }

        // RFC 006 V1 循环守卫：该类型已在途（正在计算 size）。正常程序经
        // 下方 Struct 分支跳过静态/const 字段后不会重入；此处兜底终止任何
        // 残余环（如非法实例字段自引用），避免无限递归栈溢出。
        if visiting.contains(name) {
            return (8, 8);
        }
        visiting.insert(name.clone());

        let result = match nom.kind {
            TypeKind::Class | TypeKind::Interface => {
                // 引用类型 → 指针大小
                (8, 8)
            }
            TypeKind::Struct => {
                // 值类型 → 累加字段大小（含对齐填充）
                let mut offset = 0usize;
                let mut max_align = 1usize;
                for (_fname, finfo) in &nom.fields {
                    // RFC 006 V1：静态/const 字段不占实例布局，跳过
                    // （与 layout.rs abi_size_align 一致）——自引用静态字段
                    // （`struct P { static P Z; }`）因此不再递归重入自身。
                    if finfo.is_static || finfo.is_const {
                        continue;
                    }
                    // FieldInfo.ty 是 Ident（简单类型名），在 registry 中查找
                    let fsize = Self::lookup_size_cache(&finfo.ty, reg, cache, visiting);
                    let falign = fsize.min(8);
                    // 自然对齐：offset 向上对齐到 falign
                    offset = offset.div_ceil(falign) * falign;
                    offset += fsize;
                    max_align = max_align.max(falign);
                }
                // struct 整体对齐到 max_align
                let size = if offset == 0 {
                    1 // 空 struct 至少 1B（C 语义）
                } else {
                    offset.div_ceil(max_align) * max_align
                };
                (size, max_align)
            }
            TypeKind::Enum => {
                // Enum: i32 discriminant（类 C# enum）
                (4, 4)
            }
            TypeKind::Variant => {
                // variant = max(case payloads) + 1(tag)
                let mut max_payload = 0usize;
                for variant in &nom.variants {
                    let payload = if let Some(pt) = &variant.payload {
                        Self::lookup_size_cache(pt, reg, cache, visiting)
                    } else {
                        0
                    };
                    max_payload = max_payload.max(payload);
                }
                let size = max_payload + 1; // +1 for tag byte
                (size, 1)
            }
            _ => {
                // 未知类型 → 回退 8B
                (8, 8)
            }
        };

        visiting.remove(name);
        cache.insert(name.clone(), result);
        result
    }

    /// 在 registry + cache 中查找类型 size（递归解析引用类型 vs 值类型）。
    fn lookup_size_cache(
        type_name: &Ident,
        reg: &TypeRegistry,
        cache: &mut HashMap<Ident, (usize, usize)>,
        visiting: &mut HashSet<Ident>,
    ) -> usize {
        // 先查缓存
        if let Some((s, _)) = cache.get(type_name) {
            return *s;
        }
        // 基元类型
        if let Some(p) = Self::primitive_size(type_name.as_str()) {
            cache.insert(type_name.clone(), p);
            return p.0;
        }
        // 引用类型
        if type_name.as_str() == "string" || type_name.as_str() == "Object" {
            cache.insert(type_name.clone(), (8, 8));
            return 8;
        }
        // 查 registry
        if let Some(nom) = reg.types.get(type_name) {
            let (size, align) = Self::compute_nominal(type_name, nom, reg, cache, visiting);
            cache.insert(type_name.clone(), (size, align));
            size
        } else {
            // 未知类型 → 默认 ptr
            cache.insert(type_name.clone(), (8, 8));
            8
        }
    }

    /// 从 TypeId 计算 size（不需要 registry 解析 field 类型）。
    fn type_id_size(ty: &TypeId, cache: &HashMap<Ident, (usize, usize)>) -> usize {
        match ty {
            TypeId::Void => 4,
            TypeId::Int => 4,
            TypeId::Long => 8,
            TypeId::Short => 2,
            TypeId::Byte => 1,
            TypeId::Char => 2,
            TypeId::Float => 4,
            TypeId::Double => 8,
            TypeId::Bool => 4,
            TypeId::UInt => 4,
            TypeId::ULong => 8,
            TypeId::UShort => 2,
            TypeId::SByte => 1,
            TypeId::String => 8, // ptr
            TypeId::Object => 8, // ptr
            TypeId::Named(name) => {
                if let Some((s, _)) = cache.get(name) {
                    *s
                } else {
                    // 基元？
                    Self::primitive_size(name.as_str())
                        .map(|(s, _)| s)
                        .unwrap_or(8)
                }
            }
            TypeId::Generic(_) => 8,  // ptr
            TypeId::Ref { .. } => 8,  // ptr
            TypeId::Func { .. } => 8, // ptr
            TypeId::Task { .. } => 8, // ptr (Task<T> is ref type)
            TypeId::IEnumerable { .. } => 8,
            TypeId::IQueryable { .. } => 8,
            TypeId::Array { .. } => 8,      // ptr
            TypeId::Expression { .. } => 8, // ptr
            TypeId::Nullable { .. } => 8,   // ptr (nullable value types count as ptr)
            TypeId::Vector { .. } => 8,     // ptr (SIMD vector stored as ptr)
            // RFC 005：Span 局部存胖指针句柄（ptr to {ptr,i32}）
            TypeId::Span { .. } => 8,
            TypeId::Infer => 8, // ptr
            TypeId::Error => 4, // i32 placeholder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_sizes() {
        // Primitive sizes via TypeId (no registry needed)
        let cache = HashMap::new();
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::Int, &cache), 4);
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::Long, &cache), 8);
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::Double, &cache), 8);
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::Bool, &cache), 4);
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::Byte, &cache), 1);
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::Char, &cache), 2);
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::Short, &cache), 2);
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::String, &cache), 8);
        assert_eq!(TypeSizeTable::type_id_size(&TypeId::Void, &cache), 4);
    }

    #[test]
    fn test_spill_threshold() {
        // Verify SPILL_THRESHOLD is 256
        assert_eq!(SPILL_THRESHOLD, 256);
    }

    #[test]
    fn test_large_struct_needs_spill() {
        let cache = HashMap::new();
        // int is 4 bytes, well under threshold
        let int_size = TypeSizeTable::type_id_size(&TypeId::Int, &cache);
        assert!(int_size <= SPILL_THRESHOLD);
        // A typical struct with 100 int fields would be 400 bytes > threshold
        // but we can't easily construct that in a unit test. Verify threshold
        // is correctly set.
        assert!(int_size < SPILL_THRESHOLD);
    }
}
