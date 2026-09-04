//! 文件系统操作健壮性工具（Phase 2 自更新 / 工具链共用）。
//!
//! Windows 上 Defender/AV 会对新落盘的二进制做即时扫描，可能在
//! `fs::rename`/`fs::remove_file` 窗口内短暂持有文件句柄（os error 5 拒绝访问）。
//! 自更新器 / 工具链安装是「替换可执行文件」的典型场景，故对有界重试封装
//!（对标 rustup 的 rename 重试）。

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

/// rename 最大尝试次数。
const RENAME_RETRIES: u32 = 12;
/// 尝试间隔。
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(150);

/// `fs::rename` 带重试（Windows 瞬时文件锁）；返回 `io::Result`，可与既有
/// `.map_err(|e| format!(...))` 错误链无缝对接。参数接受 `impl AsRef<Path>`
///（与 `fs::rename` 一致，`Path`/`PathBuf` 皆可）。
pub fn rename_with_retry(from: impl AsRef<Path>, to: impl AsRef<Path>) -> std::io::Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    for attempt in 0..RENAME_RETRIES {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(_e) if attempt + 1 < RENAME_RETRIES => {
                sleep(RENAME_RETRY_DELAY);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// 递归复制目录（`arc component install` 归一化 `include/` 等子目录用）。
pub fn copy_dir_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_roundtrip() {
        let d = std::env::temp_dir().join(format!("arc-fs-util-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let a = d.join("a.txt");
        let b = d.join("b.txt");
        std::fs::write(&a, b"x").unwrap();
        rename_with_retry(&a, &b).unwrap();
        assert!(b.is_file());
        assert!(!a.exists());
        assert_eq!(std::fs::read(&b).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(&d);
    }
}
