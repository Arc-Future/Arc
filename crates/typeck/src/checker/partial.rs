//! RFC 037：Partial Classes（部分类）合并模块。
//!
//! 在 `TypeChecker::check_module` 入口预合并 HIR 中的 partial class 声明：
//! 递归遍历 HirModule，按 `(namespace_path, name, generic_arity)` 分组，
//! 合并每组为单一 ClassDef 后替换首处声明，移除后续声明。下游
//! `TypeRegistry::from_module` 与 `check_module_items` 仅看到合并后的
//! ClassDef，零 partial 感知。
//!
//! 架构红线遵守：partial 是通用语言机制，本模块仅提供合并通用机制，
//! 不感知 UI/Source Generator 等消费场景。

use super::*;
use crate::error::TypeError;
use hir::{HirItem, HirModule};
use indexmap::IndexMap;

/// RFC 037：Partial class 分组键。
///
/// 同一编译单元内同 key 的多个 `partial class` 声明合并为单一类型。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartialKey {
    pub namespace_path: Vec<Ident>,
    pub class_name: Ident,
    pub generic_arity: usize,
}

impl TypeChecker {
    /// RFC 037：检测 HirModule 树中是否含 partial class 声明。
    ///
    /// 用于 `check_module` 入口判断是否需要触发 HIR 克隆 + 合并流程——
    /// 多数项目无 partial class，无需克隆整个 HIR，避免无谓开销。
    pub(crate) fn has_partial_classes(&self, module: &HirModule) -> bool {
        let mut found = false;
        module.walk_items(&mut |item| {
            if found {
                return;
            }
            if let HirItem::Class { def_ast, .. } = item {
                if def_ast.is_partial {
                    found = true;
                }
            }
        });
        found
    }

    /// RFC 037：在 cloned HIR 上递归合并 partial class 声明。
    ///
    /// 调用前应已通过 `has_partial_classes` 判定需要合并。本方法递归
    /// 遍历每个子模块：
    /// 1. 收集本模块 items 中所有 partial class 的索引（按 PartialKey 分组）
    /// 2. 对每个 group：合并所有 ClassDef → 替换首处声明 → 标记后续待移除
    /// 3. 倒序移除后续 partial 声明（保索引稳定）
    /// 4. 递归处理 child modules
    ///
    /// 合并错误与一致性冲突通过 `self.errors.push` 累积，不中断流程
    /// （保留原始 ClassDef，让下游 typeck 继续报告下游错误）。
    pub(crate) fn merge_partials_in_hir(&mut self, mut module: HirModule) -> HirModule {
        self.merge_partials_in_hir_inner(&mut module, &[]);
        module
    }

    fn merge_partials_in_hir_inner(&mut self, module: &mut HirModule, parent_ns: &[Ident]) {
        let mut path = parent_ns.to_vec();
        if let Some(name) = &module.name {
            path.push(name.clone());
        }

        // 收集 partial class 分组（item index 列表）
        let mut groups: IndexMap<PartialKey, Vec<usize>> = IndexMap::new();
        for (i, item) in module.items.iter().enumerate() {
            if let HirItem::Class { def_ast, .. } = item {
                if def_ast.is_partial {
                    let key = PartialKey {
                        namespace_path: path.clone(),
                        class_name: def_ast.name.clone(),
                        generic_arity: def_ast.generics.len(),
                    };
                    groups.entry(key).or_default().push(i);
                }
            }
        }

        // 处理每个分组
        let mut to_remove: Vec<usize> = Vec::new();
        for (key, indices) in &groups {
            if indices.len() < 2 {
                self.errors.push(TypeError::Oop(format!(
                    "partial class `{}` only has one declaration; remove the `partial` modifier",
                    key.class_name
                )));
                continue;
            }

            // 收集所有 ClassDef
            let defs: Vec<ClassDef> = indices
                .iter()
                .filter_map(|&i| match &module.items[i] {
                    HirItem::Class { def_ast, .. } => Some(def_ast.clone()),
                    _ => None,
                })
                .collect();

            match self.merge_partial_group(key, &defs) {
                Ok(merged) => {
                    // 替换首处声明的 ClassDef
                    if let HirItem::Class { def_ast, .. } = &mut module.items[indices[0]] {
                        *def_ast = merged;
                    }
                    // 后续声明标记待移除
                    to_remove.extend(indices.iter().skip(1).copied());
                }
                Err(e) => {
                    self.errors.push(e);
                }
            }
        }

        // 倒序移除（保索引稳定）
        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for i in to_remove {
            module.items.remove(i);
        }

        // 递归处理 child modules
        for child in &mut module.children {
            self.merge_partials_in_hir_inner(child, &path);
        }
    }

