//! 增量构建指纹（P0）：基于输入的**保守**指纹比对，跳过未变更构建。
//!
//! ## 设计（对标 C# MSBuild 增量编译，但保守取向）
//!
//! Arc 采用单编译单元（single TU）合并（path 依赖并入同一 TU）。增量以
//! **编译单元级**粒度：对全部可能进入 TU 的源码文件计算确定性内容指纹，
//! 若与上次成功构建的指纹一致且目标产物存在，则跳过整条
//! parse → typeck → codegen → link 流水线，直接判定 up-to-date。
//!
//! ## 正确性保证：保守（安全）取向
//!
//! 指纹**递归覆盖项目根全部 `.as` + 各 path 依赖树全部 `.as`**（含子目录，
//! 这些可能经 `using` 被拉入 TU）。**宁多勿少**——覆盖超集只会导致偶尔的
//! 多余重编译，绝不产生陈旧产物；反之（欠覆盖）会导致陈旧产物，绝对禁止。
//!
//! ## 确定性
//!
//! 使用 FNV-1a 64 位哈希（非 `DefaultHasher`——其 SipHash 种子跨进程随机，
//! 会使指纹在多次 build 间不稳定）。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::hash::hex_sha256;
use crate::manifest::ArcManifest;

/// 工具链身份（十六进制）：当前编译器可执行文件的完整内容哈希。
///
/// **根治陈旧产物**：up-to-date 判定原先只比对源码 mtime/内容，不感知编译器。
/// 编译器任一变（rebuilt）→ 本哈希必变 → 指纹必变 → 该 obj_dir 全部缓存失效、
/// 触发重编。用 `OnceLock` 缓存，进程内只哈希一次，代价可忽略。
fn toolchain_identity() -> String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            // 退化路径：拿不到当前可执行文件时退化为 crate 版本号，保证仍含身份。
            if let Ok(p) = std::env::current_exe() {
                if let Ok(bytes) = std::fs::read(&p) {
                    let mut f = Fnv::new();
                    f.feed_str("exe");
                    f.feed(&bytes);
                    return format!("{:016x}", f.finish());
                }
            }
            format!("v{}", env!("CARGO_PKG_VERSION"))
        })
        .clone()
}

/// FNV-1a 累加器（64 位）。
struct Fnv {
    hash: u64,
}

impl Fnv {
    fn new() -> Self {
        Fnv {
            hash: 0xcbf29ce484222325,
        }
    }
    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.hash ^= b as u64;
            self.hash = self.hash.wrapping_mul(0x100000001b3);
        }
    }
    fn feed_str(&mut self, s: &str) {
        self.feed(s.as_bytes());
        self.feed(b"\x1f"); // 分隔符，避免边界拼接歧义
    }
    fn finish(self) -> u64 {
        self.hash
    }
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

/// 递归收集目录下全部 `.as` 文件（跳过 obj/ / bin/ / target 生成物目录）。
fn collect_as_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "obj" || name == "bin" || name == "target" {
                continue;
            }
            collect_as_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("as") {
            out.push(path);
        }
    }
}

/// 项目根：入口文件父目录，或入口目录本身。
fn project_root_of(entry: &Path) -> PathBuf {
    if entry.is_file() {
        entry
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        entry.to_path_buf()
    }
}

/// 增量指纹输入（`arc build` / `arc test` 共用）。
///
/// `arc build` 仅填核心字段（`extra_*` 为空）；`arc test` 额外填：
/// - `extra_source_dirs`：std 源码树（保守覆盖，宁多勿少，仅导致多余重编，绝不陈旧）；
/// - `extra_inputs`：QIF 编译选项（决定合成 `__QifTestHost.Main()` 的产物形态）。
pub struct FingerprintInputs<'a> {
    pub entry: &'a Path,
    pub manifest: Option<&'a ArcManifest>,
    pub config: &'a str,
    pub triple: &'a str,
    pub out_name: &'a str,
    pub debug: bool,
    /// 额外源码目录：恒按「相对该目录的路径 + 内容」参与指纹（不做内容寻址跳过）。
    pub extra_source_dirs: Vec<PathBuf>,
    /// 额外语义输入：`(key, value)` 键值对（如 QIF 编译选项）。
    pub extra_inputs: Vec<(String, String)>,
}

