// RFC 038 M8.2：AIPlan — 复杂任务结构化计划（状态机 + 修订 + Lint + 上下文折叠）。
// RFC 043 references/plan-tree（P1）：Steps 由扁平 List<AITaskStep> 升级为 AIPlanTree
// （单层树 = 隐式根 + 叶步骤）；MarkStepDone(int) 由 MarkNodeDone(nodeId) 取代；
// AITaskStep 保留为 FromFlat 迁移输入（P2 才删）。
//
// 体系化引导复杂任务执行（对齐 Reasonix /plan / claude-code Plan Mode）：
// 模型先产出结构化计划 → 人类批准 → 有界执行 → 验证 → 完成；执行中可修订（revise_plan）。
// 计划是「门」：未批准（Pending/Rejected）时受约束的写入能力被 AIPlanGate 拦截。
//
// 结合 LLM 特性强化（区别于朴素"字符串计划"）：
//   - 状态机：Pending → Approved → Executing → Verifying → Completed / Rejected（bool 不足以表达生命周期）。
//     M8：满额步骤只推进到 Verifying（待 DoD D0–D7 判定）；Completed 唯一写入路径 = DoD 受控 API。
//   - 修订：LLM 计划常不准，执行中允许 revise_plan 产出修订版（Revision+1）并回 Pending 重审。
//   - 计划 Lint：对模型产出的计划做软化校验（步骤数/可验证性/Verification），以引导性反馈
//     驱动模型在动手前改好，而非事后惩罚。
//   - 上下文折叠（Cache-First）：已完成步骤折叠为单行 `[x] N. title (done)`，未完成/当前步骤
//     保留详情——追加式 + 前缀稳定 → 最大化 LLM KV cache 命中。
//
// 责任边界：本文件只承载「计划数据结构 + 状态机」；上下文注入（AIPlanContextProvider）与
// 门闩/审批（AIPlanGate）分列同目录独立文件，保持单文件单职责。
namespace Arc.Agent;
using Arc;
using Arc.Text;

/// <summary>计划生命周期状态（门闩只拦 Pending/Rejected）。</summary>
public enum AIPlanStatus {
    /// <summary>已创建、待人类审批（写入被拦）。</summary>
    Pending,
    /// <summary>已批准、可执行（写入放行）。</summary>
    Approved,
    /// <summary>执行中（CurrentStepIndex 推进）。</summary>
    Executing,
    /// <summary>全部步骤已完成、等待 DoD 判定（D0–D7 全勾）后才转入 Completed（M8 待判定态）。</summary>
    Verifying,
    /// <summary>DoD D0–D7 全勾后唯一经受控 API 写入的终态。</summary>
    Completed,
    /// <summary>被人类拒绝（须修订重审）。</summary>
    Rejected
}

/// <summary>单一步骤（P1 保留为 AIPlanTree.FromFlat 迁移输入；P2 删除）。</summary>
public class AITaskStep {
    /// <summary>步骤编号（1-based，用于展示）。</summary>
    public int Index;
    /// <summary>步骤标题（一句话描述要做什么；应含可验证动作）。</summary>
    public string Title;
    /// <summary>详细说明（文件范围、修改点、预期效果；可多行）。</summary>
    public string Description;
    /// <summary>预期文件路径（相对/绝对，多个用逗号分隔；可空用于规划型步骤）。</summary>
    public string Files;
    /// <summary>步骤是否已完成（执行后标记）。</summary>
    public bool Done;

    public AITaskStep() {
        this.Index = 0;
        this.Title = "";
        this.Description = "";
        this.Files = "";
        this.Done = false;
    }

    public AITaskStep(int index, string title, string description, string files) {
        this.Index = index;
        this.Title = title != null ? title : "";
        this.Description = description != null ? description : "";
        this.Files = files != null ? files : "";
        this.Done = false;
    }

    /// <summary>格式化为单行供日志/显示。</summary>
    public string ToLine() {
        string mark = this.Done ? "[x]" : "[ ]";
        return mark + " " + this.Index + ". " + this.Title + (this.Files != "" ? (" (" + this.Files + ")") : "");
    }
}

