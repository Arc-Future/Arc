// DirectionLoop —— H-4 方向环命令（/rfc /revise /reject <reason> /summary /checkpoint /rollback
// /dod，含 D5 自审槽位 + D7 一次人验收归集）。方向环逻辑一律经 AIHarnessSession 走基座，
// 本类只做薄组装 + 交互展示（不重复实现 PM/DoD）。
//
// 流程：/rfc 立项（SetRfc，Design/Acceptance 空缺 → 澄清向导追问验收/边界并落 Spec）→
//   /revise 升版纠偏（ReviseRfc，附 acceptance/design 补齐提示）→ /summary 小结判定
//   （RecordSummary）→ /checkpoint 绿点快照 / /rollback 回滚 → /dod 全门 + D5 证明槽位
//   + D7 一次人验收（接受 → CompletePlanAfterDoDAsync 受控完成；拒绝 → 记录原因停留 Verifying）。
// Acceptance 先行门闩：无验收断言 → AttachPlan 被拒（先补齐再进计划）。
namespace ArcAgent.Repl;
using Arc;
using Arc.Agent;
using Arc.Agent.Harness;
using ArcAgent.SessionLog;
using ArcAgent.Workspace;

/// <summary>方向环斜杠命令：命中返回 true（已处理），否则返回 false。</summary>
public class DirectionLoop {
    private AIHarnessSession _harness;
    private AgentWorkspace _workspace;
    private SessionEventLog _log;
    private AIPlanGate? _planGate;
    private D5SelfReview _d5;
    private ReplFixRoundProvider? _fixProvider;
    private AISession? _session;
    private int _rfcCounter;

    public DirectionLoop(AIHarnessSession harness, AgentWorkspace workspace, SessionEventLog log, AIPlanGate? planGate, AISession? session) {
        _harness = harness;
        _workspace = workspace;
        _log = log;
        _planGate = planGate;
        _d5 = new D5SelfReview();
        // D5 证明机器校验基（场景 4.1）：证明文件引用 / `arc test --list-tests` 以工作区根为解析基。
        _d5.SetProjectRoot(workspace != null ? workspace.Root : "");
        _fixProvider = session != null ? new ReplFixRoundProvider(session) : null;
        // B1 /conflict 裁决人身份基：CCB 身份优先取会话 id，无会话兜底 "ccb"。
        _session = session;
        _rfcCounter = 0;
        // Acceptance 先行门闩（RFC 043 场景 1.1）：未定义验收断言 → AttachPlan 被拒，
        // 强制「先补齐验收再进计划」。经 /revise --acceptance= 解锁。
        _harness.EnableAcceptanceGate();
    }

    /// <summary>尝试处理方向环斜杠命令；未命中返回 false。</summary>
    public async Task<bool> TryHandleAsync(string trimmed) {
        if (trimmed == "/rfc") {
            Console.WriteLine("usage: /rfc <一句话意图> — 立项一个 AIRfc（Revision 1）");
            return true;
        }
        if (trimmed.StartsWith("/rfc ")) {
            await this.RfcAsync(trimmed.Substring("/rfc ".Length).Trim());
            return true;
        }
        if (trimmed.StartsWith("/revise")) {
            await this.ReviseAsync(trimmed.Substring("/revise".Length).Trim());
            return true;
        }
        if (trimmed.StartsWith("/reject ")) {
            await this.RejectAsync(trimmed.Substring("/reject ".Length).Trim());
            return true;
        }
        if (trimmed == "/summary") {
            await this.SummaryAsync();
            return true;
        }
        if (trimmed == "/checkpoint" || trimmed.StartsWith("/checkpoint ")) {
            await this.CheckpointAsync(trimmed.Substring("/checkpoint".Length).Trim());
            return true;
        }
        if (trimmed == "/rollback" || trimmed.StartsWith("/rollback ")) {
            await this.RollbackAsync(trimmed.Substring("/rollback".Length).Trim());
            return true;
        }
        if (trimmed == "/dod") {
            await this.DodAsync();
            return true;
        }
        if (trimmed.StartsWith("/dod ")) {
            await this.DodSubAsync(trimmed.Substring("/dod ".Length).Trim());
            return true;
        }
        if (trimmed == "/save") {
            await this.SaveStateAsync();
            return true;
        }
        if (trimmed == "/conflict" || trimmed.StartsWith("/conflict ")) {
            await this.ConflictAsync(trimmed.Substring("/conflict".Length).Trim());
            return true;
        }
        return false;
    }

    // ── 立项：/rfc <意图> ──

