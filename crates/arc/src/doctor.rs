//! `arc doctor`：SDK/工具链环境自检（Phase 1）。
//!
//! 对标 `rustup doctor`：逐项 PASS / WARN / FAIL + 修复提示；存在任何 FAIL 时
//! 退出码非零（供 CI 门禁）。检测项覆盖 SDK 完整性、clang/LLVM 可用性、
//! `ARC_STD_ROOT` 一致性、rt_cache 可写、native DLL（crypto_native/wgpu_native）、
//! MSVC（Windows msvc 宿主）与环境变量。

use std::path::PathBuf;
use std::process::Command;

use crate::env::{snapshot, EnvSnapshot, ARC_CLANG_ENV, ARC_HOME_ENV, ARC_STD_ROOT_ENV};
use codegen::sdk_layout::{SdkLayoutKind, ARC_SDK_ROOT_ENV};

/// 单项检测结果状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    fn label(self) -> &'static str {
        match self {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        }
    }
}

/// 单项检测。
#[derive(Debug, Clone)]
pub struct Check {
    /// 稳定机器名（JSON `name` 字段；human 输出不使用）。
    pub name: &'static str,
    /// 人类可读标题。
    pub title: String,
    pub status: CheckStatus,
    pub detail: String,
    /// 修复提示（非必需）。
    pub hint: Option<String>,
}

impl Check {
    fn pass(name: &'static str, title: impl Into<String>, detail: impl Into<String>) -> Check {
        Check {
            name,
            title: title.into(),
            status: CheckStatus::Pass,
            detail: detail.into(),
            hint: None,
        }
    }

    fn warn(
        name: &'static str,
        title: impl Into<String>,
        detail: impl Into<String>,
        hint: impl Into<String>,
    ) -> Check {
        Check {
            name,
            title: title.into(),
            status: CheckStatus::Warn,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }

    fn fail(
        name: &'static str,
        title: impl Into<String>,
        detail: impl Into<String>,
        hint: impl Into<String>,
    ) -> Check {
        Check {
            name,
            title: title.into(),
            status: CheckStatus::Fail,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
}

/// 运行全部检测并输出报告；返回进程退出码（有 FAIL 即 1，否则 0）。
pub fn run_doctor(json: bool) -> Result<i32, String> {
    let env = snapshot();
    let mut checks: Vec<Check> = Vec::new();
    check_sdk_root(&env, &mut checks);
    check_sdk_structure(&env, &mut checks);
    check_clang(&env, &mut checks);
    check_msvc(&env, &mut checks);
    check_arc_std_root(&mut checks);
    check_rt_cache_writable(&env, &mut checks);
    check_rt_cache_integrity(&env, &mut checks);
    check_native_dlls(&env, &mut checks);
    check_env_vars(&env, &mut checks);

    if json {
        println!("{}", format_json(&env, &checks)?);
    } else {
        print!("{}", format_human(&env, &checks));
    }

    let failures = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .count();
    Ok(if failures > 0 { 1 } else { 0 })
}

fn format_human(env: &EnvSnapshot, checks: &[Check]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "arc doctor v{} (host: {})\n\n",
        env.version, env.host_triple
    ));
    let mut fails = 0usize;
    let mut warns = 0usize;
    for c in checks {
        match c.status {
            CheckStatus::Fail => fails += 1,
            CheckStatus::Warn => warns += 1,
            CheckStatus::Pass => {}
        }
        out.push_str(&format!(
            "[{}] {} — {}\n",
            c.status.label(),
            c.title,
            c.detail
        ));
        if let Some(hint) = &c.hint {
            out.push_str(&format!("       hint: {hint}\n"));
        }
    }
    out.push_str(&format!(
        "\n{} check(s) passed, {} warning(s), {} failed\n",
        checks.len() - fails - warns,
        warns,
        fails
    ));
    out
}

fn format_json(env: &EnvSnapshot, checks: &[Check]) -> Result<String, String> {
    use serde_json::json;
    let summary = json!({
        "checks": checks.len(),
        "passed": checks.iter().filter(|c| c.status == CheckStatus::Pass).count(),
        "warnings": checks.iter().filter(|c| c.status == CheckStatus::Warn).count(),
        "failed": checks.iter().filter(|c| c.status == CheckStatus::Fail).count(),
    });
    let items: Vec<serde_json::Value> = checks
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "status": c.status.label(),
                "title": c.title,
                "detail": c.detail,
                "hint": c.hint,
            })
        })
        .collect();
    let value = json!({
        "version": env.version,
        "host_triple": env.host_triple,
        "summary": summary,
        "checks": items,
    });
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

