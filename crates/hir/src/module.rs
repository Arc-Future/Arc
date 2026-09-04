//! HIR module and symbol definitions.

use ast::*;
use indexmap::IndexMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DefId(pub u32);

#[derive(Clone, Debug)]
pub enum DefKind {
    Struct,
    Class {
        /// RFC 012 M4-1: 泛型 arity（0 表示非泛型类）。
        /// 用于允许同名类按泛型 arity 重载（C# 风格 arity overloading），
        /// 如 `GenerateToAttribute`（非泛型）与 `GenerateToAttribute<T>`（泛型）共存。
        generic_arity: usize,
    },
    Interface,
    Enum,
    /// RFC 004 M1：variant 标签联合类型（tagged union）。
    Variant,
    /// GAP #5：delegate 委托类型。
    Delegate,
    Fn {
        is_async: bool,
    },
    Method {
        is_async: bool,
    },
    Field,
    Param,
    Local,
}

#[derive(Clone, Debug)]
pub struct Def {
    pub id: DefId,
    pub name: Ident,
    pub kind: DefKind,
    pub span: Span,
}

/// How a `using` directive binds symbols into scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportKind {
    /// `using N.T;` — single type (or file) import; alias defaults to `T`.
    Type,
    /// `using N;` — import all public members of namespace `N`.
    Namespace,
    /// `using Alias = N.T;` or `using Alias = N;`.
    Alias,
}

/// A `using` import resolved during HIR lowering.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportBinding {
    pub path: Vec<Ident>,
    pub alias: Ident,
    pub kind: ImportKind,
}

#[derive(Clone, Debug)]
pub struct HirModule {
    pub name: Option<Ident>,
    pub defs: IndexMap<Ident, Def>,
    pub items: Vec<HirItem>,
    pub children: Vec<HirModule>,
    pub imports: Vec<ImportBinding>,
    /// RFC 016 M3 §3.4 能力 gating Phase 1+（[4.4 能力系统]）：
    /// namespace 声明的能力集（`namespace X capability io, db { ... }`）。
    /// 仅最内层 namespace 段携带；外层段为空。typeck 沿 namespace 栈
    /// 累积父层 capabilities 形成有效能力集。多次声明同一 namespace
    ///（跨文件）时按并集合并。
    pub capabilities: Vec<Ident>,
}

#[derive(Clone, Debug)]
pub enum HirItem {
    Struct {
        def: DefId,
        def_ast: StructDef,
        /// RFC 017 M4-link Phase B §D2.1：AST 声明 span，用于 codegen 来源过滤
        /// （区分用户源码类型与 std/native/外部 .ao 符号）。
        span: Span,
    },
    Class {
        def: DefId,
        def_ast: ClassDef,
        span: Span,
    },
    Interface {
        def: DefId,
        def_ast: InterfaceDef,
        span: Span,
    },
    Enum {
        def: DefId,
        def_ast: EnumDef,
        span: Span,
    },
    /// RFC 004 M1：variant 标签联合类型。
    Variant {
        def: DefId,
        def_ast: VariantDef,
        span: Span,
    },
    /// GAP #5：delegate 委托类型。
    Delegate {
        def: DefId,
        def_ast: DelegateDef,
        span: Span,
    },
    Fn {
        def: DefId,
        def_ast: FnDef,
        span: Span,
    },
}

impl HirModule {
    pub fn resolve_name(&self, name: &Ident) -> Option<&Def> {
        self.defs.get(name).or_else(|| {
            self.imports
                .iter()
                .find(|i| &i.alias == name)
                .and_then(|i| self.defs.get(&i.alias))
        })
    }

    pub fn resolve_qualified(&self, path: &[Ident]) -> Option<&Def> {
        if path.is_empty() {
            return None;
        }
        if path.len() == 1 {
            return self.resolve_name(&path[0]);
        }
        if let Some(child) = self
            .children
            .iter()
            .find(|c| c.name.as_ref() == Some(&path[0]))
        {
            return child.resolve_qualified(&path[1..]);
        }
        let name = path.last()?;
        self.defs.get(name)
    }

    pub fn walk_items(&self, f: &mut dyn FnMut(&HirItem)) {
        for item in &self.items {
            f(item);
        }
        for child in &self.children {
            child.walk_items(f);
        }
    }
}
