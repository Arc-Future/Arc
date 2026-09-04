// RFC 043 P3（subagent-management A2）：子代理运行容器精化生命周期状态。
// 与 AITaskRunStatus（Pending/Running/Paused/Completed/Failed/Cancelled）映射：
//   Spawned ↔ 已 Spawn 未 Start（AITaskRun.Pending）
//   Interrupted ↔ 被撤单收束中断（AITaskRun.Cancelled + interrupt 标记）
//   Dead ↔ 已回收（会话租约释放后容器不可再用）
// 其余状态与 AITaskRunStatus 一一对应。wire 串编解码收敛于本文件配套静态类。
namespace Arc.Agent.Harness;

/// <summary>AISubAgentRun 生命周期精化状态。</summary>
public enum AISubAgentState {
    /// <summary>已构造未 Spawn。</summary>
    Pending,
    /// <summary>已派发（Spawn）未 Start（AITaskRun.Pending）。</summary>
    Spawned,
    /// <summary>执行中（AITaskRun.Running）。</summary>
    Running,
    /// <summary>被中断（撤单/纠偏收束；AITaskRun.Cancelled + interrupt 标记）。</summary>
    Interrupted,
    /// <summary>已暂停（AITaskRun.Paused，快照续跑）。</summary>
    Paused,
    /// <summary>正常完结（AITaskRun.Completed）。</summary>
    Completed,
    /// <summary>失败（AITaskRun.Failed）。</summary>
    Failed,
    /// <summary>取消（AITaskRun.Cancelled，非 interrupt 路径）。</summary>
    Cancelled,
    /// <summary>已回收（会话租约已释放，容器不可再用）。</summary>
    Dead,
}

/// <summary>AISubAgentState 的 wire 串编解码（未知回落 Pending）。</summary>
public static class AISubAgentStatusCodec {
    /// <summary>转 wire 串（"Pending" / "Spawned" / "Running" / "Interrupted" / "Paused" /
    /// "Completed" / "Failed" / "Cancelled" / "Dead"）。</summary>
    public static string ToWireString(AISubAgentState state) {
        switch (state) {
            case AISubAgentState.Spawned:
            {
                return "Spawned";
            }
            case AISubAgentState.Running:
            {
                return "Running";
            }
            case AISubAgentState.Interrupted:
            {
                return "Interrupted";
            }
            case AISubAgentState.Paused:
            {
                return "Paused";
            }
            case AISubAgentState.Completed:
            {
                return "Completed";
            }
            case AISubAgentState.Failed:
            {
                return "Failed";
            }
            case AISubAgentState.Cancelled:
            {
                return "Cancelled";
            }
            case AISubAgentState.Dead:
            {
                return "Dead";
            }
            default:
            {
                return "Pending";
            }
        }
    }

    /// <summary>解析 wire 串；未知值回落 Pending。</summary>
    public static AISubAgentState FromWireString(string value) {
        if (value == "Spawned") {
            return AISubAgentState.Spawned;
        }
        if (value == "Running") {
            return AISubAgentState.Running;
        }
        if (value == "Interrupted") {
            return AISubAgentState.Interrupted;
        }
        if (value == "Paused") {
            return AISubAgentState.Paused;
        }
        if (value == "Completed") {
            return AISubAgentState.Completed;
        }
        if (value == "Failed") {
            return AISubAgentState.Failed;
        }
        if (value == "Cancelled") {
            return AISubAgentState.Cancelled;
        }
        if (value == "Dead") {
            return AISubAgentState.Dead;
        }
        return AISubAgentState.Pending;
    }
}
