// RFC 043 场景 1.1 E 面修复：AIRfcContextProvider —— AIRfc 锚点活上下文源。
//
// 把当前 Active AIRfc 折叠（Intention/Design/Acceptance/Revision/Plan 摘要，
// AIRfc.ToContextBlock）作为 Rules 层上下文块注入模型请求，排在系统指令之后、
// 知识/计划等动态面之前（Rules 层固定序）。活源：每次 BuildAsync 读取
// AIHarnessSession.Rfc——/rfc /revise 后锚点自动进入模型 Instructions 上下文；
// 块内容仅随 AIRfc Revision 变更（前缀稳定 → KV cache 命中），不做每轮重注入。
//
// 这是对 AttachRfcToInstructions 字符串快照式接线的等价替代（组合根注册，经
// AIContextEngine 统一 provider 管道），避免指令字符串在宿主创建后失效的问题。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Agent;

/// <summary>
/// AIRfc 锚点活上下文源：当前 Active RFC 折叠为 Rules 层块（有 RFC 才贡献；
/// 无则静默跳过）。经 <see cref="AIContextEngine.AddProvider"/> 在组合根注册，
/// 跨会话共享实例、零会话态字段（RFC 038 provider 契约）。
/// </summary>
public class AIRfcContextProvider : AIContextProvider {
    private AIHarnessSession _harness;

    public AIRfcContextProvider(AIHarnessSession harness) {
        _harness = harness;
    }

    public override string GetName() {
        return "airfc";
    }

    public override string GetDescription() {
        return "AIRfc anchor (Rules layer; updated on rfc/revise).";
    }

    /// <summary>Rules 层布局优先级：紧随系统指令（指令 0 → 锚点 1），先于动态面。</summary>
    public override int GetPriority() {
        return 1;
    }

    public override Task<List<AIContextBlock>> ProvideContextAsync(
        AIContextQuery query, AIContextSession session, CancellationToken cancellationToken) {
        List<AIContextBlock> list = new List<AIContextBlock>();
        AIRfc? rfc = _harness != null ? _harness.Rfc : null;
        if (rfc != null) {
            list.Add(new AIContextBlock("airfc", "Rules", this.GetPriority(), rfc.ToContextBlock()));
        }
        return Task.FromResult(list);
    }
}
