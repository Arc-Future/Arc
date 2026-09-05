//! Name mangling and platform/target utilities for the LLVM IR backend.
//!
//! Standalone helpers used by the codegen pipeline: function/method name
//! mangling, clang binary path resolution, and per-OS link flag selection.

/// Source-level entry function name (C# PascalCase convention).
///
/// Arc follows C# conventions: the program entry point is `Main` (PascalCase),
/// not `main`. The emitted LLVM symbol is [`ENTRY_SYMBOL`] (lowercase `main`)
/// because the C runtime linker expects that exact symbol.
pub(super) const ENTRY_FN_NAME: &str = "Main";

/// LLVM symbol emitted for the program entry point.
///
/// The C runtime linker scans for `main`, so the source-level `Main` is
/// emitted under this lowercase symbol regardless of the source name.
pub(super) const ENTRY_SYMBOL: &str = "main";

/// Returns true if `name` is the source-level program entry function.
/// Matches `Main` (free function) or `ClassName::Main` (test host / class method).
pub(crate) fn is_entry_fn(name: &str) -> bool {
    name == ENTRY_FN_NAME || name.ends_with("::Main")
}

pub(super) fn mangle_fn_name(name: &str) -> String {
    if is_entry_fn(name) {
        return ENTRY_SYMBOL.to_string();
    }
    name.replace("::", "_")
}

pub(super) fn mangle_method(class: &str, method: &str) -> String {
    format!("{class}_{method}")
}

/// Resolve the clang binary path. Resolution order:
///
/// 1. `ARC_CLANG` environment variable (explicit override, all platforms)
/// 2. Arc-managed toolchain clang（`<tools>/llvm/current` 指针 → `<ver>/bin/clang`；
///    `arc toolchain install llvm` 落点，见 [`sdk_layout::toolchain_llvm_clang_path`]）
/// 3. SDK 捆绑 clang（`<sdk-root>/lib/llvm/bin/clang`，`-BundleLlm` 分发包落点，
///    见 [`sdk_layout::bundled_llvm_clang_path`]——解压即得离线构建基线）
/// 4. Windows-only probe of known LLVM install locations
/// 5. `clang` on PATH (fallback, all platforms)
///
/// Target triple handling is deferred to `optimize::clang_compile` / `clang_link` —
/// this function is a pure path resolver.
///
/// `pub` 供 `arc env` / `arc doctor`（`crates/arc`）复用同一解析序，避免双轨。
pub fn clang_path() -> String {
    if let Ok(p) = std::env::var("ARC_CLANG") {
        if !p.is_empty() {
            return p;
        }
    }
    if let Some(p) = crate::sdk_layout::toolchain_llvm_clang_path() {
        return p.display().to_string();
    }
    if let Some(p) = crate::sdk_layout::bundled_llvm_clang_path() {
        return p.display().to_string();
    }
    if cfg!(windows) {
        for p in [
            r"C:\Program Files\LLVM\bin\clang.exe",
            r"C:\Program Files (x86)\LLVM\bin\clang.exe",
        ] {
            if std::path::Path::new(p).exists() {
                return p.into();
            }
        }
    }
    "clang".into()
}

pub(super) enum TargetOs {
    Windows,
    Linux,
    Macos,
    Ohos,
    WebAssembly,
    Wasi,
    Host,
}

/// RFC 037 M-W3 (Draft): wasm32/wasm64 triple detection for runtime/link gating.
pub(super) fn is_wasm_triple(triple: &str) -> bool {
    triple.starts_with("wasm32-") || triple.starts_with("wasm64-")
}

pub(super) fn target_os(target: Option<&str>) -> TargetOs {
    let triple = target.unwrap_or("");
    if triple.starts_with("wasm32-unknown-unknown") || triple.starts_with("wasm64-unknown-unknown")
    {
        TargetOs::WebAssembly
    } else if triple.starts_with("wasm32-wasi")
        || triple.starts_with("wasm64-wasi")
        || triple == "wasm32-wasip1"
        || triple == "wasm32-wasip2"
    {
        TargetOs::Wasi
    } else if triple.contains("windows") {
        TargetOs::Windows
    } else if triple.contains("linux") {
        TargetOs::Linux
    } else if triple.contains("darwin") || triple.contains("macos") {
        TargetOs::Macos
    } else if triple.contains("ohos") {
        TargetOs::Ohos
    } else {
        TargetOs::Host
    }
}