/// 计算一次构建的输入指纹（16 位十六进制）。
///
/// 覆盖：**工具链身份（编译器本体哈希）** + 入口所在项目根递归 `.as` + 各 path
/// 依赖树递归 `.as` + `arc.toml` 内容 + 配置 / debug flag / 目标 triple / 产物名，
/// 外加 `extra_source_dirs`（额外源码树）与 `extra_inputs`（额外语义键值）。
/// 递归覆盖保证**保守正确**（不陈旧）。工具链身份保证**编译器变更即失效**。
///
/// RFC 017 源码打包：path 依赖源码合并进单一编译单元，其源码内容直接参与指纹
/// （无预编译对象——变更即重编，语义与 C# ProjectReference 增量一致）。
pub fn compute_fingerprint_inputs(inputs: &FingerprintInputs) -> String {
    let tc = toolchain_identity();
    let mut f = Fnv::new();
    f.feed_str("toolchain");
    f.feed_str(&tc);
    f.feed_str("cfg");
    f.feed_str(inputs.config);
    f.feed_str("debug");
    f.feed_str(if inputs.debug { "1" } else { "0" });
    f.feed_str("triple");
    f.feed_str(inputs.triple);
    f.feed_str("out");
    f.feed_str(inputs.out_name);

    let project_root = project_root_of(inputs.entry);

    // arc.toml 内容参与指纹。
    let arc_toml = project_root.join("arc.toml");
    if arc_toml.is_file() {
        if let Ok(c) = std::fs::read_to_string(&arc_toml) {
            f.feed_str("arc.toml");
            f.feed_str(&c);
        }
    }

    // 项目根全部 `.as`（含子目录，保守覆盖）。
    let mut files: Vec<PathBuf> = Vec::new();
    collect_as_files(&project_root, &mut files);

    // path 依赖：源码合并进单一编译单元（RFC 017），源码树直接参与指纹。
    if let Some(m) = inputs.manifest {
        for spec in m.dependencies.values() {
            let dep_dir = project_root.join(&spec.path);
            if dep_dir.is_dir() {
                collect_as_files(&dep_dir, &mut files);
            }
        }
    }

    // 相对路径 + 内容排序哈希，保证顺序无关且稳定。
    let mut rels: Vec<(String, String)> = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(&project_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        let content = std::fs::read(&path).unwrap_or_default();
        rels.push((rel, String::from_utf8_lossy(&content).into_owned()));
    }
    rels.sort_by(|a, b| a.0.cmp(&b.0));
    for (rel, content) in rels {
        f.feed_str("file");
        f.feed_str(&rel);
        f.feed_str(&content);
    }

    // 额外源码目录（如 test 的 std 源码树）：相对该目录路径 + 内容，排序保证稳定。
    // 与项目根扫描一样做保守超集覆盖（宁多勿少，绝不陈旧）。
    for dir in &inputs.extra_source_dirs {
        let mut extra_files: Vec<PathBuf> = Vec::new();
        collect_as_files(dir, &mut extra_files);
        let mut extra: Vec<(String, String)> = Vec::new();
        for path in extra_files {
            let rel = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let content = std::fs::read(&path).unwrap_or_default();
            extra.push((rel, String::from_utf8_lossy(&content).into_owned()));
        }
        extra.sort_by(|a, b| a.0.cmp(&b.0));
        for (rel, content) in extra {
            f.feed_str("extra-src");
            f.feed_str(&rel);
            f.feed_str(&content);
        }
    }

    // 额外语义输入（如 test 的 QIF 编译选项）。按插入顺序参与，调用方保证顺序稳定。
    for (key, value) in &inputs.extra_inputs {
        f.feed_str("extra-input");
        f.feed_str(key);
        f.feed_str(value);
    }

    hex(f.finish())
}