/// 1. SDK 根自定位（`ARC_SDK_ROOT` > `current_exe()` 逐级向上 > 开发兜底）。
fn check_sdk_root(env: &EnvSnapshot, checks: &mut Vec<Check>) {
    match (&env.sdk_root, &env.sdk_layout) {
        (Some(root), Some(layout)) => {
            checks.push(Check::pass(
                "sdk-root",
                "SDK root located",
                format!("{} ({})", root.display(), layout.label()),
            ));
        }
        (Some(root), None) => {
            checks.push(Check::fail(
                "sdk-root",
                "SDK root lacks a valid layout",
                format!("{} is not an installed or repo SDK layout", root.display()),
                "set ARC_SDK_ROOT to an SDK root: bin/arc.exe + lib/{std,rt,native} (installed) or std/ + crates/runtime (repo)",
            ));
        }
        (None, _) => {
            checks.push(Check::fail(
                "sdk-root",
                "SDK root not found",
                "self-location failed and ARC_SDK_ROOT is unset",
                "run arc from an installed SDK (bin/arc.exe + lib/) or a repo checkout (std/ + crates/runtime), or set ARC_SDK_ROOT",
            ));
        }
    }
}

/// 2. SDK 结构完整性（bin/lib 布局与关键资源目录）。
fn check_sdk_structure(env: &EnvSnapshot, checks: &mut Vec<Check>) {
    let Some(root) = &env.sdk_root else {
        checks.push(Check::fail(
            "sdk-structure",
            "SDK structure",
            "no SDK root to inspect (see sdk-root)",
            "resolve the SDK root first",
        ));
        return;
    };
    let required: Vec<PathBuf> = match env.sdk_layout {
        Some(SdkLayoutKind::Installed) => vec![
            root.join("bin/arc.exe"),
            root.join("lib/std"),
            root.join("lib/rt"),
            root.join("lib/native"),
        ],
        Some(SdkLayoutKind::Repo) => vec![
            root.join("std"),
            root.join("crates/runtime"),
            root.join("crates/arc/native"),
        ],
        None => {
            checks.push(Check::fail(
                "sdk-structure",
                "SDK structure",
                "unknown layout",
                "see sdk-root",
            ));
            return;
        }
    };
    let missing: Vec<String> = required
        .iter()
        .filter(|p| !p.exists())
        .map(|p| p.display().to_string())
        .collect();
    if missing.is_empty() {
        checks.push(Check::pass(
            "sdk-structure",
            "SDK structure",
            "required SDK directories present",
        ));
    } else {
        checks.push(Check::fail(
            "sdk-structure",
            "SDK structure incomplete",
            format!("missing: {}", missing.join(", ")),
            "reinstall or re-run the SDK packaging script (scripts/packaging/arc-pack.ps1)",
        ));
    }
}

