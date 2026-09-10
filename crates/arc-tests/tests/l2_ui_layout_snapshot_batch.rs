//! L2 批量：AI 原生布局快照硬门槛（RFC 037 §10 · ai-native-layout-snapshot）。
//!
//! 验收面（headless，无 HWND 交互）：
//! - `snapshot_tree_json`：LivePreviewHost.LoadSpec → GetLayoutSnapshot
//!   → 结构化树（StackPanel/TextBlock）+ 文本行盒 + ToJson 可断言
//! - `snapshot_before_load_null`：未 LoadSpec 时 GetLayoutSnapshot 返回 null
//! - `snapshot_deterministic`：同一 spec 两次快照 JSON 字节一致
//!
//! 宣称纪律：仅关布局快照 headless 硬门槛；**不**宣称 G1–G3 / 渲染回读 / 保真闭环。
//!
//! 依赖：`("Arc.UI", "UI/Core")`。需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];

#[test]
fn ui_layout_snapshot_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_layout_snapshot",
        &[
            BatchCase {
                name: "snapshot_tree_json",
                src: r##"using Arc;
using Arc.UI.Components;
using Arc.UI.Layout;
using Arc.UI.Markup;

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:init");
        return;
    }
    ArmlParseResult loaded = host.LoadSpec(
        "<StackPanel Width=\"200\" Height=\"80\"><TextBlock x:Name=\"Title\" FontSize=\"16\">Hello</TextBlock></StackPanel>",
        200.0, 80.0);
    if (!loaded.Success) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:load");
        return;
    }
    LayoutSnapshot snap = host.GetLayoutSnapshot();
    if (snap == null || snap.Root == null) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:null");
        return;
    }
    if ((int)snap.ViewportWidth != 200 || (int)snap.ViewportHeight != 80) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:viewport");
        return;
    }
    if (snap.Root.TypeName != "StackPanel") {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:root_type=" + snap.Root.TypeName);
        return;
    }
    if (snap.Root.Children == null || snap.Root.Children.Count != 1) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:child_count");
        return;
    }
    LayoutNode child = snap.Root.Children[0];
    if (child.TypeName != "TextBlock" || child.Name != "Title") {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:child");
        return;
    }
    if (child.TextLines == null || child.TextLines.Count != 1) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:text_lines");
        return;
    }
    if (child.TextLines[0].Width <= 0.0 || child.TextLines[0].Height <= 0.0) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:line_box");
        return;
    }
    if (child.Width <= 0.0 || child.Height <= 0.0) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:child_size");
        return;
    }
    string json = snap.ToJson();
    if (json == null || json.Length < 20) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:json_empty");
        return;
    }
    if (json.IndexOf("\"TypeName\":\"StackPanel\"") < 0) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:json_root");
        return;
    }
    if (json.IndexOf("\"Name\":\"Title\"") < 0) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:json_name");
        return;
    }
    if (json.IndexOf("\"TextLines\":[") < 0) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:json_lines");
        return;
    }
    if (json.IndexOf("\"ViewportWidth\":200") < 0) {
        Console.WriteLine("ARC_CASE:snapshot_tree_json:FAIL:json_vw");
        return;
    }
    Console.WriteLine("ARC_CASE:snapshot_tree_json:PASS");
}
"##,
            },
            BatchCase {
                name: "snapshot_before_load_null",
                src: r##"using Arc;
using Arc.UI.Components;
using Arc.UI.Layout;

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:snapshot_before_load_null:FAIL:init");
        return;
    }
    LayoutSnapshot snap = host.GetLayoutSnapshot();
    if (snap != null) {
        Console.WriteLine("ARC_CASE:snapshot_before_load_null:FAIL:expected_null");
        return;
    }
    Console.WriteLine("ARC_CASE:snapshot_before_load_null:PASS");
}
"##,
            },
            BatchCase {
                name: "snapshot_deterministic",
                src: r##"using Arc;
using Arc.UI.Components;
using Arc.UI.Layout;
using Arc.UI.Markup;

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:snapshot_deterministic:FAIL:init");
        return;
    }
    string arml = "<StackPanel Width=\"120\" Height=\"40\"><TextBlock x:Name=\"L\">AB</TextBlock></StackPanel>";
    ArmlParseResult a = host.LoadSpec(arml, 120.0, 40.0);
    if (!a.Success) {
        Console.WriteLine("ARC_CASE:snapshot_deterministic:FAIL:load1");
        return;
    }
    LayoutSnapshot s1 = host.GetLayoutSnapshot();
    if (s1 == null) {
        Console.WriteLine("ARC_CASE:snapshot_deterministic:FAIL:null1");
        return;
    }
    string j1 = s1.ToJson();

    ArmlParseResult b = host.LoadSpec(arml, 120.0, 40.0);
    if (!b.Success) {
        Console.WriteLine("ARC_CASE:snapshot_deterministic:FAIL:load2");
        return;
    }
    LayoutSnapshot s2 = host.GetLayoutSnapshot();
    if (s2 == null) {
        Console.WriteLine("ARC_CASE:snapshot_deterministic:FAIL:null2");
        return;
    }
    string j2 = s2.ToJson();
    if (j1 != j2) {
        Console.WriteLine("ARC_CASE:snapshot_deterministic:FAIL:mismatch");
        return;
    }
    Console.WriteLine("ARC_CASE:snapshot_deterministic:PASS");
}
"##,
            },
        ],
        UI_DEPS,
    );

    for name in [
        "snapshot_tree_json",
        "snapshot_before_load_null",
        "snapshot_deterministic",
    ] {
        let r = batch_case_result(&results, name);
        assert!(
            r.passed,
            "ui_layout_snapshot: case {name} failed: {:?}\nstdout:\n{}",
            r.error, r.stdout
        );
    }
}
