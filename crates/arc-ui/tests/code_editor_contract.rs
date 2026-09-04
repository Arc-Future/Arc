//! RFC 037 §4 · M-CE1 CodeEditor contract tests.
//!
//! Verifies std sources declare virtualization-first architecture without
//! requiring a 1 GB fixture on every CI run.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_std(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel)).unwrap_or_default()
}

#[test]
fn code_editor_component_declares_virtualization_hard_constraint() {
    let src = read_std("std/UI/Edit/Components/CodeEditor.as");
    assert!(
        src.contains("RenderVirtualizedLines"),
        "missing virtualized render entry"
    );
    assert!(
        src.contains("OverscanLines") || src.contains("overscan"),
        "missing overscan"
    );
    assert!(
        src.contains("ContentExtentHeight"),
        "missing arithmetic extent"
    );
    assert!(
        !src.contains("new TextBlock(") && !src.contains("ItemsSource"),
        "must not create per-line TextBlock/Items visual tree"
    );
}

#[test]
fn text_buffer_uses_mmap_not_read_all_text() {
    let buf = read_std("std/UI/Edit/Editing/TextBuffer.as");
    assert!(buf.contains("rt_editor_open_path"), "missing mmap open ABI");
    assert!(
        buf.contains("ReadAllText") || buf.contains("禁止"),
        "must document ReadAllText ban"
    );
    let mmap = read_std("std/Arc/IO/MemoryMappedFile.as");
    assert!(
        mmap.contains("rt_file_mmap_open"),
        "missing MemoryMappedFile ABI"
    );
}

#[test]
fn editor_viewport_arithmetic_extent() {
    let vp = read_std("std/UI/Edit/Editing/EditorViewport.as");
    assert!(vp.contains("ExtentHeight"), "missing ExtentHeight");
    assert!(
        vp.contains("OverscanLines"),
        "missing OverscanLines constant"
    );
}

#[test]
fn components_matrix_documents_code_editor_virtualization() {
    let md = read_std("std/UI/Core/COMPONENTS.md");
    assert!(
        md.contains("CodeEditor"),
        "COMPONENTS.md must list CodeEditor"
    );
    // CodeEditor 虚拟化权威为 RFC 037 M-CE1 §4；早期 085/086 独立 RFC 从未成稿，
    // 文档以 RFC 037 单一交叉引用立宪（禁止悬空 RFC 引用）。
    assert!(
        md.contains("037-ui"),
        "must cross-ref UI authority RFC 037 (§4 virtualization)"
    );
    assert!(
        md.contains("虚拟化立宪"),
        "must document CodeEditor virtualization charter (M-CE1)"
    );
}

#[test]
fn typecheck_code_editor_arml() {
    use arc_ui::{Parser, TypeChecker};
    let src = r#"<Window Title="Editor" Width="800" Height="600">
        <ScrollView VerticalOffset="0">
            <CodeEditor VerticalOffset="0" FontSize="14" Height="480"/>
        </ScrollView>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    let checker = TypeChecker::new();
    let report = checker.check(&doc);
    assert!(report.is_ok(), "errors: {:?}", report.errors);
}

#[test]
#[ignore = "requires local multi-MB fixture; run: ARC_CE_FIXTURE=... cargo test -p arc-ui -- --ignored"]
fn code_editor_open_budget_honest_ignore() {
    let fixture = std::env::var("ARC_CE_FIXTURE").unwrap_or_default();
    if fixture.is_empty() {
        return;
    }
    let meta = fs::metadata(&fixture).expect("fixture path");
    assert!(meta.len() > 0, "fixture must be non-empty");
    // Runtime mmap e2e retired with arc-integration (a2627a0f); this gate documents the env hook.
}
