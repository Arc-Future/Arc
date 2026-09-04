//! RFC 005 里程碑④：编译期声明级字段环检测（`arc-cycle-001` warning 通道）。
//!
//! 检测算法（RFC 005 §2.6）：以注册的 class 类型为节点构建「字段类型引用图」——
//! 边 = class 字段解析到已注册 class 的**强引用边** + **基类继承边**；DFS 带
//! on-stack visited 集，回边（含自环 A→A）即声明级环。
//!
//! 必须跳过：
//! - `Weak<T>` 类型字段（弱引用不断强环——`Weak<T>` 字段不构成边）；
//! - 门面类（如 `List_Element` 仅 `_handle: int`，无 class 字段边）；
//! - 基元 / `object` / 未解析名字（`registry.types` 中无对应 class）。
//!
//! 与运行时收集器完全正交：本模块只读声明（registry + mono_origins），
//! **不**碰 `crates/runtime/`、**不**碰 codegen 收集器路径。

use crate::error::TypeWarning;
use crate::oop_types::{TypeKind, TypeRegistry};
use crate::TypeId;
use ast::{ClassDef, Ident, MethodModifier, Span};
use indexmap::IndexMap;

/// 一条强引用边：从来源 class 指向 `target` class。
/// `field` 为字段名（强引用字段边），继承边为 `None`（基类子对象强持有）。
#[derive(Clone, Debug)]
struct Edge {
    target: Ident,
    field: Option<Ident>,
}

/// 判定类型名是否代表弱引用包装（`Weak<T>` 单态化名 `Weak_X`）。
fn is_weak_type(mono_origins: &IndexMap<String, (Ident, Vec<TypeId>)>, ty: &str) -> bool {
    // 主路径：mono_origins 记录模板名——`Weak_Element` → ("Weak", [Element])。
    if let Some((template, _)) = mono_origins.get(ty) {
        return template.as_str() == "Weak";
    }
    // 兜底：`Weak<T>` 的 mangle 命名约定（`resolve_instantiated_type_name`）。
    ty.starts_with("Weak_")
}

/// 声明级字段环检测器。
///
/// 运行时机：注册表完全填充后（`check_module_items` 之后、最终错误检查前）。
pub(crate) struct FieldCycleDetector<'a> {
    registry: &'a TypeRegistry,
    /// 单态化名 → (模板名, 实参列表)，用于识别 `Weak<T>` 实例（`Weak_Element` → `("Weak", …)`）。
    mono_origins: &'a IndexMap<String, (Ident, Vec<TypeId>)>,
    /// 类名 → 原始 `ClassDef` AST。用于补判「静态自动属性」——registry 将其
    /// 注册为 `is_static: false`（registry.rs `register_class`），但静态成员是
    /// 类级根，不构成实例级强引用环，必须从 AST 排除。
    class_defs: &'a IndexMap<Ident, ClassDef>,
}

impl<'a> FieldCycleDetector<'a> {
    pub(crate) fn new(
        registry: &'a TypeRegistry,
        mono_origins: &'a IndexMap<String, (Ident, Vec<TypeId>)>,
        class_defs: &'a IndexMap<Ident, ClassDef>,
    ) -> Self {
        Self {
            registry,
            mono_origins,
            class_defs,
        }
    }

    /// 字段名在 AST 中是否为静态声明（静态字段 / 静态自动属性）。
    fn is_static_member(&self, class: &Ident, field: &Ident) -> bool {
        let Some(cd) = self.class_defs.get(class) else {
            return false;
        };
        if cd.fields.iter().any(|f| f.name == *field && f.is_static) {
            return true;
        }
        if cd
            .properties
            .iter()
            .any(|p| p.name == *field && p.modifier == MethodModifier::Static)
        {
            return true;
        }
        false
    }

    /// 在完全填充的注册表上运行环检测，返回 `arc-cycle-001` warning 列表。
    pub(crate) fn detect(&self) -> Vec<TypeWarning> {
        // 1. 构建邻接表（仅含出边非空的 class 节点）。
        let mut adj: IndexMap<Ident, Vec<Edge>> = IndexMap::new();
        for (name, nom) in &self.registry.types {
            if nom.kind != TypeKind::Class {
                continue;
            }
            // 泛型模板不参与实例图（无实例字段布局；实例走单态化名节点）。
            if !nom.generic_params.is_empty() {
                continue;
            }
            let mut edges: Vec<Edge> = Vec::new();
            for (fname, finfo) in &nom.fields {
                if finfo.is_static || finfo.is_const {
                    continue;
                }
                // 静态自动属性在 registry 中注册为 is_static=false（registry.rs
                // `register_class`），须经 AST 补判排除——静态成员是类级根，
                // 不构成实例级强引用环（运行时试删 DFS 只沿实例强引用走）。
                if self.is_static_member(name, fname) {
                    continue;
                }
                let fty = finfo.ty.as_str();
                // 弱引用不断强环——`Weak<T>` 字段不构成边。
                if is_weak_type(self.mono_origins, fty) {
                    continue;
                }
                if let Some(target) = self.registry.types.get(&finfo.ty) {
                    if target.kind == TypeKind::Class {
                        edges.push(Edge {
                            target: finfo.ty.clone(),
                            field: Some(fname.clone()),
                        });
                    }
                }
            }
            // 基类继承边：子类对象强持有基类子对象（运行时试删 DFS 同沿 strong 边）。
            for base in &nom.bases {
                if let Some(base_nom) = self.registry.types.get(base) {
                    if base_nom.kind == TypeKind::Class {
                        edges.push(Edge {
                            target: base.clone(),
                            field: None,
                        });
                    }
                }
            }
            if !edges.is_empty() {
                adj.insert(name.clone(), edges);
            }
        }

        // 2. DFS 找环（on-stack visited）。
        //    状态：0 = 未访问，1 = 栈上，2 = 完成。
        let mut state: IndexMap<Ident, u8> = IndexMap::new();
        let mut stack: Vec<Ident> = Vec::new();
        let mut warnings: Vec<TypeWarning> = Vec::new();
        let starts: Vec<Ident> = adj.keys().cloned().collect();
        for start in starts {
            self.dfs(&start, &adj, &mut state, &mut stack, &mut warnings);
        }
        warnings
    }