/// <summary>复杂任务计划（目标 + 计划树 + 状态机 + 修订 + Lint + 折叠展示）。</summary>
public class AIPlan {
    /// <summary>
    /// 稳定计划标识（创建时分配，跨修订不变）。租约键 "plan:"+Id 与 <c>AIRfc.PlanId</c>
    /// 引用共用同一 Id（self-review TOP2 / D-02：消除以 Goal 合成键的不一致）。
    /// </summary>
    public string Id;
    /// <summary>任务目标（用户原始需求的再表述，确保理解正确）。</summary>
    public string Goal;
    /// <summary>分析与策略（问题诊断、技术选型、修改策略概要）。</summary>
    public string Analysis;
    /// <summary>计划树（单层树 = 隐式根 + 叶步骤；取代扁平 List&lt;AITaskStep&gt;）。</summary>
    public AIPlanTree Tree;
    /// <summary>最终验证标准（如何确认任务完成）。</summary>
    public string Verification;
    /// <summary>计划生命周期状态（Pending 待审 / Rejected 被拒时门闩拦截写入）。</summary>
    public AIPlanStatus Status;
    /// <summary>当前执行步骤指针（1-based；0 = 未开始）。</summary>
    public int CurrentStepIndex;
    /// <summary>修订版本号（v1/v2/...；revise_plan 递增）。</summary>
    public int Revision;
    /// <summary>计划创建时间戳（100ns ticks）。</summary>
    public long CreatedAt;
    /// <summary>最后更新时间戳。</summary>
    public long UpdatedAt;

    public AIPlan() {
        this.Id = "plan-" + Guid.NewGuid().ToString();
        this.Goal = "";
        this.Analysis = "";
        this.Tree = new AIPlanTree();
        this.Verification = "";
        this.Status = AIPlanStatus.Pending;
        this.CurrentStepIndex = 0;
        this.Revision = 1;
        this.CreatedAt = this.NowTicks();
        this.UpdatedAt = this.CreatedAt;
    }

    /// <summary>步骤投影（= Tree.Root.Children；单层树的叶步骤，向后等价于旧扁平列表）。</summary>
    public List<AIPlanNode> Steps {
        get {
            if (this.Tree == null || this.Tree.Root == null) {
                return new List<AIPlanNode>();
            }
            return this.Tree.Root.Children;
        }
    }

    /// <summary>计划总步骤数（单层树 = 根子节点数，与叶节点数等价）。</summary>
    public int TotalSteps {
        get {
            if (this.Tree == null || this.Tree.Root == null) {
                return 0;
            }
            return this.Tree.Root.Children.Count;
        }
    }

    /// <summary>已完成步骤数（= 树中 Completed 叶节点数）。</summary>
    public int CompletedSteps {
        get {
            if (this.Tree == null || this.Tree.Root == null) {
                return 0;
            }
            int count = 0;
            List<AIPlanNode> children = this.Tree.Root.Children;
            int i = 0;
            int n = children.Count;
            while (i < n) {
                AIPlanNode step = children[i];
                if (step.Status == AIPlanNodeStatus.Completed) {
                    count = count + 1;
                }
                i = i + 1;
            }
            return count;
        }
    }

    /// <summary>是否处于「门闩放行」状态（已批准/执行中/待判定/已完成）。</summary>
    public bool IsExecutable {
        get {
            return this.Status == AIPlanStatus.Approved
                || this.Status == AIPlanStatus.Executing
                || this.Status == AIPlanStatus.Verifying
                || this.Status == AIPlanStatus.Completed;
        }
    }

    /// <summary>批准：Pending/Rejected → Approved（门闩放行写入）。</summary>
    public void Approve() {
        if (this.Status == AIPlanStatus.Pending || this.Status == AIPlanStatus.Rejected) {
            this.Status = AIPlanStatus.Approved;
            this.Touch();
        }
    }

    /// <summary>拒绝：非 Rejected 态 → Rejected（门闩重新拦截，迫修订）。</summary>
    public void Reject() {
        if (this.Status != AIPlanStatus.Rejected) {
            this.Status = AIPlanStatus.Rejected;
            this.Touch();
        }
    }

    /// <summary>进入执行态（首个写入/执行动作前由门闩或编排标记；幂等）。</summary>
    public void BeginExecution() {
        if (this.Status == AIPlanStatus.Approved) {
            this.Status = AIPlanStatus.Executing;
            this.Touch();
        }
    }

    /// <summary>生成修订版骨架（Revision+1；状态回 Pending 需重审；步骤由门闩填充；Id 跨修订不变）。</summary>
    public AIPlan CreateRevision() {
        AIPlan next = new AIPlan();
        next.Id = this.Id;
        next.Revision = this.Revision + 1;
        next.CreatedAt = this.CreatedAt;
        return next;
    }

