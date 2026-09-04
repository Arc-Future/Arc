// Arc.UI.WebView —— 浏览器组件库。
//
// 把系统自带浏览器引擎（WebView2 / WKWebView / WebKitGTK）作为 wgpu 合成面内
// 的一等纹理表面集成：GPU 零拷贝捕获（wgpu-scry 方案）+ 注入脚本/自定义协议桥。
// 设计权威：docs/rfc/037-ui/references/webview-surface.md。
//
// 实现未开工（单 M · 子集边界 · 不自动开干）；本文件仅注册库命名空间，
// 使本包作为 std/UI 解决方案成员可构建（禁止以空目录冒充已实现库）。

namespace Arc.UI.WebView;
