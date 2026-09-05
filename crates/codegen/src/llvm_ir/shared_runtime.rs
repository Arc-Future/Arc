//! arc_runtime 共享运行时库构建（RFC 017 阶段一：runtime 单副本共享）。
//!
//! 阶段一裁决（RFC 017「跨库符号共享策略（混合式）」）：runtime 层全局单副本
//! 共享——插件 dll 不再内嵌 rt 机器码，改为导入引用 `arc_runtime`（对标 C#
//! `coreclr.dll` 的进程单实例形态）。本模块把 runtime C 源码集构建为单一共享
//! 库（Windows `.dll` + 导入库 / ELF `.so` / macOS `.dylib`）：
//!
//! - 对象集复用 [`super::prepare_runtime_objects`]（与 exe 链接共享内容寻址
//!   缓存），排除 UI wgpu / platform / ime 对象——与
//!   [`super::link_objects_to_dynamic_library`] 的插件排除集一致：wgpu_* 与
//!   platform ABI 符号由 host 进程提供，共享 runtime 不承载 UI 依赖；
//! - Windows MSVC：MSVC 链接器默认不导出任何符号，经 `llvm-nm` 收集链接输入
//!   中全部 defined 符号生成 `.def`（`/DEF:`）显式导出，并 `/IMPLIB:` 一步带
//!   出导入库，供插件 dll / 宿主 exe 链接引用；
//! - MinGW / ELF / macOS：链接器默认导出全局符号，无需符号清单。
//!
//! 数据符号（`rt_typeinfo_*`）永不跨映像静态引用——发射形态由发射器的
//! 函数化改造保证（`rt_typeinfo_prim` / `rt_box_vtable` 导出函数），本模块
//! 的全量导出面不依赖该约束。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::CodegenError;
use crate::llvm_ir::optimize;

/// 共享库文件名主干（`arc_runtime.dll` / `arc_runtime.so` / `arc_runtime.dylib`）。
const RUNTIME_LIB_STEM: &str = "arc_runtime";

/// 共享 runtime 库构建产物。
#[derive(Debug, Clone)]
pub(crate) struct SharedRuntimeArtifact {
    /// 共享库本体（`arc_runtime.dll` / `.so` / `.dylib`）。
    pub lib: PathBuf,
    /// Windows 导入库（`arc_runtime.lib`），供插件 dll / 宿主 exe 链接引用；
    /// 其余平台为 `None`（链接器直接以 `.so` / `.dylib` 为链接输入）。
    pub import_lib: Option<PathBuf>,
}

impl SharedRuntimeArtifact {
    /// 链接输入：MSVC 用导入库（`.lib`，链接器由此产生对 `arc_runtime.dll`
    /// 的导入表项），其余平台直接以共享库本体为输入（链接器记录 SONAME /
    /// install_name 引用）。
    pub(crate) fn link_input(&self) -> PathBuf {
        self.import_lib.clone().unwrap_or_else(|| self.lib.clone())
    }
}

/// 引用方（宿主 exe / 插件 dll）的运行期库定位标志。
///
/// Windows 无需标志——dll 与 exe 同目录属默认依赖搜索路径；ELF 以
/// `$ORIGIN`、macOS 以 `@executable_path` 把产物同目录副本纳入加载搜索。
pub(crate) fn consumer_rpath_flags(target: Option<&str>) -> Vec<String> {
    let is_windows = target
        .map(|t| t.contains("windows"))
        .unwrap_or(cfg!(target_os = "windows"));
    if is_windows {
        return Vec::new();
    }
    let is_darwin = target
        .map(|t| t.contains("darwin"))
        .unwrap_or(cfg!(target_os = "macos"));
    if is_darwin {
        vec!["-Wl,-rpath,@executable_path".into()]
    } else {
        vec!["-Wl,-rpath,$ORIGIN".into()]
    }
}

/// 链接成功后把共享库落位到产物同目录（best-effort：ARC_HOME 缓存 + 硬链接，
/// 与 vendored dll 同机制——多产物目录引用同一物理副本）。
pub(crate) fn stage_shared_runtime(output: &Path, artifact: &SharedRuntimeArtifact) {
    if let (Some(parent), Some(file_name)) = (output.parent(), artifact.lib.file_name()) {
        super::stage_vendored_dll(&artifact.lib, &parent.join(file_name));
    }
}

