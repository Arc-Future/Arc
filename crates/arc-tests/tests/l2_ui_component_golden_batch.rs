//! L2 批量：AI 原生 · 组件 Golden 最小硬门槛（RFC 037 §10 · ai-native-fidelity-loop §2）。
//!
//! 验收面（headless）：固定 Button/TextBlock fixture → LivePreviewHost.LoadSpec
//! → GetLayoutSnapshot → 结构骨架（Type/Name/整数几何，排除 Margin/TextLines/字体）
//! 与 `goldens/ui/button_textblock_layout.golden.json` 比对。
//!
//! 再生：`UPDATE_UI_COMPONENT_GOLDEN=1 cargo test -p arc-tests --features full-rt --test l2_ui_component_golden_batch`
//!
//! 宣称纪律：仅关「一条组件布局结构 Golden 可 CI」；**不**宣称 G1–G3 /
//! 控件×主题态全集 / 审视回路 / 像素闸 / 保真闭环全部完成。
//!
//! 依赖：`("Arc.UI", "UI/Core")`。需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use std::path::PathBuf;

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];
const GOLDEN_REL: &str = "goldens/ui/button_textblock_layout.golden.json";
const CANON_MARKER: &str = "ARC_COMPONENT_GOLDEN:";

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_REL)
}

#[test]
fn ui_component_golden_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_component_golden",
        &[BatchCase {
            name: "button_textblock_layout_golden",
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

string BuildCanonical(LayoutSnapshot snap) {
    StringBuilder sb = new StringBuilder(512);
    sb.Append("{\"id\":\"button_textblock_layout_v1\",");
    sb.Append("\"fixture\":\"<StackPanel Width=\\\"240\\\" Height=\\\"120\\\"><TextBlock x:Name=\\\"Title\\\" Width=\\\"240\\\" Height=\\\"40\\\" FontSize=\\\"16\\\">Hello</TextBlock><Button x:Name=\\\"OkBtn\\\" Width=\\\"80\\\" Height=\\\"32\\\" Content=\\\"OK\\\"/></StackPanel>\",");
    sb.Append("\"viewport\":{\"w\":");
    sb.Append(((int)snap.ViewportWidth).ToString());
    sb.Append(",\"h\":");
    sb.Append(((int)snap.ViewportHeight).ToString());
    sb.Append("},\"nodes\":[");
    AppendNode(sb, snap.Root, "0", true);
    sb.Append("]}");
    return sb.ToString();
}

void Main() {
    LivePreviewHost host = new LivePreviewHost();
    if (!host.Initialize()) {
        Console.WriteLine("ARC_CASE:button_textblock_layout_golden:FAIL:init");
        return;
    }
    string arml = "<StackPanel Width=\"240\" Height=\"120\"><TextBlock x:Name=\"Title\" Width=\"240\" Height=\"40\" FontSize=\"16\">Hello</TextBlock><Button x:Name=\"OkBtn\" Width=\"80\" Height=\"32\" Content=\"OK\"/></StackPanel>";
    ArmlParseResult loaded = host.LoadSpec(arml, 240.0, 120.0);
    if (!loaded.Success) {
        Console.WriteLine("ARC_CASE:button_textblock_layout_golden:FAIL:load");
        return;
    }
    LayoutSnapshot snap = host.GetLayoutSnapshot();
    if (snap == null || snap.Root == null) {
        Console.WriteLine("ARC_CASE:button_textblock_layout_golden:FAIL:null");
        return;
    }
    if (snap.Root.TypeName != "StackPanel") {
        Console.WriteLine("ARC_CASE:button_textblock_layout_golden:FAIL:root=" + snap.Root.TypeName);
        return;
    }
    if (snap.Root.Children == null || snap.Root.Children.Count != 2) {
        Console.WriteLine("ARC_CASE:button_textblock_layout_golden:FAIL:children");
        return;
    }
    if (snap.Root.Children[0].TypeName != "TextBlock" || snap.Root.Children[0].Name != "Title") {
        Console.WriteLine("ARC_CASE:button_textblock_layout_golden:FAIL:title");
        return;
    }
    if (snap.Root.Children[1].TypeName != "Button" || snap.Root.Children[1].Name != "OkBtn") {
        Console.WriteLine("ARC_CASE:button_textblock_layout_golden:FAIL:button");
        return;
    }
    string canon = BuildCanonical(snap);
    Console.WriteLine("ARC_COMPONENT_GOLDEN:" + canon);
    Console.WriteLine("ARC_CASE:button_textblock_layout_golden:PASS");
}
"##,
        }],
        UI_DEPS,
    );

    let r = batch_case_result(&results, "button_textblock_layout_golden");
    assert!(
        r.passed,
        "ui_component_golden: case failed: {:?}\nstdout:\n{}",
        r.error, r.stdout
    );

    let actual = r
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix(CANON_MARKER))
        .unwrap_or_else(|| panic!("missing {CANON_MARKER} in stdout:\n{}", r.stdout))
        .trim()
        .to_string();
    let path = golden_path();

    if std::env::var("UPDATE_UI_COMPONENT_GOLDEN").is_ok() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create goldens dir");
        }
        std::fs::write(&path, format!("{actual}\n")).unwrap_or_else(|e| {
            panic!("write {}: {e}", path.display());
        });
        return;
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
        actual, expected,
        "component layout golden mismatch; UPDATE_UI_COMPONENT_GOLDEN=1 to regenerate\npath={}",
        path.display()
    );
}
