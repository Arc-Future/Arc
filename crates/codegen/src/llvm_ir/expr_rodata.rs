//! Structured expression tree rodata emission (RFC 003 Phase 1).
//!
//! Replaces the transitional `tree_summary` string with a tagged-union node
//! array emitted to `.rodata`. Each `ExpressionTreeConst` becomes a global
//! `[%struct.expr_node]` array, addressable by pointer at runtime.
//!
//! String constants embedded in expression trees are interned via the *same*
//! pool as user-visible string literals (`string_seen` in `mod.rs`), avoiding
//! the previous `hash_string % 10000` scheme which collided distinct strings
//! onto the same `@.str.N` global.

use super::emit_expr_tree::{binop_to_str, unaryop_to_str};
use ast::{BinOp, ConstantValue, ExpressionNode, ExpressionTree, UnaryOp};
use mir::{MirRvalue, MirStatement};
use std::collections::HashMap;

/// LLVM IR type declaration for a single expression node.
pub fn emit_expr_node_type() -> String {
    "%struct.expr_node = type { i32, i32, i32, i32, i64, double, ptr }\n".to_string()
}

/// Collect `(name, tree)` pairs from a MIR CFG body.
pub fn collect_expr_trees(body: &mir::MirCfgBody) -> Vec<(String, ExpressionTree)> {
    let mut out = Vec::new();
    for block in body.blocks.values() {
        for stmt in &block.statements {
            collect_from_stmt(stmt, &mut out);
        }
    }
    out
}

fn collect_from_stmt(stmt: &MirStatement, out: &mut Vec<(String, ExpressionTree)>) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => collect_from_rvalue(rvalue, out),
        MirStatement::FieldSet { value, .. } => collect_from_rvalue(value, out),
        // RFC 006 M3：静态字段赋值走 StaticFieldSet，与 FieldSet 对偶——
        // 表达式树作为静态字段初值虽罕见，但为与 FieldSet 一致递归收集。
        MirStatement::StaticFieldSet { value, .. } => collect_from_rvalue(value, out),
        MirStatement::IndexSet { value, .. } => collect_from_rvalue(value, out),
        MirStatement::Return(Some(rv)) => collect_from_rvalue(rv, out),
        MirStatement::Throw { value } => collect_from_rvalue(value, out),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_from_stmt(s, out);
            }
            for s in catch_body {
                collect_from_stmt(s, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_from_stmt(s, out);
            }
            for s in finally {
                collect_from_stmt(s, out);
            }
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_from_stmt(s, out);
            }
            for s in else_body {
                collect_from_stmt(s, out);
            }
        }
        MirStatement::While { body, .. } | MirStatement::LinqForeach { body, .. } => {
            for s in body {
                collect_from_stmt(s, out);
            }
        }
        _ => {}
    }
}

fn collect_from_rvalue(rv: &MirRvalue, out: &mut Vec<(String, ExpressionTree)>) {
    if let MirRvalue::ExpressionTreeConst { name, tree } = rv {
        out.push((name.clone(), tree.clone()));
    }
}