/// 3. clang/LLVM 可用性（`arc build` 硬依赖）+ 支持基线（R-2：LLVM 22）。
fn check_clang(env: &EnvSnapshot, checks: &mut Vec<Check>) {
    if let Some(v) = crate::env::env_var(ARC_CLANG_ENV) {
        let p = PathBuf::from(&v);
        if !p.is_file() && v != "clang" {
            checks.push(Check::fail(
                "clang",
                "clang binary",
                format!("ARC_CLANG points to a missing file: {v}"),
                "unset ARC_CLANG or point it to a real clang executable".to_string(),
            ));
            return;
        }
    }
    let clang = &env.clang;
    match Command::new(clang).arg("--version").output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let version_line = text
                .lines()
                .find(|l| l.starts_with("clang version"))
                .unwrap_or("(version unknown)")
                .trim();
            match crate::clang_version::version_from_clang_output(&text) {
                Some(v) => {
                    if let Some(why) = crate::clang_version::ensure_clang_min_version(v) {
                        checks.push(Check::fail(
                            "clang-version",
                            "clang version below baseline",
                            format!("{clang}: {version_line} — {why}"),
                            format!(
                                "install LLVM {} (clang + lld), or `arc toolchain install llvm` (with \
                                 `--archive <zip>` for offline); older clang works for most builds but \
                                 is not a supported baseline",
                                crate::clang_version::LLVM_MIN_VERSION
                            ),
                        ));
                    } else {
                        checks.push(Check::pass(
                            "clang",
                            "clang binary",
                            format!("{clang}: {version_line}"),
                        ));
                        checks.push(Check::pass(
                            "clang-version",
                            "clang version",
                            format!("clang {v} ≥ {}", crate::clang_version::LLVM_MIN_VERSION),
                        ));
                    }
                }
                None => checks.push(Check::pass(
                    "clang",
                    "clang binary",
                    format!(
                        "{clang}: {version_line} (version not parseable; skipping floor check)"
                    ),
                )),
            }
        }
        Ok(_) | Err(_) => {
            checks.push(Check::fail(
                "clang",
                "clang binary not usable",
                format!("`{clang} --version` failed — clang is required for `arc build`"),
                format!(
                    "install LLVM (clang + lld, ≥ {}), run `arc toolchain install llvm` (with \
                     `--archive <zip>` for offline), or set ARC_CLANG to the clang executable",
                    crate::clang_version::LLVM_MIN_VERSION
                ),
            ));
        }
    }
}

/// 4. VS/MSVC 可用性（Windows msvc 宿主需要 clang 找到 MSVC CRT）。
fn check_msvc(env: &EnvSnapshot, checks: &mut Vec<Check>) {
    let is_windows_msvc = env.host_triple.contains("windows") && env.host_triple.contains("msvc");
    if !is_windows_msvc {
        checks.push(Check::pass(
            "vs-msvc",
            "MSVC toolchain",
            "not required on this host",
        ));
        return;
    }
    let clang = &env.clang;
    // 1. vswhere（权威来源，覆盖 VS 装在非默认盘的场景）。
    let vswhere =
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    if vswhere.is_file() {
        if let Ok(out) = Command::new(&vswhere)
            .args(["-latest", "-products", "*", "-property", "installationPath"])
            .output()
        {
            if out.status.success() {
                let vs = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !vs.is_empty() {
                    checks.push(Check::pass(
                        "vs-msvc",
                        "MSVC toolchain",
                        format!("Visual Studio detected at {vs}"),
                    ));
                    return;
                }
            }
        }
    }
    // 2. clang 自身探测到的 MSVC 布局。
    if let Ok(out) = Command::new(clang).arg("-print-prog-name=cl").output() {
        let prog = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !prog.is_empty() && prog != "cl" {
            checks.push(Check::pass(
                "vs-msvc",
                "MSVC toolchain",
                format!("clang resolves MSVC cl at {prog}"),
            ));
            return;
        }
    }
    // 3. 实编探测：clang 编译 + 链接一个空 C 程序（完整验证 CRT 链路）。
    if compile_probe(clang).is_ok() {
        checks.push(Check::pass(
            "vs-msvc",
            "MSVC toolchain",
            "clang compiles and links against MSVC CRT",
        ));
        return;
    }
    checks.push(Check::fail(
        "vs-msvc",
        "MSVC toolchain not found",
        "clang cannot locate the MSVC CRT/import libraries for the msvc host triple",
        "install Visual Studio Build Tools (Desktop development with C++), or configure clang's MSVC detection (vswhere)",
    ));
}

