//! Runtime `.o` 缓存**内容寻址**层（RFC 017 M4-link 的缓存失效机制）。
//!
//! 历史教训（`base64_bytes` 链接失败事件）：旧实现按
//! `cached.mtime >= src.mtime` 判定缓存命中——mtime 不可靠（git 操作、
//! 时钟回拨、并发改写、touch），且**头文件与被 `#include` 的 `.c` 变化
//! 完全不感知**（判定只比 `.c` 源自身）→ 陈旧 `.o` 被复用 → 链接期
//! `undefined symbol` 或静默行为差异。
//!
//! 本层改为**内容寻址**：每个缓存 `.o` 旁落 `<name>.o.arc-fp` 指纹文件，
//! 命中判定 = 四者全匹配：
//!   1. 源文件内容 SHA-256（单源粒度：独立 TU 的 `.c` 变化只重编自身）；
//!   2. 全局依赖树内容 SHA-256（rt_base 全树头文件 + **非独立 TU** 的
//!      `.c`——如被 `rt_barcode.c` `#include` 的 `quirc.c`，任何头文件/被
//!      include 源变化触发全量重编）；
//!   3. 编译选项指纹（target/opt/debug/sanitize/恒定注入/`-I` 注入/
//!      clang 版本）；
//!   4. 产物字节数（损坏/半写自愈：size 不匹配即重编）。
//!
//! mtime 完全退出判定：内容未变（含 touch）必命中，任一输入变化必重编。
//! 写入沿用 temp + rename 原子替换（并发多进程安全）。

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// 指纹文件后缀（`rt_text.o` → `rt_text.o.arc-fp`）。
pub const FINGERPRINT_SUFFIX: &str = "arc-fp";
/// 指纹格式版本（格式演进时递增，旧格式一律视为 miss）。
const FINGERPRINT_VERSION: &str = "v1";

/// 摘要前缀十六进制（前 8 字节 → 16 hex）。全部指纹收尾的唯一出口：
/// 摘要即最终输出，不得把摘要字节再喂给任何哈希（double-hash 曾使
/// 流式与一次性路径对同一内容恒产出不同指纹）。
fn digest_prefix(digest: &[u8]) -> String {
    digest[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// 内容寻址哈希：SHA-256 截断为 16 hex（64 bit）。缓存寻址场景碰撞概率
/// 可忽略；全量 hex 无必要（诊断时 16 hex 亦足够区分）。
pub fn sha256_hex(data: &[u8]) -> String {
    digest_prefix(&Sha256::digest(data))
}

/// 文件内容 SHA-256（16 hex）；读失败返回 `None`（调用方按 miss 处理）。
pub fn file_sha256(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(digest_prefix(&hasher.finalize()))
}

/// 编译选项指纹（影响产物的全部选项的稳定串）。
///
/// **维护规则**：新增影响 runtime `.o` 产物的编译选项时，必须在此追加分量
/// （如新的 `-D`/`-I` 注入），否则选项变化不会使缓存失效。
/// clang 版本纳入指纹：clang 升级改变代码生成 → 旧缓存须失效。
pub fn flags_hash(
    target: Option<&str>,
    release: bool,
    debug_info: bool,
    sanitize_suffix: &str,
    extra_parts: &[&str],
) -> String {
    let mut parts: Vec<&str> = vec![target.unwrap_or("default")];
    parts.push(if release { "release" } else { "debug" });
    parts.push(if debug_info { "g" } else { "nog" });
    parts.push(sanitize_suffix);
    parts.extend_from_slice(extra_parts);
    sha256_hex(parts.join("|").as_bytes())
}

/// clang 版本指纹（`clang --version` 首行）；获取失败用固定占位（该场景
/// 与同占位共享指纹——正常环境总能取到版本）。
pub fn clang_version_fingerprint(clang: &str) -> String {
    let out = std::process::Command::new(clang)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok());
    match out {
        Some(text) => sha256_hex(text.lines().next().unwrap_or("?").as_bytes()),
        None => "unknown-clang".to_string(),
    }
}

/// 单个缓存 `.o` 的期望指纹。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheFingerprint {
    pub src_hash: String,
    pub deps_hash: String,
    pub flags_hash: String,
    /// 产物字节数（快速失败路径；与 `obj_hash` 同录于指纹文件）。
    pub size: u64,
    /// 产物内容哈希（回填时计算；命中时对缓存 `.o` 重算对比——**损坏自愈**：
    /// 仅比对 size 无法发现同长度字节损坏（XOR/覆写），内容哈希才能保证
    /// 复用的 `.o` 与回填时逐字节一致）。
    pub obj_hash: String,
}

