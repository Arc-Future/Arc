//! `arc publish`：`.aopkg` 源码分发包（RFC 017 源码打包 / RFC 031 §13）。
//!
//! ## 包形态（zip 容器，顶层目录 `<name>-<version>/`）
//!
//! ```text
//! <name>-<version>/
//! ├── arc.toml                项目清单（身份与依赖声明）
//! ├── **.as / **.arml         源码（递归收集；排除 obj/ bin/ target/ dist/ .git/）
//! ├── native/**               项目自定义 native 契约（全文件保留）
//! └── FILES.json              完整性清单：逐文件 SHA256 + size（路径相对顶层目录）
//! ```
//!
//! 签名 = 对 `FILES.json` 原始字节的分离签名 `<pkg>.aopkg.sig`（单行
//! `<64-hex 公钥> <64-hex 签名>`）——与发布 manifest（[crate::release]）同一密钥、
//! 同一信任锚（编译期内置公钥 / `$ARC_RELEASE_PUBKEY` 覆盖）、同一协议。
//! `arc publish --verify <PKG>` 为消费端校验入口。
//!
//! ## 边界（对齐 RFC 017 源码打包核心裁决）
//!
//! `.aopkg` 是**源码分发包**：不编译、不做依赖求解、无预编译二进制——
//! 消费方解包后以 `[dependencies] path` 引用即参与正常构建。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::release_sign::{parse_hex32, ED25519_SEED_LEN};

/// FILES 清单文件名（包顶层目录下）。
pub const FILES_MANIFEST_NAME: &str = "FILES.json";
/// FILES 清单 schema 版本（破坏性变更时 +1，verify 拒绝未知 schema）。
pub const FILES_SCHEMA_VERSION: u32 = 1;
/// 分发包扩展名。
pub const AOPKG_EXTENSION: &str = "aopkg";

/// `arc publish` 参数（由 CLI 构建）。
#[derive(Debug, Clone, Default)]
pub struct PublishOptions {
    /// 项目路径——项目目录或 `arc.toml` 路径；缺省当前目录。
    pub project: Option<PathBuf>,
    /// 输出目录（缺省 `<project>/dist/`）。
    pub output: Option<PathBuf>,
    /// 签名 seed（64 hex；缺省 `$ARC_RELEASE_SIGNING_KEY`；两者皆无则不签名）。
    pub key: Option<String>,
}

/// `arc publish --verify` 参数（由 CLI 构建）。
#[derive(Debug, Clone, Default)]
pub struct VerifyOptions {
    /// 分发包路径（`.aopkg`）。
    pub package: PathBuf,
    /// 分离签名路径（缺省取 `<pkg>.aopkg.sig`；不存在则仅做清单完整性校验）。
    pub sig: Option<PathBuf>,
}

/// FILES 清单（包内自描述；签名覆盖其原始字节）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilesManifest {
    pub schema_version: u32,
    pub package: String,
    pub version: String,
    pub files: Vec<FilesEntry>,
}

/// FILES 清单条目（路径相对包顶层目录，`/` 分隔，字典序）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilesEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// 递归收集包内容：`.as` / `.arml` 源码 + `native/` 全文件。
///
/// 排除目录口径与 `loader::collect_as_files` 一致（obj/bin/target/.git），
/// 另排除 `dist/`（publish 自身输出，防包套包）。
fn collect_package_files(root: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if matches!(fname, "obj" | "bin" | "target" | "dist" | ".git") {
                continue;
            }
            collect_package_files(&path, base, out);
            continue;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "as" || e == "arml");
        let native_ok = path
            .strip_prefix(base)
            .ok()
            .and_then(|p| p.components().next())
            .is_some_and(|c| c.as_os_str() == "native");
        if !ext_ok && !native_ok {
            continue;
        }
        let rel = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(rel);
    }
}

