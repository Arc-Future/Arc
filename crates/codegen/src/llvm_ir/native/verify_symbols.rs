//! 编译期符号验证（RFC 016 M2）。
//!
//! 调用平台工具（`llvm-nm`/`nm`/`dumpbin`）扫描目标库的已定义符号表，
//! 校验 `NativeModule` 声明的所有符号确实存在。符号缺失即编译错误。
//!
//! 平台工具优先级（RFC §4.3.3 + §12.1 决议）：
//! - Linux/OHos/macOS/Windows MinGW：优先 `llvm-nm`（随 LLVM 22 分发），fallback `nm`
//! - Windows MSVC：优先 `dumpbin`（VS 自带），fallback `llvm-nm`
//!
//! 风险缓解（RFC §9）：所有工具不可用时退化为 Phase 1 行为（warning），
//! 不阻断编译——返回 `Ok(())` 并由调用方另行发出 warning。

use ast::{LoadStrategy, NativeModule};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 符号验证错误。
#[derive(Debug, Clone)]
pub enum SymbolVerifyError {
    /// 符号在目标库中未找到。
    Missing {
        module: String,
        symbol: String,
        lib: String,
    },
    /// 工具扫描失败（命令执行错误、退出码非 0 等）。
    ScanFailed { lib: String, detail: String },
}

impl SymbolVerifyError {
    /// 人类可读的错误描述（用于 CodegenError 与诊断信息）。
    pub fn display(&self) -> String {
        match self {
            Self::Missing {
                module,
                symbol,
                lib,
            } => format!("native symbol `{module}.{symbol}` not found in library `{lib}`"),
            Self::ScanFailed { lib, detail } => format!("failed to scan library `{lib}`: {detail}"),
        }
    }
}

/// 平台符号扫描工具。
pub enum SymbolTool {
    /// LLVM 22 分发的 `llvm-nm`，覆盖所有平台。
    LlvmNm(String),
    /// 系统 `nm`，Unix 系常见。
    Nm(String),
    /// MSVC 工具链的 `dumpbin`。
    Dumpbin(String),
}

impl SymbolTool {
    /// 构造扫描命令。Unix 系使用 `-D --defined-only`（仅动态符号中的已定义项）；
    /// macOS 使用 `-gU`（全局+未定义排除，因 `-D` 在 macOS 不支持）。
    /// Windows MSVC 使用 `dumpbin /symbols`。
    fn build_cmd(&self, lib_path: &Path) -> Command {
        let mut cmd = match self {
            Self::LlvmNm(p) | Self::Nm(p) => {
                let mut c = Command::new(p);
                if cfg!(target_os = "macos") {
                    c.args(["-gU"]);
                } else {
                    c.args(["-D", "--defined-only"]);
                }
                c.arg(lib_path);
                c
            }
            Self::Dumpbin(p) => {
                let mut c = Command::new(p);
                c.args(["/symbols"]).arg(lib_path);
                c
            }
        };
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd
    }

    /// 是否为 dumpbin（用于输出解析分支）。
    fn is_dumpbin(&self) -> bool {
        matches!(self, Self::Dumpbin(_))
    }
}

/// 探测可用的符号扫描工具。返回 `None` 表示无可用工具（RFC §9：降级为 warning）。
///
/// 探测顺序：Windows MSVC → dumpbin 优先；其他平台 → llvm-nm 优先；
/// Unix 兜底 → nm。`<tool> --version` 退出码 0 视为可用。
pub fn detect_symbol_tool() -> Option<SymbolTool> {
    if cfg!(windows) {
        if let Some(p) = probe_command("dumpbin") {
            return Some(SymbolTool::Dumpbin(p));
        }
    }
    if let Some(p) = probe_command("llvm-nm") {
        return Some(SymbolTool::LlvmNm(p));
    }
    if !cfg!(windows) {
        if let Some(p) = probe_command("nm") {
            return Some(SymbolTool::Nm(p));
        }
    }
    None
}

