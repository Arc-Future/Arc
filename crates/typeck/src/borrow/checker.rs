use ast::Ident;
use hir::HirModule;
use indexmap::IndexMap;

use crate::oop_types::TypeRegistry;
use crate::typed::TypedFn;
use crate::TypeId;

use super::binding::{Binding, Ownership};
use super::error::BorrowError;

/// 借用检查器——检查 typed HIR（`TypedFn.body`），在 MIR lower 之前运行。
///
/// 住在 `typeck`：输入是 typeck 产物与 HIR，不消费 `MirCfgBody`。
pub struct BorrowChecker {
    pub(crate) registry: TypeRegistry,
    pub(crate) fn_sigs: IndexMap<Ident, Vec<TypeId>>,
    pub(crate) bindings: IndexMap<Ident, Binding>,
    pub(crate) errors: Vec<BorrowError>,
}

impl BorrowChecker {
    pub fn new() -> Self {
        Self {
            registry: TypeRegistry {
                types: IndexMap::new(),
                extensions: IndexMap::new(),
                init_only_props: Default::default(),
                declared_properties: Default::default(),
                file_packages: Default::default(),
                internals_visible_to: Default::default(),
                shadowed_types: Default::default(),
                synth_hosts: Default::default(),
                builtin_static_props: Default::default(),
                entry_package: None,
                delegate_aliases: std::collections::HashMap::new(),
            },
            fn_sigs: IndexMap::new(),
            bindings: IndexMap::new(),
            errors: Vec::new(),
        }
    }

    pub fn check_module(
        &mut self,
        module: &HirModule,
        typed_fns: &[TypedFn],
    ) -> Result<(), Vec<BorrowError>> {
        self.registry = TypeRegistry::from_module(module);
        self.fn_sigs = typed_fns
            .iter()
            .map(|f| {
                (
                    f.name.clone(),
                    f.params.iter().map(|(_, t)| t.clone()).collect(),
                )
            })
            .collect();
        self.errors.clear();

        for tf in typed_fns {
            if let Some(body) = &tf.body {
                self.bindings.clear();
                for (name, ty) in &tf.params {
                    if name.as_str() == "this" {
                        continue;
                    }
                    self.bindings.insert(
                        name.clone(),
                        Binding {
                            ty: ty.clone(),
                            ownership: Ownership::Owned,
                        },
                    );
                }
                self.check_block(body);
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    pub(crate) fn is_move_type(&self, ty: &TypeId) -> bool {
        match ty {
            TypeId::Named(n) => {
                self.registry.is_struct(n) && self.registry.contains_class_handle(n)
            }
            _ => false,
        }
    }

    /// Whether a type needs `rt_arc_dec` on drop (class handle).
    pub fn type_needs_arc_drop(ty: &TypeId, registry: &TypeRegistry) -> bool {
        match ty {
            TypeId::Named(n) => registry.is_class(n),
            _ => false,
        }
    }
}

impl Default for BorrowChecker {
    fn default() -> Self {
        Self::new()
    }
}
