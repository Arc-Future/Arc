// 方案 B B2（conflict-branch §4/§8）：git 两阶段合并控制器 — Preview → Begin（merge
// --no-commit）→ RunMergeGate（汇总门 D0–D7）→ Commit（git commit）/ Abort（git merge
// --abort + 分支级绿点回滚兜底）。git 操作统一经 Process.RunCaptureAsync（ProcessStartInfo
// 直调 git，非裸 shell 语义）；只消费 AIDoDOrchestrator（合并门）与 AICheckpointStore
// （绿点兜底），不发明第二套锁。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.IO;

/// <summary>
/// 合并控制器：把跨分支 git 合并落地为两阶段提交（conflict-branch §4）。
/// Stage 1 = <c>git merge --no-commit --no-ff</c>（中间态可整体撤销）；Stage 2 =
/// <c>git commit</c>（原子提交）。任一失败 → <c>git merge --abort</c> 整体回滚 +
/// 分支级绿点兜底（非 git 环境 / index 损坏时）。
/// </summary>
public class AIMergeController {
    private string _root;
    private AIDoDOrchestrator? _dod;
    private AICheckpointStore? _checkpoints;
    private AIBranchLease? _branches;
    private AIRfc? _gateRfc;
    private string _mergeActor;
    private int _seq;

    public AIMergeController(string projectRoot, AIDoDOrchestrator? dod, AICheckpointStore? checkpoints, AIBranchLease? branches) {
        _root = AIMergeController.ResolveRoot(projectRoot);
        _dod = dod;
        _checkpoints = checkpoints;
        _branches = branches;
        _gateRfc = null;
        _mergeActor = "";
        _seq = 0;
    }

    /// <summary>合并门判定基准 AIRfc（RunMergeGateAsync 经 AIDoDOrchestrator 消费）；null = 无基准。</summary>
    public AIRfc? MergeGateRfc {
        get { return _gateRfc; }
        set { _gateRfc = value; }
    }

    /// <summary>合并发起方（合并权校验；空 = 跳过合并权校验）。</summary>
    public string MergeActor {
        get { return _mergeActor; }
        set { _mergeActor = value != null ? value : ""; }
    }

    /// <summary>
    /// 合并前预检（conflict-branch §5 PreviewAsync）：merge-base + 双方改动文件集交集
    /// （潜在冲突信号，非阻断）。不可计算 merge-base（非 git / 无共同祖先）→ BaseCommit 空。
    /// </summary>
    public async Task<AIMergePreview> PreviewAsync(AIBranch source, AIBranch target, CancellationToken cancellationToken) {
        AIMergePreview preview = new AIMergePreview();
        if (source == null || target == null) {
            return preview;
        }
        cancellationToken.ThrowIfCancellationRequested();
        string baseCommit = await this.GitLineAsync(
            "merge-base " + target.BranchName + " " + source.BranchName, cancellationToken);
        preview.BaseCommit = baseCommit;
        if (baseCommit == "") {
            return preview;
        }
        preview.SourceFiles = await this.GitLinesAsync(
            "diff --name-only " + baseCommit + " " + source.BranchName, cancellationToken);
        preview.TargetFiles = await this.GitLinesAsync(
            "diff --name-only " + baseCommit + " " + target.BranchName, cancellationToken);
        preview.Overlap = AIMergeController.Intersect(preview.SourceFiles, preview.TargetFiles);
        preview.HasPotentialConflict = preview.Overlap.Count > 0;
        if (_branches != null) {
            AIBranch? t = _branches.Get(target.BranchName);
            if (t != null) {
                t.BaseCommit = baseCommit;
            }
        }
        return preview;
    }

