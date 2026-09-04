//! 链接器参数注入（RFC 016 M1）。
//!
//! 收集所有 native 模块名，转换为链接器 `-l<name>` 标志。
//! 在 `compile_via_llvm_ir` 入口处计算，与平台链接标志合并后
//! 注入 `clang_link` 的 `extra_flags` 之前。
//!
//! RFC 016：生效策略为 `runtime` 的模块**不**注入 `-l<name>`——其符号由
//! 运行时懒解析器经 `rt_library_load`/`rt_library_sym` 解析，静态链接会因
//! 库缺失（机器未安装）而失败，正是该模型要规避的。

use ast::{LoadStrategy, NativeModule};
use std::collections::HashMap;

/// 收集需要静态链接的 native 模块名，用于链接器 `-l<name>` 标志注入。
///
/// 跳过 C 标准库（`libc`）——它在所有平台都是隐式链接的，
/// 显式 `-llibc` 在 Windows MSVC 上会找不到 `libc.lib` 导致链接失败。
/// `libc.ani` 声明的 `puts`/`getenv` 等符号由平台 C 运行时提供。
///
/// RFC 016 M3 §3.3：同样跳过 `arc_test`——这是 List<T> marshal e2e 测试辅助
/// 模块，其 C 实现内联在 `crates/runtime/rt_native_test.c`，通过 `rt_sources`
/// 编译进 runtime，无需也无可供链接的 `arc_test.lib`/`libarc_test.so`。
///
/// RFC 017 M2：同样跳过 `rt_library`——动态库加载 ABI（`rt_library_load/sym/unload`）
/// 内联在 `crates/runtime/rt_library.c`，编译进 runtime，无需独立链接。
///
/// RFC 027 M1：同样跳过 `rt_resources`——本地化与资源 ABI
/// 内联在 `crates/runtime/rt_resources.c`，编译进 runtime，无需独立链接。
///
/// RFC 037 §D7.2：`wgpu_native` 不再跳过——预编译二进制已 vendoring 至
/// `crates/runtime-ui/wgpu-native/bin/<os>/`，由 `mod.rs` 自动注入 lib path 与
/// DLL 复制（Windows），链接器可找到 `libwgpu_native.dll.a`（MinGW）。
pub(crate) fn native_link_libs(
    modules: &[NativeModule],
    strategies: &HashMap<String, LoadStrategy>,
) -> Vec<String> {
    modules
        .iter()
        .map(|m| m.name.to_string())
        .filter(|name| {
            !matches!(
                name.as_str(),
                "libc" | "arc_test" | "rt_library" | "rt_resources" | "rt_process"
            )
        })
        .filter(|name| strategies.get(name) != Some(&LoadStrategy::Runtime))
        .collect()
}
