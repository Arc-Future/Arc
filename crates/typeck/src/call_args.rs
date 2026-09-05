//! RFC 007：可选参数默认值折叠 + 命名/位置实参绑定脱糖。

use crate::error::TypeError;
use crate::oop_types::ConstValue;
use crate::type_id::TypeId;
use ast::*;

/// 将形参默认值表达式折叠为常量（字面量 / `default(T)`）。
///
/// `Type.Const` 引用需经 [`fold_param_default_lookup`] 解析。
pub fn fold_param_default(expr: &Expr) -> Option<ConstValue> {
    fold_param_default_lookup(expr, &|_, _| None)
}

/// 折叠形参默认值；`lookup(type_name, field)` 解析 `Type.ConstField`。
pub fn fold_param_default_lookup(
    expr: &Expr,
    lookup: &dyn Fn(&str, &str) -> Option<ConstValue>,
) -> Option<ConstValue> {
    match expr {
        Expr::IntLit(n) => Some(ConstValue::Int(*n)),
        Expr::FloatLit(ast::FloatLitValue::Double(f)) => Some(ConstValue::Float(*f)),
        Expr::FloatLit(ast::FloatLitValue::Float(f)) => Some(ConstValue::Float(*f as f64)),
        Expr::BoolLit(b) => Some(ConstValue::Bool(*b)),
        Expr::StringLit(s) => Some(ConstValue::String(s.clone())),
        Expr::Null => Some(ConstValue::Null),
        Expr::CharLit(c) => Some(ConstValue::Int(*c as i64)),
        Expr::Unary {
            op: UnaryOp::Neg,
            expr,
        } => match fold_param_default_lookup(&expr.node, lookup)? {
            ConstValue::Int(n) => Some(ConstValue::Int(-n)),
            ConstValue::Float(f) => Some(ConstValue::Float(-f)),
            _ => None,
        },
        // RFC 007 M2b：`default(T)` — 可折叠为标量 / null 的类型。
        Expr::Default { ty } => fold_default_type(&ty.node),
        // RFC 007 M2b：`Type.ConstField` 编译期常量引用。
        Expr::Field { receiver, field } => {
            let Expr::Ident(type_name) = &receiver.node else {
                return None;
            };
            lookup(type_name.as_str(), field.as_str())
        }
        _ => None,
    }
}

/// `default(T)` / `default` → 对应零值常量。
///
/// Arc 全引用类型语义：非原始命名类型 → `null`；原始类型 → 零值；
/// `Type::Infer`（裸 `default`）→ `null`（覆盖 CancellationToken/Task 等引用类型主场景）。
fn fold_default_type(ty: &Type) -> Option<ConstValue> {
    match ty {
        Type::Named { path, generics } if generics.is_empty() && path.len() == 1 => {
            match path[0].as_str() {
                "int" | "long" | "short" | "byte" | "char" | "uint" | "ulong" | "ushort"
                | "sbyte" => Some(ConstValue::Int(0)),
                "float" | "double" => Some(ConstValue::Float(0.0)),
                "bool" => Some(ConstValue::Bool(false)),
                // string/object 及所有用户定义类型、编译器 stub（CancellationToken 等）——
                // Arc 全引用类型语义，default = null
                _ => Some(ConstValue::Null),
            }
        }
        // 多段命名类型 / 泛型命名类型 — 均为引用类型，default = null
        Type::Named { .. } => Some(ConstValue::Null),
        Type::Nullable { .. } => Some(ConstValue::Null),
        Type::Array { .. } => Some(ConstValue::Null),
        Type::Ref { .. } => Some(ConstValue::Null),
        Type::Func { .. } => Some(ConstValue::Null),
        // 裸 `default`（无显式类型）— 推断为 null（覆盖引用类型主场景）
        Type::Infer => Some(ConstValue::Null),
        Type::ConstInt(_) => None,
    }
}

pub fn const_to_expr(c: &ConstValue) -> Expr {
    match c {
        ConstValue::Int(n) => Expr::IntLit(*n),
        ConstValue::Float(f) => Expr::FloatLit(FloatLitValue::Double(*f)),
        ConstValue::Bool(b) => Expr::BoolLit(*b),
        ConstValue::String(s) => Expr::StringLit(s.clone()),
        ConstValue::Null => Expr::Null,
    }
}