    /// <summary>
    /// 标记叶节点完成（按节点 Id）；推进当前指针；满额只到 Verifying（完成判定归 DoD，M8）。
    /// 取代旧 MarkStepDone(int)：叶落终态 Completed → 重算聚合态 → 根全 Completed
    /// → 根专属 Verifying（RootVerifying）→ 映射计划态 Verifying。
    /// </summary>
    public void MarkNodeDone(string nodeId) {
        if (this.Tree == null) {
            return;
        }
        AIPlanNode node = this.Tree.FindNode(nodeId);
        if (node == null || !node.IsLeaf) {
            return;
        }
        if (node.IsTerminal) {
            return;
        }
        node.Status = AIPlanNodeStatus.Completed;
        int index = this.IndexOf(node);
        if (index > this.CurrentStepIndex) {
            this.CurrentStepIndex = index;
        }
        this.Tree.ComputeStatus();
        if (this.Tree.RootVerifying) {
            // M8：满额不进 Completed——终态权威判定在 Harness/DoD（D0–D7 全勾），
            // 本方法只把计划推进到待判定态 Verifying。
            this.Status = AIPlanStatus.Verifying;
        }
        this.Touch();
    }

    /// <summary>
    /// DoD 受控完成（RFC 038 §12 / M8）：仅 Verifying（全部步骤已完成、待 DoD 判定）
    /// → Completed。这是 Completed 的唯一写入路径，由 Harness/DoD 汇总门在 D0–D7 全勾后调用；
    /// 模型路径（mark_step_done 等）禁止直改 Completed。根节点同步置 Completed（DoD 已全勾）。
    /// </summary>
    public void Complete() {
        if (this.Status == AIPlanStatus.Verifying) {
            this.Status = AIPlanStatus.Completed;
            if (this.Tree != null) {
                this.Tree.MarkRootVerified();
            }
            this.Touch();
        }
    }

    /// <summary>
    /// 回滚联动（RFC 043 场景 3.4 推倒重来）：按绿点记录的版本恢复计划状态。恢复
    /// Pending/Approved/Executing/Rejected → 叶步骤复位未完成、指针归零（回到执行前）；
    /// 恢复 Verifying/Completed → 叶步骤置为已完成、指针满额。仅改状态机与步进标记，
    /// 不重建步骤内容（计划面内容以当前 AIPlan 为准）。
    /// </summary>
    public void RestoreStatus(AIPlanStatus status) {
        if (this.Tree == null || this.Tree.Root == null) {
            return;
        }
        List<AIPlanNode> children = this.Tree.Root.Children;
        int i = 0;
        while (i < children.Count) {
            AIPlanNode node = children[i];
            node.Status = AIPlanNodeStatus.Pending;
            i = i + 1;
        }
        this.CurrentStepIndex = 0;
        if (status == AIPlanStatus.Verifying || status == AIPlanStatus.Completed) {
            int j = 0;
            while (j < children.Count) {
                AIPlanNode node = children[j];
                node.Status = AIPlanNodeStatus.Completed;
                j = j + 1;
            }
            this.CurrentStepIndex = this.TotalSteps;
        }
        this.Status = status;
        // 聚合：全 Completed → 根专属 Verifying（RootVerifying）；Completed（DoD 已全勾）→ 根直接 Completed。
        this.Tree.ComputeStatus();
        if (status == AIPlanStatus.Completed) {
            this.Tree.MarkRootVerified();
        }
        this.Touch();
    }

    /// <summary>更新时间戳（状态/进度变更后调用）。</summary>
    public void Touch() {
        this.UpdatedAt = this.NowTicks();
    }

    /// <summary>
    /// 计划 Lint：对模型产出的计划做软化校验，返回引导性问题列表（空 = 计划可接受）。
    /// 反馈驱动模型修订（revise_plan）而非硬失败——LLM 输出不稳定，引导优于惩罚。
    /// </summary>
    public List<string> Validate() {
        List<string> issues = new List<string>();
        if (this.TotalSteps == 0) {
            issues.Add("plan must contain at least one concrete step");
        }
        if (this.Verification == "") {
            issues.Add("plan is missing verification criteria — state how you will prove the task is done (commands to run, expected outcomes)");
        }
        if (this.TotalSteps > 12) {
            issues.Add("plan has " + this.TotalSteps + " steps — prefer fewer, larger steps that are each independently verifiable");
        }
        int i = 0;
        int n = this.TotalSteps;
        while (i < n) {
            AIPlanNode step = this.Steps[i];
            int idx = i + 1;
            if (step.Title == "") {
                issues.Add("step " + idx + " has no title");
            } else if (step.Title.Length < 8) {
                issues.Add("step " + idx + " title is vague (\"" + step.Title + "\") — describe the concrete action and the files it touches");
            }
            i = i + 1;
        }
        return issues;
    }

