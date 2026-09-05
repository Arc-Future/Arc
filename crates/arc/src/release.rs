//! 发布基础设施：签名发布 manifest 协议 + Ed25519 验签（RFC 031 §13）。
//!
//! ## 协议（manifest.json + 分离签名）
//!
//! 发布根（`ARC_RELEASE_BASE` / `--source`）下固定两个文件：
//!
//! ```text
//! <base>/manifest.json         版本清单 + 各平台/目标 SHA256（UTF-8，无 BOM）
//! <base>/manifest.json.sig     `<64-hex 公钥> <64-hex 签名>` 单行分离签名
//! ```
//!
//! 签名覆盖 **manifest.json 原始字节**（分离签名，无 JSON 规范化问题）。
//! 信任锚 = 编译期内置 `RELEASE_PUBLIC_KEY_HEX`；发布/测试可用
//! `ARC_RELEASE_PUBKEY` 显式覆盖。`arc self-update`、`arc release verify`
//! 与安装脚本消费同一协议。
//!
//! ## 明确不在本切片
//!
//! - PKI / 证书链 / 密钥轮换（密钥离线托管，见 `ARC_RELEASE_SIGNING_KEY`）
//! - channel 切换的客户端语义（manifest 记录 `channel`，客户端恒取 `stable`）
//! - Authenticode / 签名安装器（外部依赖，见 RFC 031 §12）

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::release_sign::{
    hex_encode, parse_hex, sign_message, verify_message, ED25519_PUBLIC_KEY_LEN, ED25519_SEED_LEN,
    ED25519_SIGNATURE_LEN,
};
use crate::version::Version;

/// manifest 文件名（发布根下）。
pub const MANIFEST_FILE_NAME: &str = "manifest.json";
/// 分离签名文件名（发布根下；单行 `<pubkey> <signature>`）。
pub const MANIFEST_SIG_FILE_NAME: &str = "manifest.json.sig";
/// manifest schema 版本（破坏性变更时 +1，客户端拒绝未知 schema）。
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
/// 默认稳定 channel。
pub const DEFAULT_CHANNEL: &str = "stable";

/// 发布根基址环境变量（`--source` 覆盖）。
pub const RELEASE_BASE_ENV: &str = "ARC_RELEASE_BASE";
/// 验签 pin 公钥（64 hex；覆盖编译期内置信任锚）。
pub const RELEASE_PUBKEY_ENV: &str = "ARC_RELEASE_PUBKEY";
/// 发布侧签名种子（64 hex → 32 字节 Ed25519 seed；仅签名命令需要）。
pub const RELEASE_SIGNING_KEY_ENV: &str = "ARC_RELEASE_SIGNING_KEY";
/// 默认发布根（占位——真实发布 URL 托管为外部依赖）。
pub const DEFAULT_RELEASE_BASE: &str = "https://static.arc.dev/dist";

/// 编译期内置发布公钥（信任锚；`arc release keygen` 生成）。
///
/// 1.0 正式发布密钥：seed 由发布者离线托管（`~/.arc/keys/`，不入库不外发）；
/// 泄露即重新 `arc release keygen` 轮换并同步替换本常量。`$ARC_RELEASE_PUBKEY`
/// 可显式覆盖信任锚（测试 / 轮换迁移期）。
pub const RELEASE_PUBLIC_KEY_HEX: &str =
    "0b2bd06a9a75dad24d809eb574ee23d23fb71a8477a44fc71d16ea531628db25";

/// 发布源：HTTP(S) 基址或本地目录（`file://` / 裸路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseSource {
    /// HTTP(S) 基址（已去尾斜杠）。
    Http(String),
    /// 本地目录。
    File(PathBuf),
}

/// 解析发布源字符串：`http(s)://` → HTTP；`file://` / 裸路径 → 本地目录。
pub fn parse_source(raw: &str) -> Result<ReleaseSource, ReleaseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ReleaseError::Message(
            "release source must not be empty".into(),
        ));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let base = trimmed.trim_end_matches('/').to_string();
        if base.ends_with("://") {
            return Err(ReleaseError::Message(format!(
                "release source `{raw}` is incomplete (need host)"
            )));
        }
        return Ok(ReleaseSource::Http(base));
    }
    Ok(ReleaseSource::File(file_uri_to_path(trimmed)))
}

/// 剥离 `file:` URI 前缀为本地路径（支持 `file:///C:/x`、`file:/x`）。
fn file_uri_to_path(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("file:") {
        let rest = rest.trim_start_matches('/');
        if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
            return PathBuf::from(rest);
        }
        if cfg!(windows) {
            return PathBuf::from(rest);
        }
        return PathBuf::from(format!("/{rest}"));
    }
    PathBuf::from(raw)
}

/// 版本清单条目（manifest `versions` 的值）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionEntry {
    /// 发布日期（ISO 8601，仅展示）。
    pub date: String,
    /// target triple → 分发包。
    pub artifacts: BTreeMap<String, ArtifactEntry>,
}