/// Intern all string constants embedded in `tree` into the shared string pool.
///
/// Must be called for every collected tree *before* `emit_expr_tree_globals`
/// (RFC 003 rodata path) or before `FnEmitter::emit_expression_tree`
/// (RFC 022 Sprint 2b runtime construction path) so that `string_seen`
/// contains the canonical `@.str.N` name for each tree string.
///
/// RFC 022 Sprint 2b: also interns identifier strings (Parameter name,
/// MemberAccess member, Call method) since the runtime construction path
/// emits them as `ptr` field values referencing the shared string pool.
pub fn intern_tree_strings(
    tree: &ExpressionTree,
    literals: &mut Vec<String>,
    seen: &mut HashMap<String, String>,
) {
    for node in &tree.nodes {
        match node {
            ExpressionNode::Constant(ConstantValue::String(s)) => {
                intern_one(s, literals, seen);
                intern_one("string", literals, seen);
            }
            // Int/Float/Bool constants have their string representation stored
            // in the StringValue field by emit_const_expr_node (so runtime
            // translators can emit text without int→string conversion). These
            // strings must be interned here too, otherwise intern_string falls
            // back to @.str.0 and the constant value is silently lost.
            // TypeName（"int"/"double"/"bool"/"string"）同步预驻留。
            ExpressionNode::Constant(ConstantValue::Int(n)) => {
                intern_one(&n.to_string(), literals, seen);
                intern_one("int", literals, seen);
            }
            ExpressionNode::Constant(ConstantValue::Float(f)) => {
                intern_one(&format!("{f}"), literals, seen);
                intern_one("double", literals, seen);
            }
            ExpressionNode::Constant(ConstantValue::Bool(b)) => {
                intern_one(if *b { "TRUE" } else { "FALSE" }, literals, seen);
                intern_one("bool", literals, seen);
            }
            ExpressionNode::Parameter { name, ty } => {
                intern_one(name.as_str(), literals, seen);
                intern_one(ty.as_str(), literals, seen);
            }
            ExpressionNode::Capture { name, ty, .. } => {
                intern_one(name.as_str(), literals, seen);
                intern_one(ty.as_str(), literals, seen);
            }
            ExpressionNode::MemberAccess { member, ty, .. } => {
                intern_one(member.as_str(), literals, seen);
                intern_one(ty.as_str(), literals, seen);
            }
            ExpressionNode::Call { method, .. } => intern_one(method.as_str(), literals, seen),
            // Binary/Unary op 与结果 TypeName 须预驻留（emit_expr_tree 写入字段）。
            ExpressionNode::Binary { op, .. } => {
                intern_one(binop_to_str(op), literals, seen);
                let result_ty = match op {
                    BinOp::Eq
                    | BinOp::NotEq
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or => "bool",
                    _ => "int",
                };
                intern_one(result_ty, literals, seen);
            }
            ExpressionNode::Unary { op, .. } => {
                intern_one(unaryop_to_str(op), literals, seen);
                let result_ty = match op {
                    UnaryOp::Not => "bool",
                    UnaryOp::Neg => "int",
                    UnaryOp::BitNot => "int",
                };
                intern_one(result_ty, literals, seen);
            }
            // RFC 022 新增：New/Cast 携带类型名字符串，由运行时构造路径
            // （emit_expr_tree.rs）作为 ptr 字段值输出，需在此注册。
            ExpressionNode::New { type_name, .. } => intern_one(type_name.as_str(), literals, seen),
            ExpressionNode::Cast { target_type, .. } => {
                intern_one(target_type.as_str(), literals, seen)
            }
            // Conditional 结果 TypeName（取真分支推断）须预驻留，供 ==/!= 分派。
            ExpressionNode::Conditional { if_true, .. } => {
                intern_one(if_true.inferred_type_name().as_str(), literals, seen);
            }
            _ => {}
        }
    }
}

fn intern_one(s: &str, literals: &mut Vec<String>, seen: &mut HashMap<String, String>) {
    if !seen.contains_key(s) {
        let name = format!("@.str.{}", literals.len());
        seen.insert(s.to_string(), name);
        literals.push(s.to_string());
    }
}

/// Emit global constants for all expression trees, keyed by name (first wins on duplicate).
///
/// `string_seen` must already contain every string constant referenced by any
/// tree (call `intern_tree_strings` first).
pub fn emit_expr_tree_globals(
    trees: &[(String, ExpressionTree)],
    string_seen: &HashMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str("; ---- Expression tree rodata ----\n");
    let mut seen_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (name, tree) in trees {
        if !seen_names.insert(name.as_str()) {
            continue;
        }
        out.push_str(&emit_single_tree_global(name, tree, string_seen));
    }
    out
}

fn emit_single_tree_global(
    name: &str,
    tree: &ExpressionTree,
    string_seen: &HashMap<String, String>,
) -> String {
    let nodes = &tree.nodes;
    let count = nodes.len();
    let mut consts = Vec::with_capacity(count);
    for (idx, node) in nodes.iter().enumerate() {
        consts.push(emit_node_const(node, idx, nodes, string_seen));
    }
    let body = consts.join(", ");
    format!("@{name} = private constant [{count} x %struct.expr_node] [{body}]\n")
}

fn emit_node_const(
    node: &ExpressionNode,
    idx: usize,
    nodes: &[ExpressionNode],
    string_seen: &HashMap<String, String>,
) -> String {
    let (tag, op, child0, child1) = node_descriptor(node, idx, nodes);
    let (int_val, float_val, str_val) = payload(node, string_seen);
    format!(
        "%struct.expr_node {{ i32 {tag}, i32 {op}, i32 {child0}, i32 {child1}, i64 {int_val}, double {float_val}, ptr {str_val} }}"
    )
}

