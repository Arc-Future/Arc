//! UI-P2：从 `std/UI/Core/Themes/{Light,Dark}.arml` 生成 `BuiltInTheme.Colors.g.as`。
//!
//! ARML 为内置色值唯一权威源；生成物由契约测试与源同步，禁止在
//! `BuiltInTheme.as` 再写 hex 字面量。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ast::ResourceDictionaryDef;
use crate::parser::Parser;

/// 相对仓库根的 Light 主题 ARML。
pub const LIGHT_ARML_REL: &str = "std/UI/Core/Themes/Light.arml";
/// 相对仓库根的 Dark 主题 ARML。
pub const DARK_ARML_REL: &str = "std/UI/Core/Themes/Dark.arml";
/// 相对仓库根的控件隐式 Style ARML。
pub const CONTROLS_ARML_REL: &str = "std/UI/Core/Themes/Controls.arml";
/// 相对仓库根的生成色值源。
pub const COLORS_G_AS_REL: &str = "std/UI/Core/Styling/BuiltInTheme.Colors.g.as";

/// `x:Key` → `BuiltInTheme` 字段名（须与 `BuiltInTheme.as` const 一一对应）。
const KEY_TO_FIELD: &[(&str, &str)] = &[
    ("Color.Background", "Background"),
    ("Color.Surface", "Surface"),
    ("Color.Border", "Border"),
    ("Color.Text.Primary", "TextPrimary"),
    ("Color.Text.Secondary", "TextSecondary"),
    ("Color.Primary", "Primary"),
    ("Color.Primary.Hover", "PrimaryHover"),
    ("Color.Primary.Pressed", "PrimaryPressed"),
    ("Color.Focus.Ring", "FocusRing"),
    ("Color.Disabled.Fill", "DisabledFill"),
    ("Color.Disabled.Text", "DisabledText"),
    ("Color.Text.OnAccent", "TextOnAccent"),
    ("Color.Transparent", "Transparent"),
    ("Color.Surface.Hover", "SurfaceHover"),
    ("Color.Surface.Stripe", "SurfaceStripe"),
    ("Color.Slider.Track", "SliderTrack"),
    ("Color.Scroll.Track", "ScrollTrack"),
    ("Color.Scroll.Thumb", "ScrollThumb"),
    ("Color.Scroll.Thumb.Hover", "ScrollThumbHover"),
    ("Color.Scroll.Thumb.Active", "ScrollThumbActive"),
    ("Color.Placeholder", "Placeholder"),
    ("Color.Overlay", "Overlay"),
    ("Color.Negative", "Negative"),
    ("Color.Accent.Gradient.A", "AccentGradientA"),
    ("Color.Accent.Gradient.B", "AccentGradientB"),
];

/// 从仓库根读取 Light/Dark ARML，生成 `BuiltInTheme.Colors.g.as` 全文。
pub fn generate_colors_g_as(repo_root: &Path) -> Result<String, String> {
    let light = load_theme_colors(&repo_root.join(LIGHT_ARML_REL))?;
    let dark = load_theme_colors(&repo_root.join(DARK_ARML_REL))?;
    validate_key_set("Light", &light)?;
    validate_key_set("Dark", &dark)?;
    if light.keys().ne(dark.keys()) {
        return Err(format!(
            "Light/Dark key sets differ: light={:?} dark={:?}",
            light.keys().collect::<Vec<_>>(),
            dark.keys().collect::<Vec<_>>()
        ));
    }
    Ok(emit_colors_g_as(&light, &dark))
}

/// 将生成物写入 `std/UI/Core/Styling/BuiltInTheme.Colors.g.as`。
pub fn write_colors_g_as(repo_root: &Path) -> Result<PathBuf, String> {
    let text = generate_colors_g_as(repo_root)?;
    let out = repo_root.join(COLORS_G_AS_REL);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(&out, text).map_err(|e| format!("write {}: {e}", out.display()))?;
    Ok(out)
}