/// 指纹文件路径（`<cached>` → `<cached>.arc-fp`）。
pub fn meta_path(cached: &Path) -> PathBuf {
    let mut name = cached
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    name.push('.');
    name.push_str(FINGERPRINT_SUFFIX);
    cached.with_file_name(name)
}

/// 序列化指纹（每行一个字段，首行版本号）。
fn serialize(fp: &CacheFingerprint) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n",
        FINGERPRINT_VERSION, fp.src_hash, fp.deps_hash, fp.flags_hash, fp.size, fp.obj_hash
    )
}

/// 解析指纹文件；格式不符返回 `None`（视为 miss，触发重编）。
fn parse(text: &str) -> Option<CacheFingerprint> {
    let mut lines = text.lines();
    if lines.next()? != FINGERPRINT_VERSION {
        return None;
    }
    Some(CacheFingerprint {
        src_hash: lines.next()?.to_string(),
        deps_hash: lines.next()?.to_string(),
        flags_hash: lines.next()?.to_string(),
        size: lines.next()?.parse().ok()?,
        obj_hash: lines.next()?.to_string(),
    })
}

/// 原子写指纹文件（temp + rename；与 `.o` 回填同一模式）。
pub fn write_meta_atomic(cached: &Path, fp: &CacheFingerprint) {
    let meta = meta_path(cached);
    let tmp = meta.with_extension("tmp");
    if fs::write(&tmp, serialize(fp)).is_ok() {
        let _ = fs::rename(&tmp, &meta);
    }
}

/// 缓存命中判定：`.o` 存在 + 指纹文件存在、src/deps/flags 三哈希匹配，
/// 且**回填时记录的产物大小与内容哈希均与实际一致**（损坏/半写自愈——
/// size 快速失败，`obj_hash` 强校验：同长度字节损坏（XOR/覆写）也能发现）。
///
/// `want.src_hash/deps_hash/flags_hash` 参与判定；`want.size/obj_hash` 不参与
/// （命中侧以指纹文件记录值为准对比实际产物）。
pub fn cache_hit(cached: &Path, want: &CacheFingerprint) -> bool {
    let Ok(obj_size) = fs::metadata(cached).map(|m| m.len()) else {
        return false;
    };
    let Ok(meta_text) = fs::read_to_string(meta_path(cached)) else {
        return false;
    };
    let Some(got) = parse(&meta_text) else {
        return false;
    };
    if got.src_hash != want.src_hash
        || got.deps_hash != want.deps_hash
        || got.flags_hash != want.flags_hash
        || got.size != obj_size
    {
        return false;
    }
    // 强校验：缓存 `.o` 内容与回填时逐字节一致（同长度损坏检测）。
    file_sha256(cached).is_some_and(|h| h == got.obj_hash)
}

