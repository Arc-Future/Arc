//! RFC 003：用户运算符重载 — typeck 将中缀/一元脱糖为 `op_*` 静态调用。

use crate::checker::TypeChecker;
use crate::type_id::TypeId;
use ast::{BinOp, Expr, Ident, MethodModifier, Spanned, UnaryOp};

impl TypeChecker {
    /// 若操作数类型声明了对应 `op_*` 静态方法，返回脱糖后的调用表达式。
    pub(crate) fn desugar_user_binary_operator(
        &self,
        op: BinOp,
        left: &Spanned<Expr>,
        right: &Spanned<Expr>,
        left_ty: &TypeId,
        right_ty: &TypeId,
    ) -> Option<Expr> {
        let method = match op {
            BinOp::Add => "op_Addition",
            BinOp::Sub => "op_Subtraction",
            BinOp::Mul => "op_Multiply",
            BinOp::Div => "op_Division",
            BinOp::Mod => "op_Modulus",
            BinOp::Eq => "op_Equality",
            BinOp::NotEq => "op_Inequality",
            _ => return None,
        };
        let left_canon = self.canonical_type(left_ty);
        let right_canon = self.canonical_type(right_ty);
        // 内置数值 / string 比较与拼接保持原路径；仅 Named（及一侧 Named）走重载。
        if is_builtin_binary_path(op, &left_canon, &right_canon) {
            return None;
        }
        let span = left.span.merge(right.span);
        if let Some(type_name) =
            self.find_operator_declaring_type(method, &left_canon, &right_canon)
        {
            return Some(static_op_call(
                &type_name,
                method,
                vec![left.clone(), right.clone()],
                span,
            ));
        }
        // `!=`：若无 `op_Inequality` 但有 `op_Equality`，脱糖为 `!(a == b)`。
        if matches!(op, BinOp::NotEq) {
            if let Some(type_name) =
                self.find_operator_declaring_type("op_Equality", &left_canon, &right_canon)
            {
                let eq = Spanned::new(
                    static_op_call(
                        &type_name,
                        "op_Equality",
                        vec![left.clone(), right.clone()],
                        span,
                    ),
                    span,
                );
                return Some(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(eq),
                });
            }
        }
        None
    }

    /// 一元 `-` → `op_UnaryNegation`。
    pub(crate) fn desugar_user_unary_neg(
        &self,
        expr: &Spanned<Expr>,
        inner_ty: &TypeId,
    ) -> Option<Expr> {
        let canon = self.canonical_type(inner_ty);
        if is_numeric_typeid(&canon) {
            return None;
        }
        let TypeId::Named(name) = &canon else {
            return None;
        };
        if !self.has_static_op(name, "op_UnaryNegation") {
            return None;
        }
        Some(static_op_call(
            name,
            "op_UnaryNegation",
            vec![expr.clone()],
            expr.span,
        ))
    }

    fn find_operator_declaring_type(
        &self,
        method: &str,
        left: &TypeId,
        right: &TypeId,
    ) -> Option<Ident> {
        for ty in [left, right] {
            if let TypeId::Named(name) = ty {
                if self.has_static_op(name, method) {
                    return Some(name.clone());
                }
            }
        }
        None
    }

    fn has_static_op(&self, type_name: &Ident, method: &str) -> bool {
        let Some(nominal) = self.registry.types.get(type_name) else {
            return false;
        };
        let method_id: Ident = method.into();
        nominal
            .methods
            .get(&method_id)
            .map(|sigs| sigs.iter().any(|s| s.modifier == MethodModifier::Static))
            .unwrap_or(false)
    }
}

fn static_op_call(
    type_name: &Ident,
    method: &str,
    args: Vec<Spanned<Expr>>,
    span: ast::Span,
) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(Spanned::new(Expr::Ident(type_name.clone()), span)),
        method: method.into(),
        args,
        type_args: vec![],
        params_span: None,
    }
}

fn is_numeric_typeid(ty: &TypeId) -> bool {
    matches!(
        ty,
        TypeId::Int
            | TypeId::Long
            | TypeId::Short
            | TypeId::Byte
            | TypeId::Char
            | TypeId::Float
            | TypeId::Double
    )
}

fn is_builtin_binary_path(op: BinOp, left: &TypeId, right: &TypeId) -> bool {
    match op {
        BinOp::Add if *left == TypeId::String || *right == TypeId::String => true,
        BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Mod
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::BitXor
        | BinOp::Shl
        | BinOp::Shr => is_numeric_typeid(left) && is_numeric_typeid(right),
        BinOp::Eq | BinOp::NotEq => {
            // string / null / 纯数值比较走内置；Named 交给 op_*
            (*left == TypeId::String || *right == TypeId::String)
                || (is_numeric_typeid(left) && is_numeric_typeid(right))
                || matches!(left, TypeId::Bool) && matches!(right, TypeId::Bool)
        }
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or => true,
    }
}
