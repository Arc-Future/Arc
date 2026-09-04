// 方案 B B2（conflict-branch §3/§8）：分支租约 + 登记表 — 同名唯一 + 合并权（Owner）。
namespace Arc.Agent.Harness;
using Arc.Collections;

/// <summary>
/// 分支租约 + 登记表（conflict-branch §3/§8）：同名分支唯一（Create 拒绝重名）+ 合并权
/// （Owner = 持约者，HolderOf 查询；AIMergeController 据此校验合并权）。单进程登记表，
/// 不发明第二套锁（跨进程租约属 B4，见 conflict-branch §6）。
/// </summary>
public class AIBranchLease {
    private List<AIBranch> _branches;

    public AIBranchLease() {
        _branches = new List<AIBranch>();
    }

    /// <summary>创建分支；同名已存在 → null（同名唯一）。CheckpointDir 按分支隔离。</summary>
    public AIBranch? Create(string branchName, List<string>? rfcIds, string owner, string baseRef) {
        if (branchName == null || branchName == "") {
            return null;
        }
        if (this.Find(branchName) != null) {
            return null;
        }
        AIBranch b = new AIBranch();
        b.BranchName = branchName;
        if (rfcIds != null) {
            int i = 0;
            while (i < rfcIds.Count) {
                b.RfcIds.Add(rfcIds[i]);
                i = i + 1;
            }
        }
        b.Owner = owner != null ? owner : "";
        b.BaseRef = baseRef != null && baseRef != "" ? baseRef : "main";
        b.BaseCommit = "";
        b.Status = AIBranchStatus.Active;
        b.CheckpointDir = "target/scratch/arc-checkpoints/" + branchName;
        _branches.Add(b);
        return b;
    }

    /// <summary>按名取分支；无 → null。</summary>
    public AIBranch? Get(string branchName) {
        return this.Find(branchName);
    }

    /// <summary>全部登记分支。</summary>
    public List<AIBranch> All() {
        List<AIBranch> outList = new List<AIBranch>();
        int i = 0;
        while (i < _branches.Count) {
            if (_branches[i] != null) {
                outList.Add(_branches[i]);
            }
            i = i + 1;
        }
        return outList;
    }

    /// <summary>分支当前持约者（Owner，合并权持有者）；无 → 空串。</summary>
    public string HolderOf(string branchName) {
        AIBranch? b = this.Find(branchName);
        return b != null ? b.Owner : "";
    }

    /// <summary>合并权校验：指定持有者是否拥有该分支（Owner 匹配；分支不存在 → false）。</summary>
    public bool CanMerge(string branchName, string holder) {
        string owner = this.HolderOf(branchName);
        string h = holder != null ? holder : "";
        return owner != "" && owner == h;
    }

    /// <summary>状态迁移（Active/Frozen/Merged/Abandoned）；分支不存在 → null。</summary>
    public AIBranch? SetStatus(string branchName, AIBranchStatus status) {
        AIBranch? b = this.Find(branchName);
        if (b == null) {
            return null;
        }
        b.Status = status;
        return b;
    }

    /// <summary>标记已合并（→ Merged）。</summary>
    public AIBranch? MarkMerged(string branchName) {
        return this.SetStatus(branchName, AIBranchStatus.Merged);
    }

    /// <summary>标记废弃（→ Abandoned）。</summary>
    public AIBranch? MarkAbandoned(string branchName) {
        return this.SetStatus(branchName, AIBranchStatus.Abandoned);
    }

    private AIBranch? Find(string branchName) {
        int i = 0;
        while (i < _branches.Count) {
            AIBranch b = _branches[i];
            if (b != null && b.BranchName == branchName) {
                return b;
            }
            i = i + 1;
        }
        return null;
    }
}