/// 计算全局依赖树哈希：`rt_base` 全树下全部头文件 + **非独立 TU** 的 `.c`
/// （独立 TU 由 `independent_sources` 给出，其内容以单源 `src_hash` 覆盖）。
///
/// 按相对路径排序后逐项合并 `(rel, file_hash)`——路径顺序稳定，内容变化
/// 必然改变结果；缺读文件按"跳过"处理（该文件不影响编译产物，保守安全
/// 方向是重编，故缺读应使指纹变化——这里按失败即返回 None 处理由调用方
/// 决定）。
pub fn compute_deps_hash(rt_base: &Path, independent_sources: &[PathBuf]) -> Option<String> {
    let independent: HashSet<PathBuf> = independent_sources
        .iter()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
        .collect();
    let mut entries: Vec<(String, String)> = Vec::new();

    fn walk(
        dir: &Path,
        independent: &HashSet<PathBuf>,
        rt_base: &Path,
        entries: &mut Vec<(String, String)>,
        failed: &mut bool,
    ) {
        let Ok(rd) = fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, independent, rt_base, entries, failed);
                continue;
            }
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
            let is_header = matches!(ext, "h" | "H" | "hpp" | "hh");
            let is_include_c = ext == "c"
                && !independent.contains(&p.canonicalize().unwrap_or_else(|_| p.clone()));
            if !is_header && !is_include_c {
                continue;
            }
            let Some(h) = file_sha256(&p) else {
                *failed = true;
                continue;
            };
            let rel = p
                .strip_prefix(rt_base)
                .unwrap_or(&p)
                .to_string_lossy()
                .to_string();
            entries.push((rel, h));
        }
    }

    let mut failed = false;
    walk(rt_base, &independent, rt_base, &mut entries, &mut failed);
    if failed {
        return None;
    }
    entries.sort();
    let mut hasher = Sha256::new();
    for (rel, h) in entries {
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        hasher.update(h.as_bytes());
        hasher.update([0u8]);
    }
    Some(digest_prefix(&hasher.finalize()))
}

