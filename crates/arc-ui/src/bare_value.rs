//! No bare values (037 fidelity-loop 1.2) — color / Thickness must reference tokens.
//!
//! Token definition surfaces (Color/Thickness/Match Value=) allow literals;
//! component attributes and Style Setters forbid bare #hex and bare Thickness tuples.

use crate::ast::{AttributeValue, MarkupKind, Span};
use crate::error::ArmlError;

/// Color-like properties (must use StaticResource / Binding / Token).
pub fn is_color_token_property(name: &str) -> bool {
    matches!(
        name,
        "Background"
            | "Foreground"
            | "BorderBrush"
            | "Fill"
            | "Stroke"
            | "CaretBrush"
            | "SelectionBrush"
            | "HighlightBrush"
    )
}

/// Thickness-like properties (must use Size.* / Spacing resources).
pub fn is_thickness_token_property(name: &str) -> bool {
    matches!(name, "Margin" | "Padding" | "BorderThickness")
}

/// Resource/adaptive definition elements: Value= literals are token authority.
pub fn is_token_definition_element(element_name: &str) -> bool {
    matches!(
        element_name,
        "Color"
            | "Thickness"
            | "Double"
            | "String"
            | "Boolean"
            | "Match"
            | "SolidColorBrush"
    )
}

/// Bare # / #AARRGGBB color.
pub fn is_bare_hex(lit: &str) -> bool {
    lit.trim().starts_with('#')
}

/// Bare Thickness tuple (comma-separated numeric).
pub fn is_bare_thickness(lit: &str) -> bool {
    let t = lit.trim();
    if !t.contains(',') {
        return false;
    }
    t.split(',').all(|p| {
        let p = p.trim();
        !p.is_empty() && p.chars().all(|c| c.is_ascii_digit() || c == '.')
    })
}

/// Diagnose bare literal on property/Setter; Ok => None.
pub fn diagnose_bare_literal(
    property: &str,
    value: &AttributeValue,
    span: Span,
) -> Option<ArmlError> {
    match value {
        AttributeValue::MarkupExtension(ext) => {
            if matches!(
                ext.kind,
                MarkupKind::StaticResource
                    | MarkupKind::Token
                    | MarkupKind::Binding
                    | MarkupKind::XBind
            ) {
                None
            } else if is_color_token_property(property) || is_thickness_token_property(property) {
                Some(ArmlError::type_error(
                    span,
                    format!(
                        "property `{}` must use `{{StaticResource}}` / `{{Token}}` / binding; bare markup `{}` forbidden",
                        property,
                        ext.kind.as_str()
                    ),
                ))
            } else {
                None
            }
        }
        AttributeValue::Literal(lit) => {
            if is_color_token_property(property) && is_bare_hex(lit) {
                return Some(ArmlError::type_error(
                    span,
                    format!(
                        "bare color `{}` on `{}` forbidden; use `{{StaticResource Color.*}}` (DesignTokenCatalog)",
                        lit, property
                    ),
                ));
            }
            if is_thickness_token_property(property) && is_bare_thickness(lit) {
                return Some(ArmlError::type_error(
                    span,
                    format!(
                        "bare Thickness `{}` on `{}` forbidden; use `{{StaticResource Size.*}}` / Spacing token",
                        lit, property
                    ),
                ));
            }
            None
        }
    }
}
