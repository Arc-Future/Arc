//! RFC 017 残余：集合表达式目标类型 `List<T>`。
//! RFC 005 M2b：集合表达式目标类型 `Span<T>` / `ReadOnlySpan<T>` →
//! [`Expr::StackSpanLit`]（与 `params`@Span 同路：`SpanFromStack` / `alloca`，**非**堆数组中转）。
//!
//! 正道仅 `[…]`（禁止 `new List<T>{ }` 双轨）。`List<T>` 脱糖为：
//! `T[] __arr = […]; List<T> __l = new List<T>();` 再按索引 `Add`，
//! 复用既有数组集合表达式路径（含 spread / 嵌套）。

use crate::checker::TypeChecker;
use crate::error::TypeError;
use crate::generics::type_id_to_ast;
use ast::{BinOp, Block, CollectionElement, Expr, Ident, Span, Spanned, Stmt, Type, TypeId};

impl TypeChecker {
    /// 若 `expr` 为集合表达式且 `expected` 为 `List_<T>`，脱糖为数组中转 + `Add` 块；
    /// 否则原样返回。三元分支递归（与 RFC 006 目标类型 `new` 同形）。
    pub(crate) fn apply_collection_list_target(
        &self,
        expr: &Expr,
        expected: &TypeId,
        span: Span,
    ) -> Result<Expr, TypeError> {
        match expr {
            Expr::CollectionExpr { elements } => {
                if !is_list_target(expected) {
                    return Ok(expr.clone());
                }
                let Some(elem_ty) = expected.enumerable_elem() else {
                    return Ok(expr.clone());
                };
                // 临时名序号按编译单元隔离（非进程全局）：并行编译各成员互不干扰。
                let seq = self.next_list_seq();
                Ok(desugar_collection_to_list(
                    elements, expected, &elem_ty, span, seq,
                ))
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                let then_p = self.apply_collection_list_target(
                    &then_branch.node,
                    expected,
                    then_branch.span,
                )?;
                let else_p = self.apply_collection_list_target(
                    &else_branch.node,
                    expected,
                    else_branch.span,
                )?;
                Ok(Expr::Ternary {
                    cond: cond.clone(),
                    then_branch: Box::new(Spanned::new(then_p, then_branch.span)),
                    else_branch: Box::new(Spanned::new(else_p, else_branch.span)),
                })
            }
            Expr::NamedArg { name, expr: inner } => {
                let prepared =
                    self.apply_collection_list_target(&inner.node, expected, inner.span)?;
                Ok(Expr::NamedArg {
                    name: name.clone(),
                    expr: Box::new(Spanned::new(prepared, inner.span)),
                })
            }
            _ => Ok(expr.clone()),
        }
    }

    /// RFC 005 M2b：`Span<T>` / `ReadOnlySpan<T> x = […];` → [`Expr::StackSpanLit`]（栈缓冲）。
    ///
    /// 含 `..spread` 时拒绝（栈 `alloca [N x T]` 需编译期固定 N；请用 `T[]` + AsSpan）。
    pub(crate) fn apply_collection_span_target(
        &self,
        expr: &Expr,
        expected: &TypeId,
        span: Span,
    ) -> Result<Expr, TypeError> {
        match expr {
            Expr::CollectionExpr { elements } => {
                let Some((elem_ty, mutable)) = span_target(expected) else {
                    return Ok(expr.clone());
                };
                desugar_collection_to_span(elements, &elem_ty, mutable, span)
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                let then_p = self.apply_collection_span_target(
                    &then_branch.node,
                    expected,
                    then_branch.span,
                )?;
                let else_p = self.apply_collection_span_target(
                    &else_branch.node,
                    expected,
                    else_branch.span,
                )?;
                Ok(Expr::Ternary {
                    cond: cond.clone(),
                    then_branch: Box::new(Spanned::new(then_p, then_branch.span)),
                    else_branch: Box::new(Spanned::new(else_p, else_branch.span)),
                })
            }
            Expr::NamedArg { name, expr: inner } => {
                let prepared =
                    self.apply_collection_span_target(&inner.node, expected, inner.span)?;
                Ok(Expr::NamedArg {
                    name: name.clone(),
                    expr: Box::new(Spanned::new(prepared, inner.span)),
                })
            }
            _ => Ok(expr.clone()),
        }
    }