/// 用 clang 编译并链接一个空 C 程序（探测完整 toolchain 链）。
fn compile_probe(clang: &str) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("arc-doctor-msvc-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let src = dir.join("probe.c");
    let exe = dir.join("probe.exe");
    let _ = std::fs::remove_file(&exe);
    let result = std::fs::write(&src, "int main(void) { return 0; }")
        .and_then(|_| Command::new(clang).arg(&src).arg("-o").arg(&exe).output())
        .map_err(|e| e.to_string());
    let ok = match result {
        Ok(out) => out.status.success() && exe.is_file(),
        Err(_) => false,
    };
    let _ = std::fs::remove_dir_all(&dir);
    if ok {
        Ok(())
    } else {
        Err("probe compile failed".into())
    }
}

/// 5. `ARC_STD_ROOT` 一致性（设置后须指向存在的目录）。
fn check_arc_std_root(checks: &mut Vec<Check>) {
    match crate::env::env_var(ARC_STD_ROOT_ENV) {
        Some(v) => {
            let p = PathBuf::from(&v);
            if p.is_dir() {
                checks.push(Check::pass(
                    "arc-std-root",
                    "ARC_STD_ROOT consistency",
                    format!("{ARC_STD_ROOT_ENV} set and valid: {}", p.display()),
                ));
            } else {
                checks.push(Check::fail(
                    "arc-std-root",
                    "ARC_STD_ROOT inconsistency",
                    format!("{ARC_STD_ROOT_ENV} points to a missing directory: {v}"),
                    "unset ARC_STD_ROOT or point it to an existing std library root",
                ));
            }
        }
        None => checks.push(Check::pass(
            "arc-std-root",
            "ARC_STD_ROOT consistency",
            "not set (SDK bundled std or workspace/std in effect)",
        )),
    }
}

/// 6. rt_cache 可写（runtime `.o` 缓存根）。
fn check_rt_cache_writable(env: &EnvSnapshot, checks: &mut Vec<Check>) {
    let dir = env.rt_cache.clone();
    let probe = dir.join("doctor-write-probe");
    let result = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&probe, b"ok"))
        .and_then(|_| std::fs::remove_file(&probe));
    match result {
        Ok(()) => checks.push(Check::pass(
            "rt-cache-writable",
            "rt_cache writable",
            format!("{}", dir.display()),
        )),
        Err(e) => checks.push(Check::fail(
            "rt-cache-writable",
            "rt_cache not writable",
            format!("{}: {e}", dir.display()),
            format!(
                "ensure {} exists and is writable, or set {ARC_HOME_ENV} to a writable directory",
                dir.display()
            ),
        )),
    }
}

/// 6b. rt_cache 完整性（内容寻址指纹）：无 `.arc-fp` 指纹的 `.o` 为旧
/// mtime 格式遗留、或指纹与产物大小不符（损坏/半写）——均会在下次构建
/// 自动重编自愈（`codegen::rt_cache` 内容寻址命中判定），此处仅提示可清理。
fn check_rt_cache_integrity(env: &EnvSnapshot, checks: &mut Vec<Check>) {
    let dir = env.rt_cache.clone();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        checks.push(Check::pass(
            "rt-cache-integrity",
            "rt_cache integrity",
            "cache dir absent (nothing cached yet)",
        ));
        return;
    };
    let mut stale = 0usize;
    let mut corrupt = 0usize;
    let mut total = 0usize;
    for key_dir in rd.flatten() {
        if !key_dir.path().is_dir() {
            continue;
        }
        let Ok(objs) = std::fs::read_dir(key_dir.path()) else {
            continue;
        };
        for o in objs.flatten() {
            let p = o.path();
            if p.extension().and_then(|s| s.to_str()) != Some("o") {
                continue;
            }
            total += 1;
            let meta = codegen::rt_cache::meta_path(&p);
            let Some(meta_text) = std::fs::read_to_string(&meta).ok() else {
                stale += 1;
                continue;
            };
            // 指纹存在但记录的大小与实际不符 → 损坏（下次构建自愈重编）。
            let size_ok = meta_text
                .lines()
                .nth(4)
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|recorded| {
                    std::fs::metadata(&p)
                        .map(|m| m.len() == recorded)
                        .unwrap_or(false)
                });
            if !size_ok {
                corrupt += 1;
            }
        }
    }
    if stale == 0 && corrupt == 0 {
        checks.push(Check::pass(
            "rt-cache-integrity",
            "rt_cache integrity",
            format!("{total} cached runtime objects, all fingerprinted"),
        ));
    } else {
        checks.push(Check::warn(
            "rt-cache-integrity",
            "rt_cache has stale/corrupt entries",
            format!("{stale} without fingerprint, {corrupt} size-mismatched (of {total})"),
            "entries are content-addressed and self-heal on next build; run `rm -rf <rt_cache>/*` to clean",
        ));
    }
}