    /// RFC 037：合并同一 partial group 的所有 ClassDef 为单一 ClassDef。
    ///
    /// 步骤：
    /// 1. 一致性校验（vis / is_static / generics / abstract）
    /// 2. bases 合并（去重，base class 唯一性校验）
    /// 3. where 子句合并（相同 param + kind 去重）
    /// 4. 字段/属性/方法/构造函数累加（重复检测）
    /// 5. attributes 累加
    /// 6. doc 取首个非 None
    fn merge_partial_group(
        &self,
        key: &PartialKey,
        defs: &[ClassDef],
    ) -> Result<ClassDef, TypeError> {
        self.validate_partial_consistency(key, defs)?;

        let mut merged_fields: Vec<FieldDef> = Vec::new();
        let mut merged_properties: Vec<PropertyDef> = Vec::new();
        let mut merged_methods: Vec<Spanned<MethodDef>> = Vec::new();
        let mut merged_constructors: Vec<Spanned<ConstructorDef>> = Vec::new();
        let mut merged_bases: Vec<Type> = Vec::new();
        let mut merged_where: Vec<TypeConstraint> = Vec::new();
        let mut merged_attrs: Vec<Attribute> = Vec::new();
        let mut merged_doc: Option<String> = None;

        for def in defs {
            // bases 合并（去重；base class 唯一性由下游 registry 验证处理）
            for base in &def.bases {
                if !merged_bases.iter().any(|b| b == base) {
                    merged_bases.push(base.clone());
                }
            }

            // where 子句合并（按 param + kind 去重）
            for c in &def.where_clause {
                let exists = merged_where.iter().any(|existing| {
                    existing.param == c.param && where_kinds_match(&existing.kind, &c.kind)
                });
                if !exists {
                    merged_where.push(c.clone());
                }
            }

            // 字段合并（重名 → 错误）
            for f in &def.fields {
                if merged_fields.iter().any(|m| m.name == f.name) {
                    return Err(TypeError::Oop(format!(
                        "partial class `{}` duplicate field `{}`",
                        key.class_name, f.name
                    )));
                }
                merged_fields.push(f.clone());
            }

            // 属性合并（重名 → 错误）
            for p in &def.properties {
                if merged_properties.iter().any(|m| m.name == p.name) {
                    return Err(TypeError::Oop(format!(
                        "partial class `{}` duplicate property `{}`",
                        key.class_name, p.name
                    )));
                }
                merged_properties.push(p.clone());
            }

            // 方法合并（同名同签名 → 错误；重载合法）
            for m in &def.methods {
                let dup = merged_methods.iter().any(|existing| {
                    existing.node.sig.name == m.node.sig.name
                        && method_signatures_match(&existing.node.sig, &m.node.sig)
                });
                if dup {
                    return Err(TypeError::Oop(format!(
                        "partial class `{}` duplicate method `{}` with same signature",
                        key.class_name, m.node.sig.name
                    )));
                }
                merged_methods.push(m.clone());
            }

            // 构造函数合并（同签名 → 错误；不同签名视为重载）
            for c in &def.constructors {
                let dup = merged_constructors
                    .iter()
                    .any(|existing| param_lists_match(&existing.node.params, &c.node.params));
                if dup {
                    return Err(TypeError::Oop(format!(
                        "partial class `{}` duplicate constructor with same signature",
                        key.class_name
                    )));
                }
                merged_constructors.push(c.clone());
            }

            // attributes 累加
            merged_attrs.extend(def.attributes.iter().cloned());

            // doc 取首个非 None
            if merged_doc.is_none() && def.doc.is_some() {
                merged_doc = def.doc.clone();
            }
        }

        // 构造合并后的 ClassDef（基于首个 def，覆盖合并字段）
        let mut merged = defs[0].clone();
        merged.is_partial = false; // 合并后等同普通 class
        merged.bases = merged_bases;
        merged.where_clause = merged_where;
        merged.fields = merged_fields;
        merged.properties = merged_properties;
        merged.methods = merged_methods;
        merged.constructors = merged_constructors;
        merged.attributes = merged_attrs;
        merged.doc = merged_doc;

        Ok(merged)
    }

