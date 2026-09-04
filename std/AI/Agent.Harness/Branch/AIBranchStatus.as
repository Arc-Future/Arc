// 方案 B B2（conflict-branch §3/§8）：分支生命周期状态 + wire 串编解码。
namespace Arc.Agent.Harness;

/// <summary>分支生命周期状态（Active / Frozen / Merged / Abandoned，conflict-branch §3）。</summary>
public enum AIBranchStatus {
    /// <summary>迭代中（可继续开发 / 合并）。</summary>
    Active,
    /// <summary>冻结窗口（禁合并，与 A.2 /freeze 联动）。</summary>
    Frozen,
    /// <summary>已合并（合并事务 Committed 后终态）。</summary>
    Merged,
    /// <summary>已废弃（撤单 / 废弃终态）。</summary>
    Abandoned,
}

/// <summary>AIBranchStatus 的 wire 串编解码（未知回落 Active）。</summary>
public static class AIBranchStatusCodec {
    /// <summary>转 wire 串（"Active" / "Frozen" / "Merged" / "Abandoned"）。</summary>
    public static string ToWireString(AIBranchStatus status) {
        switch (status) {
            case AIBranchStatus.Active:
            {
                return "Active";
            }
            case AIBranchStatus.Frozen:
            {
                return "Frozen";
            }
            case AIBranchStatus.Merged:
            {
                return "Merged";
            }
            case AIBranchStatus.Abandoned:
            {
                return "Abandoned";
            }
            default:
            {
                return "Active";
            }
        }
    }

    /// <summary>解析 wire 串；未知值回落 Active。</summary>
    public static AIBranchStatus FromWireString(string value) {
        if (value == "Frozen") {
            return AIBranchStatus.Frozen;
        }
        if (value == "Merged") {
            return AIBranchStatus.Merged;
        }
        if (value == "Abandoned") {
            return AIBranchStatus.Abandoned;
        }
        return AIBranchStatus.Active;
    }
}
