use crate::{Ident, Span, Spanned};

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named {
        path: Vec<Ident>,
        generics: Vec<Spanned<Type>>,
    },
    Ref {
        inner: Box<Spanned<Type>>,
        mutable: bool,
    },
    Func {
        params: Vec<Spanned<Type>>,
        ret: Box<Spanned<Type>>,
    },
    Array {
        inner: Box<Spanned<Type>>,
    },
    /// `T?` — nullable reference type (compile-time annotation).
    Nullable {
        inner: Box<Spanned<Type>>,
    },
    /// Compile-time integer literal as a generic argument (const generics).
    /// Only valid in `Type::Named.generics` for built-in facades like `Vector<T, N>`.
    ConstInt(i64),
    Infer,
}

impl Type {
    pub fn named(name: impl Into<Ident>) -> Spanned<Type> {
        Spanned::new(
            Type::Named {
                path: vec![name.into()],
                generics: vec![],
            },
            Span::DUMMY,
        )
    }

    pub fn is_task(&self) -> bool {
        matches!(
            self,
            Type::Named { path, .. } if path.first().map(|s| s.as_str()) == Some("Task")
        )
    }

    pub fn is_iqueryable(&self) -> bool {
        matches!(
            self,
            Type::Named { path, .. } if path.first().map(|s| s.as_str()) == Some("IQueryable")
        )
    }

    pub fn is_ienumerable(&self) -> bool {
        matches!(
            self,
            Type::Named { path, .. } if path.first().map(|s| s.as_str()) == Some("IEnumerable")
        )
    }
}
