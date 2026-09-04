// 方案 B B2（conflict-branch §4/§8）：git 两阶段合并事务 + 预览/门/结局 DTO。
namespace Arc.Agent.Harness;
using Arc.Collections;

/// <summary>
/// 合并事务（conflict-branch §8）：两阶段提交状态机——Staging（merge --no-commit 中间态）
/// | Conflict（三方冲突待裁决）| GateFailed | Ready（合并门全绿）| Committed | Aborted。
/// </summary>
public class AIMergeTransaction {
    public string Id;
    public string SourceBranch;
    public string TargetBranch;
    public string State;
    /// <summary>冲突文件（未合并路径，UU/AA 等；机器只检测登记，裁决唯一入口 = 人 CCB）。</summary>
    public List<string> Conflicts;

    public AIMergeTransaction() {
        this.Id = "";
        this.SourceBranch = "";
        this.TargetBranch = "";
        this.State = AIMergeTransaction.StateStaging();
        this.Conflicts = new List<string>();
    }

    public static string StateStaging() { return "Staging"; }
    public static string StateConflict() { return "Conflict"; }
    public static string StateGateFailed() { return "GateFailed"; }
    public static string StateReady() { return "Ready"; }
    public static string StateCommitted() { return "Committed"; }
    public static string StateAborted() { return "Aborted"; }
}

/// <summary>
/// 合并前预检结果（conflict-branch §5 PreviewAsync）：merge-base + 双方改动文件集 +
/// 重叠（引用图 / 文件集重叠 → 潜在冲突信号）。
/// </summary>
public class AIMergePreview {
    public string BaseCommit;
    public List<string> SourceFiles;
    public List<string> TargetFiles;
    /// <summary>双方改动文件集交集（潜在冲突；非阻断，供登记 / 升级人预裁决）。</summary>
    public List<string> Overlap;
    public bool HasPotentialConflict;

    public AIMergePreview() {
        this.BaseCommit = "";
        this.SourceFiles = new List<string>();
        this.TargetFiles = new List<string>();
        this.Overlap = new List<string>();
        this.HasPotentialConflict = false;
    }
}

/// <summary>合并门结果（conflict-branch §4：合并后完整 D0–D7 汇总门判定）。</summary>
public class AIMergeGateResult {
    public bool Passed;
    /// <summary>汇总门 D0–D7 各门结果（经 AIDoDOrchestrator.RunAutoGatesAsync）。</summary>
    public List<AIDoDGateResult> Gates;
    public string Detail;

    public AIMergeGateResult() {
        this.Passed = false;
        this.Gates = new List<AIDoDGateResult>();
        this.Detail = "";
    }
}

/// <summary>合并结局（Commit / Abort 返回）：Success + 终态 + 明细。</summary>
public class AIMergeOutcome {
    public bool Success;
    public string State;
    public string Detail;

    public AIMergeOutcome() {
        this.Success = false;
        this.State = "";
        this.Detail = "";
    }
}
