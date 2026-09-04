// RFC 038 M8.2：AIPlanTools — 内置计划工具（plan / mark_step_done / revise_plan）。
//
// 框架内置（能力 ai.Plan，经 AIPlanGate.InstallTools 装配）：任何启用计划门闩的 Harness
// 无需自写计划工具——schema 与状态机语义统一，杜绝每个应用重复实现导致的漂移。
// 工具经 AIToolSandbox 统一走 capability 分派（ai.Plan 白名单授权）+ 调度层计划门闩。
//
// 语义（对齐 claude-code Plan Mode）：
//   - plan：创建结构化计划 → Pending（待人类审批，写入被拦）。返回含 Lint 引导反馈。
//   - mark_step_done：标记步骤完成 → 自动推进当前指针；满额只到 Verifying（待 DoD D0–D7 判定）。
//   - revise_plan：产出修订版（Revision+1）→ 回 Pending 重审（执行中发现偏离时调用）。
//
// 实现纪律：internal 类（核心编排不外露，对齐 AIToolDispatcher 隔离）；handler 以
// AIPlanGate 引用读写计划（单一事实源），不直接触碰 provider。
namespace Arc.Agent;
using Arc;

/// <summary>内置计划工具装配（internal：仅经 AIPlanGate.InstallTools 触发）。</summary>
internal static class AIPlanTools {
    /// <summary>把字符串数组步骤归一为 List（null 安全；空数组 → 空列表）。</summary>
    public static List<string> StepList(string[] steps) {
        List<string> result = new List<string>();
        if (steps != null) {
            int n = steps.Length;
            int i = 0;
            while (i < n) {
                result.Add(steps[i] != null ? steps[i] : "");
                i = i + 1;
            }
        }
        return result;
    }

    /// <summary>把 plan / mark_step_done / revise_plan 三个内置工具注册进目标工具集。</summary>
    public static void Install(AIPlanGate gate, AIToolSet tools) {
        if (gate == null || tools == null) {
            return;
        }
        AIToolDescriptor plan = new AIToolDescriptor(
            "plan",
            "Create or replace the task plan before making changes. Call this for any non-trivial task. "
                + "The plan (goal, analysis, ordered steps, verification criteria) must be approved by the "
                + "human before any write-capable tool can be used.",
            "ai.Plan",
            false);
        plan.ParametersSchema = "{\"type\":\"object\",\"properties\":{"
            + "\"goal\":{\"type\":\"string\",\"description\":\"Restatement of the task goal in your own words.\"},"
            + "\"analysis\":{\"type\":\"string\",\"description\":\"Your analysis: problem diagnosis, approach, and modification strategy.\"},"
            + "\"steps\":{\"type\":\"array\",\"items\":{\"type\":\"string\"},\"description\":\"Ordered list of concrete, verifiable steps; mention the files each step touches.\"},"
            + "\"verification\":{\"type\":\"string\",\"description\":\"Verification criteria: commands to run and expected outcomes.\"}"
            + "},\"required\":[\"goal\",\"steps\"]}";
        tools.Add(plan, new AIPlanPlanToolHandler(gate));

        AIToolDescriptor mark = new AIToolDescriptor(
            "mark_step_done",
            "Mark a plan step as done after you complete it. Keeps the task plan and progress in sync; "
                + "when all steps are done the plan moves to Verifying and waits for the DoD verdict (D0-D7) before completion.",
            "ai.Plan",
            false);
        mark.ParametersSchema = "{\"type\":\"object\",\"properties\":{"
            + "\"index\":{\"type\":\"integer\",\"description\":\"1-based index of the step to mark done.\"}"
            + "},\"required\":[\"index\"]}";
        tools.Add(mark, new AIPlanMarkStepDoneToolHandler(gate));

        AIToolDescriptor revise = new AIToolDescriptor(
            "revise_plan",
            "Revise the task plan when the original plan is rejected or you discover it needs material change "
                + "during execution. Produces a new plan version (v+1) that must be approved again before writes.",
            "ai.Plan",
            false);
        revise.ParametersSchema = "{\"type\":\"object\",\"properties\":{"
            + "\"reason\":{\"type\":\"string\",\"description\":\"Why the plan is being revised (what changed or what was wrong).\"},"
            + "\"goal\":{\"type\":\"string\",\"description\":\"Restated task goal.\"},"
            + "\"analysis\":{\"type\":\"string\",\"description\":\"Revised analysis and strategy.\"},"
            + "\"steps\":{\"type\":\"array\",\"items\":{\"type\":\"string\"},\"description\":\"Revised ordered steps.\"},"
            + "\"verification\":{\"type\":\"string\",\"description\":\"Revised verification criteria.\"}"
            + "},\"required\":[\"reason\",\"goal\",\"steps\"]}";
        tools.Add(revise, new AIPlanReviseToolHandler(gate));
    }
}

