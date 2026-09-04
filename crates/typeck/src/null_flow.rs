//! Type / nullability flow analysis (RFC 015 + RFC 036 M2).
//!
//! Tracks:
//! - which nullable variables are known non-null (`non_null`)
//! - which variables are narrowed to a more specific type (`narrowed`)
//!
//! `if (x != null)` / `if (x is T n)` update state in the then-branch.
//! Branches merge by intersection (same as `OutParamState`).

use ast::Ident;
use indexmap::{IndexMap, IndexSet};

use crate::TypeId;

/// RFC 036 M2：原 `NullFlowState`，扩展类型窄化映射。
pub struct TypeFlowState {
    pub non_null: IndexSet<Ident>,
    /// 变量 → 窄化后的静态类型（如 `if (x is Dog)` then-branch 中 `x: Dog`）。
    pub narrowed: IndexMap<Ident, TypeId>,
}

impl TypeFlowState {
    pub fn new() -> Self {
        Self {
            non_null: IndexSet::new(),
            narrowed: IndexMap::new(),
        }
    }

    pub fn mark_non_null(&mut self, name: &Ident) {
        self.non_null.insert(name.clone());
    }

    pub fn is_non_null(&self, name: &Ident) -> bool {
        self.non_null.contains(name)
    }

    pub fn narrow(&mut self, name: &Ident, ty: TypeId) {
        self.narrowed.insert(name.clone(), ty);
    }

    pub fn narrowed_ty(&self, name: &Ident) -> Option<&TypeId> {
        self.narrowed.get(name)
    }

    pub fn un_narrow(&mut self, name: &Ident) {
        self.non_null.shift_remove(name);
        self.narrowed.shift_remove(name);
    }

    pub fn snapshot(&self) -> (IndexSet<Ident>, IndexMap<Ident, TypeId>) {
        (self.non_null.clone(), self.narrowed.clone())
    }

    pub fn restore(&mut self, snap: (IndexSet<Ident>, IndexMap<Ident, TypeId>)) {
        self.non_null = snap.0;
        self.narrowed = snap.1;
    }

    pub fn merge_intersect(&mut self, other: &(IndexSet<Ident>, IndexMap<Ident, TypeId>)) {
        self.non_null.retain(|n| other.0.contains(n));
        self.narrowed.retain(|k, v| other.1.get(k) == Some(v));
    }
}

/// 兼容旧名（内部仍可引用）。
pub type NullFlowState = TypeFlowState;