/// 解析主题 ARML，返回有序 `(x:Key → hex)`。
pub fn load_theme_colors(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Parser::parse(&src).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let dict = ResourceDictionaryDef::from_element(&doc.root).ok_or_else(|| {
        format!(
            "{}: root must be ResourceDictionary (got {})",
            path.display(),
            doc.root.name
        )
    })?;
    let mut map = BTreeMap::new();
    for entry in &dict.entries {
        let ty = entry.type_name.as_str();
        if ty != "Color" && ty != "Brush" && ty != "SolidColorBrush" {
            return Err(format!(
                "{}: unexpected entry type `{ty}` for key `{}` (color themes only)",
                path.display(),
                entry.key
            ));
        }
        let Some(value) = &entry.value else {
            return Err(format!(
                "{}: Color `{}` missing Value",
                path.display(),
                entry.key
            ));
        };
        if map.insert(entry.key.to_string(), value.clone()).is_some() {
            return Err(format!(
                "{}: duplicate x:Key `{}`",
                path.display(),
                entry.key
            ));
        }
    }
    Ok(map)
}

fn validate_key_set(label: &str, map: &BTreeMap<String, String>) -> Result<(), String> {
    let expected: Vec<&str> = KEY_TO_FIELD.iter().map(|(k, _)| *k).collect();
    for key in &expected {
        if !map.contains_key(*key) {
            return Err(format!("{label} theme missing key `{key}`"));
        }
    }
    for key in map.keys() {
        if KEY_TO_FIELD.iter().all(|(k, _)| k != key) {
            return Err(format!(
                "{label} theme has unknown key `{key}` (add BuiltInTheme const + KEY_TO_FIELD)"
            ));
        }
    }
    if map.len() != KEY_TO_FIELD.len() {
        return Err(format!(
            "{label} theme key count {} != expected {}",
            map.len(),
            KEY_TO_FIELD.len()
        ));
    }
    Ok(())
}

fn field_for_key(key: &str) -> &'static str {
    KEY_TO_FIELD
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, f)| *f)
        .expect("validated key")
}

fn emit_fill_method(method: &str, arml_rel: &str, colors: &BTreeMap<String, String>) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "    /// <summary>从 `{arml_rel}` 生成的色值注册（勿手改；改 Themes/*.arml 后再生）。</summary>\n"
    ));
    body.push_str(&format!(
        "    public static void {method}(ResourceDictionary d) {{\n"
    ));
    // Stable order: KEY_TO_FIELD declaration order (not BTreeMap alpha).
    for (key, _) in KEY_TO_FIELD {
        let hex = &colors[*key];
        let field = field_for_key(key);
        body.push_str(&format!(
            "        d.Add(BuiltInTheme.{field}, ResourceValue.Brush(Brushes.Parse(\"{hex}\")));\n"
        ));
    }
    body.push_str("    }\n");
    body
}

fn emit_colors_g_as(light: &BTreeMap<String, String>, dark: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    out.push_str("// <auto-generated>\n");
    out.push_str("// UI-P2: generated from std/UI/Core/Themes/Light.arml + Dark.arml.\n");
    out.push_str("// Do not edit by hand. Regenerate:\n");
    out.push_str("//   UPDATE_BUILTIN_THEME=1 cargo test -p arc-ui --test design_tokens_contract -- builtin_theme_colors_g_as_in_sync\n");
    out.push_str("// </auto-generated>\n\n");
    out.push_str("namespace Arc.UI.Styling;\n\n");
    out.push_str("using Arc.UI.Media;\n\n");
    out.push_str("/// <summary>内置 Light/Dark 色值填充（ARML 编译期扁平产物）。</summary>\n");
    out.push_str("internal class BuiltInThemeColors {\n");
    out.push_str(&emit_fill_method("FillLightColors", LIGHT_ARML_REL, light));
    out.push('\n');
    out.push_str(&emit_fill_method("FillDarkColors", DARK_ARML_REL, dark));
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    }

    #[test]
    fn light_dark_arml_parse_and_match_key_set() {
        let root = repo_root();
        let light = load_theme_colors(&root.join(LIGHT_ARML_REL)).expect("Light.arml");
        let dark = load_theme_colors(&root.join(DARK_ARML_REL)).expect("Dark.arml");
        validate_key_set("Light", &light).unwrap();
        validate_key_set("Dark", &dark).unwrap();
        assert_eq!(light.len(), KEY_TO_FIELD.len());
        assert_ne!(
            light.get("Color.Primary"),
            dark.get("Color.Primary"),
            "Light/Dark Primary must differ"
        );
    }
}