/// <summary>plan 工具：创建计划 → Pending + Lint 引导反馈（internal）。</summary>
internal class AIPlanPlanToolHandler : AIToolHandler {
    private AIPlanGate _gate;

    public AIPlanPlanToolHandler(AIPlanGate gate) {
        _gate = gate;
    }

    public override string Name { get { return "plan"; } }

    public override string Capability { get { return "ai.Plan"; } }

    public override async Task<AIToolResult> InvokeAsync(AIToolCall call, CancellationToken cancellationToken) {
        string cid = call != null && call.CallId != null ? call.CallId : "";
        string args = call != null && call.ArgumentsJson != null ? call.ArgumentsJson : "";
        AIToolArgsReader reader = new AIToolArgsReader(args);
        AIPlan plan = _gate.SetPlan(
            reader.GetString("goal"),
            reader.GetString("analysis"),
            AIPlanTools.StepList(reader.GetStringArray("steps")),
            reader.GetString("verification"));
        if (plan == null) {
            return AIToolResult.Fail(cid, "InvalidArgs", "plan: steps must contain at least one concrete step");
        }
        // Lint 引导反馈：问题内联进返回，驱动模型在动手前修订计划。
        string feedback = "plan v" + plan.Revision + " created with " + plan.TotalSteps
            + " steps (status: PENDING APPROVAL). Wait for human approval before using write tools.";
        List<string> issues = plan.Validate();
        if (issues.Count > 0) {
            feedback = feedback + "\nPlan review — please address before execution:";
            int n = issues.Count;
            int i = 0;
            while (i < n) {
                feedback = feedback + "\n- " + issues[i];
                i = i + 1;
            }
        }
        return AIToolResult.Ok(cid, feedback);
    }
}

/// <summary>mark_step_done 工具：标记完成 + 推进指针；满额 → Verifying（完成判定归 DoD，internal）。</summary>
internal class AIPlanMarkStepDoneToolHandler : AIToolHandler {
    private AIPlanGate _gate;

    public AIPlanMarkStepDoneToolHandler(AIPlanGate gate) {
        _gate = gate;
    }

    public override string Name { get { return "mark_step_done"; } }

    public override string Capability { get { return "ai.Plan"; } }

    public override async Task<AIToolResult> InvokeAsync(AIToolCall call, CancellationToken cancellationToken) {
        string cid = call != null && call.CallId != null ? call.CallId : "";
        string args = call != null && call.ArgumentsJson != null ? call.ArgumentsJson : "";
        AIToolArgsReader reader = new AIToolArgsReader(args);
        int index = reader.GetInt("index");
        return AIToolResult.Ok(cid, _gate.MarkStepDone(index));
    }
}

/// <summary>revise_plan 工具：产出修订版 → 回 Pending 重审 + Lint 引导反馈（internal）。</summary>
internal class AIPlanReviseToolHandler : AIToolHandler {
    private AIPlanGate _gate;

    public AIPlanReviseToolHandler(AIPlanGate gate) {
        _gate = gate;
    }

    public override string Name { get { return "revise_plan"; } }

    public override string Capability { get { return "ai.Plan"; } }

    public override async Task<AIToolResult> InvokeAsync(AIToolCall call, CancellationToken cancellationToken) {
        string cid = call != null && call.CallId != null ? call.CallId : "";
        string args = call != null && call.ArgumentsJson != null ? call.ArgumentsJson : "";
        AIToolArgsReader reader = new AIToolArgsReader(args);
        AIPlan plan = _gate.RevisePlan(
            reader.GetString("goal"),
            reader.GetString("analysis"),
            AIPlanTools.StepList(reader.GetStringArray("steps")),
            reader.GetString("verification"));
        if (plan == null) {
            return AIToolResult.Fail(cid, "InvalidArgs", "revise_plan: steps must contain at least one concrete step");
        }
        string feedback = "plan revised to v" + plan.Revision + " (" + plan.TotalSteps
            + " steps, status: PENDING APPROVAL). Wait for re-approval before writing.";
        List<string> issues = plan.Validate();
        if (issues.Count > 0) {
            feedback = feedback + "\nPlan review — please address before execution:";
            int n = issues.Count;
            int i = 0;
            while (i < n) {
                feedback = feedback + "\n- " + issues[i];
                i = i + 1;
            }
        }
        return AIToolResult.Ok(cid, feedback);
    }
}
