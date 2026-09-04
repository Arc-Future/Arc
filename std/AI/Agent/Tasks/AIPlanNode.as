// RFC 043 references/plan-tree（P1 收敛）：AIPlanNode — 树形状态树节点（计划单元 + 状态单元）。
//
// AIPlan 从「扁平 List<AITaskStep>」升级为「树形状态树」：一个节点同时承载计划结构与
// 状态聚合。字段分三组（结构 / 状态 / 绑定）。
//
// 归属 Arc.Agent；只存 RunId 字符串引用，不 import Arc.Agent.Harness 类型（依赖单向：
// Harness → Agent，043 §10）。Summary 为 string（空 = 无小结）。
//
// 去过度设计收敛（2026-08-16）：砍 Checkpoint（根 = Root/ParentId 空识别）、Blocked（Ready
// 瞬态派生）、Skipped（用 Completed+小结承载）、OrderAfter（沿用 DependsOn 现名）、
// OwnerSessionId/DelegatedSessionId（无多级委托路径，owner 恒=主代理）、WorkItemId（=Id 迁移别名）。
namespace Arc.Agent;
using Arc;

/// <summary>节点种类：Leaf 可执行叶 / Group 纯聚合组。根不设专属枚举值（由 AIPlanTree.Root 识别）。</summary>
public enum AIPlanNodeKind {
    /// <summary>可执行叶（可委托子代理）。</summary>
    Leaf,
    /// <summary>纯聚合组（无自身执行）。</summary>
    Group
}

/// <summary>节点状态六态：Pending / Ready / Running / Completed / Failed / Cancelled。
/// Verifying 为根专属（由根身份触发，不进通用枚举，见 AIPlanTree.RootVerifying）。</summary>
public enum AIPlanNodeStatus {
    /// <summary>未轮到（DependsOn 未满或未被扇出窗口选中）。</summary>
    Pending,
    /// <summary>依赖已满、可派发（等并行度节流）。</summary>
    Ready,
    /// <summary>叶：子代理在飞；组：≥1 子在飞。</summary>
    Running,
    /// <summary>叶：执行层终结；组：全子 Completed（纯聚合）。</summary>
    Completed,
    /// <summary>叶执行失败 / 组聚合含失败（红吸收向上）。</summary>
    Failed,
    /// <summary>撤单（叶或整枝）。</summary>
    Cancelled
}

/// <summary>树形状态树节点（结构 / 状态 / 绑定三组字段）。</summary>
public class AIPlanNode {
    // ── 结构 ──
    public string Id;
    public string ParentId;
    public List<AIPlanNode> Children;
    public AIPlanNodeKind Kind;
    public string Title;
    public string Description;
    public string Files;
    public List<string> DependsOn;
    public List<string> Scope;
    // ── 状态 + 绑定 ──
    public AIPlanNodeStatus Status;
    public string Summary;
    public string RunId;

    public AIPlanNode() {
        this.Id = "n" + Guid.NewGuid().ToString();
        this.ParentId = "";
        this.Children = new List<AIPlanNode>();
        this.Kind = AIPlanNodeKind.Leaf;
        this.Title = "";
        this.Description = "";
        this.Files = "";
        this.DependsOn = new List<string>();
        this.Scope = new List<string>();
        this.Status = AIPlanNodeStatus.Pending;
        this.Summary = "";
        this.RunId = "";
    }

    /// <summary>是否叶节点（Kind == Leaf）。</summary>
    public bool IsLeaf {
        get { return this.Kind == AIPlanNodeKind.Leaf; }
    }

    /// <summary>是否终态（Completed / Failed / Cancelled，不再执行）。</summary>
    public bool IsTerminal {
        get {
            return this.Status == AIPlanNodeStatus.Completed
                || this.Status == AIPlanNodeStatus.Failed
                || this.Status == AIPlanNodeStatus.Cancelled;
        }
    }
}
