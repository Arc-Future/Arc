// RFC 043 H-2c：AIRfc 生命周期状态（强类型枚举；完整边表见 airfc §4）。
// Arc 枚举不支持方法，wire 串编解码收敛于本文件配套静态类。
namespace Arc.Agent.Harness;

/// <summary>AIRfc 运行态（Active / Superseded / Rejected / Contested / Frozen / Closed / Cancelled）。</summary>
public enum AIRfcStatus {
    /// <summary>当前有效版本（唯一可修订）。</summary>
    Active,
    /// <summary>被更新 Revision 取代；只读审计。</summary>
    Superseded,
    /// <summary>用户拒绝；须新 Revision 再入 Active。</summary>
    Rejected,
    /// <summary>多来源需求冲突（A.1）；须先 ResolveContested 再回 Active。</summary>
    Contested,
    /// <summary>冻结窗口（A.2）：禁修订/拒绝，可 UnfreezeRfc 解冻。</summary>
    Frozen,
    /// <summary>收口关闭终态（D7 通过后）；禁再 Revise/Reject。</summary>
    Closed,
    /// <summary>撤单终态；WIP/绿点处置按场景协议（keep-wip / rollback）。</summary>
    Cancelled,
}

/// <summary>AIRfcStatus 的 wire 串编解码（未知回落 Active）。</summary>
public static class AIRfcStatusCodec {
    /// <summary>转 wire 串（"Active" / "Superseded" / "Rejected" / "Contested" / "Frozen" / "Closed" / "Cancelled"）。</summary>
    public static string ToWireString(AIRfcStatus status) {
        switch (status) {
            case AIRfcStatus.Active:
            {
                return "Active";
            }
            case AIRfcStatus.Superseded:
            {
                return "Superseded";
            }
            case AIRfcStatus.Rejected:
            {
                return "Rejected";
            }
            case AIRfcStatus.Contested:
            {
                return "Contested";
            }
            case AIRfcStatus.Frozen:
            {
                return "Frozen";
            }
            case AIRfcStatus.Closed:
            {
                return "Closed";
            }
            case AIRfcStatus.Cancelled:
            {
                return "Cancelled";
            }
            default:
            {
                return "Active";
            }
        }
    }

    /// <summary>解析 wire 串；未知值回落 Active。</summary>
    public static AIRfcStatus FromWireString(string value) {
        if (value == "Superseded") {
            return AIRfcStatus.Superseded;
        }
        if (value == "Rejected") {
            return AIRfcStatus.Rejected;
        }
        if (value == "Contested") {
            return AIRfcStatus.Contested;
        }
        if (value == "Frozen") {
            return AIRfcStatus.Frozen;
        }
        if (value == "Closed") {
            return AIRfcStatus.Closed;
        }
        if (value == "Cancelled") {
            return AIRfcStatus.Cancelled;
        }
        return AIRfcStatus.Active;
    }
}
