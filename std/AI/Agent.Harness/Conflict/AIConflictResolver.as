// 方案 B（conflict-branch §5/§8）：冲突仲裁 — 机器只检测与登记（Record），裁决唯一入口 =
// 人 CCB（ResolveAsync 必须 resolvedBy；RejectAsync 拒绝）。裁决后写新 Revision 基线 +
// 状态联动。挂 AIRfcRuntime（共享于绑定同一运行时/协调器的全部会话）。
namespace Arc.Agent.Harness;
using Arc.Collections;

/// <summary>
/// 冲突仲裁器（挂 <see cref="AIRfcRuntime"/>，绑定同一运行时/协调器的会话共享同一记录表）。
/// L2 检测由 <see cref="AISpecConflictDetector"/> 完成，本类只登记与裁决；ResolveAsync 以
/// 人为前置（<paramref name="resolvedBy"/> 空 → false，禁自动选胜者）。
/// </summary>
public class AIConflictResolver {
    private List<AIConflictRecord> _records;
    private AIRfcRuntime _runtime;
    private int _seq;

    public AIConflictResolver(AIRfcRuntime runtime) {
        _runtime = runtime;
        _records = new List<AIConflictRecord>();
        _seq = 0;
    }

    /// <summary>全部 Open 冲突（/conflict 列表）。</summary>
    public List<AIConflictRecord> Open() {
        List<AIConflictRecord> outList = new List<AIConflictRecord>();
        int i = 0;
        int n = _records.Count;
        while (i < n) {
            AIConflictRecord r = _records[i];
            if (r != null && r.IsOpen) {
                outList.Add(r);
            }
            i = i + 1;
        }
        return outList;
    }

    /// <summary>全部冲突记录（含已处理，审计面）。</summary>
    public List<AIConflictRecord> All() {
        List<AIConflictRecord> outList = new List<AIConflictRecord>();
        int i = 0;
        int n = _records.Count;
        while (i < n) {
            if (_records[i] != null) {
                outList.Add(_records[i]);
            }
            i = i + 1;
        }
        return outList;
    }

    /// <summary>按 id 查找；无则 null。</summary>
    public AIConflictRecord? Find(string conflictId) {
        if (conflictId == null || conflictId == "") {
            return null;
        }
        int i = 0;
        int n = _records.Count;
        while (i < n) {
            AIConflictRecord r = _records[i];
            if (r != null && r.ConflictId == conflictId) {
                return r;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>登记冲突（RFC §5 Record(kind, detail)）。</summary>
    public AIConflictRecord Record(AIConflictKind kind, string detail) {
        AIConflictRecord rec = new AIConflictRecord();
        _seq = _seq + 1;
        rec.ConflictId = "CONF-" + _seq;
        rec.Kind = kind;
        rec.Evidence = detail != null ? detail : "";
        _records.Add(rec);
        return rec;
    }

    /// <summary>
    /// 登记 L2 Spec 矛盾（检测结果 → 记录；含双方 Acceptance 快照供裁决新基线）。
    /// </summary>
    public AIConflictRecord RecordSpecContradiction(
        string rfcId,
        int revision,
        string sourceA,
        string sourceB,
        List<string> resources,
        string evidence,
        AIAcceptanceSpec beforeAcceptance,
        AIAcceptanceSpec afterAcceptance) {
        AIConflictRecord rec = new AIConflictRecord();
        _seq = _seq + 1;
        rec.ConflictId = "CONF-" + _seq;
        rec.Kind = AIConflictKind.SpecContradiction;
        rec.RfcId = rfcId != null ? rfcId : "";
        rec.Revision = revision;
        rec.Evidence = evidence != null ? evidence : "";
        rec.BeforeAcceptance = beforeAcceptance != null ? beforeAcceptance : new AIAcceptanceSpec();
        rec.AfterAcceptance = afterAcceptance != null ? afterAcceptance : new AIAcceptanceSpec();
        if (resources != null) {
            int i = 0;
            int n = resources.Count;
            while (i < n) {
                rec.Resources.Add(resources[i]);
                i = i + 1;
            }
        }
        rec.Parties.Add(sourceA != null ? sourceA : "");
        rec.Parties.Add(sourceB != null ? sourceB : "");
        _records.Add(rec);
        return rec;
    }

    /// <summary>
    /// 人 CCB 裁决（唯一入口，RFC §5）：<paramref name="resolvedBy"/> 空 → false（机器不可
    /// 自动选胜者）；记录须 Open 且 AIRfc 处于 Contested。裁决后 Contested → 新 Revision
    /// 基线（<paramref name="decision"/> 前缀 "accept-after" 采纳被拦截方向，否则维持冲突前
    /// 方向）+ 记录 Resolved。返回是否生效。
    /// </summary>
    public bool ResolveAsync(string conflictId, string decision, string reason, string resolvedBy) {
        AIConflictRecord? rec = this.Find(conflictId);
        if (rec == null || !rec.IsOpen) {
            return false;
        }
        if (resolvedBy == null || resolvedBy == "") {
            return false;
        }
        if (rec.RfcId == null || rec.RfcId == "") {
            return false;
        }
        AIAcceptanceSpec winner = rec.BeforeAcceptance;
        if (decision != null && decision.StartsWith("accept-after")) {
            winner = rec.AfterAcceptance;
        }
        AIRfc? next = _runtime.ResolveContestedWithSpec(rec.RfcId, winner, resolvedBy);
        if (next == null) {
            return false;
        }
        rec.Status = AIConflictRecord.StatusResolved();
        rec.Decision = decision != null ? decision : "";
        rec.ResolvedBy = resolvedBy;
        return true;
    }

    /// <summary>拒绝冲突（RFC §5 RejectAsync）：记录 Rejected + AIRfc Contested → Rejected。</summary>
    public bool RejectAsync(string conflictId, string reason) {
        return this.RejectAsync(conflictId, reason, "");
    }

    /// <summary>拒绝冲突（带裁决人）：语义同上，<paramref name="resolvedBy"/> 留痕。</summary>
    public bool RejectAsync(string conflictId, string reason, string resolvedBy) {
        AIConflictRecord? rec = this.Find(conflictId);
        if (rec == null || !rec.IsOpen) {
            return false;
        }
        if (rec.RfcId == null || rec.RfcId == "") {
            return false;
        }
        if (!_runtime.RejectContested(rec.RfcId)) {
            return false;
        }
        rec.Status = AIConflictRecord.StatusRejected();
        rec.ResolvedBy = resolvedBy != null ? resolvedBy : "";
        return true;
    }
}
