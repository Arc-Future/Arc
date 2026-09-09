//! Style short-key lookup (RFC 037 §0.1.1) — same order as runtime `StyleKeyResolver`.
//!
//! Authoring uses **only short keys** (`Primary`, `Small`). Dictionary may store
//! control overrides as `{TargetType}.{Short}` and globals as `{Short}`.
//!
//! Lookup for target `T` and key `K`:
//! 1. If `K` contains `.` → exact `K` only
//! 2. Else try `T.K` (control-scoped), then `K` (global)
//! 3. Miss → failure (caller formats tried keys)

/// Candidate keys in lookup order (does not consult a dictionary).
pub fn lookup_candidates(key: &str, target_type: &str) -> Vec<String> {
    let key = key.trim();
    if key.is_empty() {
        return Vec::new();
    }
    if key.contains('.') {
        return vec![key.to_string()];
    }
    let mut out = Vec::new();
    if !target_type.is_empty() {
        out.push(format!("{target_type}.{key}"));
    }
    out.push(key.to_string());
    out
}

/// Resolve against a known key set (window-local style map or builtin keys).
/// Returns the first candidate present in `known`, or `None`.
pub fn resolve_style_key(
    key: &str,
    target_type: &str,
    known: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    for cand in lookup_candidates(key, target_type) {
        if known.contains_key(&cand) {
            return Some(cand);
        }
    }
    None
}

/// Format a readable miss diagnostic.
pub fn miss_diagnostic(key: &str, target_type: &str) -> String {
    let tried = lookup_candidates(key, target_type);
    if tried.is_empty() {
        return format!("Style key is empty (TargetType={target_type})");
    }
    format!(
        "Style key `{key}` not found for TargetType `{target_type}` (tried: {})",
        tried.join(", ")
    )
}

/// Expand multi-bind args for codegen string fallback: keep **author short keys**
/// when nothing in `known` resolves, so runtime can still try scoped → global.
pub fn codegen_style_key_tokens<S: AsRef<str>>(
    keys: &[S],
    target_type: &str,
    known: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<CodegenStyleRef>, String> {
    let mut out = Vec::new();
    for k in keys {
        let raw = k.as_ref().trim();
        if raw.is_empty() {
            continue;
        }
        if let Some(resolved) = resolve_style_key(raw, target_type, known) {
            if let Some(var) = known.get(&resolved) {
                out.push(CodegenStyleRef::Typed(var.clone()));
                continue;
            }
        }
        // Theme / app domain: keep short (or exact) token for runtime fallback chain.
        out.push(CodegenStyleRef::RuntimeKey(raw.to_string()));
    }
    if out.is_empty() {
        return Err("`{StaticResource}` in `Style` requires a resource key".into());
    }
    // If any runtime key → whole binding must be string form (mixed typed+string not supported).
    if out.iter().any(|r| matches!(r, CodegenStyleRef::RuntimeKey(_))) {
        let joined = keys
            .iter()
            .map(|k| k.as_ref().trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        return Ok(vec![CodegenStyleRef::RuntimeKey(joined)]);
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenStyleRef {
    Typed(String),
    RuntimeKey(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn candidates_scoped_then_global() {
        assert_eq!(
            lookup_candidates("Small", "Button"),
            vec!["Button.Small".to_string(), "Small".to_string()]
        );
        assert_eq!(
            lookup_candidates("Primary", "Button"),
            vec!["Button.Primary".to_string(), "Primary".to_string()]
        );
        assert_eq!(
            lookup_candidates("Button.Small", "Button"),
            vec!["Button.Small".to_string()]
        );
    }

    #[test]
    fn resolve_prefers_scoped() {
        let mut known = BTreeMap::new();
        known.insert("Button.Small".into(), "_s0".into());
        known.insert("Small".into(), "_s1".into());
        assert_eq!(
            resolve_style_key("Small", "Button", &known).as_deref(),
            Some("Button.Small")
        );
    }

    #[test]
    fn resolve_falls_back_to_global() {
        let mut known = BTreeMap::new();
        known.insert("Small".into(), "_s1".into());
        assert_eq!(
            resolve_style_key("Small", "TextBox", &known).as_deref(),
            Some("Small")
        );
    }

    #[test]
    fn miss_lists_tried() {
        let msg = miss_diagnostic("Foo", "Button");
        assert!(msg.contains("Button.Foo"));
        assert!(msg.contains("Foo"));
    }
}