/// 分发包条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactEntry {
    /// 绝对 URL、`file://`、或相对发布根的相对路径。
    pub url: String,
    /// 分发包 SHA-256（64 hex 小写）。
    pub sha256: String,
    /// 分发包字节数。
    pub size: u64,
}

/// 签名 manifest 的解析结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub channel: String,
    /// 创建时间（ISO 8601，仅展示）。
    pub created: String,
    /// clang/LLVM 支持基线（如 `22.0.0`；与 doctor / toolchain 同源）。
    pub clang_min_version: String,
    pub versions: BTreeMap<String, VersionEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    #[error("{0}")]
    Message(String),
    #[error("manifest schema v{schema} is not supported (expected v{expected})")]
    UnsupportedSchema { schema: u32, expected: u32 },
    #[error("no artifact for host triple `{triple}` in version `{version}`")]
    NoArtifactForTriple { version: String, triple: String },
    #[error("requested version `{version}` not found in manifest")]
    VersionNotFound { version: String },
    #[error("invalid version `{0}` in manifest")]
    InvalidVersion(String),
    #[error("manifest signature verification failed: {0}")]
    BadSignature(String),
    #[error("SHA256 mismatch for `{name}`: expected {expected}, got {actual}")]
    Sha256Mismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("size mismatch for `{name}`: expected {expected}, got {actual}")]
    SizeMismatch {
        name: String,
        expected: u64,
        actual: u64,
    },
    #[error("fetch `{url}` failed: {detail}")]
    Fetch { url: String, detail: String },
    #[error("io `{path}`: {detail}")]
    Io { path: String, detail: String },
}

impl ReleaseError {
    fn io(path: &Path, e: &std::io::Error) -> Self {
        ReleaseError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for ReleaseError {
    fn from(e: serde_json::Error) -> Self {
        ReleaseError::Message(format!("manifest.json is not valid JSON: {e}"))
    }
}

impl From<crate::download::DownloadError> for ReleaseError {
    fn from(e: crate::download::DownloadError) -> Self {
        match e {
            crate::download::DownloadError::Fetch { url, detail } => {
                ReleaseError::Fetch { url, detail }
            }
        }
    }
}

/// 发布侧：对 manifest 原始字节签 Ed25519，返回 `.sig` 文件内容
/// `<pubkey-hex> <signature-hex>`（单行）。
pub fn sign_manifest_bytes(
    manifest_bytes: &[u8],
    seed: &[u8; ED25519_SEED_LEN],
) -> Result<String, ReleaseError> {
    let (pubkey, signature) = sign_message(seed, manifest_bytes)
        .map_err(|e| ReleaseError::BadSignature(format!("Ed25519 sign failed: {e}")))?;
    Ok(format!(
        "{} {}",
        hex_encode(&pubkey),
        hex_encode(&signature)
    ))
}

/// 消费侧：解析 `.sig` 内容并验签 manifest 原始字节。
///
/// 信任锚：`ARC_RELEASE_PUBKEY`（显式覆盖）> 编译期内置 `RELEASE_PUBLIC_KEY_HEX`。
/// 公钥不匹配或签名无效 → 硬错误（禁降级跳过）。
pub fn verify_manifest_bytes(manifest_bytes: &[u8], sig_text: &str) -> Result<(), ReleaseError> {
    let (pubkey_hex, sig_hex) = parse_sig_text(sig_text)?;
    let trusted = resolve_trusted_pubkey()?;
    if pubkey_hex != trusted {
        return Err(ReleaseError::BadSignature(format!(
            "manifest signed by an untrusted key (got pubkey {pubkey_hex}…; trusted \
             starts {trusted}…)"
        )));
    }
    let pubkey = parse_hex::<{ ED25519_PUBLIC_KEY_LEN }>(&pubkey_hex)
        .map_err(|e| ReleaseError::BadSignature(format!("bad pubkey hex: {e}")))?;
    let signature = parse_hex::<{ ED25519_SIGNATURE_LEN }>(&sig_hex)
        .map_err(|e| ReleaseError::BadSignature(format!("bad signature hex: {e}")))?;
    verify_message(&pubkey, manifest_bytes, &signature)
        .map_err(|e| ReleaseError::BadSignature(e.to_string()))
}

/// 解析 `.sig` 单行内容 `<pubkey-hex> <signature-hex>`。
fn parse_sig_text(sig_text: &str) -> Result<(String, String), ReleaseError> {
    let trimmed = sig_text.trim();
    let mut parts = trimmed.split_whitespace();
    let (Some(pubkey), Some(signature), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ReleaseError::BadSignature(format!(
            "signature file must contain `<64-hex-pubkey> <64-hex-signature>`, got \
             {trimmed:?}"
        )));
    };
    if pubkey.len() != ED25519_PUBLIC_KEY_LEN * 2 {
        return Err(ReleaseError::BadSignature(format!(
            "pubkey in .sig must be {} hex chars, got {}",
            ED25519_PUBLIC_KEY_LEN * 2,
            pubkey.len()
        )));
    }
    if signature.len() != ED25519_SIGNATURE_LEN * 2 {
        return Err(ReleaseError::BadSignature(format!(
            "signature in .sig must be {} hex chars, got {}",
            ED25519_SIGNATURE_LEN * 2,
            signature.len()
        )));
    }
    Ok((pubkey.to_string(), signature.to_string()))
}

/// 解析信任公钥：`ARC_RELEASE_PUBKEY`（覆盖）> 编译期内置。
fn resolve_trusted_pubkey() -> Result<String, ReleaseError> {
    if let Some(v) = crate::env::env_var(RELEASE_PUBKEY_ENV) {
        return Ok(v.trim().to_string());
    }
    Ok(RELEASE_PUBLIC_KEY_HEX.to_string())
}

/// 解析发布源（CLI 优先，其次 `$ARC_RELEASE_BASE`，缺省占位根）。
pub fn resolve_source(cli: Option<&str>) -> Result<ReleaseSource, ReleaseError> {
    match cli {
        Some(s) if !s.trim().is_empty() => parse_source(s),
        _ => match crate::env::env_var(RELEASE_BASE_ENV) {
            Some(base) => parse_source(&base),
            None => parse_source(DEFAULT_RELEASE_BASE),
        },
    }
}

/// 取发布根下相对文件的字节（HTTP(S) 或本地目录）。
pub fn fetch_bytes(source: &ReleaseSource, rel: &str) -> Result<Vec<u8>, ReleaseError> {
    match source {
        ReleaseSource::Http(base) => {
            let url = format!("{base}/{rel}");
            Ok(crate::download::http_get_bytes(&url)?)
        }
        ReleaseSource::File(dir) => {
            let path = dir.join(rel);
            std::fs::read(&path).map_err(|e| ReleaseError::io(&path, &e))
        }
    }
}

/// 下载并验签 manifest。
pub fn fetch_and_verify_manifest(source: &ReleaseSource) -> Result<ReleaseManifest, ReleaseError> {
    let bytes = fetch_bytes(source, MANIFEST_FILE_NAME)?;
    let sig = fetch_bytes(source, MANIFEST_SIG_FILE_NAME)?;
    let sig_text = String::from_utf8(sig)
        .map_err(|_| ReleaseError::BadSignature("manifest.json.sig is not valid UTF-8".into()))?;
    verify_manifest_bytes(&bytes, &sig_text)?;
    let manifest: ReleaseManifest = serde_json::from_slice(&bytes)?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(ReleaseError::UnsupportedSchema {
            schema: manifest.schema_version,
            expected: MANIFEST_SCHEMA_VERSION,
        });
    }
    Ok(manifest)
}

