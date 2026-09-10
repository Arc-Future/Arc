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

/// Golden 文本比对：忽略 `core.autocrlf` 把 LF blob 检出成 CRLF 的平台差（Windows CI）。
fn norm_nl(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

#[test]
fn builtin_theme_arml_sources_exist() {
    let root = repo_root();
    for rel in [
        arc_ui::LIGHT_ARML_REL,
        arc_ui::DARK_ARML_REL,
        arc_ui::CONTROLS_ARML_REL,
        arc_ui::COLORS_G_AS_REL,
        arc_ui::STYLES_G_AS_REL,
    ] {
        assert!(
            root.join(rel).is_file(),
            "missing UI-P2 theme source: {rel}"
        );
    }
    assert!(
        root.join(arc_ui::CONTROLS_DIR_REL).is_dir(),
        "missing Controls style directory"
    );
    assert!(
        !root.join("std/UI/Core/Themes/Modules").exists(),
        "Themes/Modules must be removed (replaced by Themes/Controls)"
    );
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
        norm_nl(&actual),
        norm_nl(&expected),
        "BuiltInTheme.Colors.g.as out of sync with Themes/*.arml; regenerate with UPDATE_BUILTIN_THEME=1"
    );
}

/// Dark.arml 须可由 Light Seed 经 `dark_map_derive` 可重复再生（非第二套手写硬编码权威）。
#[test]
fn dark_arml_matches_light_seed_derivation() {
    let root = repo_root();
    let expected =
        arc_ui::generate_dark_arml_from_light(&root).expect("derive Dark from Light Seed");
    let path = root.join(arc_ui::DARK_ARML_REL);
    if std::env::var("UPDATE_DARK_THEME").is_ok() {
        std::fs::write(&path, &expected).expect("write Dark.arml");
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {}: {e}; regenerate with scripts/ui-theme/derive-dark-from-light.ps1 \
             (or UPDATE_DARK_THEME=1 cargo test -p arc-ui --test design_tokens_contract -- dark_arml_matches_light_seed_derivation)",
            path.display()
        )
    });
    assert_eq!(
        norm_nl(&actual),
        norm_nl(&expected),
        "Dark.arml out of sync with Light Seed derivation; run scripts/ui-theme/derive-dark-from-light.ps1"
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
            "#FFF5F5F5",
        ),
        (
            "RT_UI_COLOR_SURFACE",
            "Surface",
            "Color.Surface",
            "#FFFFFFFF",
        ),
        ("RT_UI_COLOR_BORDER", "Border", "Color.Border", "#FFD9D9D9"),
        (
            "RT_UI_COLOR_BORDER_DISABLED",
            "BorderDisabled",
            "Color.Border.Disabled",
            "#FFD9D9D9",
        ),
        (
            "RT_UI_COLOR_TEXT_PRIMARY",
            "TextPrimary",
            "Color.Text.Primary",
            "#E0000000",
        ),
        (
            "RT_UI_COLOR_TEXT_SECONDARY",
            "TextSecondary",
            "Color.Text.Secondary",
            "#A6000000",
        ),
        (
            "RT_UI_COLOR_PRIMARY",
            "Primary",
            "Color.Primary",
            "#FF1677FF",
        ),
        (
            "RT_UI_COLOR_PRIMARY_HOVER",
            "PrimaryHover",
            "Color.Primary.Hover",
            "#FF4096FF",
        ),
        (
            "RT_UI_COLOR_PRIMARY_PRESSED",
            "PrimaryPressed",
            "Color.Primary.Pressed",
            "#FF0958D9",
        ),
        (
            "RT_UI_COLOR_FOCUS_RING",
            "FocusRing",
            "Color.Focus.Ring",
            "#661677FF",
        ),
        (
            "RT_UI_COLOR_DISABLED_FILL",
            "DisabledFill",
            "Color.Disabled.Fill",
            "#FFF5F5F5",
        ),
        (
            "RT_UI_COLOR_DISABLED_TEXT",
            "DisabledText",
            "Color.Disabled.Text",
            "#FFBFBFBF",
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
            "#FFF0F0F0",
        ),
        (
            "RT_UI_COLOR_SURFACE_HOVER",
            "SurfaceHover",
            "Color.Surface.Hover",
            "#FFE6F4FF",
        ),
        (
            "RT_UI_COLOR_SCROLL_TRACK",
            "ScrollTrack",
            "Color.Scroll.Track",
            "#FFF5F5F5",
        ),
        (
            "RT_UI_COLOR_SCROLL_THUMB",
            "ScrollThumb",
            "Color.Scroll.Thumb",
            "#FFBFBFBF",
        ),
        (
            "RT_UI_COLOR_SCROLL_THUMB_HOVER",
            "ScrollThumbHover",
            "Color.Scroll.Thumb.Hover",
            "#FF8C8C8C",
        ),
        (
            "RT_UI_COLOR_SCROLL_THUMB_PRESSED",
            "ScrollThumbPressed",
            "Color.Scroll.Thumb.Pressed",
            "#FF595959",
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
    assert!(header.contains("#define RT_UI_CONTROL_HEIGHT            32"));
    assert!(header.contains("#define RT_UI_BUTTON_PADDING_X          16"));
    assert!(header.contains("#define RT_UI_COLOR_DANGER_HOVER"));
    let metrics = read_file("std/UI/Core/Layout/ControlMetrics.as");
    assert!(
        metrics.contains("public const double ControlHeight = 32.0"),
        "ControlMetrics must own controlHeight"
    );
    assert!(
        metrics.contains("public const double ButtonPaddingX = 15.0"),
        "ControlMetrics must own button padding (Ant paddingInline=15)"
    );
}

