//! Definite-assignment analysis for `out` parameters (C# semantics).
//!
//! Tracks which `out` parameters have been assigned within the current method
//! body. Branches (if/else) merge by intersection: an `out` parameter is
//! considered assigned after an if only if both branches assign it. Loop bodies
//! do not propagate assignments outward (the body may not execute).

use ast::Ident;
use indexmap::IndexSet;

pub struct OutParamState {
    pub params: IndexSet<Ident>,
    pub assigned: IndexSet<Ident>,
}

impl OutParamState {
    pub fn new(params: IndexSet<Ident>) -> Self {
        Self {
            params,
            assigned: IndexSet::new(),
        }
    }

    pub fn mark_assigned(&mut self, name: &Ident) {
        if self.params.contains(name) {
            self.assigned.insert(name.clone());
        }
    }

    pub fn unassigned(&self) -> Vec<Ident> {
        self.params.difference(&self.assigned).cloned().collect()
    }

    pub fn snapshot(&self) -> IndexSet<Ident> {
        self.assigned.clone()
    }

    pub fn restore(&mut self, snap: IndexSet<Ident>) {
        self.assigned = snap;
    }

    pub fn merge_intersect(&mut self, other: &IndexSet<Ident>) {
        self.assigned.retain(|n| other.contains(n));
    }
}
