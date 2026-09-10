//! RFC 037 · TreeView M-VZ4 FlatIndex viewport virtualization contract.
//!
//! Source-level gate: ItemsSource path declares ItemViewport window + recycle
//! pool + arithmetic Extent. Honest boundary: FlatIndex-window only (not full
//! hierarchical path virt).

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_std(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel)).unwrap_or_default()
}

#[test]
fn treeview_declares_flatindex_viewport_window() {
    let tv = read_std("std/UI/Core/Components/TreeView.as");
    assert!(tv.contains("VerticalOffset"), "missing VerticalOffset DP");
    assert!(
        tv.contains("ContentExtentHeight"),
        "missing arithmetic extent"
    );
    assert!(tv.contains("ItemViewport"), "missing ItemViewport");
    assert!(
        tv.contains("MaterializeWindowRows"),
        "missing window materialize"
    );
    assert!(tv.contains("_rowPool"), "missing recycle pool");
    assert!(
        tv.contains("EnsureViewportMaterialization"),
        "missing EnsureViewport"
    );
    assert!(
        tv.contains("FlatIndex-window") || tv.contains("FlatIndex window"),
        "must document FlatIndex-window honest boundary"
    );
}

#[test]
fn treeview_item_declares_bind_virtual_row_and_pool() {
    let item = read_std("std/UI/Core/Components/TreeViewItem.as");
    assert!(item.contains("BindVirtualRow"), "missing BindVirtualRow");
    assert!(item.contains("PrepareForPool"), "missing PrepareForPool");
    assert!(item.contains("VisibleIndex"), "missing VisibleIndex");
    assert!(item.contains("Depth"), "missing Depth");
}

#[test]
fn components_matrix_documents_treeview_mvz4_min() {
    let md = read_std("std/UI/Core/COMPONENTS.md");
    assert!(
        md.contains("M-VZ4") && md.contains("TreeView"),
        "COMPONENTS.md must document TreeView M-VZ4"
    );
    assert!(
        md.contains("FlatIndex") || md.contains("shallow"),
        "must document FlatIndex / shallow virt boundary"
    );
}
