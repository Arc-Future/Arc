// Program.as: 程序入口文件（所有 Arc 项目统一此标准）。
//
// 对标 WPF App.g.cs 自动生成的 Main 入口，但 Arc 让用户显式控制入口文件，
// 便于定制启动流程（如注入日志、配置加载、依赖注入容器构建等）。
//
// 标准模式：
//   var app = new App();
//   app.Run();
//
// `App.Run()` 内部调用 `InitializeComponent()`（由 App.g.as 自动生成），
// 后者创建 StartupUri 指向的 MainWindow 并调用其 InitializeComponent 触发显示。

namespace ArmlDemo;

using Arc;

public void Main() {
    var app = new App();
    app.Run();
}