/// `arc build` 增量指纹（便捷封装）：无额外源码树 / 语义输入。
pub fn compute_fingerprint(
    entry: &Path,
    manifest: Option<&ArcManifest>,
    config: &str,
    triple: &str,
    out_name: &str,
    debug: bool,
) -> String {
    compute_fingerprint_inputs(&FingerprintInputs {
        entry,
        manifest,
        config,
        triple,
        out_name,
        debug,
        extra_source_dirs: Vec::new(),
        extra_inputs: Vec::new(),
    })
}

/// 增量戳文件名：`tag` 为空 → `.arc-incremental`（build）；非空 → `.arc-incremental-<tag>`。
///
/// build / test 共用同一 `obj/<config>/`，但产物链不同（源集与合成入口不同），指纹
/// 亦不同。用不同戳文件隔离，避免两套指纹互相覆盖导致交替重编译。
fn stamp_file_name(tag: Option<&str>) -> String {
    match tag {
        Some(t) if !t.is_empty() => format!(".arc-incremental-{t}"),
        _ => ".arc-incremental".to_string(),
    }
}

/// 读取上次构建的指纹（`<obj_dir>/.arc-incremental[-<tag>]`）。
fn read_stamp(obj_dir: &Path, tag: Option<&str>) -> Option<String> {
    let path = obj_dir.join(stamp_file_name(tag));
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// 写入本次构建指纹（`<obj_dir>/.arc-incremental[-<tag>]`）。
fn write_stamp(obj_dir: &Path, tag: Option<&str>, fingerprint: &str) {
    if let Err(e) = std::fs::create_dir_all(obj_dir) {
        eprintln!("incremental: create obj dir failed: {e}");
        return;
    }
    let path = obj_dir.join(stamp_file_name(tag));
    if let Err(e) = std::fs::write(&path, fingerprint) {
        eprintln!("incremental: write stamp failed: {e}");
    }
}

/// 判断是否 up-to-date：上次指纹一致 **且** 目标产物存在（build 路径，戳 `.arc-incremental`）。
pub fn is_up_to_date(obj_dir: &Path, out: &Path, fingerprint: &str) -> bool {
    is_up_to_date_tagged(obj_dir, None, out, fingerprint)
}

/// 判断是否 up-to-date：上次指纹一致 **且** 目标产物存在（可指定戳 tag，区分产物链）。
pub fn is_up_to_date_tagged(
    obj_dir: &Path,
    tag: Option<&str>,
    out: &Path,
    fingerprint: &str,
) -> bool {
    if !out.is_file() {
        return false;
    }
    read_stamp(obj_dir, tag).as_deref() == Some(fingerprint)
}

/// 构建完成后记录指纹（build 路径，戳 `.arc-incremental`）。
pub fn record_build(obj_dir: &Path, fingerprint: &str) {
    record_build_tagged(obj_dir, None, fingerprint);
}

/// 构建完成后记录指纹（可指定戳 tag，区分产物链）。
pub fn record_build_tagged(obj_dir: &Path, tag: Option<&str>, fingerprint: &str) {
    write_stamp(obj_dir, tag, fingerprint);
}

/// `--incremental-report` 输出体：本次构建哪些 `.as` 被重编 / 复用，及耗时
/// （`elapsed_ms` 由调用方回填）。
#[derive(Debug, Clone, Default)]
pub struct IncrementalReport {
    pub total_files: usize,
    pub rebuilt_files: usize,
    pub reused_files: usize,
    /// 重编文件相对路径（排序）。
    pub rebuilt: Vec<String>,
    /// 复用文件相对路径（排序）。
    pub reused: Vec<String>,
    /// 构建耗时（毫秒）。
    pub elapsed_ms: u64,
}

/// per-file 内容戳路径（`<obj_dir>/.arc-incremental-files`）。
fn file_stamp_path(obj_dir: &Path) -> PathBuf {
    obj_dir.join(".arc-incremental-files")
}

fn read_file_stamp(obj_dir: &Path) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    if let Ok(content) = std::fs::read_to_string(file_stamp_path(obj_dir)) {
        for line in content.lines() {
            if let Some((rel, hash)) = line.split_once('\t') {
                map.insert(rel.to_string(), hash.to_string());
            }
        }
    }
    map
}

