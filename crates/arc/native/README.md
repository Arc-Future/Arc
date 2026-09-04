# crates/arc/native/

编译器内置的 Arc verified FFI 契约（`*.ani`，RFC 027）。契约是**编译期输入**，
由编译器侧归属：`load_native_contracts`（`crates/arc/src/loader.rs`）扫描本目录
（`CARGO_MANIFEST_DIR/native`），解析为 `NativeModule` 供 typeck/codegen 使用。

用户项目可在其 workspace 根建 `native/` 目录放置自定义契约；同模块名时用户契约
覆盖内置（内置契约仍保留为编译器的兜底/默认）。

## 契约清单

| 文件 | 模块 | 角色 |
|------|------|------|
| `libc.ani` | `libc` | C 标准库契约（`puts`/`getenv`/`frexp` 等） |
| `rt_library.ani` | `rt_library` | 动态库加载 ABI（`load`/`sym`/`unload`，RFC 100 热卸载闭环） |
| `rt_resources.ani` | `rt_resources` | 本地化与资源 ABI（RFC 054） |
| `rt_process.ani` | `rt_process` | Process 体系 ABI（spawn/pipe/PTY） |
| `arc_test.ani` | `arc_test` | List\<T\> marshal e2e 测试辅助（实现于 `crates/runtime/rt_native_test.c`） |
| `wgpu-native.ani` | `wgpu_native` | wgpu-native C API 契约（实现于 `crates/runtime-ui/rt_wgpu_native.c`） |
| `ani_auto_module.ani` | `ani_auto_module` | `load = "auto"` 降级语义 e2e 辅助 |
| `ani_env_lib.ani` | `ani_env_lib` | `library` 环境变量形态（形态②）e2e 辅助 |
| `ani_runtime_probe.ani` | `ani_runtime_probe` | `load = "runtime"` 懒解析 e2e 辅助 |
| `ani_env_library.ani` | — | 已被收口流程移除（`4530c591`） |

## 相关运行时代码归属

- 平台 C/C++ 后端 → `crates/runtime-ui/platform/<os>/`（UI 运行时随 `crates/runtime-ui/`；非 UI 运行时另在 `crates/runtime/`）
- UI runtime ABI + wgpu 渲染后端 → `crates/runtime-ui/`（`rt_ui_abi.h` + `wgpu-native/`）
- sqlite 运行时代码 → `crates/runtime-sqlite/`