    private async Task RfcAsync(string intention) {
        if (intention == "") {
            Console.WriteLine("usage: /rfc <一句话意图> — 立项一个 AIRfc（Revision 1）");
            return;
        }
        _rfcCounter = _rfcCounter + 1;
        string rfcId = "RFC-" + _rfcCounter;
        AIRfc? created = _harness.SetRfc(
            rfcId, new AIIntentionSpec(intention), new AIDesignSpec(), new AIAcceptanceSpec());
        if (created == null) {
            Console.WriteLine("[rfc] 立项被拒（RfcSpec 租约冲突或重复 RfcId）");
            return;
        }
        _d5.Reset(created);
        Console.WriteLine("[rfc] " + created.RfcId + " v" + created.Revision + " 已立项（airfc:created）");
        Console.WriteLine("  intention: " + created.Intention.Text);
        // 澄清向导（场景 1.1 收敛协议）：Design/Acceptance 空缺 → 机器引导追问，先立项后 refine。
        await this.ClarifyAsync(created);
        AIRfc? latest = _harness.Rfc;
        if (latest == null) {
            return;
        }
        if (!_harness.AcceptanceDefined(latest)) {
            Console.WriteLine("  acceptance 未定义——Acceptance 门闩拒绝 AttachPlan；用 /revise --acceptance=<验收> 补齐");
        } else {
            Console.WriteLine("  acceptance: " + latest.Acceptance.Assertions);
        }
        if (!_harness.DesignDefined(latest)) {
            Console.WriteLine("  design 未定义——design-review 轻量提示：建议 /revise --design=<边界/远见> 补齐");
        }
    }

    // ── 澄清向导：/rfc 模糊意图 → 追问验收/边界 → airfc:clarify → 先立项后 refine ──

    /// <summary>
    /// Design / Acceptance 空缺时主动追问（验收标准 · 影响面/边界 · 是否接受先立项后逐步
    /// refine）；答复以 airfc:clarify 决策事件入轨迹。用户确认后以 ReviseRfc 落 Spec（Revision+1，
    /// 经 <see cref="AIHarnessSession.Rfc"/> 可见最新版）。
    /// </summary>
    private async Task ClarifyAsync(AIRfc rfc) {
        bool needAcceptance = !_harness.AcceptanceDefined(rfc);
        bool needDesign = !_harness.DesignDefined(rfc);
        if (!needAcceptance && !needDesign) {
            return;
        }
        Console.WriteLine("[rfc] ⚠ 需求模糊 → 澄清向导（补验收/边界；Acceptance 先行才允许进入计划）:");
        string acceptanceText = "";
        if (needAcceptance) {
            Console.Write("  验收标准（要什么结果）？[回车=暂不定义，之后 /revise --acceptance= 补齐] ");
            string a = Console.ReadLine();
            if (a != null && a.Trim() != "") {
                acceptanceText = a.Trim();
            }
        }
        string designText = "";
        if (needDesign) {
            Console.Write("  影响面/边界（可选）？[回车=暂不定义] ");
            string d = Console.ReadLine();
            if (d != null && d.Trim() != "") {
                designText = d.Trim();
            }
        }
        Console.Write("  是否接受先立项、后续逐步 refine？[y]es / [n]o ");
        string ans = Console.ReadLine();
        string confirm = ans != null ? ans.Trim().ToLower() : "";
        if (confirm != "y" && confirm != "yes") {
            _harness.RecordClarify("是否接受先立项后逐步 refine？", "no");
            Console.WriteLine("[rfc] 澄清未确认 → 保持 design/acceptance 空缺；Acceptance 门闩拒绝 AttachPlan 直至 /revise 补齐");
            return;
        }
        _harness.RecordClarify("验收标准/影响面/边界", "yes");
        if (acceptanceText == "" && designText == "") {
            Console.WriteLine("[rfc] 澄清确认（先立项后 refine）→ design/acceptance 仍空缺，可后续 /revise 逐步补齐");
            return;
        }
        AIIntentionSpec? iSpec = null;
        AIDesignSpec? dSpec = null;
        if (designText != "") {
            dSpec = new AIDesignSpec();
            dSpec.Structure = designText;
        }
        AIAcceptanceSpec? aSpec = null;
        if (acceptanceText != "") {
            aSpec = new AIAcceptanceSpec();
            aSpec.Assertions = acceptanceText;
        }
        AIRfc? next = _harness.ReviseRfc(iSpec, dSpec, aSpec, "clarify");
        if (next == null) {
            Console.WriteLine("[rfc] 澄清落 Spec 被拒（RfcSpec 租约冲突）——维持 v" + rfc.Revision);
            return;
        }
        _d5.Reset(next);
        Console.WriteLine("[rfc] 澄清落 Spec → " + next.RfcId + " v" + next.Revision + "（airfc:revised）");
    }

    // ── 升版纠偏：/revise <理由> [--intention=<文本>] [--design=<文本>] [--acceptance=<文本>] [--test=<测试名>] [--verify=<命令>] ──

