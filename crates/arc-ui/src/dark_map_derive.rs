//! Build-time Dark Map Token derivation from Light Seed (Ant Design 6 darkAlgorithm snapshot).
//!
//! Honest scope:
//! - **Not** a runtime darkAlgorithm / @ant-design/colors port.
//! - For the **default** Light Seed set (colorPrimary=#1677FF + default Danger/Success/Warning),
//!   emits the documented Ant Design 6 dark map token table into Dark.arml.
//! - Changing Seed without extending [DEFAULT_SEED_DARK_MAP] fails loudly — no silent fake palette.
//!
//! Regenerate: scripts/ui-theme/derive-dark-from-light.ps1 or
//! UPDATE_DARK_THEME=1 cargo test -p arc-ui --test design_tokens_contract -- dark_arml_matches_light_seed_derivation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::builtin_theme_gen::{load_theme_colors, DARK_ARML_REL, KEY_TO_FIELD, LIGHT_ARML_REL};

/// Supported default Light Seed -> Dark Map (Ant Design 6 darkAlgorithm reference snapshot).
/// Neutrals use bgBase=#000 / textBase=#fff; primary family from dark generate(#1677ff).
struct SeedDarkMap {
    /// Light Color.Primary (ARGB).
    primary_seed: &'static str,
    dark_primary: &'static str,
    dark_primary_hover: &'static str,
    dark_primary_pressed: &'static str,
    /// Dark primary-1 approx (colorPrimaryBg).
    dark_surface_hover: &'static str,
    danger_seed: &'static str,
    dark_danger: &'static str,
    dark_danger_hover: &'static str,
    dark_danger_pressed: &'static str,
    success_seed: &'static str,
    dark_success: &'static str,
    warning_seed: &'static str,
    dark_warning: &'static str,
}

const DEFAULT_SEED_DARK_MAP: SeedDarkMap = SeedDarkMap {
    primary_seed: "#FF1677FF",
    dark_primary: "#FF1668DC",
    dark_primary_hover: "#FF3C89E8",
    dark_primary_pressed: "#FF1554AD",
    dark_surface_hover: "#FF111A2C",
    danger_seed: "#FFFF4D4F",
    dark_danger: "#FFDC4446",
    dark_danger_hover: "#FFE86E6B",
    dark_danger_pressed: "#FFAD3030",
    success_seed: "#FF52C41A",
    dark_success: "#FF49AA19",
    warning_seed: "#FFFAAD14",
    dark_warning: "#FFD89614",
};

/// Normalize #AARRGGBB / #RRGGBB to uppercase #AARRGGBB for comparison.
pub fn normalize_argb_hex(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if !s.starts_with('#') {
        return Err(format!("hex must start with #: {raw}"));
    }
    let body = &s[1..];
    let upper = body.to_ascii_uppercase();
    match upper.len() {
        8 => Ok(format!("#{upper}")),
        6 => Ok(format!("#FF{upper}")),
        _ => Err(format!("unsupported hex length in {raw} (want #RRGGBB or #AARRGGBB)")),
    }
}

fn replace_alpha(argb: &str, alpha: u8) -> Result<String, String> {
    let n = normalize_argb_hex(argb)?;
    Ok(format!("#{alpha:02X}{}", &n[3..]))
}

