//! RFC 027 M-U1：自适应编译期检查（值类型元素 / `Match` / `Tiers` / 单位校验）
//! 与 `.arml.as` 污染检查（P1 红线）的 crate 级测试。
//!
//! 原 5 个 `adaptive_reject_*_e2e` 的 CLI 级端到端验证随 arc-integration 退场（a2627a0f）。

use arc_ui::{
    check_codebehind_pollution, verify_report, verify_report_with_strict, Parser, TypeChecker,
};

fn check(src: &str, strict: bool) -> (Vec<String>, Vec<String>) {
    let doc = Parser::parse(src).expect("parse");
    let checker = TypeChecker::new();
    let report = if strict {
        verify_report_with_strict(&doc, &checker, true)
    } else {
        verify_report(&doc, &checker)
    };
    let errors: Vec<String> = report
        .adaptive_issues
        .iter()
        .map(|e| e.to_string())
        .collect();
    let warnings: Vec<String> = report
        .adaptive_warnings
        .iter()
        .map(|e| e.to_string())
        .collect();
    (errors, warnings)
}

fn errs_any(src: &str, strict: bool, needle: &str) -> bool {
    check(src, strict).0.iter().any(|m| m.contains(needle))
}

fn warns_any(src: &str, strict: bool, needle: &str) -> bool {
    check(src, strict).1.iter().any(|m| m.contains(needle))
}

// ===== 干净文档：全部合法，无 error/warning =====

#[test]
fn adaptive_clean_document_passes() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="Spacing.Page">
            <Match Tier="sm" Value="8" />
            <Match Tier="md" Value="16" />
            <Match Tier="lg" Value="24" />
            <Match Value="16" />
        </Double>
        <Color x:Key="Color.Primary">
            <Match Media="dark" Value="#8FB8FF" />
            <Match Value="#2D6CDF" />
        </Color>
        <TrackList x:Key="Grid.MasterDetail" Value="1,2" />
    </Window.Resources>
</Window>"##;
    let (errors, warnings) = check(src, true);
    assert!(errors.is_empty(), "errors: {errors:?}");
    // x:Key 已定义但未引用 → 未使用 Token warning（非严格类）
    assert!(
        warnings.iter().any(|m| m.contains("unused Token")),
        "expected unused Token warnings, got: {warnings:?}"
    );
}

// ===== 单位校验（§11.1） =====

#[test]
fn adaptive_reject_bad_unit_on_double() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X" Value="16em" />
    </Window.Resources>
</Window>"##;
    assert!(errs_any(src, false, "unknown length unit `em`"));
}

#[test]
fn adaptive_accept_all_units_on_double() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="A" Value="16" />
        <Double x:Key="B" Value="16px" />
        <Double x:Key="C" Value="16%" />
        <Double x:Key="D" Value="16lpx" />
        <Double x:Key="E" Value="12.5vp" />
    </Window.Resources>
</Window>"##;
    let (errors, _) = check(src, false);
    assert!(errors.is_empty(), "errors: {errors:?}");
}

#[test]
fn adaptive_reject_breakpoint_with_unit() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match MinWidth="600px" Value="8" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(errs_any(src, false, "not a plain number"));
}

#[test]
fn adaptive_reject_bad_value_literals() {
    let bad = [
        r##"<Double x:Key="X" Value="red" />"##,
        r##"<Color x:Key="X" Value="red" />"##,
        r##"<TrackList x:Key="X" Value="1,,2" />"##,
        r##"<Thickness x:Key="X" Value="1,2,3" />"##,
        r##"<Boolean x:Key="X" Value="yes" />"##,
    ];
    for src in bad {
        let arml = format!(
            r##"<Window x:Class="Ns.W"><Window.Resources>{src}</Window.Resources></Window>"##
        );
        let (errors, _) = check(&arml, false);
        assert!(!errors.is_empty(), "expected error for `{src}`, got none");
    }
}

// ===== 档位（§11.3） =====

#[test]
fn adaptive_reject_unknown_tier() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match Tier="xl" Value="8" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(errs_any(src, false, "undefined Tier `xl`"));
}

