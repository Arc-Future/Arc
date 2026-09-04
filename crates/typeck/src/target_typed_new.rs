//! RFC 006：目标类型 `new()` —— 在已知期望类型处将 `Type::Infer` 填入构造类型。

use crate::checker::TypeChecker;
use crate::error::TypeError;
use crate::generics::type_id_to_ast;
use crate::type_id::TypeId;
use ast::{CollectionElement, Expr, Spanned, Type};

impl TypeChecker {
    /// 若 `expr`（或其目标上下文子表达式）含目标类型 `new(...)`（`ty == Infer`），
    /// 用 `expected` 填类型；否则原样返回。
    ///
    /// M1：顶层 `New`。M2：集合元素 / 三元分支递归；调用实参由
    /// `bind_args_to_slots` / Call·MethodCall / `Expr::New` 实参路径传入期望类型。
    ///
    /// `T?` 期望取内层 `T`。无可用目标类型时返回硬错误（禁止静默 Infer）。
    pub(crate) fn apply_target_typed_new(
        &self,
        expr: &Expr,
        expected: &TypeId,
    ) -> Result<Expr, TypeError> {
        match expr {
            Expr::New { ty, args, obj_init } if matches!(ty.node, Type::Infer) => {
                let target = unwrap_nullable_target(expected);
                if !is_constructible_target(target) {
                    return Err(TypeError::Oop(
                        "target-typed `new()` requires a concrete type context \
                         (e.g. `T x = new(...)`; `var x = new()` is not allowed)"
                            .into(),
                    ));
                }
                Ok(Expr::New {
                    ty: Spanned::new(type_id_to_ast(target), ty.span),
                    args: args.clone(),
                    obj_init: obj_init.clone(),
                })
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                let then_p = self.apply_target_typed_new(&then_branch.node, expected)?;
                let else_p = self.apply_target_typed_new(&else_branch.node, expected)?;
                Ok(Expr::Ternary {
                    cond: cond.clone(),
                    then_branch: Box::new(Spanned::new(then_p, then_branch.span)),
                    else_branch: Box::new(Spanned::new(else_p, else_branch.span)),
                })
            }
            Expr::CollectionExpr { elements } => {
                let Some(elem_ty) = expected.enumerable_elem() else {
                    return Ok(expr.clone());
                };
                let mut out = Vec::with_capacity(elements.len());
                for item in elements {
                    match item {
                        CollectionElement::Element(e) => {
                            let prepared = self.apply_target_typed_new(&e.node, &elem_ty)?;
                            out.push(CollectionElement::Element(Spanned::new(prepared, e.span)));
                        }
                        CollectionElement::Spread(e) => {
                            let spread_exp = TypeId::Array {
                                elem: Box::new(elem_ty.clone()),
                            };
                            let prepared = self.apply_target_typed_new(&e.node, &spread_exp)?;
                            out.push(CollectionElement::Spread(Spanned::new(prepared, e.span)));
                        }
                    }
                }
                Ok(Expr::CollectionExpr { elements: out })
            }
            Expr::NamedArg { name, expr: inner } => {
                let prepared = self.apply_target_typed_new(&inner.node, expected)?;
                Ok(Expr::NamedArg {
                    name: name.clone(),
                    expr: Box::new(Spanned::new(prepared, inner.span)),
                })
            }
            _ => Ok(expr.clone()),
        }
    }
}

/// 实参树是否含尚未填类型的目标类型 `new(...)`（驱动 Call/MethodCall 走绑定填类型）。
pub(crate) fn contains_target_typed_new(expr: &Expr) -> bool {
    match expr {
        Expr::New { ty, .. } if matches!(ty.node, Type::Infer) => true,
        Expr::NamedArg { expr: inner, .. } => contains_target_typed_new(&inner.node),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => {
            contains_target_typed_new(&then_branch.node)
                || contains_target_typed_new(&else_branch.node)
        }
        Expr::CollectionExpr { elements } => elements.iter().any(|item| match item {
            CollectionElement::Element(e) | CollectionElement::Spread(e) => {
                contains_target_typed_new(&e.node)
            }
        }),
        _ => false,
    }
}

fn unwrap_nullable_target(ty: &TypeId) -> &TypeId {
    match ty {
        TypeId::Nullable { inner } => inner.as_ref(),
        other => other,
    }
}

fn is_constructible_target(ty: &TypeId) -> bool {
    !matches!(
        ty,
        TypeId::Infer | TypeId::Void | TypeId::Error | TypeId::Func { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{Ident, Span, Type};

    #[test]
    fn unwrap_nullable() {
        let inner = TypeId::Named("Point".into());
        let n = TypeId::Nullable {
            inner: Box::new(inner.clone()),
        };
        assert_eq!(unwrap_nullable_target(&n), &inner);
        assert_eq!(unwrap_nullable_target(&inner), &inner);
    }

    #[test]
    fn constructible_named() {
        assert!(is_constructible_target(&TypeId::Named(Ident::from(
            "Point"
        ))));
        assert!(!is_constructible_target(&TypeId::Infer));
        assert!(!is_constructible_target(&TypeId::Void));
    }

    #[test]
    fn detects_infer_new_in_nested() {
        let infer_new = Expr::New {
            ty: Spanned::new(Type::Infer, Span::DUMMY),
            args: vec![],
            obj_init: None,
        };
        assert!(contains_target_typed_new(&infer_new));
        assert!(contains_target_typed_new(&Expr::NamedArg {
            name: Ident::from("p"),
            expr: Box::new(Spanned::new(infer_new.clone(), Span::DUMMY)),
        }));
        assert!(!contains_target_typed_new(&Expr::IntLit(1)));
    }
}