pub(super) fn platform_link_flags(target: Option<&str>) -> Vec<&'static str> {
    match target_os(target) {
        TargetOs::WebAssembly | TargetOs::Wasi => vec![],
        TargetOs::Windows => vec![
            "-luser32",
            "-lgdi32",
            // RFC 026 M2 ARML 渲染升级：Direct2D + DirectWrite（Windows 系统库）
            "-ld2d1",
            "-ldwrite",
            "-limm32",
            "-lwindowscodecs",
            "-lole32",
            "-luuid",
            // RFC 038 M4 IOCP Reactor：rt_reactor_iocp 依赖 WinSock 2 + 扩展（AcceptEx/ConnectEx）
            "-lws2_32",
            "-lmswsock",
            // rt_env_user_name → GetUserNameA
            "-ladvapi32",
            "-limm32",
            // RFC 038 M1：vendored wgpu_native.lib 为 Rust 静态库，需 Rust std 的
            // Windows 系统库依赖（ntdll/userenv/bcrypt 恒可用，不影响非 wgpu 程序）。
            "-lntdll",
            "-luserenv",
            "-lbcrypt",
        ],
        TargetOs::Linux => vec![
            "-lX11",
            // RFC 017: rt_library_load/sym/unload 依赖 libdl（dlopen/dlsym/dlclose）
            "-ldl",
        ],
        TargetOs::Macos => vec![
            "-framework",
            "AppKit",
            "-framework",
            "Foundation",
            "-framework",
            "CoreGraphics",
        ],
        TargetOs::Ohos => vec![],
        TargetOs::Host => {
            // No target specified — infer from host OS to avoid infinite recursion.
            if cfg!(windows) {
                vec![
                    "-luser32",
                    "-lgdi32",
                    "-ld2d1",
                    "-ldwrite",
                    "-limm32",
                    "-lwindowscodecs",
                    "-lole32",
                    "-luuid",
                    // RFC 038 M4 IOCP Reactor：rt_reactor_iocp 依赖 WinSock 2 + 扩展
                    "-lws2_32",
                    "-lmswsock",
                    // rt_env_user_name → GetUserNameA
                    "-ladvapi32",
                    // RFC 038 M1：vendored wgpu_native.lib 为 Rust 静态库，需 Rust std 系统库
                    "-lntdll",
                    "-luserenv",
                    "-lbcrypt",
                ]
            } else if cfg!(target_os = "linux") {
                vec![
                    "-lX11", // RFC 017: rt_library_load/sym/unload 依赖 libdl
                    "-ldl",
                ]
            } else if cfg!(target_os = "macos") {
                vec![
                    "-framework",
                    "AppKit",
                    "-framework",
                    "Foundation",
                    "-framework",
                    "CoreGraphics",
                ]
            } else {
                vec![]
            }
        }
    }
}

/// 判断目标是否为 Windows（含主机直连的 `None` 目标，与 `platform_link_flags`
/// 的 `Host` 分支一致——用 `cfg!(windows)` 兜底）。
pub(super) fn is_windows_target(target: Option<&str>) -> bool {
    matches!(target_os(target), TargetOs::Windows)
        || (matches!(target_os(target), TargetOs::Host) && cfg!(windows))
}

/// Windows GUI 子系统链接标志：消除运行时弹出的控制台窗口（"黑框"）。
///
/// 仅 Windows 目标有效；非 UI 可执行文件不应调用（由调用方以 `needs_platform_window`
/// 门控）。GUI 子系统隐藏控制台，但 Arc 程序的真实入口仍是 `main`
///（[`ENTRY_SYMBOL`]），故 MSVC/lld-link 下必须显式 `/ENTRY:mainCRTStartup`
/// 让 CRT 启动流程继续走 `main`；MinGW/GNU 用 `--subsystem,windows` 且其 CRT
/// 天然保留 `main` 入口，无需额外指定。
pub(super) fn gui_subsystem_flags(target: Option<&str>) -> Vec<String> {
    if !is_windows_target(target) {
        return vec![];
    }
    let gnu = target.map(|t| t.contains("gnu")).unwrap_or(false);
    if gnu {
        vec!["-Wl,--subsystem,windows".to_string()]
    } else {
        vec![
            "-Wl,/SUBSYSTEM:WINDOWS".to_string(),
            "-Wl,/ENTRY:mainCRTStartup".to_string(),
        ]
    }
}

/// RFC 026 §D7.2：wgpu-native vendoring 平台子目录名。
///
/// 预编译二进制位于 `crates/runtime-ui/wgpu-native/bin/<subdir>/`：
/// - Windows：`windows`（GNU 变体 `wgpu_native.dll` + `libwgpu_native.dll.a`）
/// - Linux：`linux`（`libwgpu_native.so`，M3+ 接入）
/// - macOS：当前返回 `None`（M3+ 接入 Metal 后端）
///
/// 当目标平台未 vendoring 时返回 `None`，调用方跳过 lib path 注入与 DLL 复制。
pub(super) fn wgpu_native_vendor_subdir(target: Option<&str>) -> Option<&'static str> {
    match target_os(target) {
        TargetOs::WebAssembly | TargetOs::Wasi => None,
        TargetOs::Windows => Some("windows"),
        TargetOs::Linux => Some("linux"),
        TargetOs::Macos | TargetOs::Ohos => None,
        TargetOs::Host => {
            if cfg!(windows) {
                Some("windows")
            } else if cfg!(target_os = "linux") {
                Some("linux")
            } else {
                None
            }
        }
    }
}

/// RFC 026 M1：vendored 密码学底座平台子目录名。
///
/// 预编译二进制位于 `crates/runtime-crypto/bin/<subdir>/`：
/// - Windows：`windows`（`crypto_native.dll` + `crypto_native.lib` +
///   `libcrypto_native.dll.a`，由 `scripts/fetch-boringssl-native.ps1` 生成）
/// - 其余平台：当前返回 `None`（未入库；e2e 按 clang/DLL 软跳过纪律门禁）
///
/// 当目标平台未 vendoring 时返回 `None`，调用方跳过 lib path 注入与 DLL 复制。
pub(super) fn crypto_native_vendor_subdir(target: Option<&str>) -> Option<&'static str> {
    match target_os(target) {
        TargetOs::WebAssembly | TargetOs::Wasi => None,
        TargetOs::Windows => Some("windows"),
        TargetOs::Linux | TargetOs::Macos | TargetOs::Ohos => None,
        TargetOs::Host => {
            if cfg!(windows) {
                Some("windows")
            } else {
                None
            }
        }
    }
}
