//! RFC 037 §4 / UI-P2: BuiltInTheme 与 C 光栅 Light 默认值一致性契约。
//!
//! 色值权威源为 `std/UI/Core/Themes/{Light,Dark}.arml`（生成物
//! `BuiltInTheme.Colors.g.as` 须与 ARML 同步。几何/motion 仍在 BuiltInTheme.as。

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn read_file(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[test]
fn builtin_theme_arml_sources_exist() {
    let root = repo_root();
    for rel in [
        arc_ui::LIGHT_ARML_REL,
        arc_ui::DARK_ARML_REL,
        arc_ui::CONTROLS_ARML_REL,
        arc_ui::COLORS_G_AS_REL,
    ] {
        assert!(
            root.join(rel).is_file(),
            "missing UI-P2 theme source: {rel}"
        );
    }
}

#[test]
fn builtin_theme_colors_g_as_in_sync() {
    let root = repo_root();
    let expected = arc_ui::generate_colors_g_as(&root).expect("generate from ARML");
    let path = root.join(arc_ui::COLORS_G_AS_REL);
    if std::env::var("UPDATE_BUILTIN_THEME").is_ok() {
        std::fs::write(&path, &expected).expect("write Colors.g.as");
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {}: {e}; regenerate with UPDATE_BUILTIN_THEME=1 cargo test -p arc-ui --test design_tokens_contract -- builtin_theme_colors_g_as_in_sync",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "BuiltInTheme.Colors.g.as out of sync with Themes/*.arml; regenerate with UPDATE_BUILTIN_THEME=1"
    );
}

#[test]
fn builtin_theme_as_has_no_color_brush_literals() {
    let tokens = read_file("std/UI/Core/Styling/BuiltInTheme.as");
    assert!(
        !tokens.contains("brush(\"#"),
        "BuiltInTheme.as must not embed color brush(\"#…\") literals (UI-P2 ARML source)"
    );
    assert!(
        !tokens.contains("Brushes.Parse(\"#"),
        "BuiltInTheme.as must not embed Brushes.Parse hex (use BuiltInThemeColors from ARML)"
    );
    assert!(tokens.contains("BuiltInThemeColors.FillLightColors(d)"));
    assert!(tokens.contains("BuiltInThemeColors.FillDarkColors(d)"));
    assert!(tokens.contains("BuiltInTheme.FillNonColor(d)"));
}

#[test]
fn builtin_theme_header_matches_arc_tokens() {
    let header = read_file("crates/runtime-ui/platform/common/rt_ui_design_tokens.h");
    let tokens = read_file("std/UI/Core/Styling/BuiltInTheme.as");
    let light_arml = read_file(arc_ui::LIGHT_ARML_REL);
    let colors_g = read_file(arc_ui::COLORS_G_AS_REL);

    // (C 宏, 键常量, 资源 key, Light hex)：C 头 / 键常量 / ARML / 生成物对齐。
    let pairs = [
        (
            "RT_UI_COLOR_BACKGROUND",
            "Background",
            "Color.Background",
            "#FFFAFAFA",
        ),
        (
            "RT_UI_COLOR_SURFACE",
            "Surface",
            "Color.Surface",
            "#FFFFFFFF",
        ),
        ("RT_UI_COLOR_BORDER", "Border", "Color.Border", "#FFE6E6EC"),
        (
            "RT_UI_COLOR_TEXT_PRIMARY",
            "TextPrimary",
            "Color.Text.Primary",
            "#FF1A1A1A",
        ),
        (
            "RT_UI_COLOR_TEXT_SECONDARY",
            "TextSecondary",
            "Color.Text.Secondary",
            "#FF8A8A93",
        ),
        (
            "RT_UI_COLOR_PRIMARY",
            "Primary",
            "Color.Primary",
            "#FF4F46E5",
        ),
        (
            "RT_UI_COLOR_PRIMARY_HOVER",
            "PrimaryHover",
            "Color.Primary.Hover",
            "#FF6366F1",
        ),
        (
            "RT_UI_COLOR_PRIMARY_PRESSED",
            "PrimaryPressed",
            "Color.Primary.Pressed",
            "#FF4338CA",
        ),
        (
            "RT_UI_COLOR_FOCUS_RING",
            "FocusRing",
            "Color.Focus.Ring",
            "#734F46E5",
        ),
        (
            "RT_UI_COLOR_DISABLED_FILL",
            "DisabledFill",
            "Color.Disabled.Fill",
            "#FFF3F3F6",
        ),
        (
            "RT_UI_COLOR_DISABLED_TEXT",
            "DisabledText",
            "Color.Disabled.Text",
            "#FFB8B8C0",
        ),
        (
            "RT_UI_COLOR_TEXT_ON_PRIMARY",
            "TextOnAccent",
            "Color.Text.OnAccent",
            "#FFFFFFFF",
        ),
        (
            "RT_UI_COLOR_SLIDER_TRACK",
            "SliderTrack",
            "Color.Slider.Track",
            "#FFE0E0E6",
        ),
        (
            "RT_UI_COLOR_SURFACE_HOVER",
            "SurfaceHover",
            "Color.Surface.Hover",
            "#FFF4F4FF",
        ),
        (
            "RT_UI_COLOR_SCROLL_TRACK",
            "ScrollTrack",
            "Color.Scroll.Track",
            "#FFF0F0F0",
        ),
        (
            "RT_UI_COLOR_SCROLL_THUMB",
            "ScrollThumb",
            "Color.Scroll.Thumb",
            "#FF8A8A93",
        ),
        (
            "RT_UI_COLOR_SCROLL_THUMB_HOVER",
            "ScrollThumbHover",
            "Color.Scroll.Thumb.Hover",
            "#FF6E6E7A",
        ),
        (
            "RT_UI_COLOR_SCROLL_THUMB_ACTIVE",
            "ScrollThumbActive",
            "Color.Scroll.Thumb.Active",
            "#FF4A4A55",
        ),
    ];

    for (c_macro, as_field, key, hex) in pairs {
        assert!(header.contains(c_macro), "missing C token {c_macro}");
        assert!(
            tokens.contains(&format!("public const string {as_field} = \"{key}\"")),
            "missing string key constant {as_field} = {key}"
        );
        assert!(
            light_arml.contains(&format!("x:Key=\"{key}\""))
                && light_arml.contains(&format!("Value=\"{hex}\"")),
            "Light.arml missing {key} = {hex}"
        );
        assert!(
            colors_g.contains(&format!(
                "d.Add(BuiltInTheme.{as_field}, ResourceValue.Brush(Brushes.Parse(\"{hex}\")))"
            )),
            "Colors.g.as missing Light {as_field} = {hex}"
        );
    }

    assert!(header.contains("#define RT_UI_RADIUS_CONTROL          6"));
    assert!(tokens.contains("public const string RadiusControl = \"Radius.Control\""));
    assert!(header.contains("#define RT_UI_BUTTON_MIN_HEIGHT         32"));
    assert!(header.contains("#define RT_UI_INPUT_MIN_HEIGHT          32"));
}

#[test]
fn builtin_theme_declares_light_dark_resource_dictionaries() {
    let tokens = read_file("std/UI/Core/Styling/BuiltInTheme.as");
    assert!(tokens.contains("public static ResourceDictionary CreateLight()"));
    assert!(tokens.contains("public static ResourceDictionary CreateDark()"));
    assert!(tokens.contains("BuiltInTheme.FillNonColor(d)"));
}

/// RFC 037 §4 属性值继承（2026-08-17）：字体为环境 DP，隐式样式体系已删——
/// `ThemeDictionary.AddImplicitStyles` 随字体 Setter 一并移除；`Controls.arml`
/// 仅承载 chrome（字体禁入），全局字体默认单一源 = `Control` DP 默认值。
#[test]
fn theme_dictionary_holds_light_dark_without_implicit_styles() {
    let theme = read_file("std/UI/Core/Styling/ThemeDictionary.as");
    let controls = read_file(arc_ui::CONTROLS_ARML_REL);
    assert!(theme.contains("Themes.Add(\"Light\", BuiltInTheme.CreateLight())"));
    assert!(theme.contains("Themes.Add(\"Dark\", BuiltInTheme.CreateDark())"));
    assert!(
        !theme.contains("AddImplicitStyles"),
        "implicit-style pipeline was removed with ambient font DPs (RFC 037 §4)"
    );
    assert!(
        controls.contains("TargetType=\"Window\""),
        "Controls.arml carries chrome-only implicit style (Window placeholder)"
    );
    for font_prop in ["FontFamily", "FontSize", "FontWeight", "Foreground"] {
        assert!(
            !controls.contains(&format!("Property=\"{font_prop}\"")),
            "Controls.arml must not carry {font_prop} setter: fonts are ambient DPs (RFC 037 §4)"
        );
    }
    assert!(!theme.contains("static ThemeDictionary Current"));
}

#[test]
fn application_is_single_resolution_root() {
    let app = read_file("std/UI/Core/Components/Application.as");
    assert!(app.contains("public static Application Current"));
    assert!(app.contains("public ThemeDictionary ThemeDictionaries"));
    assert!(app.contains("Resources.MergedDictionaries.Add(ThemeDictionaries.Active)"));
    assert!(app.contains("public string ResolveColor(string key)"));
    assert!(app.contains("public double ResolveNumber(string key)"));
    assert!(app.contains("public void SwitchTheme(string name)"));
}