#[test]
fn builtin_theme_styles_g_as_in_sync() {
    let root = repo_root();
    let expected = arc_ui::generate_styles_g_as(&root).expect("generate styles from Controls ARML");
    let path = root.join(arc_ui::STYLES_G_AS_REL);
    if std::env::var("UPDATE_BUILTIN_THEME").is_ok() {
        std::fs::write(&path, &expected).expect("write Styles.g.as");
        return;
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing {}: {e}; regenerate with UPDATE_BUILTIN_THEME=1 cargo test -p arc-ui --test design_tokens_contract -- builtin_theme_styles_g_as_in_sync",
            path.display()
        )
    });
    assert_eq!(
        norm_nl(&actual),
        norm_nl(&expected),
        "BuiltInTheme.Styles.g.as out of sync with Themes/Controls.arml; regenerate with UPDATE_BUILTIN_THEME=1"
    );
}

#[test]
fn theme_style_controls_exist_chrome_only() {
    let root = repo_root();
    let controls = read_file(arc_ui::CONTROLS_ARML_REL);
    for rel in arc_ui::CONTROL_STYLE_SOURCES {
        assert!(
            controls.contains(rel),
            "Controls.arml must MergedDictionaries-index {rel}"
        );
        let path = root.join("std/UI/Core/Themes").join(rel);
        assert!(path.is_file(), "missing control style: {rel}");
        let text = std::fs::read_to_string(&path).unwrap();
        let has_setter = text.contains("<Setter ");
        let has_based_on = text.contains("BasedOn=");
        assert!(
            has_setter || has_based_on,
            "{rel} must carry Setter resources or BasedOn inheritance (empty placeholder forbidden)"
        );
    }
    let styles = arc_ui::load_controls_styles(&root).expect("load controls styles");
    assert!(
        styles.len() >= arc_ui::CONTROL_STYLE_SOURCES.len(),
        "merged styles must cover every Controls/* source (keyed Styles allowed)"
    );
    assert!(
        styles.iter().any(|s| s.key.as_deref() == Some("Primary")),
        "Shared.arml must register global Primary short key"
    );
    assert!(
        styles.iter().any(|s| s.key.as_deref() == Some("Small")),
        "Shared.arml must register global Small short key"
    );
    assert!(
        !styles.iter().any(|s| s.key.as_deref() == Some("Button.Small")),
        "Button.Small must not exist when Shared Small already carries Padding (no duplicate override)"
    );
    assert!(
        !styles
            .iter()
            .any(|s| s.key.as_deref() == Some("Button.Size.SM")),
        "Button.Size.SM dual-track key must not exist (use Small short key)"
    );
    for style in &styles {
        let is_implicit = style.key.as_deref().unwrap_or("").is_empty();
        if !is_implicit {
            continue;
        }
        for setter in &style.setters {
            let prop = setter.property.as_str();
            assert!(
                !matches!(
                    prop,
                    "FontFamily" | "FontSize" | "FontWeight" | "Foreground"
                ),
                "implicit Style TargetType={:?} must not carry {prop}",
                style.target_type
            );
        }
    }
}