/// 测试命令是否可执行（运行 `<tool> --version` 检查退出码）。
fn probe_command(tool: &str) -> Option<String> {
    let status = Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Some(tool.to_string()),
        _ => None,
    }
}

/// 扫描目标库的已定义符号集合。
pub fn scan_library_symbols(
    lib_path: &Path,
    tool: &SymbolTool,
) -> Result<HashSet<String>, SymbolVerifyError> {
    let output = tool
        .build_cmd(lib_path)
        .output()
        .map_err(|e| SymbolVerifyError::ScanFailed {
            lib: lib_path.display().to_string(),
            detail: format!("spawn failed: {e}"),
        })?;
    if !output.status.success() {
        return Err(SymbolVerifyError::ScanFailed {
            lib: lib_path.display().to_string(),
            detail: format!(
                "exit code {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_symbols(&stdout, tool.is_dumpbin()))
}

/// 解析工具输出的符号名。
///
/// - nm/llvm-nm：`<addr> <type> <name>` 三列格式
/// - dumpbin：`<...> | <name>` 管道分隔，取右侧
fn parse_symbols(stdout: &str, is_dumpbin: bool) -> HashSet<String> {
    let mut syms = HashSet::new();
    for line in stdout.lines() {
        if is_dumpbin {
            if let Some(name) = line.split('|').nth(1) {
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    syms.insert(trimmed.to_string());
                }
            }
        } else {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                syms.insert(parts[2].to_string());
            } else if parts.len() == 2 {
                syms.insert(parts[1].to_string());
            }
        }
    }
    syms
}

/// 验证 `NativeModule` 声明的所有符号在目标库中存在。
///
/// M2 仅校验符号存在性，ABI 兼容性校验（参数数量/类型）留待 Phase 3。
/// 工具不可用时由调用方另行处理（RFC §9 降级为 warning，不在此返回错误）。
///
/// 调用方应在调用前用 `detect_symbol_tool()` 探测工具，`None` 时跳过验证。
pub fn verify_native_symbols(
    module: &NativeModule,
    lib_path: &Path,
    tool: &SymbolTool,
) -> Result<(), Vec<SymbolVerifyError>> {
    let symbols = scan_library_symbols(lib_path, tool).map_err(|e| vec![e])?;
    let lib_str = lib_path.display().to_string();
    let mut errors = Vec::new();
    for fn_decl in &module.functions {
        let sym_name = fn_decl.symbol.as_ref().unwrap_or(&fn_decl.name);
        if !symbols.contains(sym_name.as_str()) {
            errors.push(SymbolVerifyError::Missing {
                module: module.name.to_string(),
                symbol: sym_name.to_string(),
                lib: lib_str.clone(),
            });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// 在 `lib_paths` 中解析 native 模块对应的库文件路径（RFC 016 M2）。
///
/// 平台命名约定：
/// - Windows MSVC：`<module>.lib`
/// - Windows MinGW：`lib<module>.dll.a`（import lib）或 `lib<module>.a`（static lib）
/// - Linux：`lib<module>.so` 或 `lib<module>.a`
/// - macOS：`lib<module>.dylib` 或 `lib<module>.a`
///
/// 返回第一个找到的库文件路径。找不到返回 `None`（调用方跳过验证）。
/// `libc` 等隐式链接的模块应由调用方提前过滤，不进入此函数。
pub fn resolve_native_lib(module_name: &str, lib_paths: &[PathBuf]) -> Option<PathBuf> {
    let candidates: Vec<String> = if cfg!(windows) {
        // 同时覆盖 MSVC `.lib` 与 MinGW `lib<name>.dll.a` / `lib<name>.a`。
        // wgpu-native vendoring 提供的是 MinGW import lib `libwgpu_native.dll.a`。
        vec![
            format!("{module_name}.lib"),
            format!("lib{module_name}.lib"),
            format!("lib{module_name}.dll.a"),
            format!("lib{module_name}.a"),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            format!("lib{module_name}.dylib"),
            format!("lib{module_name}.a"),
        ]
    } else {
        vec![
            format!("lib{module_name}.so"),
            format!("lib{module_name}.a"),
        ]
    };
    for path in lib_paths {
        for cand in &candidates {
            let full = path.join(cand);
            if full.exists() {
                return Some(full);
            }
        }
    }
    None
}

/// 构造模块的库搜索路径列表（RFC 016 M4 多库体系隔离）。
///
/// per-module 契约内 `library` 目录（最高优先）在前，全局 `lib_paths`（
/// `ani-native-lib`）在后。符号验证按此顺序解析模块库文件。
pub(crate) fn module_lib_search_paths(
    module: &NativeModule,
    lib_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = &module.library {
        out.push(dir.clone());
    }
    out.extend(lib_paths.iter().cloned());
    out
}

/// RFC 016：计算每个 native 模块的**生效**加载策略（`load` 统一模型）。
///
/// - `Static`（缺省/显式）：编译期符号验证 + 静态链接（零行为变更）。
/// - `Runtime`（显式）：运行时懒加载——编译期**跳过**符号验证与静态链接。
/// - `Auto`：编译期可定位（`resolve_native_lib` + `verify_native_symbols` 成功）
///   → static；否则（库无法定位 / 符号缺失）→ runtime 降级。
///
/// 隐式链接模块（`libc`）与符号内联在 runtime `.o` 的模块（`wgpu_native` /
/// `rt_resources` / `rt_process`）永远视为 static——auto 对它们无意义。
/// 符号工具不可用时 auto 保守降为 static（与既有 verify 降级 warning 行为一致）。
///
/// 调用方（符号验证 / 链接标志 / codegen 分类）统一取本函数分类，单一事实来源。
pub fn effective_load_strategies(
    modules: &[NativeModule],
    lib_paths: &[PathBuf],
) -> HashMap<String, LoadStrategy> {
    let tool = detect_symbol_tool();
    let mut out = HashMap::new();
    for module in modules {
        let name = module.name.to_string();
        let strategy = match module.load {
            LoadStrategy::Static => LoadStrategy::Static,
            LoadStrategy::Runtime => LoadStrategy::Runtime,
            LoadStrategy::Auto => {
                if matches!(
                    name.as_str(),
                    "libc" | "wgpu_native" | "rt_resources" | "rt_process"
                ) {
                    LoadStrategy::Static
                } else {
                    let search = module_lib_search_paths(module, lib_paths);
                    match resolve_native_lib(&name, &search) {
                        // 库文件存在 → 有可静态链接的候选：工具可用且符号验证通过
                        // 则 static；工具不可用则保守 static（RFC 016 §9 降级一致）。
                        Some(lib) => {
                            if let Some(tool) = &tool {
                                if verify_native_symbols(module, &lib, tool).is_ok() {
                                    LoadStrategy::Static
                                } else {
                                    LoadStrategy::Runtime
                                }
                            } else {
                                LoadStrategy::Static
                            }
                        }
                        // 库文件无法定位 → 机器上可能未安装 → 降级 runtime。
                        // 该判定仅依赖文件系统存在性，**不依赖符号工具**，任何
                        // 平台确定性成立（auto 语义的确定性基线）。
                        None => LoadStrategy::Runtime,
                    }
                }
            }
        };
        out.insert(name, strategy);
    }
    out
}

/// 对所有非 libc 的 native 模块执行符号验证（RFC 016 M2 管线入口）。
///
/// 在 `compile_via_llvm_ir` 链接前调用。行为：
/// - 工具不可用 → `Ok(())`（RFC §9 降级为 warning，不阻断编译）
/// - 模块库路径无法定位 → 跳过该模块（不报错，可能由 libc 等隐式链接机制提供）
/// - 符号缺失 → `Err(String)`，包含所有缺失符号的可读描述
///
/// 库解析顺序（RFC 016 M4 多库体系隔离）：per-module 契约内 `library` 目录
/// （最高优先）→ `lib_paths` 搜索列表（`ani-native-lib`）→ vendor 注入 → 系统路径。
///
/// `libc` 永远跳过：所有平台隐式链接，无法定位到单一库文件。
///
/// RFC 016：生效策略为 `runtime`（显式 `load = "runtime"` 或 `auto` 降级）的
/// 模块同样跳过——符号验证仅适用于静态链接模块，运行时懒解析由 codegen 生成
/// 的懒解析器在运行时承担。
///
/// `source_impl`（用户 .ani 同目录 `.c` 源实现模块，见 `mod.rs::prepare_user_native_objects`）
/// 同样跳过——符号由本地编译进产物的 `.o` 提供，无需也无法对外部库验证。
pub fn verify_all_native_modules(
    modules: &[NativeModule],
    lib_paths: &[PathBuf],
    source_impl: &HashSet<String>,
) -> Result<(), String> {
    let strategies = effective_load_strategies(modules, lib_paths);
    let Some(tool) = detect_symbol_tool() else {
        // RFC §9：工具不可用 → 降级为 warning，不阻断编译。
        return Ok(());
    };
    let mut all_errors: Vec<String> = Vec::new();
    for module in modules {
        // 用户源实现模块：符号由本地编译 `.o` 提供，跳过外部验证。
        if source_impl.contains(module.name.as_str()) {
            continue;
        }
        // RFC 016：运行时加载模块不做编译期符号验证（懒解析语义）。
        if strategies.get(&module.name.to_string()) == Some(&LoadStrategy::Runtime) {
            continue;
        }
        // libc 是特殊模块，在所有平台都是隐式链接的，无法定位到具体库文件。
        if module.name.as_str() == "libc" {
            continue;
        }
        // RFC 037 §D7.2：wgpu_native 是 shim 包装模块——`.ani` 声明的
        // `wgpu_create_instance` 等符号实现在 `rt_wgpu_native.c`（runtime .o），
        // 而 `libwgpu_native.dll.a` 导出的是上游 wgpu-native C API
        // `wgpuCreateInstance`（驼峰命名）。两者符号集不同，验证会误报缺失。
        // 链接器最终通过 `rt_wgpu_native.o` + `-lwgpu_native` 解析全部符号。
        if module.name.as_str() == "wgpu_native" {
            continue;
        }
        // RFC 027 M1：rt_resources 的符号实现在 `rt_resources.c`（runtime .o），
        // 无需独立 .lib 文件。符号已由 runtime 对象提供，验证会误报缺失。
        if module.name.as_str() == "rt_resources" {
            continue;
        }
        // Process 体系：rt_process 的符号实现在 rt_proc.c（runtime .o），无需独立 .lib。
        if module.name.as_str() == "rt_process" {
            continue;
        }
        // RFC 016 M4：per-module `library` 目录优先于全局搜索列表（多库体系隔离）。
        let search_paths = module_lib_search_paths(module, lib_paths);
        let Some(lib_path) = resolve_native_lib(module.name.as_ref(), &search_paths) else {
            // 库路径无法定位 → 跳过该模块的验证（不报错）。
            continue;
        };
        if let Err(errors) = verify_native_symbols(module, &lib_path, &tool) {
            for e in errors {
                all_errors.push(e.display());
            }
        }
    }
    if all_errors.is_empty() {
        Ok(())
    } else {
        Err(all_errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{CallingConv, NativeFn};

    #[test]
    fn detect_tool_does_not_panic() {
        // 平台依赖；仅验证探测逻辑不 panic。
        let _ = detect_symbol_tool();
    }

    #[test]
    fn parse_nm_output_extracts_third_column() {
        let stdout = "0000000000001234 T puts\n\
                       0000000000005678 T getenv\n\
                       0000000000009abc T __cxa_finalize\n";
        let syms = parse_symbols(stdout, false);
        assert!(syms.contains("puts"));
        assert!(syms.contains("getenv"));
        assert!(syms.contains("__cxa_finalize"));
    }

    #[test]
    fn parse_nm_output_handles_two_column_macos() {
        let stdout = "00000001000001234 _puts\n\
                       00000001000005678 _getenv\n";
        let syms = parse_symbols(stdout, false);
        assert!(syms.contains("_puts"));
        assert!(syms.contains("_getenv"));
    }

    #[test]
    fn parse_dumpbin_output_extracts_after_pipe() {
        let stdout = "003 00000000 SECT4  notype  External  | puts\n\
                       004 00000010 SECT4  notype  External  | getenv\n";
        let syms = parse_symbols(stdout, true);
        assert!(syms.contains("puts"));
        assert!(syms.contains("getenv"));
    }

    #[test]
    fn resolve_native_lib_returns_none_for_missing() {
        // 不存在的路径 → None（调用方跳过验证）
        let paths = vec![PathBuf::from("/nonexistent/path/12345")];
        assert!(resolve_native_lib("testffi", &paths).is_none());
    }

    #[test]
    fn verify_all_native_modules_skips_libc() {
        // libc 是特殊模块，永远跳过验证（所有平台隐式链接）
        let module = NativeModule {
            name: "libc".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        let result = verify_all_native_modules(&[module], &[], &HashSet::new());
        assert!(result.is_ok(), "libc should be skipped: {result:?}");
    }

    #[test]
    fn verify_all_native_modules_empty_is_ok() {
        // 空模块列表 → Ok
        let result = verify_all_native_modules(&[], &[], &HashSet::new());
        assert!(result.is_ok());
    }

    #[test]
    fn verify_all_native_modules_skips_unresolvable_lib() {
        // 非 libc 模块但库路径无法定位 → 跳过（Ok）
        let module = NativeModule {
            name: "nonexistent_lib_12345".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        let paths = vec![PathBuf::from("/nonexistent/path/12345")];
        let result = verify_all_native_modules(&[module], &paths, &HashSet::new());
        assert!(
            result.is_ok(),
            "unresolvable lib should be skipped: {result:?}"
        );
    }

    /// RFC 016 M4：per-module `library` 目录隔离——每个模块从各自声明的目录解析库，
    /// 互不混放；全局 `ani-native-lib` 列表兜底。
    #[test]
    fn resolve_native_lib_prefers_module_library_dir() {
        use std::fs;
        let base = std::env::temp_dir().join(format!("arc-ffi-isolation-{}", std::process::id()));
        let dir_a = base.join("vendor_a");
        let dir_b = base.join("vendor_b");
        let global = base.join("global");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();
        fs::create_dir_all(&global).unwrap();

        // 平台命名约定（与 `resolve_native_lib` 候选顺序一致）。
        let candidate = |module: &str| -> String {
            if cfg!(windows) {
                format!("{module}.lib")
            } else if cfg!(target_os = "macos") {
                format!("lib{module}.dylib")
            } else {
                format!("lib{module}.so")
            }
        };

        fs::write(dir_a.join(candidate("alpha")), "a").unwrap();
        fs::write(dir_b.join(candidate("beta")), "b").unwrap();
        fs::write(global.join(candidate("gamma")), "g").unwrap();

        let global_paths = vec![global.clone()];
        let alpha = NativeModule {
            name: "alpha".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            callbacks: vec![],
            library: Some(dir_a.clone()),
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
        };
        let beta = NativeModule {
            name: "beta".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            callbacks: vec![],
            library: Some(dir_b.clone()),
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
        };
        let gamma = NativeModule {
            name: "gamma".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            callbacks: vec![],
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
        };

        // 各模块从自身声明的目录解析，互不混放。
        let a =
            resolve_native_lib("alpha", &module_lib_search_paths(&alpha, &global_paths)).unwrap();
        assert_eq!(a, dir_a.join(candidate("alpha")));
        let b = resolve_native_lib("beta", &module_lib_search_paths(&beta, &global_paths)).unwrap();
        assert_eq!(b, dir_b.join(candidate("beta")));
        // 未声明 library 的模块从全局 `ani-native-lib` 列表兜底。
        let g =
            resolve_native_lib("gamma", &module_lib_search_paths(&gamma, &global_paths)).unwrap();
        assert_eq!(g, global.join(candidate("gamma")));
        // 隔离性：即使全局目录存在同名文件，模块仍优先自身目录。
        fs::write(global.join(candidate("alpha")), "shadow").unwrap();
        let a2 =
            resolve_native_lib("alpha", &module_lib_search_paths(&alpha, &global_paths)).unwrap();
        assert_eq!(
            a2,
            dir_a.join(candidate("alpha")),
            "module library dir must win over global list"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// RFC 016：`effective_load_strategies`——显式 static / runtime 直通。
    #[test]
    fn effective_strategies_explicit_static_and_runtime() {
        let static_mod = NativeModule {
            name: "libfoo".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        let runtime_mod = NativeModule {
            name: "gpu".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Runtime,
            callbacks: vec![],
        };
        let paths = vec![PathBuf::from("/nonexistent/path/12345")];
        let s = effective_load_strategies(&[static_mod, runtime_mod], &paths);
        assert_eq!(s.get("libfoo"), Some(&LoadStrategy::Static));
        assert_eq!(s.get("gpu"), Some(&LoadStrategy::Runtime));
    }

    /// RFC 016：`auto` 模块库无法定位时**确定性地**降级 runtime（不依赖符号
    /// 工具——纯文件存在性判定），且不阻断编译（库缺失是运行时语义）。
    #[test]
    fn effective_strategies_auto_unresolvable_degrades_to_runtime() {
        let auto_mod = NativeModule {
            name: "optional_gpu_12345".into(),
            functions: vec![NativeFn {
                name: "init".into(),
                symbol: None,
                params: vec![],
                ret: None,
                calling_conv: CallingConv::C,
            }],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Auto,
            callbacks: vec![],
        };
        let paths = vec![PathBuf::from("/nonexistent/path/12345")];
        let s = effective_load_strategies(std::slice::from_ref(&auto_mod), &paths);
        assert_eq!(s.get("optional_gpu_12345"), Some(&LoadStrategy::Runtime));
        // auto 模块不阻断编译（库缺失是运行时语义）。
        let result =
            verify_all_native_modules(std::slice::from_ref(&auto_mod), &paths, &HashSet::new());
        assert!(
            result.is_ok(),
            "auto module must not hard-fail verification: {result:?}"
        );
    }

    /// RFC 016：`auto` 模块为隐式链接名（libc 等）→ 恒 static。
    #[test]
    fn effective_strategies_auto_implicitly_linked_is_static() {
        let auto_libc = NativeModule {
            name: "libc".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Auto,
            callbacks: vec![],
        };
        let s = effective_load_strategies(&[auto_libc], &[]);
        assert_eq!(s.get("libc"), Some(&LoadStrategy::Static));
    }

    /// RFC 016：`runtime` 模块被 `verify_all_native_modules` 跳过验证——
    /// 库缺失（机器未安装）正是该模型要规避的编译期硬失败。
    #[test]
    fn verify_all_skips_runtime_modules() {
        let runtime_mod = NativeModule {
            name: "not_installed_gpu_12345".into(),
            functions: vec![NativeFn {
                name: "init".into(),
                symbol: None,
                params: vec![],
                ret: None,
                calling_conv: CallingConv::C,
            }],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Runtime,
            callbacks: vec![],
        };
        let paths = vec![PathBuf::from("/nonexistent/path/12345")];
        let result = verify_all_native_modules(&[runtime_mod], &paths, &HashSet::new());
        assert!(
            result.is_ok(),
            "runtime module must skip verification: {result:?}"
        );
    }
}
