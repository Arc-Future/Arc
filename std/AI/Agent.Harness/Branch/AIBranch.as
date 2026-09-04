// 方案 B B2（conflict-branch §3）：分支实体 — 分支 ↔ AIRfc 映射 + 基线所有权 + 分支级绿点目录。
namespace Arc.Agent.Harness;
using Arc.Collections;

/// <summary>
/// 分支实体（conflict-branch §3）：一个分支承载一个或多个 AIRfc；Owner 为基线所有权
/// （合并权）；CheckpointDir 为分支级绿点隔离目录（arc-checkpoints/&lt;branch&gt;/）。
/// </summary>
public class AIBranch {
    /// <summary>分支名（"feature/&lt;rfcId&gt;-&lt;topic&gt;"）。</summary>
    public string BranchName;
    /// <summary>本分支承载的 AIRfc。</summary>
    public List<string> RfcIds;
    /// <summary>基线所有权（合并权持有者）。</summary>
    public string Owner;
    /// <summary>分叉基点（默认 main）。</summary>
    public string BaseRef;
    /// <summary>merge-base(main, branch) 记录（PreviewAsync 回填）。</summary>
    public string BaseCommit;
    /// <summary>分支状态（Active / Frozen / Merged / Abandoned）。</summary>
    public AIBranchStatus Status;
    /// <summary>分支级绿点目录（target/scratch/arc-checkpoints/&lt;branch&gt;/）。</summary>
    public string CheckpointDir;

    public AIBranch() {
        this.BranchName = "";
        this.RfcIds = new List<string>();
        this.Owner = "";
        this.BaseRef = "main";
        this.BaseCommit = "";
        this.Status = AIBranchStatus.Active;
        this.CheckpointDir = "";
    }
}
