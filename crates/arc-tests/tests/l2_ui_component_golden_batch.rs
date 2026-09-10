//! L2 批量：AI 原生 · 组件 Golden 布局结构硬门槛（RFC 037 §10 · ai-native-fidelity-loop §2）。
//!
//! 验收面（headless）：固定 fixture → LivePreviewHost.LoadSpec
//! → GetLayoutSnapshot → 结构骨架（Type/Name/整数几何，排除 Margin/TextLines/字体）
//! 与 `goldens/ui/*.golden.json` 比对。
//!
//! 当前矩阵：Button/TextBlock + CheckBox（IsChecked=true）各一条布局结构 Golden。
//!
//! 再生：`UPDATE_UI_COMPONENT_GOLDEN=1 cargo test -p arc-tests --features full-rt --test l2_ui_component_golden_batch`
//!
//! 宣称纪律：仅关「上述布局结构 Golden 可 CI」；**不**宣称 G1–G3 /
//! 控件×主题态全集 / 审视回路 / 像素闸 / 保真闭环全部完成。
//!
//! 依赖：`("Arc.UI", "UI/Core")`。需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use std::path::PathBuf;

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];
const CANON_MARKER: &str = "ARC_COMPONENT_GOLDEN:";

/// `(golden id 前缀, 相对 CARGO_MANIFEST_DIR 的路径)`
const GOLDEN_FILES: &[(&str, &str)] = &[
    (
        "button_textblock_layout_v1",
        "goldens/ui/button_textblock_layout.golden.json",
    ),
    ("checkbox_layout_v1", "goldens/ui/checkbox_layout.golden.json"),
];

fn golden_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn extract_golden_id(canon: &str) -> Option<&str> {
    let key = "\"id\":\"";
    let start = canon.find(key)? + key.len();
    let end = canon[start..].find('"')? + start;
    Some(&canon[start..end])
}

#[test]
fn ui_component_golden_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_component_golden",
        &[BatchCase {
            name: "component_layout_golden_matrix",
            src: r##"using Arc;
using Arc.Text;
using Arc.UI.Components;
using Arc.UI.Layout;
using Arc.UI.Markup;

string JsonEsc(string value) {
    if (value == null) {
        return "null";
    }
    StringBuilder sb = new StringBuilder(value.Length + 8);
    sb.Append("\"");
    for (int i = 0; i < value.Length; i++) {
        char ch = value[i];
        if (ch == '"') {
            sb.Append("\\\"");
        } else if (ch == '\\') {
            sb.Append("\\\\");
        } else {
            sb.Append(ch);
        }
    }
    sb.Append("\"");
    return sb.ToString();
}

void AppendNode(StringBuilder sb, LayoutNode node, string path, bool first) {
    if (!first) {
        sb.Append(",");
    }
    sb.Append("{\"path\":");
    sb.Append(JsonEsc(path));
    sb.Append(",\"type\":");
    sb.Append(JsonEsc(node.TypeName));
    sb.Append(",\"name\":");
    sb.Append(JsonEsc(node.Name));
    sb.Append(",\"x\":");
    sb.Append(((int)node.X).ToString());
    sb.Append(",\"y\":");
    sb.Append(((int)node.Y).ToString());
    sb.Append(",\"w\":");
    sb.Append(((int)node.Width).ToString());
    sb.Append(",\"h\":");
    sb.Append(((int)node.Height).ToString());
    sb.Append(",\"visible\":");
    if (node.Visible) {
        sb.Append("true");
    } else {
        sb.Append("false");
    }
    sb.Append("}");
    if (node.Children == null) {
        return;
    }
    for (int i = 0; i < node.Children.Count; i++) {
        string childPath = path + "." + i.ToString();
        AppendNode(sb, node.Children[i], childPath, false);
    }
}

string BuildCanonical(string id, string fixtureArml, LayoutSnapshot snap) {
    StringBuilder sb = new StringBuilder(512);
    sb.Append("{\"id\":");
    sb.Append(JsonEsc(id));
    sb.Append(",\"fixture\":");
    sb.Append(JsonEsc(fixtureArml));
    sb.Append(",\"viewport\":{\"w\":");
    sb.Append(((int)snap.ViewportWidth).ToString());
    sb.Append(",\"h\":");
    sb.Append(((int)snap.ViewportHeight).ToString());
    sb.Append("},\"nodes\":[");
    AppendNode(sb, snap.Root, "0", true);
    sb.Append("]}");
    return sb.ToString();
}

bool RunFixture(
    string caseTag,
    string id,
    string arml,
    double vw,
    double vh,
    string expectChildType,
    string expectChildName) {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:" + caseTag + ":init");
        return false;
    }
    ArmlParseResult loaded = host.LoadSpec(arml, vw, vh);
    if (!loaded.Success) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:" + caseTag + ":load");
        return false;
    }
    LayoutSnapshot snap = host.GetLayoutSnapshot();
    if (snap == null || snap.Root == null) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:" + caseTag + ":null");
        return false;
    }
    if (snap.Root.TypeName != "StackPanel") {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:" + caseTag + ":root=" + snap.Root.TypeName);
        return false;
    }
    if (snap.Root.Children == null || snap.Root.Children.Count < 1) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:" + caseTag + ":children");
        return false;
    }
    LayoutNode child = snap.Root.Children[0];
    if (child.TypeName != expectChildType || child.Name != expectChildName) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:" + caseTag + ":child");
        return false;
    }
    string canon = BuildCanonical(id, arml, snap);
    Console.WriteLine("ARC_COMPONENT_GOLDEN:" + canon);
    return true;
}