    private async Task ReviseAsync(string rest) {
        AIRfc? current = _harness.Rfc;
        if (current == null) {
            Console.WriteLine("[revise] 无活跃 AIRfc——先 /rfc <意图> 立项");
            return;
        }
        if (rest == "") {
            Console.WriteLine("usage: /revise <理由> [--intention=<文本>] [--design=<文本>] [--acceptance=<文本>]");
            Console.WriteLine("  结构化验收（推荐）：--acceptance=<断言> --test=<测试名> [--verify=<验证命令>] — 落结构化条目，D5/D3 可机器对照");
            Console.WriteLine("  无旗标时整句即新 Intention 并作为理由；可组合多个 -- 旗标");
            return;
        }
        string reason = this.TakeReason(rest);
        string intention = this.TakeValue(rest, "--intention");
        string design = this.TakeValue(rest, "--design");
        string acceptance = this.TakeValue(rest, "--acceptance");
        string test = this.TakeValue(rest, "--test");
        string verify = this.TakeValue(rest, "--verify");
        if (intention == "" && design == "" && acceptance == "") {
            intention = rest.Trim();
            reason = rest.Trim();
        }
        if (reason == "") {
            reason = "revise";
        }
        AIIntentionSpec? iSpec = null;
        if (intention != "") {
            iSpec = new AIIntentionSpec(intention);
        }
        AIDesignSpec? dSpec = null;
        if (design != "") {
            dSpec = new AIDesignSpec();
            dSpec.Structure = design;
        }
        AIAcceptanceSpec? aSpec = null;
        if (acceptance != "") {
            AIAcceptanceSpec acc = new AIAcceptanceSpec();
            if (test != "" || verify != "") {
                // 结构化验收条目（场景 4.1 验收对照源头）：断言 + 可选测试名/验证命令。
                acc.AddItem("", acceptance, test != "" ? test : null, verify != "" ? verify : null);
            } else {
                acc.Assertions = acceptance;
            }
            aSpec = acc;
        }
        AIRfc? next = _harness.ReviseRfc(iSpec, dSpec, aSpec, reason);
        if (next == null) {
            Console.WriteLine("[revise] 升版被拒（RfcSpec 租约冲突）");
            return;
        }
        if (next.Status == AIRfcStatus.Contested) {
            Console.WriteLine("[revise] ⚠ L2 冲突检测（B1）：同 acceptance 项被不同来源反方向覆盖 → "
                + next.RfcId + " v" + next.Revision + " Contested；用 /conflict 查看并行方向/来源，人 CCB 裁决后落新 Revision 基线");
            return;
        }
        _d5.Reset(next);
        Console.WriteLine("[revise] " + next.RfcId + " v" + next.Revision + "（airfc:revised）");
        Console.WriteLine(next.ToContextBlock());
        if (!_harness.AcceptanceDefined(next)) {
            Console.WriteLine("  ⚠ acceptance 未定义——Acceptance 门闩拒绝 AttachPlan，直至 /revise --acceptance= 补齐");
        }
        if (!_harness.DesignDefined(next)) {
            Console.WriteLine("  ⚠ design 未定义——design-review 轻量提示：建议补齐远见/边界再进实现");
        }
    }

    // ── 拒绝方向：/reject <reason>（注意：/reject 无参仍是计划门闩拒绝） ──

    private async Task RejectAsync(string reason) {
        AIRfc? current = _harness.Rfc;
        if (current == null) {
            Console.WriteLine("[reject] 无活跃 AIRfc——先 /rfc <意图> 立项");
            return;
        }
        if (reason == "") {
            Console.WriteLine("usage: /reject <reason> — 拒绝当前方向（Active → Rejected，airfc:rejected）");
            return;
        }
        AIRfc? rejected = _harness.RejectRfc(reason);
        if (rejected == null) {
            Console.WriteLine("[reject] 拒绝被拒（RfcSpec 租约冲突）");
            return;
        }
        await _log.AppendAsync(SessionEvent.Approval("rfc", "rejected", reason), new CancellationToken());
        Console.WriteLine("[reject] " + rejected.RfcId + " v" + rejected.Revision + " → Rejected（airfc:rejected）");
        Console.WriteLine("  可 /revise <理由> 升版再入 Active");
    }

    // ── 小结判定：/summary（交互录入五字段 → work_summary + 偏差判定） ──

