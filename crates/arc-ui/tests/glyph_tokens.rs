//! Syntax token colors (RFC 037 alignment). 文本绘制由 wgpu 唯一后端承担
//! （WgpuRender.DrawText），本测试仅校验 token 颜色常量的 RFC 037 对齐。

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn syntax_tokens_header_aligns_rfc037_primary() {
    let hdr = fs::read_to_string(
        repo_root().join("crates/runtime-ui/platform/common/rt_ui_syntax_tokens.h"),
    )
    .expect("rt_ui_syntax_tokens.h");
    assert!(
        hdr.contains("RT_UI_COLOR_PRIMARY"),
        "keyword must use RFC 037 Primary"
    );
    assert!(
        hdr.contains("RT_UI_COLOR_TEXT_SECONDARY"),
        "comment must use RFC 037 Secondary"
    );
    assert!(
        hdr.contains("RT_UI_COLOR_TEXT_PRIMARY"),
        "default must use RFC 037 Text.Primary"
    );
    assert!(
        hdr.contains("rt_ui_syntax_token_color"),
        "must expose token color resolver"
    );
}
