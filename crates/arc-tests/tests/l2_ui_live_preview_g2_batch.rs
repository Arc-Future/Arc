//! L2 批量：AI 原生 G2 最小硬门槛（RFC 037 §10 · ai-native-live-preview）。
//!
//! 验收面（headless，无 HWND / 无帧泵）：
//! - `g2_loadspec_capture_png`：LoadSpec(ARML 字符串) → CapturePng
//!   → 文件存在 / PNG 魔数 / IHDR 尺寸（生成即见：spec→可见帧，无编译 ARML 工程）
//! - `g2_apply_patch_text_layout`：补丁 TextBlock.Text → 属性值变 + 行盒变宽（改即见）
//! - `g2_apply_patch_unknown_false`：未知路径返回 false，树不变
//! - `g2_loadspec_empty_diag`：空 spec 失败且带诊断，不建树
//! - `g2_reset_clears`：Reset 后快照 null、补丁拒绝
//!
//! 宣称纪律：仅关「LivePreviewHost G2 最小可 CI」；**不**宣称 G1 双宿主像素一致、
//! G3 VideoSurface、运行时 arc-ui typeck、审视回路、像素闸。
//!
//! 依赖：`("Arc.UI", "UI/Core")`。需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];

#[test]
fn ui_live_preview_g2_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_live_preview_g2",
        &[
            BatchCase {
                name: "g2_loadspec_capture_png",
                src: r##"using Arc;
using Arc.IO;
using Arc.UI.Components;
using Arc.UI.Markup;

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:init");
        return;
    }
    int w = 64;
    int h = 48;
    ArmlParseResult loaded = host.LoadSpec(
        "<StackPanel Width=\"64\" Height=\"48\"><TextBlock x:Name=\"Title\" FontSize=\"14\">Hi</TextBlock></StackPanel>",
        (double)w, (double)h);
    if (!loaded.Success || host.RootElement == null) {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:load");
        return;
    }
    if (host.RootElement.TypeName != "StackPanel") {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:root=" + host.RootElement.TypeName);
        return;
    }
    string path = Path.Combine(Path.GetTempPath(), "arc_ui_g2_preview_64x48.png");
    if (File.Exists(path)) {
        File.Delete(path);
    }
    if (!host.CapturePng(path)) {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:capture");
        return;
    }
    if (!File.Exists(path)) {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:missing");
        return;
    }
    byte[] bytes = File.ReadAllBytes(path);
    File.Delete(path);
    if (bytes.Length < 24) {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:too_short=" + bytes.Length);
        return;
    }
    if ((bytes[0] & 0xFF) != 0x89 || (bytes[1] & 0xFF) != 0x50
        || (bytes[2] & 0xFF) != 0x4E || (bytes[3] & 0xFF) != 0x47
        || (bytes[4] & 0xFF) != 0x0D || (bytes[5] & 0xFF) != 0x0A
        || (bytes[6] & 0xFF) != 0x1A || (bytes[7] & 0xFF) != 0x0A) {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:magic");
        return;
    }
    if ((bytes[12] & 0xFF) != 0x49 || (bytes[13] & 0xFF) != 0x48
        || (bytes[14] & 0xFF) != 0x44 || (bytes[15] & 0xFF) != 0x52) {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:ihdr_tag");
        return;
    }
    int iw = ((bytes[16] & 0xFF) << 24) | ((bytes[17] & 0xFF) << 16)
        | ((bytes[18] & 0xFF) << 8) | (bytes[19] & 0xFF);
    int ih = ((bytes[20] & 0xFF) << 24) | ((bytes[21] & 0xFF) << 16)
        | ((bytes[22] & 0xFF) << 8) | (bytes[23] & 0xFF);
    if (iw != w || ih != h) {
        Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:FAIL:size=" + iw + "x" + ih);
        return;
    }
    Console.WriteLine("ARC_CASE:g2_loadspec_capture_png:PASS");
}
"##,
            },
            BatchCase {
                name: "g2_apply_patch_text_layout",
                src: r##"using Arc;
using Arc.UI.Components;
using Arc.UI.Layout;
using Arc.UI.Markup;

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:init");
        return;
    }
    ArmlParseResult loaded = host.LoadSpec(
        "<StackPanel Width=\"400\" Height=\"80\"><TextBlock x:Name=\"Title\" FontSize=\"16\">A</TextBlock></StackPanel>",
        400.0, 80.0);
    if (!loaded.Success) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:load");
        return;
    }
    LayoutSnapshot before = host.GetLayoutSnapshot();
    if (before == null || before.Root == null || before.Root.Children == null
        || before.Root.Children.Count != 1) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:before_tree");
        return;
    }
    LayoutNode child0 = before.Root.Children[0];
    if (child0.TextLines == null || child0.TextLines.Count != 1) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:before_lines");
        return;
    }
    int wBefore = (int)child0.TextLines[0].Width;
    if (wBefore <= 0) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:before_w=" + wBefore);
        return;
    }

    if (!host.ApplyPatch("Root/Title", "Text", "HelloWorldHelloWorld")) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:patch");
        return;
    }
    TextBlock tb = (TextBlock)host.RootElement.Children[0];
    if (tb.Text != "HelloWorldHelloWorld") {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:text=" + tb.Text);
        return;
    }

    LayoutSnapshot after = host.GetLayoutSnapshot();
    if (after == null || after.Root == null || after.Root.Children == null
        || after.Root.Children.Count != 1) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:after_tree");
        return;
    }
    LayoutNode child1 = after.Root.Children[0];
    if (child1.TextLines == null || child1.TextLines.Count != 1) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:after_lines");
        return;
    }
    int wAfter = (int)child1.TextLines[0].Width;
    if (wAfter <= wBefore) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:FAIL:width before="
            + wBefore + " after=" + wAfter);
        return;
    }
    Console.WriteLine("ARC_CASE:g2_apply_patch_text_layout:PASS");
}
"##,
            },
            BatchCase {
                name: "g2_apply_patch_unknown_false",
                src: r##"using Arc;
using Arc.UI.Components;
using Arc.UI.Markup;

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_unknown_false:FAIL:init");
        return;
    }
    ArmlParseResult loaded = host.LoadSpec(
        "<StackPanel><TextBlock x:Name=\"Title\">Keep</TextBlock></StackPanel>",
        200.0, 40.0);
    if (!loaded.Success) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_unknown_false:FAIL:load");
        return;
    }
    if (host.ApplyPatch("Root/Missing", "Text", "Nope")) {
        Console.WriteLine("ARC_CASE:g2_apply_patch_unknown_false:FAIL:accepted");
        return;
    }
    TextBlock tb = (TextBlock)host.RootElement.Children[0];
    if (tb.Text != "Keep") {
        Console.WriteLine("ARC_CASE:g2_apply_patch_unknown_false:FAIL:mutated=" + tb.Text);
        return;
    }
    Console.WriteLine("ARC_CASE:g2_apply_patch_unknown_false:PASS");
}
"##,
            },
            BatchCase {
                name: "g2_loadspec_empty_diag",
                src: r##"using Arc;
using Arc.UI.Components;
using Arc.UI.Markup;

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:g2_loadspec_empty_diag:FAIL:init");
        return;
    }
    ArmlParseResult loaded = host.LoadSpec("");
    if (loaded.Success) {
        Console.WriteLine("ARC_CASE:g2_loadspec_empty_diag:FAIL:accepted_empty");
        return;
    }
    if (loaded.Diagnostics == null || loaded.Diagnostics.Count < 1) {
        Console.WriteLine("ARC_CASE:g2_loadspec_empty_diag:FAIL:no_diag");
        return;
    }
    if (host.RootElement != null) {
        Console.WriteLine("ARC_CASE:g2_loadspec_empty_diag:FAIL:tree_built");
        return;
    }
    Console.WriteLine("ARC_CASE:g2_loadspec_empty_diag:PASS");
}
"##,
            },
            BatchCase {
                name: "g2_reset_clears",
                src: r##"using Arc;
using Arc.UI.Components;
using Arc.UI.Layout;
using Arc.UI.Markup;

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:g2_reset_clears:FAIL:init");
        return;
    }
    ArmlParseResult loaded = host.LoadSpec(
        "<StackPanel><TextBlock x:Name=\"Title\">Hi</TextBlock></StackPanel>",
        120.0, 40.0);
    if (!loaded.Success || host.RootElement == null) {
        Console.WriteLine("ARC_CASE:g2_reset_clears:FAIL:load");
        return;
    }
    host.Reset();
    if (host.RootElement != null) {
        Console.WriteLine("ARC_CASE:g2_reset_clears:FAIL:root");
        return;
    }
    LayoutSnapshot snap = host.GetLayoutSnapshot();
    if (snap != null) {
        Console.WriteLine("ARC_CASE:g2_reset_clears:FAIL:snap");
        return;
    }
    if (host.ApplyPatch("Root/Title", "Text", "x")) {
        Console.WriteLine("ARC_CASE:g2_reset_clears:FAIL:patch");
        return;
    }
    Console.WriteLine("ARC_CASE:g2_reset_clears:PASS");
}
"##,
            },
        ],
        UI_DEPS,
    );

    for name in [
        "g2_loadspec_capture_png",
        "g2_apply_patch_text_layout",
        "g2_apply_patch_unknown_false",
        "g2_loadspec_empty_diag",
        "g2_reset_clears",
    ] {
        let r = batch_case_result(&results, name);
        assert!(
            r.passed,
            "ui_live_preview_g2: case {name} failed: {:?}\nstdout:\n{}",
            r.error, r.stdout
        );
    }
}
