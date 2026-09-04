// RFC 043 H-2c：AIRfc 工作项状态（强类型枚举）。
// Arc 枚举不支持方法，wire 串编解码收敛于本文件配套静态类。
namespace Arc.Agent.Harness;

/// <summary>AIRfcWorkItem 生命周期状态（Open / InProgress / Blocked / Done / Failed / Cancelled）。</summary>
public enum AIRfcWorkItemStatus {
    /// <summary>已登记未启动。</summary>
    Open,
    /// <summary>进行中。</summary>
    InProgress,
    /// <summary>受阻（等待外部解除）。</summary>
    Blocked,
    /// <summary>已完成。</summary>
    Done,
    /// <summary>已失败（终态；不进就绪面、不计 remaining，失败信号持久承载跨会话可查）。</summary>
    Failed,
    /// <summary>已取消（撤单收束；不进下一波，不再执行）。</summary>
    Cancelled,
}

/// <summary>AIRfcWorkItemStatus 的 wire 串编解码（未知回落 Open）。</summary>
public static class AIRfcWorkItemStatusCodec {
    /// <summary>转 wire 串（"Open" / "InProgress" / "Blocked" / "Done" / "Failed" / "Cancelled"）。</summary>
    public static string ToWireString(AIRfcWorkItemStatus status) {
        switch (status) {
            case AIRfcWorkItemStatus.Open:
            {
                return "Open";
            }
            case AIRfcWorkItemStatus.InProgress:
            {
                return "InProgress";
            }
            case AIRfcWorkItemStatus.Blocked:
            {
                return "Blocked";
            }
            case AIRfcWorkItemStatus.Done:
            {
                return "Done";
            }
            case AIRfcWorkItemStatus.Failed:
            {
                return "Failed";
            }
            case AIRfcWorkItemStatus.Cancelled:
            {
                return "Cancelled";
            }
            default:
            {
                return "Open";
            }
        }
    }

    /// <summary>解析 wire 串；未知值回落 Open。</summary>
    public static AIRfcWorkItemStatus FromWireString(string value) {
        if (value == "InProgress") {
            return AIRfcWorkItemStatus.InProgress;
        }
        if (value == "Blocked") {
            return AIRfcWorkItemStatus.Blocked;
        }
        if (value == "Done") {
            return AIRfcWorkItemStatus.Done;
        }
        if (value == "Failed") {
            return AIRfcWorkItemStatus.Failed;
        }
        if (value == "Cancelled") {
            return AIRfcWorkItemStatus.Cancelled;
        }
        return AIRfcWorkItemStatus.Open;
    }
}