/// 构建 arc_runtime 共享库（内容寻址缓存命中时仅重链接）。
///
/// - `rt_base`：runtime 源码根（`sdk_layout::sdk_runtime_base()`）；
/// - `work_dir`：工作目录（runtime `.o` 缓存与 `.def` / 产物落点）。
pub(crate) fn build_shared_runtime(
    rt_base: &Path,
    clang: &str,
    work_dir: &Path,
    target: Option<&str>,
    level: optimize::OptLevel,
    debug_info: bool,
) -> Result<SharedRuntimeArtifact, CodegenError> {
    fs::create_dir_all(work_dir)
        .map_err(|e| CodegenError::Llvm(format!("create work dir failed: {e}")))?;

    let runtime_objs =
        super::prepare_runtime_objects(rt_base, clang, work_dir, level, target, debug_info)?;

    // 排除集与 `link_objects_to_dynamic_library` 一致：host 进程才提供
    // wgpu / platform / ime 符号，共享 runtime 不承载 UI 依赖。
    let link_objs: Vec<&Path> = runtime_objs
        .iter()
        .map(|p| p.as_path())
        .filter(|obj| {
            let name = obj.file_name().and_then(|s| s.to_str()).unwrap_or("");
            !(name == "rt_wgpu_native.o"
                || super::is_platform_runtime_object(name)
                || super::is_ui_ime_runtime_object(name))
        })
        .collect();

    let is_msvc = target
        .map(|t| t.contains("msvc"))
        .unwrap_or(cfg!(target_os = "windows"));

    // 系统库注入与 `link_objects_to_dynamic_library` 一致（rt_env_user_name →
    // advapi32、rt_reactor_iocp → ws2_32 等），否则共享库独立链接缺 CRT 外
    // 系统符号（冒烟教训：GetUserNameA undefined）。
    let mut extra_flags: Vec<String> = super::mangle::platform_link_flags(target)
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut import_lib = None;
    if is_msvc {
        let def_path = work_dir.join(format!("{RUNTIME_LIB_STEM}.def"));
        let symbols = collect_defined_symbols(&link_objs)?;
        write_def_file(&def_path, &symbols)?;
        let implib_path = work_dir.join(format!("{RUNTIME_LIB_STEM}.lib"));
        extra_flags.push(format!("-Wl,/DEF:{}", def_path.display()));
        extra_flags.push(format!("-Wl,/IMPLIB:{}", implib_path.display()));
        import_lib = Some(implib_path);
    } else if target
        .map(|t| t.contains("windows"))
        .unwrap_or(cfg!(target_os = "windows"))
    {
        // MinGW：PE 目标无 SONAME/install_name 语义，链接器默认导出即用。
    } else {
        // 固定 SONAME / install_name，使依赖按库文件名（而非构建期绝对路径）
        // 解析；运行期由引用方 rpath（consumer_rpath_flags）定位产物同目录
        // 副本，宿主与插件因此命中同一已加载映像（进程单实例）。
        let is_darwin = target
            .map(|t| t.contains("darwin"))
            .unwrap_or(cfg!(target_os = "macos"));
        if is_darwin {
            extra_flags.push(format!("-Wl,-install_name,@rpath/{RUNTIME_LIB_STEM}.dylib"));
        } else {
            extra_flags.push(format!("-Wl,-soname,{RUNTIME_LIB_STEM}.so"));
        }
    }

    let lib_path = shared_lib_path(work_dir, target);
    let flag_refs: Vec<&str> = extra_flags.iter().map(|s| s.as_str()).collect();
    let status = optimize::clang_link_shared(clang, &link_objs, &lib_path, target, &flag_refs, &[])
        .status()
        .map_err(|e| CodegenError::Llvm(format!("shared link failed: {e}")))?;
    if !status.success() {
        return Err(CodegenError::Llvm("shared runtime link failed".into()));
    }

    Ok(SharedRuntimeArtifact {
        lib: lib_path,
        import_lib,
    })
}

/// 共享库输出路径（按 target 三元组选扩展名，host 回退按编译期平台）。
fn shared_lib_path(work_dir: &Path, target: Option<&str>) -> PathBuf {
    let ext = if target
        .map(|t| t.contains("msvc") || t.contains("windows-gnu"))
        .unwrap_or(cfg!(target_os = "windows"))
    {
        "dll"
    } else if target
        .map(|t| t.contains("darwin"))
        .unwrap_or(cfg!(target_os = "macos"))
    {
        "dylib"
    } else {
        "so"
    };
    work_dir.join(format!("{RUNTIME_LIB_STEM}.{ext}"))
}