fn write_file_stamp(obj_dir: &Path, rels: &[(String, String)]) {
    if let Err(e) = std::fs::create_dir_all(obj_dir) {
        eprintln!("incremental: create obj dir failed: {e}");
        return;
    }
    let mut content = String::new();
    for (rel, hash) in rels {
        content.push_str(rel);
        content.push('\t');
        content.push_str(hash);
        content.push('\n');
    }
    if let Err(e) = std::fs::write(file_stamp_path(obj_dir), content) {
        eprintln!("incremental: write file stamp failed: {e}");
    }
}

/// 计算本次构建的增量报告：对比上次 per-file 内容戳，列出重编 / 复用文件，并刷新戳。
///
/// 文件集与 [`compute_fingerprint`] 口径一致：入口项目根全部 `.as` + path 依赖树
/// 全部 `.as`（源码打包：依赖源码合并进单一编译单元，RFC 017）。
pub fn compute_incremental_report(
    entry: &Path,
    manifest: Option<&ArcManifest>,
    obj_dir: &Path,
) -> IncrementalReport {
    let project_root = project_root_of(entry);
    let mut files: Vec<PathBuf> = Vec::new();
    collect_as_files(&project_root, &mut files);
    if let Some(m) = manifest {
        for spec in m.dependencies.values() {
            let dep_dir = project_root.join(&spec.path);
            if dep_dir.is_dir() {
                collect_as_files(&dep_dir, &mut files);
            }
        }
    }

    let mut rels: Vec<(String, String)> = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(&project_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        let content = std::fs::read(&path).unwrap_or_default();
        rels.push((rel, hex_sha256(&content)));
    }
    rels.sort_by(|a, b| a.0.cmp(&b.0));

    let prev = read_file_stamp(obj_dir);
    let mut rebuilt = Vec::new();
    let mut reused = Vec::new();
    for (rel, hash) in &rels {
        if prev.get(rel) == Some(hash) {
            reused.push(rel.clone());
        } else {
            rebuilt.push(rel.clone());
        }
    }
    write_file_stamp(obj_dir, &rels);

    IncrementalReport {
        total_files: rels.len(),
        rebuilt_files: rebuilt.len(),
        reused_files: reused.len(),
        rebuilt,
        reused,
        elapsed_ms: 0,
    }
}