    private async Task SummaryAsync() {
        AIRfc? rfc = _harness.Rfc;
        if (rfc == null) {
            Console.WriteLine("[summary] 无活跃 AIRfc——先 /rfc <意图> 立项");
            return;
        }
        Console.WriteLine("[summary] 录入当前工作单元小结（对照 AIRfc v" + rfc.Revision + "；困难/绕过、发现必答）");
        Console.Write("  单元 id（回车=unit-N）: ");
        string unitId = Console.ReadLine();
        if (unitId == null || unitId.Trim() == "") {
            unitId = "unit-" + rfc.Revision;
        }
        Console.Write("  做了什么: ");
        string did = Console.ReadLine();
        Console.Write("  对齐（设计/需求 ✓ 或偏差点）: ");
        string alignment = Console.ReadLine();
        Console.Write("  验证（命令 + 绿/红 + 覆盖）: ");
        string verification = Console.ReadLine();
        Console.Write("  困难/绕过（必答；无 → 无）: ");
        string difficulty = Console.ReadLine();
        Console.Write("  发现（必答；无 → 无）: ");
        string findings = Console.ReadLine();
        AIWorkSummary summary = new AIWorkSummary(unitId.Trim(), did, alignment, verification);
        summary.Difficulty = difficulty;
        summary.Findings = findings;
        _harness.RecordSummary(summary);
        Console.WriteLine("\n" + summary.Format());
        // 偏差判定：对齐字段对照当前 Revision；含偏差点/绕过/发现 → 触发纠偏或评审信号。
        if (alignment != null && (alignment.IndexOf("偏") >= 0 || alignment.IndexOf("✓") < 0)) {
            Console.WriteLine("[summary] ⚠ 对齐含偏差点 → 建议 /revise 纠偏或确认方向");
        }
        if (summary.HasBypass) {
            Console.WriteLine("[summary] ⚠ 有绕过 → 必须上报（纠偏协议/LLM 门闩）；建议 /dod 或 /revise");
        }
        if (summary.HasFindings) {
            Console.WriteLine("[summary] ⚠ 有发现 → 触发设计评审信号（collaboration-checkpoints）");
        }
        if (difficulty == null || difficulty.Trim() == "" || findings == null || findings.Trim() == "") {
            Console.WriteLine("[summary] 未完成：困难/绕过、发现为必答字段（work_summary 已记录，但需补齐）");
        }
    }

    // ── 绿点：/checkpoint [label] ──

    private async Task CheckpointAsync(string rest) {
        string label = rest != "" ? rest : "checkpoint";
        bool captured = await _harness.CheckpointGreenAsync(label, new CancellationToken());
        if (captured) {
            Console.WriteLine("[checkpoint] 绿点已捕获（checkpoint:green，快照 " + _harness.Checkpoints.StoreDir + "）");
        } else {
            Console.WriteLine("[checkpoint] 绿点事件已记录，但快照未捕获（项目根不可解析 → snapshot:none）");
        }
    }

    // ── 回滚：/rollback [--cp=<绿点id>] [reason] ──
    // 场景 3.4 多绿点历史：--cp= 指定回滚到某绿点（缺省最近）。reason 可省略（自动生成）。

    private async Task RollbackAsync(string rest) {
        string checkpointId = this.TakeValue(rest, "--cp");
        string reason = this.RollbackReason(rest, checkpointId);
        bool ok = await _harness.CheckpointRollbackAsync(
            checkpointId != "" ? checkpointId : null, "rollback", reason, new CancellationToken());
        string target = checkpointId != "" ? checkpointId : "最近绿点";
        if (ok) {
            Console.WriteLine("[rollback] 已回滚到 " + target + "（checkpoint:rollback；AIRfc/AIPlan 已联动恢复，门下次 /dod 重跑）");
        } else {
            Console.WriteLine("[rollback] 无可用绿点快照（" + target + "）→ 回滚失败，升级人确认方向");
        }
    }

    private static string RollbackReason(string rest, string checkpointId) {
        if (checkpointId == "") {
            return rest.Trim() != "" ? rest.Trim() : "manual rollback";
        }
        string marker = "--cp=" + checkpointId;
        int idx = rest.IndexOf(marker);
        if (idx < 0) {
            return rest.Trim();
        }
        string before = rest.Substring(0, idx).Trim();
        string after = rest.Substring(idx + marker.Length).Trim();
        string combined = (before + " " + after).Trim();
        return combined != "" ? combined : ("rollback to " + checkpointId);
    }

    // ── D5 子命令：/dod d5 [<序号> <证明>] ──

    private async Task DodSubAsync(string rest) {
        if (rest == "d5") {
            AIRfc? rfc = _harness.Rfc;
            if (rfc == null) {
                Console.WriteLine("[dod] 无活跃 AIRfc——先 /rfc <意图> 立项");
                return;
            }
            this.PrintD5(rfc);
            return;
        }
        if (rest.StartsWith("d5 ")) {
            string args = rest.Substring(3).Trim();
            int sp = args.IndexOf(" ");
            if (sp <= 0) {
                Console.WriteLine("usage: /dod d5 <序号> <证明> — 填 D5 证明（测试/文件，机器校验引用存在性）");
                return;
            }
            string idxText = args.Substring(0, sp).Trim();
            string proof = args.Substring(sp + 1).Trim();
            int index = Convert.ToInt32(idxText);
            bool ok = _d5.SetProof(index, proof);
            if (ok) {
                // 场景 4.1：填证明即触发机器校验（文件/`--list-tests` 测试名引用存在性）。
                await _d5.ValidateProofsAsync(new CancellationToken());
                D5ProofEntry entry = _d5.Entries[index - 1];
                Console.WriteLine("[dod] D5 槽位 " + index + " 已填证明：'" + proof + "'（机器校验: "
                    + this.ProofStatusName(entry.Status) + "；" + _d5.ProvenCount + "/" + _d5.Entries.Count + " 有效）");
                if (entry.Status == D5ProofVerdict.Invalid) {
                    Console.WriteLine("  ⚠ 证明引用不存在（文件/测试名无法解析）→ 标红；改填真实测试/文件路径");
                }
            } else {
                Console.WriteLine("[dod] 槽位 " + idxText + " 越界（1.." + _d5.Entries.Count + "）");
            }
            return;
        }
        Console.WriteLine("usage: /dod | /dod d5 | /dod d5 <序号> <证明>");
    }

