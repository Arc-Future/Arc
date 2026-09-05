//! Optimization level selection and clang invocation flags (RFC 015 Phase B).
//!
//! Centralizes release/debug optimization policy and all clang `Command`
//! construction so `mod.rs` stays focused on orchestration. Release builds
//! use `-O3` (not `-O2`) to match the project goal of rivaling Rust/C++;
//! runtime C sources are compiled with the same optimization level as the
//! generated IR so the runtime is not a hot-path bottleneck.
//!
//! Each builder takes the clang binary path (+ optional target triple) and
//! returns a fresh `Command` — no shared mutable state, no clone pitfalls.
//!
//! Dead code elimination (RFC 031 §7): `clang_compile` emits each
//! function/data into its own section (`-ffunction-sections -fdata-sections`),
//! and `clang_link` passes platform-specific section GC flags so the linker
//! discards unreferenced sections. Hello-style programs no longer carry
//! unused runtime symbols (e.g. `rt_tensor_*`, `rt_list_*`).
//!
//! Future work: add `-march=native`, `-flto=thin`, and per-function FastMath
//! flags once the MIR carries the necessary purity annotations.

use std::path::Path;
use std::process::Command;

use super::mangle;

/// Optimization level passed to clang.
#[derive(Clone, Copy, Debug)]
pub enum OptLevel {
    Debug,
    Release,
}

impl OptLevel {
    /// clang flag for compiling IR / C sources.
    pub fn flag(&self) -> &'static str {
        match self {
            OptLevel::Debug => "-O0",
            OptLevel::Release => "-O3",
        }
    }
}

fn apply_target(cmd: &mut Command, target: Option<&str>) {
    if let Some(triple) = target {
        cmd.arg(format!("--target={triple}"));
    }
}

/// Linker flags for dead code elimination (RFC 031 §7 section GC).
///
/// Returns platform-specific linker flags that discard unreferenced
/// function/data sections from the final binary. Must be paired with
/// `-ffunction-sections -fdata-sections` at compile time (see `clang_compile`).
///
/// - macOS (`darwin`/`macos`): `-Wl,-dead_strip` (ld64)
/// - Windows MSVC (`windows-msvc`): `-Wl,/OPT:REF` (lld-link)
/// - Windows MinGW / Linux / OHos / other ELF: `-Wl,--gc-sections` (lld / ld.bfd)
fn linker_gc_flags(target: Option<&str>) -> &'static [&'static str] {
    let triple = target.unwrap_or("");
    if mangle::is_wasm_triple(triple) {
        return &[];
    }
    if triple.contains("darwin") || triple.contains("macos") {
        &["-Wl,-dead_strip"]
    } else if triple.contains("windows-msvc") {
        // lld-link (MSVC driver) uses /OPT:REF instead of --gc-sections.
        // -ffunction-sections -fdata-sections map to /Gy /Gw at the MSVC level.
        &["-Wl,/OPT:REF"]
    } else {
        // Windows MinGW, Linux, OHos, and other ELF targets use --gc-sections.
        &["-Wl,--gc-sections"]
    }
}

/// Reproducible build flags (RFC 036 §2 determinism-first): zero the PE header
/// `TimeDateStamp` so identical inputs yield byte-identical binaries.
///
/// - Windows MSVC: `-Wl,/Brepro` (accepted by both `lld-link` and `link.exe`)
/// - Other targets: no PE timestamp, nothing to zero here
fn linker_reproducible_flags(target: Option<&str>) -> &'static [&'static str] {
    if mangle::is_windows_target(target) {
        &["-Wl,/Brepro"]
    } else {
        &[]
    }
}

/// 诊断辅助：`ARC_SANITIZE` 环境变量（如 `=address`）为非空时返回
/// `-fsanitize=<val>`，供编译与链接阶段注入 sanitizer（仅用于崩溃定位，
/// 非生产路径）。空值返回 None。
pub(crate) fn sanitize_flag() -> Option<String> {
    let v = std::env::var("ARC_SANITIZE").ok()?;
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    Some(format!("-fsanitize={v}"))
}