/// 校验形参列表的可选后缀规则与默认值合法性（声明点）。
///
/// `lookup(type_name, field)` 解析 `Type.Const`；无 const 引用时传 `|_, _| None`。
pub fn validate_param_defaults_lookup(
    params: &[Param],
    lookup: &dyn Fn(&str, &str) -> Option<ConstValue>,
) -> Result<(), TypeError> {
    let mut seen_default = false;
    for p in params {
        if let Some(default) = p.default.as_ref() {
            if p.is_ref || p.is_out || p.is_in {
                return Err(TypeError::Oop(
                    "ref/out/in parameters cannot have default values".into(),
                ));
            }
            if p.is_extension_receiver {
                return Err(TypeError::Oop(
                    "extension `this` parameter cannot have a default value".into(),
                ));
            }
            if fold_param_default_lookup(&default.node, lookup).is_none() {
                return Err(TypeError::Oop(format!(
                    "parameter `{}` default must be a compile-time constant \
                     (literal, default(T), or Type.Const; RFC 007 M2b)",
                    p.name
                )));
            }
            seen_default = true;
        } else if seen_default {
            return Err(TypeError::Oop(format!(
                "parameter `{}` must have a default value (optional parameters are a suffix)",
                p.name
            )));
        }
    }
    Ok(())
}

/// 形参槽：名称、类型、默认值。
#[derive(Clone, Debug)]
pub struct ParamSlot {
    pub name: Ident,
    pub ty: TypeId,
    pub default: Option<ConstValue>,
    /// RFC 005：末位 `params Span`/`ReadOnlySpan`。
    pub is_params: bool,
}

/// 绑定调用实参到形参槽，返回完整位置实参列表（已填默认值）及各实参类型。
///
/// **不**做赋值兼容检查——由调用方用 `types_compatible` 复核（含接口实现 /
/// variance）。本函数只负责位置/命名绑定与默认值填充。
pub fn bind_call_args<F>(
    params: &[ParamSlot],
    args: &[Spanned<Expr>],
    mut check_value: F,
) -> Result<(Vec<Spanned<Expr>>, Vec<TypeId>), TypeError>
where
    F: FnMut(&Expr, &TypeId) -> Result<(TypeId, Expr), TypeError>,
{
    let mut positional: Vec<(Spanned<Expr>, TypeId, Expr)> = Vec::new();
    let mut named: Vec<(Ident, Spanned<Expr>, TypeId, Expr)> = Vec::new();
    let mut seen_named = false;
    for a in args {
        match &a.node {
            Expr::NamedArg { name, expr } => {
                seen_named = true;
                // 命名实参的槽类型在下方对齐后才知道——此处以 Infer 期望走
                // 独立检查（目标化分支不触发）。
                let (ty, rewritten) = check_value(&expr.node, &TypeId::Infer)?;
                named.push((
                    name.clone(),
                    Spanned::new(rewritten.clone(), expr.span),
                    ty,
                    rewritten,
                ));
            }
            _ => {
                if seen_named {
                    return Err(TypeError::Oop(
                        "positional argument cannot follow named argument".into(),
                    ));
                }
                let idx = positional.len();
                let expected = params
                    .get(idx)
                    .map(|p| p.ty.clone())
                    .unwrap_or(TypeId::Infer);
                let (ty, rewritten) = check_value(&a.node, &expected)?;
                positional.push((Spanned::new(rewritten.clone(), a.span), ty, rewritten));
            }
        }
    }

    let n = params.len();
    let mut slots: Vec<Option<(Spanned<Expr>, TypeId)>> = vec![None; n];

    if positional.len() > n {
        return Err(TypeError::Mismatch {
            expected: format!("at most {n} arguments"),
            found: format!("{} arguments", positional.len()),
        });
    }
    for (i, (arg, ty, _)) in positional.into_iter().enumerate() {
        slots[i] = Some((arg, ty));
    }

    for (name, arg, ty, _) in named {
        let idx = params
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| TypeError::Oop(format!("unknown named argument `{name}`")))?;
        if slots[idx].is_some() {
            return Err(TypeError::Oop(format!(
                "argument `{name}` specified multiple times"
            )));
        }
        slots[idx] = Some((arg, ty));
    }

    let mut out = Vec::with_capacity(n);
    let mut out_tys = Vec::with_capacity(n);
    for (i, slot) in slots.into_iter().enumerate() {
        match slot {
            Some((e, ty)) => {
                out.push(e);
                out_tys.push(ty);
            }
            None => {
                let def = params[i]
                    .default
                    .as_ref()
                    .ok_or_else(|| TypeError::Mismatch {
                        expected: format!("argument for parameter `{}`", params[i].name),
                        found: "missing".into(),
                    })?;
                out.push(Spanned::new(const_to_expr(def), Span::DUMMY));
                out_tys.push(params[i].ty.clone());
            }
        }
    }
    Ok((out, out_tys))
}