    // ── 全门：/dod ──

    private async Task DodAsync() {
        AIRfc? rfc = _harness.Rfc;
        if (rfc == null) {
            Console.WriteLine("[dod] 无活跃 AIRfc——先 /rfc <意图> 立项");
            return;
        }
        AIPlan plan = _planGate != null ? _planGate.GetPlan() : null;
        if (plan == null) {
            Console.WriteLine("[dod] 未绑定计划——先让模型产出 AIPlan、/approve 批准并执行完步骤");
            return;
        }
        if (!_d5.HasEntries) {
            _d5.Reset(rfc);
        }
        CancellationToken ct = new CancellationToken();
        // 场景 4.1：跑 /dod 前先机器校验全部 D5 证明（文件/`--list-tests` 测试名引用存在性），
        // 无有效证明的槽位标红（禁「字符串非空即 Passed」）。
        await _d5.ValidateProofsAsync(ct);
        Console.WriteLine("[dod] 运行 DoD 门（" + rfc.RfcId + " v" + rfc.Revision + " · 计划 " + this.PlanStatusName(plan.Status) + "）");
        List<AIDoDGateResult> results = await _harness.DoD.RunAutoGatesAsync(rfc, ct);
        int i = 0;
        int n = results.Count;
        while (i < n) {
            AIDoDGateResult r = results[i];
            if (r != null) {
                this.PrintGate(r);
            }
            i = i + 1;
        }
        this.PrintD5(rfc);
        bool d0ToD3Ok = this.D0ToD3Ok(results);
        if (!d0ToD3Ok) {
            // L2 自动迭代（RFC 043 场景 2.3/4.3）：D0–D3 失败 → 结构化回喂 → ≤3 轮修复 →
            // 收敛或超限回滚 + 升级人（机器闭环，替代「打印后人手重跑 /dod」）。
            await this.RunAutoFixLoopAsync(rfc);
            return;
        }
        bool autoOk = this.AutoGatesOk(results);
        if (!autoOk) {
            Console.WriteLine("[dod] 自动门未全 Passed（Pending ≠ Passed）→ 修复后重跑 /dod");
            return;
        }
        if (!_d5.AllProven) {
            Console.WriteLine("[dod] D5 未过：有槽位无有效证明（无证明 / 未机器校验 / 引用不存在标红）→ 用 /dod d5 <序号> <证明> 填有效证明后重跑 /dod");
            return;
        }
        List<string> highRisk = await CollaborationCheckpoints.DetectAsync(_workspace, plan);
        await this.D7AcceptAsync(rfc, plan, highRisk);
    }

    /// <summary>D0–D3 机器迭代面是否全 Passed（D0–D3 任一 Failed/NeedsHuman/Pending → 进入自动迭代）。</summary>
    private bool D0ToD3Ok(List<AIDoDGateResult> results) {
        if (results == null || results.Count == 0) {
            return false;
        }
        int i = 0;
        int n = results.Count;
        while (i < n) {
            AIDoDGateResult r = results[i];
            if (r != null
                && r.Gate != AIDoDGateKind.D4DiffCoverage
                && r.Gate != AIDoDGateKind.D5SelfReview
                && r.Gate != AIDoDGateKind.D6AntiPattern
                && r.Gate != AIDoDGateKind.D7HumanAccept) {
                if (r.Status != AIDoDGateStatus.Passed) {
                    return false;
                }
            }
            i = i + 1;
        }
        return true;
    }

    /// <summary>
    /// L2 自动迭代闭环（场景 2.3/4.3 断点修复）：经 <see cref="AIHarnessSession.RunFixLoopAsync"/>
    /// 跑 D0–D3（Gate=D3）——无绿点先提示 /checkpoint（升级人，不烧预算）；≤3 轮收敛 →
    /// 提示重跑 /dod 完成 D4–D7；超限 → 自动回滚最近绿点 + 升级人。
    /// </summary>
    private async Task RunAutoFixLoopAsync(AIRfc rfc) {
        CancellationToken ct = new CancellationToken();
        AIDoDFixLoopResult loop = await _harness.RunFixLoopAsync(
            AIDoDGateKind.D3Behavior, _fixProvider, ct);
        if (loop.IsPassed) {
            Console.WriteLine("[dod] L2 自动迭代收敛（" + loop.FixRounds + " 轮修复）→ D0–D3 全绿；重跑 /dod 完成 D4–D7 验收");
            return;
        }
        if (loop.RolledBack) {
            Console.WriteLine("[dod] L2 迭代超限（" + loop.FixRounds + " 轮）→ 已自动回滚最近绿点（checkpoint:rollback）；升级人确认方向");
            return;
        }
        Console.WriteLine("[dod] L2 迭代未收敛 → " + loop.Reason);
    }