/// 辅助：设置文件 mtime（仅测试用）。
#[cfg(test)]
fn filetime_set(path: &Path, when: std::time::SystemTime) -> std::io::Result<()> {
    let f = fs::File::options().write(true).open(path)?;
    f.set_times(std::fs::FileTimes::new().set_modified(when))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("arc-rtcache-{tag}-{uniq}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn file_hash_is_content_addressing_not_mtime() {
        let dir = temp_dir("hash");
        let f = dir.join("x.c");
        fs::write(&f, "int x;\n").unwrap();
        let h1 = file_sha256(&f).unwrap();
        // 仅改 mtime（touch）→ 哈希不变（mtime 方案的失效根因）。
        let past = SystemTime::now() - std::time::Duration::from_secs(3600);
        let _ = filetime_set(&f, past);
        assert_eq!(
            file_sha256(&f).unwrap(),
            h1,
            "touch 改变 mtime 不得改变内容哈希"
        );
        // 内容变化 → 哈希必变。
        fs::write(&f, "int x = 1;\n").unwrap();
        assert_ne!(file_sha256(&f).unwrap(), h1, "内容变化必须改变哈希");
    }

    #[test]
    fn deps_hash_covers_headers_and_include_c_excludes_independent_sources() {
        let base = temp_dir("deps");
        fs::create_dir_all(base.join("runtime")).unwrap();
        fs::create_dir_all(base.join("runtime-drawing")).unwrap();
        fs::write(base.join("runtime/rt_abi.h"), "// abi v1\n").unwrap();
        fs::write(base.join("runtime/rt_text.c"), "// text\n").unwrap();
        fs::write(base.join("runtime-drawing/quirc.c"), "// quirc\n").unwrap();
        fs::write(base.join("runtime-drawing/rt_barcode.c"), "// barcode\n").unwrap();

        let independent = vec![
            base.join("runtime/rt_text.c"),
            base.join("runtime-drawing/rt_barcode.c"),
        ];
        let h1 = compute_deps_hash(&base, &independent).unwrap();

        // 独立 TU 的 .c 变化 → deps 不变（由 src_hash 单源覆盖）。
        fs::write(base.join("runtime/rt_text.c"), "// text v2\n").unwrap();
        assert_eq!(
            compute_deps_hash(&base, &independent).unwrap(),
            h1,
            "独立 TU 源不应进入 deps_hash"
        );

        // 头文件变化 → deps 变（mtime 方案盲区）。
        fs::write(base.join("runtime/rt_abi.h"), "// abi v2\n").unwrap();
        assert_ne!(
            compute_deps_hash(&base, &independent).unwrap(),
            h1,
            "头文件变化必须改变 deps_hash"
        );

        // 被 include 的非独立 .c 变化 → deps 变。
        fs::write(base.join("runtime-drawing/quirc.c"), "// quirc v2\n").unwrap();
        let h2 = compute_deps_hash(&base, &independent).unwrap();
        assert_ne!(h2, h1, "被 include 的 .c 变化必须改变 deps_hash");
    }

    #[test]
    fn cache_hit_requires_full_fingerprint_match() {
        let dir = temp_dir("hit");
        let cached = dir.join("rt_text.o");
        let want = CacheFingerprint {
            src_hash: "a".repeat(16),
            deps_hash: "b".repeat(16),
            flags_hash: "c".repeat(16),
            size: 100,
            obj_hash: file_sha256_path_bytes(&[0u8; 100]),
        };
        // 无 .o → miss。
        assert!(!cache_hit(&cached, &want));
        fs::write(&cached, vec![0u8; 100]).unwrap();
        // 有 .o 无指纹 → miss。
        assert!(!cache_hit(&cached, &want));
        // 指纹匹配（含产物内容哈希）→ hit。
        write_meta_atomic(&cached, &want);
        assert!(cache_hit(&cached, &want));
        // 大小不符（截断/半写）→ miss。
        fs::write(&cached, vec![0u8; 99]).unwrap();
        assert!(!cache_hit(&cached, &want), "产物大小不符必须 miss");
        fs::write(&cached, vec![0u8; 100]).unwrap();
        assert!(cache_hit(&cached, &want));
        // **同长度字节损坏（XOR 覆写）→ 内容哈希不符 → miss**（size 判定盲区）。
        let mut damaged = vec![0u8; 100];
        damaged[0] = 0xFF;
        fs::write(&cached, &damaged).unwrap();
        assert!(
            !cache_hit(&cached, &want),
            "同长度损坏必须 miss（obj_hash 强校验）"
        );
        // 恢复 → hit。
        fs::write(&cached, vec![0u8; 100]).unwrap();
        assert!(cache_hit(&cached, &want));
        // 任一指纹字段变化 → miss。
        let mut changed = want.clone();
        changed.src_hash = "d".repeat(16);
        assert!(!cache_hit(&cached, &changed));
        // 指纹文件损坏 → miss。
        fs::write(meta_path(&cached), "garbage").unwrap();
        assert!(!cache_hit(&cached, &want));
    }

    #[cfg(test)]
    fn file_sha256_path_bytes(bytes: &[u8]) -> String {
        sha256_hex(bytes)
    }

    #[test]
    fn file_sha256_matches_one_shot_digest() {
        let dir = temp_dir("oneshot");
        let f = dir.join("x.c");
        // 200KB > 64KB 流式缓冲：强制多轮 read，覆盖缓冲边界。
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(&f, &body).unwrap();
        assert_eq!(
            file_sha256(&f).unwrap(),
            sha256_hex(&body),
            "流式读与一次性摘要必须给出同一指纹"
        );
        assert_eq!(file_sha256(&f).unwrap().len(), 16, "指纹为 16 hex 截断");
    }

    #[test]
    fn meta_roundtrip_and_versioning() {
        let dir = temp_dir("meta");
        let cached = dir.join("x.o");
        let fp = CacheFingerprint {
            src_hash: "1".repeat(16),
            deps_hash: "2".repeat(16),
            flags_hash: "3".repeat(16),
            size: 42,
            obj_hash: "4".repeat(16),
        };
        write_meta_atomic(&cached, &fp);
        let text = fs::read_to_string(meta_path(&cached)).unwrap();
        assert_eq!(parse(&text).unwrap(), fp);
        // 版本不符 → None。
        let old = text.replacen("v1", "v0", 1);
        assert!(parse(&old).is_none());
    }

    #[test]
    fn flags_hash_stable_and_sensitive() {
        let a = flags_hash(Some("x"), false, true, "", &["wgpu", "sqlite"]);
        let b = flags_hash(Some("x"), false, true, "", &["wgpu", "sqlite"]);
        assert_eq!(a, b, "相同选项指纹必须稳定");
        let c = flags_hash(Some("x"), false, true, "", &["wgpu", "drawing"]);
        assert_ne!(a, c, "选项变化必须改变指纹");
    }
}