/// 解析 artifact URL：绝对 `http(s)://` / `file://` 原样使用；相对路径按发布根拼接。
pub fn resolve_artifact_url(source: &ReleaseSource, url: &str) -> Result<String, ReleaseError> {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(url.to_string());
    }
    if lower.starts_with("file://") {
        return Ok(url.to_string());
    }
    match source {
        ReleaseSource::Http(base) => Ok(format!("{base}/{url}")),
        ReleaseSource::File(_) => Ok(url.to_string()),
    }
}

/// 取分发包字节：HTTP(S) 绝对 URL、`file://`、或相对发布根的路径。
pub fn fetch_artifact(source: &ReleaseSource, url: &str) -> Result<Vec<u8>, ReleaseError> {
    let resolved = resolve_artifact_url(source, url)?;
    let lower = resolved.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(crate::download::http_get_bytes(&resolved)?);
    }
    let path = if lower.starts_with("file://") {
        file_uri_to_path(&resolved)
    } else {
        match source {
            // 相对 artifact URL 以发布根（本地目录）为基准解析。
            ReleaseSource::File(dir) => {
                let p = Path::new(&resolved);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    dir.join(p)
                }
            }
            ReleaseSource::Http(_) => PathBuf::from(&resolved),
        }
    };
    std::fs::read(&path).map_err(|e| ReleaseError::io(&path, &e))
}

/// 校验分发包字节与 manifest 声明一致（SHA256 全量 + 尺寸）。
pub fn verify_artifact(
    name: &str,
    entry: &ArtifactEntry,
    bytes: &[u8],
) -> Result<(), ReleaseError> {
    if bytes.len() as u64 != entry.size {
        return Err(ReleaseError::SizeMismatch {
            name: name.to_string(),
            expected: entry.size,
            actual: bytes.len() as u64,
        });
    }
    let actual = crate::hash::content_sha256(bytes);
    if actual != entry.sha256.to_ascii_lowercase() {
        return Err(ReleaseError::Sha256Mismatch {
            name: name.to_string(),
            expected: entry.sha256.to_ascii_lowercase(),
            actual,
        });
    }
    Ok(())
}

