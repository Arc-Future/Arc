//! UI-P2：从 `std/UI/Core/Themes/{Light,Dark}.arml` 生成 `BuiltInTheme.Colors.g.as`；
//! 从 `Themes/Controls.arml`（MergedDictionaries → `Controls/*.arml`）生成
//! `BuiltInTheme.Styles.g.as`。
//!
//! ARML 为内置色值与隐式 Style 唯一权威源；生成物由契约测试与源同步，禁止在
//! `BuiltInTheme.as` 再写 hex 字面量或手写隐式 Style 双源。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ast::{
    AttributeValue, MarkupKind, ResourceDictionaryDef, StyleDef,
};
use crate::parser::Parser;

/// 相对仓库根的 Light 主题 ARML。
pub const LIGHT_ARML_REL: &str = "std/UI/Core/Themes/Light.arml";
/// 相对仓库根的 Dark 主题 ARML。
pub const DARK_ARML_REL: &str = "std/UI/Core/Themes/Dark.arml";
/// 相对仓库根的控件隐式 Style 聚合 ARML。
pub const CONTROLS_ARML_REL: &str = "std/UI/Core/Themes/Controls.arml";
/// 相对仓库根的控件 Style 分文件目录。
pub const CONTROLS_DIR_REL: &str = "std/UI/Core/Themes/Controls";
/// 相对仓库根的生成色值源。
pub const COLORS_G_AS_REL: &str = "std/UI/Core/Styling/BuiltInTheme.Colors.g.as";
/// 相对仓库根的生成隐式 Style 源。
pub const STYLES_G_AS_REL: &str = "std/UI/Core/Styling/BuiltInTheme.Styles.g.as";

/// 聚合必须索引的控件 Style 分文件（相对 Themes/）。
pub const CONTROL_STYLE_SOURCES: &[&str] = &[
    "Controls/Shared.arml",
    "Controls/Window.arml",
    "Controls/Button.arml",
    "Controls/ToggleButton.arml",
    "Controls/CheckBox.arml",
    "Controls/RadioButton.arml",
    "Controls/TextBox.arml",
    "Controls/PasswordBox.arml",
    "Controls/Border.arml",
    "Controls/ComboBox.arml",
    "Controls/ProgressBar.arml",
    "Controls/Slider.arml",
];