#[test]
fn builtin_theme_declares_light_dark_resource_dictionaries() {
    let tokens = read_file("std/UI/Core/Styling/BuiltInTheme.as");
    assert!(tokens.contains("public static ResourceDictionary CreateLight()"));
    assert!(tokens.contains("public static ResourceDictionary CreateDark()"));
    assert!(tokens.contains("BuiltInTheme.FillNonColor(d)"));
    assert!(tokens.contains("BuiltInTheme.AddImplicitStyles(d)"));
    assert!(tokens.contains("public static ResourceDictionary CreateControls()"));
    assert!(
        tokens.contains("DefaultControlTemplates.AttachToStyles(controls)")
            && tokens.contains("d.MergedDictionaries.Add(controls)"),
        "AddImplicitStyles must AttachDefaultTemplates then merge controls dict"
    );
}

/// RFC 037 §4：字体为环境 DP（禁入隐式 Style）；chrome 隐式 Style 经
/// `BuiltInTheme.AddImplicitStyles` → `CreateControls` MergedDictionaries 并入
/// CreateLight/Dark；`Application.ApplyStyleTree` 启动与 SwitchTheme 重应用。
#[test]
fn theme_dictionary_merges_controls_implicit_styles() {
    let theme = read_file("std/UI/Core/Styling/ThemeDictionary.as");
    let tokens = read_file("std/UI/Core/Styling/BuiltInTheme.as");
    let app = read_file("std/UI/Core/Components/Application.as");
    let styles_g = read_file(arc_ui::STYLES_G_AS_REL);
    assert!(theme.contains("Themes.Add(\"Light\", BuiltInTheme.CreateLight())"));
    assert!(theme.contains("Themes.Add(\"Dark\", BuiltInTheme.CreateDark())"));
    assert!(
        tokens.contains("AddImplicitStyles"),
        "CreateLight/Dark must merge Controls via AddImplicitStyles"
    );
    assert!(
        app.contains("void ApplyStyleTree()"),
        "Application must re-apply styles on startup and SwitchTheme"
    );
    assert!(app.contains("this.ApplyStyleTree()"));
    assert!(
        styles_g.contains("TargetType = \"Button\""),
        "Styles.g.as must register Button implicit style"
    );
    assert!(
        styles_g.contains(".Key = \"Primary\""),
        "Styles.g.as must register global Primary short key"
    );
    assert!(
        styles_g.contains(".BasedOn = \"Medium\""),
        "Styles.g.as must emit BasedOn Medium for control implicits"
    );
    assert!(
        !styles_g.contains(".Key = \"Button.Small\""),
        "Styles.g.as must not register Button.Small when Shared owns size+padding"
    );
    assert!(
        !styles_g.contains(".Key = \"Button.Size.SM\""),
        "Styles.g.as must not register Button.Size.SM dual-track key"
    );
    assert!(
        styles_g.contains("SetterValue.StaticResource(\"Size.Control.Height\")")
            || styles_g.contains("SetterValue.StaticResource(\"Color.Background\")"),
        "Styles.g.as Setters must reference StaticResource tokens"
    );
    // Fonts banned on implicit (empty Key) styles only — keyed size Styles may set FontSize later.
    let mut in_implicit = false;
    for line in styles_g.lines() {
        if line.contains(".Key = \"\"") {
            in_implicit = true;
        } else if line.contains(".Key = \"") {
            in_implicit = false;
        }
        if in_implicit {
            for font_prop in ["FontFamily", "FontSize", "FontWeight", "Foreground"] {
                assert!(
                    !line.contains(&format!("Property = \"{font_prop}\"")),
                    "implicit Styles.g.as must not carry {font_prop}"
                );
            }
        }
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
    assert!(app.contains("sm.ApplyAllStyles(this.MainWindow, windowResources, this.Resources)"));
}

/// RFC 037 §3 + theme-style-interaction-architecture §0.1：文档/演示/注释不得
/// 再推 StyleClass、Appearance 作者面、Variant= 或「字体进隐式 Style」漂移口径。
#[test]
fn style_variant_docs_match_rfc037() {
    let arch = read_file("docs/rfc/037-ui/references/theme-style-interaction-architecture.md");
    assert!(
        arch.contains("0.1.1 Style 键标准") && arch.contains("查找顺序"),
        "architecture §0.1.1 must declare Style key lookup standard"
    );
    assert!(
        arch.contains("`T.K`") && arch.contains("全局"),
        "architecture must document scoped-then-global lookup"
    );
    assert!(
        arch.contains("禁") && arch.contains("StyleClass"),
        "architecture must ban StyleClass"
    );
    assert!(
        arch.contains("Button.Size.SM") && arch.contains("第二"),
        "architecture must ban Button.Size.SM dual-track authoring"
    );

    let components = read_file("std/UI/Core/COMPONENTS.md");
    assert!(
        components.contains("Style=\"{StaticResource Primary, Small}\""),
        "COMPONENTS Button row must document short-key Style authoring"
    );
    assert!(
        !components.contains("Appearance**(Primary") && components.contains("无 Appearance DP"),
        "COMPONENTS must not list Appearance DP"
    );
    assert!(
        components.contains("禁 Class/StyleClass") || components.contains("禁 Class"),
        "COMPONENTS must ban Class/StyleClass"
    );

    let demo_readme = read_file("examples/ArmlDemo/README.md");
    assert!(
        !demo_readme.contains("Variant="),
        "ArmlDemo README must not recommend Variant="
    );
    assert!(
        demo_readme.contains("Style=\"{StaticResource Primary"),
        "ArmlDemo README must show short-key Style multi-bind"
    );
    assert!(
        !demo_readme.contains("Appearance=Primary"),
        "ArmlDemo README must not push Appearance= authoring"
    );
    assert!(
        !demo_readme.contains("Button.Size.SM") && !demo_readme.contains("Button.Size.MD"),
        "ArmlDemo README must not push Size.SM dual-track authoring"
    );

    let production = read_file("docs/rfc/037-ui/references/production-surface.md");
    assert!(
        !production.contains("字体三件套经隐式 Style"),
        "production-surface F1 must not say fonts come from implicit Style"
    );
    assert!(
        production.contains("环境 DP")
            || production.contains("禁**进隐式 Style")
            || production.contains("禁**烤进隐式 Style"),
        "production-surface F1 must say ambient DP / ban fonts in implicit Style"
    );

    let vsm = read_file("std/UI/Core/Styling/VisualStateManager.as");
    assert!(
        !vsm.contains("Style Class") && !vsm.contains("keyed Style Class"),
        "VSM comments must not call Style keys 'Class'"
    );

    let button = read_file("std/UI/Core/Components/Button.as");
    assert!(
        button.contains("Style=\"{StaticResource Primary") || button.contains("短键"),
        "Button.as must document short-key Style authoring"
    );
    assert!(
        !button.contains("AppearanceProperty") && !button.contains("Appearance {"),
        "Button.Appearance DP must be deleted"
    );

    let resolver = read_file("std/UI/Core/Styling/StyleKeyResolver.as");
    assert!(
        resolver.contains("Candidates") && resolver.contains("MissDiagnostic"),
        "StyleKeyResolver must expose Candidates + MissDiagnostic"
    );
    assert!(
        read_file("crates/arc-ui/src/style_key.rs").contains("lookup_candidates"),
        "arc-ui style_key must implement lookup_candidates (scoped then global)"
    );

    let builtin = read_file("docs/rfc/037-ui/references/builtin-theme-resources.md");
    assert!(
        builtin.contains("Shared.arml") && builtin.contains("Button.arml"),
        "builtin-theme-resources must declare Shared.arml + per-control files"
    );
    assert!(
        !builtin.contains("Button.Styles.arml") && !builtin.contains("Button.Size.SM"),
        "builtin-theme-resources must not list Styles.arml or Size.SM dual track"
    );

    assert!(
        !std::path::Path::new("std/UI/Core/Themes/Controls/Button.Styles.arml").exists(),
        "Button.Styles.arml must not exist"
    );
    assert!(
        repo_root()
            .join("std/UI/Core/Themes/Controls/Shared.arml")
            .exists(),
        "Shared.arml must exist for global short keys"
    );

    let button_arml = read_file("std/UI/Core/Themes/Controls/Button.arml");
    assert!(
        button_arml.contains("BasedOn=\"{StaticResource Medium}\"")
            && !button_arml.contains("x:Key=\"Button.Small\""),
        "Button.arml must BasedOn Medium and not duplicate Button.Small"
    );
    assert!(
        !button_arml.contains("Button.Size.SM") && !button_arml.contains("Property=\"Appearance\""),
        "Button.arml must not use Size.SM dual track or Appearance Setter"
    );

    let shared = read_file("std/UI/Core/Themes/Controls/Shared.arml");
    assert!(
        shared.contains("x:Key=\"Small\"")
            && shared.contains("x:Key=\"Primary\"")
            && shared.contains("Size.Control.Padding"),
        "Shared.arml must define global Small/Primary with size+padding ladder"
    );

    assert!(
        arch.contains("0.2 VSM") || arch.contains("VSM 审查裁决"),
        "architecture must document VSM ruling §0.2"
    );
    assert!(
        arch.contains("内部交互态引擎") || arch.contains("实现细节"),
        "architecture must classify VSM as internal / implementation detail"
    );

    let typeck = read_file("crates/arc-ui/src/typeck.rs");
    assert!(
        !typeck.contains("with_property(\"Appearance\""),
        "arc-ui typeck must not expose Appearance as ARML authoring property"
    );

    let styles = arc_ui::load_controls_styles(&repo_root()).expect("load controls styles");
    for key in ["Primary", "Default", "Danger", "Small", "Medium", "Large"] {
        assert!(
            styles.iter().any(|s| s.key.as_deref() == Some(key)),
            "Controls merge must register Style `{key}`"
        );
    }
    for banned in [
        "Button.Size.SM",
        "Button.Primary",
        "TextBox.Size.SM",
        "CheckBox.Size.MD",
        "Button.Small",
        "Button.Medium",
        "Button.Large",
        "ToggleButton.Small",
    ] {
        assert!(
            !styles.iter().any(|s| s.key.as_deref() == Some(banned)),
            "dual-track / redundant override key `{banned}` must not be registered"
        );
    }

    // Lookup order unit contract (scoped → global); Button has no Button.Small → falls to Shared
    let cands = arc_ui::lookup_candidates("Small", "Button");
    assert_eq!(cands, vec!["Button.Small".to_string(), "Small".to_string()]);
    let miss = arc_ui::miss_diagnostic("Nope", "Button");
    assert!(miss.contains("Button.Nope") && miss.contains("Nope"));

    // Implicit control styles BasedOn Medium (no duplicated MinHeight Setter)
    for target in [
        "Button",
        "ToggleButton",
        "CheckBox",
        "RadioButton",
        "TextBox",
        "PasswordBox",
        "ComboBox",
    ] {
        let implicit = styles
            .iter()
            .find(|s| {
                s.key.as_deref().unwrap_or("").is_empty()
                    && s.target_type.as_deref() == Some(target)
            })
            .unwrap_or_else(|| panic!("missing implicit Style for {target}"));
        assert!(
            implicit.based_on.is_some(),
            "{target} implicit must BasedOn Medium (Shared size ladder)"
        );
        assert!(
            implicit.setters.is_empty(),
            "{target} implicit must not duplicate Shared MinHeight/Padding Setters"
        );
    }
    let shared_medium = styles
        .iter()
        .find(|s| s.key.as_deref() == Some("Medium"))
        .expect("Shared Medium");
    assert!(
        shared_medium
            .setters
            .iter()
            .any(|s| s.property.as_str() == "MinHeight")
            && shared_medium
                .setters
                .iter()
                .any(|s| s.property.as_str() == "Padding"),
        "Shared Medium must carry MinHeight + Padding"
    );
}

/// Controls/*.arml Setter 禁裸 Thickness 数字串与裸 #hex（须 StaticResource）。
/// 例外：无——色态标识字符串（Primary/Default…）不含 `#` 与四元组。
#[test]
fn controls_arml_no_bare_thickness_or_hex() {
    let root = repo_root().join("std/UI/Core/Themes/Controls");
    let mut files = 0usize;
    for entry in std::fs::read_dir(&root).expect("Controls dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("arml") {
            continue;
        }
        files += 1;
        let text = std::fs::read_to_string(&path).expect("read control arml");
        let name = path.file_name().unwrap().to_string_lossy();
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("<!--") || trimmed.starts_with("//") {
                continue;
            }
            // Setter Value="#…" 或任意属性裸 hex
            if trimmed.contains("Value=\"#") || trimmed.contains("=\"#") {
                panic!(
                    "{name}:{} bare hex forbidden in Controls arml: {trimmed}",
                    i + 1
                );
            }
            // Thickness 四元组数字：Value="1,1,1,1" / "16,8,16,8"
            if let Some(rest) = trimmed.strip_prefix("<Setter ") {
                if rest.contains("Value=\"") {
                    let start = rest.find("Value=\"").unwrap() + "Value=\"".len();
                    let end = rest[start..].find('"').unwrap() + start;
                    let val = &rest[start..end];
                    if val
                        .split(',')
                        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit() || c == '.'))
                        && val.contains(',')
                    {
                        panic!(
                            "{name}:{} bare Thickness `{val}` forbidden; use Size.* StaticResource",
                            i + 1
                        );
                    }
                }
            }
        }
    }
    assert!(files >= 10, "expected Controls/*.arml files, found {files}");

    let border = read_file("std/UI/Core/Themes/Controls/Border.arml");
    assert!(
        border.contains("Size.Border.Thickness") && border.contains("Size.Border.Padding"),
        "Border.arml must use Size.Border.* resource keys"
    );
    assert!(
        !border.contains("Value=\"1,1,1,1\"") && !border.contains("Value=\"12,12,12,12\""),
        "Border.arml must not embed bare Thickness literals"
    );

    let builtin = read_file("std/UI/Core/Styling/BuiltInTheme.as");
    assert!(
        builtin.contains("Size.Border.Thickness")
            && builtin.contains("Size.Border.Padding")
            && builtin.contains("Color.Text.Selection")
            && builtin.contains("Color.Image.Fill")
            && builtin.contains("Color.Text.Highlight"),
        "BuiltInTheme must declare Border size + selection/placeholder/highlight color keys"
    );

    let demo = read_file("examples/ArmlDemo/MainWindow.arml");
    assert!(
        !demo.contains("=\"#FF") && !demo.contains("=\"#E0") && !demo.contains("=\"#00"),
        "ArmlDemo MainWindow.arml must not hardcode theme hex"
    );
    let demo_app = read_file("examples/ArmlDemo/App.arml");
    assert!(
        !demo_app.contains("Value=\"#"),
        "ArmlDemo App.arml must not hardcode theme hex in Setters"
    );
}