    /// RFC 006 目标类型 `new` + RFC 017 `List<T>` / RFC 005 Span 集合目标，同一准备点。
    pub(crate) fn prepare_target_expr(
        &self,
        expr: &Expr,
        expected: &TypeId,
        span: Span,
    ) -> Result<Expr, TypeError> {
        let with_new = self.apply_target_typed_new(expr, expected)?;
        let with_list = self.apply_collection_list_target(&with_new, expected, span)?;
        self.apply_collection_span_target(&with_list, expected, span)
    }

    /// 取下一个 `List<T>` 脱糖临时名序号（内部计数器，按编译单元隔离、确定性递增）。
    fn next_list_seq(&self) -> u32 {
        self.list_target_seq.set(self.list_target_seq.get() + 1);
        self.list_target_seq.get()
    }

    /// RFC 017：`T[] x = [e…]` 按目标元素类型检查（含 int→byte 等数值隐式转换）。
    ///
    /// 与 RFC 005 数组元素不变性并存：不把已推成的 `int[]` 变量赋给 `byte[]`，
    /// 仅对集合表达式本体做目标元素绑定。
    pub(crate) fn try_bind_collection_array_target(
        &mut self,
        expr: &Expr,
        expected: &TypeId,
    ) -> Result<bool, TypeError> {
        let TypeId::Array { elem: target_elem } = expected else {
            return Ok(false);
        };
        let Expr::CollectionExpr { elements } = expr else {
            return Ok(false);
        };
        if elements.is_empty() {
            return Ok(true);
        }
        for item in elements {
            match item {
                CollectionElement::Element(e) => {
                    let t = self.check_expr_at(e.span, &e.node)?;
                    if !self.types_compatible(target_elem, &t.ty) {
                        return Err(TypeError::Mismatch {
                            expected: target_elem.display(),
                            found: t.ty.display(),
                        });
                    }
                }
                CollectionElement::Spread(e) => {
                    let t = self.check_expr_at(e.span, &e.node)?;
                    let spread_elem = match &t.ty {
                        TypeId::Array { elem: inner } => self.canonical_type(inner),
                        other => {
                            return Err(TypeError::Mismatch {
                                expected: format!("{}[] (spread)", target_elem.display()),
                                found: other.display(),
                            });
                        }
                    };
                    let want = self.canonical_type(target_elem);
                    // spread 走数组不变性：禁止 int[] 展开进 byte[]
                    if want != spread_elem
                        && !matches!(want, TypeId::Infer)
                        && !matches!(spread_elem, TypeId::Infer)
                    {
                        return Err(TypeError::Mismatch {
                            expected: format!("{}[]", want.display()),
                            found: t.ty.display(),
                        });
                    }
                }
            }
        }
        Ok(true)
    }
}

fn is_list_target(ty: &TypeId) -> bool {
    match ty {
        TypeId::Named(n) => n.starts_with("List_"),
        TypeId::Nullable { inner } => is_list_target(inner),
        _ => false,
    }
}

fn span_target(ty: &TypeId) -> Option<(TypeId, bool)> {
    match ty {
        TypeId::Span { elem, mutable } => Some((elem.as_ref().clone(), *mutable)),
        TypeId::Nullable { inner } => span_target(inner),
        _ => None,
    }
}

/// 实参树是否含集合表达式（驱动 MethodCall 走绑定以便 `List<T>` 目标脱糖）。
pub(crate) fn contains_collection_expr(expr: &Expr) -> bool {
    match expr {
        Expr::CollectionExpr { .. } => true,
        Expr::NamedArg { expr: inner, .. } => contains_collection_expr(&inner.node),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => {
            contains_collection_expr(&then_branch.node)
                || contains_collection_expr(&else_branch.node)
        }
        _ => false,
    }
}

