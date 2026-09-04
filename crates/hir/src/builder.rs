//! Lower AST programs to HIR.

use crate::module::*;
use ast::*;
use indexmap::IndexMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HirError {
    #[error("undefined symbol `{0}`")]
    UndefinedSymbol(String),
    #[error("duplicate definition `{0}`")]
    DuplicateDefinition(String),
    #[error("unresolved import `{0}`")]
    UnresolvedImport(String),
}

pub struct HirBuilder {
    next_id: u32,
    modules: Vec<HirModule>,
}

impl HirBuilder {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            modules: vec![HirModule {
                name: None,
                defs: IndexMap::new(),
                items: vec![],
                children: vec![],
                imports: vec![],
                capabilities: vec![],
            }],
        }
    }

    fn alloc_def(&mut self, name: Ident, kind: DefKind, span: Span) -> Result<DefId, HirError> {
        let module = self.modules.last_mut().unwrap();
        if let Some(existing) = module.defs.get(&name) {
            // RFC 012 M4-1: 允许同名类按泛型 arity 重载（C# 风格 arity overloading）。
            // 如 `GenerateToAttribute`（非泛型）与 `GenerateToAttribute<T>`（泛型）可共存。
            // 仅当两者均为 Class 且 generic_arity 不同时跳过重复检查。
            let is_class_arity_overload = matches!(
                (&existing.kind, &kind),
                (DefKind::Class { generic_arity: a }, DefKind::Class { generic_arity: b }) if a != b
            );
            if !is_class_arity_overload {
                return Err(HirError::DuplicateDefinition(name.to_string()));
            }
        }
        let id = DefId(self.next_id);
        self.next_id += 1;
        module.defs.insert(
            name.clone(),
            Def {
                id,
                name,
                kind,
                span,
            },
        );
        Ok(id)
    }

    /// RFC 037：分配 class DefId，支持 partial class 跨文件复用。
    ///
    /// partial class 同名同 arity 在多文件中重复声明时，复用首个声明的 DefId，
    /// 不插入新的 def 条目——多份 ClassDef 由 typeck `partial.rs` 在
    /// `check_module` 阶段合并为单一 ClassDef。
    fn alloc_class_def(
        &mut self,
        name: Ident,
        generic_arity: usize,
        is_partial: bool,
    ) -> Result<DefId, HirError> {
        if is_partial {
            let module = self.modules.last_mut().unwrap();
            if let Some(existing) = module.defs.get(&name) {
                if matches!(
                    existing.kind,
                    DefKind::Class { generic_arity: a } if a == generic_arity
                ) {
                    return Ok(existing.id);
                }
            }
        }
        self.alloc_def(name, DefKind::Class { generic_arity }, Span::DUMMY)
    }

    pub fn lower_program(&mut self, program: &Program) -> Result<HirModule, HirError> {
        for item in &program.items {
            self.lower_item(item)?;
        }
        Ok(self.modules.remove(0))
    }

    fn lower_item(&mut self, item: &Spanned<Item>) -> Result<(), HirError> {
        match &item.node {
            Item::Namespace(ns) => {
                self.push_namespace_path(&ns.path, &ns.capabilities);
                for inner in &ns.items {
                    self.lower_item(inner)?;
                }
                self.pop_namespace_path(&ns.path);
            }
            Item::Use(use_item) => {
                let (alias, kind) = if let Some(alias) = &use_item.alias {
                    (alias.clone(), ImportKind::Alias)
                } else if use_item.path.len() == 1 {
                    (use_item.path[0].clone(), ImportKind::Namespace)
                } else {
                    (
                        use_item
                            .path
                            .last()
                            .cloned()
                            .unwrap_or_else(|| "unknown".into()),
                        ImportKind::Type,
                    )
                };
                self.modules
                    .last_mut()
                    .unwrap()
                    .imports
                    .push(ImportBinding {
                        path: use_item.path.clone(),
                        alias,
                        kind,
                    });
            }
            Item::Struct(s) => {
                let id = self.alloc_def(s.name.clone(), DefKind::Struct, Span::DUMMY)?;
                self.modules
                    .last_mut()
                    .unwrap()
                    .items
                    .push(HirItem::Struct {
                        def: id,
                        def_ast: s.clone(),
                        span: item.span,
                    });
            }
            Item::Class(c) => {
                let id = self.alloc_class_def(c.name.clone(), c.generics.len(), c.is_partial)?;
                self.modules.last_mut().unwrap().items.push(HirItem::Class {
                    def: id,
                    def_ast: c.clone(),
                    span: item.span,
                });
            }
            Item::Interface(i) => {
                let id = self.alloc_def(i.name.clone(), DefKind::Interface, Span::DUMMY)?;
                self.modules
                    .last_mut()
                    .unwrap()
                    .items
                    .push(HirItem::Interface {
                        def: id,
                        def_ast: i.clone(),
                        span: item.span,
                    });
            }
            Item::Enum(e) => {
                let id = self.alloc_def(e.name.clone(), DefKind::Enum, Span::DUMMY)?;
                self.modules.last_mut().unwrap().items.push(HirItem::Enum {
                    def: id,
                    def_ast: e.clone(),
                    span: item.span,
                });
            }
            // RFC 004 M1：variant 标签联合类型 HIR lowering
            Item::Variant(v) => {
                let id = self.alloc_def(v.name.clone(), DefKind::Variant, Span::DUMMY)?;
                self.modules
                    .last_mut()
                    .unwrap()
                    .items
                    .push(HirItem::Variant {
                        def: id,
                        def_ast: v.clone(),
                        span: item.span,
                    });
            }
            // GAP #5：delegate 委托类型 HIR lowering
            Item::Delegate(d) => {
                let id = self.alloc_def(d.name.clone(), DefKind::Delegate, Span::DUMMY)?;
                self.modules
                    .last_mut()
                    .unwrap()
                    .items
                    .push(HirItem::Delegate {
                        def: id,
                        def_ast: d.clone(),
                        span: item.span,
                    });
            }
            Item::Fn(f) => {
                let id = self.alloc_def(
                    f.name.clone(),
                    DefKind::Fn {
                        is_async: f.is_async,
                    },
                    Span::DUMMY,
                )?;
                self.modules.last_mut().unwrap().items.push(HirItem::Fn {
                    def: id,
                    def_ast: f.clone(),
                    span: item.span,
                });
            }
            // .ani 契约文件由管线层（pipeline.rs）直接路由到 typeck/codegen，
            // 不经过 hir lowering。若此处命中 Native，说明管线编排错误。
            Item::Native(_) => unreachable!(
                "native contract (.ani) must not enter hir; route it directly to typeck/codegen"
            ),
        }
        Ok(())
    }

    /// 推入 namespace 路径，并将 `leaf_capabilities` 合并到最内层 HirModule。
    ///
    /// RFC 037：同命名空间多次声明时复用已存在的子模块，使跨文件的 `partial class`
    /// 落到同一 `HirModule.items` 下，让 typeck partial.rs 能按 (ns, name, arity)
    /// 正确分组。
    ///
    /// RFC 016 M3 §3.4 能力 gating Phase 1+：`leaf_capabilities` 仅赋给最内层段。
    /// 跨文件多次声明同一 namespace 时，capabilities 取并集（声明更多能力不破坏
    /// 安全性，最终判定仍由 typeck 在调用点完成）。
    fn push_namespace_path(&mut self, path: &[Ident], leaf_capabilities: &[Ident]) {
        let len = path.len();
        for (i, seg) in path.iter().enumerate() {
            let parent = self.modules.last_mut().unwrap();
            let existing_idx = parent
                .children
                .iter()
                .position(|c| c.name.as_ref() == Some(seg));
            if let Some(idx) = existing_idx {
                let existing = parent.children.remove(idx);
                self.modules.push(existing);
            } else {
                self.modules.push(HirModule {
                    name: Some(seg.clone()),
                    defs: IndexMap::new(),
                    items: vec![],
                    children: vec![],
                    imports: vec![],
                    capabilities: vec![],
                });
            }
            // 最内层段：合并 leaf_capabilities（并集，去重）
            if i == len - 1 && !leaf_capabilities.is_empty() {
                let leaf = self.modules.last_mut().unwrap();
                for cap in leaf_capabilities {
                    if !leaf.capabilities.contains(cap) {
                        leaf.capabilities.push(cap.clone());
                    }
                }
            }
        }
    }

    fn pop_namespace_path(&mut self, path: &[Ident]) {
        for _ in path {
            let child = self.modules.pop().unwrap();
            // push 时若复用已有子模块，已从 parent.children 移除；
            // pop 时直接 push 回 parent.children（不会产生同名重复）。
            self.modules.last_mut().unwrap().children.push(child);
        }
    }

    pub fn resolve_name(&self, name: &Ident) -> Option<&Def> {
        self.modules.last()?.resolve_name(name)
    }
}

impl Default for HirBuilder {
    fn default() -> Self {
        Self::new()
    }
}
