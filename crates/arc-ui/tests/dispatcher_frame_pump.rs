//! RFC 037 M-AS1: UiDispatcher / FramePump skeleton source presence.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn ui_dispatcher_skeleton_source_present() {
    let path = workspace_root().join("std/UI/Core/Internal/UIDispatcher.as");
    assert!(
        path.is_file(),
        "missing UIDispatcher.as at {}",
        path.display()
    );
    let content = std::fs::read_to_string(&path).expect("read UIDispatcher.as");
    assert!(content.contains("namespace Arc.UI.Internal"));
    assert!(content.contains("internal class UIDispatcher"));
    assert!(!content.contains("UiDispatcher"));
    assert!(content.contains("Post(UIPriority"));
    assert!(content.contains("InvokeAsync"));
    assert!(content.contains("DrainPostedWork"));
    assert!(content.contains("M-AS1"));
    assert!(!content.contains("async void"));
}

#[test]
fn frame_pump_skeleton_source_present() {
    let path = workspace_root().join("std/UI/Core/Internal/FramePump.as");
    assert!(path.is_file(), "missing FramePump.as at {}", path.display());
    let content = std::fs::read_to_string(&path).expect("read FramePump.as");
    assert!(content.contains("namespace Arc.UI.Internal"));
    assert!(content.contains("PumpOnce"));
    assert!(content.contains("RunAsync"));
    assert!(content.contains("DrainPostedWork"));
}

#[test]
fn application_run_async_entry_present() {
    let path = workspace_root().join("std/UI/Core/Components/Application.as");
    let content = std::fs::read_to_string(&path).expect("read Application.as");
    assert!(content.contains("RunAsync"));
    assert!(content.contains("compat"));
    assert!(content.contains("非终态"));
}

#[test]
fn rfc037_mas1_documented() {
    let path = workspace_root().join("docs/rfc/037-ui.md");
    let content = std::fs::read_to_string(&path).expect("read RFC 037");
    assert!(content.contains("M-AS1"));
    assert!(content.contains("UiDispatcher"));
    assert!(content.contains("FramePump"));
    assert!(content.contains("Post"));
}