/// 7. native DLL（crypto_native 必须；wgpu_native 可选按需）。
fn check_native_dlls(env: &EnvSnapshot, checks: &mut Vec<Check>) {
    if !env.host_triple.contains("windows") {
        checks.push(Check::pass("native-dll", "native DLLs", "n/a on this host"));
        return;
    }
    let crypto = env.rt_base.join("runtime-crypto/bin/windows");
    let crypto_dll = crypto.join("crypto_native.dll");
    let crypto_lib = crypto.join("crypto_native.lib");
    if crypto_dll.is_file() && crypto_lib.is_file() {
        checks.push(Check::pass(
            "native-dll",
            "crypto_native DLL",
            format!("{} (+ import lib)", crypto_dll.display()),
        ));
    } else {
        checks.push(Check::fail(
            "native-dll",
            "crypto_native DLL missing",
            format!("expected {} and {}", crypto_dll.display(), crypto_lib.display()),
            "restore the SDK package, or run scripts/fetch-boringssl-native.ps1 (vendored mbedTLS base)",
        ));
    }
    let wgpu_dll = env
        .rt_base
        .join("runtime-ui/wgpu-native/bin/windows/wgpu_native.dll");
    // 组件管理优先：`arc component install wgpu` 落点在 components/wgpu/<ver>/bin/windows。
    let component_wgpu_dll = crate::components::active_dir(crate::components::COMPONENT_WGPU)
        .map(|d| d.join("bin/windows/wgpu_native.dll"))
        .filter(|p| p.is_file());
    if let Some(dll) = &component_wgpu_dll {
        checks.push(Check::pass(
            "native-dll-wgpu",
            "wgpu_native DLL",
            format!("{} (arc component wgpu)", dll.display()),
        ));
    } else if wgpu_dll.is_file() {
        checks.push(Check::pass(
            "native-dll-wgpu",
            "wgpu_native DLL",
            format!("{}", wgpu_dll.display()),
        ));
    } else {
        checks.push(Check::warn(
            "native-dll-wgpu",
            "wgpu_native DLL missing",
            "optional component not present (soft-skip per e2e discipline)",
            "run `arc component install wgpu`, or scripts/fetch-wgpu-native.ps1 to vendor into the SDK",
        ));
    }
}

/// 8. 环境变量一览（信息性）。
fn check_env_vars(env: &EnvSnapshot, checks: &mut Vec<Check>) {
    let vars = [
        ARC_SDK_ROOT_ENV,
        ARC_STD_ROOT_ENV,
        ARC_HOME_ENV,
        ARC_CLANG_ENV,
    ];
    let detail = vars
        .iter()
        .map(|name| {
            let value = crate::env::env_var(name).unwrap_or_else(|| "<unset>".to_string());
            format!("{name}={value}")
        })
        .collect::<Vec<_>>()
        .join(" | ");
    // `env` 参数保持借用以避免未使用告警（快照已含各资源解析结果）。
    let _ = &env.host_triple;
    checks.push(Check::pass("env-vars", "environment variables", detail));
}