    private void PrintGate(AIDoDGateResult r) {
        Console.WriteLine("  " + this.GateName(r.Gate) + "  [" + this.StatusName(r.Status) + "]  " + r.Signal);
    }

    private void PrintD5(AIRfc rfc) {
        if (!_d5.HasEntries) {
            Console.WriteLine("  D5 自审  [Pending]  无 Acceptance——先用 /revise --acceptance=<验收> 定义验收");
            return;
        }
        if (_d5.AllProven) {
            Console.WriteLine("  D5 自审  [Passed]  " + _d5.ProvenCount + "/" + _d5.Entries.Count + " 项有机器校验证明（引用真实测试/文件）");
        } else {
            Console.WriteLine("  D5 自审  [Failed]  " + _d5.ProvenCount + "/" + _d5.Entries.Count + " 项有有效证明（" + (_d5.Entries.Count - _d5.ProvenCount) + " 项无证明/证明无效标红）");
        }
        Console.WriteLine(_d5.Render());
    }

    /// <summary>自动门子集（D0–D4 + D6）全 Passed；D5/D7 人类门不参与此判定。</summary>
    private bool AutoGatesOk(List<AIDoDGateResult> results) {
        if (results == null || results.Count == 0) {
            return false;
        }
        int i = 0;
        int n = results.Count;
        while (i < n) {
            AIDoDGateResult r = results[i];
            if (r != null
                && r.Gate != AIDoDGateKind.D5SelfReview
                && r.Gate != AIDoDGateKind.D7HumanAccept) {
                if (r.Status != AIDoDGateStatus.Passed) {
                    return false;
                }
            }
            i = i + 1;
        }
        return true;
    }

    /// <summary>D7 一次人验收（协作确认点强确认 + 一次接受/拒绝；拒绝记录原因停留 Verifying）。</summary>
    private async Task D7AcceptAsync(AIRfc rfc, AIPlan plan, List<string> highRisk) {
        if (plan.Status != AIPlanStatus.Verifying) {
            Console.WriteLine("[dod] 计划未到 Verifying（全部步骤完成）→ 当前 " + this.PlanStatusName(plan.Status) + "；CompletePlanAfterDoDAsync 不会放行");
        }
        if (highRisk != null && highRisk.Count > 0) {
            Console.WriteLine("[!] 协作确认点（D7 强确认 · collaboration-checkpoints）：");
            int i = 0;
            int n = highRisk.Count;
            while (i < n) {
                Console.WriteLine("  - " + highRisk[i]);
                i = i + 1;
            }
            Console.Write("    是否确认上述变更方向？[yes] / [no]（no 记录原因停留 Verifying）: ");
            string confirm = Console.ReadLine();
            string c = confirm != null ? confirm.Trim().ToLower() : "";
            if (c != "yes") {
                string reason = confirm != null && confirm.Trim() != "" ? confirm.Trim() : "high-risk not confirmed";
                await _log.AppendAsync(SessionEvent.Approval("dod", "rejected", reason), new CancellationToken());
                Console.WriteLine("[dod] 高风险确认未通过 → 停留 Verifying；原因已记录");
                return;
            }
        }
        Console.Write("D7 一次人验收：是否接受本工作项交付？[y]es / [n]o（拒绝记录原因）: ");
        string ans = Console.ReadLine();
        string a = ans != null ? ans.Trim().ToLower() : "";
        if (a == "y" || a == "yes") {
            bool done = await _harness.CompletePlanAfterDoDAsync(new CancellationToken(), true, true);
            if (done) {
                await _log.AppendAsync(SessionEvent.Approval("dod", "accepted", "D7 人验收通过 — DoD D0-D7 全勾"), new CancellationToken());
                Console.WriteLine("[dod] D7 接受 → AIPlan.Completed + checkpoint:green 已记录");
            } else {
                await _log.AppendAsync(SessionEvent.Approval("dod", "accepted", "D7 接受但汇总门未放行（自动门/计划状态未满足）"), new CancellationToken());
                Console.WriteLine("[dod] D7 已接受，但 CompletePlanAfterDoDAsync 未放行——自动门未全 Passed 或计划未在 Verifying；修复后重跑 /dod");
            }
        } else {
            string reason = a != "" ? a : "user rejected delivery";
            await _log.AppendAsync(SessionEvent.Approval("dod", "rejected", reason), new CancellationToken());
            Console.WriteLine("[dod] D7 拒绝 → 记录原因停留 Verifying；可用 /revise 纠偏后重跑 /dod");
        }
    }

    // ── AIRfc 状态持久化：/save 落盘 · /resume 恢复（2.4 续跑前提，非 transcript 重放冒充）──

