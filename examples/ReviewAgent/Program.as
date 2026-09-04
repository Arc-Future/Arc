// Program.as —— ReviewAgent 入口（无 namespace，C# Program.cs 惯例）。
//
// 组合根装配流程：
//   Provider（真实 DeepSeek） → 会话选项（领域提示 + 能力白名单 review.Run/fs.Read/fs.Write）
//   → AIHost.Create → 会话 → AIHarnessSession（领域 evaluator：ReviewDoDGateEvaluator）
//   → 决策轨迹接线（AttachSession）→ 冲突织物 + 计划门闩接线（AttachCoordinator/AttachPlanGate）
//   → ReviewRepl（薄壳 REPL）。
//
// 复用纪律（RFC 043 P5）：本项目不依赖 Arc.Agent.Harness.Coding —— AIRfc / DoD 门骨架 /
// 事件单轨全部来自基座 Arc.Agent.Harness；领域差异只体现在领域工具（[AITool] review.*）
// 与领域 DoD 判定（ReviewDoDGateEvaluator）。
//
// 运行（启动后先输入 DeepSeek API 密钥与工作区根目录）：
//   cargo run -p arc -- build examples/ReviewAgent
//   cargo run -p arc -- run examples/ReviewAgent/Program.as
using Arc;
using Arc.Agent;
using Arc.Agent.DeepSeek;
using Arc.Agent.Harness;
using ReviewAgent.DoD;
using ReviewAgent.Host;
using ReviewAgent.Repl;

async Task<void> Main() {
    // ── 1. 配置：API 密钥（环境变量优先；缺则提示输入） ──
    string apiKey = Environment.GetEnvironmentVariable("ARC_DEEPSEEK_API_KEY");
    if (apiKey == null || apiKey == "") {
        Console.Write("DeepSeek API key: ");
        apiKey = Console.ReadLine();
    }

    // ── 2. 工作区根（回车 = 当前目录） ──
    Console.Write("Workspace root (Enter = current dir): ");
    string wsInput = Console.ReadLine();
    string wsRoot = (wsInput == null || wsInput.Trim() == "") ? Environment.GetCurrentDirectory() : wsInput;

    // ── 3. 宿主 + 会话（领域二薄组装：Provider + 选项 + Host） ──
    AIChatClient provider = ReviewHost.CreateProvider(apiKey);
    AIHost host = AIHost.Create(provider, ReviewHost.BuildOptions());
    AISession session = host.CreateSession();

    // ── 4. Harness 基座（RFC 043 P5）：复用 AIHarnessSession —— 仅注入领域 evaluator ──
    AIHarnessSession harness = new AIHarnessSession(wsRoot, new ReviewDoDGateEvaluator());
    harness.AttachSession(session);
    harness.AttachCoordinator(host.Coordinator, session.SessionId);
    harness.AttachPlanGate(host.PlanGate);

    // ── 5. REPL（方向环命令经 AIHarnessSession 走基座；领域工具直测） ──
    ReviewRepl repl = new ReviewRepl(host, session, harness, wsRoot);
    await repl.RunAsync();

    // ── 6. 退出：释放 ──
    Console.WriteLine("decision events: " + session.DecisionEvents.Count);
    session.Dispose();
    host.Dispose();
}