/// 收集链接输入中全部 defined 符号（llvm-nm 单次调用，BTreeSet 去重）。
///
/// 仅保留外部可见符号（符号类型大写：text/data/bss/rodata/weak/absolute）；
/// `static` 内部符号（小写类型）与段符号（`.` 开头）不导出。
fn collect_defined_symbols(objs: &[&Path]) -> Result<BTreeSet<String>, CodegenError> {
    if objs.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut cmd = Command::new(llvm_nm_path());
    cmd.arg("--defined-only");
    for obj in objs {
        cmd.arg(obj.to_str().unwrap_or_default());
    }
    let output = cmd
        .output()
        .map_err(|e| CodegenError::Llvm(format!("llvm-nm not found: {e}")))?;
    if !output.status.success() {
        return Err(CodegenError::Llvm(format!(
            "llvm-nm failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut symbols = BTreeSet::new();
    for line in stdout.lines() {
        let line = line.trim_end();
        // "path/to/obj.o:" 文件段落头——多输入模式下的分隔行。
        if line.ends_with(':') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        // 常见输出："0000000000000000 T symbol"；无地址变体："T symbol"。
        let (ty, name) = match parts.as_slice() {
            [addr, ty, name] if addr.chars().all(|c| c.is_ascii_hexdigit()) => (*ty, *name),
            [ty, name] => (*ty, *name),
            _ => continue,
        };
        let first = ty.chars().next().unwrap_or('\0');
        // 合法 C 标识符不含 `@`——凡含之者皆为编译器/链接器内部符号
        //（MSVC mangled 字符串字面量 `??_C@...`、浮点常量 `__real@...`、
        // 特性标记 `@feat.00`），模块映像自足，不入导出面。
        if first.is_ascii_uppercase() && !name.starts_with('.') && !name.contains('@') {
            symbols.insert(name.to_string());
        }
    }
    Ok(symbols)
}

/// 写出 MSVC 模块定义文件（`LIBRARY` 行省略——模块名由 dll 文件名决定）。
fn write_def_file(path: &Path, symbols: &BTreeSet<String>) -> Result<(), CodegenError> {
    let mut body = String::from("EXPORTS\n");
    for sym in symbols {
        body.push_str(&format!("    {sym}\n"));
    }
    fs::write(path, body).map_err(|e| CodegenError::Llvm(format!("write def failed: {e}")))
}

/// Resolve the `llvm-nm` binary path（与 `arcdbg::llvm_nm_path` 同序探测）。
fn llvm_nm_path() -> String {
    if cfg!(windows) {
        for p in [
            r"C:\Program Files\LLVM\bin\llvm-nm.exe",
            r"C:\Program Files (x86)\LLVM\bin\llvm-nm.exe",
        ] {
            if Path::new(p).exists() {
                return p.into();
            }
        }
    }
    "llvm-nm".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_lib_path_by_target() {
        let wd = Path::new("wd");
        assert_eq!(
            shared_lib_path(wd, Some("x86_64-pc-windows-msvc")),
            wd.join("arc_runtime.dll")
        );
        assert_eq!(
            shared_lib_path(wd, Some("aarch64-apple-darwin")),
            wd.join("arc_runtime.dylib")
        );
        assert_eq!(
            shared_lib_path(wd, Some("x86_64-unknown-linux-gnu")),
            wd.join("arc_runtime.so")
        );
    }

    #[test]
    fn def_file_lists_sorted_symbols() {
        let mut syms = BTreeSet::new();
        syms.insert("rt_box_create".to_string());
        syms.insert("rt_env_init".to_string());
        let path = std::env::temp_dir().join("arc_test_shared_runtime.def");
        write_def_file(&path, &syms).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(text, "EXPORTS\n    rt_box_create\n    rt_env_init\n");
    }

    /// 工具链冒烟（RFC 017 阶段一验收前置）：真实构建 arc_runtime 共享库，
    /// 断言产物、导入库与导出/排除面。首次运行全量编译 runtime `.o`
    /// （约 1-2 分钟），显式 `--ignored` 触发：
    /// `cargo test -p codegen --lib shared_runtime -- --ignored`
    #[test]
    #[ignore = "需本机 clang / llvm-nm；首次全量编译 runtime .o 约需 1-2 分钟"]
    fn builds_shared_runtime_and_exports_symbols() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let rt_base = manifest.parent().expect("crate parent").to_path_buf();
        let work_dir = manifest
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .join("target")
            .join("shared-runtime-smoke");

        let artifact = build_shared_runtime(
            &rt_base,
            &crate::llvm_ir::mangle::clang_path(),
            &work_dir,
            None,
            optimize::OptLevel::Debug,
            false,
        )
        .expect("shared runtime build");
        assert!(
            artifact.lib.exists(),
            "missing shared lib: {}",
            artifact.lib.display()
        );

        if cfg!(target_os = "windows") {
            let implib = artifact
                .import_lib
                .as_ref()
                .expect("msvc host build must emit import lib");
            assert!(implib.exists(), "missing import lib: {}", implib.display());
        }

        let def_text =
            fs::read_to_string(work_dir.join(format!("{RUNTIME_LIB_STEM}.def"))).expect("def file");
        assert!(
            def_text.lines().any(|l| l.trim() == "rt_type_init"),
            "def must export core rt symbols"
        );
        let ui_exports: Vec<&str> = def_text
            .lines()
            .filter_map(|l| l.trim().strip_prefix("rt_wgpu_").map(|_| l.trim()))
            .chain(
                def_text
                    .lines()
                    .filter_map(|l| l.trim().strip_prefix("rt_ui_ime_").map(|_| l.trim())),
            )
            .collect();
        assert!(
            ui_exports.is_empty(),
            "wgpu/ime symbols must stay out of shared runtime: {ui_exports:?}"
        );
    }
}