    /// RFC 037：一致性校验——所有 partial 声明必须有相同的 vis / is_static /
    /// generics / abstract 修饰。
    ///
    /// C# 规范对齐：partial 声明的修饰符必须完全一致（abstract、static 等）；
    /// bases 与 where 子句可分散声明（合并阶段处理）。
    fn validate_partial_consistency(
        &self,
        key: &PartialKey,
        defs: &[ClassDef],
    ) -> Result<(), TypeError> {
        let first = &defs[0];

        if first.is_record || defs.iter().any(|d| d.is_record) {
            return Err(TypeError::Oop(format!(
                "partial record `{}` is not supported (RFC 037 D7.2 / RFC 006)",
                key.class_name
            )));
        }

        // abstract 修饰一致性（通过 method modifier 推断；class 本身的 abstract
        // 由是否有 abstract 方法体现——M1 简化：仅校验 vis/static/generics）
        for def in defs.iter().skip(1) {
            if def.vis != first.vis {
                return Err(TypeError::Oop(format!(
                    "partial class `{}` visibility mismatch: {:?} vs {:?}",
                    key.class_name, first.vis, def.vis
                )));
            }
            if def.is_static != first.is_static {
                return Err(TypeError::Oop(format!(
                    "partial class `{}` `static` modifier mismatch",
                    key.class_name
                )));
            }
            // 泛型 arity 一致
            if def.generics.len() != first.generics.len() {
                return Err(TypeError::Oop(format!(
                    "partial class `{}` generic arity mismatch: {} vs {}",
                    key.class_name,
                    first.generics.len(),
                    def.generics.len()
                )));
            }
            // 泛型参数名称、顺序一致
            for (i, (a, b)) in first.generics.iter().zip(def.generics.iter()).enumerate() {
                if a.name != b.name {
                    return Err(TypeError::Oop(format!(
                        "partial class `{}` generic parameter {} name mismatch: `{}` vs `{}`",
                        key.class_name, i, a.name, b.name
                    )));
                }
            }
        }
        Ok(())
    }
}

/// 判断两个 where 子句约束种类是否等价（用于去重）。
///
/// `Type` 变体按 AST `Type` 的 PartialEq 比较；元约束按变体种类比较。
fn where_kinds_match(a: &ConstraintKind, b: &ConstraintKind) -> bool {
    match (a, b) {
        (ConstraintKind::Class, ConstraintKind::Class) => true,
        (ConstraintKind::Struct, ConstraintKind::Struct) => true,
        (ConstraintKind::New, ConstraintKind::New) => true,
        (ConstraintKind::Type(t1), ConstraintKind::Type(t2)) => t1.node == t2.node,
        _ => false,
    }
}

/// 判断两个方法签名是否完全相同（用于重复方法检测；重载合法）。
///
/// 比较方法名、参数列表（类型 + ref/out 修饰）、泛型 arity。
/// 返回类型不参与比较（C# 重载不允许仅返回类型不同）。
fn method_signatures_match(a: &MethodSig, b: &MethodSig) -> bool {
    a.name == b.name
        && a.generics.len() == b.generics.len()
        && param_lists_match(&a.params, &b.params)
}

/// 判断两个参数列表是否类型完全一致（用于构造函数重载检测）。
fn param_lists_match(a: &[Param], b: &[Param]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.ty.node == y.ty.node && x.is_ref == y.is_ref && x.is_out == y.is_out)
}