/// ItemsControl / ItemSourceView：破单活跃槽——须 route 表 + Dispatch*Change，
/// 禁 `_activeViewHost` / `_activeObservableView` 回潮。
#[test]
fn items_control_observable_multi_route() {
    let items = read_file("std/UI/Core/Components/ItemsControl.as");
    let view = read_file("std/UI/Core/Components/ItemSourceView.as");
    assert!(
        !items.contains("_activeViewHost"),
        "ItemsControl must not use single-slot _activeViewHost"
    );
    assert!(
        items.contains("_viewHosts") && items.contains("DispatchViewChange"),
        "ItemsControl must route via _viewHosts + DispatchViewChange"
    );
    assert!(
        items.contains("routeSlot") || items.contains("_viewRouteSlot"),
        "ItemsControl OnChanged must capture route slot by value"
    );
    assert!(
        !view.contains("_activeObservableView"),
        "ItemSourceView must not use single-slot _activeObservableView"
    );
    assert!(
        view.contains("_obsViews") && view.contains("DispatchSourceChange"),
        "ItemSourceView must route via _obsViews + DispatchSourceChange"
    );
    let components = read_file("std/UI/Core/COMPONENTS.md");
    assert!(
        !components.contains("ItemsControl/ItemSourceView 单活跃")
            && !components.contains("ItemsControl/ItemSourceView Observable 仍单活跃"),
        "COMPONENTS must not claim ItemsControl/ItemSourceView single-slot debt"
    );
}

