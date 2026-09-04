//! RFC 037 §4 · 主题切换闭环契约：Light/Dark 全链（VSM → Application → MotionEngine）验收。
//!
//! 源契约（不依赖 GPU/真窗）：验证「切主题即全链生效」的闭环：
//!   1. Dark ARML / Colors.g.as 提供与 Light 不同的深色值（同 key 集完整注册）；
//!   2. `Application.SwitchTheme` 切换活动主题并重建 `MergedDictionaries` + 触发重绘；
//!   3. 全链解析：`VisualStateManager` 产出资源键 → `Application.Current.ResolveColor`
//!      （本地覆盖 > 活动主题）→ `MotionEngine` 插值上屏，唯一解析根闭环；
//!   4. 主题覆盖定制：编译期平坦 `RegisterTheme`（无运行期 Merged）。

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
fn dark_theme_provides_distinct_values_for_all_color_keys() {
    let tokens = read_file("std/UI/Core/Styling/BuiltInTheme.as");
    let dark_arml = read_file(arc_ui::DARK_ARML_REL);
    let colors_g = read_file(arc_ui::COLORS_G_AS_REL);

    // Dark 值与 Light 不同（深色系），且经 ARML → Colors.g.as 注册。
    let dark_pairs = [
        ("Background", "Color.Background", "#FF0F0F12"),
        ("Surface", "Color.Surface", "#FF17171C"),
        ("Border", "Color.Border", "#FF2A2A33"),
        ("TextPrimary", "Color.Text.Primary", "#FFF3F3F7"),
        ("TextSecondary", "Color.Text.Secondary", "#FF9C9CA8"),
        ("Primary", "Color.Primary", "#FF6366F1"),
        ("PrimaryHover", "Color.Primary.Hover", "#FF818CF8"),
        ("PrimaryPressed", "Color.Primary.Pressed", "#FF4F46E5"),
        ("FocusRing", "Color.Focus.Ring", "#8C6366F1"),
        ("DisabledFill", "Color.Disabled.Fill", "#FF23232A"),
        ("DisabledText", "Color.Disabled.Text", "#FF5A5A66"),
        ("SurfaceHover", "Color.Surface.Hover", "#FF222229"),
        ("ScrollTrack", "Color.Scroll.Track", "#FF1A1A20"),
        ("ScrollThumb", "Color.Scroll.Thumb", "#FF5A5A66"),
        ("ScrollThumbHover", "Color.Scroll.Thumb.Hover", "#FF7A7A88"),
        (
            "ScrollThumbActive",
            "Color.Scroll.Thumb.Active",
            "#FF9A9AA8",
        ),
        ("SliderTrack", "Color.Slider.Track", "#FF2E2E38"),
    ];
    for (field, key, val) in dark_pairs {
        assert!(
            dark_arml.contains(&format!("x:Key=\"{key}\""))
                && dark_arml.contains(&format!("Value=\"{val}\"")),
            "Dark.arml 缺失/值不符: {key} -> {val}"
        );
        assert!(
            colors_g.contains(&format!(
                "d.Add(BuiltInTheme.{field}, ResourceValue.Brush(Brushes.Parse(\"{val}\")))"
            )),
            "Colors.g.as Dark 缺失/值不符: {field} -> {val}"
        );
    }

    assert!(tokens.contains("public const string Primary = \"Color.Primary\";"));
    assert!(tokens.contains("public static ResourceDictionary CreateDark()"));
    assert!(tokens.contains("BuiltInTheme.FillNonColor(d)"));
    assert!(tokens.contains("BuiltInThemeColors.FillDarkColors(d)"));
}

#[test]
fn switch_theme_rebuilds_merged_dictionaries_and_invalidates() {
    let app = read_file("std/UI/Core/Components/Application.as");
    assert!(app.contains("public void SwitchTheme(string name)"));
    assert!(app.contains("ThemeDictionaries.Switch(name)"));
    assert!(app.contains("Resources.MergedDictionaries.Clear()"));
    assert!(app.contains("Resources.MergedDictionaries.Add(ThemeDictionaries.Active)"));
    assert!(
        app.contains("FramePump.Invalidate()"),
        "SwitchTheme 必须触发帧失效以驱动全链重绘"
    );

    let theme = read_file("std/UI/Core/Styling/ThemeDictionary.as");
    assert!(theme.contains("public void Switch(string name)"));
    assert!(theme.contains("public void RegisterTheme(string name, ResourceDictionary dict)"));
    assert!(
        !theme.contains("ResourceDictionary.Merged"),
        "编译期聚合后 RegisterTheme 不得残留运行期 Merged 合并"
    );
    let codegen = read_file("crates/arc-ui/src/codegen.rs");
    assert!(codegen.contains("resolve_theme_chain"));
    assert!(codegen.contains("BuiltInTheme.CreateLight()"));
    assert!(codegen.contains("BuiltInTheme.CreateDark()"));
    assert!(codegen.contains("RegisterTheme"));
}

#[test]
fn full_chain_resolves_through_single_root() {
    let vsm = read_file("std/UI/Core/Styling/VisualStateManager.as");
    let app = read_file("std/UI/Core/Components/Application.as");
    let render = read_file("std/UI/Core/Rendering/wgpu/WgpuRender.RenderTree.as");
    let motion = read_file("std/UI/Core/Internal/MotionEngine.as");

    assert!(vsm.contains("public string Background;"));
    assert!(vsm.contains("BuiltInTheme.PrimaryPressed"));
    assert!(!vsm.contains("ThemeDictionary.Current"));

    assert!(vsm.contains("internal struct ControlState"));
    assert!(vsm.contains("public int Hover;"));
    assert!(vsm.contains("public int Selected;"));
    assert!(vsm.contains("public string GradientStart;"));
    assert!(vsm.contains("public double MotionDuration;"));
    assert!(vsm.contains("BuiltInTheme.MotionHoverMs"));
    assert!(vsm.contains("BuiltInTheme.AccentGradientA"));

    assert!(render.contains("Application.Current.ResolveColor(key)"));
    assert!(render.contains("StateColorMotion"));
    assert!(render.contains("MotionEngine.ResolveColorDur"));

    assert!(
        motion.contains("public static Color ResolveColor(long handle, int role, string target)")
    );
    assert!(motion.contains("public static Color ResolveColorDur(long handle, int role, string target, double durationMs)"));

    assert!(app.contains("public string ResolveColor(string key)"));
    assert!(app.contains("Resources.TryLookup(key, ref v)"));
}
