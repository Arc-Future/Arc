// Program.as —— ArcAgent 入口（无 namespace，C# Program.cs 惯例）。
//
// 组合根装配流程（AgentHost.CreateAsync）：
//   Provider（真实 DeepSeek） → 工作区（根 + 沙箱 + git 状态） → 记忆（Wiki 持久化）
//   → 会话选项（系统指令含工作区边界 + 能力白名单 + 流式 + 知识面注入）
//   → AIHost.Create → 并入记忆 → 注册项目约定上下文源 → 会话事件日志 → ReplSession（REPL）。
//
// 运行（启动后先输入 DeepSeek API 密钥与工作区根目录）：
//   cargo run -p arc -- run examples/ArcAgent/Program.as
using Arc;
using Arc.Agent;
using Arc.Agent.DeepSeek;
using Arc.Agent.Harness;
using Arc.Agent.Harness.Coding;
using ArcAgent.Context;
using ArcAgent.Host;
using ArcAgent.Repl;
using ArcAgent.SessionLog;
using ArcAgent.Workspace;

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
    AgentWorkspace workspace = AgentWorkspace.Resolve(wsRoot);

    // ── 3. 上下文 + 会话事件日志（记忆 .arcagent/memory/wiki.json；会话 .arcagent/sessions/） ──
    AgentContext context = new AgentContext(workspace.Root + "/.arcagent/memory/wiki.json");
    SessionEventLog log = context.NewSessionLog();
    await log.StartAsync("ArcAgent session", new CancellationToken());

    // ── 4. Harness 基座 + Coding 判定（RFC 043 H-2b/H-2c）+ 装配宿主 → 会话 ──
    // quality.* 经 Arc.Agent.Harness.Coding [AITool] 声明式装配；需求本尊 = AIRfc。
    AIHarnessSession harness = new AIHarnessSession(workspace.Root, new CodingDoDGateEvaluator());
    AIChatClient provider = AgentHost.CreateProvider(apiKey);
    AIHost host = await AgentHost.CreateAsync(provider, workspace, context, new CancellationToken());
    AISession session = host.CreateSession();
    // 决策轨迹接线：Harness 的 airfc:*/checkpoint:* 经 Agent 会话事件面写入（M5–M6 单轨）。
    harness.AttachSession(session);
    // AIRfc 锚点注入（RFC 043 场景 1.1 E 面修复）：活上下文源——/rfc /revise 后
    // AIRfc 锚点（Intention/Design/Acceptance/Revision/Plan 摘要）以 Rules 层块进入模型
    // Instructions 上下文；块内容仅随 Revision 变更 → 前缀稳定吃 KV cache（非每轮重注入）。
    host.Context.AddProvider(new AIRfcContextProvider(harness));
    // 冲突织物 + 计划门闩接线（RFC 043 M9 薄组装）：Coordinator 经 AIHarnessSession →
    // AIRfcRuntime 传递，RfcSpec 租约在真实路径生效（后到拒绝）；挂 AIPlanGate 后
    // M8 汇总门 CompletePlanAfterDoDAsync 可经受控 API 把计划转入 Completed。
    harness.AttachCoordinator(host.Coordinator, session.SessionId);
    harness.AttachPlanGate(host.PlanGate);

    // ── 5. REPL（读输入 → 命令路由 / 任务回合；会话事件自动落日志） ──
    // 方向环（H-4）：harness 传入 Repl，/rfc /revise /summary /checkpoint /rollback /dod 经
    // AIHarnessSession 走基座（薄组装，Repl 不重复实现 PM/DoD）。
    ReplSession repl = new ReplSession(host, session, context, workspace, log, harness);
    await repl.RunAsync();

    // ── 6. 退出：持久化记忆 + 释放 ──
    bool persisted = await context.SaveWikiAsync(host.Wiki, new CancellationToken());
    Console.WriteLine("memory " + (persisted ? "saved to " + context.MemoryFile : "not persisted"));
    Console.WriteLine("decision events: " + session.DecisionEvents.Count);
    session.Dispose();
    host.Dispose();
}