    /// <summary>
    /// 持久化当前 AIRfc 聚合根（SaveRfcAsync → target/scratch/arcagent-state/airfc.json）。
    /// 无 AIRfc / 落盘失败 → 提示。AIPlan/门状态持久化登记次阶段。
    /// </summary>
    private async Task SaveStateAsync() {
        bool saved = await _harness.SaveRfcAsync(new CancellationToken());
        if (saved) {
            AIRfc? rfc = _harness.Rfc;
            string id = rfc != null ? rfc.RfcId + " v" + rfc.Revision : "";
            Console.WriteLine("[save] AIRfc " + id + " 已持久化（target/scratch/arcagent-state/airfc.json）");
        } else {
            Console.WriteLine("[save] 无 AIRfc 或落盘失败——先 /rfc 立项");
        }
    }

    /// <summary>从 target/scratch/arcagent-state/airfc.json 恢复 AIRfc 聚合根（经 /resume 调用）。</summary>
    public async Task<bool> RestoreStateAsync() {
        bool restored = await _harness.RestoreRfcAsync(new CancellationToken());
        if (restored) {
            AIRfc? rfc = _harness.Rfc;
            if (rfc != null) {
                _d5.Reset(rfc);
            }
            Console.WriteLine("[resume] AIRfc 聚合根已重建（" + (rfc != null ? rfc.RfcId + " v" + rfc.Revision + " " + AIRfcStatusCodec.ToWireString(rfc.Status) : "?") + "）——非 transcript 重放冒充");
        } else {
            Console.WriteLine("[resume] 无 AIRfc 状态可恢复（或已是最新）——继续 transcript 重放");
        }
        return restored;
    }

    // ── 冲突仲裁（B1）：/conflict [list|detail|resolve|reject] ──
    // /conflict                      列出 Open 冲突（方向/来源/evidence）
    // /conflict <conflictId>         查看冲突详情（含双方 acceptance 快照）
    // /conflict resolve <id> [--after] [--by=<CCB>] <reason>   人 CCB 裁决（缺省维持冲突前
    //                                方向；--after 采纳被拦截方向）→ 新 Revision 基线 + airfc:resolved
    // /conflict reject <id> [--by=<CCB>] <reason>               拒绝冲突（AIRfc → Rejected）

    private async Task ConflictAsync(string rest) {
        if (rest == "") {
            this.ListConflicts();
            return;
        }
        string cmd = rest;
        string args = "";
        int sp = rest.IndexOf(" ");
        if (sp > 0) {
            cmd = rest.Substring(0, sp).Trim();
            args = rest.Substring(sp + 1).Trim();
        }
        if (cmd == "resolve") {
            await this.ResolveConflictAsync(args);
            return;
        }
        if (cmd == "reject") {
            await this.RejectConflictAsync(args);
            return;
        }
        // 其余视为冲突 id → 详情。
        this.ShowConflictDetail(rest);
    }

    private void ListConflicts() {
        List<AIConflictRecord> open = _harness.Conflicts.Open();
        if (open.Count == 0) {
            Console.WriteLine("[conflict] 无 Open 冲突（/conflict <id> 查看详情；resolve/reject 走人 CCB 裁决）");
            return;
        }
        Console.WriteLine("[conflict] Open 冲突（" + open.Count + "）：");
        int i = 0;
        int n = open.Count;
        while (i < n) {
            AIConflictRecord r = open[i];
            Console.WriteLine("  " + r.ConflictId + "  " + this.KindName(r.Kind)
                + "  " + r.RfcId + " v" + r.Revision
                + "  来源 " + r.Parties[0] + " ↔ " + r.Parties[1]);
            Console.WriteLine("      evidence: " + r.Evidence.Trim());
            i = i + 1;
        }
        Console.WriteLine("[conflict] 人 CCB 裁决：/conflict resolve <id> [--after] [--by=<CCB>] <reason> | /conflict reject <id> [--by=<CCB>] <reason>");
    }

    private void ShowConflictDetail(string conflictId) {
        AIConflictRecord? r = _harness.Conflicts.Find(conflictId);
        if (r == null) {
            Console.WriteLine("[conflict] 未找到冲突 " + conflictId + "（/conflict 列出 Open）");
            return;
        }
        Console.WriteLine("[conflict] " + r.ConflictId + " [" + this.KindName(r.Kind) + "] " + r.Status);
        Console.WriteLine("  rfc: " + r.RfcId + " v" + r.Revision);
        Console.WriteLine("  来源: " + r.Parties[0] + " ↔ " + r.Parties[1]);
        Console.WriteLine("  evidence: " + r.Evidence.Trim());
        Console.WriteLine("  冲突前 acceptance: " + r.BeforeAcceptance.Assertions);
        Console.WriteLine("  被拦截 acceptance: " + r.AfterAcceptance.Assertions);
        if (r.Status == "Resolved") {
            Console.WriteLine("  已裁决: " + r.Decision + " by " + r.ResolvedBy);
        }
    }

