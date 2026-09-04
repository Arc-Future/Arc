//! RFC 037 §4 · M-VZ1 ItemsControl / VirtualizingStackPanel contract tests.
//!
//! Verifies std sources declare viewport virtualization without requiring
//! a 100k-item e2e fixture on every CI run.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_std(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel)).unwrap_or_default()
}

#[test]
fn item_viewport_arithmetic_extent() {
    let vp = read_std("std/UI/Core/Layout/ItemViewport.as");
    assert!(vp.contains("ExtentHeight"), "missing ExtentHeight");
    assert!(vp.contains("FirstIndex"), "missing FirstIndex");
    assert!(vp.contains("LastIndex"), "missing LastIndex");
}

#[test]
fn virtualizing_stack_panel_declares_viewport_window() {
    let vsp = read_std("std/UI/Core/Components/Layout/VirtualizingStackPanel.as");
    assert!(
        vsp.contains("EnsureViewportMaterialization"),
        "missing viewport materialize"
    );
    assert!(vsp.contains("ExtentHeight"), "missing arithmetic extent");
    assert!(vsp.contains("CacheLengthBefore"), "missing cache before");
}

#[test]
fn item_container_generator_recycle_pool_not_full_refresh() {
    let gen = read_std("std/UI/Core/Components/ItemContainerGenerator.as");
    assert!(gen.contains("EnsureRange"), "missing EnsureRange");
    assert!(gen.contains("_recyclePool"), "missing recycle pool");
    assert!(
        !gen.contains("void Refresh("),
        "must not expose full Refresh materialization"
    );
}

#[test]
fn items_control_uses_virtualizing_panel() {
    let ic = read_std("std/UI/Core/Components/ItemsControl.as");
    assert!(
        ic.contains("VirtualizingStackPanel"),
        "must use VirtualizingStackPanel host"
    );
    assert!(
        ic.contains("ContentExtentHeight"),
        "missing arithmetic extent surface"
    );
}

#[test]
fn components_matrix_documents_items_virtualization() {
    let md = read_std("std/UI/Core/COMPONENTS.md");
    assert!(
        md.contains("VirtualizingStackPanel"),
        "COMPONENTS.md must list VirtualizingStackPanel"
    );
    assert!(md.contains("M-VZ1"), "must mark M-VZ1 status");
}