/// 选择目标版本。
///
/// - `requested`：精确版本（找不到 → 错误）。
/// - 缺省：manifest 中**高于 `current` 的最高版本**（需有 host triple artifact）。
pub fn select_target(
    manifest: &ReleaseManifest,
    triple: &str,
    current: &Version,
    requested: Option<&str>,
) -> Result<Option<(String, VersionEntry)>, ReleaseError> {
    if let Some(req) = requested {
        let entry =
            manifest
                .versions
                .get(req)
                .cloned()
                .ok_or_else(|| ReleaseError::VersionNotFound {
                    version: req.to_string(),
                })?;
        return Ok(Some((req.to_string(), entry)));
    }
    let mut best: Option<(String, Version)> = None;
    for (ver, entry) in &manifest.versions {
        if !entry.artifacts.contains_key(triple) {
            continue;
        }
        let parsed = ver
            .parse::<Version>()
            .map_err(|e| ReleaseError::InvalidVersion(format!("{ver}: {e}")))?;
        if parsed <= *current {
            continue;
        }
        if best.as_ref().is_none_or(|(_, b)| parsed > *b) {
            best = Some((ver.clone(), parsed));
        }
    }
    Ok(best.map(|(ver, _)| {
        let entry = manifest.versions[&ver].clone();
        (ver, entry)
    }))
}

/// 构建发布清单（生成侧）：单版本 + 多平台 artifact。
pub fn build_manifest(
    version: &str,
    clang_min_version: &str,
    artifacts: BTreeMap<String, ArtifactEntry>,
    created: String,
    date: String,
) -> ReleaseManifest {
    let mut versions = BTreeMap::new();
    versions.insert(version.to_string(), VersionEntry { date, artifacts });
    ReleaseManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        channel: DEFAULT_CHANNEL.into(),
        created,
        clang_min_version: clang_min_version.to_string(),
        versions,
    }
}

/// 生成发布签名密钥对；`seed` 缺省随机生成。
///
/// 返回 `(seed_hex, pubkey_hex)`——seed 即 Ed25519 私钥（离线托管），
/// pubkey 为编译期内置信任锚（`RELEASE_PUBLIC_KEY_HEX`）。
pub fn generate_keypair(
    seed: Option<[u8; ED25519_SEED_LEN]>,
) -> Result<(String, String), ReleaseError> {
    let seed = match seed {
        Some(s) => s,
        None => {
            let mut s = [0u8; ED25519_SEED_LEN];
            getrandom::fill(&mut s)
                .map_err(|e| ReleaseError::Message(format!("failed to generate seed: {e}")))?;
            s
        }
    };
    let (pk, _) = sign_message(&seed, b"arc-release-root-of-trust")
        .map_err(|e| ReleaseError::BadSignature(format!("Ed25519 sign failed: {e}")))?;
    Ok((hex_encode(&seed), hex_encode(&pk)))
}

/// 将 manifest 原始字节与签名写为发布根下的 `manifest.json` 与 `manifest.json.sig`。
pub fn write_manifest_files(
    dir: &Path,
    manifest_bytes: &[u8],
    sig_text: &str,
) -> Result<(), ReleaseError> {
    std::fs::create_dir_all(dir).map_err(|e| ReleaseError::io(dir, &e))?;
    let manifest_path = dir.join(MANIFEST_FILE_NAME);
    let sig_path = dir.join(MANIFEST_SIG_FILE_NAME);
    std::fs::write(&manifest_path, manifest_bytes)
        .map_err(|e| ReleaseError::io(&manifest_path, &e))?;
    std::fs::write(&sig_path, sig_text.as_bytes()).map_err(|e| ReleaseError::io(&sig_path, &e))?;
    Ok(())
}

/// 当前 UTC 时间 ISO-8601（`2026-08-15T08:00:00Z`；无外部 chrono 依赖）。
pub fn iso8601_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant 民用历法算法：epoch 天数 → (年, 月, 日)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// --- CLI 执行入口（参数解析在 main.rs，逻辑在此进程内可测）---

/// `arc release manifest` 参数束。
#[derive(Debug, Clone, Default)]
pub struct ManifestArgs {
    /// 版本号（缺省 `CARGO_PKG_VERSION`）。
    pub version: Option<String>,
    /// target triple（与 archives 配对；缺省逐条取宿主 triple）。
    pub triples: Vec<String>,
    /// 分发包本地路径（计算 sha256/size）。
    pub archives: Vec<PathBuf>,
    /// 分发包 URL（缺省按 `--url-prefix` + 包文件名派生）。
    pub urls: Vec<String>,
    /// 发布根 URL 前缀（派生 artifact URL）。
    pub url_prefix: Option<String>,
    /// 输出目录（写 `manifest.json` 与 `manifest.json.sig`；缺省当前目录）。
    pub output: Option<PathBuf>,
    /// 签名 seed（64 hex；缺省 `$ARC_RELEASE_SIGNING_KEY`）。
    pub key: Option<String>,
}