/// Derive Dark Map Token hex map from Light theme colors (Seed-aware snapshot table).
pub fn derive_dark_map_from_light(
    light: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let primary = normalize_argb_hex(require_key(light, "Color.Primary")?)?;
    let danger = normalize_argb_hex(require_key(light, "Color.Danger")?)?;
    let success = normalize_argb_hex(require_key(light, "Color.Success")?)?;
    let warning = normalize_argb_hex(require_key(light, "Color.Warning")?)?;

    let m = &DEFAULT_SEED_DARK_MAP;
    if primary != m.primary_seed
        || danger != m.danger_seed
        || success != m.success_seed
        || warning != m.warning_seed
    {
        return Err(format!(
            "Light Seed set not in DEFAULT_SEED_DARK_MAP (extend dark_map_derive, do not invent runtime darkAlgorithm).\n\
             got Primary={primary} Danger={danger} Success={success} Warning={warning}\n\
             expected Primary={} Danger={} Success={} Warning={}",
            m.primary_seed, m.danger_seed, m.success_seed, m.warning_seed
        ));
    }

    let focus_ring = replace_alpha(m.dark_primary, 0x66)?;
    let text_selection = replace_alpha(m.dark_primary, 0x40)?;

    let pairs = [
        ("Color.Background", "#FF000000".to_string()),
        ("Color.Surface", "#FF141414".to_string()),
        ("Color.Border", "#FF424242".to_string()),
        ("Color.Border.Disabled", "#FF424242".to_string()),
        ("Color.Text.Primary", "#D9FFFFFF".to_string()),
        ("Color.Text.Secondary", "#A6FFFFFF".to_string()),
        ("Color.Primary", m.dark_primary.to_string()),
        ("Color.Primary.Hover", m.dark_primary_hover.to_string()),
        ("Color.Primary.Pressed", m.dark_primary_pressed.to_string()),
        ("Color.Focus.Ring", focus_ring),
        ("Color.Disabled.Fill", "#FF1F1F1F".to_string()),
        ("Color.Disabled.Text", "#FF595959".to_string()),
        ("Color.Text.OnAccent", "#FFFFFFFF".to_string()),
        ("Color.Transparent", "#00000000".to_string()),
        ("Color.Surface.Hover", m.dark_surface_hover.to_string()),
        ("Color.Surface.Stripe", "#FF1F1F1F".to_string()),
        ("Color.Slider.Track", "#FF303030".to_string()),
        ("Color.Scroll.Track", "#FF1F1F1F".to_string()),
        ("Color.Scroll.Thumb", "#FF595959".to_string()),
        ("Color.Scroll.Thumb.Hover", "#FF8C8C8C".to_string()),
        ("Color.Scroll.Thumb.Pressed", "#FFBFBFBF".to_string()),
        ("Color.Text.Placeholder", "#FF595959".to_string()),
        ("Color.Overlay", "#73000000".to_string()),
        ("Color.Danger", m.dark_danger.to_string()),
        ("Color.Danger.Hover", m.dark_danger_hover.to_string()),
        ("Color.Danger.Pressed", m.dark_danger_pressed.to_string()),
        ("Color.Success", m.dark_success.to_string()),
        ("Color.Warning", m.dark_warning.to_string()),
        ("Color.Text.Selection", text_selection),
        ("Color.Image.Fill", "#FF303030".to_string()),
        ("Color.Image.Border", "#FF8C8C8C".to_string()),
        ("Color.Text.Highlight", "#FF5C4B1F".to_string()),
    ];

    let mut out = BTreeMap::new();
    for (key, hex) in pairs {
        out.insert(key.to_string(), hex);
    }
    validate_derived_key_set(&out)?;
    Ok(out)
}

fn require_key<'a>(map: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    map.get(key)
        .map(|s| s.as_str())
        .ok_or_else(|| format!("Light theme missing Seed key {key}"))
}

fn validate_derived_key_set(map: &BTreeMap<String, String>) -> Result<(), String> {
    let expected: Vec<&str> = KEY_TO_FIELD.iter().map(|(k, _)| *k).collect();
    for key in &expected {
        if !map.contains_key(*key) {
            return Err(format!("derived Dark missing key {key}"));
        }
    }
    if map.len() != KEY_TO_FIELD.len() {
        return Err(format!(
            "derived Dark key count {} != expected {}",
            map.len(),
            KEY_TO_FIELD.len()
        ));
    }
    Ok(())
}

/// Emit Dark.arml text from a derived color map (stable KEY_TO_FIELD order).
pub fn emit_dark_arml(colors: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<!-- Built-in Dark theme — derived from Light Seed via arc-ui::dark_map_derive\n");
    out.push_str("     (Ant Design 6.x darkAlgorithm map token snapshot; seed colorPrimary=#1677ff).\n");
    out.push_str("     Neutrals: generateNeutralColorPalettes(bgBase=#000, textBase=#fff).\n");
    out.push_str("     Primary map: dark generate(#1677ff) -> #1668dc / hover #3c89e8 / active #1554ad.\n");
    out.push_str("     Honest: build-time Seed->Map derivation only; full darkAlgorithm runtime = P3.\n");
    out.push_str("     Token alignment only — not pixel DOM/CSS clone. Same key set as Light.arml.\n");
    out.push_str("     Regenerate: scripts/ui-theme/derive-dark-from-light.ps1 -->\n");
    out.push_str("<ResourceDictionary\n");
    out.push_str("    xmlns=\"http://schemas.arc.dev/winfx/2026\"\n");
    out.push_str("    xmlns:x=\"http://schemas.arc.dev/xaml\">\n");
    for (key, _) in KEY_TO_FIELD {
        let hex = &colors[*key];
        let comment = dark_key_comment(key);
        if !comment.is_empty() {
            out.push_str(&format!("    <!-- {comment} -->\n"));
        }
        out.push_str(&format!("    <Color x:Key=\"{key}\" Value=\"{hex}\"/>\n"));
    }
    out.push_str("</ResourceDictionary>\n");
    out
}