    /// <summary>
    /// Stage 1：切到 target → <c>git merge --no-commit --no-ff source</c>。合并前先落
    /// pre-merge 基线绿点（无 checkpoints 接线 → 跳过，诚实暴露）。冲突 → 收集未合并文件
    /// 进 <see cref="AIMergeTransaction.Conflicts"/>（机器只检测登记，不选胜者）。
    /// </summary>
    public async Task<AIMergeTransaction> BeginAsync(AIBranch source, AIBranch target, CancellationToken cancellationToken) {
        _seq = _seq + 1;
        AIMergeTransaction tx = new AIMergeTransaction();
        tx.Id = "MTX-" + _seq;
        tx.SourceBranch = source != null ? source.BranchName : "";
        tx.TargetBranch = target != null ? target.BranchName : "";
        if (source == null || target == null) {
            tx.State = AIMergeTransaction.StateAborted();
            return tx;
        }
        cancellationToken.ThrowIfCancellationRequested();
        ProcessRunResult checkout = await this.RunGitAsync(
            "checkout " + AIMergeController.Quote(target.BranchName), cancellationToken);
        if (checkout == null || checkout.ExitCode != 0) {
            tx.State = AIMergeTransaction.StateAborted();
            return tx;
        }
        if (_checkpoints != null) {
            int rev = _gateRfc != null ? _gateRfc.Revision : 0;
            await _checkpoints.CaptureAsync("pre-merge", rev, "", cancellationToken);
        }
        ProcessRunResult merge = await this.RunGitAsync(
            "merge --no-commit --no-ff " + AIMergeController.Quote(source.BranchName), cancellationToken);
        if (merge != null && merge.ExitCode == 0) {
            tx.State = AIMergeTransaction.StateStaging();
            return tx;
        }
        List<string> conflicts = await this.GitLinesAsync("diff --name-only --diff-filter=U", cancellationToken);
        int i = 0;
        while (i < conflicts.Count) {
            tx.Conflicts.Add(conflicts[i]);
            i = i + 1;
        }
        tx.State = tx.Conflicts.Count > 0 ? AIMergeTransaction.StateConflict() : AIMergeTransaction.StateStaging();
        return tx;
    }

    /// <summary>
    /// 合并门（conflict-branch §4）：经 <see cref="AIDoDOrchestrator.RunAutoGatesAsync"/>
    /// 跑合并后完整 D0–D7（Pending ≠ Passed 由 <see cref="AIDoDOrchestrator.AllPassed"/>
    /// 强制）。无 orchestrator / 无基准 AIRfc → 非 Passed（诚实暴露）。
    /// </summary>
    public async Task<AIMergeGateResult> RunMergeGateAsync(AIMergeTransaction tx, CancellationToken cancellationToken) {
        AIMergeGateResult gate = new AIMergeGateResult();
        if (tx == null) {
            gate.Detail = "merge-gate:no-transaction";
            return gate;
        }
        cancellationToken.ThrowIfCancellationRequested();
        if (_dod == null || _gateRfc == null) {
            gate.Detail = "merge-gate:no-orchestrator-or-rfc";
            tx.State = AIMergeTransaction.StateGateFailed();
            return gate;
        }
        List<AIDoDGateResult> results = await _dod.RunAutoGatesAsync(_gateRfc, cancellationToken);
        gate.Gates = results;
        gate.Passed = AIDoDOrchestrator.AllPassed(results);
        if (gate.Passed) {
            gate.Detail = "merge-gate:passed";
            tx.State = AIMergeTransaction.StateReady();
        } else {
            gate.Detail = "merge-gate:failed";
            tx.State = AIMergeTransaction.StateGateFailed();
        }
        return gate;
    }

    /// <summary>
    /// Stage 2：<c>git commit</c>（一次原子提交）。合并权校验（conflict-branch §3：合并权 =
    /// 基线 owner）；成功 → source 分支 MarkMerged + 打合并绿点；失败 → 保持原状态。
    /// </summary>
    public async Task<AIMergeOutcome> CommitAsync(AIMergeTransaction tx, CancellationToken cancellationToken) {
        AIMergeOutcome outcome = new AIMergeOutcome();
        if (tx == null) {
            outcome.Detail = "commit:no-transaction";
            return outcome;
        }
        cancellationToken.ThrowIfCancellationRequested();
        if (_branches != null && _mergeActor != "") {
            if (!_branches.CanMerge(tx.TargetBranch, _mergeActor)) {
                outcome.Detail = "commit:merge-right-denied";
                outcome.State = tx.State;
                return outcome;
            }
        }
        ProcessRunResult r = await this.RunGitAsync(
            "commit -m merge-" + tx.SourceBranch + "-into-" + tx.TargetBranch, cancellationToken);
        if (r == null || r.ExitCode != 0) {
            outcome.Detail = "commit:failed";
            outcome.State = tx.State;
            return outcome;
        }
        tx.State = AIMergeTransaction.StateCommitted();
        if (_branches != null) {
            _branches.MarkMerged(tx.SourceBranch);
        }
        if (_checkpoints != null) {
            int rev = _gateRfc != null ? _gateRfc.Revision : 0;
            await _checkpoints.CaptureAsync("merge-committed", rev, "", cancellationToken);
        }
        outcome.Success = true;
        outcome.State = tx.State;
        outcome.Detail = "commit:merged " + tx.SourceBranch + " -> " + tx.TargetBranch;
        return outcome;
    }

