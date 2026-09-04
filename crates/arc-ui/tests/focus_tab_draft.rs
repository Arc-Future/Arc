//! RFC 037 M-focus Draft: FocusManager source presence (compile chain via examples).

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn focus_manager_draft_source_present() {
    let path = workspace_root().join("std/UI/Core/Internal/FocusManager.as");
    assert!(
        path.is_file(),
        "missing FocusManager.as at {}",
        path.display()
    );
    let content = std::fs::read_to_string(&path).expect("read FocusManager.as");
    assert!(content.contains("namespace Arc.UI.Internal"));
    assert!(content.contains("RegisterTabStop"));
    assert!(content.contains("RouteKey"));
}

/// RFC 037 §8（M-focus2 契约化修订）：单一键盘通道 ABI 为
/// `rt_ui_dispatch_key`/`rt_ui_dispatch_text`（旧 Draft `rt_ui_set_keyboard_handler`
/// 固定槽位设计已废弃）。
#[test]
fn focus_keyboard_appendix_present() {
    let path = workspace_root().join("docs/rfc/037-ui.md");
    assert!(path.is_file(), "missing focus appendix");
    let content = std::fs::read_to_string(&path).expect("read appendix");
    assert!(content.contains("M-focus"));
    assert!(content.contains("rt_ui_dispatch_key"));
    assert!(content.contains("rt_ui_dispatch_text"));
}