#[test]
fn adaptive_accept_custom_tier_from_application_tiers() {
    let src = r##"<Application x:Class="Ns.App">
    <Application.Tiers Default="sm:600 md:960 lg:1280 xl:1600" />
    <Application.Resources>
        <Double x:Key="X">
            <Match Tier="xl" Value="8" />
            <Match Value="16" />
        </Double>
    </Application.Resources>
</Application>"##;
    let (errors, _) = check(src, false);
    assert!(errors.is_empty(), "errors: {errors:?}");
}

#[test]
fn adaptive_window_local_tier_drift_warns_and_strict_errors() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Tiers Default="sm:520 md:880" />
    <Window.Resources>
        <Double x:Key="X">
            <Match Tier="lg" Value="8" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
</Window>"##;
    // lg 未在局部声明 → 隐式漂移 warning（strict = error）
    assert!(warns_any(src, false, "implicit tier threshold drift"));
    assert!(errs_any(src, true, "implicit tier threshold drift"));
}

#[test]
fn adaptive_reject_duplicate_window_tiers() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Tiers Default="sm:520" />
    <Window.Tiers Default="sm:600" />
</Window>"##;
    assert!(errs_any(src, false, "duplicate `<Window>.Tiers`"));
}

// ===== 断点区间（§11.3） =====

#[test]
fn adaptive_reject_breakpoint_overlap() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match MinWidth="600" Value="8" />
            <Match MinWidth="400" Value="16" />
            <Match Value="24" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(errs_any(src, false, "breakpoint intervals overlap"));
}

#[test]
fn adaptive_reject_mix_tier_and_breakpoint() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match Tier="md" MinWidth="600" Value="8" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(errs_any(
        src,
        false,
        "mixes `Tier` with `MinWidth`/`MaxWidth`"
    ));
}

// ===== 未全覆盖（无兜底） =====

#[test]
fn adaptive_uncovered_warns_and_strict_errors() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match Tier="sm" Value="8" />
            <Match Tier="md" Value="16" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(warns_any(src, false, "no fallback"));
    assert!(errs_any(src, true, "no fallback"));
}

// ===== 死分支 / 歧义 =====

#[test]
fn adaptive_dead_branch_warns_and_strict_errors() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Color x:Key="C">
            <Match Idiom="Desktop" Media="dark" Value="#000000" />
            <Match Media="dark" Value="#111111" />
            <Match Value="#222222" />
        </Color>
    </Window.Resources>
</Window>"##;
    assert!(warns_any(src, false, "dead `<Match>`"));
    assert!(errs_any(src, true, "dead `<Match>`"));
}

#[test]
fn adaptive_ambiguity_warns_and_strict_errors() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match Media="dark" Value="8" />
            <Match Media="dark" Value="16" />
            <Match Value="24" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(warns_any(src, false, "ambiguous `<Match>`"));
    assert!(errs_any(src, true, "ambiguous `<Match>`"));
}

// ===== Media 坐标 / MediaValue（§11.3） =====

#[test]
fn adaptive_reject_undefined_media() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match Media="sepia" Value="8" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(errs_any(src, false, "undefined Media `sepia`"));
}

#[test]
fn adaptive_media_value_pairing() {
    // 参数化坐标缺 MediaValue → error
    let missing = r##"<Application x:Class="Ns.App">
    <Application.Media><Media Name="font-scale" Type="Ratio" /></Application.Media>
    <Application.Resources>
        <Double x:Key="X">
            <Match Media="font-scale" Value="20" />
            <Match Value="16" />
        </Double>
    </Application.Resources>
</Application>"##;
    assert!(errs_any(missing, false, "requires `MediaValue`"));

    // 内置坐标带 MediaValue → error
    let builtin = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match Media="dark" MediaValue="1.3" Value="20" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(errs_any(builtin, false, "not parameterized"));

    // MediaValue 无 Media → error
    let orphan = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X">
            <Match MediaValue="1.3" Value="20" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
</Window>"##;
    assert!(errs_any(orphan, false, "requires a paired `Media`"));

    // 正确配对 → ok
    let ok = r##"<Application x:Class="Ns.App">
    <Application.Media>
        <Media Name="safe-area-inset-top" Type="Length(vp)" />
        <Media Name="font-scale" Type="Ratio" />
    </Application.Media>
    <Application.Resources>
        <Double x:Key="X">
            <Match Media="font-scale" MediaValue="1.3" Value="20" />
            <Match Media="safe-area-inset-top" MediaValue="47" Value="24" />
            <Match Value="16" />
        </Double>
    </Application.Resources>
</Application>"##;
    let (errors, _) = check(ok, false);
    assert!(errors.is_empty(), "errors: {errors:?}");
}