/// ControlMetrics 须拥有本刀收敛的几何令牌；控件 Measure 禁裸轨高/滑块高字面量。
#[test]
fn control_metrics_owns_geometry() {
    let metrics = read_file("std/UI/Core/Layout/ControlMetrics.as");
    for token in [
        "ProgressBarThickness",
        "ProgressBarIndeterminateFraction",
        "ProgressBarIndeterminatePeriodFactor",
        "SliderDefaultHeight",
        "VScrollWidth",
        "VScrollMinThumb",
        "VScrollThumbInset",
        "VScrollThumbRadius",
        "FocusRingOutset",
        "ComboChevronInset",
        "ComboChevronStroke",
        "ComboChevronCenterNudgeY",
        "ComboChevronMidWidth",
        "ComboChevronTipWidth",
        "ComboChevronStepY",
        "TabHeaderBarHeight",
        "TabHeaderFontSize",
        "TabHeaderPaddingX",
        "TabHeaderMinWidth",
        "TabOverflowArrowWidth",
        "TabOverflowScrollStep",
        "MessageBoxIconSize",
    ] {
        assert!(
            metrics.contains(&format!("public const double {token}")),
            "ControlMetrics must own {token}"
        );
    }
    let progress = read_file("std/UI/Core/Components/ProgressBar.as");
    assert!(
        progress.contains("ControlMetrics.ProgressBarThickness"),
        "ProgressBar Measure must use ControlMetrics.ProgressBarThickness"
    );
    assert!(
        !progress.contains("double h = 8.0"),
        "ProgressBar must not hardcode track height 8.0"
    );
    let render = read_file("std/UI/Core/Rendering/wgpu/WgpuRender.TemplateChrome.as");
    assert!(
        render.contains("IsIndeterminate")
            && render.contains("MotionEngine.ResolveLoop01")
            && render.contains("ProgressBarIndeterminateFraction")
            && render.contains("MotionDurationNormal"),
        "ProgressBar IsIndeterminate must sweep via MotionEngine.ResolveLoop01 + Motion/ControlMetrics tokens"
    );
    let slider = read_file("std/UI/Core/Components/Slider.as");
    assert!(
        slider.contains("ControlMetrics.SliderDefaultHeight"),
        "Slider Measure must use ControlMetrics.SliderDefaultHeight"
    );
    assert!(
        !slider.contains("double h = 24.0"),
        "Slider must not hardcode default height 24.0"
    );
    let grid = read_file("std/UI/Core/Components/DataGrid.as");
    assert!(
        grid.contains("ControlMetrics.ControlHeight") && grid.contains("ControlMetrics.SpacingSM"),
        "DataGrid ResolveRowStride must use ControlMetrics"
    );
    assert!(
        !grid.contains("return 32.0"),
        "DataGrid must not hardcode row height 32.0"
    );
    let render = read_file("std/UI/Core/Rendering/wgpu/WgpuRender.RenderTree.as");
    let chrome = read_file("std/UI/Core/Rendering/wgpu/WgpuRender.TemplateChrome.as");
    assert!(
        (render.contains("ControlMetrics.ComboChevronStroke")
            || chrome.contains("ControlMetrics.ComboChevronStroke"))
            && (render.contains("ControlMetrics.ComboChevronStepY")
                || chrome.contains("ControlMetrics.ComboChevronStepY"))
            && render.contains("ControlMetrics.TabHeaderBarHeight")
            && render.contains("ControlMetrics.TabOverflowArrowWidth")
            && render.contains("HeaderOverflow"),
        "RenderTree/TemplateChrome chevron/Tab micro-geometry must use ControlMetrics (incl. overflow arrows)"
    );
    assert!(
        !render.contains("chevronCy + 2.0")
            && !chrome.contains("chevronCy + 2.0")
            && !render.contains("- 2.25")
            && !chrome.contains("- 2.25"),
        "RenderTree/TemplateChrome must not hardcode chevron stack magic numbers"
    );
    let msg = read_file("std/UI/Core/Components/MessageBox.as");
    assert!(
        msg.contains("ControlMetrics.MessageBoxIconSize"),
        "MessageBox icon badge must use ControlMetrics.MessageBoxIconSize"
    );
    let vsm = read_file("std/UI/Core/Styling/VisualStateManager.as");
    assert!(
        vsm.contains("ControlMetrics.VScrollThumbRadius"),
        "VSM ScrollBar radius must use ControlMetrics.VScrollThumbRadius"
    );
}