/// 解析项目根与 manifest：路径可为项目目录或 `arc.toml` 文件（缺省当前目录）。
fn resolve_project(
    project: Option<&Path>,
) -> Result<(PathBuf, crate::manifest::ArcManifest), String> {
    let start = project
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // 精确项目定位：publish 不向上行走——目录本身须含 arc.toml，
    // 防止在嵌套目录中误发布父项目。
    let dir = if start.is_file() {
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    let manifest_path = dir.join("arc.toml");
    if !manifest_path.is_file() {
        return Err(format!(
            "no `arc.toml` in \"{}\" — publish packages a single project; \
             workspace solutions are not publishable (run inside a member project)",
            dir.display()
        ));
    }
    let manifest = match crate::manifest::ArcManifest::load(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            // 仅含 `[workspace]` 的解决方案根不可发布——给出定向指引而非泛化解析错误。
            if matches!(
                crate::manifest::WorkspaceSection::from_file(&manifest_path),
                Ok(ws) if ws.is_solution()
            ) {
                return Err(
                    "refusing to publish a workspace solution — run `arc publish` inside \
                     a member project"
                        .to_string(),
                );
            }
            return Err(format!(
                "invalid arc.toml at {}: {e}",
                manifest_path.display()
            ));
        }
    };
    Ok((dir, manifest))
}

/// 解析签名 seed：`--key` > `$ARC_RELEASE_SIGNING_KEY`；皆无 → None（不签名）。
fn resolve_signing_seed(key: Option<&str>) -> Result<Option<[u8; ED25519_SEED_LEN]>, String> {
    let seed_hex = match key {
        Some(k) if !k.trim().is_empty() => Some(k.trim().to_string()),
        Some(_) => None,
        None => crate::env::env_var(crate::release::RELEASE_SIGNING_KEY_ENV),
    };
    match seed_hex {
        None => Ok(None),
        Some(hex) => Ok(Some(
            parse_hex32(&hex).map_err(|e| format!("invalid signing seed: {e}"))?,
        )),
    }
}

/// `arc publish`：打包项目为 `.aopkg`（+ 可选分离签名）。
pub fn run_publish(opts: &PublishOptions) -> Result<(), String> {
    let (root, manifest) = resolve_project(opts.project.as_deref())?;
    if manifest.workspace.is_solution() {
        return Err(
            "refusing to publish a workspace solution — run `arc publish` inside a member project"
                .to_string(),
        );
    }
    let name = manifest.package.name.clone();
    let version = manifest.package.version.trim().to_string();
    if name.trim().is_empty() {
        return Err("publish requires `[package].name` in arc.toml".to_string());
    }
    if version.is_empty() {
        return Err(format!(
            "publish requires `[package].version` in arc.toml (package `{name}`)"
        ));
    }

    let mut rels = Vec::new();
    collect_package_files(&root, &root, &mut rels);
    // 包身份文件：arc.toml 恒入包（walker 只收源码/契约，不收清单本身）。
    rels.push("arc.toml".to_string());
    rels.sort();
    rels.dedup();
    if rels.is_empty() {
        return Err(format!(
            "no `.as` / `.arml` / `native/` files found under {} (nothing to package)",
            root.display()
        ));
    }

    let top_dir = format!("{name}-{version}");
    let mut files = Vec::new();
    let mut entries = Vec::new();
    for rel in &rels {
        let abs = root.join(rel.replace('/', "\\"));
        let bytes = std::fs::read(&abs).map_err(|e| format!("read {}: {e}", abs.display()))?;
        entries.push(FilesEntry {
            path: rel.clone(),
            sha256: crate::hash::content_sha256(&bytes),
            size: bytes.len() as u64,
        });
        files.push((format!("{top_dir}/{rel}"), bytes));
    }
    let files_manifest = FilesManifest {
        schema_version: FILES_SCHEMA_VERSION,
        package: name.clone(),
        version: version.clone(),
        files: entries,
    };
    let files_bytes = serde_json::to_vec_pretty(&files_manifest)
        .map_err(|e| format!("serialize {FILES_MANIFEST_NAME}: {e}"))?;
    files.push((
        format!("{top_dir}/{FILES_MANIFEST_NAME}"),
        files_bytes.clone(),
    ));

    let pkg_bytes = crate::archive::create_zip(&files)?;
    let out_dir = opts.output.clone().unwrap_or_else(|| root.join("dist"));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    let pkg_path = out_dir.join(format!("{top_dir}.{AOPKG_EXTENSION}"));
    std::fs::write(&pkg_path, &pkg_bytes)
        .map_err(|e| format!("write {}: {e}", pkg_path.display()))?;
    println!(
        "packed {} ({} files, {} bytes)",
        pkg_path.display(),
        rels.len(),
        pkg_bytes.len()
    );

    match resolve_signing_seed(opts.key.as_deref())? {
        Some(seed) => {
            let sig_text = crate::release::sign_manifest_bytes(&files_bytes, &seed)
                .map_err(|e| e.to_string())?;
            let sig_path = out_dir.join(format!("{top_dir}.{AOPKG_EXTENSION}.sig"));
            std::fs::write(&sig_path, sig_text.as_bytes())
                .map_err(|e| format!("write {}: {e}", sig_path.display()))?;
            println!("signed: {}", sig_path.display());
        }
        None => {
            println!(
                "unsigned package (set ${} or --key <seed> to sign; generate via \
                 `arc release keygen`)",
                crate::release::RELEASE_SIGNING_KEY_ENV
            );
        }
    }
    Ok(())
}

