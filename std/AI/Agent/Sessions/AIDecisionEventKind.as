// RFC 038 §13 / RFC 043 M5–M6：会话级决策轨迹事件种类（强类型枚举）。
// kind 命名与 038 approval 并列：airfc:created | airfc:revised | airfc:rejected |
// airfc:clarify | airfc:closed | airfc:cancelled | checkpoint:green | checkpoint:rollback |
// work_summary。
// Arc 枚举不支持方法，wire 串编解码收敛于本文件配套静态类；wire 串保持既有 JSON 落盘面。
namespace Arc.Agent;

/// <summary>决策轨迹事件种类。</summary>
public enum AIDecisionEventKind {
    /// <summary>AIRfc 创建。</summary>
    AirfcCreated,
    /// <summary>AIRfc 纠偏升版。</summary>
    AirfcRevised,
    /// <summary>AIRfc 被拒（方向回退）。</summary>
    AirfcRejected,
    /// <summary>绿点（checkpoint 通过）。</summary>
    CheckpointGreen,
    /// <summary>回滚绿点信号。</summary>
    CheckpointRollback,
    /// <summary>工作单元小结。</summary>
    WorkSummary,
    /// <summary>需求澄清（场景 1.1 澄清向导；用户对验收/边界/是否 refine 的答复）。</summary>
    AirfcClarify,
    /// <summary>人机审批决策（与 038 approval 并列）。</summary>
    Approval,
    /// <summary>AIRfc 收口关闭（D7 通过后禁再 Revise 的终态）。</summary>
    AirfcClosed,
    /// <summary>AIRfc 撤单（终态）。</summary>
    AirfcCancelled,
    /// <summary>子代理中断（撤单收束；A2 subagent-management）。</summary>
    SubagentInterrupt,
    /// <summary>子代理决策同步（广播/定向重对齐；A3 subagent-management）。</summary>
    SubagentSync,
    /// <summary>子代理成本用量上报（token/轮次统计；A5 subagent-management 成本核算）。</summary>
    SubagentUsage,
    /// <summary>L2 Spec 矛盾检测（B1：同 acceptance 项被反方向覆盖 → Contested + 冲突记录）。</summary>
    ConflictDetected,
    /// <summary>人 CCB 裁决冲突（B1：冲突记录 Resolved）。</summary>
    ConflictResolved,
    /// <summary>冲突被否（B1：冲突记录 Rejected + AIRfc → Rejected）。</summary>
    ConflictRejected,
    /// <summary>冲突裁决落新基线（B1：Contested → Active 新 Revision，airfc:resolved）。</summary>
    AirfcResolved,
}

/// <summary>AIDecisionEventKind 的 wire 串编解码（未知回落 WorkSummary）。</summary>
public static class AIDecisionEventKindCodec {
    /// <summary>转 wire 串（"airfc:created" / "airfc:revised" / "airfc:rejected" /
    /// "airfc:clarify" / "airfc:closed" / "airfc:cancelled" / "checkpoint:green" /
    /// "checkpoint:rollback" / "work_summary" / "approval" / "subagent:interrupt" /
    /// "subagent:sync" / "subagent:usage" / "conflict:detected" / "conflict:resolved" / "conflict:rejected" /
    /// "airfc:resolved"）。</summary>
    public static string ToWireString(AIDecisionEventKind kind) {
        switch (kind) {
            case AIDecisionEventKind.AirfcCreated:
            {
                return "airfc:created";
            }
            case AIDecisionEventKind.AirfcRevised:
            {
                return "airfc:revised";
            }
            case AIDecisionEventKind.AirfcRejected:
            {
                return "airfc:rejected";
            }
            case AIDecisionEventKind.CheckpointGreen:
            {
                return "checkpoint:green";
            }
            case AIDecisionEventKind.CheckpointRollback:
            {
                return "checkpoint:rollback";
            }
            case AIDecisionEventKind.AirfcClarify:
            {
                return "airfc:clarify";
            }
            case AIDecisionEventKind.Approval:
            {
                return "approval";
            }
            case AIDecisionEventKind.AirfcClosed:
            {
                return "airfc:closed";
            }
            case AIDecisionEventKind.AirfcCancelled:
            {
                return "airfc:cancelled";
            }
            case AIDecisionEventKind.SubagentInterrupt:
            {
                return "subagent:interrupt";
            }
            case AIDecisionEventKind.SubagentSync:
            {
                return "subagent:sync";
            }
            case AIDecisionEventKind.SubagentUsage:
            {
                return "subagent:usage";
            }
            case AIDecisionEventKind.ConflictDetected:
            {
                return "conflict:detected";
            }
            case AIDecisionEventKind.ConflictResolved:
            {
                return "conflict:resolved";
            }
            case AIDecisionEventKind.ConflictRejected:
            {
                return "conflict:rejected";
            }
            case AIDecisionEventKind.AirfcResolved:
            {
                return "airfc:resolved";
            }
            default:
            {
                return "work_summary";
            }
        }
    }

    /// <summary>解析 wire 串；未知值回落 WorkSummary。</summary>
    public static AIDecisionEventKind FromWireString(string value) {
        if (value == "airfc:created") {
            return AIDecisionEventKind.AirfcCreated;
        }
        if (value == "airfc:revised") {
            return AIDecisionEventKind.AirfcRevised;
        }
        if (value == "airfc:rejected") {
            return AIDecisionEventKind.AirfcRejected;
        }
        if (value == "checkpoint:green") {
            return AIDecisionEventKind.CheckpointGreen;
        }
        if (value == "checkpoint:rollback") {
            return AIDecisionEventKind.CheckpointRollback;
        }
        if (value == "airfc:clarify") {
            return AIDecisionEventKind.AirfcClarify;
        }
        if (value == "approval") {
            return AIDecisionEventKind.Approval;
        }
        if (value == "airfc:closed") {
            return AIDecisionEventKind.AirfcClosed;
        }
        if (value == "airfc:cancelled") {
            return AIDecisionEventKind.AirfcCancelled;
        }
        if (value == "subagent:interrupt") {
            return AIDecisionEventKind.SubagentInterrupt;
        }
        if (value == "subagent:sync") {
            return AIDecisionEventKind.SubagentSync;
        }
        if (value == "subagent:usage") {
            return AIDecisionEventKind.SubagentUsage;
        }
        if (value == "conflict:detected") {
            return AIDecisionEventKind.ConflictDetected;
        }
        if (value == "conflict:resolved") {
            return AIDecisionEventKind.ConflictResolved;
        }
        if (value == "conflict:rejected") {
            return AIDecisionEventKind.ConflictRejected;
        }
        if (value == "airfc:resolved") {
            return AIDecisionEventKind.AirfcResolved;
        }
        return AIDecisionEventKind.WorkSummary;
    }
}
