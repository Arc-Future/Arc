// RFC 043 场景 2.4：AIPlan 持久化载体 — 计划树 JSON 外壳（跨会话 resume 重建）。
//
// AIPlan / AIPlanTree / AIPlanNode 属 Arc.Agent（另一包，且可能被并行任务触碰），
// 本载体不修改其类型，只读其公开字段手动序列化（对齐 AIRfcState 的「载体外壳」模式）。
// 门状态不持久化——下次 /dod 由 RunAutoGatesAsync 实时重算（诚实标注「门状态重跑对齐」）。
// 时间戳（CreatedAt/UpdatedAt）不落盘，恢复时由 AIPlan 构造器重算为当前时刻。
namespace Arc.Agent.Harness;
using Arc.Agent;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// AIPlan 持久化载体：计划树快照的 JSON 外壳（<c>{"id","goal",...,"nodes":[...]}</c>）。
/// 与 <see cref="AIHarnessSession.SavePlanAsync"/> / <see cref="AIHarnessSession.RestorePlanAsync"/>
/// 配套；不参与业务 API 面。节点树按结构嵌套重建（ParentId 由结构推导，不落盘）。
/// </summary>
public class AIPlanState : IJsonSerializable, IJsonDeserializable {
    /// <summary>被序列化 / 反序列化出的计划（null = 无计划）。</summary>
    public AIPlan Plan;

    public AIPlanState() {
        this.Plan = null;
    }