/// Build a clang command for compiling a single source to an object file.
///
/// When `debug_info` is true, `-g` is passed so clang embeds DWARF 5 debug
/// sections in the object file (RFC 031 §2 / RFC 020 Phase B.2).
///
/// `-ffunction-sections -fdata-sections` (RFC 031 §7) emits each function
/// and global into its own section so the linker can discard unreferenced
/// ones via `--gc-sections` / `-dead_strip` (see `clang_link`).
pub fn clang_compile(
    clang_path: &str,
    src: &Path,
    obj: &Path,
    level: OptLevel,
    target: Option<&str>,
    debug_info: bool,
) -> Command {
    let mut cmd = Command::new(clang_path);
    apply_target(&mut cmd, target);
    cmd.args([
        level.flag(),
        "-c",
        src.to_str().unwrap(),
        "-o",
        obj.to_str().unwrap(),
    ]);
    // RFC 031 §7: per-function/per-data sections enable linker GC.
    cmd.args(["-ffunction-sections", "-fdata-sections"]);
    // RFC 020 Phase C: Release 启用 ThinLTO + native arch 优化。
    // ThinLTO 在编译阶段生成 LLVM bitcode summary，链接阶段由 lld 跨 TU 内联。
    // -march=native 启用 host CPU 全部 SIMD 指令集（AVX2/AVX-512/NEON）。
    if matches!(level, OptLevel::Release) && !target.map(mangle::is_wasm_triple).unwrap_or(false) {
        cmd.args(["-flto=thin", "-march=native"]);
    }
    if debug_info {
        cmd.arg("-g");
        cmd.args(["-gdwarf-5"]);
    }
    if let Some(sflag) = sanitize_flag() {
        cmd.arg(&sflag);
    }
    cmd
}

/// Build a clang command for linking object files into an executable.
///
/// Platform-specific section GC flags (RFC 031 §7) are inserted before
/// `extra_flags` (library flags) so the linker resolves references introduced
/// by libraries after enabling dead code elimination.
pub fn clang_link(
    clang_path: &str,
    objs: &[&Path],
    output: &Path,
    target: Option<&str>,
    level: OptLevel,
    extra_flags: &[&str],
) -> Command {
    let mut cmd = Command::new(clang_path);
    apply_target(&mut cmd, target);
    for o in objs {
        cmd.arg(o.to_str().unwrap());
    }
    // RFC 031 §7: linker-level dead code elimination. Placed before
    // library flags so GC runs after all object inputs are visible but before
    // library resolution pulls in additional symbols.
    for flag in linker_gc_flags(target) {
        cmd.arg(flag);
    }
    for flag in linker_reproducible_flags(target) {
        cmd.arg(flag);
    }
    if target.map(mangle::is_wasm_triple).unwrap_or(false) {
        cmd.args(["-nostdlib", "-Wl,--export=main", "-Wl,--no-entry"]);
    }
    if matches!(level, OptLevel::Release) && !target.map(mangle::is_wasm_triple).unwrap_or(false) {
        cmd.arg("-flto=thin");
        // ThinLTO 链接器按**目标**选择（S1 平台审计 #4/#6：不可按宿主 cfg 决策）：
        // - Windows MSVC → lld-link；Windows GNU/MinGW → lld（COFF GNU 口味）
        // - ELF 系（Linux / OHOS）→ lld（`ld.lld`）
        // - macOS → 不注入：Apple clang 无 lld，`-fuse-ld=lld` 注入即 Release 必败；
        //   系统 ld64 原生支持 thin LTO，走工具链默认链接器
        // - Host（未指定 target）→ 按宿主 OS 套同规则
        let windows_linker = |t: Option<&str>| {
            if t.is_none_or(|x| x.contains("msvc")) {
                "-fuse-ld=lld-link"
            } else {
                "-fuse-ld=lld"
            }
        };
        match mangle::target_os(target) {
            mangle::TargetOs::Windows => {
                cmd.arg(windows_linker(target));
            }
            mangle::TargetOs::Linux | mangle::TargetOs::Ohos => {
                cmd.arg("-fuse-ld=lld");
            }
            mangle::TargetOs::Macos => {}
            mangle::TargetOs::Host => {
                if cfg!(windows) {
                    cmd.arg(windows_linker(target));
                } else if cfg!(target_os = "linux") {
                    cmd.arg("-fuse-ld=lld");
                }
            }
            mangle::TargetOs::WebAssembly | mangle::TargetOs::Wasi => {}
        }
    }
    for flag in extra_flags {
        cmd.arg(flag);
    }
    if let Some(sflag) = sanitize_flag() {
        cmd.arg(&sflag);
    }
    // 取证基建：`ARC_DEBUG_LINK_SYMBOLS=1` 时产出完整符号表/PDB（lld-link
    // /DEBUG:FULL），供 VEH 崩溃探针的 rip 模块偏移做函数级符号化。仅诊断，
    // 非生产路径（同 ARC_SANITIZE 先例）。
    if std::env::var("ARC_DEBUG_LINK_SYMBOLS").as_deref() == Ok("1") {
        cmd.arg("-Wl,/DEBUG:FULL");
    }
    cmd.arg("-o").arg(output);
    cmd
}