// ===== 值元素结构 =====

#[test]
fn adaptive_match_outside_value_element_errors() {
    let src = r##"<Window x:Class="Ns.W">
    <Match Tier="sm" Value="8" />
</Window>"##;
    assert!(errs_any(
        src,
        false,
        "must be a child of a type-valued element"
    ));
}

#[test]
fn adaptive_value_element_without_value_errors() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="X" />
    </Window.Resources>
</Window>"##;
    assert!(errs_any(src, false, "requires `Value`"));
}

#[test]
fn adaptive_adaptive_element_conditions() {
    // `<Adaptive>` 条件与 Match 同一套；Tier+MinWidth 混用 → error
    let src = r##"<Window x:Class="Ns.W">
    <Adaptive Tier="md" MinWidth="600">
        <TextBlock Text="hi" />
    </Adaptive>
</Window>"##;
    assert!(errs_any(
        src,
        false,
        "mixes `Tier` with `MinWidth`/`MaxWidth`"
    ));
}

#[test]
fn adaptive_adaptive_sibling_overlap_errors() {
    let src = r##"<Window x:Class="Ns.W">
    <StackPanel>
        <Adaptive MinWidth="600"><TextBlock Text="a" /></Adaptive>
        <Adaptive MinWidth="400"><TextBlock Text="b" /></Adaptive>
    </StackPanel>
</Window>"##;
    assert!(errs_any(src, false, "breakpoint intervals overlap"));
}

// ===== 未使用 Token（死符号） =====

#[test]
fn adaptive_used_token_no_warning() {
    let src = r##"<Window x:Class="Ns.W">
    <Window.Resources>
        <Double x:Key="Spacing.Gap" Value="4" />
    </Window.Resources>
    <StackPanel Padding="{Token Spacing.Gap}" />
</Window>"##;
    let (_, warnings) = check(src, false);
    assert!(
        !warnings.iter().any(|m| m.contains("unused Token")),
        "warnings: {warnings:?}"
    );
}

// ===== `.arml.as` 污染检查（P1 红线，双文件配对扫描） =====

#[test]
fn adaptive_codebehind_pollution_detects_size_wording() {
    let dir = std::env::temp_dir().join("arc_ui_pollution_test");
    std::fs::create_dir_all(&dir).unwrap();
    let arml = dir.join("MainWindow.arml");
    std::fs::write(&arml, r##"<Window x:Class="Ns.MainWindow"/>"##).unwrap();
    std::fs::write(
        dir.join("MainWindow.arml.as"),
        "namespace Ns;\npublic partial class MainWindow {\n    double w = this.ActualWidth;\n}\n",
    )
    .unwrap();

    let issues = check_codebehind_pollution(&arml);
    assert!(
        issues.iter().any(|e| e.to_string().contains("ActualWidth")),
        "expected ActualWidth pollution, got: {issues:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn adaptive_codebehind_pollution_ignores_word_boundaries() {
    let dir = std::env::temp_dir().join("arc_ui_pollution_clean_test");
    std::fs::create_dir_all(&dir).unwrap();
    let arml = dir.join("MainWindow.arml");
    std::fs::write(&arml, r##"<Window x:Class="Ns.MainWindow"/>"##).unwrap();
    std::fs::write(
        dir.join("MainWindow.arml.as"),
        "namespace Ns;\npublic partial class MainWindow {\n    public void OnLoaded() { }\n}\n",
    )
    .unwrap();

    let issues = check_codebehind_pollution(&arml);
    assert!(issues.is_empty(), "issues: {issues:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