/// RFC 005 M2b：`[e…]` 目标 Span/ROS → 与 `params`@Span 同构的 [`Expr::StackSpanLit`]。
fn desugar_collection_to_span(
    elements: &[CollectionElement],
    elem_ty: &TypeId,
    mutable: bool,
    _span: Span,
) -> Result<Expr, TypeError> {
    if elements.iter().any(CollectionElement::is_spread) {
        return Err(TypeError::Oop(
            "collection expression targeting `Span`/`ReadOnlySpan` cannot use `..spread` \
             (stack buffer needs a fixed element count at compile time; \
             use `T[]` + AsSpan instead)"
                .into(),
        ));
    }
    let exprs: Vec<Spanned<Expr>> = elements
        .iter()
        .map(|el| match el {
            CollectionElement::Element(e) => e.clone(),
            CollectionElement::Spread(_) => unreachable!("spread rejected above"),
        })
        .collect();
    Ok(Expr::StackSpanLit {
        elements: exprs,
        mutable,
        elem: elem_ty.clone(),
    })
}

fn desugar_collection_to_list(
    elements: &[CollectionElement],
    _list_ty: &TypeId,
    elem_ty: &TypeId,
    span: Span,
    seq: u32,
) -> Expr {
    let arr_name: Ident = format!("__collexpr_arr_{}_{}", span.start, seq).into();
    let list_name: Ident = format!("__collexpr_list_{}_{}", span.start, seq).into();
    let idx_name: Ident = format!("__collexpr_i_{}_{}", span.start, seq).into();

    let arr_ty = TypeId::Array {
        elem: Box::new(elem_ty.clone()),
    };
    let arr_ty_ast = type_id_to_ast(&arr_ty);
    // 必须写 `List<T>`（非 mangled `List_int`），否则仅出现在实参目标上下文时
    // 尚未注册单态类，会报 `undefined type List_int`。
    let list_ty_ast = Type::Named {
        path: vec!["List".into()],
        generics: vec![Spanned::new(type_id_to_ast(elem_ty), span)],
    };

    let mut stmts = Vec::new();

    // `T[] __arr = […];` — 保留原 CollectionExpr，走既有数组路径。
    stmts.push(Spanned::new(
        Stmt::Let {
            mutable: false,
            name: arr_name.clone(),
            ty: Some(Spanned::new(arr_ty_ast, span)),
            init: Some(Spanned::new(
                Expr::CollectionExpr {
                    elements: elements.to_vec(),
                },
                span,
            )),
        },
        span,
    ));

    // `List<T> __l = new List<T>();`
    stmts.push(Spanned::new(
        Stmt::Let {
            mutable: false,
            name: list_name.clone(),
            ty: Some(Spanned::new(list_ty_ast.clone(), span)),
            init: Some(Spanned::new(
                Expr::New {
                    ty: Spanned::new(list_ty_ast, span),
                    args: vec![],
                    obj_init: None,
                },
                span,
            )),
        },
        span,
    ));

    // `int __i = 0;`
    stmts.push(Spanned::new(
        Stmt::Let {
            mutable: true,
            name: idx_name.clone(),
            ty: Some(Spanned::new(
                Type::Named {
                    path: vec!["int".into()],
                    generics: vec![],
                },
                span,
            )),
            init: Some(Spanned::new(Expr::IntLit(0), span)),
        },
        span,
    ));

    // `while (__i < __arr.Length) { __l.Add(__arr[__i]); __i = __i + 1; }`
    let arr_ident = Spanned::new(Expr::Ident(arr_name.clone()), span);
    let idx_ident = Spanned::new(Expr::Ident(idx_name.clone()), span);
    let list_ident = Spanned::new(Expr::Ident(list_name.clone()), span);

    let length_field = Spanned::new(
        Expr::Field {
            receiver: Box::new(arr_ident.clone()),
            field: "Length".into(),
        },
        span,
    );
    let cond = Spanned::new(
        Expr::Binary {
            op: BinOp::Lt,
            left: Box::new(idx_ident.clone()),
            right: Box::new(length_field),
        },
        span,
    );

    let index_get = Spanned::new(
        Expr::Index {
            receiver: Box::new(arr_ident),
            index: Box::new(idx_ident.clone()),
        },
        span,
    );
    let add_call = Spanned::new(
        Expr::MethodCall {
            receiver: Box::new(list_ident),
            method: "Add".into(),
            args: vec![index_get],
            type_args: vec![],
            params_span: None,
        },
        span,
    );
    let incr = Spanned::new(
        Stmt::Assign {
            target: Spanned::new(Expr::Ident(idx_name.clone()), span),
            value: Spanned::new(
                Expr::Binary {
                    op: BinOp::Add,
                    left: Box::new(Spanned::new(Expr::Ident(idx_name), span)),
                    right: Box::new(Spanned::new(Expr::IntLit(1), span)),
                },
                span,
            ),
        },
        span,
    );

    stmts.push(Spanned::new(
        Stmt::While {
            cond,
            body: Block {
                stmts: vec![Spanned::new(Stmt::Expr(add_call), span), incr],
                tail: None,
            },
        },
        span,
    ));

    Expr::Block(Block {
        stmts,
        tail: Some(Box::new(Spanned::new(Expr::Ident(list_name), span))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_list_target_only() {
        assert!(is_list_target(&TypeId::Named("List_int".into())));
        assert!(is_list_target(&TypeId::Nullable {
            inner: Box::new(TypeId::Named("List_string".into())),
        }));
        assert!(!is_list_target(&TypeId::Array {
            elem: Box::new(TypeId::Int),
        }));
        assert!(!is_list_target(&TypeId::IEnumerable {
            inner: Box::new(TypeId::Int),
        }));
        assert!(!is_list_target(&TypeId::Named("HashSet_int".into())));
    }

    #[test]
    fn detects_span_target() {
        assert_eq!(
            span_target(&TypeId::Span {
                elem: Box::new(TypeId::Int),
                mutable: true,
            }),
            Some((TypeId::Int, true))
        );
        assert_eq!(
            span_target(&TypeId::Span {
                elem: Box::new(TypeId::Byte),
                mutable: false,
            }),
            Some((TypeId::Byte, false))
        );
        assert!(span_target(&TypeId::Array {
            elem: Box::new(TypeId::Int),
        })
        .is_none());
    }

    #[test]
    fn desugars_collection_to_stack_span_lit() {
        let e = desugar_collection_to_span(
            &[
                CollectionElement::Element(Spanned::new(Expr::IntLit(1), Span::DUMMY)),
                CollectionElement::Element(Spanned::new(Expr::IntLit(2), Span::DUMMY)),
            ],
            &TypeId::Int,
            true,
            Span::DUMMY,
        )
        .unwrap();
        match e {
            Expr::StackSpanLit {
                elements,
                mutable,
                elem,
            } => {
                assert!(mutable);
                assert_eq!(elem, TypeId::Int);
                assert_eq!(elements.len(), 2);
            }
            other => panic!("expected StackSpanLit, got {other:?}"),
        }
    }

    #[test]
    fn desugars_empty_collection_to_stack_span() {
        let e = desugar_collection_to_span(&[], &TypeId::Int, false, Span::DUMMY).unwrap();
        match e {
            Expr::StackSpanLit {
                elements,
                mutable,
                elem,
            } => {
                assert!(!mutable);
                assert_eq!(elem, TypeId::Int);
                assert!(elements.is_empty());
            }
            other => panic!("expected StackSpanLit, got {other:?}"),
        }
    }

    #[test]
    fn spread_to_span_rejected() {
        let err = desugar_collection_to_span(
            &[CollectionElement::Spread(Spanned::new(
                Expr::Ident("xs".into()),
                Span::DUMMY,
            ))],
            &TypeId::Int,
            true,
            Span::DUMMY,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("spread") || msg.contains("Span"),
            "unexpected error: {msg}"
        );
    }
}