/// `arc release manifest`：从本地分发包构建签名版本清单。
pub fn run_manifest(args: &ManifestArgs) -> Result<(), String> {
    if args.archives.is_empty() {
        return Err(
            "--archive <archive> is required (compute sha256/size from the packaged artifact)"
                .into(),
        );
    }
    let n = args.archives.len();
    let triples: Vec<String> = if args.triples.is_empty() {
        vec![crate::target::TargetTriple::host().as_str().to_string(); n]
    } else if args.triples.len() == n {
        args.triples.clone()
    } else {
        return Err(format!(
            "--triple count ({}) must match --archive count ({n})",
            args.triples.len()
        ));
    };
    let urls: Vec<String> = if args.urls.is_empty() {
        vec![String::new(); n]
    } else if args.urls.len() == n {
        args.urls.clone()
    } else {
        return Err(format!(
            "--url count ({}) must match --archive count ({n})",
            args.urls.len()
        ));
    };
    let version = args
        .version
        .clone()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let mut artifacts = BTreeMap::new();
    for i in 0..n {
        let bytes = std::fs::read(&args.archives[i])
            .map_err(|e| format!("read {}: {e}", args.archives[i].display()))?;
        // URL 缺省 = 前缀 + 包文件名（zip/tar.xz 均按实际文件名派生）。
        let file_name = args.archives[i]
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("--archive {} has no file name", args.archives[i].display()))?;
        let u = if !urls[i].is_empty() {
            urls[i].clone()
        } else if let Some(prefix) = &args.url_prefix {
            format!("{}/{}", prefix.trim_end_matches('/'), file_name)
        } else {
            file_name
        };
        artifacts.insert(
            triples[i].clone(),
            ArtifactEntry {
                url: u,
                sha256: crate::hash::content_sha256(&bytes),
                size: bytes.len() as u64,
            },
        );
    }
    let now = iso8601_now();
    let date = now.split('T').next().unwrap_or(&now).to_string();
    let manifest = build_manifest(
        &version,
        crate::clang_version::LLVM_MIN_VERSION,
        artifacts,
        now,
        date,
    );
    let bytes = serde_json::to_vec(&manifest).map_err(|e| format!("serialize manifest: {e}"))?;
    let seed_hex = match &args.key {
        Some(k) => k.clone(),
        None => crate::env::env_var(RELEASE_SIGNING_KEY_ENV).ok_or_else(|| {
            format!(
                "signing key required: `--key <seed>` or ${RELEASE_SIGNING_KEY_ENV} \
                 (64 hex; generate via `arc release keygen`)"
            )
        })?,
    };
    let seed: [u8; ED25519_SEED_LEN] =
        crate::release_sign::parse_hex32(&seed_hex).map_err(|e| format!("invalid seed: {e}"))?;
    let sig_text = sign_manifest_bytes(&bytes, &seed).map_err(|e| e.to_string())?;
    let out_dir = args.output.clone().unwrap_or_else(|| PathBuf::from("."));
    write_manifest_files(&out_dir, &bytes, &sig_text).map_err(|e| e.to_string())?;
    println!(
        "wrote {} and {}",
        out_dir.join(MANIFEST_FILE_NAME).display(),
        out_dir.join(MANIFEST_SIG_FILE_NAME).display()
    );
    Ok(())
}

/// `arc release verify` 参数束。
#[derive(Debug, Clone, Default)]
pub struct VerifyArgs {
    /// 发布源：`https://…`、`file:///…` 或本地目录。
    pub source: String,
    /// 校验指定版本的分发包（缺省仅验证 manifest 本体）。
    pub version: Option<String>,
    /// 分发包 target triple（缺省宿主 triple）。
    pub triple: Option<String>,
    /// 本地分发包路径（比对 sha256/size；缺省按 manifest 从发布源下载）。
    pub archive: Option<PathBuf>,
}

/// `arc release verify`：解析 → 验签 → 可选比对分发包 SHA256。
pub fn run_verify(args: &VerifyArgs) -> Result<(), String> {
    let source = parse_source(&args.source).map_err(|e| e.to_string())?;
    let manifest = fetch_and_verify_manifest(&source).map_err(|e| e.to_string())?;
    println!(
        "manifest ok: schema v{}, channel {}, clang_min_version {}, {} version(s)",
        manifest.schema_version,
        manifest.channel,
        manifest.clang_min_version,
        manifest.versions.len()
    );
    if let Some(ver) = &args.version {
        let triple = args
            .triple
            .clone()
            .unwrap_or_else(|| crate::target::TargetTriple::host().as_str().to_string());
        let entry = manifest
            .versions
            .get(ver)
            .ok_or_else(|| format!("version `{ver}` not found in manifest"))?
            .artifacts
            .get(&triple)
            .ok_or_else(|| format!("no artifact for `{triple}` in version `{ver}`"))?;
        let name = entry
            .url
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&entry.url)
            .to_string();
        let bytes = match &args.archive {
            Some(p) => std::fs::read(p).map_err(|e| format!("read {}: {e}", p.display()))?,
            None => fetch_artifact(&source, &entry.url).map_err(|e| e.to_string())?,
        };
        verify_artifact(&name, entry, &bytes).map_err(|e| e.to_string())?;
        println!("artifact verified: {name} (sha256 {}…)", &entry.sha256[..8]);
    }
    Ok(())
}

