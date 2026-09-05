//! 分发包容器打包与解压（self-update / `arc publish` / toolchain 共用）。
//!
//! 当前统一采用标准 `.zip`（deflate）作为分发包容器——`arc self-update`、
//! `arc publish` 与安装脚本消费同一格式。tar.xz / tar.gz（LLVM 官方
//! 工具链分发包的解析）留待真实发布端点定版后补齐——见 RFC 031 §12 外部依赖。
//!
//! 安全：解压目录穿越防御——仅接受 `zip::ZipFile::enclosed_name()`（拒绝 `..` 与
//! 平台绝对路径）；额外拒绝以 `/` 或 `\` 开头的条目（Windows 下 `/x` 非绝对，
//! 显式防御），条目按 zip 内相对路径落盘。条目自带 Unix 权限位（zip external
//! attrs）时，Unix 提取端按位还原（Windows 无意义）；Windows 产线容器通常不带
//! 权限位，由消费方按各自布局契约补执行位（见 `self_update` staging 恢复）。

use std::io::{Read as _, Write as _};
use std::path::Path;

/// 将 zip 字节解压到 `dest`（`dest` 须已存在）。
pub fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("artifact is not a valid zip: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        // 原始条目名先行防御：`/` 或 `\` 开头在部分平台非绝对路径
        //（`enclosed_name` 会先规范化掉前导斜杠），须在规范化前拦截。
        let raw = file.name();
        if raw.starts_with('/') || raw.starts_with('\\') {
            return Err(format!("zip entry {i} is an absolute path (unsafe): {raw}"));
        }
        let rel = file
            .enclosed_name()
            .ok_or_else(|| format!("zip entry {i} escapes the package root (unsafe path)"))?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.starts_with('/') {
            return Err(format!("zip entry {i} is an absolute path (unsafe): {rel}"));
        }
        let out = dest.join(&rel);
        if file.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out_file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut out_file).map_err(|e| e.to_string())?;
            drop(out_file);
            #[cfg(unix)]
            apply_stored_unix_mode(file.unix_mode(), &out)?;
        }
    }
    Ok(())
}

/// 条目自带 Unix 权限位时应用之（zip external attrs 的权限位）。
///
/// 仅 Unix 提取端生效；Windows 权限位无意义。mode 缺失（Windows 产线容器
/// 常态）保持默认权限，由消费方按布局契约处理。
#[cfg(unix)]
fn apply_stored_unix_mode(mode: Option<u32>, out: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(mode) = mode {
        std::fs::set_permissions(out, std::fs::Permissions::from_mode(mode & 0o7777))
            .map_err(|e| format!("set permissions on {}: {e}", out.display()))?;
    }
    Ok(())
}

/// 将内存文件集（`zip 内相对路径 → 字节`）打包为 zip 字节。
///
/// 供 `arc publish`（`.aopkg` 源码分发包）等生成侧使用；路径一律以 `/`
/// 分隔的 zip 相对路径（调用方负责规范化，目录条目无需显式给出）。
pub fn create_zip(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        for (name, data) in files {
            if name.is_empty() || name.starts_with('/') || name.starts_with('\\') {
                return Err(format!("invalid zip entry name: {name:?}"));
            }
            zip.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
                .map_err(|e| format!("zip entry `{name}`: {e}"))?;
            zip.write_all(data)
                .map_err(|e| format!("zip entry `{name}`: {e}"))?;
        }
        zip.finish().map_err(|e| format!("finish zip: {e}"))?;
    }
    Ok(cursor.into_inner())
}

/// 读取 zip 字节中的单个条目（按 zip 内相对路径精确匹配）。
pub fn read_zip_entry(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("artifact is not a valid zip: {e}"))?;
    let mut file = archive
        .by_name(name)
        .map_err(|e| format!("zip entry `{name}` not found: {e}"))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| format!("read zip entry `{name}`: {e}"))?;
    Ok(buf)
}

/// 列出 zip 字节中的全部文件条目名（跳过目录条目）。
pub fn list_zip_entries(bytes: &[u8]) -> Result<Vec<String>, String> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("artifact is not a valid zip: {e}"))?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        if !file.is_dir() {
            names.push(file.name().to_string());
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arc-archive-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_zip(dir: &Path, entries: &[(&str, &[u8])]) -> Vec<u8> {
        let zip_path = dir.join("a.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, data) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
        std::fs::read(&zip_path).unwrap()
    }

    #[test]
    fn zip_roundtrip() {
        let dir = temp_dir("zip");
        let bytes = make_zip(&dir, &[("pkg/bin/arc.exe", b"binary-bytes")]);
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        extract_zip(&bytes, &out).unwrap();
        assert!(out.join("pkg/bin/arc.exe").is_file());
        assert_eq!(
            std::fs::read(out.join("pkg/bin/arc.exe")).unwrap(),
            b"binary-bytes"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_applies_stored_unix_mode() {
        let dir = temp_dir("mode");
        let zip_path = dir.join("m.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "pkg/bin/tool",
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        let bytes = std::fs::read(&zip_path).unwrap();
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        extract_zip(&bytes, &out).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(out.join("pkg/bin/tool"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777;
            assert_eq!(mode, 0o755);
        }
        #[cfg(not(unix))]
        {
            assert!(out.join("pkg/bin/tool").is_file());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_path_traversal() {
        let dir = temp_dir("traversal");
        let bytes = make_zip(&dir, &[("../evil.txt", b"evil")]);
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        assert!(extract_zip(&bytes, &out).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_absolute_entry() {
        let dir = temp_dir("absolute");
        let bytes = make_zip(&dir, &[("/evil.txt", b"evil")]);
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        assert!(extract_zip(&bytes, &out).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_non_zip() {
        let dir = temp_dir("notzip");
        assert!(extract_zip(b"not a zip", &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_read_list_roundtrip() {
        let files = vec![
            ("pkg/arc.toml".to_string(), b"name = \"x\"\n".to_vec()),
            ("pkg/src/main.as".to_string(), b"void Main() {}".to_vec()),
        ];
        let bytes = create_zip(&files).unwrap();
        assert_eq!(
            read_zip_entry(&bytes, "pkg/src/main.as").unwrap(),
            b"void Main() {}"
        );
        assert!(read_zip_entry(&bytes, "pkg/missing.txt").is_err());
        let mut names = list_zip_entries(&bytes).unwrap();
        names.sort();
        assert_eq!(names, ["pkg/arc.toml", "pkg/src/main.as"]);
        // create_zip 产物可被 extract_zip 消费（同一格式契约）。
        let dir = temp_dir("roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        extract_zip(&bytes, &dir).unwrap();
        assert!(dir.join("pkg/src/main.as").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_zip_rejects_absolute_names() {
        assert!(create_zip(&[("/abs.txt".to_string(), b"x".to_vec())]).is_err());
        assert!(create_zip(&[(String::new(), b"x".to_vec())]).is_err());
    }
}
