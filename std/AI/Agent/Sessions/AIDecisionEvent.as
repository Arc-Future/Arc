// RFC 038 §13 / RFC 043 M5–M6：会话级决策轨迹事件（append-only 单一决策轨迹）。
// kind 命名与 038 approval 并列：airfc:created | airfc:revised | airfc:rejected |
// checkpoint:green | checkpoint:rollback | work_summary。取代 Harness 独立事件日志双轨。
namespace Arc.Agent;
using Arc;

/// <summary>
/// 一条决策轨迹事件（会话级 append-only 审计单元）。Revision 为 0 表示未关联 AIRfc 版本
/// （版本信息由调用方折进 Detail）；Harness 轨迹经 <see cref="AISession.AppendDecisionEvent"/> 写入。
/// </summary>
public class AIDecisionEvent {
    public AIDecisionEventKind Kind;
    public int Revision;
    public string Detail;
    public string Reason;
    public DateTime At;

    public AIDecisionEvent() {
        this.Kind = AIDecisionEventKind.WorkSummary;
        this.Revision = 0;
        this.Detail = "";
        this.Reason = "";
        this.At = DateTime.Now;
    }

    public static AIDecisionEvent Create(AIDecisionEventKind kind, string detail, string reason, int revision) {
        AIDecisionEvent e = new AIDecisionEvent();
        e.Kind = kind;
        e.Revision = revision;
        e.Detail = detail != null ? detail : "";
        e.Reason = reason != null ? reason : "";
        e.At = DateTime.Now;
        return e;
    }

    /// <summary>导出单行审计文本（kind + 版本 + 详情 + 原因；kind 走 wire 串保持既有落盘面）。</summary>
    public string ToLine() {
        return AIDecisionEventKindCodec.ToWireString(this.Kind) + " v" + this.Revision + " " + this.Detail
            + (this.Reason != "" ? " | " + this.Reason : "");
    }
}