/// `x:Key` → `BuiltInTheme` 字段名（须与 `BuiltInTheme.as` const 一一对应）。
pub(crate) const KEY_TO_FIELD: &[(&str, &str)] = &[
    ("Color.Background", "Background"),
    ("Color.Surface", "Surface"),
    ("Color.Border", "Border"),
    ("Color.Border.Disabled", "BorderDisabled"),
    ("Color.Text.Primary", "TextPrimary"),
    ("Color.Text.Secondary", "TextSecondary"),
    ("Color.Primary", "Primary"),
    ("Color.Primary.Hover", "PrimaryHover"),
    ("Color.Primary.Pressed", "PrimaryPressed"),
    ("Color.Focus.Ring", "FocusRing"),
    ("Color.Disabled.Fill", "DisabledFill"),
    ("Color.Disabled.Text", "DisabledText"),
    ("Color.Text.OnAccent", "TextOnAccent"),
    ("Color.Text.Placeholder", "TextPlaceholder"),
    ("Color.Transparent", "Transparent"),
    ("Color.Surface.Hover", "SurfaceHover"),
    ("Color.Surface.Stripe", "SurfaceStripe"),
    ("Color.Slider.Track", "SliderTrack"),
    ("Color.Scroll.Track", "ScrollTrack"),
    ("Color.Scroll.Thumb", "ScrollThumb"),
    ("Color.Scroll.Thumb.Hover", "ScrollThumbHover"),
    ("Color.Scroll.Thumb.Pressed", "ScrollThumbPressed"),
    ("Color.Overlay", "Overlay"),
    ("Color.Danger", "Danger"),
    ("Color.Danger.Hover", "DangerHover"),
    ("Color.Danger.Pressed", "DangerPressed"),
    ("Color.Success", "Success"),
    ("Color.Warning", "Warning"),
    ("Color.Text.Selection", "TextSelection"),
    ("Color.Image.Fill", "ImageFill"),
    ("Color.Image.Border", "ImageBorder"),
    ("Color.Text.Highlight", "TextHighlight"),
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

/// 解析 Controls 聚合 ARML（含 Source 合并），返回隐式 Style 列表（声明序）。
pub fn load_controls_styles(repo_root: &Path) -> Result<Vec<StyleDef>, String> {
    let themes_dir = repo_root.join("std/UI/Core/Themes");
    let root_path = repo_root.join(CONTROLS_ARML_REL);
    let dict = load_resource_dictionary(&root_path)?;
    let mut styles = Vec::new();
    collect_styles_resolved(&dict, &themes_dir, &mut styles, &mut Vec::new())?;
    if styles.is_empty() {
        return Err(format!(
            "{}: no Style after resolving MergedDictionaries",
            CONTROLS_ARML_REL
        ));
    }
    for style in &styles {
        let key = style.key.as_deref().unwrap_or("");
        let target = style.target_type.as_deref().unwrap_or("");
        let is_keyed = !key.is_empty();
        // 隐式 Style 必须有 TargetType；全局短键（Shared）可无 TargetType。
        if !is_keyed && target.is_empty() {
            return Err("Controls implicit style missing TargetType".to_string());
        }
        // 色态标识（Primary…）与 BasedOn 继承（隐式 → Medium）允许无本地 Setter。
        let leaf = key.rsplit('.').next().unwrap_or(key);
        let is_chrome_marker = matches!(
            leaf,
            "Primary" | "Default" | "Dashed" | "Text" | "Link" | "Danger" | "Ghost"
        );
        let has_based_on = style.based_on.is_some();
        if style.setters.is_empty() && !is_chrome_marker && !has_based_on {
            return Err(format!(
                "Controls Style key={key:?} TargetType={target} has no Setters (empty placeholder forbidden; use BasedOn or real Setters)"
            ));
        }
        if !is_keyed {
            for setter in &style.setters {
                let prop = setter.property.as_str();
                if matches!(prop, "FontFamily" | "FontSize" | "FontWeight" | "Foreground") {
                    return Err(format!(
                        "Controls Style TargetType={target} must not carry {prop} setter"
                    ));
                }
            }
        }
    }
    Ok(styles)
}

/// 从 Controls ARML 生成 `BuiltInTheme.Styles.g.as` 全文。
pub fn generate_styles_g_as(repo_root: &Path) -> Result<String, String> {
    let styles = load_controls_styles(repo_root)?;
    Ok(emit_styles_g_as(&styles))
}

/// 将生成物写入 `std/UI/Core/Styling/BuiltInTheme.Styles.g.as`。
pub fn write_styles_g_as(repo_root: &Path) -> Result<PathBuf, String> {
    let text = generate_styles_g_as(repo_root)?;
    let out = repo_root.join(STYLES_G_AS_REL);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(&out, text).map_err(|e| format!("write {}: {e}", out.display()))?;
    Ok(out)
}

fn load_resource_dictionary(path: &Path) -> Result<ResourceDictionaryDef, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc = Parser::parse(&src).map_err(|e| format!("parse {}: {e}", path.display()))?;
    ResourceDictionaryDef::from_element(&doc.root).ok_or_else(|| {
        format!(
            "{}: root must be ResourceDictionary (got {})",
            path.display(),
            doc.root.name
        )
    })
}

fn collect_styles_resolved(
    dict: &ResourceDictionaryDef,
    themes_dir: &Path,
    out: &mut Vec<StyleDef>,
    stack: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for style in &dict.styles {
        out.push(style.clone());
    }
    for merged in &dict.merged {
        if let Some(source) = merged.source.as_deref() {
            let path = themes_dir.join(source);
            let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
            if stack.iter().any(|p| p == &canon) {
                return Err(format!(
                    "ResourceDictionary Source cycle involving {}",
                    source
                ));
            }
            stack.push(canon);
            let child = load_resource_dictionary(&path)?;
            collect_styles_resolved(&child, themes_dir, out, stack)?;
            stack.pop();
        } else {
            collect_styles_resolved(merged, themes_dir, out, stack)?;
        }
    }
    Ok(())
}

