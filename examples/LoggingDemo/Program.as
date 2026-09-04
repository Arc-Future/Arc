// LoggingDemo —— Arc.Logging 基础设施演示。
//
// 覆盖：
//   1. 独立 LoggerFactory + 内置 Console Provider + 全局最低级别过滤
//   2. 结构化消息模板（{Name} 占位符按出现顺序绑定 args；支持对齐 / 转义 / 数字格式子集）
//   3. DI 集成：services.AddLogging() → 注入 ILogger / ILoggerFactory
//
// ⚠ 已知编译器缺口（本 demo 暂无法端到端运行，属编译器开发范畴，非 Log 库缺陷）：
//   G1. `params ReadOnlySpan<string>`（string 元素）调用点打包为裸指针而非 span 胖指针，
//       运行时访问 args 触发 0xC0000005。Log 便捷方法（LogInformation(msg, a, b, ...)）
//       均依赖此特性。已用纯 Arc 最小用例独立复现。
//   G2. `LoggerFactoryExtensions::CreateLogger<T>` 泛型扩展可达性偶发
//       `undefined name ...CreateLogger_1`（DI 的 GetService<T> 等泛型扩展正常，属特例）。
//   Log 库本体完整可用；待上述编译器修复后本 demo 即可编译运行并输出 "logging:ok"。
namespace LoggingDemo;

using Arc;
using Arc.Logging;
using Arc.DI;

class DemoService { }

public void Main() {
    // ── 1. 独立工厂 + 控制台 Provider + 最低级别过滤 ──
    ILoggerFactory factory = new LoggerFactory()
        .AddConsole()
        .SetMinimumLevel(LogLevel.Debug);

    ILogger logger = factory.CreateLogger<DemoService>();

    logger.LogTrace("Trace 应被过滤（MinimumLevel=Debug）");
    logger.LogDebug("Debug 消息：占位符 {0} / {1}", "42", "arc");
    logger.LogInformation("用户 {Id,-6} 于 {Time} 登录", "" + 7, "08:00");
    logger.LogWarning("磁盘使用率 {Usage}%", "87");
    logger.LogError(new Exception("模拟失败"), "处理订单 {OrderId} 失败", "A-100");
    logger.LogCritical("致命错误 event={Evt}", "" + 5001);

    // 转义花括号演示
    logger.LogInformation("字面量花括号 {{ }} 与占位符 {0}", "1");

    // ── 2. DI 集成：AddLogging 注册 ILogger / ILoggerFactory ──
    var services = new ServiceCollection();
    services.AddLogging();
    IServiceProvider sp = services.Build();

    ILogger injected = sp.GetService<ILogger>();
    injected.LogInformation("从 DI 解析的 ILogger 生效");

    ILoggerFactory diFactory = sp.GetService<ILoggerFactory>();
    diFactory.CreateLogger<DemoService>().LogInformation("DI 工厂创建的类型化 Logger");

    Console.WriteLine("logging:ok");
}