    fn dfs(
        &self,
        node: &Ident,
        adj: &IndexMap<Ident, Vec<Edge>>,
        state: &mut IndexMap<Ident, u8>,
        stack: &mut Vec<Ident>,
        warnings: &mut Vec<TypeWarning>,
    ) {
        match state.get(node).copied().unwrap_or(0) {
            2 => return,
            1 => return, // 调用方保证不重复入栈
            _ => {}
        }
        state.insert(node.clone(), 1);
        stack.push(node.clone());
        if let Some(edges) = adj.get(node) {
            for e in edges {
                match state.get(&e.target).copied().unwrap_or(0) {
                    0 => self.dfs(&e.target, adj, state, stack, warnings),
                    1 => {
                        // 回边：从 e.target 沿栈到 node 构成环（含自环 node == e.target）。
                        let start = stack
                            .iter()
                            .position(|n| n == &e.target)
                            .expect("on-stack target must be on stack");
                        let cycle: Vec<Ident> = stack[start..].to_vec();
                        warnings.push(TypeWarning {
                            code: "arc-cycle-001",
                            message: self.format_cycle(adj, &cycle, e),
                            span: self
                                .registry
                                .types
                                .get(node)
                                .map(|n| n.span)
                                .unwrap_or(Span::DUMMY),
                        });
                    }
                    _ => {}
                }
            }
        }
        state.insert(node.clone(), 2);
        stack.pop();
    }

    /// 组装 RFC 005 §2.6 消息模板。
    ///
    /// `cycle` 为 on-stack 路径 `[n0, …, nk]`，`back_edge` 为 `nk → n0` 的边；
    /// 每跳 `ni → n(i+1)` 用出边字段名标注，继承边标注为「（基类）」。
    /// 消息措辞保持友好，无「借用/生命周期」类术语。
    fn format_cycle(
        &self,
        adj: &IndexMap<Ident, Vec<Edge>>,
        cycle: &[Ident],
        back_edge: &Edge,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();
        for i in 0..cycle.len() {
            let from = &cycle[i];
            let to = if i + 1 < cycle.len() {
                &cycle[i + 1]
            } else {
                &back_edge.target
            };
            let field = if i + 1 < cycle.len() {
                Self::find_field(adj, from, to)
            } else {
                back_edge.field.clone()
            };
            parts.push(Self::hop(from, field));
        }
        let start = cycle
            .first()
            .cloned()
            .unwrap_or_else(|| back_edge.target.clone());
        format!(
            "字段类型引用环：{} → {}（声明级环不必然泄漏；运行时收集器回收真实环；用 Weak<T> 求确定性）",
            parts.join(" → "),
            start
        )
    }

    /// 在邻接表中查 `from → to` 的字段名（继承边返回 None）。
    fn find_field(adj: &IndexMap<Ident, Vec<Edge>>, from: &Ident, to: &Ident) -> Option<Ident> {
        adj.get(from)
            .and_then(|edges| edges.iter().find(|e| &e.target == to))
            .and_then(|e| e.field.clone())
    }

    /// 单跳渲染：强引用字段 `Class.field`，继承边 `Class（基类）`。
    fn hop(class: &Ident, field: Option<Ident>) -> String {
        match field {
            Some(f) => format!("{}.{}", class, f),
            None => format!("{}（基类）", class),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_detection_via_mono_origins_and_prefix() {
        let mut origins: IndexMap<String, (Ident, Vec<TypeId>)> = IndexMap::new();
        origins.insert(
            "Weak_Element".to_string(),
            ("Weak".into(), vec![TypeId::Named("Element".into())]),
        );
        // 主路径：mono_origins 记录模板名。
        assert!(is_weak_type(&origins, "Weak_Element"));
        // 兜底：`Weak<T>` mangle 命名约定（mono_origins 尚未填充时）。
        assert!(is_weak_type(&IndexMap::new(), "Weak_List_Element"));
        // 非弱引用不误判。
        assert!(!is_weak_type(&origins, "Element"));
        assert!(!is_weak_type(&IndexMap::new(), "WeakDictionary"));
        assert!(!is_weak_type(&IndexMap::new(), "List_Element"));
    }
}
