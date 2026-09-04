//! LINQ `foreach` lowering: indexed iteration over an enumerable source.
//!
//! Replaces the previous Phase A placeholder (which emitted a non-terminating
//! loop). The source `MirOperand` is treated as a `List<T>` object handle:
//!   1. Load the runtime handle from offset 16 (ArcHeader is 16 bytes).
//!   2. Query `rt_list_size` for the upper bound.
//!   3. Emit a canonical indexed loop: `i = 0; while i < size { body; i++ }`.
//!   4. Each iteration stores the element into the loop variable's alloca slot
//!      via `rt_list_get`, so the body's `MirStatement`s see the correct value.
//!
//! Full LINQ operator inlining (`Where`/`Select` predicates) is a future task
//! tracked under RFC 003 Phase 2; this module exists to make the Enumerable
//! path *correct* before it is made *fast*.

use super::*;
use ast::Ident;
use mir::{LinqChain, MirStatement};

impl<'a> FnEmitter<'a> {
    /// Emit a terminating indexed loop for `foreach (var x in <chain>) { body }`.
    ///
    /// `var` is the source-level identifier; it is resolved to a `LocalId`
    /// via `cfg.locals` so the body statements (which reference `LocalId`s)
    /// observe the current element.
    pub fn emit_linq_foreach(&mut self, var: &Ident, chain: &LinqChain, body: &[MirStatement]) {
        let (var_id, var_ty) = match self.resolve_loop_var(var) {
            Some(info) => info,
            None => {
                // Without a backing local we cannot bind the element. Fall back
                // to executing the body once with an undefined binding rather
                // than emitting an infinite loop. This is a defensive measure;
                // a well-formed MIR always has the binding.
                self.emit("; linq foreach: loop variable not found; emitting body once");
                for (i, s) in body.iter().enumerate() {
                    self.stmt_path.push(i);
                    self.emit_stmt(s);
                    self.stmt_path.pop();
                }
                return;
            }
        };

        let _ = var_ty; // element type is implied by the alloca slot
        let var_ptr = self.local_ptr(var_id);

        // Load the List handle from the source object (offset 16 = sizeof ArcHeader).
        let (_, src_val) = self.emit_operand(&chain.source);
        let handle_ptr = self.fresh_temp();
        self.emit(&format!(
            "{handle_ptr} = getelementptr inbounds i8, ptr {src_val}, i32 16"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load ptr, ptr {handle_ptr}"));

        // size = rt_list_size(handle)
        let size = self.fresh_temp();
        self.emit(&format!("{size} = call i32 @rt_list_size(ptr {handle})"));

        // i = 0
        let idx_ptr = self.fresh_temp();
        self.emit(&format!("{idx_ptr} = alloca i32"));
        self.emit("store i32 0, ptr {idx_ptr}");

        let header = self.fresh_label();
        let body_label = self.fresh_label();
        let exit = self.fresh_label();

        self.emit(&format!("br label %{header}"));
        self.emit_label(&header);

        // cond = i < size
        let idx_cur = self.fresh_temp();
        self.emit(&format!("{idx_cur} = load i32, ptr {idx_ptr}"));
        let cond = self.fresh_temp();
        self.emit(&format!("{cond} = icmp slt i32 {idx_cur}, {size}"));
        self.emit(&format!("br i1 {cond}, label %{body_label}, label %{exit}"));

        self.emit_label(&body_label);

        // var = rt_list_get(handle, i)
        self.emit(&format!(
            "call void @rt_list_get(ptr {handle}, i32 {idx_cur}, ptr {var_ptr})"
        ));

        // body
        for (i, s) in body.iter().enumerate() {
            self.stmt_path.push(i);
            self.emit_stmt(s);
            self.stmt_path.pop();
        }

        // i = i + 1
        let idx_next = self.fresh_temp();
        self.emit(&format!("{idx_next} = load i32, ptr {idx_ptr}"));
        let inc = self.fresh_temp();
        self.emit(&format!("{inc} = add i32 {idx_next}, 1"));
        self.emit(&format!("store i32 {inc}, ptr {idx_ptr}"));
        self.emit(&format!("br label %{header}"));

        self.emit_label(&exit);
    }

    /// Resolve a source-level loop variable name to its `LocalId` and type.
    fn resolve_loop_var(&self, var: &Ident) -> Option<(mir::LocalId, TypeId)> {
        for (id, (name, ty)) in &self.cfg.locals {
            if name == var {
                return Some((*id, ty.clone()));
            }
        }
        None
    }
}