fn dark_key_comment(key: &str) -> &'static str {
    match key {
        "Color.Background" => "colorBgLayout getSolidColor(#000, 0)",
        "Color.Surface" => "colorBgContainer getSolidColor(#000, 8)",
        "Color.Border" => "colorBorder getSolidColor(#000, 26)",
        "Color.Border.Disabled" => "colorBorderDisabled (Ant Design 6; Dark same band as colorBorder)",
        "Color.Text.Primary" => "colorText rgba(255,255,255,0.85)",
        "Color.Text.Secondary" => "colorTextSecondary rgba(255,255,255,0.65)",
        "Color.Primary" => "dark generate(colorPrimary) step-6",
        "Color.Primary.Hover" => "dark generate(colorPrimary) hover",
        "Color.Primary.Pressed" => "dark generate(colorPrimary) active",
        "Color.Focus.Ring" => "dark colorPrimary @ 40% (focus ring)",
        "Color.Disabled.Fill" => "colorFillQuaternary rgba(255,255,255,0.04) solid",
        "Color.Disabled.Text" => "colorTextQuaternary rgba(255,255,255,0.25) solid",
        "Color.Text.OnAccent" => "colorTextLightSolid",
        "Color.Transparent" => "transparent",
        "Color.Surface.Hover" => "colorPrimaryBg (dark primary-1 approx)",
        "Color.Surface.Stripe" => "colorFillQuaternary solid",
        "Color.Slider.Track" => "colorFillSecondary rgba(255,255,255,0.12) solid",
        "Color.Scroll.Track" => "colorFillQuaternary solid",
        "Color.Scroll.Thumb" => "neutral scroll thumb",
        "Color.Scroll.Thumb.Hover" => "neutral scroll thumb hover",
        "Color.Scroll.Thumb.Pressed" => "neutral scroll thumb pressed",
        "Color.Text.Placeholder" => "colorTextPlaceholder solid",
        "Color.Overlay" => "colorBgMask rgba(0,0,0,0.45)",
        "Color.Danger" => "colorError (dark map) -> Danger",
        "Color.Danger.Hover" => "colorErrorHover (dark)",
        "Color.Danger.Pressed" => "colorErrorActive (dark)",
        "Color.Success" => "colorSuccess (dark map)",
        "Color.Warning" => "colorWarning (dark map)",
        "Color.Text.Selection" => "dark colorPrimary @ 25%",
        "Color.Image.Fill" => "Image decode-fail / empty-source fill",
        "Color.Image.Border" => "Image decode-fail / empty-source border",
        "Color.Text.Highlight" => "DrawText highlight backdrop",
        _ => "",
    }
}

/// Load Light.arml, derive Dark map, return ARML text.
pub fn generate_dark_arml_from_light(repo_root: &Path) -> Result<String, String> {
    let light = load_theme_colors(&repo_root.join(LIGHT_ARML_REL))?;
    let dark = derive_dark_map_from_light(&light)?;
    Ok(emit_dark_arml(&dark))
}

/// Write derived Dark.arml under the repo root.
pub fn write_dark_arml_from_light(repo_root: &Path) -> Result<PathBuf, String> {
    let text = generate_dark_arml_from_light(repo_root)?;
    let out = repo_root.join(DARK_ARML_REL);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(&out, text).map_err(|e| format!("write {}: {e}", out.display()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_theme_gen::load_theme_colors;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    #[test]
    fn default_light_seed_derives_known_dark_primary() {
        let light = load_theme_colors(&repo_root().join(LIGHT_ARML_REL)).expect("Light");
        let dark = derive_dark_map_from_light(&light).expect("derive");
        assert_eq!(dark.get("Color.Primary").map(String::as_str), Some("#FF1668DC"));
        assert_eq!(
            dark.get("Color.Primary.Hover").map(String::as_str),
            Some("#FF3C89E8")
        );
        assert_eq!(
            dark.get("Color.Focus.Ring").map(String::as_str),
            Some("#661668DC")
        );
        assert_eq!(
            dark.get("Color.Text.Selection").map(String::as_str),
            Some("#401668DC")
        );
    }

    #[test]
    fn foreign_seed_fails_loudly() {
        let mut light = load_theme_colors(&repo_root().join(LIGHT_ARML_REL)).expect("Light");
        light.insert("Color.Primary".to_string(), "#FFFF0000".to_string());
        let err = derive_dark_map_from_light(&light).expect_err("must reject");
        assert!(err.contains("DEFAULT_SEED_DARK_MAP"));
    }
}
