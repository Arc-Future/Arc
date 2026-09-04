# crates/runtime-ui/

Arc UI 运行时代码 crate（RFC 026 §7.6 独立 C 资源目录）。

| 内容 | 角色 |
|------|------|
| `rt_ui_abi.h` | UI 运行时 ABI 声明（Window/Element/IME 等，纯声明） |
| `wgpu-native/` | wgpu-native 渲染后端（架构重构后由 `native/wgpu-native/` 迁入） |
| `wgpu-native/include/` | 跨平台头文件（`webgpu.h` + `wgpu.h`） |
| `wgpu-native/bin/<os>/` | 平台预编译二进制（`wgpu_native.dll`/`.lib`；fetch 脚本 `scripts/fetch-wgpu-native.ps1`） |
| `wgpu_native.lib` | 链接期兜底库路径哨兵（`effective_native_lib_paths` fallback） |

构建时由 `codegen` 注入 wgpu 头文件搜索路径与 vendor lib 目录；
平台 C/C++ 窗口/IME 后端在 `crates/runtime-ui/platform/<os>/`（随本 crate 迁移，非 UI 运行时另在 `crates/runtime/`）。