fn emit_styles_g_as(styles: &[StyleDef]) -> String {
    let mut out = String::new();
    out.push_str("// <auto-generated>\n");
    out.push_str("// UI-P2: generated from std/UI/Core/Themes/Controls.arml + Controls/*.arml.\n");
    out.push_str("// Do not edit by hand. Regenerate:\n");
    out.push_str("//   UPDATE_BUILTIN_THEME=1 cargo test -p arc-ui --test design_tokens_contract -- builtin_theme_styles_g_as_in_sync\n");
    out.push_str("// </auto-generated>\n\n");
    out.push_str("namespace Arc.UI.Styling;\n\n");
    out.push_str("using Arc.Collections;\n\n");
    out.push_str("/// <summary>内置控件隐式 Style（Controls.arml 编译期扁平产物）。</summary>\n");
    out.push_str("internal class BuiltInThemeStyles {\n");
    out.push_str("    /// <summary>从 Themes/Controls.arml 合并生成的隐式 Style 字典（勿手改）。</summary>\n");
    out.push_str("    public static ResourceDictionary Create() {\n");
    out.push_str("        ResourceDictionary d = new ResourceDictionary();\n");
    for (i, style) in styles.iter().enumerate() {
        let style_var = format!("_style_{i}");
        let target = style.target_type.as_deref().unwrap_or("");
        let key = style.key.as_deref().unwrap_or("");
        out.push_str(&format!("        Style {style_var} = new Style();\n"));
        out.push_str(&format!(
            "        {style_var}.TargetType = \"{}\";\n",
            escape_arc_string(target)
        ));
        out.push_str(&format!(
            "        {style_var}.Key = \"{}\";\n",
            escape_arc_string(key)
        ));
        if let Some(based_on) = &style.based_on {
            let parent_key = match based_on {
                AttributeValue::Literal(s) => Some(s.as_str()),
                AttributeValue::MarkupExtension(ext) if ext.kind == MarkupKind::StaticResource => {
                    ext.args.first().map(|s| s.as_str())
                }
                _ => None,
            };
            if let Some(parent_key) = parent_key {
                out.push_str(&format!(
                    "        {style_var}.BasedOn = \"{}\";\n",
                    escape_arc_string(parent_key)
                ));
            }
        }
        for (si, setter) in style.setters.iter().enumerate() {
            let setter_var = format!("_setter_{i}_{si}");
            out.push_str(&format!("        Setter {setter_var} = new Setter();\n"));
            out.push_str(&format!(
                "        {setter_var}.Property = \"{}\";\n",
                escape_arc_string(setter.property.as_str())
            ));
            out.push_str(&format!(
                "        {setter_var}.Value = {};\n",
                format_style_setter_value(&setter.value)
            ));
            out.push_str(&format!(
                "        {style_var}.Setters.Add({setter_var});\n"
            ));
        }
        out.push_str(&format!("        d.AddStyle({style_var});\n"));
    }
    out.push_str("        return d;\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

fn format_style_setter_value(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Literal(val) => {
            if val == "true" || val == "false" {
                return format!("SetterValue.Boolean({val})");
            }
            if let Ok(n) = val.parse::<f64>() {
                if n.fract() == 0.0 {
                    return format!("SetterValue.Number({}.0)", n as i64);
                }
                return format!("SetterValue.Number({val})");
            }
            format!("SetterValue.String(\"{}\")", escape_arc_string(val))
        }
        AttributeValue::MarkupExtension(ext) if ext.kind == MarkupKind::StaticResource => {
            let key = ext
                .args
                .first()
                .map(|s| s.as_str())
                .unwrap_or("");
            format!(
                "SetterValue.StaticResource(\"{}\")",
                escape_arc_string(key)
            )
        }
        AttributeValue::MarkupExtension(ext) => {
            panic!(
                "unsupported markup in Controls Style Setter: {}",
                ext.kind.as_str()
            )
        }
    }
}

fn escape_arc_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
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

    #[test]
    fn controls_arml_resolves_merged_styles() {
        let root = repo_root();
        let styles = load_controls_styles(&root).expect("Controls styles");
        let targets: Vec<&str> = styles
            .iter()
            .filter_map(|s| s.target_type.as_deref())
            .collect();
        assert!(targets.contains(&"Button"));
        assert!(targets.contains(&"Window"));
        assert!(targets.contains(&"TextBox"));
        assert!(
            styles.len() >= CONTROL_STYLE_SOURCES.len(),
            "merged styles >= source files (keyed Button Styles expand count)"
        );
        assert!(styles.iter().any(|s| s.key.as_deref() == Some("Danger")));
        assert!(styles.iter().any(|s| s.key.as_deref() == Some("Large")));
        assert!(styles.iter().any(|s| s.key.as_deref() == Some("Small")));
        assert!(
            !styles.iter().any(|s| s.key.as_deref() == Some("Button.Large")),
            "no redundant Button.Large when Shared Large owns size+padding"
        );
        assert!(
            styles.iter().any(|s| {
                s.key.as_deref().unwrap_or("").is_empty()
                    && s.target_type.as_deref() == Some("Button")
                    && s.based_on.is_some()
            }),
            "Button implicit must BasedOn Medium"
        );
    }
}
