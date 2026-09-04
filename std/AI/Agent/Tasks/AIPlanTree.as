// RFC 043 references/plan-tree（P1 收敛）：AIPlanTree — 树形状态树容器。
//
// 承载 Root（隐式根，ParentId 空，Kind=Group 纯聚合）+ ComputeStatus（fail-closed 聚合）+
// Validate（结构 Lint）+ FromFlat（单层树迁移）+ FindNode。
//
// 聚合规则（父 = 子态上确界，命中即停）：Failed > Cancelled > Running > Ready/Pending > Completed；
// 其中 Cancelled 视同红（父 Failed）。根全子 Completed → 根专属 Verifying（RootVerifying 标志，
// 不进通用枚举；组级全 Completed → Completed，无 Verifying）。
namespace Arc.Agent;
using Arc;

/// <summary>树形状态树容器（根 + 聚合 + 校验 + 迁移）。</summary>
public class AIPlanTree {

    public AIPlanTree() {
        Root = new AIPlanNode();
        Root.Kind = AIPlanNodeKind.Group;
        Root.Status = AIPlanNodeStatus.Pending;
        RootVerifying = false;
    }

    /// <summary>隐式根（ParentId 空；Kind=Group 纯聚合）。</summary>
    public AIPlanNode Root { get; }

    /// <summary>根专属 Verifying：全叶 Completed 且无失败、待汇总门 D0–D7（不进通用枚举，仅根可达）。</summary>
    public bool RootVerifying { get; set; }

    /// <summary>自底向上重算聚合态；叶不改写，组/根按子态上确界聚合；根全 Completed → RootVerifying。</summary>
    public void ComputeStatus() {
        this.ComputeNode(Root);
        RootVerifying = this.IsRootVerifiable();
    }

    /// <summary>根完成（DoD 全勾，由 AIPlan.Complete 唯一调用）：清 Verifying 标志、根落 Completed。</summary>
    public void MarkRootVerified() {
        RootVerifying = false;
        Root.Status = AIPlanNodeStatus.Completed;
    }

    /// <summary>
    /// 树结构 Lint（DependsOn 引用 / 非法态 I1–I2、I7–I8）。返回问题列表（空 = 树一致）。
    /// I4（根 Completed ⇔ DoD 全勾）由 AIPlan.Complete 与汇总门受控 API 强制，不在本方法内判。
    /// </summary>
    public List<string> Validate() {
        List<string> issues = new List<string>();
        List<AIPlanNode> all = new List<AIPlanNode>();
        this.CollectAll(Root, all);
        int n = 0;
        while (n < all.Count) {
            AIPlanNode node = all[n];
            List<string> dependsOn = node.DependsOn;
            int k = 0;
            while (k < dependsOn.Count) {
                if (this.FindNode(dependsOn[k]) == null) {
                    issues.Add("I7: node " + node.Id + " DependsOn references unknown id " + dependsOn[k]);
                }
                k = k + 1;
            }
            n = n + 1;
        }
        this.ValidateNode(Root, issues);
        return issues;
    }

    /// <summary>迁移构造：存量扁平 AITaskStep 列表 → 单层树（根 Group → N 个 Leaf）。</summary>
    public static AIPlanTree FromFlat(List<AITaskStep> steps) {
        AIPlanTree tree = new AIPlanTree();
        if (steps != null) {
            int i = 0;
            while (i < steps.Count) {
                AITaskStep s = steps[i];
                AIPlanNode node = new AIPlanNode();
                node.Kind = AIPlanNodeKind.Leaf;
                node.Title = s.Title;
                node.Description = s.Description;
                node.Files = s.Files;
                node.Status = s.Done ? AIPlanNodeStatus.Completed : AIPlanNodeStatus.Pending;
                node.ParentId = tree.Root.Id;
                tree.Root.Children.Add(node);
                i = i + 1;
            }
        }
        tree.ComputeStatus();
        return tree;
    }

    /// <summary>按节点 Id 查找（含根与全体后代）；未找到 → null。</summary>
    public AIPlanNode? FindNode(string id) {
        if (id == null || id == "") {
            return null;
        }
        List<AIPlanNode> all = new List<AIPlanNode>();
        this.CollectAll(Root, all);
        int i = 0;
        while (i < all.Count) {
            AIPlanNode node = all[i];
            if (node.Id == id) {
                return node;
            }
            i = i + 1;
        }
        return null;
    }

    // ── 私有：聚合 / 遍历 / 校验 ──

