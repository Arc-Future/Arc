// RFC 043 subagent-management §4 / §8（A3）：子代理消息——旁路注入 / 打断回合双通道载荷。
// Interruptive=false = 旁路注入（不打断当前回合，下回合边界拼入 prompt delta，不动前缀
// 稳定块）；Interruptive=true = 打断回合（检查点 + 重对齐后恢复，决策同步路径）。
namespace Arc.Agent.Harness;

/// <summary>
/// 子代理消息：非打断注入（soft）/ 打断决策（hard）统一载荷。投递经管理器
/// <c>EnqueueMessageAsync</c> 挂到 <see cref="AISubAgentRun.PendingMessages"/> 邮箱，
/// reconcile 循环在下一次 <c>RunStepAsync</c> 前拼入 prompt delta。
/// </summary>
public class AISubAgentMessage {
    /// <summary>"revision-changed" | "decision-sync" | "work-item-rescope" | "wrap-up"。</summary>
    public string Kind;

    /// <summary>消息关联的 AIRfc 版本（旁路注入的修订上下文；0 = 无）。</summary>
    public int RfcRevision;

    /// <summary>消息载荷（旁路注入正文 / wrap-up 收束指示）。</summary>
    public string Payload;

    /// <summary>true = 打断回合（决策同步）；false = 旁路注入（下回合生效）。</summary>
    public bool Interruptive;

    public AISubAgentMessage() {
        this.Kind = "";
        this.RfcRevision = 0;
        this.Payload = "";
        this.Interruptive = false;
    }
}