/// Build a clang command for linking object files into a shared dynamic library.
///
/// RFC 017：动态库链接入口——对齐 C# 程序集模型，动态库 = 干净的
/// 库逻辑 + 引用链接信息。产物在 Windows 是 `.dll`，Linux 是 `.so`，
/// macOS 是 `.dylib`。
///
/// # 跨平台链接标志
///
/// - Linux / OHos / 其他 ELF：`-shared` + `-fPIC`
/// - macOS：`-shared` + `-fPIC`（`-dynamiclib` 也可，但 `-shared` 更通用）
/// - Windows MSVC：`-shared`（clang 翻译为 lld-link `/DLL`）+ 不需要 `-fPIC`
/// - Windows MinGW：`-shared` + 不需要 `-fPIC`
///
/// # 导出符号
///
/// `export_symbols` 列出必须被 host 通过 `rt_library_sym` 查找到的领域约定符号
/// （如 QIF 的 `__qif_init`，见 RFC 017）。
///
/// - Linux/macOS：默认所有全局符号导出（visibility default），无需额外标志；
///   `export_symbols` 仅用于诊断验证（v1.0 不强制）
/// - Windows MSVC：默认不导出任何符号，必须显式 `/EXPORT:<symbol>`；
///   `export_symbols` 转换为 `/EXPORT:` 标志
/// - Windows MinGW：默认导出所有全局符号（`--export-all-symbols`），
///   `export_symbols` 仅用于诊断验证
///
/// # 与 `clang_link` 的差异
///
/// - 不应用 section GC（`--gc-sections` / `-dead_strip`）：动态库的所有导出
///   符号必须保留，section GC 可能错误裁剪被 host 期望的符号
/// - 不要求 `main` 函数（动态库无入口点，领域约定符号由 host 按需查找）
pub fn clang_link_shared(
    clang_path: &str,
    objs: &[&Path],
    output: &Path,
    target: Option<&str>,
    extra_flags: &[&str],
    export_symbols: &[&str],
) -> Command {
    let mut cmd = Command::new(clang_path);
    apply_target(&mut cmd, target);

    // RFC 017: -shared 是动态库的核心标志。
    // clang 在所有平台都支持 -shared，内部翻译为对应链接器的 DLL/动态库标志。
    cmd.arg("-shared");

    // -fPIC 在 ELF 与 macOS 上必需（位置无关代码），Windows 上无害但不需要。
    // 简化策略：除 Windows MSVC 外都加 -fPIC。
    let triple = target.unwrap_or("");
    let is_windows_msvc = triple.contains("windows-msvc");
    if !is_windows_msvc {
        cmd.arg("-fPIC");
    }

    for o in objs {
        cmd.arg(o.to_str().unwrap());
    }

    // Windows MSVC: 显式导出每个领域约定符号。
    // Linux/macOS/MinGW: 默认导出全局符号，无需额外标志。
    if is_windows_msvc {
        for sym in export_symbols {
            cmd.arg(format!("-Wl,/EXPORT:{sym}"));
        }
    }

    for flag in linker_reproducible_flags(target) {
        cmd.arg(flag);
    }

    for flag in extra_flags {
        cmd.arg(flag);
    }
    // 取证基建（同 clang_link）：`ARC_DEBUG_LINK_SYMBOLS=1` 时产出 dll 符号/PDB。
    if std::env::var("ARC_DEBUG_LINK_SYMBOLS").as_deref() == Ok("1") {
        cmd.arg("-Wl,/DEBUG:FULL");
    }
    cmd.arg("-o").arg(output);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn release_compile_includes_lto_and_native() {
        let cmd = clang_compile(
            "clang",
            Path::new("test.ll"),
            Path::new("test.o"),
            OptLevel::Release,
            None,
            false,
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(
            args.contains(&"-flto=thin".to_string()),
            "Release compile missing -flto=thin: {:?}",
            args
        );
        assert!(
            args.contains(&"-march=native".to_string()),
            "Release compile missing -march=native: {:?}",
            args
        );
        assert!(
            args.contains(&"-O3".to_string()),
            "Release compile missing -O3"
        );
    }

    #[test]
    fn debug_compile_omits_lto_and_native() {
        let cmd = clang_compile(
            "clang",
            Path::new("test.ll"),
            Path::new("test.o"),
            OptLevel::Debug,
            None,
            false,
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(
            !args.contains(&"-flto=thin".to_string()),
            "Debug compile should not have -flto=thin: {:?}",
            args
        );
        assert!(
            !args.contains(&"-march=native".to_string()),
            "Debug compile should not have -march=native: {:?}",
            args
        );
        assert!(
            args.contains(&"-O0".to_string()),
            "Debug compile missing -O0"
        );
    }

    #[test]
    fn release_link_includes_lto_and_fuse_ld() {
        let cmd = clang_link(
            "clang",
            &[Path::new("a.o")],
            Path::new("out"),
            None,
            OptLevel::Release,
            &[],
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(
            args.contains(&"-flto=thin".to_string()),
            "Release link missing -flto=thin: {:?}",
            args
        );
        // ThinLTO requires lld linker; verify -fuse-ld is set appropriately
        #[cfg(target_os = "windows")]
        let expected_fuse_ld = "lld-link";
        #[cfg(not(target_os = "windows"))]
        let expected_fuse_ld = "lld";
        assert!(
            args.contains(&format!("-fuse-ld={expected_fuse_ld}")),
            "Release link missing -fuse-ld={expected_fuse_ld}: {:?}",
            args
        );
    }

    #[test]
    fn release_linker_follows_target_not_host() {
        let args_of = |target: Option<&str>| -> Vec<String> {
            let cmd = clang_link(
                "clang",
                &[Path::new("a.o")],
                Path::new("out"),
                target,
                OptLevel::Release,
                &[],
            );
            cmd.get_args()
                .map(|a| a.to_string_lossy().to_string())
                .collect()
        };
        // Linux / OHOS（ELF）目标 → lld（与宿主无关）。
        let linux = args_of(Some("x86_64-unknown-linux-gnu"));
        assert!(
            linux.contains(&"-fuse-ld=lld".to_string()),
            "linux target missing -fuse-ld=lld: {linux:?}"
        );
        // macOS 目标 → 不注入（Apple clang 无 lld；系统 ld64 原生 thin LTO）。
        let mac = args_of(Some("x86_64-apple-darwin"));
        assert!(
            !mac.iter().any(|a| a.starts_with("-fuse-ld=")),
            "macOS target must not force -fuse-ld: {mac:?}"
        );
        // Windows MSVC → lld-link。
        let msvc = args_of(Some("x86_64-pc-windows-msvc"));
        assert!(
            msvc.contains(&"-fuse-ld=lld-link".to_string()),
            "msvc target missing -fuse-ld=lld-link: {msvc:?}"
        );
        // Windows GNU/MinGW → lld（COFF GNU 口味）。
        let gnu = args_of(Some("x86_64-pc-windows-gnu"));
        assert!(
            gnu.contains(&"-fuse-ld=lld".to_string()),
            "windows-gnu target missing -fuse-ld=lld: {gnu:?}"
        );
    }

    #[test]
    fn debug_link_omits_lto_and_fuse_ld() {
        let cmd = clang_link(
            "clang",
            &[Path::new("a.o")],
            Path::new("out"),
            None,
            OptLevel::Debug,
            &[],
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(
            !args.contains(&"-flto=thin".to_string()),
            "Debug link should not have -flto=thin: {:?}",
            args
        );
        // Debug builds should NOT include -fuse-ld (use default linker)
        let has_fuse_ld = args.iter().any(|a| a.starts_with("-fuse-ld="));
        assert!(
            !has_fuse_ld,
            "Debug link should not have -fuse-ld: {:?}",
            args
        );
    }

    #[test]
    fn link_includes_brepro_for_windows_msvc() {
        let cmd = clang_link(
            "clang",
            &[Path::new("a.o")],
            Path::new("out"),
            Some("x86_64-pc-windows-msvc"),
            OptLevel::Debug,
            &[],
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(
            args.contains(&"-Wl,/Brepro".to_string()),
            "Windows MSVC link missing -Wl,/Brepro: {:?}",
            args
        );
    }

    #[test]
    fn link_omits_brepro_for_elf() {
        let cmd = clang_link(
            "clang",
            &[Path::new("a.o")],
            Path::new("out"),
            Some("x86_64-unknown-linux-gnu"),
            OptLevel::Debug,
            &[],
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(
            !args.contains(&"-Wl,/Brepro".to_string()),
            "ELF link should not include -Wl,/Brepro: {:?}",
            args
        );
    }
}