/// `arc release keygen`：生成（或从给定 seed 派生）发布签名密钥对。
pub fn run_keygen(seed: Option<&str>) -> Result<(), String> {
    let seed_bytes = match seed {
        Some(s) => {
            Some(crate::release_sign::parse_hex32(s).map_err(|e| format!("invalid --seed: {e}"))?)
        }
        None => None,
    };
    let (seed_hex, pubkey_hex) = generate_keypair(seed_bytes).map_err(|e| e.to_string())?;
    println!("ARC_RELEASE_SIGNING_KEY (保管离线处) = {seed_hex}");
    println!("ARC_RELEASE_PUBKEY (内嵌信任锚) = {pubkey_hex}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release_sign::{sign_message, ED25519_SEED_LEN};

    fn test_keypair() -> ([u8; ED25519_SEED_LEN], [u8; ED25519_PUBLIC_KEY_LEN]) {
        let seed = [0x5au8; ED25519_SEED_LEN];
        let (pk, _sig) = sign_message(&seed, b"probe").unwrap();
        (seed, pk)
    }

    /// `$ARC_RELEASE_PUBKEY` 等进程级环境变量在并行测试下互斥。
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn temp_dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arc-release-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_manifest() -> ReleaseManifest {
        ReleaseManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            channel: DEFAULT_CHANNEL.into(),
            created: "2026-08-15T00:00:00Z".into(),
            clang_min_version: "22.0.0".into(),
            versions: BTreeMap::from([(
                "0.1.0".into(),
                VersionEntry {
                    date: "2026-08-15".into(),
                    artifacts: BTreeMap::from([(
                        "x86_64-pc-windows-msvc".into(),
                        ArtifactEntry {
                            url: "arc-0.1.0-x86_64-pc-windows-msvc.zip".into(),
                            sha256: "ab".repeat(32),
                            size: 1024,
                        },
                    )]),
                },
            )]),
        }
    }

    #[test]
    fn sign_verify_roundtrip() {
        let _env_guard = env_lock();
        let (seed, pk) = test_keypair();
        let manifest = make_manifest();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let sig = sign_manifest_bytes(&bytes, &seed).unwrap();
        std::env::set_var(RELEASE_PUBKEY_ENV, hex_encode(&pk));
        verify_manifest_bytes(&bytes, &sig).unwrap();
        std::env::remove_var(RELEASE_PUBKEY_ENV);
    }

    #[test]
    fn tampered_manifest_fails_verify() {
        let _env_guard = env_lock();
        let (seed, pk) = test_keypair();
        let manifest = make_manifest();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let sig = sign_manifest_bytes(&bytes, &seed).unwrap();
        let mut tampered = bytes.clone();
        let last = tampered.last_mut().unwrap();
        *last = last.wrapping_add(1);
        std::env::set_var(RELEASE_PUBKEY_ENV, hex_encode(&pk));
        let err = verify_manifest_bytes(&tampered, &sig).unwrap_err();
        assert!(matches!(err, ReleaseError::BadSignature(_)), "{err}");
        std::env::remove_var(RELEASE_PUBKEY_ENV);
    }

    #[test]
    fn untrusted_key_rejected() {
        let _env_guard = env_lock();
        let (seed, _) = test_keypair();
        let manifest = make_manifest();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let sig = sign_manifest_bytes(&bytes, &seed).unwrap();
        // 信任锚 = 内置 pubkey（测试环境未覆盖）；生成的公钥与其不同 → 拒绝。
        let err = verify_manifest_bytes(&bytes, &sig).unwrap_err();
        assert!(matches!(err, ReleaseError::BadSignature(_)), "{err}");
    }

    #[test]
    fn builtin_pubkey_is_valid_hex() {
        assert_eq!(RELEASE_PUBLIC_KEY_HEX.len(), ED25519_PUBLIC_KEY_LEN * 2);
        assert!(parse_hex::<{ ED25519_PUBLIC_KEY_LEN }>(RELEASE_PUBLIC_KEY_HEX).is_ok());
    }

    #[test]
    fn malformed_sig_rejected() {
        let _env_guard = env_lock();
        let (seed, pk) = test_keypair();
        let bytes = serde_json::to_vec(&make_manifest()).unwrap();
        let sig = sign_manifest_bytes(&bytes, &seed).unwrap();
        std::env::set_var(RELEASE_PUBKEY_ENV, hex_encode(&pk));
        // 合法签名通过（对照基线）。
        verify_manifest_bytes(&bytes, &sig).unwrap();
        assert!(matches!(
            verify_manifest_bytes(&bytes, "only-one-field"),
            Err(ReleaseError::BadSignature(_))
        ));
        assert!(matches!(
            verify_manifest_bytes(&bytes, "abc def"),
            Err(ReleaseError::BadSignature(_))
        ));
        std::env::remove_var(RELEASE_PUBKEY_ENV);
    }

    #[test]
    fn parse_source_variants() {
        assert!(matches!(
            parse_source("https://static.arc.dev/dist/").unwrap(),
            ReleaseSource::Http(base) if base == "https://static.arc.dev/dist"
        ));
        assert!(matches!(
            parse_source("file:///C:/release").unwrap(),
            ReleaseSource::File(p) if p.to_string_lossy() == "C:/release" || p.to_string_lossy() == "/C:/release"
        ));
        assert!(matches!(
            parse_source(r"D:\release").unwrap(),
            ReleaseSource::File(_)
        ));
        assert!(matches!(
            parse_source("").unwrap_err(),
            ReleaseError::Message(_)
        ));
    }

    #[test]
    fn fetch_and_verify_manifest_from_file_source() {
        let _env_guard = env_lock();
        let (seed, pk) = test_keypair();
        let dir = temp_dir("fetch");
        let manifest = make_manifest();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let sig = sign_manifest_bytes(&bytes, &seed).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE_NAME), &bytes).unwrap();
        std::fs::write(dir.join(MANIFEST_SIG_FILE_NAME), sig.as_bytes()).unwrap();
        std::env::set_var(RELEASE_PUBKEY_ENV, hex_encode(&pk));
        let fetched = fetch_and_verify_manifest(&ReleaseSource::File(dir.clone())).unwrap();
        assert_eq!(fetched.channel, "stable");
        std::env::remove_var(RELEASE_PUBKEY_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsupported_schema_rejected() {
        let _env_guard = env_lock();
        let (seed, pk) = test_keypair();
        let dir = temp_dir("schema");
        let mut manifest = make_manifest();
        manifest.schema_version = 99;
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let sig = sign_manifest_bytes(&bytes, &seed).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE_NAME), &bytes).unwrap();
        std::fs::write(dir.join(MANIFEST_SIG_FILE_NAME), sig.as_bytes()).unwrap();
        std::env::set_var(RELEASE_PUBKEY_ENV, hex_encode(&pk));
        let err = fetch_and_verify_manifest(&ReleaseSource::File(dir.clone())).unwrap_err();
        assert!(
            matches!(err, ReleaseError::UnsupportedSchema { .. }),
            "{err}"
        );
        std::env::remove_var(RELEASE_PUBKEY_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_artifact_sha256_and_size() {
        let dir = temp_dir("artifact");
        let bytes = b"arc-0.1.0-artifact-bytes".to_vec();
        let entry = ArtifactEntry {
            url: "arc.zip".into(),
            sha256: crate::hash::content_sha256(&bytes),
            size: bytes.len() as u64,
        };
        verify_artifact("arc.zip", &entry, &bytes).unwrap();
        let bad = ArtifactEntry {
            url: "arc.zip".into(),
            sha256: crate::hash::content_sha256(&bytes),
            size: bytes.len() as u64 + 1,
        };
        assert!(matches!(
            verify_artifact("arc.zip", &bad, &bytes),
            Err(ReleaseError::SizeMismatch { .. })
        ));
        let bad_hash = ArtifactEntry {
            url: "arc.zip".into(),
            sha256: "00".repeat(32),
            size: bytes.len() as u64,
        };
        assert!(matches!(
            verify_artifact("arc.zip", &bad_hash, &bytes),
            Err(ReleaseError::Sha256Mismatch { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn select_target_picks_highest_above_current() {
        let manifest = ReleaseManifest {
            schema_version: 1,
            channel: "stable".into(),
            created: "x".into(),
            clang_min_version: "22.0.0".into(),
            versions: BTreeMap::from([
                (
                    "0.1.0".into(),
                    VersionEntry {
                        date: "d".into(),
                        artifacts: BTreeMap::from([(
                            "x86_64-pc-windows-msvc".into(),
                            ArtifactEntry {
                                url: "a".into(),
                                sha256: "ab".repeat(32),
                                size: 1,
                            },
                        )]),
                    },
                ),
                (
                    "0.3.0".into(),
                    VersionEntry {
                        date: "d".into(),
                        artifacts: BTreeMap::from([(
                            "x86_64-pc-windows-msvc".into(),
                            ArtifactEntry {
                                url: "c".into(),
                                sha256: "ab".repeat(32),
                                size: 1,
                            },
                        )]),
                    },
                ),
                (
                    // 无宿主 artifact → 不可选
                    "9.9.9".into(),
                    VersionEntry {
                        date: "d".into(),
                        artifacts: BTreeMap::new(),
                    },
                ),
            ]),
        };
        let current = "0.1.0".parse::<Version>().unwrap();
        let (ver, _) = select_target(&manifest, "x86_64-pc-windows-msvc", &current, None)
            .unwrap()
            .unwrap();
        assert_eq!(ver, "0.3.0");
        // 已最新 → None
        let latest = "0.3.0".parse::<Version>().unwrap();
        assert!(
            select_target(&manifest, "x86_64-pc-windows-msvc", &latest, None)
                .unwrap()
                .is_none()
        );
        // 精确版本
        let (ver, _) = select_target(&manifest, "x86_64-pc-windows-msvc", &current, Some("0.1.0"))
            .unwrap()
            .unwrap();
        assert_eq!(ver, "0.1.0");
        // 缺失版本
        assert!(matches!(
            select_target(&manifest, "x86_64-pc-windows-msvc", &current, Some("0.2.0"))
                .unwrap_err(),
            ReleaseError::VersionNotFound { .. }
        ));
    }

    #[test]
    fn iso8601_now_formats_utc() {
        let s = iso8601_now();
        // `YYYY-MM-DDThh:mm:ssZ`
        assert_eq!(s.len(), 20, "{s}");
        assert!(s.ends_with('Z'), "{s}");
        assert_eq!(&s[4..5], "-", "{s}");
        assert_eq!(&s[7..8], "-", "{s}");
        assert_eq!(&s[10..11], "T", "{s}");
    }

    #[test]
    fn civil_from_days_known_dates() {
        // 1970-01-01
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-02-29（闰年）：1970→2000-01-01 = 10957 天，+31（Jan）+28 = 11016
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // 2026-08-15：1970→2026-01-01 = 20454 天，+226 = 20680
        assert_eq!(civil_from_days(20_680), (2026, 8, 15));
    }

    #[test]
    fn build_manifest_roundtrip() {
        let manifest = build_manifest(
            "0.2.0",
            "22.0.0",
            BTreeMap::from([(
                "x86_64-pc-windows-msvc".into(),
                ArtifactEntry {
                    url: "arc-0.2.0-x86_64-pc-windows-msvc.zip".into(),
                    sha256: "ab".repeat(32),
                    size: 123,
                },
            )]),
            "2026-08-15T00:00:00Z".into(),
            "2026-08-15".into(),
        );
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.channel, "stable");
        assert_eq!(manifest.versions["0.2.0"].artifacts.len(), 1);
    }

    #[test]
    fn generate_keypair_derives_matching_pubkey() {
        let (seed_hex, pubkey_hex) = generate_keypair(Some([0x42; 32])).unwrap();
        assert_eq!(seed_hex, "42".repeat(32));
        assert_eq!(seed_hex.len(), 64);
        assert_eq!(pubkey_hex.len(), 64);
    }

    #[test]
    fn fetch_bytes_http_roundtrip() {
        // 最小 HTTP 服务：单响应回环验证 Http 源取回路径（含 ?/From 转换链）。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            let body = b"manifest-bytes-over-http";
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n")
                .unwrap();
            stream
                .write_all(format!("Content-Length: {}\r\n", body.len()).as_bytes())
                .unwrap();
            stream.write_all(b"Connection: close\r\n\r\n").unwrap();
            stream.write_all(body).unwrap();
        });
        let bytes = fetch_bytes(
            &ReleaseSource::Http(format!("http://{addr}")),
            "manifest.json",
        )
        .unwrap();
        assert_eq!(bytes, b"manifest-bytes-over-http");
        server.join().unwrap();
    }

    #[test]
    fn run_manifest_writes_signed_files_and_verify_reads_them() {
        let _env_guard = env_lock();
        let (seed, pk) = test_keypair();
        let dir = temp_dir("cli");
        let archive = dir.join("arc-0.1.0-x86_64-pc-windows-msvc.zip");
        std::fs::write(&archive, b"archive-bytes").unwrap();
        let out = dir.join("out");
        std::env::set_var(RELEASE_PUBKEY_ENV, hex_encode(&pk));
        run_manifest(&ManifestArgs {
            version: Some("0.1.0".into()),
            triples: vec!["x86_64-pc-windows-msvc".into()],
            archives: vec![archive.clone()],
            urls: vec![],
            url_prefix: Some("https://static.arc.dev/dist".into()),
            output: Some(out.clone()),
            key: Some(hex_encode(&seed)),
        })
        .unwrap();
        // URL = 前缀 + 包文件名；sha256/size 与本地包一致。
        let text = std::fs::read_to_string(out.join(MANIFEST_FILE_NAME)).unwrap();
        assert!(
            text.contains("https://static.arc.dev/dist/arc-0.1.0-x86_64-pc-windows-msvc.zip"),
            "{text}"
        );
        assert!(
            text.contains(&crate::hash::content_sha256(b"archive-bytes")),
            "{text}"
        );
        // 发布根 = out 本地目录；verify 走完整验签链。
        run_verify(&VerifyArgs {
            source: out.to_string_lossy().into_owned(),
            version: Some("0.1.0".into()),
            triple: Some("x86_64-pc-windows-msvc".into()),
            archive: Some(archive),
        })
        .unwrap();
        std::env::remove_var(RELEASE_PUBKEY_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_manifest_requires_signing_key() {
        let _env_guard = env_lock();
        let dir = temp_dir("nokey");
        let archive = dir.join("a.zip");
        std::fs::write(&archive, b"x").unwrap();
        std::env::remove_var(RELEASE_SIGNING_KEY_ENV);
        let err = run_manifest(&ManifestArgs {
            version: Some("0.1.0".into()),
            triples: vec![],
            archives: vec![archive],
            urls: vec![],
            url_prefix: None,
            output: Some(dir.clone()),
            key: None,
        })
        .unwrap_err();
        assert!(err.contains("signing key required"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