    private void ComputeNode(AIPlanNode node) {
        int i = 0;
        while (i < node.Children.Count) {
            AIPlanNode child = node.Children[i];
            this.ComputeNode(child);
            i = i + 1;
        }
        if (node.Kind != AIPlanNodeKind.Leaf) {
            node.Status = this.AggregateChildren(node);
        }
    }

    private AIPlanNodeStatus AggregateChildren(AIPlanNode node) {
        // 优先级 1–5 命中即停（数字越小优先级越高；5 = 默认全 Completed）。
        int priority = 5;
        int i = 0;
        while (i < node.Children.Count) {
            AIPlanNode child = node.Children[i];
            if (child.Status == AIPlanNodeStatus.Failed) {
                priority = AIPlanTree.Min(priority, 1);
            } else if (child.Status == AIPlanNodeStatus.Cancelled) {
                priority = AIPlanTree.Min(priority, 2);
            } else if (child.Status == AIPlanNodeStatus.Running) {
                priority = AIPlanTree.Min(priority, 3);
            } else if (child.Status == AIPlanNodeStatus.Ready || child.Status == AIPlanNodeStatus.Pending) {
                priority = AIPlanTree.Min(priority, 4);
            }
            // Completed 不参与吸收（保持 priority 5）。
            i = i + 1;
        }
        if (priority <= 2) {
            return AIPlanNodeStatus.Failed;
        }
        if (priority == 3) {
            return AIPlanNodeStatus.Running;
        }
        if (priority == 4) {
            return AIPlanNodeStatus.Pending;
        }
        return AIPlanNodeStatus.Completed;
    }

    private bool IsRootVerifiable() {
        if (Root.Children.Count == 0) {
            return false;
        }
        int i = 0;
        while (i < Root.Children.Count) {
            if (Root.Children[i].Status != AIPlanNodeStatus.Completed) {
                return false;
            }
            i = i + 1;
        }
        return true;
    }

    private void CollectAll(AIPlanNode node, List<AIPlanNode> result) {
        result.Add(node);
        int i = 0;
        while (i < node.Children.Count) {
            AIPlanNode child = node.Children[i];
            this.CollectAll(child, result);
            i = i + 1;
        }
    }

    private void ValidateNode(AIPlanNode node, List<string> issues) {
        if (node.Kind == AIPlanNodeKind.Leaf) {
            // I8：叶终态必交小结（无小结不得 Completed/Failed/Cancelled）。
            if (node.IsTerminal && (node.Summary == null || node.Summary == "")) {
                issues.Add("I8: leaf " + node.Id + " terminal without summary");
            }
        } else {
            bool hasFailed = false;
            bool hasNonTerminal = false;
            int i = 0;
            while (i < node.Children.Count) {
                AIPlanNode child = node.Children[i];
                if (child.Status == AIPlanNodeStatus.Failed) {
                    hasFailed = true;
                }
                if (child.Status == AIPlanNodeStatus.Pending
                    || child.Status == AIPlanNodeStatus.Ready
                    || child.Status == AIPlanNodeStatus.Running) {
                    hasNonTerminal = true;
                }
                i = i + 1;
            }
            // I1：子 Failed 但父非 Failed（红吸收必须逐层上浮）。
            if (hasFailed && node.Status != AIPlanNodeStatus.Failed) {
                issues.Add("I1: parent " + node.Id + " has Failed child but status " + this.StatusName(node.Status));
            }
            // I2：子 Pending/Ready/Running 但父 Completed（未派发子不得出现在已完成父下）。
            if (hasNonTerminal && node.Status == AIPlanNodeStatus.Completed) {
                issues.Add("I2: parent " + node.Id + " Completed but has non-terminal child");
            }
        }
        int j = 0;
        while (j < node.Children.Count) {
            AIPlanNode child = node.Children[j];
            this.ValidateNode(child, issues);
            j = j + 1;
        }
    }

    private string StatusName(AIPlanNodeStatus status) {
        if (status == AIPlanNodeStatus.Pending) { return "Pending"; }
        if (status == AIPlanNodeStatus.Ready) { return "Ready"; }
        if (status == AIPlanNodeStatus.Running) { return "Running"; }
        if (status == AIPlanNodeStatus.Completed) { return "Completed"; }
        if (status == AIPlanNodeStatus.Failed) { return "Failed"; }
        return "Cancelled";
    }

    private static int Min(int a, int b) {
        return a < b ? a : b;
    }
}