/// 格式化 `--incremental-report` 输出（供 main.rs 打印；门禁脚本按行解析）。
pub fn format_incremental_report(report: &IncrementalReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "incremental-report: total_files={} rebuilt_files={} reused_files={} elapsed_ms={}",
        report.total_files, report.rebuilt_files, report.reused_files, report.elapsed_ms
    );
    for f in &report.rebuilt {
        let _ = writeln!(out, "incremental-report: file rebuilt {f}");
    }
    for f in &report.reused {
        let _ = writeln!(out, "incremental-report: file reused {f}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arc-incr-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn fingerprint_stable_and_content_sensitive() {
        let d = tmp("stable");
        fs::write(d.join("a.as"), "namespace A { class X {} }").unwrap();
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("sub/b.as"), "namespace A.Sub { class Y {} }").unwrap();
        fs::write(d.join("arc.toml"), "[package]\nname=\"p\"\n").unwrap();

        let f1 = compute_fingerprint(&d.join("a.as"), None, "Debug", "x86_64", "p.exe", false);
        let f2 = compute_fingerprint(&d.join("a.as"), None, "Debug", "x86_64", "p.exe", false);
        assert_eq!(f1, f2, "identical inputs must give identical fingerprint");

        // 改子目录文件（可能经 using 拉入 TU）→ 指纹变化（保守覆盖）
        fs::write(d.join("sub/b.as"), "namespace A.Sub { class Y2 {} }").unwrap();
        let f3 = compute_fingerprint(&d.join("a.as"), None, "Debug", "x86_64", "p.exe", false);
        assert_ne!(f1, f3, "content change must change fingerprint");

        // 配置变化 → 指纹变化
        let f4 = compute_fingerprint(&d.join("a.as"), None, "Release", "x86_64", "p.exe", false);
        assert_ne!(f1, f4, "config change must change fingerprint");

        // debug flag 变化 → 指纹变化
        let f5 = compute_fingerprint(&d.join("a.as"), None, "Debug", "x86_64", "p.exe", true);
        assert_ne!(f1, f5, "debug flag change must change fingerprint");

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn toolchain_identity_stable_and_nonempty() {
        let a = toolchain_identity();
        let b = toolchain_identity();
        assert!(!a.is_empty(), "toolchain identity must be non-empty");
        assert_eq!(a, b, "toolchain identity must be stable within a process");
    }

    #[test]
    fn up_to_date_requires_matching_stamp_and_existing_out() {
        let d = tmp("uptodate");
        let obj = d.join("obj/Debug");
        let out = d.join("bin/Debug/p.exe");
        let fp = compute_fingerprint(&d.join("a.as"), None, "Debug", "x", "p.exe", false);

        // 无产物 → 非 up-to-date
        assert!(!is_up_to_date(&obj, &out, &fp));

        // 有产物但无 stamp → 非 up-to-date
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(&out, "exe").unwrap();
        assert!(!is_up_to_date(&obj, &out, &fp));

        // stamp 匹配 + 产物存在 → up-to-date
        record_build(&obj, &fp);
        assert!(is_up_to_date(&obj, &out, &fp));

        // 指纹变化 → 非 up-to-date
        let fp2 = compute_fingerprint(&d.join("a.as"), None, "Debug", "x", "other.exe", false);
        assert!(!is_up_to_date(&obj, &out, &fp2));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn path_dep_source_participates_in_fingerprint() {
        let d = tmp("pathdep");
        // path 依赖与入口项目同级（`../lib`），避免落入入口项目根的源码扫描。
        let app = d.join("app");
        let lib = d.join("lib");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&lib).unwrap();
        fs::write(app.join("main.as"), "void Main() {}").unwrap();
        fs::write(
            app.join("arc.toml"),
            "[package]\nname=\"app\"\n\n[dependencies]\nmylib = { path = \"../lib\" }\n",
        )
        .unwrap();
        let manifest = ArcManifest::load(&app.join("arc.toml")).unwrap();
        let entry = app.join("main.as");
        let src = lib.join("a.as");

        // 无依赖源码 → 指纹基准。
        let f_nodep = compute_fingerprint(&entry, None, "Debug", "x86_64", "app.exe", false);

        // 依赖源码 v1 → 指纹变化（源码合并进单一编译单元，RFC 017）。
        fs::write(
            &src,
            "namespace mylib; public class A { public int F() { return 1; } }",
        )
        .unwrap();
        let f_v1 =
            compute_fingerprint(&entry, Some(&manifest), "Debug", "x86_64", "app.exe", false);
        assert_ne!(
            f_nodep, f_v1,
            "adding a path dep must change the fingerprint"
        );

        // 依赖源码内容变化 → 指纹变化（无对象缓存捷径，变更即重编）。
        fs::write(
            &src,
            "namespace mylib; public class A { public int F() { return 2; } }",
        )
        .unwrap();
        let f_v2 =
            compute_fingerprint(&entry, Some(&manifest), "Debug", "x86_64", "app.exe", false);
        assert_ne!(
            f_v1, f_v2,
            "path dep source change must change the fingerprint"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn incremental_report_rebuilt_then_reused() {
        let d = tmp("report");
        fs::write(d.join("main.as"), "void Main() {}").unwrap();
        fs::write(d.join("arc.toml"), "[package]\nname=\"app\"\n").unwrap();
        let obj = d.join("obj").join("Debug");

        let r1 = compute_incremental_report(&d.join("main.as"), None, &obj);
        assert_eq!(r1.total_files, 1);
        assert_eq!(r1.rebuilt_files, 1);
        assert_eq!(r1.reused_files, 0);

        let r2 = compute_incremental_report(&d.join("main.as"), None, &obj);
        assert_eq!(r2.rebuilt_files, 0);
        assert_eq!(r2.reused_files, 1);

        fs::write(d.join("main.as"), "void Main() { int x = 1; }").unwrap();
        let r3 = compute_incremental_report(&d.join("main.as"), None, &obj);
        assert_eq!(r3.rebuilt_files, 1);
        assert_eq!(r3.reused_files, 0);
        let _ = fs::remove_dir_all(&d);
    }

    fn test_inputs<'a>(
        entry: &'a Path,
        extra_source_dirs: Vec<PathBuf>,
        extra_inputs: Vec<(String, String)>,
    ) -> FingerprintInputs<'a> {
        FingerprintInputs {
            entry,
            manifest: None,
            config: "Debug",
            triple: "x86_64",
            out_name: "UnitTest.test.exe",
            debug: false,
            extra_source_dirs,
            extra_inputs,
        }
    }

    #[test]
    fn fingerprint_inputs_extras_change_fingerprint() {
        let d = tmp("extras");
        fs::write(d.join("main.as"), "void Main() {}").unwrap();
        let std_dir = d.join("std");
        fs::create_dir_all(std_dir.join("Arc")).unwrap();
        fs::write(
            std_dir.join("Arc/Core.as"),
            "namespace Arc; public class Core {}",
        )
        .unwrap();

        let base =
            compute_fingerprint_inputs(&test_inputs(&d.join("main.as"), Vec::new(), Vec::new()));

        // 相同输入 → 相同指纹。
        let again =
            compute_fingerprint_inputs(&test_inputs(&d.join("main.as"), Vec::new(), Vec::new()));
        assert_eq!(base, again);

        // 额外源码树（std）内容变化 → 指纹变化。
        let with_std = compute_fingerprint_inputs(&test_inputs(
            &d.join("main.as"),
            vec![std_dir.clone()],
            Vec::new(),
        ));
        assert_ne!(
            base, with_std,
            "adding std source tree must change fingerprint"
        );
        fs::write(
            std_dir.join("Arc/Core.as"),
            "namespace Arc; public class Core { public int X; }",
        )
        .unwrap();
        let std_changed = compute_fingerprint_inputs(&test_inputs(
            &d.join("main.as"),
            vec![std_dir.clone()],
            Vec::new(),
        ));
        assert_ne!(
            with_std, std_changed,
            "std source change must change fingerprint"
        );

        // 额外语义输入（QIF 选项）变化 → 指纹变化。
        let qif1 = vec![("qif.output_format".to_string(), "human".to_string())];
        let qif2 = vec![("qif.output_format".to_string(), "json".to_string())];
        let f_qif1 = compute_fingerprint_inputs(&test_inputs(&d.join("main.as"), Vec::new(), qif1));
        let f_qif2 = compute_fingerprint_inputs(&test_inputs(&d.join("main.as"), Vec::new(), qif2));
        assert_ne!(f_qif1, f_qif2, "qif option change must change fingerprint");
        assert_ne!(base, f_qif1, "adding qif options must change fingerprint");

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn stamp_tag_isolates_build_and_test() {
        let d = tmp("tag");
        let obj = d.join("obj/Debug");
        let out = d.join("bin/Debug/UnitTest.test.exe");
        // build / test 两套产物链指纹不同，隔离必须按各自指纹独立判定。
        let test_fp = "testfp000000000000".to_string();
        let build_fp = "buildfp00000000000".to_string();

        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(&out, "exe").unwrap();

        // 记录 test 戳 → 仅 test tag 判定 up-to-date，build（无 tag）不命中。
        record_build_tagged(&obj, Some("test"), &test_fp);
        assert!(is_up_to_date_tagged(&obj, Some("test"), &out, &test_fp));
        assert!(
            !is_up_to_date(&obj, &out, &test_fp),
            "test stamp must not satisfy build"
        );

        // 记录 build 戳 → 仅 build 判定 up-to-date，test 不命中（即使同 obj_dir）。
        record_build(&obj, &build_fp);
        assert!(is_up_to_date(&obj, &out, &build_fp));
        assert!(
            !is_up_to_date_tagged(&obj, Some("test"), &out, &build_fp),
            "build stamp must not satisfy test"
        );
        assert!(
            is_up_to_date_tagged(&obj, Some("test"), &out, &test_fp),
            "test stamp must remain intact after build record"
        );

        let _ = fs::remove_dir_all(&d);
    }
}
