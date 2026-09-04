//! RFC 005 游标提升：纯 `sb.Append(char)` 循环的判定与 guard。
//!
//! 游标提升的安全前提：循环体内，StringBuilder 头字段（data/len/cap）除该
//! 追加本身外**不被任何其它语句读写**。判定据此保守收紧——仅当 body/cond
//! 中接收者 local 唯一被引用的地方就是那一次 `Append(char)` 时，才可提升。
//! 任何无法分析/可能别名/逃逸的形态一律拒绝（返回 None），绝不冒险。

use mir::{LocalId, MirOperand, MirRvalue, MirStatement};

/// `stmt`（含嵌套子语句）是否含 `return`。
fn has_return(stmt: &MirStatement) -> bool {
    match stmt {
        MirStatement::Return(_) => true,
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => then_body.iter().any(has_return) || else_body.iter().any(has_return),
        MirStatement::While { body, .. }
        | MirStatement::LinqForeach { body, .. }
        | MirStatement::TryFinally { body, .. } => body.iter().any(has_return),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => try_body.iter().any(has_return) || catch_body.iter().any(has_return),
        _ => false,
    }
}

/// 若 `stmt` 恰为 StringBuilder `Append(char)` 的 MethodCall 赋值（接收者为裸
/// `Local`），返回 `(receiver_id, args_reference_receiver)`。
fn sb_append_char_receiver(stmt: &MirStatement) -> Option<(LocalId, bool)> {
    let MirStatement::Assign { rvalue, .. } = stmt else {
        return None;
    };
    let MirRvalue::MethodCall {
        receiver,
        method,
        receiver_type,
        target_fn,
        args,
        ..
    } = rvalue
    else {
        return None;
    };
    if method != "Append" || receiver_type != "StringBuilder" {
        return None;
    }
    let is_char = target_fn.as_deref().is_some_and(|t| t.ends_with("_char"));
    if !is_char {
        return None;
    }
    let MirOperand::Local(rid) = receiver else {
        return None; // 接收者非裸 local（如链式 temp）→ 不提升
    };
    let rid = *rid;
    let args_ok = !args.iter().any(|a| operand_refs(a, rid));
    Some((rid, args_ok))
}

/// `stmt` 是否引用 local `id`（含嵌套子语句）。
fn stmt_refs(stmt: &MirStatement, id: LocalId) -> bool {
    match stmt {
        MirStatement::Assign { rvalue, .. } => rvalue_refs(rvalue, id),
        MirStatement::Drop(l) => *l == id,
        MirStatement::Break | MirStatement::Continue => false,
        MirStatement::Return(Some(rv)) => rvalue_refs(rv, id),
        MirStatement::Return(None) => false,
        MirStatement::If {
            cond,
            then_body,
            else_body,
        } => {
            operand_refs(cond, id)
                || then_body.iter().any(|s| stmt_refs(s, id))
                || else_body.iter().any(|s| stmt_refs(s, id))
        }
        MirStatement::While {
            cond,
            body,
            foreach_source,
        } => {
            rvalue_refs(cond, id)
                || foreach_source
                    .as_ref()
                    .is_some_and(|src| operand_refs(src, id))
                || body.iter().any(|s| stmt_refs(s, id))
        }
        MirStatement::FieldSet { object, value, .. } => {
            operand_refs(object, id) || rvalue_refs(value, id)
        }
        MirStatement::StaticFieldSet { value, .. } => rvalue_refs(value, id),
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => operand_refs(array, id) || operand_refs(index, id) || rvalue_refs(value, id),
        MirStatement::Await { task, .. } => rvalue_refs(task, id),
        MirStatement::Throw { value } => rvalue_refs(value, id),
        // 复杂控制流：保守拒绝提升。
        MirStatement::LinqForeach { body, .. }
        | MirStatement::TryCatch { try_body: body, .. }
        | MirStatement::TryFinally { body, .. } => body.iter().any(|s| stmt_refs(s, id)),
    }
}

