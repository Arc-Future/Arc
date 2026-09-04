//! RFC 006 M2：record `with` 脱糖与 `==`/`!=` 值相等重写。
//! RFC 006 M5+：`with` × 自定义 init 体（clone 字段 + obj_init 调 setter）。

use crate::checker::TypeChecker;
use crate::error::TypeError;
use crate::generics::type_id_to_ast;
use ast::{BinOp, Expr, Ident, IsPattern, Span, Spanned, TypeId, UnaryOp};

impl TypeChecker {
    /// `recv with { F = v, … }` → `new R(recv.F1, …) { Prop = v, … }`
    ///
    /// - 字段 / auto-init 属性：写入构造实参（覆盖或从 receiver 拷贝）
    /// - 自定义 `init`/`set` 属性：写入对象初始化器（M5 setter 路径；M5+）
    /// - clone 可读私有 backing 字段：检查期临时将 `current_class` 升为 record 类型
    pub(crate) fn desugar_record_with(
        &self,
        receiver: &Spanned<Expr>,
        inits: &[(Ident, Spanned<Expr>)],
        recv_ty: &TypeId,
        span: Span,
    ) -> Result<Expr, TypeError> {
        let TypeId::Named(name) = recv_ty else {
            return Err(TypeError::Oop(
                "`with` requires a record receiver type".into(),
            ));
        };
        let nominal = self.registry.types.get(name).ok_or_else(|| {
            TypeError::Oop(format!("unknown type `{name}` for `with` expression"))
        })?;
        if !nominal.is_record {
            return Err(TypeError::Oop(format!(
                "`with` is only valid on record types; `{name}` is not a record (RFC 006)"
            )));
        }
        let fields: Vec<_> = nominal
            .fields
            .values()
            .filter(|f| !f.is_static)
            .cloned()
            .collect();

        let mut field_overrides: indexmap::IndexMap<Ident, Spanned<Expr>> =
            indexmap::IndexMap::new();
        let mut prop_inits: Vec<(Ident, Spanned<Expr>)> = Vec::new();
        for (fname, val) in inits {
            if fields.iter().any(|f| &f.name == fname) {
                field_overrides.insert(fname.clone(), val.clone());
                continue;
            }
            // RFC 006 M5+：自定义 init/set 属性（无 backing field 入 fields）
            let setter: Ident = format!("set_{fname}").into();
            if nominal.methods.contains_key(&setter) {
                prop_inits.push((fname.clone(), val.clone()));
                continue;
            }
            return Err(TypeError::Oop(format!(
                "no field or settable property `{fname}` on record `{name}` in `with` initializer"
            )));
        }

        let arity = fields.len();
        let has_matching_ctor = nominal
            .constructors
            .iter()
            .any(|c| c.param_types.len() == arity);
        if !has_matching_ctor && arity > 0 {
            return Err(TypeError::Oop(format!(
                "record `{name}` has no constructor with {arity} parameter(s) for `with` desugar (RFC 006 M2)"
            )));
        }
        let args: Vec<Spanned<Expr>> = fields
            .iter()
            .map(|f| {
                if let Some(v) = field_overrides.get(&f.name) {
                    v.clone()
                } else {
                    Spanned::new(
                        Expr::Field {
                            receiver: Box::new(receiver.clone()),
                            field: f.name.clone(),
                        },
                        span,
                    )
                }
            })
            .collect();
        let obj_init = if prop_inits.is_empty() {
            None
        } else {
            Some(prop_inits)
        };
        Ok(Expr::New {
            ty: Spanned::new(type_id_to_ast(recv_ty), span),
            args,
            obj_init,
        })
    }

    /// 两侧均为同一 record 时，将 `==`/`!=` 重写为值相等 `Equals` 调用。
    ///
    /// - record（引用类型）：null 安全三元式
    /// - record struct：直接 `a.Equals(b)`（值类型无 null）
    pub(crate) fn desugar_record_equality(
        &self,
        op: BinOp,
        left: &Spanned<Expr>,
        right: &Spanned<Expr>,
        left_ty: &TypeId,
        right_ty: &TypeId,
    ) -> Option<Expr> {
        if !matches!(op, BinOp::Eq | BinOp::NotEq) {
            return None;
        }
        // `x == null` / `null == x`：保留引用空比较（仅 class）
        if matches!(left.node, Expr::Null) || matches!(right.node, Expr::Null) {
            return None;
        }
        let TypeId::Named(lname) = left_ty else {
            return None;
        };
        let TypeId::Named(rname) = right_ty else {
            return None;
        };
        if lname != rname {
            return None;
        }
        let nominal = self.registry.types.get(lname)?;
        if !nominal.is_record {
            return None;
        }
        let span = left.span.merge(right.span);
        let equals_call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(left.clone()),
                method: "Equals".into(),
                args: vec![right.clone()],
                type_args: vec![],
                params_span: None,
            },
            span,
        );
        let eq_expr = if matches!(nominal.kind, crate::oop_types::TypeKind::Struct) {
            equals_call.node
        } else {
            // (a is null) ? (b is null) : ((b is null) ? false : a.Equals(b))
            let a_is_null = Spanned::new(
                Expr::Is {
                    expr: Box::new(left.clone()),
                    pattern: IsPattern::Null,
                },
                span,
            );
            let b_is_null = Spanned::new(
                Expr::Is {
                    expr: Box::new(right.clone()),
                    pattern: IsPattern::Null,
                },
                span,
            );
            let inner = Spanned::new(
                Expr::Ternary {
                    cond: Box::new(b_is_null.clone()),
                    then_branch: Box::new(Spanned::new(Expr::BoolLit(false), span)),
                    else_branch: Box::new(equals_call),
                },
                span,
            );
            Expr::Ternary {
                cond: Box::new(a_is_null),
                then_branch: Box::new(b_is_null),
                else_branch: Box::new(inner),
            }
        };
        Some(match op {
            BinOp::Eq => eq_expr,
            BinOp::NotEq => Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(Spanned::new(eq_expr, span)),
            },
            _ => unreachable!(),
        })
    }
}
