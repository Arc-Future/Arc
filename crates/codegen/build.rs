// codegen crate 不再把系统库链进产物（编译器 CLI 自身不调用 X11）。
//
// Linux 早期遗留的 `cargo:rustc-link-lib=X11` 已移除：X11 仅在**目标程序**
// 链接期按需注入（见 `mangle::platform_link_flags` 的 Linux 分支——
// runtime-ui X11 窗口后端对象随目标链接），与编译器自身二进制无关。该链接
// 曾使 headless Linux 上连 `arc --version` 都因缺少 libX11.so.6 无法启动
// （平台审计 Top-10 #9；CI 此前以安装 libx11-dev 掩盖）。
fn main() {}