/// `rv` 是否引用 local `id`。
fn rvalue_refs(rv: &MirRvalue, id: LocalId) -> bool {
    match rv {
        MirRvalue::Use(o) => operand_refs(o, id),
        MirRvalue::Coalesce { left, right } => operand_refs(left, id) || operand_refs(right, id),
        MirRvalue::Binary { left, right, .. } => operand_refs(left, id) || operand_refs(right, id),
        MirRvalue::Call { args, .. } | MirRvalue::New { args, .. } => {
            args.iter().any(|a| operand_refs(a, id))
        }
        MirRvalue::FieldGet { object, .. }
        | MirRvalue::MakeIface { object, .. }
        | MirRvalue::MakeIfaceDyn { object, .. }
        | MirRvalue::AdaptIface { object, .. }
        | MirRvalue::Box { src: object, .. }
        | MirRvalue::Unbox { src: object, .. } => operand_refs(object, id),
        MirRvalue::MethodCall { receiver, args, .. }
        | MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            operand_refs(receiver, id) || args.iter().any(|a| operand_refs(a, id))
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            operand_refs(receiver, id)
                || args.iter().any(|a| operand_refs(a, id))
                || operand_refs(default, id)
        }
        MirRvalue::StructLit { fields, .. } => fields.iter().any(|(_, v)| operand_refs(v, id)),
        MirRvalue::ArrayLit { elements, .. } => elements.iter().any(|e| match e {
            mir::ArrayLitElement::Value(v) => rvalue_refs(v, id),
            mir::ArrayLitElement::Spread(o) => operand_refs(o, id),
        }),
        MirRvalue::IndexGet { array, index, .. } => {
            operand_refs(array, id) || operand_refs(index, id)
        }
        MirRvalue::SpanFromArray { array, start, .. } => {
            operand_refs(array, id) || start.as_ref().is_some_and(|s| operand_refs(s, id))
        }
        MirRvalue::SpanFromStack { elements, .. } => elements.iter().any(|e| operand_refs(e, id)),
        MirRvalue::SpanSlice { span, start, .. } => {
            operand_refs(span, id) || operand_refs(start, id)
        }
        MirRvalue::SpanFill { span, value, .. } => {
            operand_refs(span, id) || operand_refs(value, id)
        }
        MirRvalue::SpanClear { span, .. } => operand_refs(span, id),
        MirRvalue::SpanCopyTo { src, dest, .. } => operand_refs(src, id) || operand_refs(dest, id),
        MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            operand_refs(src, id) || operand_refs(dest, id)
        }
        MirRvalue::SpanToArray { span, .. } => operand_refs(span, id),
        MirRvalue::SoaFieldGet { array, index, .. } => {
            operand_refs(array, id) || operand_refs(index, id)
        }
        MirRvalue::IndirectCall { func, args } => {
            operand_refs(func, id) || args.iter().any(|a| operand_refs(a, id))
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => operand_refs(cond, id) || operand_refs(then_val, id) || operand_refs(else_val, id),
        MirRvalue::NullCondField {
            receiver, default, ..
        } => operand_refs(receiver, id) || operand_refs(default, id),
        MirRvalue::ForceDerefField { receiver, .. } => operand_refs(receiver, id),
        MirRvalue::VariantConstruct { payload, .. } => {
            payload.as_ref().is_some_and(|p| operand_refs(p, id))
        }
        MirRvalue::VariantTag { scrutinee, .. } | MirRvalue::VariantExtract { scrutinee, .. } => {
            operand_refs(scrutinee, id)
        }
        MirRvalue::NewArray { length, .. } => operand_refs(length, id),
        // 无 local 引用 / 非值形态。
        MirRvalue::LinqChain(_)
        | MirRvalue::ExpressionTreeConst { .. }
        | MirRvalue::FnPtr { .. } => false,
    }
}

/// `op` 是否引用 local `id`。
fn operand_refs(op: &MirOperand, id: LocalId) -> bool {
    match op {
        MirOperand::Local(l) | MirOperand::AddrOf(l) => *l == id,
        MirOperand::Field { object, .. }
        | MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. }
        | MirOperand::UnboxString { object }
        | MirOperand::UnboxGeneric { object, .. } => operand_refs(object, id),
        MirOperand::Closure { env, .. } => env.iter().any(|(_, o)| operand_refs(o, id)),
        // 常量 / 符号 / 类型 token 无 local 引用。
        MirOperand::ConstInt(_)
        | MirOperand::ConstFloat(_)
        | MirOperand::ConstString(_)
        | MirOperand::ConstBool(_)
        | MirOperand::ConstNull
        | MirOperand::ConstDefault { .. }
        | MirOperand::FnPtr { .. }
        | MirOperand::TypeId { .. }
        | MirOperand::TypeInfoPtr { .. }
        | MirOperand::StaticField { .. } => false,
    }
}

/// CFG 版纯追加判定：`body_stmts` 为 flag 式 `while` 中 `if(then)` 的真实循环体
/// （含 `sb.Append(char)` 与增量），`other_blocks` 为循环内其余各块（header /
/// 嵌套 If 头 / flag=false 块 / backedge 块）。返回 StringBuilder 接收者 local；
/// 任何无法分析/别名/逃逸形态一律 `None`（不提升）。
pub(super) fn sb_append_cfg_receiver(
    body_stmts: &[MirStatement],
    other_blocks: &[&mir::MirBlock],
) -> Option<LocalId> {
    let mut receiver: Option<LocalId> = None;
    for stmt in body_stmts {
        if let Some((rid, args_ok)) = sb_append_char_receiver(stmt) {
            if !args_ok {
                return None;
            }
            match receiver {
                None => receiver = Some(rid),
                Some(prev) if prev == rid => {}
                _ => return None,
            }
        }
    }
    let rid = receiver?;
    // 循环体 `return` 会绕过出口 flush（stale 头泄露）→ 拒绝提升。
    if body_stmts.iter().any(has_return) {
        return None;
    }
    // 除追加本身外，循环体其余语句不得引用 rid。
    for stmt in body_stmts {
        if sb_append_char_receiver(stmt).is_some() {
            continue;
        }
        if stmt_refs(stmt, rid) {
            return None;
        }
    }
    // 循环内其余各块（header/嵌套 If 头/flag=false/backedge）不得读取 stale 头。
    for blk in other_blocks {
        if blk.statements.iter().any(|s| stmt_refs(s, rid)) {
            return None;
        }
    }
    Some(rid)
}