void Main() {
    string btnArml = "<StackPanel Width=\"240\" Height=\"120\"><TextBlock x:Name=\"Title\" Width=\"240\" Height=\"40\" FontSize=\"16\">Hello</TextBlock><Button x:Name=\"OkBtn\" Width=\"80\" Height=\"32\" Content=\"OK\"/></StackPanel>";
    LivePreviewHost hostBtn = new LivePreviewHost();
    if (!hostBtn.Initialize()) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:btn:init");
        return;
    }
    ArmlParseResult loadedBtn = hostBtn.LoadSpec(btnArml, 240.0, 120.0);
    if (!loadedBtn.Success) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:btn:load");
        return;
    }
    LayoutSnapshot snapBtn = hostBtn.GetLayoutSnapshot();
    if (snapBtn == null || snapBtn.Root == null) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:btn:null");
        return;
    }
    if (snapBtn.Root.TypeName != "StackPanel") {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:btn:root=" + snapBtn.Root.TypeName);
        return;
    }
    if (snapBtn.Root.Children == null || snapBtn.Root.Children.Count != 2) {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:btn:children");
        return;
    }
    if (snapBtn.Root.Children[0].TypeName != "TextBlock" || snapBtn.Root.Children[0].Name != "Title") {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:btn:title");
        return;
    }
    if (snapBtn.Root.Children[1].TypeName != "Button" || snapBtn.Root.Children[1].Name != "OkBtn") {
        Console.WriteLine("ARC_CASE:component_layout_golden_matrix:FAIL:btn:button");
        return;
    }
    Console.WriteLine("ARC_COMPONENT_GOLDEN:" + BuildCanonical("button_textblock_layout_v1", btnArml, snapBtn));

    string checkArml = "<StackPanel Width=\"240\" Height=\"48\"><CheckBox x:Name=\"Feature\" Width=\"200\" Height=\"32\" Content=\"Enable\" IsChecked=\"true\"/></StackPanel>";
    if (!RunFixture("check", "checkbox_layout_v1", checkArml, 240.0, 48.0, "CheckBox", "Feature")) {
        return;
    }

    Console.WriteLine("ARC_CASE:component_layout_golden_matrix:PASS");
}
"##,
        }],
        UI_DEPS,
    );

    let r = batch_case_result(&results, "component_layout_golden_matrix");
    assert!(
        r.passed,
        "ui_component_golden: case failed: {:?}\nstdout:\n{}",
        r.error, r.stdout
    );

    let mut by_id: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in r.stdout.lines() {
        let Some(rest) = line.strip_prefix(CANON_MARKER) else {
            continue;
        };
        let canon = rest.trim();
        let id = extract_golden_id(canon).unwrap_or_else(|| {
            panic!("missing id in golden payload:\n{canon}")
        });
        by_id.insert(id.to_string(), canon.to_string());
    }

    let update = std::env::var("UPDATE_UI_COMPONENT_GOLDEN").is_ok();
    for (id, rel) in GOLDEN_FILES {
        let actual = by_id.get(*id).unwrap_or_else(|| {
            panic!(
                "missing golden id `{id}` in stdout (have {:?}):\n{}",
                by_id.keys().collect::<Vec<_>>(),
                r.stdout
            )
        });
        let path = golden_path(rel);
        if update {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create goldens dir");
            }
            std::fs::write(&path, format!("{actual}\n")).unwrap_or_else(|e| {
                panic!("write {}: {e}", path.display());
            });
            continue;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| {
                panic!(
                    "missing {}; regenerate with UPDATE_UI_COMPONENT_GOLDEN=1 cargo test -p arc-tests --features full-rt --test l2_ui_component_golden_batch ({e})",
                    path.display()
                );
            })
            .trim()
            .to_string();
        assert_eq!(
            actual, &expected,
            "component layout golden mismatch for `{id}`; UPDATE_UI_COMPONENT_GOLDEN=1 to regenerate\npath={}",
            path.display()
        );
    }
}