/// 定位包内 `FILES.json` 条目（顶层目录内恰好一份）。
fn locate_files_manifest(entries: &[String]) -> Result<String, String> {
    let matches: Vec<&String> = entries
        .iter()
        .filter(|e| *e == FILES_MANIFEST_NAME || e.ends_with(&format!("/{FILES_MANIFEST_NAME}")))
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!(
            "package has no {FILES_MANIFEST_NAME} (not an .aopkg source package)"
        )),
        _ => Err(format!(
            "package contains multiple {FILES_MANIFEST_NAME} entries (ambiguous)"
        )),
    }
}

/// `arc publish --verify`：清单完整性（逐文件 SHA256/size）+ 可选分离签名验签。
pub fn run_verify(opts: &VerifyOptions) -> Result<(), String> {
    let pkg_bytes = std::fs::read(&opts.package)
        .map_err(|e| format!("read {}: {e}", opts.package.display()))?;
    let entries = crate::archive::list_zip_entries(&pkg_bytes)?;
    let files_entry = locate_files_manifest(&entries)?;
    let files_bytes = crate::archive::read_zip_entry(&pkg_bytes, &files_entry)?;
    let manifest: FilesManifest = serde_json::from_slice(&files_bytes)
        .map_err(|e| format!("{FILES_MANIFEST_NAME} is not valid JSON: {e}"))?;
    if manifest.schema_version != FILES_SCHEMA_VERSION {
        return Err(format!(
            "{FILES_MANIFEST_NAME} schema v{} is not supported (expected v{FILES_SCHEMA_VERSION})",
            manifest.schema_version
        ));
    }
    let top_dir = format!("{}-{}", manifest.package, manifest.version);
    let prefix = format!("{top_dir}/");

    // 包内容封闭性：除 FILES.json 外，每个 zip 条目都必须在清单内且路径
    // 位于顶层目录下（防夹带 / 防清单与内容漂移）。
    let expected: std::collections::BTreeSet<&str> =
        manifest.files.iter().map(|f| f.path.as_str()).collect();
    let mut failures = Vec::new();
    for entry in &entries {
        if entry == &files_entry {
            continue;
        }
        let Some(rel) = entry.strip_prefix(&prefix) else {
            failures.push(format!("unexpected entry outside `{top_dir}/`: {entry}"));
            continue;
        };
        if !expected.contains(rel) {
            failures.push(format!("entry not listed in {FILES_MANIFEST_NAME}: {rel}"));
        }
    }

    for file in &manifest.files {
        let entry = format!("{prefix}{}", file.path);
        let bytes = match crate::archive::read_zip_entry(&pkg_bytes, &entry) {
            Ok(b) => b,
            Err(_) => {
                failures.push(format!("missing entry: {}", file.path));
                continue;
            }
        };
        if bytes.len() as u64 != file.size {
            failures.push(format!(
                "size mismatch for `{}`: expected {}, got {}",
                file.path,
                file.size,
                bytes.len()
            ));
            continue;
        }
        let actual = crate::hash::content_sha256(&bytes);
        if actual != file.sha256.to_ascii_lowercase() {
            failures.push(format!(
                "sha256 mismatch for `{}`: expected {}, got {}",
                file.path, file.sha256, actual
            ));
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "package verification failed ({} file(s) checked):\n  - {}",
            manifest.files.len(),
            failures.join("\n  - ")
        ));
    }

    // 分离签名：`--sig` > 包旁 `<pkg>.aopkg.sig`；无签名文件 = 仅完整性校验。
    let sig_path = opts.sig.clone().unwrap_or_else(|| {
        let mut p = opts.package.clone().into_os_string();
        p.push(".sig");
        PathBuf::from(p)
    });
    if sig_path.is_file() {
        let sig_text = std::fs::read_to_string(&sig_path)
            .map_err(|e| format!("read {}: {e}", sig_path.display()))?;
        crate::release::verify_manifest_bytes(&files_bytes, &sig_text)
            .map_err(|e| format!("signature verification failed: {e}"))?;
        let pubkey = sig_text.split_whitespace().next().unwrap_or("");
        println!(
            "signature ok: {} v{} ({} file(s), pubkey {}…)",
            manifest.package,
            manifest.version,
            manifest.files.len(),
            &pubkey[..pubkey.len().min(12)]
        );
    } else {
        println!(
            "unsigned package: {} v{} — {FILES_MANIFEST_NAME} integrity verified ({} file(s)); \
             no signature file at {}",
            manifest.package,
            manifest.version,
            manifest.files.len(),
            sig_path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arc-publish-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 最小项目夹具：arc.toml + 两个源文件 + arml + native 契约 + 应排除的杂物。
    fn fixture_project(label: &str) -> PathBuf {
        let root = temp_dir(label);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("native")).unwrap();
        std::fs::create_dir_all(root.join("obj")).unwrap();
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(
            root.join("arc.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"1\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.as"),
            "namespace Demo;\nvoid Main() {}\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.as"), "namespace Demo;\n").unwrap();
        std::fs::write(root.join("App.arml"), "<Page/>").unwrap();
        std::fs::write(root.join("native/custom.ani"), "module custom {}\n").unwrap();
        // 应被排除：构建产物与历史发布物。
        std::fs::write(root.join("obj/junk.as"), "namespace Junk;\n").unwrap();
        std::fs::write(root.join("dist/old.aopkg"), b"old").unwrap();
        root
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn publish_and_verify_roundtrip_signed() {
        let _env_guard = env_lock();
        let (seed, pk) = {
            let s = [0x7au8; ED25519_SEED_LEN];
            let (pk, _) = crate::release_sign::sign_message(&s, b"probe").unwrap();
            (s, pk)
        };
        let root = fixture_project("roundtrip");
        let out = temp_dir("roundtrip-out");
        std::env::set_var(
            crate::release::RELEASE_PUBKEY_ENV,
            crate::release_sign::hex_encode(&pk),
        );
        run_publish(&PublishOptions {
            project: Some(root.clone()),
            output: Some(out.clone()),
            key: Some(crate::release_sign::hex_encode(&seed)),
        })
        .unwrap();
        let pkg = out.join("demo-0.1.0.aopkg");
        let sig = out.join("demo-0.1.0.aopkg.sig");
        assert!(pkg.is_file(), "{pkg:?}");
        assert!(sig.is_file(), "{sig:?}");
        // 消费端校验：默认取包旁 .sig，走信任锚验签。
        run_verify(&VerifyOptions {
            package: pkg,
            sig: None,
        })
        .unwrap();
        std::env::remove_var(crate::release::RELEASE_PUBKEY_ENV);
    }

    #[test]
    fn publish_excludes_build_dirs_and_keeps_native() {
        let _env_guard = env_lock();
        let root = fixture_project("excludes");
        let out = temp_dir("excludes-out");
        run_publish(&PublishOptions {
            project: Some(root.clone()),
            output: Some(out.clone()),
            key: None,
        })
        .unwrap();
        let pkg_bytes = std::fs::read(out.join("demo-0.1.0.aopkg")).unwrap();
        let names = crate::archive::list_zip_entries(&pkg_bytes).unwrap();
        let joined = names.join("\n");
        assert!(joined.contains("demo-0.1.0/arc.toml"), "{joined}");
        assert!(joined.contains("demo-0.1.0/native/custom.ani"), "{joined}");
        assert!(joined.contains("demo-0.1.0/src/main.as"), "{joined}");
        assert!(joined.contains("demo-0.1.0/App.arml"), "{joined}");
        assert!(!joined.contains("obj/"), "{joined}");
        assert!(!joined.contains("dist/"), "{joined}");
        assert!(
            joined.ends_with(&format!("demo-0.1.0/{FILES_MANIFEST_NAME}")),
            "{joined}"
        );
        // 无签名 key → 未签名包；verify 仍以完整性模式通过。
        assert!(!out.join("demo-0.1.0.aopkg.sig").exists());
        run_verify(&VerifyOptions {
            package: out.join("demo-0.1.0.aopkg"),
            sig: None,
        })
        .unwrap();
    }

    #[test]
    fn verify_rejects_content_tampering() {
        let root = fixture_project("tamper");
        let out = temp_dir("tamper-out");
        run_publish(&PublishOptions {
            project: Some(root),
            output: Some(out.clone()),
            key: None,
        })
        .unwrap();
        let pkg_bytes = std::fs::read(out.join("demo-0.1.0.aopkg")).unwrap();
        // 手工重打：src/main.as 内容与 FILES.json 声明不符（确定性篡改）。
        let original =
            crate::archive::read_zip_entry(&pkg_bytes, "demo-0.1.0/src/main.as").unwrap();
        let files_json =
            crate::archive::read_zip_entry(&pkg_bytes, "demo-0.1.0/FILES.json").unwrap();
        let tampered_files = {
            let mut m: FilesManifest = serde_json::from_slice(&files_json).unwrap();
            for f in &mut m.files {
                if f.path == "src/main.as" {
                    f.sha256 = "00".repeat(32);
                }
            }
            serde_json::to_vec_pretty(&m).unwrap()
        };
        let rebuilt = crate::archive::create_zip(&[
            ("demo-0.1.0/src/main.as".to_string(), original),
            ("demo-0.1.0/FILES.json".to_string(), tampered_files),
        ])
        .unwrap();
        let tampered_pkg = out.join("tampered.aopkg");
        std::fs::write(&tampered_pkg, &rebuilt).unwrap();
        let err = run_verify(&VerifyOptions {
            package: tampered_pkg,
            sig: None,
        })
        .unwrap_err();
        assert!(err.contains("sha256 mismatch for `src/main.as`"), "{err}");
    }

    #[test]
    fn verify_rejects_unlisted_entries() {
        let out = temp_dir("unlisted");
        // 清单只声明 main.as，但包内夹带 smuggled.txt → 封闭性校验拒绝。
        let files_json = serde_json::to_vec_pretty(&FilesManifest {
            schema_version: FILES_SCHEMA_VERSION,
            package: "demo".into(),
            version: "0.1.0".into(),
            files: vec![FilesEntry {
                path: "src/main.as".into(),
                sha256: crate::hash::content_sha256(b"void Main() {}"),
                size: b"void Main() {}".len() as u64,
            }],
        })
        .unwrap();
        let pkg = crate::archive::create_zip(&[
            (
                "demo-0.1.0/src/main.as".to_string(),
                b"void Main() {}".to_vec(),
            ),
            ("demo-0.1.0/smuggled.txt".to_string(), b"evil".to_vec()),
            ("demo-0.1.0/FILES.json".to_string(), files_json),
        ])
        .unwrap();
        let pkg_path = out.join("demo-0.1.0.aopkg");
        std::fs::write(&pkg_path, &pkg).unwrap();
        let err = run_verify(&VerifyOptions {
            package: pkg_path,
            sig: None,
        })
        .unwrap_err();
        assert!(err.contains("not listed in FILES.json"), "{err}");
    }

    #[test]
    fn verify_rejects_tampered_signature() {
        let _env_guard = env_lock();
        let (seed, pk) = {
            let s = [0x7bu8; ED25519_SEED_LEN];
            let (pk, _) = crate::release_sign::sign_message(&s, b"probe").unwrap();
            (s, pk)
        };
        let root = fixture_project("badsig");
        let out = temp_dir("badsig-out");
        std::env::set_var(
            crate::release::RELEASE_PUBKEY_ENV,
            crate::release_sign::hex_encode(&pk),
        );
        run_publish(&PublishOptions {
            project: Some(root),
            output: Some(out.clone()),
            key: Some(crate::release_sign::hex_encode(&seed)),
        })
        .unwrap();
        std::env::remove_var(crate::release::RELEASE_PUBKEY_ENV);
        // 篡改签名行 → 信任锚验签拒绝。
        let sig_path = out.join("demo-0.1.0.aopkg.sig");
        let sig_text = std::fs::read_to_string(&sig_path).unwrap();
        let mut chars: Vec<char> = sig_text.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == 'a' { 'b' } else { 'a' };
        std::fs::write(&sig_path, chars.into_iter().collect::<String>()).unwrap();
        let err = run_verify(&VerifyOptions {
            package: out.join("demo-0.1.0.aopkg"),
            sig: None,
        })
        .unwrap_err();
        assert!(err.contains("signature verification failed"), "{err}");
    }

    #[test]
    fn publish_rejects_empty_version_and_solution() {
        // [package].version 缺省 "0.1.0"（manifest.rs）；显式空串触发身份守卫。
        let root = temp_dir("noversion");
        std::fs::write(
            root.join("arc.toml"),
            "[package]\nname = \"x\"\nversion = \"\"\nedition = \"1\"\n",
        )
        .unwrap();
        std::fs::write(root.join("main.as"), "void Main() {}\n").unwrap();
        let err = run_publish(&PublishOptions {
            project: Some(root.clone()),
            output: None,
            key: None,
        })
        .unwrap_err();
        assert!(err.contains("`[package].version`"), "{err}");

        let ws = temp_dir("solution");
        std::fs::write(ws.join("arc.toml"), "[workspace]\nmembers = [\"a\"]\n").unwrap();
        let err = run_publish(&PublishOptions {
            project: Some(ws),
            output: None,
            key: None,
        })
        .unwrap_err();
        assert!(err.contains("workspace solution"), "{err}");
    }

    #[test]
    fn publish_requires_arc_toml_in_exact_dir() {
        let outer = temp_dir("exact");
        let inner = outer.join("nested");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(
            outer.join("arc.toml"),
            "[package]\nname = \"outer\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        // 嵌套目录自身无 arc.toml → 不向上行走，直接报错。
        let err = run_publish(&PublishOptions {
            project: Some(inner),
            output: None,
            key: None,
        })
        .unwrap_err();
        assert!(err.contains("no `arc.toml`"), "{err}");
    }
}