    public void WriteJson(JsonWriter writer) {
        AIPlan p = this.Plan;
        if (p == null) {
            writer.WriteNull();
            return;
        }
        writer.WriteStartObject();
        writer.WriteString("id", p.Id);
        writer.WriteString("goal", p.Goal);
        writer.WriteString("analysis", p.Analysis);
        writer.WriteString("verification", p.Verification);
        writer.WriteString("status", AIPlanState.PlanStatusName(p.Status));
        writer.WriteNumber("currentStepIndex", p.CurrentStepIndex);
        writer.WriteNumber("revision", p.Revision);
        writer.WritePropertyName("nodes");
        writer.WriteStartArray();
        List<AIPlanNode> children = p.Tree != null && p.Tree.Root != null
            ? p.Tree.Root.Children : new List<AIPlanNode>();
        int i = 0;
        while (i < children.Count) {
            this.WriteNode(writer, children[i]);
            i = i + 1;
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    public void ReadJson(JsonReader reader) {
        AIPlan plan = new AIPlan();
        this.Plan = plan;
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                break;
            }
            if (reader.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string prop = reader.GetString();
            if (prop == "nodes") {
                this.ReadNodes(reader, plan);
            } else {
                if (!reader.Read()) {
                    return;
                }
                if (prop == "id") {
                    plan.Id = reader.GetString();
                } else if (prop == "goal") {
                    plan.Goal = reader.GetString();
                } else if (prop == "analysis") {
                    plan.Analysis = reader.GetString();
                } else if (prop == "verification") {
                    plan.Verification = reader.GetString();
                } else if (prop == "status") {
                    AIPlanStatus status = AIPlanStatus.Pending;
                    if (AIPlanState.TryParsePlanStatus(reader.GetString(), out status)) {
                        plan.Status = status;
                    }
                } else if (prop == "currentStepIndex") {
                    plan.CurrentStepIndex = reader.GetInt32();
                } else if (prop == "revision") {
                    plan.Revision = reader.GetInt32();
                } else {
                    reader.Skip();
                }
            }
        }
        // 结构重建后重算聚合态（根/组状态 + RootVerifying）。
        plan.Tree.ComputeStatus();
    }

    // ── 节点树 ──

    private void WriteNode(JsonWriter writer, AIPlanNode node) {
        writer.WriteStartObject();
        writer.WriteString("id", node.Id);
        writer.WriteString("kind", AIPlanState.KindName(node.Kind));
        writer.WriteString("title", node.Title);
        writer.WriteString("description", node.Description);
        writer.WriteString("files", node.Files);
        writer.WritePropertyName("dependsOn");
        writer.WriteStartArray();
        int d = 0;
        while (d < node.DependsOn.Count) {
            writer.WriteString(node.DependsOn[d]);
            d = d + 1;
        }
        writer.WriteEndArray();
        writer.WritePropertyName("scope");
        writer.WriteStartArray();
        int s = 0;
        while (s < node.Scope.Count) {
            writer.WriteString(node.Scope[s]);
            s = s + 1;
        }
        writer.WriteEndArray();
        writer.WriteString("status", AIPlanState.NodeStatusName(node.Status));
        writer.WriteString("summary", node.Summary);
        writer.WriteString("runId", node.RunId);
        writer.WritePropertyName("children");
        writer.WriteStartArray();
        int c = 0;
        while (c < node.Children.Count) {
            this.WriteNode(writer, node.Children[c]);
            c = c + 1;
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    /// <summary>读取根子节点数组并挂到计划树的隐式根下。</summary>
    private void ReadNodes(JsonReader reader, AIPlan plan) {
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
            return;
        }
        bool cont = true;
        while (cont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                cont = false;
                break;
            }
            if (reader.TokenType != JsonTokenType.StartObject) {
                continue;
            }
            AIPlanNode node = this.ReadNode(reader, plan.Tree.Root.Id);
            plan.Tree.Root.Children.Add(node);
        }
    }

    /// <summary>递归读取节点对象（reader 定位在 StartObject）。</summary>
    private AIPlanNode ReadNode(JsonReader reader, string parentId) {
        AIPlanNode node = new AIPlanNode();
        node.ParentId = parentId;
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                break;
            }
            if (reader.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string prop = reader.GetString();
            if (prop == "children") {
                if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
                    continue;
                }
                bool cont = true;
                while (cont && reader.Read()) {
                    if (reader.TokenType == JsonTokenType.EndArray) {
                        cont = false;
                        break;
                    }
                    if (reader.TokenType != JsonTokenType.StartObject) {
                        continue;
                    }
                    AIPlanNode child = this.ReadNode(reader, node.Id);
                    node.Children.Add(child);
                }
            } else if (prop == "dependsOn" || prop == "scope") {
                List<string> target = prop == "dependsOn" ? node.DependsOn : node.Scope;
                this.ReadStringArray(reader, target);
            } else {
                if (!reader.Read()) {
                    return node;
                }
                if (prop == "id") {
                    node.Id = reader.GetString();
                } else if (prop == "kind") {
                    node.Kind = AIPlanState.ParseKind(reader.GetString());
                } else if (prop == "title") {
                    node.Title = reader.GetString();
                } else if (prop == "description") {
                    node.Description = reader.GetString();
                } else if (prop == "files") {
                    node.Files = reader.GetString();
                } else if (prop == "status") {
                    node.Status = AIPlanState.ParseNodeStatus(reader.GetString());
                } else if (prop == "summary") {
                    node.Summary = reader.GetString();
                } else if (prop == "runId") {
                    node.RunId = reader.GetString();
                } else {
                    reader.Skip();
                }
            }
        }
        return node;
    }

    private void ReadStringArray(JsonReader reader, List<string> target) {
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
            return;
        }
        bool cont = true;
        while (cont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                cont = false;
                break;
            }
            if (reader.TokenType != JsonTokenType.String) {
                continue;
            }
            target.Add(reader.GetString());
        }
    }

    // ── wire 编解码（AIPlanStatus / AIPlanNodeStatus / AIPlanNodeKind）──

    private static string PlanStatusName(AIPlanStatus status) {
        if (status == AIPlanStatus.Pending) { return "Pending"; }
        if (status == AIPlanStatus.Approved) { return "Approved"; }
        if (status == AIPlanStatus.Executing) { return "Executing"; }
        if (status == AIPlanStatus.Verifying) { return "Verifying"; }
        if (status == AIPlanStatus.Completed) { return "Completed"; }
        return "Rejected";
    }

    private static bool TryParsePlanStatus(string text, out AIPlanStatus status) {
        if (text == "Pending") { status = AIPlanStatus.Pending; return true; }
        if (text == "Approved") { status = AIPlanStatus.Approved; return true; }
        if (text == "Executing") { status = AIPlanStatus.Executing; return true; }
        if (text == "Verifying") { status = AIPlanStatus.Verifying; return true; }
        if (text == "Completed") { status = AIPlanStatus.Completed; return true; }
        if (text == "Rejected") { status = AIPlanStatus.Rejected; return true; }
        status = AIPlanStatus.Pending;
        return false;
    }

    private static string NodeStatusName(AIPlanNodeStatus status) {
        if (status == AIPlanNodeStatus.Pending) { return "Pending"; }
        if (status == AIPlanNodeStatus.Ready) { return "Ready"; }
        if (status == AIPlanNodeStatus.Running) { return "Running"; }
        if (status == AIPlanNodeStatus.Completed) { return "Completed"; }
        if (status == AIPlanNodeStatus.Failed) { return "Failed"; }
        return "Cancelled";
    }

    private static AIPlanNodeStatus ParseNodeStatus(string text) {
        if (text == "Ready") { return AIPlanNodeStatus.Ready; }
        if (text == "Running") { return AIPlanNodeStatus.Running; }
        if (text == "Completed") { return AIPlanNodeStatus.Completed; }
        if (text == "Failed") { return AIPlanNodeStatus.Failed; }
        if (text == "Cancelled") { return AIPlanNodeStatus.Cancelled; }
        return AIPlanNodeStatus.Pending;
    }

    private static string KindName(AIPlanNodeKind kind) {
        return kind == AIPlanNodeKind.Group ? "Group" : "Leaf";
    }

    private static AIPlanNodeKind ParseKind(string text) {
        return text == "Group" ? AIPlanNodeKind.Group : AIPlanNodeKind.Leaf;
    }
}
