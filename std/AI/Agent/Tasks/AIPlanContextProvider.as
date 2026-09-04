// RFC 038 M8.2：AIPlanContextProvider — 任务计划上下文源（Task 层注入）。
//
// 把当前 `AIPlan` 格式化为 `Task` 层上下文块注入模型请求，排在 Rules（系统指令）之后、
// UserProfile 之前（AIContextEngine 固定顺序），保持前缀稳定 → KV cache 命中。
// 无计划（尚未创建 / 已清除）→ 不贡献上下文。
//
// 与 AIPlanGate 的关联：本 provider 是「当前计划」的单一持有者；AIPlanGate（门闩/审批）
// 与内置 plan 工具经同一引用读写计划，应用侧展示（/plan）同源可见。
namespace Arc.Agent;
using Arc;

/// <summary>
/// 内置上下文源：当前任务计划。按需注入折叠后的 markdown（已完成步骤折叠为单行）；
/// 无计划时不贡献上下文。
/// </summary>
public class AIPlanContextProvider : AIContextProvider {
    private AIPlan _plan;

    public override string GetName() {
        return "task-plan";
    }

    public override string GetDescription() {
        return "Current task plan (Task layer, after instructions).";
    }

    public override int GetPriority() {
        return 10;
    }

    /// <summary>设置（存储）当前计划；更新后下次请求模型可见。</summary>
    public void SetPlan(AIPlan plan) {
        _plan = plan;
    }

    /// <summary>读取当前计划；null = 无计划。</summary>
    public AIPlan GetPlan() {
        return _plan;
    }

    /// <summary>清除当前计划（新任务开始时调用）。</summary>
    public void ClearPlan() {
        _plan = null;
    }

    /// <summary>当前是否有已设置的计划。</summary>
    public bool HasPlan {
        get { return _plan != null; }
    }

    public override Task<List<AIContextBlock>> ProvideContextAsync(AIContextQuery query, AIContextSession session, CancellationToken cancellationToken) {
        List<AIContextBlock> result = new List<AIContextBlock>();
        if (_plan == null || _plan.TotalSteps == 0) {
            return Task.FromResult(result);
        }
        result.Add(new AIContextBlock("task-plan", "Task", GetPriority(), _plan.ToMarkdown()));
        return Task.FromResult(result);
    }
}