    /// <summary>
    /// 整体回滚（conflict-branch §4）：<c>git merge --abort</c> 回到合并前 index / 工作区；
    /// abort 失败（非 git / index 损坏）→ 分支级绿点兜底快照恢复。成功 = git 或绿点任一回滚生效。
    /// </summary>
    public async Task<AIMergeOutcome> AbortAsync(AIMergeTransaction tx, CancellationToken cancellationToken) {
        AIMergeOutcome outcome = new AIMergeOutcome();
        if (tx == null) {
            outcome.Detail = "abort:no-transaction";
            return outcome;
        }
        cancellationToken.ThrowIfCancellationRequested();
        ProcessRunResult r = await this.RunGitAsync("merge --abort", cancellationToken);
        bool aborted = r != null && r.ExitCode == 0;
        bool rolled = false;
        if (!aborted && _checkpoints != null && _checkpoints.HasSnapshot()) {
            AICheckpointRollbackOutcome rollback = await _checkpoints.RollbackAsync(null, cancellationToken);
            rolled = rollback != null && rollback.Success;
        }
        tx.State = AIMergeTransaction.StateAborted();
        outcome.Success = aborted || rolled;
        outcome.State = tx.State;
        if (aborted) {
            outcome.Detail = "abort:merge-aborted";
        } else if (rolled) {
            outcome.Detail = "abort:greenpoint-rolled-back";
        } else {
            outcome.Detail = "abort:failed";
        }
        return outcome;
    }

    // ── git 封装（ProcessStartInfo 直调，非裸 shell 语义）──

    private async Task<ProcessRunResult> RunGitAsync(string args, CancellationToken cancellationToken) {
        ProcessStartInfo si = new ProcessStartInfo();
        si.FileName = "git";
        si.Arguments = args;
        si.WorkingDirectory = _root;
        try {
            return await Process.RunCaptureAsync(si, cancellationToken);
        } catch (Exception) {
            return null;
        }
    }

    private async Task<string> GitLineAsync(string args, CancellationToken cancellationToken) {
        ProcessRunResult r = await this.RunGitAsync(args, cancellationToken);
        if (r == null || r.ExitCode != 0) {
            return "";
        }
        return r.StandardOutput != null ? r.StandardOutput.Trim() : "";
    }

    private async Task<List<string>> GitLinesAsync(string args, CancellationToken cancellationToken) {
        List<string> outList = new List<string>();
        ProcessRunResult r = await this.RunGitAsync(args, cancellationToken);
        if (r == null || r.ExitCode != 0) {
            return outList;
        }
        string text = r.StandardOutput != null ? r.StandardOutput : "";
        if (text == "") {
            return outList;
        }
        string[] lines = text.Split("\n");
        int i = 0;
        while (i < lines.Length) {
            string line = lines[i] != null ? lines[i].Trim() : "";
            if (line != "") {
                outList.Add(line);
            }
            i = i + 1;
        }
        return outList;
    }

    private static List<string> Intersect(List<string> a, List<string> b) {
        List<string> outList = new List<string>();
        int i = 0;
        while (i < a.Count) {
            if (AIMergeController.Contains(b, a[i]) && !AIMergeController.Contains(outList, a[i])) {
                outList.Add(a[i]);
            }
            i = i + 1;
        }
        return outList;
    }

    private static bool Contains(List<string> list, string value) {
        int i = 0;
        while (i < list.Count) {
            if (list[i] == value) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private static string ResolveRoot(string project) {
        string target = project != null && project != "" ? project : ".";
        string root = target;
        if (File.Exists(target)) {
            string parent = Path.GetDirectoryName(target);
            root = parent != null && parent != "" ? parent : ".";
        }
        return root != null ? root : "";
    }

    private static string Quote(string value) {
        if (value == null) {
            return "\"\"";
        }
        if (value.IndexOf(" ") >= 0) {
            return "\"" + value + "\"";
        }
        return value;
    }
}