    /// <summary>
    /// 格式化为 markdown（供上下文注入 + 控制台展示）。上下文折叠：已完成步骤折叠为单行
    /// `[x] N. title (done)`、丢弃 Description——追加式前缀稳定 → 缓存友好；未完成/当前步骤
    /// 保留详情，当前步骤带 _(current)_ 高亮。
    /// </summary>
    public string ToMarkdown() {
        StringBuilder sb = new StringBuilder();
        sb.Append("# Task Plan");
        if (this.Revision > 1) {
            sb.Append(" (v" + this.Revision + ")");
        }
        sb.Append("\n\n");
        sb.Append("## Goal\n");
        sb.Append(this.Goal + "\n\n");
        if (this.Analysis != "") {
            sb.Append("## Analysis\n");
            sb.Append(this.Analysis + "\n\n");
        }
        sb.Append("## Steps\n");
        List<AIPlanNode> steps = this.Steps;
        int i = 0;
        int n = steps.Count;
        while (i < n) {
            AIPlanNode step = steps[i];
            int idx = i + 1;
            bool done = step.Status == AIPlanNodeStatus.Completed;
            string mark = done ? "x" : " ";
            if (done) {
                // 已完成：折叠为单行（省 token + 前缀稳定）。
                sb.Append("- [" + mark + "] **" + idx + ". " + step.Title + "**");
                if (step.Files != "") {
                    sb.Append(" — *" + step.Files + "*");
                }
                sb.Append(" _(done)_\n");
            } else {
                bool current = idx == this.CurrentStepIndex;
                sb.Append("- [" + mark + "] **" + idx + ". " + step.Title + "**");
                if (current) {
                    sb.Append(" _(current)_");
                }
                sb.Append("\n");
                if (step.Files != "") {
                    sb.Append("  - Files: " + step.Files + "\n");
                }
                if (step.Description != "") {
                    sb.Append("  - " + step.Description + "\n");
                }
            }
            i = i + 1;
        }
        sb.Append("\n");
        if (this.Verification != "") {
            sb.Append("## Verification\n");
            sb.Append(this.Verification + "\n");
        }
        sb.Append("\n**Status: " + this.StatusLabel() + "**");
        if (this.Status == AIPlanStatus.Pending) {
            sb.Append(" — PENDING APPROVAL. No writes will be performed until a human approves this plan.");
        } else if (this.Status == AIPlanStatus.Rejected) {
            sb.Append(" — REJECTED. Revise the plan (revise_plan) and wait for re-approval before writing.");
        } else if (this.Status == AIPlanStatus.Verifying) {
            sb.Append(" — all steps done. Awaiting Definition-of-Done verdict (D0–D7) before marking complete.");
        } else if (this.Status == AIPlanStatus.Completed) {
            sb.Append(" — all steps done and DoD D0–D7 passed.");
        } else {
            sb.Append(" — approved. You may execute the steps.");
        }
        return sb.ToString();
    }

    private string StatusLabel() {
        if (this.Status == AIPlanStatus.Pending) {
            return "PENDING APPROVAL";
        }
        if (this.Status == AIPlanStatus.Approved) {
            return "APPROVED";
        }
        if (this.Status == AIPlanStatus.Executing) {
            return "EXECUTING (step " + this.CurrentStepIndex + "/" + this.TotalSteps + ")";
        }
        if (this.Status == AIPlanStatus.Verifying) {
            return "VERIFYING (all steps done; awaiting DoD D0–D7)";
        }
        if (this.Status == AIPlanStatus.Completed) {
            return "COMPLETED (DoD D0–D7 passed)";
        }
        return "REJECTED";
    }

    /// <summary>节点在根子列表中的 1-based 位置；未找到 → 0。</summary>
    private int IndexOf(AIPlanNode node) {
        List<AIPlanNode> children = this.Steps;
        int i = 0;
        while (i < children.Count) {
            AIPlanNode step = children[i];
            if (step.Id == node.Id) {
                return i + 1;
            }
            i = i + 1;
        }
        return 0;
    }

    private long NowTicks() {
        DateTime now = DateTime.Now;
        return now.Ticks;
    }
}
