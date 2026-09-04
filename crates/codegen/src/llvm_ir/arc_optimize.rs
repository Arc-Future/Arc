//! ARC retain/release pair elimination (RFC 015 Phase C).
//!
//! Operates on the emitted LLVM IR text post-codegen, pre-clang. Eliminates
//! adjacent `rt_arc_inc` / `rt_arc_dec` pairs within the same basic block
//! when they operate on the same pointer.
//!
//! ## Strategy
//!
//! Conservative: only removes pairs in the **same basic block**. Cross-block
//! elimination requires data-flow analysis (lifetime tracking across branches)
//! which is deferred to a MIR-level pass.
//!
//! ## Patterns recognized
//!
//! ```text
//! call void @rt_arc_inc(ptr %x)    ; eliminated
//! call void @rt_arc_dec(ptr %x)    ; eliminated
//! ```
//!
//! The pass also handles the reverse (dec-before-inc, e.g. setter overwrite)
//! since the ordering doesn't matter for correctness when both operate on the
//! same object in the same basic block.
//!
//! ## Non-goals
//!
//! - Cross-BB elimination (deferred to MIR data-flow pass)
//! - Triple elimination (retain-retain-release → single retain)
//! - Null-check elimination (rt_arc_inc already guards NULL internally)

/// Parse the operand of an ARC call. Returns `Some(ptr_name)` if line matches
/// `call void @rt_arc_{inc,dec}(ptr %NAME)` or `call void @rt_arc_{inc,dec}(ptr @NAME)`.
fn parse_arc_operand(line: &str) -> Option<&str> {
    let line = line.trim();
    // Match: call void @rt_arc_inc(ptr %x)
    for prefix in &["call void @rt_arc_inc(ptr ", "call void @rt_arc_dec(ptr "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            // rest = "%x)" or "@x)"
            if let Some(end) = rest.strip_suffix(')') {
                return Some(end);
            }
        }
    }
    None
}

/// Check if a line is an ARC inc or dec call.
fn is_arc_call(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("call void @rt_arc_inc(") || line.starts_with("call void @rt_arc_dec(")
}

/// Eliminate adjacent ARC retain/release pairs within the same basic block.
///
/// Scans the IR lines. When two consecutive lines are both ARC calls
/// (inc/dec or dec/inc) operating on the same pointer, both are replaced
/// with comments (`; ARC-optimized: ...`).
///
/// Returns the number of pairs eliminated.
pub fn eliminate_arc_pairs(ir: &mut String) -> usize {
    let lines: Vec<&str> = ir.lines().collect();
    let mut result = String::with_capacity(ir.len());
    let mut i = 0;
    let mut eliminated = 0;

    while i < lines.len() {
        // Check if this line and the next form an ARC pair to eliminate
        if i + 1 < lines.len() && is_arc_call(lines[i]) && is_arc_call(lines[i + 1]) {
            if let (Some(a), Some(b)) =
                (parse_arc_operand(lines[i]), parse_arc_operand(lines[i + 1]))
            {
                if a == b {
                    // Same pointer — eliminate both
                    let label = if lines[i].contains("rt_arc_inc") {
                        "inc/dec"
                    } else {
                        "dec/inc"
                    };
                    result.push_str(&format!("  ; ARC-eliminated: {label} pair on {a}\n"));
                    i += 2;
                    eliminated += 1;
                    continue;
                }
            }
        }
        // Keep this line
        result.push_str(lines[i]);
        result.push('\n');
        i += 1;
    }

    *ir = result;
    eliminated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eliminates_adjacent_inc_dec() {
        let mut ir = r#"entry:
  call void @rt_arc_inc(ptr %x)
  call void @rt_arc_dec(ptr %x)
  ret void
"#
        .to_string();
        let count = eliminate_arc_pairs(&mut ir);
        assert_eq!(count, 1);
        assert!(
            !ir.contains("rt_arc_inc"),
            "inc should be eliminated\n{}",
            ir
        );
        assert!(
            !ir.contains("rt_arc_dec"),
            "dec should be eliminated\n{}",
            ir
        );
        assert!(ir.contains("ARC-eliminated"));
    }

    #[test]
    fn eliminates_adjacent_dec_inc() {
        let mut ir = r#"entry:
  call void @rt_arc_dec(ptr %old)
  call void @rt_arc_inc(ptr %old)
  ret void
"#
        .to_string();
        let count = eliminate_arc_pairs(&mut ir);
        assert_eq!(count, 1);
        assert!(!ir.contains("rt_arc_inc"));
        assert!(!ir.contains("rt_arc_dec"));
    }

    #[test]
    fn preserves_different_operands() {
        let mut ir = r#"entry:
  call void @rt_arc_inc(ptr %x)
  call void @rt_arc_dec(ptr %y)
  ret void
"#
        .to_string();
        let count = eliminate_arc_pairs(&mut ir);
        assert_eq!(count, 0);
        assert!(ir.contains("rt_arc_inc"));
        assert!(ir.contains("rt_arc_dec"));
    }

    #[test]
    fn preserves_non_adjacent() {
        let mut ir = r#"entry:
  call void @rt_arc_inc(ptr %x)
  store i32 1, ptr %tmp
  call void @rt_arc_dec(ptr %x)
  ret void
"#
        .to_string();
        let count = eliminate_arc_pairs(&mut ir);
        assert_eq!(count, 0);
        assert!(ir.contains("rt_arc_inc"));
        assert!(ir.contains("rt_arc_dec"));
    }

    #[test]
    fn handles_multiple_pairs() {
        let mut ir = r#"entry:
  call void @rt_arc_inc(ptr %x)
  call void @rt_arc_dec(ptr %x)
  call void @rt_arc_inc(ptr %y)
  call void @rt_arc_dec(ptr %y)
  ret void
"#
        .to_string();
        let count = eliminate_arc_pairs(&mut ir);
        assert_eq!(count, 2);
        assert!(!ir.contains("rt_arc_inc"));
        assert!(!ir.contains("rt_arc_dec"));
    }

    #[test]
    fn handles_null_operand() {
        let mut ir = r#"entry:
  call void @rt_arc_inc(ptr null)
  call void @rt_arc_dec(ptr null)
  ret void
"#
        .to_string();
        let count = eliminate_arc_pairs(&mut ir);
        assert_eq!(count, 1, "null operands should be eliminated");
    }
}
