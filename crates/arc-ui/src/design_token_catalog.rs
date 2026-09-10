//! DesignTokenCatalog — AI-native fidelity-loop compile gate (037 §1).
//!
//! Builds a machine-readable catalog from Light.arml colors + BuiltInTheme
//! non-color keys. Color authority remains Themes/*.arml; this module only
//! aggregates for LLM context and bare-value checks (no dual source).

use std::path::Path;

use crate::builtin_theme_gen::{load_theme_colors, LIGHT_ARML_REL};

/// Published catalog path relative to repo root (contract-tested).
pub const CATALOG_JSON_REL: &str = "std/UI/Core/Themes/DesignTokenCatalog.json";

/// Token category (L0 progressive disclosure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenCategory {
    Color,
    Spacing,
    Size,
    Radius,
    Font,
    Motion,
}

impl TokenCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            TokenCategory::Color => "Color",
            TokenCategory::Spacing => "Spacing",
            TokenCategory::Size => "Size",
            TokenCategory::Radius => "Radius",
            TokenCategory::Font => "Font",
            TokenCategory::Motion => "Motion",
        }
    }
}

/// One semantic token entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignTokenEntry {
    pub name: String,
    pub category: TokenCategory,
    /// Short role (e.g. Primary / Text.Primary).
    pub role: String,
    /// Always true for catalog tokens (must use StaticResource / Token).
    pub bare_forbidden: bool,
    /// Light theme authority literal (colors only).
    pub light_value: Option<String>,
}

/// Full catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignTokenCatalog {
    pub schema: &'static str,
    pub tokens: Vec<DesignTokenEntry>,
}

impl DesignTokenCatalog {
    pub const SCHEMA: &'static str = "arc.ui.design-token-catalog/v1";

    /// Build catalog from repo root (Light.arml + non-color key table).
    pub fn build_from_repo(repo_root: &Path) -> Result<Self, String> {
        let colors = load_theme_colors(&repo_root.join(LIGHT_ARML_REL))?;
        let mut tokens = Vec::new();
        for (key, value) in &colors {
            let role = key
                .strip_prefix("Color.")
                .unwrap_or(key.as_str())
                .to_string();
            tokens.push(DesignTokenEntry {
                name: key.clone(),
                category: TokenCategory::Color,
                role,
                bare_forbidden: true,
                light_value: Some(value.clone()),
            });
        }
        for (name, category, role) in NON_COLOR_TOKENS {
            tokens.push(DesignTokenEntry {
                name: (*name).to_string(),
                category: *category,
                role: (*role).to_string(),
                bare_forbidden: true,
                light_value: None,
            });
        }
        tokens.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self {
            schema: Self::SCHEMA,
            tokens,
        })
    }

    /// Stable JSON (hand-written; no serde).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"schema\": \"{}\",\n", self.schema));
        out.push_str("  \"tokens\": [\n");
        for (i, t) in self.tokens.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"name\": \"{}\",\n", escape_json(&t.name)));
            out.push_str(&format!(
                "      \"category\": \"{}\",\n",
                t.category.as_str()
            ));
            out.push_str(&format!("      \"role\": \"{}\",\n", escape_json(&t.role)));
            out.push_str("      \"bareForbidden\": true");
            if let Some(v) = &t.light_value {
                out.push_str(",\n");
                out.push_str(&format!("      \"lightValue\": \"{}\"\n", escape_json(v)));
            } else {
                out.push('\n');
            }
            out.push_str("    }");
            if i + 1 < self.tokens.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ]\n");
        out.push_str("}\n");
        out
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tokens.iter().any(|t| t.name == name)
    }
}

/// BuiltInTheme non-color keys (aligned with BuiltInTheme.as consts).
const NON_COLOR_TOKENS: &[(&str, TokenCategory, &str)] = &[
    ("Radius.Control", TokenCategory::Radius, "Control"),
    ("Radius.Surface", TokenCategory::Radius, "Surface"),
    ("Radius.Pill", TokenCategory::Radius, "Pill"),
    (
        "Size.Control.Height.SM",
        TokenCategory::Size,
        "Control.Height.SM",
    ),
    ("Size.Control.Height", TokenCategory::Size, "Control.Height"),
    (
        "Size.Control.Height.LG",
        TokenCategory::Size,
        "Control.Height.LG",
    ),
    (
        "Size.Control.Padding",
        TokenCategory::Size,
        "Control.Padding",
    ),
    (
        "Size.Control.Padding.SM",
        TokenCategory::Size,
        "Control.Padding.SM",
    ),
    (
        "Size.Control.Padding.LG",
        TokenCategory::Size,
        "Control.Padding.LG",
    ),
    (
        "Size.Border.Thickness",
        TokenCategory::Size,
        "Border.Thickness",
    ),
    ("Size.Border.Padding", TokenCategory::Size, "Border.Padding"),
    (
        "Size.MessageBox.Padding",
        TokenCategory::Size,
        "MessageBox.Padding",
    ),
    (
        "Size.Progress.Thickness",
        TokenCategory::Size,
        "Progress.Thickness",
    ),
    ("Size.Slider.Height", TokenCategory::Size, "Slider.Height"),
    ("Spacing.XS", TokenCategory::Spacing, "XS"),
    ("Spacing.SM", TokenCategory::Spacing, "SM"),
    ("Spacing.MD", TokenCategory::Spacing, "MD"),
    ("Spacing.LG", TokenCategory::Spacing, "LG"),
    ("Spacing.XL", TokenCategory::Spacing, "XL"),
    ("Font.Body.Size", TokenCategory::Font, "Body.Size"),
    ("Font.Body.Family", TokenCategory::Font, "Body.Family"),
    ("Font.Caption.Size", TokenCategory::Font, "Caption.Size"),
    ("Font.Heading.Size", TokenCategory::Font, "Heading.Size"),
    (
        "Motion.Duration.Fast",
        TokenCategory::Motion,
        "Duration.Fast",
    ),
    (
        "Motion.Duration.Normal",
        TokenCategory::Motion,
        "Duration.Normal",
    ),
    (
        "Motion.Easing.Standard",
        TokenCategory::Motion,
        "Easing.Standard",
    ),
    (
        "Motion.Easing.Linear",
        TokenCategory::Motion,
        "Easing.Linear",
    ),
    ("Motion.Easing.In", TokenCategory::Motion, "Easing.In"),
    ("Motion.Easing.InOut", TokenCategory::Motion, "Easing.InOut"),
];

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Generate catalog JSON text.
pub fn generate_catalog_json(repo_root: &Path) -> Result<String, String> {
    Ok(DesignTokenCatalog::build_from_repo(repo_root)?.to_json())
}

/// Write catalog to DesignTokenCatalog.json.
pub fn write_catalog_json(repo_root: &Path) -> Result<std::path::PathBuf, String> {
    let text = generate_catalog_json(repo_root)?;
    let out = repo_root.join(CATALOG_JSON_REL);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&out, text).map_err(|e| e.to_string())?;
    Ok(out)
}