fn slot_type_ok(expected: &TypeId, found: &TypeId) -> bool {
    if expected == found {
        return true;
    }
    // 默认值折叠路径的粗兼容（无 TypeChecker）；完整赋值规则见 `types_compatible`。
    match (expected, found) {
        (TypeId::String, TypeId::Nullable { .. }) => true,
        (TypeId::Nullable { inner }, other) if inner.as_ref() == other => true,
        (TypeId::Object, _) => !matches!(found, TypeId::Void),
        _ => expected.display() == found.display(),
    }
}

fn const_type_id(c: &ConstValue) -> TypeId {
    match c {
        ConstValue::Int(_) => TypeId::Int,
        ConstValue::Float(_) => TypeId::Double,
        ConstValue::Bool(_) => TypeId::Bool,
        ConstValue::String(_) => TypeId::String,
        ConstValue::Null => TypeId::Nullable {
            inner: Box::new(TypeId::Infer),
        },
    }
}

/// 校验已折叠默认值与形参类型粗兼容（声明点辅助）。
pub fn check_default_type(param_ty: &TypeId, default: &ConstValue) -> Result<(), TypeError> {
    let dty = const_type_id(default);
    let null_ok = matches!(default, ConstValue::Null)
        && matches!(
            param_ty,
            TypeId::String | TypeId::Named(_) | TypeId::Object | TypeId::Nullable { .. }
        );
    if slot_type_ok(param_ty, &dty) || null_ok {
        Ok(())
    } else {
        Err(TypeError::Mismatch {
            expected: param_ty.display(),
            found: dty.display(),
        })
    }
}

/// RFC 007 M2c：lambda 形参可选后缀 + 默认值常量性（声明点）。
pub fn validate_lambda_param_defaults(params: &[LambdaParam]) -> Result<(), TypeError> {
    let mut seen_default = false;
    for p in params {
        if let Some(expr) = p.default.as_ref() {
            if fold_param_default(&expr.node).is_none() {
                return Err(TypeError::Oop(format!(
                    "lambda parameter `{}` default must be a compile-time constant (RFC 007 M2c)",
                    p.name
                )));
            }
            seen_default = true;
        } else if seen_default {
            return Err(TypeError::Oop(format!(
                "lambda parameter `{}` must have a default value (optional parameters are a suffix)",
                p.name
            )));
        }
    }
    Ok(())
}

/// 从 lambda 形参构建 `ParamSlot`（IIFE 调用点绑定用）。
///
/// 无显式类型时，若有默认值则从常量折叠类型；否则报错（无法推断）。
pub fn lambda_param_slots(
    params: &[LambdaParam],
    lower_ty: &mut dyn FnMut(&Type) -> Result<TypeId, TypeError>,
) -> Result<Vec<ParamSlot>, TypeError> {
    validate_lambda_param_defaults(params)?;
    let mut out = Vec::with_capacity(params.len());
    for p in params {
        let default = p
            .default
            .as_ref()
            .map(|e| fold_param_default(&e.node).expect("validated"));
        let ty = if let Some(t) = &p.ty {
            lower_ty(&t.node)?
        } else if let Some(ref d) = default {
            const_type_id(d)
        } else {
            return Err(TypeError::Oop(format!(
                "lambda parameter `{}` needs an explicit type when used with defaults (RFC 007 M2c IIFE)",
                p.name
            )));
        };
        if let Some(ref d) = default {
            let dty = const_type_id(d);
            let null_ok = matches!(d, ConstValue::Null)
                && matches!(
                    ty,
                    TypeId::String | TypeId::Named(_) | TypeId::Object | TypeId::Nullable { .. }
                );
            if !slot_type_ok(&ty, &dty) && !null_ok {
                return Err(TypeError::Mismatch {
                    expected: ty.display(),
                    found: dty.display(),
                });
            }
        }
        out.push(ParamSlot {
            name: p.name.clone(),
            ty,
            default,
            is_params: false,
        });
    }
    Ok(out)
}

/// lambda 是否声明了任一可选默认值。
pub fn lambda_has_defaults(params: &[LambdaParam]) -> bool {
    params.iter().any(|p| p.default.is_some())
}