    private async Task ResolveConflictAsync(string args) {
        int sp = args.IndexOf(" ");
        string conflictId = sp > 0 ? args.Substring(0, sp).Trim() : args.Trim();
        string rest2 = sp > 0 ? args.Substring(sp + 1).Trim() : "";
        if (conflictId == "") {
            Console.WriteLine("usage: /conflict resolve <id> [--after] [--by=<CCB>] <reason>");
            return;
        }
        bool after = rest2.IndexOf("--after") >= 0;
        string ccbBy = this.TakeValue(rest2, "--by");
        string reason = this.TakeReason(rest2);
        if (ccbBy == "") {
            ccbBy = _session != null ? _session.SessionId : "ccb";
        }
        string decision = after ? "accept-after" : "accept-before";
        AIRfc? resolved = _harness.ResolveConflictAsync(conflictId, decision, reason, ccbBy);
        if (resolved == null) {
            Console.WriteLine("[conflict] 裁决未生效（冲突不存在 / 非 Open / 未被确权；机器不可自动选胜者——resolvedBy 必须显式人）");
            return;
        }
        Console.WriteLine("[conflict] " + conflictId + " 已由 CCB(" + ccbBy + ") 裁决 → " + resolved.RfcId + " v" + resolved.Revision
            + " Active（airfc:resolved；新基线" + (after ? "采纳被拦截方向" : "维持冲突前方向") + "）");
        Console.WriteLine(resolved.ToContextBlock());
    }

    private async Task RejectConflictAsync(string args) {
        int sp = args.IndexOf(" ");
        string conflictId = sp > 0 ? args.Substring(0, sp).Trim() : args.Trim();
        string rest2 = sp > 0 ? args.Substring(sp + 1).Trim() : "";
        if (conflictId == "") {
            Console.WriteLine("usage: /conflict reject <id> [--by=<CCB>] <reason>");
            return;
        }
        string ccbBy = this.TakeValue(rest2, "--by");
        string reason = this.TakeReason(rest2);
        if (ccbBy == "") {
            ccbBy = _session != null ? _session.SessionId : "ccb";
        }
        AIRfc? rejected = _harness.RejectConflictAsync(conflictId, reason, ccbBy);
        if (rejected == null) {
            Console.WriteLine("[conflict] 拒绝未生效（冲突不存在 / 非 Open / 未被确权）");
            return;
        }
        Console.WriteLine("[conflict] " + conflictId + " 已被 CCB(" + ccbBy + ") 拒绝 → " + rejected.RfcId + " v" + rejected.Revision
            + " Rejected（conflict:rejected；可 /revise 升版再入 Active）");
    }

    private static string KindName(AIConflictKind kind) {
        if (kind == AIConflictKind.LeaseConflict) { return "L1租约"; }
        if (kind == AIConflictKind.MergeConflict) { return "L3合并"; }
        return "L2Spec矛盾";
    }

    // ── 解析工具 ──

    private static string TakeReason(string rest) {
        int idx = rest.IndexOf(" --");
        if (idx < 0) {
            return rest.Trim();
        }
        return rest.Substring(0, idx).Trim();
    }

    private static string TakeValue(string rest, string flag) {
        string marker = flag + "=";
        int idx = rest.IndexOf(marker);
        if (idx < 0) {
            return "";
        }
        string tail = rest.Substring(idx + marker.Length);
        int next = tail.IndexOf(" --");
        if (next < 0) {
            return tail.Trim();
        }
        return tail.Substring(0, next).Trim();
    }

    private static string GateName(AIDoDGateKind gate) {
        if (gate == AIDoDGateKind.D0Compile) { return "D0 编译"; }
        if (gate == AIDoDGateKind.D1Semantics) { return "D1 语义"; }
        if (gate == AIDoDGateKind.D2Contract) { return "D2 契约"; }
        if (gate == AIDoDGateKind.D3Behavior) { return "D3 行为"; }
        if (gate == AIDoDGateKind.D4DiffCoverage) { return "D4 diff 覆盖"; }
        if (gate == AIDoDGateKind.D5SelfReview) { return "D5 自审"; }
        if (gate == AIDoDGateKind.D6AntiPattern) { return "D6 反模式"; }
        return "D7 人验收";
    }

    private static string StatusName(AIDoDGateStatus status) {
        if (status == AIDoDGateStatus.Passed) { return "Passed"; }
        if (status == AIDoDGateStatus.Failed) { return "Failed"; }
        if (status == AIDoDGateStatus.NeedsHuman) { return "NeedsHuman"; }
        return "Pending";
    }

    private static string ProofStatusName(D5ProofVerdict status) {
        if (status == D5ProofVerdict.Valid) { return "Valid"; }
        if (status == D5ProofVerdict.Invalid) { return "Invalid"; }
        if (status == D5ProofVerdict.Unchecked) { return "Unchecked"; }
        return "Missing";
    }

    private static string PlanStatusName(AIPlanStatus status) {
        if (status == AIPlanStatus.Pending) { return "Pending"; }
        if (status == AIPlanStatus.Approved) { return "Approved"; }
        if (status == AIPlanStatus.Executing) { return "Executing"; }
        if (status == AIPlanStatus.Verifying) { return "Verifying"; }
        if (status == AIPlanStatus.Completed) { return "Completed"; }
        return "Rejected";
    }
}