fn node_descriptor(
    node: &ExpressionNode,
    idx: usize,
    nodes: &[ExpressionNode],
) -> (i32, i32, i32, i32) {
    match node {
        ExpressionNode::Constant(_) => (0, 0, -1, -1),
        ExpressionNode::Parameter { .. } => (1, 0, -1, -1),
        ExpressionNode::Capture { .. } => (7, 0, -1, -1),
        ExpressionNode::MemberAccess { .. } => {
            let c0 = first_child_idx(idx, nodes);
            (2, 0, c0, -1)
        }
        ExpressionNode::Binary { op, left, .. } => {
            let c0 = first_child_idx(idx, nodes);
            let c1 = c0 + subtree_size(left);
            (3, binop_tag(op) as i32, c0, c1)
        }
        ExpressionNode::Unary { op, .. } => {
            let c0 = first_child_idx(idx, nodes);
            (4, unaryop_tag(op) as i32, c0, -1)
        }
        ExpressionNode::Call { .. } => (5, 0, -1, -1),
        ExpressionNode::Lambda { .. } => (6, 0, -1, -1),
        // RFC 022 新增节点（rodata 为 RFC 003 遗留路径，仅尽力编码；
        // 运行时构造由 emit_expr_tree.rs 负责）。
        ExpressionNode::Index { object, .. } => {
            let c0 = first_child_idx(idx, nodes);
            let c1 = c0 + subtree_size(object);
            (8, 0, c0, c1)
        }
        ExpressionNode::Conditional { test, .. } => {
            // 三槽子节点无法完整编码进 (child0, child1)，仅编码 test 与 if_true。
            let c0 = first_child_idx(idx, nodes);
            let c1 = c0 + subtree_size(test);
            (9, 0, c0, c1)
        }
        ExpressionNode::New { .. } => (10, 0, -1, -1),
        ExpressionNode::Cast { .. } => {
            let c0 = first_child_idx(idx, nodes);
            (11, 0, c0, -1)
        }
        // RFC 022 §2.2.10 L2/L3 节点（28 变体）不进入 codegen rodata 发射路径
        // （设计原则 4：codegen 仅消费 L1 12 变体）。这里给一个保守 fallback
        // 标签 12（与 L1 标签 0-11 区分），子节点 -1 表示无子节点。
        // 实际上 codegen 不应该接收到这些节点——若收到说明上游 typeck/lower
        // 路径有 bug，需排查。
        _ => (12, 0, -1, -1),
    }
}

fn first_child_idx(parent_idx: usize, _nodes: &[ExpressionNode]) -> i32 {
    (parent_idx as i32) + 1
}

fn subtree_size(node: &ExpressionNode) -> i32 {
    1 + match node {
        ExpressionNode::Constant(_) => 0,
        ExpressionNode::Parameter { .. } => 0,
        ExpressionNode::Capture { .. } => 0,
        ExpressionNode::MemberAccess { object, .. } => subtree_size(object),
        ExpressionNode::Binary { left, right, .. } => subtree_size(left) + subtree_size(right),
        ExpressionNode::Unary { operand, .. } => subtree_size(operand),
        ExpressionNode::Call { target, args, .. } => {
            target.as_ref().map(|t| subtree_size(t)).unwrap_or(0)
                + args.iter().map(subtree_size).sum::<i32>()
        }
        ExpressionNode::Lambda { body, .. } => subtree_size(body),
        ExpressionNode::Index { object, index } => subtree_size(object) + subtree_size(index),
        ExpressionNode::Conditional {
            test,
            if_true,
            if_false,
        } => subtree_size(test) + subtree_size(if_true) + subtree_size(if_false),
        ExpressionNode::New { args, .. } => args.iter().map(subtree_size).sum(),
        ExpressionNode::Cast { operand, .. } => subtree_size(operand),
        // RFC 022 §2.2.10 L2/L3 节点不进入 codegen rodata 发射路径。
        // 返回 0 表示无子节点贡献——与 fallback 标签 12 配合，rodata
        // 序列化不会展开 L2/L3 子树（实际上 codegen 不应接收到这些节点）。
        _ => 0,
    }
}

fn payload(node: &ExpressionNode, string_seen: &HashMap<String, String>) -> (i64, String, String) {
    match node {
        ExpressionNode::Constant(cv) => match cv {
            ConstantValue::Int(n) => (*n, "0.0".into(), "null".into()),
            ConstantValue::Float(f) => {
                let bits = f.to_bits() as i64;
                (bits, "0.0".into(), "null".into())
            }
            ConstantValue::Bool(b) => (if *b { 1 } else { 0 }, "0.0".into(), "null".into()),
            ConstantValue::String(s) => {
                // Lookup in the shared string pool. The pool was populated by
                // `intern_tree_strings` before emission, so a missing entry is
                // a compiler bug — fall back to `null` rather than crashing so
                // that downstream clang still produces a diagnostic.
                let name = string_seen.get(s).cloned().unwrap_or_else(|| "null".into());
                (0, "0.0".into(), name)
            }
        },
        _ => (0, "0.0".into(), "null".into()),
    }
}

fn binop_tag(op: &BinOp) -> u32 {
    match op {
        BinOp::Add => 1,
        BinOp::Sub => 2,
        BinOp::Mul => 3,
        BinOp::Div => 4,
        BinOp::Mod => 5,
        BinOp::Eq => 6,
        BinOp::NotEq => 7,
        BinOp::Lt => 8,
        BinOp::Le => 9,
        BinOp::Gt => 10,
        BinOp::Ge => 11,
        BinOp::And => 12,
        BinOp::Or => 13,
        BinOp::BitAnd => 14,
        BinOp::BitOr => 15,
        BinOp::BitXor => 16,
        BinOp::Shl => 17,
        BinOp::Shr => 18,
    }
}

fn unaryop_tag(op: &UnaryOp) -> u32 {
    match op {
        UnaryOp::Not => 1,
        UnaryOp::Neg => 2,
        UnaryOp::BitNot => 3,
    }
}
